import asyncio
import pytest

from   procstar import proto, spec
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
        assert proc.result is None

        # First, a result with no status set.
        res = await proc.wait()
        assert res is not None
        assert res["status"] is None
        pid = res["pid"]
        assert pid is not None

        # Now a result when the process completes.
        res = await proc.wait()
        assert res["pid"] == pid
        assert res["status"] is not None
        assert res["status"]["exit_code"] == 0
        assert res["fds"]["stdout"]["text"] == "Hello, world!\n"
        assert res["fds"]["stderr"]["text"] == ""

        # # Request, receive, and check the list of current proc IDs.
        # conn = next(iter(inst.server.connections.values()))
        # await conn.send(proto.ProcidListRequest())
        # msg = await wait_for(inst.server, proto.ProcidList)
        # assert msg.proc_ids == [proc_id]

        # Delete the proc.
        await inst.server.delete(proc_id)
        res = await proc.wait()
        assert res is None

        # # There should be no more proc IDs.
        # conn = next(iter(inst.server.connections.values()))
        # await conn.send(proto.ProcidListRequest())
        # msg = await wait_for(inst.server, proto.ProcidList)
        # assert msg.proc_ids == []


    # echo0   = spec.make_proc(["/usr/bin/echo", "Hello, world!"])
    # echo1   = spec.make_proc("echo This 'is a test.'")
    # sleep0  = spec.make_proc(["/usr/bin/sleep", 1])
    # sleep1  = spec.make_proc("sleep 1")


