from   contextlib import closing
import pytest
from   procstar.testing.inst import Instance

#-------------------------------------------------------------------------------

def test_sync_instance():
    with closing(Instance()) as inst:
        assert inst.client.get_procs() == {}


@pytest.mark.asyncio
async def test_async_instance():
    with closing(Instance()) as inst:
        assert inst.async_client.get_procs
