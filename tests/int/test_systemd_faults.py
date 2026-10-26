"""
Fault injection for the cleanup of a run's systemd units.

These run the agent against `examples/fake_systemd` on a private bus instead of
the real user manager, so that the error paths in cleanup can be exercised:
a stop job that fails, never finishes, or fails only once; a scope that stays
failed; a reset that errors; a property read that hangs.  Each case asserts
that cleanup stays bounded, says so when it did not finish, and still reports
the evidence it managed to collect.

Everything the agent is told here is a lie, so a bug can hang it.  The agent
therefore runs in its own process group under a deadline, and is killed by
group if it overruns.
"""

import json
import os
import selectors
import signal
import subprocess
import sys
import time

import pytest

from procstar.testing import get_procstar_path
from procstar.testing import systemd

# Cleanup is bounded by a 90 s budget inside the agent.  Allow for that plus
# startup, and fail rather than hang if it is exceeded.
DEADLINE = 150
# Starting the bus and the stand-in should be immediate.
STARTUP_TIMEOUT = 30

FAKE_SYSTEMD = get_procstar_path().parent / "examples" / "fake_systemd"

requires_fake_systemd = pytest.mark.skipif(
    condition=not FAKE_SYSTEMD.is_file(),
    reason=f"{FAKE_SYSTEMD} not built; run `cargo build --example fake_systemd`",
)
requires_cgroups = pytest.mark.skipif(
    condition=not systemd.available(), reason="cgroups v2 unavailable"
)


def _readline(pipe, timeout):
    """
    Reads one line, or raises if nothing arrives within `timeout`.
    """
    selector = selectors.DefaultSelector()
    selector.register(pipe, selectors.EVENT_READ)
    try:
        if not selector.select(timeout):
            raise AssertionError(f"no output within {timeout} s")
        return pipe.readline().strip()
    finally:
        selector.close()


def _kill_group(proc):
    """
    Kills `proc` and anything it started.
    """
    if proc is None or proc.poll() is not None:
        return
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        proc.kill()
    proc.wait()


class FakeSystemd:
    """
    A private bus with a stand-in systemd on it.
    """

    def __init__(self, mode):
        self.mode = mode
        self.env = None
        self.__bus = None
        self.__service = None

    def __enter__(self):
        try:
            self.__bus = subprocess.Popen(
                ["dbus-daemon", "--session", "--print-address", "--nofork"],
                stdout=subprocess.PIPE,
                text=True,
                start_new_session=True,
            )
            address = _readline(self.__bus.stdout, STARTUP_TIMEOUT)
            assert address, "dbus-daemon printed no address"
            self.env = {**os.environ, "DBUS_SESSION_BUS_ADDRESS": address}
            self.__service = subprocess.Popen(
                [str(FAKE_SYSTEMD)],
                env={**self.env, "FAKE_SYSTEMD_MODE": self.mode},
                stdout=subprocess.PIPE,
                text=True,
                start_new_session=True,
            )
            ready = _readline(self.__service.stdout, STARTUP_TIMEOUT)
            assert ready == "ready", f"fake systemd said {ready!r}"
            return self
        except BaseException:
            # __exit__ does not run when __enter__ fails.
            self.__exit__()
            raise

    def __exit__(self, *_exc):
        _kill_group(self.__service)
        _kill_group(self.__bus)

    def run(self, spec):
        """
        Runs `spec` with the agent pointed at this bus, under a deadline.
        """
        # The agent goes in its own process group, so that a hang can be killed
        # along with the wrapper that started it.
        assert self.env is not None, "not started"
        script = (
            "import json, sys;"
            "from procstar.testing.proc import run1;"
            "print('RESULT:' + json.dumps(run1(json.loads(sys.argv[1]))))"
        )
        proc = subprocess.Popen(
            [sys.executable, "-c", script, json.dumps(spec)],
            env={**self.env, "PROCSTAR": str(get_procstar_path())},
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
        try:
            out, err = proc.communicate(timeout=DEADLINE)
        except subprocess.TimeoutExpired:
            _kill_group(proc)
            raise AssertionError(f"{self.mode}: no result within {DEADLINE} s") from None
        result = next(
            (line for line in out.splitlines() if line.startswith("RESULT:")),
            None,
        )
        assert result is not None, f"no result: rc={proc.returncode} err={err[-2000:]}"
        return json.loads(result[len("RESULT:") :])


SPEC = {
    "argv": ["/usr/bin/true"],
    "systemd_properties": {"slice": {"memory_accounting": True, "memory_max": 10**9}},
}


@requires_fake_systemd
@requires_cgroups
@pytest.mark.parametrize(
    ("mode", "scope_result"),
    [
        # The scope unloaded itself after a clean stop: nothing to reset.
        ("ok", None),
        # Reported gone, which is the same thing by another route.
        ("scope-gone", None),
        # Failed scope, reset works: cleanup completes and keeps the verdict.
        ("scope-failed", "oom-kill"),
        # The first stop fails; the retry gets there.
        ("stop-fails-once", None),
    ],
)
def test_cleanup_succeeds(mode, scope_result):
    with FakeSystemd(mode) as fake:
        start = time.monotonic()
        res = fake.run(SPEC)
    assert time.monotonic() - start < 60
    assert res["status"]["exit_code"] == 0
    assert res["cleanup_incomplete"] is False, f"{mode}: wrongly reported incomplete"
    assert res["scope_result"] == scope_result
    assert res["systemd_enabled"] is True


@requires_fake_systemd
@requires_cgroups
@pytest.mark.parametrize(
    "mode",
    [
        # systemd reports every stop job as failed.
        "stop-job-fails",
        # The scope stays failed because the reset errors.
        "reset-fails",
        # A property read never replies, so the diagnostic deadline expires.
        "get-unit-hangs",
        # The stop job never finishes, so the stop deadline expires.
        "stop-never-completes",
        # Stopping the slice never returns, so the whole budget expires.
        "slice-stop-hangs",
    ],
)
def test_cleanup_reports_itself_incomplete(mode):
    """
    Cleanup cannot succeed in these cases.  It must say so rather than report
    success, and it must still finish inside its budget.
    """
    with FakeSystemd(mode) as fake:
        start = time.monotonic()
        res = fake.run(SPEC)
    elapsed = time.monotonic() - start
    assert res["cleanup_incomplete"] is True, f"{mode}: cleanup wrongly reported complete"
    # The agent's own budget is 90 s; it must not exceed it by much.
    assert elapsed < 120, f"{mode}: cleanup took {elapsed:.0f} s"
    # The process itself ran, and a cleanup fault does not change its status.
    assert res["status"]["exit_code"] == 0


@requires_fake_systemd
@requires_cgroups
def test_evidence_survives_a_failed_reset():
    """
    A failed reset must not cost us what cleanup already collected: the scope's
    verdict and the cgroup accounting are still reported.
    """
    with FakeSystemd("reset-fails") as fake:
        res = fake.run(SPEC)
    assert res["cleanup_incomplete"] is True
    assert res["scope_result"] == "oom-kill"
    accounting = res["cgroup_accounting"]
    assert accounting is not None, "accounting was discarded along with the failed cleanup"
    assert accounting["memory"]["events"] is not None


@requires_fake_systemd
@requires_cgroups
def test_evidence_survives_the_budget_expiring():
    """
    The scope result and the accounting are collected before the slice is
    stopped.  If that stop then hangs until the cleanup budget expires, what
    was already collected must still be reported.
    """
    with FakeSystemd("slice-stop-hangs") as fake:
        res = fake.run(SPEC)
    assert res["cleanup_incomplete"] is True
    assert res["scope_result"] == "oom-kill"
    assert res["cgroup_accounting"] is not None


@requires_fake_systemd
@requires_cgroups
def test_a_later_success_does_not_erase_a_failure():
    """
    systemd reports `success` for a scope once it has been reset, so a retry
    looks at a scope that no longer admits it was OOM-killed.  The verdict
    recorded on the first look has to survive that.
    """
    with FakeSystemd("retry-after-failed-scope") as fake:
        res = fake.run(SPEC)
    assert res["scope_result"] == "oom-kill", "the retry overwrote the recorded failure"
