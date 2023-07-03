from   base import run1, SCRIPTS_DIR, run_spec

#-------------------------------------------------------------------------------

def test_echo_hello():
    res = run_spec("echo-hello")
    assert res["status"]


def test_stdout_stderr(tmp_path):
    stdout_path = tmp_path / "stdout"
    stderr_path = tmp_path / "stderr"
    res = run1({
        "argv": [SCRIPTS_DIR / "test.py", "--exit", "42"],
        "fds": [
            ["1", {"file": {"path": str(stdout_path)}}],
            ["2", {"file": {"path": str(stderr_path)}}],
        ]
    })

    assert res["status"] == {
        "status": 42 << 8,
        "exit_code": 42,
        "signum": None,
        "core_dump": False,
    }

    assert stdout_path.read_text() == (
        "message 0 to stdout\n"
        "message 2 to stdout\n"
    )
    assert stderr_path.read_text() == (
        "message 1 to stderr\n"
    )


def test_stdout_stderr_merge(tmp_path):
    stderr_path = tmp_path / "stderr"
    res = run1({
        "argv": [SCRIPTS_DIR / "test.py", "--exit", "42"],
        "fds": [
            ["stderr", {"file": {"path": str(stderr_path)}}],
            ["stdout", {"dup": {"fd": 2}}],
        ]
    })

    assert res["status"] == {
        "status": 42 << 8,
        "exit_code": 42,
        "signum": None,
        "core_dump": False,
    }

    assert stderr_path.read_text() == (
        "message 0 to stdout\n"
        "message 1 to stderr\n"
        "message 2 to stdout\n"
    )


