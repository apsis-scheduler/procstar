"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
from   dataclasses import dataclass
import datetime
import ipaddress
import logging
import random
import websockets.server
from   websockets.exceptions import ConnectionClosedError

from   . import proto
from   .lib.time import now
from   .proto import ProtocolError, serialize_message, deserialize_message

# Timeout to receive an initial login message.
TIMEOUT_LOGIN = 60

# FIXME: What is the temporal scope of a connection?

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


@dataclass
class ConnectionInfo:
    address: ipaddress._BaseAddress
    port: int

    def __str__(self):
        return f"{self.address}:{self.port}"



@dataclass
class Connection:
    info: ConnectionInfo
    ws: asyncio.protocols.Protocol
    group: str
    connect_time: datetime.datetime

    @property
    def open(self):
        return self.ws.open


    async def send(self, msg):
        data = serialize_message(msg)

        try:
            await self.ws.send(data)
        except ConnectionClosedError:
            # Connection closed; drop it.
            # FIXME: Don't forget the connection.
            self.logger.warning(f"{self.info}: connection closed")
            # FIXME: Mark it as closed?  Or is its internal closed flag enough?
            assert self.ws.closed
            # removed = self.connections.pop(conn_id)
            # assert removed is self




class Connections:

    def __init__(self):
        self.__connections = {}
        self.__groups = {}


    def keys(self):
        return self.__connections.keys()


    def values(self):
        return self.__connections.values()


    def __len__(self):
        return len(self.__connections)


    def __iter__(self):
        return iter(self.__connections)


    def __setitem__(self, conn_id, connection: Connection):
        """
        Adds a new connection.

        :param conn_id:
          The new connection ID, which must not already be in use.
        """
        try:
            self.__delitem__(conn_id)
        except KeyError:
            pass

        assert conn_id not in self.__connections
        self.__connections[conn_id] = connection

        # Add it to the group.
        group = self.__groups.setdefault(connection.group, set())
        group.add(conn_id)


    def __getitem__(self, conn_id):
        return self.__connections[conn_id]


    def __delitem__(self, conn_id):
        self.pop(conn_id)


    def pop(self, conn_id) -> Connection:
        connection = self.__connections.pop(conn_id)

        # Remove it from its group.
        group = self.__groups[connection.group]
        group.remove(conn_id)
        # If the group is now empty, clean it up.
        if len(group) == 0:
            del self.__groups[connection.group]

        return connection


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
            if (c := self.__connections[i]).open
        ]
        if len(connections) == 0:
            raise NoOpenConnectionInGroup(group)

        # FIXME: Better choice mechanism.
        return random.choice(connections)



class Process:

    def __init__(self, connection, proc_id):
        self.connection = connection
        self.proc_id = proc_id
        self.__messages = asyncio.Queue()


    def __aiter__(self):
        return self


    async def __anext__(self):
        # FIXME: Queue can be closed.
        return await self.__messages.get()


    async def request_result(self):
        """
        Sends a request for the current process result.
        """
        await self.connection.send(proto.ProcResultRequest(self.proc_id))




class Server:

    def __init__(self):
        # Mapping from connection name to connection.
        self.connections = Connections()
        # Use the module logger by default.
        self.logger = logging.getLogger(__name__)


    async def serve_connection(self, ws):
        """
        Serves an incoming connection.

        Use this bound method with `websockets.server.serve()`.
        """
        log = self.logger
        connect_time = now()

        # Collect remote loc.
        address, port, *_ = ws.remote_address
        address = ipaddress.ip_address(address)
        info = ConnectionInfo(address, port)

        try:
            # Wait for a Connect message.
            try:
                async with asyncio.timeout(TIMEOUT_LOGIN):
                    msg = await ws.recv()
            except TimeoutError:
                raise ProtocolError(f"no register in {TIMEOUT_LOGIN} s")
            except ConnectionClosedError:
                raise ProtocolError("closed before register")
            log.debug(f"msg: {msg}")

            # Only Connect is acceptable.
            msg_type, msg = deserialize_message(msg)
            if msg_type != "Register":
                raise ProtocolError(f"expected register; got {msg_type}")

        except Exception as exc:
            log.warning(f"{info}: {exc}")
            ws.close()
            return

        conn_id = msg.conn_id

        try:
            old = self.connections.pop(conn_id)
        except KeyError:
            pass
        else:
            log.info(f"reconnected: {conn_id} was @{old.info}")
            old.ws.close()
            old = None

        connection = self.connections[conn_id] = Connection(info, ws, msg.group, connect_time)
        log.info(f"{info}: connected: {conn_id} group {msg.group}")

        # Receive messages.
        while True:
            try:
                msg = await ws.recv()
            except ConnectionClosedError:
                log.info(f"{info}: connection closed")
                break
            type, msg = deserialize_message(msg)
            log.info(f"received: {msg}")

            # FIXME: For testing.
            if type == "ProcResult" and msg.res["status"] is not None:
                await connection.send(proto.ProcDeleteRequest(msg.proc_id))

        ws.close()


    async def start(self, group, proc_id, spec) -> Process:
        """
        Starts a new process on a connection in `group`.

        :return:
          The connection on which the process starts.
        """
        connection = self.connections.choose_connection(group)
        await connection.send(proto.ProcStart(procs={proc_id: spec}))
        return Process(connection, proc_id)



class Connection_:

    async def request_result(self, proc_id):
        """
        Requests current results for a process.
        """


    async def request_delete(self, proc_id):
        """
        Requests deletion of a completed process.
        """





async def run(server, loc=(None, proto.DEFAULT_PORT)):
    """
    Creates a WebSockets server at `loc` and accepts connections to it with
    `server`.

    :param loc:
      The (host, port) on which to run.
    """
    host, port = loc

    proc_id = "testproc0"
    spec = {"argv": ["/usr/bin/sleep", "5"]}
    process = None

    async with websockets.server.serve(server.serve_connection, host, port):
        # FIXME: For testing.
        while True:
            await asyncio.sleep(1)
            print(f"connections: {', '.join(server.connections)}")
            for connection in server.connections.values():
                await connection.send(proto.ProcidListRequest())

            if process is None and len(server.connections) > 0:
                await asyncio.sleep(1)
                process = await server.start(
                    group   ="default",
                    proc_id =proc_id,
                    spec    =spec,
                )

            if process is not None:
                await process.request_result()

        # await asyncio.Future()



if __name__ == "__main__":
    logging.basicConfig(
        level=logging.DEBUG,
        format="%(asctime)s [%(levelname)-7s] %(message)s",
    )
    logging.getLogger("websockets.server").setLevel(logging.INFO)
    try:
        asyncio.run(run(Server()))
    except KeyboardInterrupt:
        pass

