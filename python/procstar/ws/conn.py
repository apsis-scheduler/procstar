import asyncio
from   collections.abc import Mapping
from   dataclasses import dataclass
import ipaddress
import logging
import random
from   websockets.exceptions import ConnectionClosedError

from   .exc import NoGroupError, NoOpenConnectionInGroup
from   procstar.lib.asyn import Subscribeable
from   procstar.proto import ConnectionInfo, ProcessInfo
from   procstar.proto import serialize_message

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

@dataclass
class SocketInfo:
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
class ProcstarInfo:
    """
    Information about a procstar instance and the connection to it.
    """

    conn: ConnectionInfo
    socket: SocketInfo
    proc: ProcessInfo



@dataclass
class Connection:
    """
    A connection to a single procstar instance.

    The connection object survives disconnection and reconnection from the
    procstar instance.  Thus, the websocket may be closed, and the remote
    procstar instance may no longer exist.  If the same procstar instance later
    reconnects, it uses the same `Connection` instance.
    """

    info: ProcstarInfo
    ws: asyncio.protocols.Protocol = None

    def __hash__(self):
        return hash(self.info.conn.conn_id)


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
            logger.warning(f"{self.info.socket}: connection closed")
            # FIXME: Mark it as closed?  Or is its internal closed flag enough?
            # FIXME: Think carefully the temporarily dropped connection logic.



class Connections(Mapping, Subscribeable):

    def __init__(self):
        super().__init__()
        self.__conns = {}
        self.__groups = {}


    def _add(self, procstar_info: ProcstarInfo, ws) -> Connection:
        """
        Adds a new connection or readds an existing one.

        :return:
          The connection object.
        :raise RuntimeError:
          The connection could not be added.
        """
        conn_id = procstar_info.conn.conn_id
        group_id = procstar_info.conn.group_id
        socket_info = SocketInfo.from_ws(ws)

        try:
            # Look for an existing connection with this ID.
            old_conn = self.__conns[conn_id]

        except KeyError:
            # New connection.
            conn = self.__conns[conn_id] = Connection(info=procstar_info, ws=ws)
            # Add it to the group.
            group = self.__groups.setdefault(group_id, set())
            group.add(conn_id)

        else:
            # Previous connection with the same ID.  First, some sanity checks.
            if socket_info.address != old_conn.info.socket.address:
                # Allow the address to change, in case the remote reconnects
                # through a different interface.  The port may always be
                # different, of course.
                logger.warning(f"[{conn_id}] new address: {socket_info.address}")
            if group_id != old_conn.info.conn.group_id:
                # The same instance shouldn't connect under a different group.
                raise RuntimeError(f"[{conn_id}] new group: {group}")

            # If the old connection websocket still open, close it.
            if not old_conn.ws.closed:
                logger.warning(f"[{conn_id}] closing old connection")
                _ = asyncio.create_task(old_conn.ws.close())

            # Use the new websocket with the old connection object.
            old_conn.ws = ws
            old_conn.info.socket = socket_info
            conn = old_conn

        self._publish((conn_id, conn))
        return conn


    def _pop(self, conn_id) -> Connection:
        """
        Deletes and returns a connection.
        """
        conn = self.__conns.pop(conn_id)
        group_id = conn.info.conn.group_id
        # Remove it from its group.
        group = self.__groups[group_id]
        group.remove(conn_id)
        # If the group is now empty, clean it up.
        if len(group) == 0:
            del self.__groups[group_id]
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

    def choose_connection(self, group_id) -> Connection:
        """
        Chooses an open connection in 'group'.
        """
        try:
            conn_ids = self.__groups[group_id]
        except KeyError:
            raise NoGroupError(group_id) from None

        connections = [
            c
            for i in conn_ids
            if (c := self[i]).open
        ]
        if len(connections) == 0:
            raise NoOpenConnectionInGroup(group_id)

        # FIXME: Better choice mechanism.
        return random.choice(connections)



