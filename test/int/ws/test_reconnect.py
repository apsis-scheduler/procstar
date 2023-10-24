import asyncio
import pytest

from   procstar import spec
from   procstar.testing import Instance

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_ws_reconnect():
    """
    Starts a couple of processes, drops the websocket connection, waits for
    reconnection, and confirms that process results are still accessible.
    """
    async with Instance.start() as inst:
        proc0 = await inst.server.start(
            "reconnect0",
            spec.make_proc(["/usr/bin/sleep", "0.2"])
        )
        proc1 = await inst.server.start(
            "reconnect1",
            spec.make_proc(["/usr/bin/sleep", "0.4"])
        )

        with inst.server.connections.subscription() as sub:
            for _ in range(3):
                # Close the connection.
                conn, = inst.server.connections.values()
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
    async with Instance.start() as inst:
        proc0 = await inst.server.start(
            "reconnect0",
            spec.make_proc(["/usr/bin/sleep", "0.2"])
        )
        proc1 = await inst.server.start(
            "reconnect1",
            spec.make_proc(["/usr/bin/sleep", "0.4"])
        )

        # Close the connection.
        conn, = inst.server.connections.values()
        await conn.ws.close()
        assert conn.ws.closed

        # Wait for results anyway.  The procstar instance should reconnect.
        res0, res1 = await asyncio.gather(
            proc0.wait_for_completion(),
            proc1.wait_for_completion()
        )
        assert res0.status.exit_code == 0
        assert res1.status.exit_code == 0


@pytest.mark.asyncio
async def test_proc_reconnect():
    """
    Reconnects both the ws and `Process` instances, simulating restart.
    """
    async with Instance.start() as inst:
        proc0 = await inst.server.start(
            "reconnect0",
            spec.make_proc(["/usr/bin/sleep", "0.2"])
        )
        proc1 = await inst.server.start(
            "reconnect1",
            spec.make_proc(["/usr/bin/sleep", "0.4"])
        )

        # Restart server.
        print("stopping")
        await inst.stop_server()
        print("starting")
        await inst.start_server()

        print("waiting")
        res0, res1 = await asyncio.gather(
            proc0.wait_for_completion(),
            proc1.wait_for_completion(),
        )
        assert res0.status.exit_code == 0
        assert res1.status.exit_code == 0


