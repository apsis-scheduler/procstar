import contextlib
from   dataclasses import dataclass
import functools
import logging
import os
from   pathlib import Path
import signal
import socket
import subprocess
import urllib.parse

from   procstar import proto
from   procstar.lib.net import make_netloc
import procstar.ws_service

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

@functools.cache
def get_procstar_path() -> Path:
    """
    Returns the path to the procstar binary.
    """
    try:
        path = os.environ["PROCSTAR"]
    except KeyError:
        # FIXME: This is not the right place, or the right way.
        path = Path(__file__).parents[2] / "target" / "debug" / "procstar"

    assert os.access(path, os.X_OK), f"missing exe {PROCSTAR_PATH}"
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


@dataclass
class TestInstance:
    name: str
    group: str

    server: procstar.ws_service.Server
    ws_server: object  # FIXME
    process: subprocess.Popen

    @functools.cached_property
    def locs(self):
        return tuple(_get_local(self.ws_server))


    @functools.cached_property
    def urls(self):
        return tuple(
            f"ws://{h}:{p}"
            for h, p in self.locs
        )



@contextlib.asynccontextmanager
async def test_instance(
        name=None,
        group=proto.DEFAULT_GROUP,
        loc=("localhost", None),
):
    server = procstar.ws_service.Server()

    # Start the websocket service.
    async with server.run(loc=loc) as ws_server:
        # The server may be connected to multiple interfaces.  Choose the first
        # one and build a URL.
        ws_loc = next(_get_local(ws_server))
        url = urllib.parse.urlunsplit(("ws", make_netloc(ws_loc), "", "", ""))
        # Start procstar, telling it to connect to the the service.
        process = subprocess.Popen(
            [get_procstar_path(), "--connect", url],
            env={"RUST_BACKTRACE": "1"} | os.environ,
        )

        try:
            # Wait for the first message, the procstar instance registering.
            conn_id, msg = await anext(aiter(server))
            assert isinstance(msg, proto.Register)
            assert msg.group == group
            assert msg.conn_id == conn_id

            # Ready for use.
            yield TestInstance(
                name        =name,
                group       =group,
                server      =server,
                ws_server   =ws_server,
                process     =process,
            )

        finally:
            try:
                status = process.wait(timeout=0)
            except subprocess.TimeoutExpired:
                # Good, the process should not have ended yet.
                pass
            else:
                raise RuntimeError(f"procstar terminated with status {status}")

            # Shut down procstar.
            process.send_signal(signal.SIGTERM)
            status = process.wait()


