"""
Fault injection for the cleanup of a run's systemd units.

These run the agent against `examples/fake_systemd` on a private bus instead of
the real user manager, so that the error paths in cleanup can be exercised:
a stop job that fails or never finishes, a scope that stays failed, a reset that
errors, a property read that hangs.  Each case asserts that cleanup stays
bounded, says so when it did not finish, and still reports the evidence it
managed to collect.
"""

import json
import os
import subprocess
import sys
import time

import pytest

from procstar.testing import get_procstar_path
from procstar.testing import systemd

# Cleanup is bounded by a 90 s budget inside the agent; nothing here may take
# materially longer, and a hang must fail the test rather than block it.
DEADLINE = 150

FAKE_SYSTEMD = get_procstar_path().parent / "examples" / "fake_systemd"

requires_fake_systemd = pytest.mark.skipif(
    condition=not FAKE_SYSTEMD.is_file(),
    reason=f"{FAKE_SYSTEMD} not built; run `cargo build --example fake_systemd`",
)


class FakeSystemd:
    """
    A private bus with a stand-in systemd on it.
    """

    def __init__(self, mode):
        self.__bus = None
        self.__service = None
        self.mode = mode

    def __enter__(self):
        self.__bus = subprocess.Popen(
            ["dbus-daemon", "--session", "--print-address", "--nofork"],
            stdout=subprocess.PIPE,
            text=True,
        )
        assert self.__bus.stdout is not None
        address = self.__bus.stdout.readline().strip()
        assert address, "dbus-daemon printed no address"
        self.env = {**os.environ, "DBUS_SESSION_BUS_ADDRESS": address}
        self.__service = subprocess.Popen(
            [str(FAKE_SYSTEMD)],
            env={**self.env, "FAKE_SYSTEMD_MODE": self.mode},
            stdout=subprocess.PIPE,
            text=True,
        )
        assert self.__service.stdout is not None
        assert self.__service.stdout.readline().strip() == "ready", "fake systemd did not start"
        return self

    def __exit__(self, *_exc):
        for proc in (self.__service, self.__bus):
            if proc is not None:
                proc.kill()
                proc.wait()

    def run(self, spec):
        """
        Runs `spec` with the agent pointed at this bus, under a hard deadline.
        """
        # Run in a subprocess so that a hang is killable rather than fatal.
        script = (
            "import json, sys;"
            "from procstar.testing.proc import run1;"
            "print('RESULT:' + json.dumps(run1(json.loads(sys.argv[1]))))"
        )
        done = subprocess.run(
            [sys.executable, "-c", script, json.dumps(spec)],
            env={**self.env, "PROCSTAR": str(get_procstar_path())},
            capture_output=True,
            text=True,
            timeout=DEADLINE,
        )
        line = next(
            (l for l in done.stdout.splitlines() if l.startswith("RESULT:")),
            None,
        )
        assert line is not None, f"no result: rc={done.returncode} err={done.stderr[-2000:]}"
        return json.loads(line[len("RESULT:") :])


SPEC = {
    "argv": ["/usr/bin/true"],
    "systemd_properties": {"slice": {"memory_accounting": True, "memory_max": 10**9}},
}


@requires_fake_systemd
@pytest.mark.skipif(condition=not systemd.available(), reason="cgroups v2 unavailable")
@pytest.mark.parametrize(
    ("mode", "incomplete", "scope_result"),
    [
        # The scope unloaded itself after a clean stop: nothing to reset.
        ("ok", False, None),
        # Reported gone, which is the same thing by another route.
        ("scope-gone", False, None),
        # Failed scope, reset works: cleanup completes and keeps the verdict.
        ("scope-failed", False, "oom-kill"),
    ],
)
def test_cleanup_succeeds(mode, incomplete, scope_result):
    with FakeSystemd(mode) as fake:
        start = time.monotonic()
        res = fake.run(SPEC)
    assert time.monotonic() - start < 30
    assert res["status"]["exit_code"] == 0
    assert res["cleanup_incomplete"] is incomplete
    assert res["scope_result"] == scope_result
    assert res["systemd_enabled"] is True


@requires_fake_systemd
@pytest.mark.skipif(condition=not systemd.available(), reason="cgroups v2 unavailable")
@pytest.mark.parametrize(
    "mode",
    [
        # systemd reports the stop job as failed.
        "stop-job-fails",
        # The scope stays failed because the reset errors.
        "reset-fails",
        # A property read never replies, so the diagnostic deadline expires.
        "get-unit-hangs",
        # The stop job never finishes, so the stop deadline expires.
        "stop-never-completes",
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
    # The process itself ran and its status is unaffected by the cleanup fault.
    assert res["status"]["exit_code"] == 0


@requires_fake_systemd
@pytest.mark.skipif(condition=not systemd.available(), reason="cgroups v2 unavailable")
def test_evidence_survives_a_cleanup_fault():
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
