import pytest

from   procstar import spec
from   procstar.testing.agent import Assembly

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_bad_exe():
    async with Assembly.start() as asm:
        proc = await asm.server.start(
            "bad_exe",
            spec.make_proc(["/dev/null/bad_exe", "Hello, world!"]).to_jso()
        )
        res = await proc.results.wait()
        assert res.state == "error"
        assert len(res.errors) == 1
        assert "bad_exe" in res.errors[0]


@pytest.mark.asyncio
async def test_bad_fd_path():
    async with Assembly.start() as asm:
        proc = await asm.server.start(
            "bad_fd",
            spec.make_proc(
                ["/usr/bin/echo", "Hello, world!"],
                fds={
                    "stdout": spec.Proc.Fd.File("/not_a_dir/out"),
                }
            )
        )
        res = await proc.results.wait()
        assert res.state == "error"
        assert len(res.errors) == 1
        assert "No such file or directory" in res.errors[0]


