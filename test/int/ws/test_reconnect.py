import asyncio
import pytest

from   procstar import spec
from   procstar.testing import Assembly

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_ws_reconnect():
    """
    Starts a couple of processes, drops the websocket connection, waits for
    reconnection, and confirms that process results are still accessible.
    """
    async with Assembly.start() as asm:
        proc0 = await asm.server.start(
            "reconnect0",
            spec.make_proc(["/usr/bin/sleep", "0.2"])
        )
        proc1 = await asm.server.start(
            "reconnect1",
            spec.make_proc(["/usr/bin/sleep", "0.4"])
        )

        with asm.server.connections.subscription() as sub:
            for _ in range(3):
                # Close the connection.
                conn, = asm.server.connections.values()
                await conn.ws.close()
                assert conn.ws.closed
                # Wait for reconnect.
                conn_id, conn = await anext(sub)

        # Results should be available.
        res0, res1 = await asyncio.gather(
            proc0.wait_for_completion(),
            proc1.wait_for_completion()
        )
        assert res0.status.exit_code == 0
        assert res1.status.exit_code == 0


@pytest.mark.asyncio
async def test_ws_reconnect_nowait():
    """
    Starts a couple of processes, drops the websocket connection, and
    confirms that process results are still accessible, without waiting
    explicitly for reconnect.
    """
    async with Assembly.start() as asm:
        proc0 = await asm.server.start(
            "reconnect0",
            spec.make_proc(["/usr/bin/sleep", "0.2"])
        )
        proc1 = await asm.server.start(
            "reconnect1",
            spec.make_proc(["/usr/bin/sleep", "0.4"])
        )

        # Close the connection.
        conn, = asm.server.connections.values()
        await conn.ws.close()
        assert conn.ws.closed

        # Wait for results anyway.  The procstar asmance should reconnect.
        res0, res1 = await asyncio.gather(
            proc0.wait_for_completion(),
            proc1.wait_for_completion()
        )
        assert res0.status.exit_code == 0
        assert res1.status.exit_code == 0


@pytest.mark.asyncio
async def test_proc_reconnect():
    """
    Reconnects both the ws and `Process` asmances, simulating restart.
    """
    async with Assembly.start() as asm:
        proc0 = await asm.server.start(
            "reconnect0",
            spec.make_proc(["/usr/bin/sleep", "0.2"])
        )
        proc1 = await asm.server.start(
            "reconnect1",
            spec.make_proc(["/usr/bin/sleep", "0.4"])
        )

        # Restart server.
        print("stopping")
        await asm.stop_server()
        print("starting")
        await asm.start_server()

        print("waiting")
        res0, res1 = await asyncio.gather(
            proc0.wait_for_completion(),
            proc1.wait_for_completion(),
        )
        assert res0.status.exit_code == 0
        assert res1.status.exit_code == 0


