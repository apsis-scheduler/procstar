import asyncio
from   contextlib import asynccontextmanager, suppress
import functools
import logging
import os
from   pathlib import Path
import secrets
import shutil
import signal
import socket
import tempfile
import uuid

from   procstar import proto
import procstar.agent.server

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
        path = shutil.which("procstar")
        if path is None:
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


class ProcstarError(RuntimeError):
    """
    A procstar process terminated unexpectedly.
    """


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
        self.access_token = access_token

        # The procstar server.
        self.server = procstar.agent.server.Server()

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

        # Temporary files.
        self.temp_dir = tempfile.TemporaryDirectory()


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
            host        ="localhost",
            port        =self.port,
            tls_cert    =(TLS_CERT_PATH, TLS_KEY_PATH),
            access_token=self.access_token,
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


    def _build(self, conn_id, group_id, access_token, args):
        """
        Returns argv and env to start a procstar process.
        """
        token = (
            self.access_token if access_token is DEFAULT
            else access_token
        )
        return (
            [
                get_procstar_path(),
                "--agent",
                "--agent-host", "localhost",
                "--agent-port", str(self.port),
                "--group-id", group_id,
                "--conn-id", conn_id,
                "--connect-count-max", "1",
                *args,
            ],
            {
                "RUST_BACKTRACE": "1",
                "PROCSTAR_AGENT_CERT": str(TLS_CERT_PATH),
                "PROCSTAR_AGENT_TOKEN": token,
            }
            | os.environ
        )


    async def start_procstars(self, counts, *, access_token=DEFAULT, args=[]):
        """
        Starts procstar instances and waits for them to connect.

        :param counts:
          Mapping from group ID to instance count.
        :param args:
          Additional command line args to pass to procstar.
        """
        conns = set(
            (g, str(uuid.uuid4()))
            for g, n in counts.items()
            for _ in range(n)
        )
        procs = set()

        with self.server.connections.subscription() as events:
            # Start the processes.
            for group_id, conn_id in conns:
                argv, env = self._build(conn_id, group_id, access_token, args)
                conn_dir = Path(self.temp_dir.name) / conn_id
                conn_dir.mkdir()
                proc = await asyncio.create_subprocess_exec(
                    *argv,
                    cwd=conn_dir,
                    env=env,
                )
                procs.add(proc)
                self.conn_procs[conn_id] = proc

            async def wait_for_connect(conns):
                """
                Waits for procstar processes to connect.
                """
                connected = set()
                async for _, conn in events:
                    if conn is not None:
                        logger.info(f"instance connected: {conn_id}")
                        connected.add(
                            (conn.info.conn.group_id, conn.info.conn.conn_id)
                        )
                        if len(connected) == len(conns):
                            assert connected == conns
                            return None

            # Create a task to await incoming connections from all procstar
            # processes, and one task awaiting each procstar processes.
            aws = [wait_for_connect(conns)] + [ p.wait() for p in procs ]
            tasks = [ asyncio.create_task(a) for a in aws ]
            try:
                # Wait for a task to complete.  We expect it to be the task
                # awaiting incoming connections.
                res = await next(iter(asyncio.as_completed(tasks)))
                if res is not None:
                    # A procstar process failed before connecting.
                    raise ProcstarError(
                        f"procstar process failed with {res} before connecting")
            finally:
                for task in tasks:
                    task.cancel()


    def start_procstar(self, *, group_id=proto.DEFAULT_GROUP, args=[]):
        """
        Starts a single procstar instance.
        """
        return self.start_procstars({group_id: 1}, args=args)


    async def stop_instance(self, conn_id):
        """
        Stops a procstar instance.
        """
        process = self.conn_procs.pop(conn_id)
        with suppress(ProcessLookupError):
            process.send_signal(signal.SIGKILL)
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


    async def aclose(self):
        """
        Shuts everything down.
        """
        await self.stop_instances()
        await self.stop_server()
        self.temp_dir.cleanup()


    @classmethod
    @asynccontextmanager
    async def start(cls, *, counts={"default": 1}, access_token=DEFAULT):
        """
        Async context manager for a ready-to-go assembly.

        Yields an assembley with procstar instances and the websocket server
        already started.  Shuts them down on exit.
        """
        asm = cls(access_token=access_token)
        await asm.start_server()
        await asm.start_procstars(counts)
        try:
            yield asm
        finally:
            await asm.aclose()



