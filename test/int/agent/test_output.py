import pytest
import sys

from   procstar import proto
from   procstar.spec import Proc, make_proc
from   procstar.testing.agent import Assembly

#-------------------------------------------------------------------------------

@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
@pytest.mark.asyncio
async def test_fd_output(mode):
    """
    Tests retrieval of detached output.
    """
    PROC_ID = "test_fd_output"

    async with Assembly.start() as asm:
        proc = await asm.server.start(
            PROC_ID,
            make_proc(
                ["/usr/bin/echo", "Hello, world!"],
                fds={
                    "stdout": Proc.Fd.Capture(mode, "utf-8", attached=False),
                },
            ).to_jso()
        )

        result = await anext(proc.results)
        assert result.status is None

        result = await(anext(proc.results))
        assert result.status.exit_code == 0
        assert result.fds.stdout.type == "detached"

        conn_id = asm.server.processes[PROC_ID].conn_id
        conn = asm.server.connections[conn_id]

        # Request the entire stdout.
        await conn.send(proto.ProcFdDataRequest(PROC_ID, "stdout"))

        await asm.server.delete(PROC_ID)

        output, encoding = await proc.results.get_fd_res("stdout")
        assert output == b"Hello, world!\n"
        assert encoding == "utf-8"


@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
@pytest.mark.asyncio
async def test_fd_output_large(mode):
    """
    Tests retrieval of large detached output.
    """
    PROC_ID = "test_fd_output_large"
    SIZE = 16 * 1024**2

    async with Assembly.start() as asm:
        proc = await asm.server.start(
            PROC_ID,
            make_proc(
                [sys.executable, "-c", f"print('x' * {SIZE}, end='')"],
                fds={
                    "stdout": Proc.Fd.Capture(mode, "utf-8", attached=False),
                },
            ).to_jso()
        )

        result = await anext(proc.results)
        assert result.status is None

        result = await(anext(proc.results))
        assert result.status.exit_code == 0
        assert result.fds.stdout.type == "detached"

        conn_id = asm.server.processes[PROC_ID].conn_id
        conn = asm.server.connections[conn_id]

        # Request the entire stdout.
        await conn.send(proto.ProcFdDataRequest(PROC_ID, "stdout"))

        await asm.server.delete(PROC_ID)

        output, encoding = await proc.results.get_fd_res("stdout")
        assert output == b"x" * SIZE
        assert encoding == "utf-8"



if __name__ == "__main__":
    import asyncio
    from procstar.lib import logging
    logging.configure(level="debug")
    asyncio.run(test_fd_output_large("tempfile"))


