from   base import Process
import pytest
import signal
import time

#-------------------------------------------------------------------------------

@pytest.mark.parametrize(
    "signums", [
        (signal.SIGTERM , signal.SIGTERM),
        (signal.SIGINT  , signal.SIGTERM),
        (signal.SIGQUIT , signal.SIGKILL),
    ]
)
def test_sigterm(signums):
    """
    Sends SIGTERM to procstar and confirms that processes all receive SIGTERM.
    """
    send_signum, recv_signum = signums

    proc_ids = ("0", "1", "2")
    proc = Process({
        "specs": {
            i: {"argv": ["/usr/bin/sleep", "1"]}
            for i in proc_ids
        },
    })
    time.sleep(0.1)
    proc.send_signal(send_signum)
    res = proc.wait_result()
    assert set(res) == set(proc_ids)
    for proc_id in proc_ids:
        assert res[proc_id]["status"]["signum"] == recv_signum


if __name__ == "__main__":
    test_sigterm((signal.SIGTERM, signal.SIGTERM))
