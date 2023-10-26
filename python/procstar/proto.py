from   dataclasses import dataclass
import orjson
from   typing import Dict, List

#-------------------------------------------------------------------------------

DEFAULT_PORT = 18782
DEFAULT_GROUP = "default"

class ProtocolError(Exception):
    """
    Error in the procstar WebSocket message protocol.
    """



#-------------------------------------------------------------------------------

@dataclass
class ProcStart:
    specs: Dict[str, dict]



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

def serialize_message(msg):
    """
    Serializes a message as a WebSocket message.

    :param msg:
      An instance of an `OUTGOING_MESSAGE_TYPES` class.
    """
    cls = msg.__class__
    type = cls.__name__
    assert OUTGOING_MESSAGE_TYPES[type] is cls
    return orjson.dumps({"type": type} | msg.__dict__)


#-------------------------------------------------------------------------------

@dataclass
class ConnectionInfo:
    conn_id: str
    group_id: str


@dataclass
class ProcessInfo:
    pid: int
    uid: int
    euid: int
    username: str
    gid: int
    egid: int
    groupname: str
    hostname: str


@dataclass
class Register:
    conn: ConnectionInfo
    proc: ProcessInfo

    @classmethod
    def from_jso(cls, jso):
        return cls(
            conn=ConnectionInfo(**jso["conn"]),
            proc=ProcessInfo(**jso["proc"]),
        )



@dataclass
class IncomingMessageError:
    msg: dict
    err: str



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
            IncomingMessageError,
            ProcidList,
            ProcResult,
            ProcDelete,
            Register,
    )
}

def deserialize_message(msg):
    """
    Parses a WebSocket message to a message type.

    :return:
      The message type, and an instance of an INCOMING_MESSAGE_TYPES class.
    :raise ProtocolError:
      An invalid message.
    """
    # We use only binary WebSocket messages.
    if not isinstance(msg, bytes):
        raise ProtocolError(f"wrong ws msg type: {type(msg)}")
    # Parse JSON.
    try:
        jso = orjson.loads(msg)
    except orjson.JSONDecodeError as err:
        raise ProtocolError(f"ws msg JSON error: {err}") from None
    if not isinstance(jso, dict):
        raise ProtocolError("msg not a dict")
    # All messages are tagged.
    try:
        type_name = jso.pop("type")
    except KeyError:
        raise ProtocolError("msg missing type") from None
    # Look up the corresponding class.
    try:
        cls = INCOMING_MESSAGE_TYPES[type_name]
    except KeyError:
        raise ProtocolError(f"unknown msg type: {type_name}") from None
    # Convert to an instance of the message class.
    try:
        from_jso = cls.from_jso
    except AttributeError:
        from_jso = lambda o: cls(**o)
    try:
        obj = from_jso(jso)
    except (TypeError, ValueError) as exc:
        raise ProtocolError(f"invalid {type_name} msg: {exc}") from None

    return type_name, obj


