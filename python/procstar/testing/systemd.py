from pathlib import Path

CGROUP_ROOT = Path("/sys/fs/cgroup")


def available() -> bool:
    return (CGROUP_ROOT / "cgroup.controllers").is_file()
