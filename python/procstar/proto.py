from dataclasses import dataclass
import enum
import msgpack
from typing import Dict, List

from .lib.py import format_ctor
from .lib.string import elide

# -------------------------------------------------------------------------------

DEFAULT_PORT = 18782
DEFAULT_GROUP = "default"


class ProtocolError(Exception):
    """
    Error in the procstar WebSocket message protocol.
    """


# -------------------------------------------------------------------------------

ShutdownState = enum.Enum("ShutdownState", ["active", "idling", "done"])

# -------------------------------------------------------------------------------


@dataclass
class Registered:
    pass


@dataclass
class ProcStartRequest:
    specs: Dict[str, dict]


@dataclass
class ProcidListRequest:
    pass


@dataclass
class ProcResultRequest:
    proc_id: str


@dataclass
class ProcSignalRequest:
    proc_id: str
    signum: int


@dataclass
class ProcFdDataRequest:
    proc_id: str
    fd: str
    start: int = 0
    stop: int | None = None


@dataclass
class ProcDeleteRequest:
    proc_id: str


OUTGOING_MESSAGE_TYPES = {
    c.__name__: c
    for c in (
        Registered,
        ProcStartRequest,
        ProcidListRequest,
        ProcResultRequest,
        ProcSignalRequest,
        ProcFdDataRequest,
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
    return msgpack.dumps({"type": type} | msg.__dict__)


# -------------------------------------------------------------------------------


@dataclass
class ConnectionInfo:
    conn_id: str
    group_id: str
    restricted_exe: str | None

    def to_jso(self):
        return dict(self.__dict__)


@dataclass
class ProcessInfo:
    pid: int
    ppid: int
    uid: int
    euid: int
    username: str
    gid: int
    egid: int
    groupname: str
    hostname: str

    def to_jso(self):
        return dict(self.__dict__)


@dataclass
class Register:
    conn: ConnectionInfo
    proc: ProcessInfo
    proc_ids: List[str]
    access_token: str = ""
    shutdown_state: ShutdownState = ShutdownState.active

    @classmethod
    def from_jso(cls, jso):
        return cls(
            conn=ConnectionInfo(**jso["conn"]),
            proc=ProcessInfo(**jso["proc"]),
            # Allow none for backward compatibility.  CLEANUP#38
            proc_ids=jso.get("proc_ids", None),
            access_token=jso["access_token"],
            shutdown_state=ShutdownState[jso["shutdown_state"]],
        )

    def __repr__(self):
        # Don't format the access token.
        return format_ctor(
            self,
            conn=self.conn,
            proc=self.proc,
            access_token="***",
            shutdown_state=self.shutdown_state.name,
            proc_ids=self.proc_ids,
        )


@dataclass
class RequestError:
    msg: dict
    err: str


@dataclass
class ProcUnknown:
    proc_id: str


@dataclass
class ProcidList:
    proc_ids: List[str]


@dataclass
class ProcResult:
    proc_id: str
    res: dict

    def __str__(self):
        class Omitted:
            def __repr__(self):
                return "…"

        OMITTED = Omitted()

        # Omit fd data from the output.
        name = self.__class__.__name__
        proc_id = self.proc_id
        res = self.res.copy()
        for fd in res["fds"].values():
            if fd is not None and "data" in fd:
                fd["data"] = OMITTED
        for key in ("proc_stat", "proc_statm", "rusage"):
            if key in res:
                res[key] = OMITTED
        return f"{name}(proc_id={proc_id!r}, res={res})"


@dataclass
class ProcFdData:
    proc_id: str
    fd: str
    start: int
    stop: int
    encoding: str | None
    data: str

    def __str__(self):
        # Don't formet the entire data, which may be large.
        return format_ctor(
            self,
            proc_id=self.proc_id,
            fd=self.fd,
            start=self.start,
            stop=self.stop,
            encoding=self.encoding,
            data=elide(self.data, 32, ellipsis=b"...", pos=0.8),
        )


@dataclass
class ProcDelete:
    proc_id: str


@dataclass
class ShutDown:
    shutdown_state: ShutdownState

    @classmethod
    def from_jso(cls, jso):
        return cls(
            shutdown_state=ShutdownState[jso["shutdown_state"]],
        )


INCOMING_MESSAGE_TYPES = {
    c.__name__: c
    for c in (
        RequestError,
        ProcDelete,
        ProcResult,
        ProcFdData,
        ProcUnknown,
        ProcidList,
        Register,
        ShutDown,
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
    # Parse MessagePack.
    try:
        jso = msgpack.loads(msg)
    except msgpack.UnpackException as err:
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


# -------------------------------------------------------------------------------


@dataclass
class ConnectionTimeout:
    pass
