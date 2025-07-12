import asyncio
from collections import Counter
import itertools
import os
import pytest
import socket

from procstar import spec
from procstar.testing.agent import Assembly

# -------------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_connect():
    """
    Basic connection tests.
    """
    async with Assembly.start() as asm:
        assert asm.access_token is not None
        assert len(asm.server.connections) == 1
        conn = next(iter(asm.server.connections.values()))
        assert conn.info.conn.group_id == "default"
        conn_proc = asm.conn_procs[conn.info.conn.conn_id]
        assert conn.info.proc.pid == conn_proc.pid
        assert conn.info.proc.euid == os.geteuid()
        assert conn.info.proc.hostname == socket.gethostname()


@pytest.mark.asyncio
async def test_connect_no_token():
    """
    Tests connection without access token.
    """
    async with Assembly.start(access_token="") as asm:
        assert asm.access_token == ""
        assert len(asm.server.connections) == 1


@pytest.mark.asyncio
async def test_connect_multi():
    """
    Tests multiple procstar instances in more than one group.
    """
    counts = {"red": 1, "green": 3, "blue": 2}
    async with Assembly.start(counts=counts) as asm:
        conns = asm.server.connections
        assert len(conns) == 6
        assert dict(Counter(c.info.conn.group_id for c in conns.values())) == counts


@pytest.mark.asyncio
async def test_run_proc():
    proc_id = "testproc"

    async with Assembly.start() as asm:
        proc, res = await asm.server.start(
            proc_id, spec.make_proc(["/usr/bin/echo", "Hello, world!"]).to_jso()
        )
        assert proc.proc_id == proc_id

        assert len(asm.server.processes) == 1
        assert next(iter(asm.server.processes)) == proc_id
        assert next(iter(asm.server.processes.values())) is proc

        assert res is not None
        assert res.status is None
        assert res.pid is not None
        pid = res.pid

        # Now a result when the process completes.
        res = await anext(proc.updates)
        assert res is not None
        assert res.pid == pid
        assert res.status is not None
        assert res.status.exit_code == 0
        assert res.fds.stdout.text == "Hello, world!\n"
        assert res.fds.stderr.text == ""

        # Delete the proc.
        await proc.delete()

        assert len(asm.server.processes) == 0


@pytest.mark.asyncio
async def test_run_procs():
    """
    Runs a handful of simple processes.
    """
    specs = {
        "e0": spec.make_proc(["/usr/bin/echo", "Hello, world!"]),
        "e1": spec.make_proc("echo This 'is a test.'"),
        "s0": spec.make_proc(["/usr/bin/sleep", 0.25]),
        "s1": spec.make_proc("sleep 0.75"),
    }

    async with Assembly.start() as asm:
        starts = await asyncio.gather(*(asm.server.start(i, s) for i, s in specs.items()))
        ress = dict(zip(specs, await asyncio.gather(*(asm.wait(p) for p, _ in starts))))

        assert all(r.status.exit_code == 0 for r in ress.values())
        assert ress["e0"].fds.stdout.text == "Hello, world!\n"
        assert ress["e1"].fds.stdout.text == "This is a test.\n"
        assert ress["s0"].fds.stdout.text == ""
        assert ress["s1"].fds.stdout.text == ""
        assert 0.25 < ress["s0"].times.elapsed < 0.5
        assert 0.75 < ress["s1"].times.elapsed < 1.0
        print([r.fds.stderr.text for r in ress.values()])
        assert all(r.fds.stderr.text == "" for r in ress.values())


@pytest.mark.asyncio
async def test_run_multi():
    """
    Runs multiple processes on multiple instances.
    """
    counts = {"red": 1, "green": 3, "blue": 2}
    group_ids = itertools.cycle(counts.keys())

    async with Assembly.start(counts=counts) as asm:
        # Start a bunch of processes in various groups.
        starts = await asyncio.gather(
            *(
                asm.server.start(
                    f"proc{i}-{(g := next(group_ids))}",
                    spec.make_proc(["/usr/bin/echo", "group", g]),
                    group_id=g,
                )
                for i in range(64)
            )
        )
        procs = [p for p, _ in starts]

        # Each should have been assigned to the right group.
        for proc in procs:
            group = proc.proc_id.split("-", 1)[1]
            assert asm.server.connections[proc.conn_id].info.conn.group_id == group

        # Each should complete successfully.
        ress = await asyncio.gather(*(asm.wait(p) for p in procs))
        for proc, res in zip(procs, ress):
            group = proc.proc_id.split("-", 1)[1]
            assert res.status.exit_code == 0
            assert res.fds.stdout.text == f"group {group}\n"

            # Check that it ran in the right instance.
            assert res.procstar.conn.group_id == group
            conn_id = res.procstar.conn.conn_id
            assert res.procstar.proc.pid == asm.conn_procs[conn_id].pid
            assert res.procstar.proc.ppid == os.getpid()


if __name__ == "__main__":
    import logging

    logging.getLogger().setLevel(logging.INFO)
    asyncio.run(test_run_proc())
