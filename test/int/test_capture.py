import base64
from   base import run1, SCRIPTS_DIR
import pytest

#-------------------------------------------------------------------------------

@pytest.mark.parametrize("mode", ["tempfile", "memory"])
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


@pytest.mark.parametrize("mode", ["tempfile", "memory"])
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


@pytest.mark.parametrize("mode", ["tempfile", "memory"])
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


@pytest.mark.parametrize("mode", ["tempfile", "memory"])
@pytest.mark.parametrize("format", ["text", "base64"])
def test_detached(mode, format):
    """
    Tests that detached outputs aren't included in results.
    """
    res = run1({
        "argv": ["/bin/echo", "Hello, world.", "How are you?"],
        "fds": [
            [
                "stdout", {
                    "capture": {
                        "mode": "memory",
                        "format": format,
                        "attached": False,
                    },
                },
            ],
        ],
    })

    assert res["status"]["status"] == 0
    assert res["fds"]["stdout"] is None


