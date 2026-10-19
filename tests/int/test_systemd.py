from pathlib import Path

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


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_oom():
    res = run1(OOM_SPEC)
    assert res["status"]["signal"] == "SIGKILL"
    assert res["scope_result"] == "oom-kill"
    memory = res["cgroup_accounting"]["memory"]
    assert memory["max"] == MEM_MAX
    # The kill is counted hierarchically (it happened in the scope); the slice,
    # which carries the limit, records the exhaustion locally.
    assert memory["events"]["oom_kill"] >= 1
    assert memory["events_local"]["oom"] >= 1
    assert memory["events_local"]["max"] >= 1


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_child_oom_parent_exits_zero():
    """
    A child is OOM-killed while the parent would exit 0.  systemd's OOMPolicy
    stops the whole scope, and the result identifies the OOM.
    """
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
