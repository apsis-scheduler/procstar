import pytest

from   procstar import proto
from   procstar.spec import Proc, make_proc
from   procstar.testing.agent import Assembly

#-------------------------------------------------------------------------------

@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
@pytest.mark.asyncio
async def test_fd_output(mode):
    """
    Tests
    """
    proc_id = "test_fd_output"

    async with Assembly.start() as asm:
        proc = await asm.server.start(
            proc_id,
            make_proc(
                ["/usr/bin/echo", "Hello, world!"],
                fds={
                    "stdout": Proc.Fd.Capture(mode, "utf8", attached=False),
                },
            ).to_jso()
        )

        result = await anext(proc.results)
        assert result.status is None

        result = await(anext(proc.results))
        assert result.status.exit_code == 0
        assert result.fds.stdout.type == "detached"

        conn_id = asm.server.processes[proc_id].conn_id
        conn = asm.server.connections[conn_id]

        # Request the entire stdout.
        await conn.send(proto.ProcFdDataRequest(proc_id, "stdout"))

        await asm.server.delete(proc_id)

        output, encoding = await proc.results.get_fd_res("stdout")
        assert output == b"Hello, world!\n"
        assert encoding == "utf-8"


if __name__ == "__main__":
    import asyncio
    from procstar.lib import logging
    logging.configure(level="debug")
    asyncio.run(test_fd_output("tempfile"))


