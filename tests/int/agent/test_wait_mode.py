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
async def test_agent_rejects_second_run():
    """
    Test that after accepting one run, agent becomes unavailable for more runs.
    """
    async with Assembly.start(args=["--wait"]) as asm:
        conn = next(iter(asm.server.connections.values()))

        # Start first process
        proc_spec1 = spec.Proc(["/bin/sleep", "1"])
        proc1, result1 = await asm.server.start(
            proc_id="test-proc-1", group_id="default", spec=proc_spec1
        )

        # Give time for agent to set shutdown state to idling
        await asyncio.sleep(0.15)

        # Agent should now be in idling state, making it unavailable for new work
        conn = asm.server.connections.get(conn.info.conn.conn_id)
        assert conn.shutdown_state.name in ["idling"]

        # Attempt to start second process should fail due to no available connections
        proc_spec2 = spec.Proc(["/bin/echo", "second"])

        with pytest.raises(NoOpenConnectionInGroup):
            await asm.server.start(
                proc_id="test-proc-2",
                group_id="default",
                spec=proc_spec2,
                conn_timeout=0.5,  # Short timeout
            )

        # Wait for first process to complete
        async for update in proc1.updates:
            if isinstance(update, Result) and update.state != "running":
                break


@pytest.mark.asyncio
async def test_agent_state_transitions():
    """
    Test the exact state transitions: active -> idling -> done.
    """
    async with Assembly.start(args=["--wait"]) as asm:
        conn = next(iter(asm.server.connections.values()))
        assert len(asm.server.connections) >= 1
        assert conn.shutdown_state.name == "active", (
            f"Agent should be active when idle, got: {conn.shutdown_state.name}"
        )

        await asyncio.sleep(1)
        #  agent stays active when no work is assigned
        assert conn.shutdown_state.name == "active", (
            f"Agent should remain active when idle, got: {conn.shutdown_state.name}"
        )

        # Initially active
        assert conn.shutdown_state.name == "active"

        # Start a process that runs for a short time
        proc_spec = spec.Proc(["/bin/sleep", "0.2"])
        proc, result = await asm.server.start(
            proc_id="test-proc", group_id="default", spec=proc_spec
        )

        # Give time for state transition to idling
        await asyncio.sleep(0.15)

        conn_id = conn.info.conn.conn_id
        # Should now be idling (prevents new work)
        conn = asm.server.connections.get(conn_id)
        if conn:
            assert conn.shutdown_state.name == "idling"

        # Wait for process to complete
        async for update in proc.updates:
            if isinstance(update, Result) and update.state != "running":
                break

        # Give time for final state transition and cleanup
        await asyncio.sleep(0.3)

        # Connection should be done or cleaned up
        conn = asm.server.connections.get(conn_id)
        if conn:
            # Agent might still be connected but should be done or idling (about to disconnect)
            assert conn.shutdown_state.name in ["done", "idling"]


@pytest.mark.asyncio
async def test_multiple_agents():
    """
    Test multiple single-run agents can each accept one run.
    """
    async with Assembly.start(counts={"default": 3}, args=["--wait"]) as asm:
        assert len(asm.server.connections) == 3

        for conn in asm.server.connections.values():
            assert conn.shutdown_state.name == "active"

        # Start 3 processes - each should go to a different agent
        procs = []
        for i in range(3):
            proc_spec = spec.Proc(["/bin/echo", f"task-{i}"])
            proc, result = await asm.server.start(
                proc_id=f"test-proc-{i}", group_id="default", spec=proc_spec
            )
            procs.append(proc)
            # Give time for agent state changes to propagate
            await asyncio.sleep(0.2)

        # Give time for state transitions
        await asyncio.sleep(0.8)

        # All agents should now be idling (no longer accepting work)
        active_count = sum(
            1 for conn in asm.server.connections.values() if conn.shutdown_state.name == "active"
        )
        assert active_count == 0, "All agents should have transitioned from active state"

        # Wait for all processes to complete
        for proc in procs:
            async for update in proc.updates:
                if isinstance(update, Result) and update.state != "running":
                    break

        # Give time for cleanup
        await asyncio.sleep(0.1)

        with pytest.raises(NoOpenConnectionInGroup):
            proc_spec = spec.Proc(["/bin/echo", "should-fail"])
            await asm.server.start(
                proc_id="test-proc-fail",
                group_id="default",
                spec=proc_spec,
                conn_timeout=0.1,
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
