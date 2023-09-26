"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
from   collections.abc import Sequence
from   dataclasses import dataclass
import ipaddress
import logging
import orjson
from   typing import Dict, List
import websockets.server

#-------------------------------------------------------------------------------

DEFAULT_PORT = 18782

# Timeout to receive an initial login message.
TIMEOUT_LOGIN = 60

class ProtocolError(Exception):
    """
    Error in the procstar WebSocket message protocol.
    """



#-------------------------------------------------------------------------------

@dataclass
class ProcStart:
    procs: Dict[str, dict]



@dataclass
class ProcidListRequest:
    pass



@dataclass
class ProcResultRequest:
    proc_id: str



@dataclass
class ProcDeleteRequest:
    proc_id: str



OUTGOING_MESSAGE_TYPES = {
    c.__name__: c
    for c in (
            ProcStart,
            ProcidListRequest,
            ProcResultRequest,
            ProcDeleteRequest,
    )
}

#-------------------------------------------------------------------------------

@dataclass
class Connect:
    name: str
    group: str



@dataclass
class ProcidList:
    proc_ids: List[str]



@dataclass
class ProcResult:
    proc_id: str
    res: dict



@dataclass
class ProcDelete:
    proc_id: str



INCOMING_MESSAGE_TYPES = {
    c.__name__: c
    for c in (
            Connect,
            ProcidList,
            ProcResult,
            ProcDelete,
    )
}

def _serialize_message(msg):
    """
    Serializes a message as a WebSocket message.

    :param msg:
      An instance of an `OUTGOING_MESSAGE_TYPES` class.
    """
    cls = msg.__class__
    type = cls.__name__
    assert OUTGOING_MESSAGE_TYPES[type] is cls
    return orjson.dumps({"type": type} | msg.__dict__)


def _deserialize_message(msg):
    """
    Parses a WebSocket message to a message type.

    :return:
      An instance of an INCOMING_MESSAGE_TYPES class.
    :raise ProtocolError:
      An invalid message.
    """
    # We use only binary WebSocket messages.
    if not isinstance(msg, bytes):
        raise ProtocolError(f"wrong ws msg type: {type(msg)}")
    # Parse JSON.
    try:
        msg = orjson.loads(msg)
    except orjson.JSONDecodeError as err:
        raise ProtocolError(f"ws msg JSON error: {err}") from None
    if not isinstance(msg, dict):
        raise ProtocolError("msg not a dict")
    # All messages are tagged.
    try:
        type_name = msg.pop("type")
    except KeyError:
        raise ProtocolError("msg missing type") from None
    # Look up the corresponding class.
    try:
        cls = INCOMING_MESSAGE_TYPES[type_name]
    except KeyError:
        raise ProtocolError(f"unknown msg type: {type_name}") from None
    # Convert to an instance of the message class.
    try:
        obj = cls(**msg)
    except TypeError as exc:
        raise ProtocolError(f"invalid {type_name} msg: {exc}") from None

    return type_name, obj


#-------------------------------------------------------------------------------

@dataclass
class ConnectionInfo:
    address: ipaddress._BaseAddress
    port: int

    def __str__(self):
        return f"{self.address}:{self.port}"



# FIXME: What is the temporal scope of a connection?

class Server:

    @dataclass
    class __Connection:
        info: ConnectionInfo
        ws: asyncio.protocols.Protocol
        group: str

        async def send(self, msg):
            data = _serialize_message(msg)
            await self.ws.send(data)



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
                raise ProtocolError(
                    f"no Connect msg in {TIMEOUT_LOGIN} s") from None
            self.logger.debug(f"msg: {msg}")

            # Only Connect is acceptable.
            msg_type, msg = _deserialize_message(msg)
            if msg_type != "Connect":
                raise ProtocolError(f"expected Connect msg; got {msg_type}")

        except Exception as exc:
            self.logger.error(f"connection failed: {info}: {exc}")

        old = self.__connections.pop(msg.name, None)
        if old is not None:
            self.logger.info(f"reconnected: {msg.name} was @{old.info}")
            old.ws.close()
            old = None

        self.__connections[msg.name] = self.__Connection(info, ws, msg.group)
        self.logger.info(f"connected: {msg.name} group {msg.group} @{info}")

        # Receive messages.
        while True:
            msg = await ws.recv()
            msg = _deserialize_message(msg)
            self.logger.info(f"received: {msg}")


    @property
    def names(self) -> Sequence[str]:
        """
        Names of current connections.
        """
        return tuple(self.__connections.keys())


    async def request_proc_ids(self, name):
        connection = self.__connections[name]
        await connection.send(ProcidListRequest())


    async def request_proc_result(self, name, proc_id):
        connection = self.__connections[name]
        await connection.send(ProcResultRequest(proc_id))


    async def start_proc(self, name, proc_id, spec):
        connection = self.__connections[name]
        await connection.send(ProcStart(procs={proc_id: spec}))



async def run(server, loc=(None, DEFAULT_PORT)):
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
    logging.basicConfig(level=logging.DEBUG)
    logging.getLogger("websockets.server").setLevel(logging.INFO)
    asyncio.run(run(Server()))


