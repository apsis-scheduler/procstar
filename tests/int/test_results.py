from pathlib import Path

from procstar.testing.proc import run1

SCRIPTS_DIR = Path(__file__).parent / "scripts"

# -------------------------------------------------------------------------------


def test_general():
    proc = run1(
        {
            "argv": [
                str(SCRIPTS_DIR / "general"),
                "--allocate",
                "1073741824",  # 1 GB
                "--work",
                "0.25",
                # FIXME: Record start/stop/elapsed time.
                # "--sleep", "0.5",
                "--exit-code",
                "42",
            ],
        }
    )

    assert proc["status"]["exit_code"] == 42
    rusage = proc["rusage"]
    utime = rusage["utime"]
    assert 0.25 < utime < 0.5
    assert 1073741824 < rusage["maxrss"] < 1100000000
