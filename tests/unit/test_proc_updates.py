import asyncio

import pytest

from procstar import proto
from procstar.agent.proc import Process, SignalNotDelivered


class _Conn:
    conn_id = "c0"


@pytest.mark.asyncio
async def test_signal_not_delivered_is_yielded():
    proc = Process(_Conn(), "p0")
    proc._on_message(
        proto.RequestError(
            msg={"type": "ProcSignalRequest", "proc_id": "p0", "signum": 15},
            err="no process",
        )
    )
    proc._on_message(proto.ProcDelete("p0"))
    updates = [u async for u in await asyncio.wait_for(_collect(proc), 5)]
    assert updates == [SignalNotDelivered(signum=15)]


async def _collect(proc):
    async def gen():
        async for u in proc.updates:
            yield u

    return gen()
