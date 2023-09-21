"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
from   collections.abc import Sequence
from   dataclasses import dataclass
import ipaddress
import logging
import orjson
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
class Connect:
    name: str
    group: str



INCOMING_MESSAGE_TYPES = {
    c.__name__: c
    for c in (
            Connect,
    )
}

def _parse_message(msg):
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
    # Check types of field values.
    for name, field in cls.__dataclass_fields__.items():
        value = getattr(obj, name)
        if not isinstance(getattr(obj, name), field.type):
            raise ProtocolError(f"wrong type for {name}: {value!r}")

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
        socket: asyncio.protocols.Protocol
        group: str



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
            msg_type, msg = _parse_message(msg)
            if msg_type != "Connect":
                raise ProtocolError(f"expected Connect msg; got {msg_type}")

        except Exception as exc:
            self.logger.error(f"connection failed: {info}: {exc}")


        self.logger.info(f"connected: {msg.name} group {msg.group} @{info}")


    @property
    def names(self) -> Sequence[str]:
        """
        Names of current connections.
        """
        return tuple(self.__connections.keys())


    async def request_proc_ids(self, name):
        pass



async def run(server, loc=(None, DEFAULT_PORT)):
    """
    Creates a WebSockets server at `loc` and accepts connections to it with
    `server`.

    :param loc:
      The (host, port) on which to run.
    """
    host, port = loc
    async with websockets.server.serve(server.serve_connection, host, port):
        await asyncio.Future()



if __name__ == "__main__":
    logging.basicConfig(level=logging.DEBUG)
    logging.getLogger("websockets.server").setLevel(logging.INFO)
    asyncio.run(run(Server()))


