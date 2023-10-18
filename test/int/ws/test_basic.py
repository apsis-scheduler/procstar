import pytest

from   procstar.testing import test_instance

#-------------------------------------------------------------------------------

# FIXME
# @pytest.mark.asyncio
async def test_connect():
    async with test_instance() as inst:
        async for msg in inst.server:
            print(msg)


if __name__ == "__main__":
    import asyncio
    import logging

    logging.getLogger().setLevel(logging.INFO)

    try:
        asyncio.run(test_connect())
    except KeyboardInterrupt:
        pass

