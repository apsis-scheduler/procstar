import logging
import pytest
import sys

from procstar import proto
from procstar.spec import Proc, make_proc
from procstar.testing.agent import Assembly

logger = logging.getLogger(__name__)

# -------------------------------------------------------------------------------


@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
@pytest.mark.asyncio
async def test_fd_output(mode):
    """
    Tests retrieval of detached output.
    """
    PROC_ID = "test_fd_output"

    async with Assembly.start() as asm:
        proc, res = await asm.server.start(
            PROC_ID,
            make_proc(
                ["/usr/bin/echo", "Hello, world!"],
                fds={
                    "stdout": Proc.Fd.Capture(mode, "utf-8", attached=False),
                },
            ).to_jso(),
        )

        assert res.status is None

        res = await anext(proc.updates)
        assert res.status.exit_code == 0
        assert res.fds.stdout.type == "detached"

        # Request the entire stdout.
        await proc.request_fd_data("stdout")
        fd_data = await anext(proc.updates)
        assert fd_data.fd == "stdout"
        assert fd_data.encoding == "utf-8"
        assert fd_data.data == b"Hello, world!\n"

        await proc.delete()


@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
@pytest.mark.asyncio
async def test_fd_output_large(mode):
    """
    Tests retrieval of large detached output.
    """
    PROC_ID = "test_fd_output_large"
    SIZE = 64 * 1024**2

    async with Assembly.start() as asm:
        proc, res = await asm.server.start(
            PROC_ID,
            make_proc(
                [sys.executable, "-c", f"print('x' * {SIZE}, end='')"],
                fds={
                    "stdout": Proc.Fd.Capture(mode, "utf-8", attached=False),
                },
            ).to_jso(),
        )
        assert res.state == "running"
        assert res.status is None

        res = await anext(proc.updates)
        assert res.status.exit_code == 0
        assert res.fds.stdout.type == "detached"

        conn_id = asm.server.processes[PROC_ID].conn_id
        conn = asm.server.connections[conn_id]

        # Request the entire stdout.
        await conn.send(proto.ProcFdDataRequest(PROC_ID, "stdout"))
        fd_data = await anext(proc.updates)
        assert fd_data.encoding == "utf-8"
        assert fd_data.data == b"x" * SIZE

        await proc.delete()


if __name__ == "__main__":
    import asyncio
    from procstar.lib import logging

    logging.configure(level="debug")
    asyncio.run(test_fd_output_large("tempfile"))
