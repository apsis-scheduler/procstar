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


@dataclass
class TestInstance:
    """
    The procstar server.
    """
    server: procstar.ws_service.Server

    """
    The underlying websocket server.
    """
    ws_server: object  # FIXME

    """
    The procstar process.
    """
    process: subprocess.Popen

    @functools.cached_property
    def locs(self):
        """
        Returns a sequence of host, port to which the websocket server is bound.
        """
        return tuple(_get_local(self.ws_server))



@contextlib.asynccontextmanager
async def test_instance(
        name=None,
        group=proto.DEFAULT_GROUP,
        loc=("localhost", None),
):
    """
    Creates a test setup consisting of a server and a connected procstar
    instance.

    :return:
      An async context manager which yields an instance of `TestInstance`.
    """
    server = procstar.ws_service.Server()

    # FIXME: Run procstar in a tempdir.
    # FIXME: Capture procstar's own logging.

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
            logger.info("procstar connected")

            # Ready for use.
            yield TestInstance(
                server      =server,
                ws_server   =ws_server,
                process     =process,
            )

        finally:
            # Shut down procstar, if it hasn't already shut down.
            process.send_signal(signal.SIGTERM)
            _ = process.wait(timeout=1)


