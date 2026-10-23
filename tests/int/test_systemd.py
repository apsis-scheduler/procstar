from pathlib import Path
import subprocess
import time

import pytest

from procstar.spec import Proc
from procstar.testing.proc import run1
from procstar.testing import systemd

SCRIPTS_DIR = Path(__file__).parent / "scripts"

MEM_MAX = 268435456  # 256 MiB
OOM_SPEC = {
    "argv": [str(SCRIPTS_DIR / "general"), "--allocate", str(2 * MEM_MAX), "--sleep", "10"],
    "systemd_properties": {"slice": {"memory_max": MEM_MAX, "memory_swap_max": 0}},
}


def _procstar_units():
    """
    Returns the names of all loaded procstar transient units in the user manager.
    """
    out = subprocess.run(
        ["systemctl", "--user", "list-units", "--all", "--no-legend", "--plain", "procstar*"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return {line.split()[0] for line in out.splitlines() if line.strip()}


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_accounting():
    res = run1(
        {
            "argv": ["/usr/bin/true"],
            "systemd_properties": {"slice": {"memory_accounting": True, "tasks_accounting": True}},
        }
    )
    accounting = res["cgroup_accounting"]
    assert accounting["pids"]["peak"] == 1
    assert accounting["cpu_stat"]["usage_usec"] > 0
    memory = accounting["memory"]
    assert memory["peak"] > 0
    assert memory["max"] == "max"
    assert memory["events"]["oom_kill"] == 0
    assert memory["events_local"]["max"] == 0
    assert res["systemd_enabled"] is True
    assert res["scope_result"] is None  # unloaded after a clean stop
    assert res["cleanup_incomplete"] is False


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_oom():
    before = _procstar_units()
    res = run1(OOM_SPEC)
    assert res["status"]["signal"] == "SIGKILL"
    assert res["scope_result"] == "oom-kill"
    assert res["cleanup_incomplete"] is False
    memory = res["cgroup_accounting"]["memory"]
    assert memory["max"] == MEM_MAX
    # The kill is counted hierarchically (it happened in the scope); the slice,
    # which carries the limit, records the exhaustion locally.
    assert memory["events"]["oom_kill"] >= 1
    assert memory["events_local"]["oom"] >= 1
    assert memory["events_local"]["max"] >= 1
    # The failed scope was reset, so no transient unit is left behind.
    assert _procstar_units() == before


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_child_oom_parent_exits_zero():
    """
    A child is OOM-killed while the parent would exit 0.  systemd's OOMPolicy
    stops the whole scope, and the result identifies the OOM.
    """
    before = _procstar_units()
    child = f"{SCRIPTS_DIR / 'general'} --allocate {2 * MEM_MAX} --sleep 10 || true; exit 0"
    res = run1(
        {
            "argv": ["/usr/bin/bash", "-c", child],
            "systemd_properties": {"slice": {"memory_max": MEM_MAX, "memory_swap_max": 0}},
        }
    )
    assert res["scope_result"] == "oom-kill"
    assert res["cgroup_accounting"]["memory"]["events"]["oom_kill"] >= 1
    assert res["cgroup_accounting"]["memory"]["events_local"]["oom"] >= 1
    assert res["status"]["exit_code"] != 0 or res["status"]["signal"] is not None
    assert _procstar_units() == before


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_parent_exits_with_lingering_child():
    """
    The primary process exits while a child keeps running; finalization must
    complete promptly and terminate the child with the scope.
    """
    before = _procstar_units()
    start = time.monotonic()
    res = run1(
        {
            "argv": ["/usr/bin/bash", "-c", "sleep 300 & exit 0"],
            "systemd_properties": {"slice": {"memory_accounting": True}},
        }
    )
    assert time.monotonic() - start < 30
    assert res["status"]["exit_code"] == 0
    assert res["cleanup_incomplete"] is False
    assert _procstar_units() == before


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_repeated_oom_leaves_no_units():
    before = _procstar_units()
    for _ in range(3):
        res = run1(OOM_SPEC)
        assert res["scope_result"] == "oom-kill"
    assert _procstar_units() == before


@pytest.mark.skipif(condition=systemd.available(), reason="systemd available")
def test_unavailable():
    res = run1(
        {
            "argv": ["/usr/bin/true"],
        }
    )
    assert res["cgroup_accounting"] is None
    assert res["systemd_enabled"] is False
    assert res["scope_result"] is None


def test_backward_compatibility():
    """
    Double check that a spec that doesn't mention systemd is still compatible with
    old agents that don't yet have systemd features.
    """
    assert "systemd_properties" not in Proc(["/usr/bin/true"]).to_jso()
    assert (
        "systemd_properties"
        in Proc(["/usr/bin/true"], systemd_properties=Proc.SystemdProperties()).to_jso()
    )
