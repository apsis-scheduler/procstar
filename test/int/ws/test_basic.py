import asyncio
from   collections import Counter
import pytest

from   procstar import spec
from   procstar.testing import Instance

#-------------------------------------------------------------------------------

@pytest.mark.asyncio
async def test_connect():
    """
    Basic connection tests.
    """
    async with Instance.start() as inst:
        assert len(inst.server.connections) == 1
        conn = next(iter(inst.server.connections.values()))
        assert conn.group == "default"


@pytest.mark.asyncio
async def test_connect_multi():
    """
    Tests multiple procstar instances in more than one group.
    """
    counts = {"red": 1, "green": 3, "blue": 2}
    async with Instance.start(counts=counts) as inst:
        conns = inst.server.connections
        assert len(conns) == 6
        assert dict(Counter( c.group for c in conns.values() )) == counts


@pytest.mark.asyncio
async def test_run_proc():
    proc_id = "testproc"

    async with Instance.start() as inst:
        proc = await inst.server.start(
            proc_id,
            spec.make_proc(["/usr/bin/echo", "Hello, world!"]).to_jso()
        )
        assert proc.proc_id == proc_id
        assert proc.results.latest is None

        assert len(inst.server.processes) == 1
        assert next(iter(inst.server.processes)) == proc_id
        assert next(iter(inst.server.processes.values())) is proc

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
        await inst.server.delete(proc_id)
        res = await anext(proc.results)
        assert res is None

        assert len(inst.server.processes) == 0


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

    async with Instance.start() as inst:
        procs = { i: await inst.server.start(i, s) for i, s in specs.items() }

        futs = ( p.wait_for_completion() for p in procs.values() )
        ress = dict(zip(specs, await asyncio.gather(*futs)))

        assert all( r.status.exit_code == 0 for r in ress.values() )
        assert ress["e0"].fds.stdout.text == "Hello, world!\n"
        assert ress["e1"].fds.stdout.text == "This is a test.\n"
        assert ress["s0"].fds.stdout.text == ""
        assert ress["s1"].fds.stdout.text == ""
        assert all( r.fds.stderr.text == "" for r in ress.values() )


