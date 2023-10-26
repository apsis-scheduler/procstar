import asyncio
from   contextlib import asynccontextmanager
import functools
import logging
import os
from   pathlib import Path
import signal
import socket
import tempfile
import urllib.parse
import uuid

from   procstar import proto
from   procstar.lib.net import make_netloc
import procstar.ws_service

logger = logging.getLogger(__name__)

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
        path = Path(__file__).parents[2] / "target" / "debug" / "procstar"

    assert os.access(path, os.X_OK), f"missing exe {path}"
    logging.info(f"using {path}")
    return path


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

    BIND_ADDR = "127.0.0.1"

    def __init__(self):
        """
        Does not start the websocket server or any procstar instances.
        """
        # The procstar server.
        self.server = procstar.ws_service.Server()

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
        self.ws_server = await self.server.run(loc=(self.BIND_ADDR, self.port))
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


    @property
    def urls(self):
        return tuple(
            urllib.parse.urlunsplit(("ws", make_netloc(l), "", "", ""))
            for l in self.locs
        )


    async def start_instances(self, counts):
        """
        Starts procstar instances and waits for them to connect.

        :param counts:
          Mapping from group name to instance count.
        """
        url = self.urls[0]
        conns = set(
            (g, str(uuid.uuid4()))
            for g, n in counts.items()
            for _ in range(n)
        )

        with self.server.connections.subscription() as events:
            # Start the processes.
            for group, conn_id in conns:
                self.conn_procs[conn_id] = await asyncio.create_subprocess_exec(
                    # FIXME: s/--name/--conn-id/
                    get_procstar_path(),
                    "--connect", url,
                    "--group", group,
                    "--name", conn_id,
                    # FIXME: cwd=tmp_dir
                    env={"RUST_BACKTRACE": "1"} | os.environ,
                )

            # Wait for them to connect.
            connected = set()
            async for _, conn in events:
                if conn is not None:
                    logger.info(f"instance connected: {conn_id}")
                    connected.add((conn.info.conn.group_id, conn.info.conn.conn_id))
                    if len(connected) == len(conns):
                        assert connected == conns
                        return


    def start_instance(self, *, group=proto.DEFAULT_GROUP):
        """
        Starts a single procstar instance.
        """
        return self.start_instances({group: 1})


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
    async def start(cls, *, counts={"default": 1}):
        """
        Async context manager for a ready-to-go assembly.

        Yields an assembley with procstar instances and the websocket server
        already started.  Shuts them down on exit.
        """
        inst = cls()
        await inst.start_server()
        await inst.start_instances(counts)
        try:
            yield inst
        finally:
            await inst.close()



