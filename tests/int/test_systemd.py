from pathlib import Path

import pytest

from procstar.spec import Proc
from procstar.testing.proc import run1
from procstar.testing import systemd

SCRIPTS_DIR = Path(__file__).parent / "scripts"


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
    assert accounting["memory"]["peak"] > 0


@pytest.mark.skipif(condition=not systemd.available(), reason="systemd unavailable")
def test_oom():
    res = run1(
        {
            "argv": [
                str(SCRIPTS_DIR / "general"),
                "--allocate",
                "536870912",  # 500 MB
                "--sleep",
                "10",
            ],
            "systemd_properties": {"slice": {"memory_max": 268435456, "memory_swap_max": 0}},
        }
    )
    assert res["status"]["signal"] == "SIGKILL"


@pytest.mark.skipif(condition=systemd.available(), reason="systemd available")
def test_unavailable():
    res = run1(
        {
            "argv": ["/usr/bin/true"],
        }
    )
    assert res["cgroup_accounting"] is None


def test_backward_compatibility():
    """
    Double check that a spec that doesn't mention systemd is still compatible with
    old agents that don't yet have systemd features.
    """
    assert not "systemd_properties" in Proc(["/usr/bin/true"]).to_jso()
    assert (
        "systemd_properties"
        in Proc(["/usr/bin/true"], systemd_properties=Proc.SystemdProperties()).to_jso()
    )
