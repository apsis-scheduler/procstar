import pytest

from   procstar.testing.proc import run1, Errors

#-------------------------------------------------------------------------------

def test_bad_exe():
    """
    Tests error reporting on bad executable.
    """
    with pytest.raises(Errors) as exc_info:
        run1({"argv": ["/usr/bin/bogus"],})
    assert any( "No such file or directory" in e for e in exc_info.value.errors )


def test_bad_capture_path():
    """
    Tests error reporting for a bad capture file path.
    """
    with pytest.raises(Errors) as exc_info:
        run1({
            "argv": ["/usr/bin/echo", "Hello, world!"],
            "fds": [
                ["stdout", {"file": {"path": "/not/a/valid/path",}}],
                ["stderr", {"file": {"path": "/not/a/valid/path/either",}}],
            ]
        })
    assert any( "failed to set up fd 1" in e for e in exc_info.value.errors )
    assert any( "failed to set up fd 2" in e for e in exc_info.value.errors )


def test_bad_dup_fd():
    with pytest.raises(Errors) as exc_info:
        run1({
            "argv": ["/usr/bin/echo", "Hello, world!"],
            "fds": [
                ["stderr", "inherit"],
                # Hopefully this fd is not in use!
                ["stdout", {"dup": {"fd": 152}}],
            ]
        })
    assert any( "failed to set up fd 1" in e for e in exc_info.value.errors )


