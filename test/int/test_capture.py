import base64
from   pathlib import Path
import pytest

from   procstar.spec import Proc
from   procstar.testing.proc import run1

SPECS_DIR = Path(__file__).parent / "specs"
SCRIPTS_DIR = Path(__file__).parent / "scripts"

#-------------------------------------------------------------------------------

@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
@pytest.mark.parametrize("format", ["text", "base64"])
def test_echo(mode, format):
    """
    Tests basic capture of stdout.
    """
    res = run1({
        "argv": ["/bin/echo", "Hello, world.", "How are you?"],
        "fds": [
            [
                "stdout", {
                    "capture": {
                        "mode": mode,
                        "format": format,
                    }
                }
            ],
        ]
    })

    assert res["status"]["status"] == 0

    stdout = res["fds"]["stdout"]
    text = "Hello, world. How are you?\n"
    if mode == "text":
        assert stdout["text"] == text
    elif mode == "base64":
        assert stdout["encoding"] == "base64"
        assert stdout["text"] == base64.b64encode(text.encode())


@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
def test_interleaved(mode):
    """
    Tests interleaved stdout and stderr.
    """
    exe = SCRIPTS_DIR / "interleaved.py"
    assert exe.exists()

    res = run1({
        "argv": [
            str(exe),
        ],
        "fds": [
            [
                "stdout", {
                    "capture": {
                        "mode": mode,
                        "format": "base64",
                    }
                },
            ],
            [
                "stderr", {
                    "capture": {
                        "mode": mode,
                        "format": "base64",
                    }
                },
            ]
        ]
    })

    assert res["status"]["status"] == 0

    out = base64.standard_b64decode(res["fds"]["stdout"]["data"])
    err = base64.standard_b64decode(res["fds"]["stderr"]["data"])
    assert out == b"".join( bytes([i]) * i for i in range(256) if i % 3 != 0 )
    assert err == b"".join( bytes([i]) * i for i in range(256) if i % 3 == 0 )


@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
def test_utf8_sanitize(mode):
    """
    Tests capturing invalid UTF-8 as text.
    """
    res = run1({
        "argv": [
            "/usr/bin/printf",
            "abc\200\200def",
        ],
        "fds": [
            ["stdout", {"capture": {"mode": mode}}],
        ],
    })

    assert res["status"]["status"] == 0

    out = res["fds"]["stdout"]["text"]
    assert len(out) == 8
    assert out[: 3] == "abc"
    assert out[-3 :] == "def"


@pytest.mark.parametrize("mode", Proc.Fd.Capture.MODES)
@pytest.mark.parametrize("attached", [None, True, False])
def test_detached(mode, attached):
    """
    Tests that captured outputs are included in results iff attached.
    """
    res = run1({
        "argv": ["/bin/echo", "Hello, world.", "How are you?"],
        "fds": [
            [
                "stdout", {
                    "capture": {
                        "mode": mode,
                        "format": "text",
                        **({} if attached is None else {"attached": attached}),
                    },
                },
            ],
        ],
    })

    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"] == (
        {"text": "Hello, world. How are you?\n"}
        if attached in (True, None)
        else None
    )


