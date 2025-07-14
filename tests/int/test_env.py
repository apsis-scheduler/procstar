import os
from pathlib import Path

from procstar.testing.proc import run_spec

SPECS_DIR = Path(__file__).parent / "specs"

# -------------------------------------------------------------------------------


def test_env_vars():
    res = run_spec(SPECS_DIR / "test_env_vars.json")["test"]
    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"]["text"].splitlines() == ["hello", "test value…!"]


def test_env_override():
    os.environ.setdefault("SHELL", "/usr/bin/sh")
    # This spec overrides it.
    res = run_spec(SPECS_DIR / "test_env_override.json")["test"]
    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"]["text"].splitlines() == ["hello", "/usr/bin/not-a-shell"]


def test_env_inherit_names():
    user = os.environ.setdefault("USER", "testuser")
    os.environ.setdefault("SHELL", "/usr/bin/sh")
    res = run_spec(SPECS_DIR / "test_env_inherit_names.json")["test"]
    # Some vars not present, so printenv returns a failure code.
    assert res["status"]["exit_code"] > 0
    assert res["fds"]["stdout"]["text"].splitlines() == ["foobar", user]
