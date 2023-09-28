"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
from   collections.abc import Sequence
from   dataclasses import dataclass
import datetime
import ipaddress
import logging
import websockets.server
from   websockets.exceptions import ConnectionClosedError

from   . import proto
from   .lib.time import now
from   .proto import ProtocolError, serialize_message, deserialize_message

# Timeout to receive an initial login message.
TIMEOUT_LOGIN = 60

# FIXME: What is the temporal scope of a connection?

#-------------------------------------------------------------------------------

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



class Connections:

    def __init__(self):
        self.__connections = {}
        self.__groups = {}


    def keys(self):
        return self.__connections.keys()


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

        self.connections[conn_id] = Connection(info, ws, msg.group, connect_time)
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
                await self.__send(conn_id, proto.ProcDeleteRequest(msg.proc_id))

        ws.close()


    @property
    def conn_ids(self) -> Sequence[str]:
        """
        IDs of current connections.
        """
        return tuple(self.connections.keys())


    async def __send(self, conn_id, msg):
        try:
            connection = self.connections[conn_id]
        except KeyError:
            raise NoConnectionError(f"no connection: {conn_id}") from None

        data = serialize_message(msg)

        try:
            await connection.ws.send(data)
        except ConnectionClosedError:
            # Connection closed; drop it.
            # FIXME: Don't forget the connection.
            self.logger.warning(f"{connection.info}: connection closed")
            removed = self.connections.pop(conn_id)
            assert removed is self


    async def request_proc_ids(self, conn_id):
        await self.__send(conn_id, proto.ProcidListRequest())


    async def request_proc_result(self, conn_id, proc_id):
        await self.__send(conn_id, proto.ProcResultRequest(proc_id))


    async def start_proc(self, conn_id, proc_id, spec):
        await self.__send(conn_id, proto.ProcStart(procs={proc_id: spec}))







async def run(server, loc=(None, proto.DEFAULT_PORT)):
    """
    Creates a WebSockets server at `loc` and accepts connections to it with
    `server`.

    :param loc:
      The (host, port) on which to run.
    """
    host, port = loc

    started = False
    proc_id = "testproc0"

    async with websockets.server.serve(server.serve_connection, host, port):
        # FIXME: For testing.
        while True:
            await asyncio.sleep(2)
            print(f"connections: {', '.join(server.conn_ids)}")
            for conn_id in server.conn_ids:
                await server.request_proc_ids(conn_id)

            if not started and len(server.conn_ids) > 0:
                await asyncio.sleep(2)
                conn_id = server.conn_ids[0]
                print(f"starting {proc_id}")
                await server.start_proc(conn_id, proc_id, {"argv": ["/usr/bin/sleep", "5"]})
                started = True

            if started:
                await server.request_proc_result(conn_id, proc_id)

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

