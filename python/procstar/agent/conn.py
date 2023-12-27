"""
Incoming connections to the websocket service.
"""

import asyncio
from   collections.abc import Mapping
from   dataclasses import dataclass
import ipaddress
import logging
import random
import time
from   websockets.exceptions import ConnectionClosedError

from   .exc import NoOpenConnectionInGroup, NoConnectionError
from   procstar.lib.asyn import Subscribeable
from   procstar.proto import ConnectionInfo, ProcessInfo
from   procstar.proto import serialize_message

logger = logging.getLogger(__name__)

# FIXME: Age out old connections.

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


    def to_jso(self):
        return {
            "address"   : str(self.address),
            "port"      : self.port,
        }



@dataclass
class ProcstarInfo:
    """
    Information about a procstar instance and the connection to it.
    """

    conn: ConnectionInfo
    socket: SocketInfo
    proc: ProcessInfo

    def to_jso(self):
        return {
            "conn"  : self.conn.to_jso(),
            "socket": self.socket.to_jso(),
            "proc"  : self.proc.to_jso(),
        }



#-------------------------------------------------------------------------------

@dataclass
class Connection:
    """
    A current or recent connection to a single procstar instance.

    The connection object survives disconnection and reconnection from the
    procstar instance.  Thus, the websocket may be closed, and the remote
    procstar instance may no longer exist.  If the same procstar instance later
    reconnects, it uses the same `Connection` instance.
    """

    info: ProcstarInfo
    ws: asyncio.protocols.Protocol = None

    def __hash__(self):
        return hash(self.info.conn.conn_id)


    def to_jso(self):
        return {
            "info": self.info.to_jso(),
        }


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



#-------------------------------------------------------------------------------

class Connections(Mapping, Subscribeable):
    """
    Tracks current and recent connections.

    Maps connetion ID to `Connection` instances.
    """

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


    def _get_open_conns_in_group(self, group_id):
        """
        Returns a sequence of connection IDs of open connections in a group.
        """
        conn_ids = self.__groups.get(group_id, ())
        return tuple( c for i in conn_ids if (c := self.__conns[i]).open )


    @property
    def groups(self):
        """
        A mapping from group ID to a sequence of conn IDs in the group.
        """
        return dict(self.__groups)


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



async def choose_connection(
        connections: Connections,
        group_id,
        *,
        policy  ="random",
        timeout =0,
) -> Connection:
    """
    Chooses an open connection for 'group_id'.

    :param timeout:
      Timeout to wait for an open connection for `group_id`.
    :raise NoOpenConnectionInGroup:
      Timeout waiting for an open connection.
    """
    deadline = time.monotonic() + timeout

    with connections.subscription() as sub:
        while len(conns := connections._get_open_conns_in_group(group_id)) == 0:
            # Wait to be informed of a new connection.
            remain = deadline - time.monotonic()
            if remain < 0:
                raise NoOpenConnectionInGroup(group_id)

            logger.debug(
                f"no open connection for {group_id}; waiting {remain:.1f} s")
            try:
                # We don't care what precisely happened.
                _ = await asyncio.wait_for(anext(sub), timeout=remain)
            except asyncio.TimeoutError:
                pass

    match policy:
        case "random":
            return random.choice(conns)
        case _:
            raise ValueError(f"unknown policy: {policy}")


async def get_connection(
        connections: Connections,
        conn_id,
        *,
        timeout=0,
) -> Connection:
    """
    Returns a connection, waiting for it if not connected.

    :raise NoConnectionError:
      Timeout waiting for connection `conn_id`.
    """
    deadline = time.monotonic() + timeout

    with connections.subscription() as sub:
        while True:
            try:
                conn = connections[conn_id]
            except KeyError:
                logging.debug(f"no connection: {conn_id}")
            else:
                if conn.open:
                    return conn
                else:
                    logging.debug("connection not open: {conn_id}")

            # Not open connection.  Wait to be informed of a new connection.
            remain = deadline - time.monotonic()
            if remain < 0:
                raise NoConnectionError(conn_id)

            try:
                _ = await asyncio.wait_for(anext(sub), timeout=remain)
            except asyncio.TimeoutError:
                pass


