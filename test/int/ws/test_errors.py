import pytest

from   procstar import spec
from   procstar.testing import Instance

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_bad_exe():
    async with Instance.start() as inst:
        proc = await inst.server.start(
            "bad_exe",
            spec.make_proc(["/dev/null/bad_exe", "Hello, world!"]).to_jso()
        )
        res = await proc.wait_for_completion()
        assert len(res.errors) == 1
        # FIXME: Needs to be indicated better; see execve() failure handling in
        # start_procs().
        assert "bad_exe" in res.errors[0]
        assert res.status.exit_code == 63


