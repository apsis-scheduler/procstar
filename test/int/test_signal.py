from   base import Process
import signal
import time

#-------------------------------------------------------------------------------

def test_sigterm():
    """
    Sends SIGTERM to procstar and confirms that processes all receive SIGTERM.
    """
    proc_ids = ("0", "1", "2")
    proc = Process({
        "specs": {
            i: {"argv": ["/usr/bin/sleep", "1"]}
            for i in proc_ids
        },
    })
    time.sleep(0.1)
    proc.send_signal(signal.SIGTERM)
    res = proc.wait_result()
    assert set(res) == set(proc_ids)
    for proc_id in proc_ids:
        assert res[proc_id]["status"]["signal"] == "SIGTERM"


if __name__ == "__main__":
    test_sigterm()
