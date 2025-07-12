import contextlib
import functools
import httpx
import logging
import random
import signal
import subprocess
import tempfile

from . import get_procstar_path
import procstar.http.client

log = logging.getLogger(__name__)

# -------------------------------------------------------------------------------


def _build(shutdown, serve, serve_port=None):
    argv = [
        get_procstar_path(),
        "--log-level",
        "info",
    ]

    match shutdown:
        case "exit":
            argv.append("--exit")
        case "wait":
            argv.append("--wait")
        case None:
            pass
        case _:
            raise ValueError(f"bad shutdown: {shutdown!r}")

    if serve:
        argv.append("--serve")
        if serve_port is not None:
            argv.extend(["--serve-port", str(serve_port)])

    env = {
        "RUST_BACKTRACE": "1",
    }

    return argv, env


class Instance:
    def __init__(self, *, shutdown=None, serve=True):
        # FIXME: This is dumb.
        self.serve_port = random.randint(20000, 30000)
        argv, env = _build(
            shutdown=shutdown,
            serve=serve,
            serve_port=self.serve_port,
        )
        self.dir = tempfile.TemporaryDirectory()
        self.proc = subprocess.Popen(
            argv,
            env=env,
            cwd=self.dir.name,
        )

    def close(self):
        # KILL is drastic but otherwise there's a shutdown delay if any
        # undeleted procs remain.
        self.proc.send_signal(signal.SIGKILL)
        self.proc.wait()
        self.proc = None
        self.dir.cleanup()

    @functools.cached_property
    def client(self):
        return procstar.http.client.Client(("localhost", self.serve_port))

    @contextlib.asynccontextmanager
    async def async_client(self):
        async with httpx.AsyncClient() as http_client:
            yield procstar.http.client.AsyncClient(("localhost", self.serve_port), http_client)
