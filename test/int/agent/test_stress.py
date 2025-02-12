# Note: Stress tests may require more open file descriptors than the default
# limits.  Raise them with `ulimit -n 65536` or similar.

import asyncio
import pytest
import sys

from   procstar import spec
from   procstar.agent.proc import ConnectionTimeoutError
from   procstar.agent.server import maybe_set_reconnect_timeout
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
        procs = [ p for p, _ in procs ]

        # Restart server.
        await asm.stop_server()
        await asm.start_server()

        res = await asyncio.wait_for(
            asyncio.gather(*( asm.wait(p) for p in procs )),
            timeout=30
        )
        assert all( r.status.exit_code == 0 for r in res )


@pytest.mark.asyncio
async def test_reconnect_timeout():
    """
    Test that reconnect timeout wont be set on live connections if agents concurrently
    reconnect before the timeout would be set
    """
    async with Assembly.start(counts={"default": 1}, reconnect_timeout=0) as asm:
        proc, _ = await asm.server.start(
            "proc", spec.make_proc(["/usr/bin/sleep", "1"])
        )

        # trigger a disconnection
        conn_id, = asm.conn_procs.keys()
        await asm.server.connections[conn_id].ws.close()

        with pytest.raises(ConnectionTimeoutError):
            await asm.wait(proc)


@pytest.mark.asyncio
async def test_reconnect_timeout_race(monkeypatch):
    """
    Test that reconnect timeout wont be set on live connections if agents concurrently
    reconnect before the timeout would be set
    """
    async with Assembly.start(counts={"default": 1}, reconnect_timeout=0) as asm:
        proc, _ = await asm.server.start(
            "proc", spec.make_proc(["/usr/bin/sleep", "3"])
        )

        # trigger a disconnection
        conn_id, = asm.conn_procs.keys()
        await asm.server.connections[conn_id].ws.close()

        async def mock_set_reconnect(*args, **kwargs):
            # give agent a chance to reconnect before setting the reconnect timeout
            await asyncio.sleep(1)
            return await maybe_set_reconnect_timeout(*args, **kwargs)

        monkeypatch.setattr(
            "procstar.agent.server.maybe_set_reconnect_timeout", mock_set_reconnect
        )
        res = await asm.wait(proc)
        assert res.status.exit_code == 0
