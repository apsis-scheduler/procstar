"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
from   collections.abc import Mapping
from   dataclasses import dataclass
import ipaddress
import logging
import random
from   typing import Optional
import websockets.server
from   websockets.exceptions import ConnectionClosedError

from   . import proto
from   .lib.asyn import Subscribeable
from   .lib.json import Jso
from   .proto import ProtocolError, serialize_message, deserialize_message

# Timeout to receive an initial login message.
TIMEOUT_LOGIN = 60

# FIXME: What is the temporal scope of a connection?

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

class NoGroupError(LookupError):
    """
    No group with the given group name.
    """



class NoOpenConnectionInGroup(RuntimeError):
    """
    The group contains no open connections.
    """



class NoConnectionError(LookupError):
    """
    No connection with the given name.
    """


#-------------------------------------------------------------------------------

@dataclass
class ConnectionInfo:
    address: ipaddress._BaseAddress
    port: int

    def __str__(self):
        return f"{self.address}:{self.port}"


    @classmethod
    def from_ws(cls, ws):
        address, port, *_ = ws.remote_address
        address = ipaddress.ip_address(address)
        return cls(address, port)



@dataclass
class Connection:
    """
    A connection to a single procstar instance.

    The connection object survives disconnection and reconnection from the
    procstar instance.  Thus, the websocket may be closed, and the remote
    procstar instance may no longer exist.  If the same procstar instance later
    reconnects, it uses the same `Connection` instance.
    """

    conn_id: str
    group: str = None
    ws: asyncio.protocols.Protocol = None

    def __hash__(self):
        return hash(self.conn_id)


    @property
    def info(self):
        return None if self.ws is None else ConnectionInfo.from_ws(self.ws)


    @property
    def open(self):
        return self.ws.open


    async def send(self, msg):
        data = serialize_message(msg)

        try:
            await self.ws.send(data)
        except ConnectionClosedError:
            assert self.ws.closed
            # Connection closed.  Don't forget about it; it may reconnect.
            logger.warning(f"{self.info}: connection closed")
            # FIXME: Mark it as closed?  Or is its internal closed flag enough?
            # FIXME: Think carefully the temporarily dropped connection logic.



class Connections(Mapping, Subscribeable):

    def __init__(self):
        super().__init__()
        self.__conns = {}
        self.__groups = {}


    def _add(self, *, conn_id, group, ws) -> Connection:
        """
        Adds a new connection or readds an existing one.

        :return:
          The connection object.
        :raise RuntimeError:
          The connection could not be added.
        """
        conn_id = conn_id

        try:
            # Look for an existing connection with this ID.
            old_conn = self.__conns[conn_id]

        except KeyError:
            # New connection.
            conn = self.__conns[conn_id] = Connection(
                conn_id =conn_id,
                group   =group,
                ws      =ws,
            )
            # Add it to the group.
            group = self.__groups.setdefault(group, set())
            group.add(conn_id)

        else:
            # Previous connection with the same ID.  First, some sanity checks.
            info = ConnectionInfo.from_ws(ws)
            if info.address != old_conn.info.address:
                # Allow the address to change, in case the remote reconnects
                # through a different interface.  The port may always be
                # different, of course.
                logger.warning(f"[{conn_id}] new address: {info.address}")
            if group != old_conn.group:
                # The same instance shouldn't connect under a different group.
                raise RuntimeError(f"[{conn_id}] new group: {group}")

            # If the old connection websocket still open, close it.
            if not old_conn.ws.closed:
                logger.warning(f"[{conn_id}] closing old connection")
                _ = asyncio.create_task(old_conn.ws.close())

            # Use the new websocket with the old connection object.
            old_conn.ws = ws
            conn = old_conn

        self._publish((conn_id, conn))
        return conn


    def _pop(self, conn_id) -> Connection:
        """
        Deletes and returns a connection.
        """
        conn = self.__conns.pop(conn_id)
        # Remove it from its group.
        group = self.__groups[conn.group]
        group.remove(conn_id)
        # If the group is now empty, clean it up.
        if len(group) == 0:
            del self.__groups[conn.group]
        self._publish((conn_id, None))
        return conn


    # Mapping methods.

    def __contains__(self, conn_id):
        return self.__conns.__contains__(conn_id)


    def __getitem__(self, conn_id):
        return self.__conns.__getitem__(conn_id)


    def __len__(self):
        return self.__conns.__len__()


    def __iter__(self):
        return self.__conns.__iter__()


    def values(self):
        return self.__conns.values()


    def items(self):
        return self.__conns.items()


    # Group methods

    def choose_connection(self, group) -> Connection:
        """
        Chooses an open connection in 'group'.
        """
        try:
            conn_ids = self.__groups[group]
        except KeyError:
            raise NoGroupError(group) from None

        connections = [
            c
            for i in conn_ids
            if (c := self[i]).open
        ]
        if len(connections) == 0:
            raise NoOpenConnectionInGroup(group)

        # FIXME: Better choice mechanism.
        return random.choice(connections)



#-------------------------------------------------------------------------------

class Process:
    """
    A process running under a connected procstar instance.
    """

    class Results:
        """
        Process result updates.

        Acts as a one-shot async iterable and iterator of result updates.  The
        `latest` property always returns the most recent, which may be none.
        """

        def __init__(self):
            self.__latest = None
            self.__updates = asyncio.Queue()


        @property
        def latest(self) -> Optional[Jso]:
            """
            Most recent received process result.
            """
            return self.__latest


        def __aiter__(self):
            return self


        def __anext__(self):
            return self.__updates.get()


        def _update(self, result):
            result = None if result is None else Jso(result)
            self.__latest = result
            self.__updates.put_nowait(result)


    proc_id: str
    conn_id: str

    results: Results

    # FIXME
    errors: list[str]

    # FIXME: What happens when the connection is closed?

    def __init__(self, conn_id, proc_id):
        self.proc_id = proc_id
        self.conn_id = conn_id
        self.results = self.Results()
        # FIXME: Receive proc-specific errors.
        self.errors = []


    async def wait_for_completion(self) -> Jso:
        """
        Awaits and returns a completed result.
        """
        # Is it already completed?
        res = self.results.latest
        if res is not None and res.status is not None:
            return res

        while True:
            res = await anext(self.results)
            if res.status is not None:
                return res



class Processes(Mapping):
    """
    Tracks processes running under connected procstar instances.
    """

    def __init__(self):
        self.__procs = {}


    def create(self, conn_id, proc_id) -> Process:
        """
        Creates and returns a new process on `connection` with `proc_id`.

        `proc_id` must be unknown.
        """
        assert proc_id not in self.__procs
        self.__procs[proc_id] = proc = Process(conn_id, proc_id)
        return proc


    def on_message(self, conn_id, msg):
        """
        Processes `msg` received from `conn_id` to the corresponding
        process.
        """
        def get_proc(proc_id):
            try:
                return self.__procs[proc_id]
            except KeyError:
                logger.info(f"new proc on {conn_id}: {proc_id}")
                return self.create(conn_id, proc_id)

        match msg:
            case proto.ProcidList(proc_ids):
                logger.debug(f"msg proc_id list: {proc_ids}")
                # Make sure we track a proc for each proc ID the instance knows.
                for proc_id in proc_ids:
                    _ = get_proc(proc_id)

            case proto.ProcResult(proc_id, res):
                proc = get_proc(proc_id)
                logger.debug(f"msg proc result: {proc_id}")
                proc.result = res
                proc.results._update(res)

            case proto.ProcDelete(proc_id):
                proc = get_proc(proc_id)
                logger.debug(f"msg proc delete: {proc_id}")
                del self.__procs[proc_id]
                proc.results._update(None)

            case proto.Register:
                # We should receive this only immediately after connection.
                logger.error(f"msg unexpected: {msg}")

            case proto.IncomingMessageError():
                # FIXME: Implement.
                # FIXME: Proc-specific errors.
                raise NotImplementedError()


    # Mapping methods

    def __contains__(self, proc_id):
        return self.__procs.__contains__(proc_id)


    def __getitem__(self, proc_id):
        return self.__procs.__getitem__(proc_id)


    def __len__(self):
        return self.__procs.__len__()


    def __iter__(self):
        return self.__procs.__iter__()


    def values(self):
        return self.__procs.values()


    def items(self):
        return self.__procs.items()



#-------------------------------------------------------------------------------

class Server:

    def __init__(self):
        self.connections = Connections()
        self.processes = Processes()


    def run(self, loc=(None, None)):
        """
        Returns an async context manager that runs the websocket server.

        :param loc:
          `host, port` pair.  If `host` is none, runs on all interfaces.
          If `port` is none, chooses an unused port on each interface.
        """
        host, port = loc
        return websockets.server.serve(self._serve_connection, host, port)


    async def _update_connection(self, conn):
        """
        Refreshes process state from a connection.
        """
        if conn is not None:
            # Ask the procstar instance to tell us all proc IDs it knows
            # about.
            await conn.send(proto.ProcidListRequest())
            # Ask for updates on all processes we think are at this
            # instance.
            await asyncio.gather(*(
                conn.send(proto.ProcResultRequest(p.proc_id))
                for p in self.processes.values()
                if p.conn_id == conn.conn_id
            ))


    async def _serve_connection(self, ws):
        """
        Serves an incoming connection.

        Use this bound method with `websockets.server.serve()`.
        """
        assert ws.open

        try:
            # Wait for a Register message.
            try:
                msg = await asyncio.wait_for(ws.recv(), TIMEOUT_LOGIN)
            except TimeoutError:
                raise ProtocolError(f"no register in {TIMEOUT_LOGIN} s")
            except ConnectionClosedError:
                raise ProtocolError("closed before register")

            # Only Register is acceptable.
            type, msg = deserialize_message(msg)
            if type != "Register":
                raise ProtocolError(f"expected register; got {type}")

        except Exception as exc:
            logger.warning(f"{ws}: {exc}")
            await ws.close()
            return

        # Add or re-add the connection.
        try:
            conn = self.connections._add(
                conn_id =msg.conn_id,
                group   =msg.group,
                ws      =ws,
            )
        except RuntimeError as exc:
            logging.error(str(exc))
            return
        await self._update_connection(conn)

        # Receive messages.
        while True:
            try:
                msg = await ws.recv()
            except ConnectionClosedError:
                logger.info(f"[{conn.conn_id}] connection closed")
                break
            type, msg = deserialize_message(msg)
            # Process the message.
            logging.info(f"RECV: {msg}")
            self.processes.on_message(conn.conn_id, msg)

        await ws.close()
        assert ws.closed
        # Don't forget the connection; the other end may reconnect.


    async def start(
            self,
            proc_id,
            spec,
            *,
            group=proto.DEFAULT_GROUP,
    ) -> Process:
        """
        Starts a new process on a connection in `group`.

        :return:
          The connection on which the process starts.
        """
        try:
            spec = spec.to_jso()
        except AttributeError:
            pass

        conn = self.connections.choose_connection(group)
        # FIXME: If the connection is closed, choose another.
        await conn.send(proto.ProcStart(specs={proc_id: spec}))
        return self.processes.create(conn.conn_id, proc_id)


    async def reconnect(self, conn_id, proc_id) -> Process:
        # FIXME
        raise NotImplementedError()


    async def delete(self, proc_id):
        """
        Deletes a process.
        """
        # FIXME: No proc?
        proc = self.processes[proc_id]
        # FIXME: No connection?
        conn = self.connections[proc.conn_id]
        await conn.send(proto.ProcDeleteRequest(proc_id))



#-------------------------------------------------------------------------------

def main():
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--host", metavar="ADDR", default=None,
        help="serve from interface bound to ADDR [def: all]")
    parser.add_argument(
        "--port", metavar="PORT", type=int, default=proto.DEFAULT_PORT,
        help=f"serve from PORT [def: {proto.DEFAULT_PORT}]")
    args = parser.parse_args()

    async def run(server, loc):
        async with server.run(loc):
            while True:
                async for conn_id, msg in server:
                    logger.info(f"[{conn_id}] received {msg}")


    logging.basicConfig(
        level=logging.DEBUG,
        format="%(asctime)s [%(levelname)-7s] %(message)s",
    )
    logging.getLogger("websockets.server").setLevel(logging.INFO)
    try:
        asyncio.run(run(Server(), loc=(args.host, args.port)))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    logging.getLogger().setLevel(logging.DEBUG)
    main()

