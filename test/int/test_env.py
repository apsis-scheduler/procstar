from   base import run_spec
import os

#-------------------------------------------------------------------------------

def test_env_vars():
    res = run_spec("test_env_vars.json")["test"]
    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"]["text"].splitlines() == ["hello", "test value…!"]


def test_env_override():
    # The SHELL env var should be set already.
    assert "SHELL" in os.environ
    # This spec overrides it.
    res = run_spec("test_env_override.json")["test"]
    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"]["text"].splitlines() == ["hello", "/usr/bin/not-a-shell"]


def test_env_inherit_names():
    # The USER and SHELL env vars should already be set.
    user = os.environ["USER"]
    assert "SHELL" in os.environ
    res = run_spec("test_env_inherit_names.json")["test"]
    # Some vars not present, so printenv returns a failure code.
    assert res["status"]["exit_code"] > 0
    assert res["fds"]["stdout"]["text"].splitlines() == ["foobar", user]


