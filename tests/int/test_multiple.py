from   base import run, SCRIPTS_DIR
import sys

#-------------------------------------------------------------------------------

def test_multiple():
    procs = run({
        "procs": {
            f"{i}": {
                "argv": ["/bin/echo", f"This is process #{i}."],
                "fds": [
                    ["stdout", {"capture": {"mode": "memory"}}],
                ],
            }
            for i in range(8)
        },
    })

    assert len(procs) == 8
    for i, proc in procs.items():
        assert proc["status"]["status"] == 0
        assert proc["fds"]["stdout"]["text"] == f"This is process #{i}.\n"


def test_subprocs1():
    """
    Runs a bunch of scripts, each of which has a tree of subprocs.
    """
    procs = run({
        "procs": {
            f"{i}": {
                "argv": [sys.executable, str(SCRIPTS_DIR / "subprocs1.py")],
                "fds": [
                    ["stdout", {"capture": {"mode": "memory"}}],
                ],
            }
            for i in range(8)
        },
    })

    assert len(procs) == 8
    for proc in procs.values():
        assert proc["status"]["status"] == 0
        lines = proc["fds"]["stdout"]["text"].splitlines()
        forked = { int(l[8 :]) for l in lines if l.startswith("forked: ") }
        waited = { int(l[8 :]) for l in lines if l.startswith("waited: ") }
        assert forked == waited
        assert lines[-1] == "done"


def test_concurrent_print():
    """
    Runs several scripts that produce large amounts of output, and collects it.
    """
    procs = run({
        "procs": {
            f"{i}": {
                "argv": [
                    str(SCRIPTS_DIR / "general"),
                    "--print", f"{1 << i}x{(1 << (22 - i)) + 1}",
                ],
                "fds": [
                    ["stdout", {"capture": {"mode": "memory"}}],
                ],
            }
            for i in range(8)
        },
    })

    for i, proc in procs.items():
        i = int(i)
        lines = proc["fds"]["stdout"]["text"].splitlines()
        assert len(lines) == 1 << i
        expected = "x" * ((1 << (22 - i)) + 1)
        assert all( l == expected for l in lines )


