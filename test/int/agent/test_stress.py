# Note: Stress tests may require more open file descriptors than the default
# limits.  Raise them with `ulimit -n 65536` or similar.

import asyncio
import pytest
import sys

from   procstar import spec
from   procstar.testing.agent import Assembly

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_reconnect():
    """
    Runs a large number of processes with large output.  Restarts the server
    while they are running.
    """
    NUM_PROCS = 256
    LEN_OUTPUT = 1024 ** 2

    async with Assembly.start() as asm:
        procs = await asyncio.gather(*(
            asm.server.start(
                f"proc{i}",
                spec.make_proc([
                    sys.executable,
                    "-c", f"print('x' * {LEN_OUTPUT})",
                ])
            )
            for i in range(NUM_PROCS)
        ))

        # Restart server.
        await asm.stop_server()
        await asm.start_server()

        res = await asyncio.wait_for(
            asyncio.gather(*( p.results.wait() for p in procs )),
            timeout=30
        )
        assert all( r.status.exit_code == 0 for r in res )


