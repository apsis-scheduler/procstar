import pytest

from   procstar import spec
from   procstar.agent.testing import Assembly

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_bad_exe():
    async with Assembly.start() as asm:
        proc = await asm.server.start(
            "bad_exe",
            spec.make_proc(["/dev/null/bad_exe", "Hello, world!"]).to_jso()
        )
        res = await proc.results.wait()
        assert len(res.errors) == 1
        assert "bad_exe" in res.errors[0]
        assert res.status.exit_code != 0


