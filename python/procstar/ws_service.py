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

class Error(Exception):

    pass



class NoGroupError(Exception):
    """
    No group with the given group name.
    """



class NoOpenConnectionInGroup(Exception):
    """
    The group contains no open connections.
    """



class NoConnectionError(Exception):
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


    def serve(self, loc=(None, proto.DEFAULT_PORT)):
        """
        Returns an async context manager that runs the service.
        """
        host, port = loc
        return websockets.server.serve(self._serve_connection, host, port)


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
            # Wait for a Connect message.
            try:
                async with asyncio.timeout(TIMEOUT_LOGIN):
                    msg = await ws.recv()
            except TimeoutError:
                raise ProtocolError(f"no register in {TIMEOUT_LOGIN} s")
            except ConnectionClosedError:
                raise ProtocolError("closed before register")
            logger.debug(f"msg: {msg}")

            # Only Connect is acceptable.
            msg_type, msg = deserialize_message(msg)
            if msg_type != "Register":
                raise ProtocolError(f"expected register; got {msg_type}")

        except Exception as exc:
            logger.warning(f"{info}: {exc}")
            await ws.close()
            return

        conn_id = msg.conn_id

        # Do we recognize this connection?
        try:
            old_connection = self.connections[conn_id]

        except KeyError:
            # A new connection ID.
            connection = Connection(info, ws, msg.group, connect_time)
            self.connections[conn_id] = connection
            logger.info(f"{info}: connected: {conn_id} group {msg.group}")

        else:
            # Previous connection with the same ID.  First, some sanity checks.
            if info.address != old_connection.info.address:
                # Allow the address to change, in case the remote reconnects
                # through a different interface.  The port may always be
                # different, of course.
                logger.warning(
                    f"{info}: reconnect {conn_id}: "
                    f"old address was {old_connection.info.address}"
                )
            if msg.group != old_connection.group:
                logger.error(
                    f"{info}: reconnect {conn_id}: group changed: {msg.group}")
                ws.close()
                return

            # Is the old connection socket still (purportedly) open?
            if not old_connection.ws.closed:
                logger.warning(f"reconnect {conn_id}: closing old connection")
                old_connection.ws.close()
                assert not old_connection.ws.open

            # Use the new socket with the old connection.
            old_connection.info = info
            old_connection.ws = ws
            old_connection.group = msg.group
            old_connection.connect_time = connect_time
            connection = old_connection
            logger.info(f"{info}: reconnected: {conn_id}")

        # Receive messages.
        while True:
            try:
                msg = await ws.recv()
            except ConnectionClosedError:
                logger.info(f"{info}: connection closed")
                break
            type, msg = deserialize_message(msg)
            logger.info(f"received: {msg}")

            # FIXME: For testing.
            if type == "ProcResult" and msg.res["status"] is not None:
                await connection.send(proto.ProcDeleteRequest(msg.proc_id))

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
        await connection.send(proto.ProcStart(procs={proc_id: spec}))
        return Process(connection, proc_id)



async def run(server, loc=(None, proto.DEFAULT_PORT)):
    """
    Creates a WebSockets server at `loc` and accepts connections to it with
    `server`.

    :param loc:
      The (host, port) on which to run.
    """
    proc_id = "testproc0"
    spec = {"argv": ["/usr/bin/sleep", "5"]}
    process = None

    async with server.serve(loc):
        # FIXME: For testing.
        while True:
            await asyncio.sleep(1)
            print(f"connections: {', '.join(server.connections)}")
            print(f"groups: {server.connections._Connections__groups}")
            for connection in server.connections.values():
                await connection.send(proto.ProcidListRequest())

            if process is None and len(server.connections) > 0:
                await asyncio.sleep(1)
                process = await server.start(
                    group   ="default",
                    proc_id =proc_id,
                    spec    =spec,
                )
                # await next(iter(server.connections.values())).ws.close()

            if process is not None and process.connection in server.connections.values():
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

