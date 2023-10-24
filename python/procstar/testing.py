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


class Instance:

    BIND_ADDR = "127.0.0.1"

    def __init__(self):
        self.port = None
        self.server = procstar.ws_service.Server()
        self.ws_server = None
        self.ws_task = None
        self.processes = {}


    async def start_server(self):
        assert self.ws_server is None
        assert self.ws_task is None
        # Create the websockets server, that runs our protocol server.  Choose a
        # new port the first time, then keep using the same port, so procstar
        # instances can reconnect.
        self.ws_server = await self.server.run(loc=(self.BIND_ADDR, self.port))
        self.port = self.locs[0][1]
        print(f"started on port {self.port}")
        # Start it up in a task.
        self.ws_task = asyncio.create_task(self.ws_server.serve_forever())


    async def stop_server(self):
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


    # @functools.cached_property
    @property
    def locs(self):
        """
        Returns a sequence of host, port to which the websocket server is bound.
        """
        return tuple(_get_local(self.ws_server))


    # @functools.cached_property
    @property
    def urls(self):
        return tuple(
            urllib.parse.urlunsplit(("ws", make_netloc(l), "", "", ""))
            for l in self.locs
        )


    async def start_procstar_instance(self, *, group=proto.DEFAULT_GROUP):
        url = self.urls[0]
        with self.server.connections.subscription() as events:
            conn_id = str(uuid.uuid4())
            process = await asyncio.create_subprocess_exec(
                # FIXME: s/--name/--conn-id/
                get_procstar_path(), "--connect", url, "--name", conn_id,
                # FIXME: cwd=tmp_dir
                env={"RUST_BACKTRACE": "1"} | os.environ,
            )

            async for _, conn in events:
                if conn is not None and conn.conn_id == conn_id:
                    assert conn.group == group
                    logger.info(f"procstar connected: {conn_id}")
                    self.processes[conn_id] = process
                    return conn_id


    async def stop_procstar_instance(self, conn_id):
        process = self.processes.pop(conn_id)
        process.send_signal(signal.SIGTERM)
        await process.wait()


    def stop_procstar_instances(self):
        conn_ids = tuple(self.processes.keys())
        return asyncio.gather(*(
            self.stop_procstar_instance(c)
            for c in conn_ids
        ))


    async def close(self):
        await self.stop_procstar_instances()
        await self.stop_server()


    @classmethod
    @asynccontextmanager
    async def start(cls, *, instances={"default": 1}):
        inst = cls()
        await inst.start_server()
        await asyncio.gather(
            *(
                inst.start_procstar_instance(group=g)
                for g, c in instances.items()
                for _ in range(c)
            )
        )

        try:
            yield inst
        finally:
            await inst.close()



