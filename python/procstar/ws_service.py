"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
from   collections.abc import Sequence
from   dataclasses import dataclass
import ipaddress
import logging
import websockets.server
from   websockets.exceptions import ConnectionClosedError

from   . import proto
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



class Server:

    def __init__(self):
        # Mapping from connection name to connection.
        self.__connections = {}
        # Use the module logger by default.
        self.logger = logging.getLogger(__name__)


    async def serve_connection(self, ws):
        """
        Serves an incoming connection.

        Use this bound method with `websockets.server.serve()`.
        """
        log = self.logger

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

        name = msg.name

        old = self.__connections.pop(name, None)
        if old is not None:
            log.info(f"reconnected: {name} was @{old.info}")
            old.ws.close()
            old = None

        self.__connections[name] = Connection(info, ws, msg.group)
        log.info(f"{info}: connected: {name} group {msg.group}")

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
                await self.__send(name, proto.ProcDeleteRequest(msg.proc_id))

        ws.close()


    @property
    def names(self) -> Sequence[str]:
        """
        Names of current connections.
        """
        return tuple(self.__connections.keys())


    async def __send(self, name, msg):
        try:
            connection = self.__connections[name]
        except KeyError:
            raise NoConnectionError(f"no connection: {name}") from None

        data = serialize_message(msg)

        try:
            await connection.ws.send(data)
        except ConnectionClosedError:
            # Connection closed; drop it.
            # FIXME: Don't forget the connection.
            self.logger.warning(f"{connection.info}: connection closed")
            removed = self.__connections.pop(name)
            assert removed is self


    async def request_proc_ids(self, name):
        await self.__send(name, proto.ProcidListRequest())


    async def request_proc_result(self, name, proc_id):
        await self.__send(name, proto.ProcResultRequest(proc_id))


    async def start_proc(self, name, proc_id, spec):
        await self.__send(name, proto.ProcStart(procs={proc_id: spec}))



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
            print(f"connections: {', '.join(server.names)}")
            for name in server.names:
                await server.request_proc_ids(name)

            if not started and len(server.names) > 0:
                await asyncio.sleep(2)
                name = server.names[0]
                print(f"starting {proc_id}")
                await server.start_proc(name, proc_id, {"argv": ["/usr/bin/sleep", "5"]})
                started = True

            if started:
                await server.request_proc_result(name, proc_id)

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

