from   base import Process
import pytest
import signal
import time

#-------------------------------------------------------------------------------

@pytest.mark.parametrize(
    "signum",
    [signal.SIGTERM, signal.SIGINT, signal.SIGQUIT]
)
def test_shutdown_signal(signum):
    """
    Sends a shutdown signal to procstar and confirms that processes all
    receive the appropriate signal.
    """
    # The signum we expect the client procs to receive.
    proc_signum = {
        signal.SIGTERM  : signal.SIGTERM,
        signal.SIGINT   : signal.SIGTERM,
        signal.SIGQUIT  : signal.SIGKILL,
    }[signum]

    specs = {
        str(i): {"argv": ["/usr/bin/sleep", "1"]}
        for i in range(3)
    }

    # Start procstar.
    proc = Process({"specs": specs})
    # Send the signal.
    proc.send_signal(signum)
    res = proc.wait_result()
    assert set(res) == set(specs)
    for proc_id in specs:
        assert res[proc_id]["status"]["signum"] == proc_signum


def test_idle_signal():
    """
    Sends SIGUSR1 and confirms that procstar exits when idle.
    """
    specs = {
        str(i): {"argv": ["/usr/bin/sleep", str(i)]}
        for i in [0.2, 0.3, 0.4]
    }

    # Start procstar.
    proc = Process({"specs": specs})
    # Send the signal.
    proc.send_signal(signal.SIGUSR1)
    res = proc.wait_result()
    assert set(res) == set(specs)
    # All procs should have slept the specified time and exited successfully.
    for proc_id in specs:
        r = res[proc_id]
        assert r["status"]["exit_code"] == 0
        assert abs(r["times"]["elapsed"] - float(proc_id)) < 0.01


if __name__ == "__main__":
    test_shutdown_signal(signal.SIGTERM)


