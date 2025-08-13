import asyncio
import pytest
import signal

from procstar import spec
from procstar.agent.proc import Result
from procstar.agent.exc import NoOpenConnectionInGroup
from procstar.testing.agent import Assembly


@pytest.mark.asyncio
async def test_agent_selection_logic():
    """
    Test that the agent selection logic works correctly.
    """
    async with Assembly.start(args=["--wait"]) as asm:
        available_conns = asm.server.connections._get_open_conns_in_group("default")

        assert len(available_conns) == 1, (
            f"Expected 1 available connection, got {len(available_conns)}"
        )

        conn = available_conns[0]
        assert conn.shutdown_state.name == "active"


@pytest.mark.asyncio
async def test_agent_state_transitions():
    async with Assembly.start(args=["--wait"]) as asm:
        conn = next(iter(asm.server.connections.values()))
        assert len(asm.server.connections) >= 1
        assert conn.shutdown_state.name == "active", (
            f"Agent should be active when idle, got: {conn.shutdown_state.name}"
        )

        await asyncio.sleep(1)
        #  agent stays active until no work is assigned
        assert conn.shutdown_state.name == "active", (
            f"Agent should remain active until some work is assigned, got: {conn.shutdown_state.name}"
        )

        # Start a process that runs for a short time
        proc_spec = spec.Proc(["/bin/sleep", "0.2"])
        proc, result = await asm.server.start(
            proc_id="test-proc", group_id="default", spec=proc_spec
        )

        async for update in proc.updates:
            if isinstance(update, Result) and update.state != "running":
                break

        conn_id = conn.info.conn.conn_id
        conn = asm.server.connections.get(conn_id)
        assert conn.shutdown_state.name == "active", (
            "Agent should stay active until all processes are deleted"
        )

        # Delete the completed process to trigger the shutdown sequence
        await proc.delete()

        await asyncio.sleep(1)

        active_conns = asm.server.connections._get_open_conns_in_group("default")
        assert len(active_conns) == 0, f"Expected no active connections, got {len(active_conns)}"

        # Attempt to start second process should fail due to no available connections
        proc_spec2 = spec.Proc(["/bin/echo", "second"])

        with pytest.raises(NoOpenConnectionInGroup):
            await asm.server.start(
                proc_id="test-proc-2",
                group_id="default",
                spec=proc_spec2,
                conn_timeout=0.5,
            )


@pytest.mark.asyncio
async def test_agent_shutdown_before_receiving_work():
    """
    Test that agent responds to shutdown signals even before receiving work.
    """
    async with Assembly.start(args=["--wait"]) as asm:
        conn = next(iter(asm.server.connections.values()))
        conn_id = conn.info.conn.conn_id

        # Send shutdown signal to the procstar process
        procstar_proc = asm.conn_procs[conn_id]
        procstar_proc.send_signal(signal.SIGUSR1)

        # Wait for graceful shutdown
        await asyncio.sleep(0.5)

        # Connection should be cleaned up
        assert conn_id not in asm.server.connections


@pytest.mark.asyncio
async def test_agent_timeout_no_work():
    """
    Test that agent times out and shuts down when no work is assigned within the timeout period.
    """
    async with Assembly.start(args=["--wait", "--wait-timeout", "1"]) as asm:
        # Verify agent starts with active connection
        active_conns = asm.server.connections._get_open_conns_in_group("default")
        assert len(active_conns) == 1, f"Expected 1 active connection, got {len(active_conns)}"

        # Wait for timeout
        await asyncio.sleep(1)

        # Agent should have shut down due to timeout - no more active connections
        active_conns = asm.server.connections._get_open_conns_in_group("default")
        assert len(active_conns) == 0, (
            f"Expected no active connections after timeout, got {len(active_conns)}"
        )
