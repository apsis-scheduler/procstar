"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
from   functools import partial
import logging
import os
from   pathlib import Path
import ssl
import websockets.server
from   websockets.exceptions import ConnectionClosedOK, ConnectionClosedError

from   . import DEFAULT_PORT
from   .conn import Connections
from   .conn import choose_connection, get_connection
from   .proc import Processes, Process, ProcessDeletedError
from   procstar import proto
from   procstar.lib.time import now

FROM_ENV = object()

# Timeout to receive an initial login message.
TIMEOUT_LOGIN = 60

# FIXME: What is the temporal scope of a connection?

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

def _get_tls_from_env():
    """
    Returns TLS cert and key file paths from environment, or none if absent.
    """
    try:
        cert_path = Path(os.environ["PROCSTAR_AGENT_CERT"])
    except KeyError:
        # No cert available.
        return None, None
    cert_path = cert_path.absolute()
    if not cert_path.is_file():
        raise RuntimeError(f"PROCSTAR_AGENT_CERT file {cert_path} missing")

    try:
        key_path = Path(os.environ["PROCSTAR_AGENT_KEY"])
    except KeyError:
        # Assume it's next to the cert file.
        key_path = cert_path.with_suffix(".key")
    key_path = key_path.absolute()
    if not key_path.is_file():
        raise RuntimeError(f"PROCSTAR_AGENT_KEY file {key_path} missing")

    return cert_path, key_path


class Server:

    def __init__(self):
        self.connections = Connections()
        self.processes = Processes()


    def run(
            self, *,
            host        =FROM_ENV,
            port        =FROM_ENV,
            tls_cert    =FROM_ENV,
            access_token=FROM_ENV,
        ):
        """
        Returns an async context manager that runs the websocket server.

        :param host:
          Interface on which to run.  If `FROM_ENV`, uses the env var
          `PROCSTAR_AGENT_HOST`.  The default value, `"*"`, runs on all
          interfaces.
        :param port:
           Port on which to run.  If `FROM_ENV`, uses the env var
           `PROCSTAR_AGENT_PORT`.  The default value is `DEFAULT_PORT`.
        :param tls_cert:
          TLS (cert path, key path) to use.  If `FROM_ENV`, uses the env vars
          `PROCSTAR_AGENT_CERT` and `PROCSTAR_AGENT_KEY`.  By default, uses cert
          in the system cert bundle.
        :param access_token:
          Secret access token required for agent connections.  If `FROM_ENV`,
          uses the env var `PROCSTAR_AGENT_TOKEN`.  By default, uses an empty
          string.
        """
        if host is FROM_ENV:
            host = os.environ.get("PROCSTAR_AGENT_HOST", "*")
            if host == "*":
                # Serve on all interfaces.
                host = None
        if port is FROM_ENV:
            port = int(os.environ.get("PROCSTAR_AGENT_PORT", DEFAULT_PORT))

        if tls_cert is FROM_ENV:
            cert_path, key_path = _get_tls_from_env()
        elif tls_cert is None:
            cert_path, key_path = None, None
        else:
            cert_path, key_path = tls_cert

        if access_token is FROM_ENV:
            access_token = os.environ.get("PROCSTAR_AGENT_TOKEN", "")

        ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        if cert_path is not None:
            logger.info(f"using TLS cert {cert_path} key {key_path}")
            ssl_context.load_cert_chain(cert_path, key_path)

        # For debugging TLS handshake.
        if False:
            def msg_callback(*args):
                logger.debug(f"TLS: {args}")
            ssl_context._msg_callback = msg_callback

        return websockets.server.serve(
            partial(self._serve_connection, access_token),
            host, port,
            ssl=ssl_context,
            max_size=64 * 1024**2
        )


    async def run_forever(self, **kw_args):
        server = await self.run(**kw_args)
        # FIXME: Log the/a server URL.


    async def _serve_connection(self, access_token, ws):
        """
        Serves an incoming connection.

        Use this bound method with `websockets.server.serve()`.
        """
        assert ws.open
        time = now()

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
            logger.debug(f"recv: {msg}")
            if type != "Register":
                raise proto.ProtocolError(f"expected register; got {type}")

            # Check the access token.
            if register_msg.access_token != access_token:
                raise proto.ProtocolError("permission denied")

            # Respond with a Registered message.
            data = proto.serialize_message(proto.Registered())
            await ws.send(data)

            logger.info(f"[{register_msg.conn.conn_id}] connected")

        except Exception as exc:
            logger.warning(f"{ws}: {exc}", exc_info=True)
            await ws.close()
            return

        # Add or re-add the connection.
        try:
            conn = self.connections._add(
                register_msg.conn, register_msg.proc, time, ws
            )
            conn.info.stats.num_received += 1  # the Register message
        except RuntimeError as exc:
            logger.error(str(exc))
            return

        # Request results for all procs on this connection.
        try:
            for proc_id, proc in self.processes.items():
                if proc.conn_id == register_msg.conn.conn_id:
                    await conn.send(proto.ProcResultRequest(proc_id))

        except Exception as exc:
            logger.warning(f"{ws}: {exc}", exc_info=True)
            await ws.close()
            return

        # Receive messages.
        while True:
            try:
                msg = await ws.recv()
            except ConnectionClosedOK:
                logger.info(f"[{conn.info.conn.conn_id}] connection closed")
                break
            except ConnectionClosedError as err:
                logger.warning(f"[{conn.info.conn.conn_id}] connection closed: {err}")
                break
            type, msg = proto.deserialize_message(msg)
            # Process the message.
            logger.info(f"recv: {msg}")
            conn.info.stats.num_received += 1
            self.processes.on_message(conn.info, msg)

        # Update stats.
        conn.info.stats.connected = False
        conn.info.stats.last_disconnect_time = now()

        await ws.close()
        assert ws.closed
        # Don't forget the connection; the other end may reconnect.


    async def start(
            self,
            proc_id,
            spec,
            *,
            group_id=proto.DEFAULT_GROUP,
            conn_timeout=0,
    ) -> Process:
        """
        Starts a new process on a connection in `group`.

        :param group_id:
          The group from which to choose a connection.
        :param conn_timeout:
          Timeout to wait for an open connection for `group_id`.
        :return:
          The connection on which the process starts.
        """
        try:
            spec = spec.to_jso()
        except AttributeError:
            pass

        conn = await choose_connection(
            self.connections,
            group_id,
            timeout=conn_timeout,
        )

        await conn.send(proto.ProcStart(specs={proc_id: spec}))
        return self.processes.create(conn.info.conn.conn_id, proc_id)


    async def reconnect(self, conn_id, proc_id, *, conn_timeout=0) -> Process:
        """
        :param conn_timeout:
          Timeout to wait for a connection from `conn_id`.
        :raise NoConnectionError:
          Timeout waiting for connection.
        """
        conn = await get_connection(
            self.connections, conn_id, timeout=conn_timeout)

        try:
            proc = self.processes[proc_id]
        except KeyError:
            proc = self.processes.create(conn_id, proc_id)

        await conn.send(proto.ProcResultRequest(proc_id))
        return proc


    async def send_signal(self, proc_id, signum):
        try:
            proc = self.processes[proc_id]
        except KeyError:
            raise ValueError(f"no process: {proc_id}")
        conn = self.connections[proc.conn_id]

        await conn.send(proto.ProcSignalRequest(proc_id, signum))


    async def delete(self, proc_id):
        """
        Deletes a process.
        """
        try:
            proc = self.processes[proc_id]
        except KeyError:
            raise ValueError(f"no process: {proc_id}")
        conn = self.connections[proc.conn_id]

        await conn.send(proto.ProcDeleteRequest(proc_id))

        # Wait for the deletion message.
        try:
            async for _ in proc.results:
                pass
        except ProcessDeletedError:
            # Good.
            logger.info(f"deleted: {proc_id}")



