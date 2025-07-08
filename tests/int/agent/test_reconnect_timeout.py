import asyncio
import shutil
import time

import pytest

from procstar import spec
from procstar.agent.exc import ProcessUnknownError
from procstar.agent.proc import ConnectionTimeoutError
from procstar.agent.server import maybe_set_reconnect_timeout
from procstar.testing.agent import Assembly


@pytest.mark.asyncio
async def test_reconnect_timeout():
    async with Assembly.start(counts={"default": 1}, reconnect_timeout=0) as asm:
        proc, _ = await asm.server.start("proc", spec.make_proc([shutil.which("sleep"), "1"]))

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

    async def mock_set_reconnect(*args, **kwargs):
        # give agent a chance to reconnect before setting the reconnect timeout
        await asyncio.sleep(1)
        return await maybe_set_reconnect_timeout(*args, **kwargs)

    monkeypatch.setattr("procstar.agent.server.maybe_set_reconnect_timeout", mock_set_reconnect)

    async with Assembly.start(counts={"default": 1}, reconnect_timeout=0) as asm:
        proc, _ = await asm.server.start("proc", spec.make_proc([shutil.which("sleep"), "3"]))

        # trigger a disconnection
        (conn_id,) = asm.conn_procs.keys()
        await asm.server.connections[conn_id].ws.close()

        res = await asm.wait(proc)
        assert res.status.exit_code == 0

@pytest.mark.asyncio
async def test_dropped_procstart_on_reconnect():
    """
    Replicates the bug where ProcStartRequest messages are dropped when
    the agent reconnects after a ping timeout due to blocked Python event loop.

    This test demonstrates the specific timing issue where messages sent
    during the brief window between disconnect and successful reconnect are lost.
    """
    # Use short read timeout to make test faster
    async with Assembly.start(
        counts={"default": 1}, args=["--agent-read-timeout", "1"]
    ) as asm:
        # block the event loop for longer than agent read timeout
        time.sleep(2)
        try:
            # request a proc start right away before the agent gets a chance to
            # reconnect
            proc1, _ = await asm.server.start(
                "proc1", spec.make_proc(["/usr/bin/true"]), conn_timeout=5,
            )
            await anext(proc1.updates)
        except ProcessUnknownError as e:
            # This is the expected behavior - the message was dropped
            assert False, f"ProcStartRequest dropped: {e}"

        res = await asm.wait(proc1)
        assert res.status.exit_code == 0
