import asyncio
import pytest

from   procstar import spec
from   procstar.testing import make_test_instance

#-------------------------------------------------------------------------------

def wait_for(server, msg_type, proc_id=None, *, timeout=1):
    """
    Waits for and returns the next message of `msg_type`, for `proc_id` if
    not none, discarding any intervening messages.

    :raise asyncio.TimeoutError:
      `timeout` seconds elapsed before receiving such a message.
    """
    # FIXME: Python 3.11: Use asyncio.timeout to simplify this.

    async def wait():
        async for _, msg in server:
            if (
                    isinstance(msg, msg_type)
                    and (proc_id is None or msg.proc_id == proc_id)
            ):
                return msg

    return asyncio.wait_for(wait(), timeout)


@pytest.mark.asyncio
async def test_connect():
    """
    Basic connection tests.
    """
    async with make_test_instance() as inst:
        assert len(inst.server.connections) == 1
        conn = next(iter(inst.server.connections.values()))
        assert conn.group == "default"


@pytest.mark.asyncio
async def test_run_proc():
    """
    Runs a handful of simple processes.
    """
    proc_id = "testproc"

    async with make_test_instance() as inst:
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
        assert res["status"] is None
        pid = res["pid"]
        assert pid is not None

        # Now a result when the process completes.
        res = await anext(proc.results)
        assert res["pid"] == pid
        assert res["status"] is not None
        assert res["status"]["exit_code"] == 0
        assert res["fds"]["stdout"]["text"] == "Hello, world!\n"
        assert res["fds"]["stderr"]["text"] == ""

        # Delete the proc.
        await inst.server.delete(proc_id)
        res = await anext(proc.results)
        assert res is None

        assert len(inst.server.processes) == 0


@pytest.mark.asyncio
async def test_run_procs():
    specs = {
        "e0": spec.make_proc(["/usr/bin/echo", "Hello, world!"]),
        "e1": spec.make_proc("echo This 'is a test.'"),
        "s0": spec.make_proc(["/usr/bin/sleep", 1]),
        "s1": spec.make_proc("sleep 1"),
    }

    async with make_test_instance() as inst:
        procs = { i: await inst.server.start(i, s) for i, s in specs.items() }

        async def wait_for(proc):
            async for res in proc.results:
                if res["status"] is not None:
                    return res

        waits = ( wait_for(p) for p in procs.values() )
        ress = dict(zip(specs, await asyncio.gather(*waits)))

        assert all( r["status"]["exit_code"] == 0 for r in ress.values() )
        assert ress["e0"]["fds"]["stdout"]["text"] == "Hello, world!\n"
        assert ress["e1"]["fds"]["stdout"]["text"] == "This is a test.\n"
        assert ress["s0"]["fds"]["stdout"]["text"] == ""
        assert ress["s1"]["fds"]["stdout"]["text"] == ""
        assert all( r["fds"]["stderr"]["text"] == "" for r in ress.values() )


