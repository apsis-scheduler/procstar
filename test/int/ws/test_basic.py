import asyncio
from   collections import Counter
import itertools
import os
import pytest
import socket

from   procstar import spec
from   procstar.testing import Assembly

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_connect():
    """
    Basic connection tests.
    """
    async with Assembly.start() as asm:
        assert len(asm.server.connections) == 1
        conn = next(iter(asm.server.connections.values()))
        assert conn.info.conn.group_id == "default"
        conn_proc = asm.conn_procs[conn.info.conn.conn_id]
        assert conn.info.proc.pid == conn_proc.pid
        assert conn.info.proc.euid == os.geteuid()
        assert conn.info.proc.hostname == socket.gethostname()


@pytest.mark.asyncio
async def test_connect_multi():
    """
    Tests multiple procstar instances in more than one group.
    """
    counts = {"red": 1, "green": 3, "blue": 2}
    async with Assembly.start(counts=counts) as asm:
        conns = asm.server.connections
        assert len(conns) == 6
        assert dict(Counter( c.info.conn.group_id for c in conns.values() )) == counts


@pytest.mark.asyncio
async def test_run_proc():
    proc_id = "testproc"

    async with Assembly.start() as asm:
        proc = await asm.server.start(
            proc_id,
            spec.make_proc(["/usr/bin/echo", "Hello, world!"]).to_jso()
        )
        assert proc.proc_id == proc_id
        assert proc.results.latest is None

        assert len(asm.server.processes) == 1
        assert next(iter(asm.server.processes)) == proc_id
        assert next(iter(asm.server.processes.values())) is proc

        # First, a result with no status set.
        res = await anext(proc.results)
        assert res is not None
        assert res.status is None
        pid = res.pid
        assert pid is not None

        # Now a result when the process completes.
        res = await anext(proc.results)
        assert res.pid == pid
        assert res.status is not None
        assert res.status.exit_code == 0
        assert res.fds.stdout.text == "Hello, world!\n"
        assert res.fds.stderr.text == ""

        # Delete the proc.
        await asm.server.delete(proc_id)
        res = await anext(proc.results)
        assert res is None

        assert len(asm.server.processes) == 0


@pytest.mark.asyncio
async def test_run_procs():
    """
    Runs a handful of simple processes.
    """
    specs = {
        "e0": spec.make_proc(["/usr/bin/echo", "Hello, world!"]),
        "e1": spec.make_proc("echo This 'is a test.'"),
        "s0": spec.make_proc(["/usr/bin/sleep", 1]),
        "s1": spec.make_proc("sleep 1"),
    }

    async with Assembly.start() as asm:
        procs = { i: await asm.server.start(i, s) for i, s in specs.items() }

        futs = ( p.wait_for_completion() for p in procs.values() )
        ress = dict(zip(specs, await asyncio.gather(*futs)))

        assert all( r.status.exit_code == 0 for r in ress.values() )
        assert ress["e0"].fds.stdout.text == "Hello, world!\n"
        assert ress["e1"].fds.stdout.text == "This is a test.\n"
        assert ress["s0"].fds.stdout.text == ""
        assert ress["s1"].fds.stdout.text == ""
        assert all( r.fds.stderr.text == "" for r in ress.values() )


@pytest.mark.asyncio
async def test_run_multi():
    """
    Runs multiple processes on multiple asmances.
    """
    counts = {"red": 1, "green": 3, "blue": 2}
    group_ids = itertools.cycle(counts.keys())

    async with Assembly.start(counts=counts) as asm:
        # Start a bunch of processes in various groups.
        procs = await asyncio.gather(*(
            asm.server.start(
                f"proc{i}-{(g := next(group_ids))}",
                spec.make_proc(["/usr/bin/echo", "group", g]),
                group=g,
            )
            for i in range(64)
        ))

        # Each should have been assigned to the right group.
        for proc in procs:
            group = proc.proc_id.split("-", 1)[1]
            assert asm.server.connections[proc.conn_id].info.conn.group_id == group

        # Each should complete successfully.
        ress = await asyncio.gather(*( p.wait_for_completion() for p in procs ))
        for proc, res in zip(procs, ress):
            group = proc.proc_id.split("-", 1)[1]
            assert res.status.exit_code == 0
            assert res.fds.stdout.text == f"group {group}\n"

            # FIXME: Check procstar process pid, once this is available in results.
            assert res.procstar.conn.group_id == group
            conn_id = res.procstar.conn.conn_id
            assert res.procstar.proc.pid == asm.conn_procs[conn_id].pid
            assert res.procstar.proc.ppid == os.getpid()


# FIXME
if __name__ == "__main__":
    import logging
    logging.getLogger().setLevel(logging.INFO)
    asyncio.run(test_run_proc())

