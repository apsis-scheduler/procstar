import functools
import json
import os
from   pathlib import Path
import shutil
import sys
import uuid

#-------------------------------------------------------------------------------

@functools.cache
def get_bash_path():
    path = shutil.which("bash")
    if path is None:
        raise RuntimeError("no bash in PATH") from None
    path = Path(path.strip())
    assert path.name == "bash"
    assert os.access(path, os.X_OK)
    return path


#-------------------------------------------------------------------------------

class Proc:

    class Env:

        MINIMUM_ENV_VARS = ("HOME", "SHELL", "USER")

        def __init__(self, *, inherit=True, vars={}):
            self.__inherit = (
                inherit if isinstance(inherit, bool)
                else tuple( str(n) for n in inherit )
            )
            self.__vars = {
                str(n): None if v is None else str(v)
                for n, v in vars.items()
            }


        def to_jso(self):
            return {
                "inherit": self.__inherit,
                "vars": dict(self.__vars),
            }



    class Fd:

        ALIASES = {
            "0": "stdin",
            "1": "stdout",
            "2": "stderr",
        }

        OPEN_FLAGS = {
            "Default",
            "Read",
            "Write",
            "Create",
            "Replace",
            "CreateAppend",
            "Append",
            "ReadWrite",
        }

        def normalize(fd):
            """
            Normalizes a fd number or name.
            """
            fd = str(fd)
            if fd in Proc.Fd.ALIASES.values():
                return fd
            try:
                return Proc.Fd.ALIASES[fd]
            except KeyError:
                try:
                    n = int(fd)
                except ValueError:
                    raise ValueError(f"invalid fd: {fd}") from None
                else:
                    if n < 0:
                        raise ValueError(f"invalid fd: {fd}") from None
                return str(n)


        class Inherit:

            def to_jso(self):
                return {
                    "inherit": {},
                }



        class Close:

            def to_jso(self):
                return {
                    "close": {},
                }



        class Null:

            def __init__(self, flags="Default"):
                if flags not in Proc.Fd.OPEN_FLAGS:
                    raise ValueError(f"unknown flags: {flags}")
                self.__flags = flags


            def to_jso(self):
                return {
                    "null": {
                        "flags": self.__flags,
                    }
                }



        class File:

            def __init__(self, path, flags="Default", mode=0o666):
                if flags not in Proc.Fd.OPEN_FLAGS:
                    raise ValueError(f"unknown flags: {flags}")

                self.__path = str(path)
                self.__flags = str(flags)
                self.__mode = int(mode)


            def to_jso(self):
                return {
                    "file": {
                        "path"  : self.__path,
                        "flags" : self.__flags,
                        "mode"  : self.__mode,
                    }
                }



        class Dup:

            def __init__(self, fd):
                fd = int(fd)
                if not 0 <= fd:
                    raise ValueError(f"bad fd: {fd}")
                self.__fd = fd


            def to_jso(self):
                return {
                    "dup": {
                        "fd": self.__fd,
                    }
                }



        class Capture:

            MODES = {"tempfile", "memory"}
            FORMATS = {"text", "base64"}

            def __init__(self, mode, format):
                if mode not in self.MODES:
                    raise ValueError(f"bad mode: {mode}")
                if format not in self.FORMATS:
                    raise ValueError(f"bad format: {format}")
                self.__mode = mode
                self.__format = format


            def to_jso(self):
                return {
                    "capture": {
                        "mode": self.__mode,
                        "format": self.__format,
                    }
                }




    def __init__(self, argv, *, env=Env(), fds={}):
        self.__argv = tuple( str(a) for a in argv )
        self.__env  = env
        self.__fds = dict(fds)


    def to_jso(self):
        return {
            "argv"  : self.__argv,
            "env"   : self.__env.to_jso(),
            "fds"   : [ (n, f.to_jso()) for n, f in self.__fds.items() ],
        }



def make_proc(what, /, *, env_vars={}, fds={}):
    """
    Constructs a process spec.

    Defaults to maximum isolation:
    - all fds other than stdin/out/err closed
    - minimal env vars inherited

    :param what:
      A string shell command, or an argv iterable.
    :param env_vars:
      Mapping of env vars to set; an env var with value `True` is inherited.
    :param fds:
      Mapping from fd to file descripor spec.  Unless specified, stdin is
      attached to /dev/null; stdout and stderr are each captured to memory as
      text, and all other fds are closed.
    """
    if isinstance(what, str):
        argv = [get_bash_path(), "-c", what]
    else:
        argv = [ str(a) for a in what ]

    fds = dict(fds)
    fds  = { Proc.Fd.normalize(n): s for n, s in fds.items() }
    if "stdin" not in fds:
        fds["stdin"] = Proc.Fd.Null()
    if "stdout" not in fds:
        fds["stdout"] = Proc.Fd.Capture("memory", "text")
    if "stderr" not in fds:
        fds["stderr"] = Proc.Fd.Capture("memory", "text")
    # FIXME: Close the rest.

    env = Proc.Env(inherit=Proc.Env.MINIMUM_ENV_VARS, vars=env_vars)

    return Proc(argv, fds=fds, env=env)


def to_jso(*unnamed, **by_name):
    specs = {
        str(uuid.uuid4()): s.to_jso()
        for s in unnamed
    } | {
        i: s.to_jso()
        for i, s in by_name.items()
    }
    return {"specs": specs}


