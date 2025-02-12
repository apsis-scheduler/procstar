import asyncio

import pytest

from procstar import spec
from procstar.agent.proc import ConnectionTimeoutError
from procstar.agent.server import maybe_set_reconnect_timeout
from procstar.testing.agent import Assembly


@pytest.mark.asyncio
async def test_reconnect_timeout():
    async with Assembly.start(counts={"default": 1}, reconnect_timeout=0) as asm:
        proc, _ = await asm.server.start(
            "proc", spec.make_proc(["/usr/bin/sleep", "1"])
        )

        # trigger a disconnection
        (conn_id,) = asm.conn_procs.keys()
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
        (conn_id,) = asm.conn_procs.keys()
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
