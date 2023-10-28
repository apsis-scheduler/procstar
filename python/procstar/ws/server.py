"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
import logging
from   pathlib import Path
import ssl
import websockets.server
from   websockets.exceptions import ConnectionClosedError

from   .conn import Connections, ProcstarInfo, SocketInfo
from   .proc import Processes, Process
from   procstar import proto

# Timeout to receive an initial login message.
TIMEOUT_LOGIN = 60

# FIXME: What is the temporal scope of a connection?

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

class Server:

    def __init__(self):
        self.connections = Connections()
        self.processes = Processes()


    def run(self, *, loc=(None, None), tls_cert=None):
        """
        Returns an async context manager that runs the websocket server.

        :param loc:
          `host, port` pair.  If `host` is none, runs on all interfaces.
          If `port` is none, chooses an unused port on each interface.
        :param cert:
          If not none, a (cert file path, key file path) pair to use for TLS.
        """
        host, port = loc

        ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        if tls_cert is not None:
            cert_path, key_path = tls_cert
            ssl_context.load_cert_chain(cert_path, key_path)

        # For debugging TLS handshake.
        if False:
            def msg_callback(*args):
                logger.debug(f"TLS: {args}")
            ssl_context._msg_callback = msg_callback

        return websockets.server.serve(
            self._serve_connection,
            host, port,
            ssl=ssl_context,
        )


    async def _update_connection(self, conn):
        """
        Refreshes process state from a connection.
        """
        if conn is not None:
            # Ask the procstar instance to tell us all proc IDs it knows
            # about.
            await conn.send(proto.ProcidListRequest())
            # Ask for updates on all processes we think are at this
            # instance.
            await asyncio.gather(*(
                conn.send(proto.ProcResultRequest(p.proc_id))
                for p in self.processes.values()
                if p.conn_id == conn.info.conn.conn_id
            ))


    async def _serve_connection(self, ws):
        """
        Serves an incoming connection.

        Use this bound method with `websockets.server.serve()`.
        """
        assert ws.open

        try:
            # Wait for a Register message.
            try:
                msg = await asyncio.wait_for(ws.recv(), TIMEOUT_LOGIN)
            except TimeoutError:
                raise proto.ProtocolError(f"no register in {TIMEOUT_LOGIN} s")
            except ConnectionClosedError:
                raise proto.ProtocolError("closed before register")

            # Only Register is acceptable.
            type, register_msg = proto.deserialize_message(msg)
            logging.info(f"recv: {msg}")
            if type != "Register":
                raise proto.ProtocolError(f"expected register; got {type}")

        except Exception as exc:
            logger.warning(f"{ws}: {exc}")
            await ws.close()
            return

        # Add or re-add the connection.
        try:
            procstar_info = ProcstarInfo(
                conn        =register_msg.conn,
                socket      =SocketInfo.from_ws(ws),
                proc        =register_msg.proc,
            )
            conn = self.connections._add(procstar_info, ws)
        except RuntimeError as exc:
            logging.error(str(exc))
            return
        await self._update_connection(conn)

        # Receive messages.
        while True:
            try:
                msg = await ws.recv()
            except ConnectionClosedError:
                logger.info(f"[{conn.info.conn.conn_id}] connection closed")
                break
            type, msg = proto.deserialize_message(msg)
            # Process the message.
            logging.info(f"recv: {msg}")
            self.processes.on_message(conn.info, msg)

        await ws.close()
        assert ws.closed
        # Don't forget the connection; the other end may reconnect.


    async def start(
            self,
            proc_id,
            spec,
            *,
            group=proto.DEFAULT_GROUP,
    ) -> Process:
        """
        Starts a new process on a connection in `group`.

        :return:
          The connection on which the process starts.
        """
        try:
            spec = spec.to_jso()
        except AttributeError:
            pass

        conn = self.connections.choose_connection(group)
        # FIXME: If the connection is closed, choose another.
        await conn.send(proto.ProcStart(specs={proc_id: spec}))
        return self.processes.create(conn.info.conn.conn_id, proc_id)


    async def reconnect(self, conn_id, proc_id) -> Process:
        # FIXME
        raise NotImplementedError()


    async def delete(self, proc_id):
        """
        Deletes a process.
        """
        # FIXME: No proc?
        proc = self.processes[proc_id]
        # FIXME: No connection?
        conn = self.connections[proc.conn_id]
        await conn.send(proto.ProcDeleteRequest(proc_id))



