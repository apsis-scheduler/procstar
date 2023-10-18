import pytest

from   procstar.testing import make_test_instance

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_connect():
    async with make_test_instance() as inst:
        assert len(inst.server.connections) == 1


