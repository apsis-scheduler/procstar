import asyncio
from   contextlib import asynccontextmanager
import functools
import logging
import os
from   pathlib import Path
import secrets
import signal
import socket
import tempfile
import uuid

from   procstar import proto
import procstar.ws.server

logger = logging.getLogger(__name__)

DEFAULT = object()

#-------------------------------------------------------------------------------

@functools.cache
def get_procstar_path() -> Path:
    """
    Returns the path to the procstar binary.

    Uses the env var `PROCSTAR`, if set.
    """
    try:
        path = os.environ["PROCSTAR"]
    except KeyError:
        # FIXME: This is not the right place, or the right way.
        path = Path(__file__).parents[3] / "target" / "debug" / "procstar"

    assert os.access(path, os.X_OK), f"missing exe {path}"
    logging.info(f"using {path}")
    return path


# Use a self-signed cert for localhost for integration tests.
TLS_CERT_PATH = Path(__file__).parent / "localhost.crt"
TLS_KEY_PATH = TLS_CERT_PATH.with_suffix(".key")

#-------------------------------------------------------------------------------

def _get_local(ws_server):
    """
    Returns an iterable of local socket names bound by `ws_server`.
    """
    return (
        s.getsockname()[: 2]
        for s in ws_server.sockets
        if s.type == socket.SOCK_STREAM
        and s.family in (socket.AF_INET, socket.AF_INET6)
    )


class Assembly:
    """
    Integration test assembly consisting of a websocket server and multiple
    procstar instances connecting to it.
    """

    def __init__(self, *, access_token=DEFAULT):
        """
        Does not start the websocket server or any procstar instances.
        """
        if access_token is DEFAULT:
            access_token = secrets.token_urlsafe(32)

        # The procstar server.
        self.server = procstar.ws.server.Server(access_token=access_token)

        # The port on which the websocket server is running.  Automatically
        # assigned the first time the server starts.
        self.port = None
        # The websocket server.
        self.ws_server = None
        # The task running the websocket server.
        self.ws_task = None

        # Async (OS) process objects for the procstar instance processes, keyed
        # by conn_id.
        self.conn_procs = {}


    async def start_server(self):
        """
        Starts the websocket server.

        :precondition:
          The server is not started.
        """
        assert self.ws_server is None
        assert self.ws_task is None
        # Create the websockets server, that runs our protocol server.  Choose a
        # new port the first time, then keep using the same port, so procstar
        # instances can reconnect.
        self.ws_server = await self.server.run(
            loc=("localhost", self.port),
            tls_cert=(TLS_CERT_PATH, TLS_KEY_PATH),
        )
        self.port = self.locs[0][1]
        logger.info(f"started on port {self.port}")
        # Start it up in a task.
        self.ws_task = asyncio.create_task(self.ws_server.serve_forever())


    async def stop_server(self):
        """
        Stops the websocket server.

        Idempotent.
        """
        if self.ws_server is None and self.ws_task is None:
            # Not started.
            return

        self.ws_server.close()
        await self.ws_server.wait_closed()
        try:
            await self.ws_task
        except asyncio.CancelledError:
            pass

        self.ws_server = None
        self.ws_task = None


    @property
    def locs(self):
        """
        A sequence of host, port to which the websocket server is bound.
        """
        return tuple(_get_local(self.ws_server))


    async def start_instances(self, counts, *, access_token=DEFAULT):
        """
        Starts procstar instances and waits for them to connect.

        :param counts:
          Mapping from group ID to instance count.
        """
        url = f"wss://localhost:{self.port}"
        conns = set(
            (g, str(uuid.uuid4()))
            for g, n in counts.items()
            for _ in range(n)
        )
        token = (
            self.server.access_token if access_token is DEFAULT
            else access_token
        )

        with self.server.connections.subscription() as events:
            # Start the processes.
            for group_id, conn_id in conns:
                self.conn_procs[conn_id] = await asyncio.create_subprocess_exec(
                    # FIXME: s/--name/--conn-id/
                    get_procstar_path(),
                    "--connect", url,
                    "--group-id", group_id,
                    "--name", conn_id,
                    # FIXME: cwd=tmp_dir
                    env={
                        "RUST_BACKTRACE": "1",
                        "PROCSTAR_CERT": str(TLS_CERT_PATH),
                    }
                    | ({"PROCSTAR_TOKEN": token} if token else {})
                    | os.environ,
                )

            # Wait for them to connect.
            # FIXME: Timeout.
            connected = set()
            async for _, conn in events:
                if conn is not None:
                    logger.info(f"instance connected: {conn_id}")
                    connected.add((conn.info.conn.group_id, conn.info.conn.conn_id))
                    if len(connected) == len(conns):
                        assert connected == conns
                        return


    def start_instance(self, *, group_id=proto.DEFAULT_GROUP):
        """
        Starts a single procstar instance.
        """
        return self.start_instances({group_id: 1})


    async def stop_instance(self, conn_id):
        """
        Stops a procstar instance.
        """
        process = self.conn_procs.pop(conn_id)
        process.send_signal(signal.SIGTERM)
        await process.wait()


    def stop_instances(self):
        """
        Stops all procstar instances.
        """
        conn_ids = tuple(self.conn_procs.keys())
        return asyncio.gather(*(
            self.stop_instance(c)
            for c in conn_ids
        ))


    async def close(self):
        """
        Shuts everything down.
        """
        await self.stop_instances()
        await self.stop_server()


    @classmethod
    @asynccontextmanager
    async def start(cls, *, counts={"default": 1}, access_token=DEFAULT):
        """
        Async context manager for a ready-to-go assembly.

        Yields an assembley with procstar instances and the websocket server
        already started.  Shuts them down on exit.
        """
        inst = cls(access_token=access_token)
        await inst.start_server()
        await inst.start_instances(counts)
        try:
            yield inst
        finally:
            await inst.close()



