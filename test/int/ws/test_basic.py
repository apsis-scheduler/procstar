import asyncio
import pytest

from   procstar import proto, spec
from   procstar.testing import make_test_instance

#-------------------------------------------------------------------------------

def wait_for(server, msg_type, proc_id, *, timeout=1):
    """
    Waits for and returns the next message of `msg_type` for `proc_id`,
    discarding any intervening messages.

    :raise asyncio.TimeoutError:
      `timeout` seconds elapsed before receiving such a message.
    """
    # FIXME: Python 3.11: Use asyncio.timeout to simplify this.

    async def wait():
        async for _, msg in server:
            if isinstance(msg, msg_type) and msg.proc_id == proc_id:
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

        # First, a result with no status set.
        msg = await wait_for(inst.server, proto.ProcResult, proc_id)
        assert msg.res["status"] is None
        pid = msg.res["pid"]

        msg = await wait_for(inst.server, proto.ProcResult, proc_id)
        assert msg.res["pid"] == pid
        assert msg.res["status"] is not None
        assert msg.res["status"]["exit_code"] == 0
        assert msg.res["fds"]["stdout"]["text"] == "Hello, world!\n"
        assert msg.res["fds"]["stderr"]["text"] == ""


    # echo0   = spec.make_proc(["/usr/bin/echo", "Hello, world!"])
    # echo1   = spec.make_proc("echo This 'is a test.'")
    # sleep0  = spec.make_proc(["/usr/bin/sleep", 1])
    # sleep1  = spec.make_proc("sleep 1")
