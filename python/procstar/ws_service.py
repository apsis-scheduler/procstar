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


@dataclass
class ConnectionInfo:
    address: ipaddress._BaseAddress
    port: int

    def __str__(self):
        return f"{self.address}:{self.port}"



@dataclass
class Connection:
    """
    A connection to a single procstar instance.

    The connection object survives disconnection and reconnection from the
    procstar instance.  Thus, the websocket may be closed, and the remote
    procstar instance may no longer exist.  If the same procstar instance later
    reconnects, it uses the same `Connection` instance.
    """

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
            logger.warning(f"{self.info}: connection closed")
            # FIXME: Mark it as closed?  Or is its internal closed flag enough?
            assert self.ws.closed
            # removed = self.connections.pop(conn_id)
            # assert removed is self




class Connections(dict):

    def __init__(self):
        super().__init__()
        self.__groups = {}


    def __setitem__(self, conn_id, connection: Connection):
        """
        Adds a new connection.

        :param conn_id:
          The new connection ID, which must not already be in use.
        """
        assert conn_id not in self
        super().__setitem__(conn_id, connection)

        # Add it to the group.
        group = self.__groups.setdefault(connection.group, set())
        group.add(conn_id)


    def __delitem__(self, conn_id):
        self.pop(conn_id)


    def pop(self, conn_id) -> Connection:
        connection = super().pop(conn_id)

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
            if (c := self[i]).open
        ]
        if len(connections) == 0:
            raise NoOpenConnectionInGroup(group)

        # FIXME: Better choice mechanism.
        return random.choice(connections)



class Process:

    # FIXME: What happens when the connection is closed?

    def __init__(self, connection, proc_id):
        self.connection = connection
        self.proc_id = proc_id


    def __aiter__(self):
        return self


    def request_result(self):
        """
        Sends a request for the current process result.

        :return:
          An awaitable.
        """
        return self.connection.send(proto.ProcResultRequest(self.proc_id))


    def request_delete(self):
        """
        Sends a request to delete the process.

        :return:
          An awaitable.
        """
        return self.connection.send(proto.ProcResultDelete(self.proc_id))



class Server:

    def __init__(self):
        # Mapping from connection name to connection.
        self.connections = Connections()
        # Queue for incoming messages and other notifications.
        self.__messages = asyncio.Queue()


    def run(self, loc=(None, None)):
        """
        Returns an async context manager that runs the websocket server.

        :param loc:
          `host, port` pair.  If `host` is none, runs on all interfaces.
          If `port` is none, chooses an unused port on each interface.
        """
        host, port = loc
        return websockets.server.serve(self._serve_connection, host, port)


    def __enqueue(self, conn_id, msg):
        self.__messages.put_nowait((conn_id, msg))


    def __aiter__(self):
        """
        Returns an async iterator of (conn_id, message) containing messages
        from all connections to this server.
        """
        # FIXME: At the moment, there is no shutdown procedure.  (And this is
        # why asyncio.Queue doesn't implement __aiter__ directly.)
        async def messages(queue):
            while True:
                yield await queue.get()

        return messages(self.__messages)


    async def _serve_connection(self, ws):
        """
        Serves an incoming connection.

        Use this bound method with `websockets.server.serve()`.
        """
        assert ws.open

        connect_time = now()

        # Collect remote loc.
        address, port, *_ = ws.remote_address
        address = ipaddress.ip_address(address)
        info = ConnectionInfo(address, port)

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
            logger.warning(f"{info}: {exc}")
            await ws.close()
            return

        conn_id = msg.conn_id

        # Do we recognize this connection?
        try:
            connection = self.connections[conn_id]

        except KeyError:
            # A new connection ID.
            logger.info(f"[{conn_id}] connecting from {info} group {msg.group}")
            connection = self.connections[conn_id] = Connection(
                info        =info,
                ws          =ws,
                group       =msg.group,
                connect_time=connect_time,
            )

        else:
            logger.info(f"[{conn_id}] reconnecting")

            # Previous connection with the same ID.  First, some sanity checks.
            if info.address != connection.info.address:
                # Allow the address to change, in case the remote reconnects
                # through a different interface.  The port may always be
                # different, of course.
                logger.warning(
                    f"[{conn_id}] new address: {connection.info.address}")
            if msg.group != connection.group:
                logger.error(f"[{conn_id}] new group: {msg.group}")
                ws.close()
                return

            # Is the old connection socket still (purportedly) open?
            if not connection.ws.closed:
                logger.warning(f"[{conn_id}] closing old connection")
                connection.ws.close()
                assert not connection.ws.open

            # Use the new socket with the old connection.
            connection.info = info
            connection.ws = ws
            connection.group = msg.group
            connection.connect_time = connect_time

        # Enqueue the register message we already received.
        self.__enqueue(conn_id, msg)

        # Receive messages.
        while True:
            try:
                msg = await ws.recv()
            except ConnectionClosedError:
                logger.info(f"[{conn_id}] connection closed")
                break
            type, msg = deserialize_message(msg)
            self.__enqueue(conn_id, msg)

        await ws.close()
        assert ws.closed
        # Don't forget the connection; the other end may reconnect.


    async def start(self, group, proc_id, spec) -> Process:
        """
        Starts a new process on a connection in `group`.

        :return:
          The connection on which the process starts.
        """
        connection = self.connections.choose_connection(group)
        await connection.send(proto.ProcStart(specs={proc_id: spec}))
        return Process(connection, proc_id)



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
    main()

