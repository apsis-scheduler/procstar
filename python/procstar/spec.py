import functools
import os
from   pathlib import Path
import shutil
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
                return {}



        class Close:

            def to_jso(self):
                return {}



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

            MODES           = {"tempfile", "memory"}
            ENCODINGS       = {None, "utf-8"}

            def __init__(self, mode, encoding, attached=True):
                if mode not in self.MODES:
                    raise ValueError(f"bad mode: {mode}")
                if encoding not in self.ENCODINGS:
                    raise ValueError(f"bad encoding: {encoding}")

                self.__mode         = mode
                self.__encoding     = encoding
                self.__attached     = bool(attached)


            def to_jso(self):
                return {
                    "capture": {
                        "mode"          : self.__mode,
                        "encoding"      : self.__encoding,
                        "attached"      : self.__attached,
                    }
                }



        class PipeWrite:

            def to_jso(self):
                return {}



        class PipeRead:

            def __init__(self, proc_id, fd):
                self.__proc_id = proc_id
                self.__fd = fd


            def to_jso(self):
                return {
                    "proc_id": self.__proc_id,
                    "fd": self.__fd,
                }

    class SystemdProperties:
        class Scope:
            def __init__(
                self,
                oom_policy: str | None = None,
                runtime_max_usec: int | None = None,
                runtime_randomized_extra_usec: int | None = None,
            ):
                self.__oom_policy = oom_policy
                self.__runtime_max_usec = runtime_max_usec
                self.__runtime_randomized_usec = runtime_randomized_extra_usec

            def to_jso(self):
                return {
                    "oom_policy": self.__oom_policy,
                    "runtime_max_usec": self.__runtime_max_usec,
                    "runtime_randomized_extra_usec": self.__runtime_randomized_usec
                }

        class Slice:
            def __init__(
                self,
                memory_accounting: bool | None = None,
                memory_min: int | None = None,
                memory_low: int | None = None,
                memory_high: int | None = None,
                memory_max: int | None = None,
                memory_swap_max: int | None = None,
                tasks_accounting: bool | None = None,
                tasks_max: int | None = None,
            ):
                self.__memory_accounting = memory_accounting
                self.__memory_min = memory_min
                self.__memory_low = memory_low
                self.__memory_high = memory_high
                self.__memory_max = memory_max
                self.__memory_swap_max = memory_swap_max
                self.__tasks_accounting = tasks_accounting
                self.__tasks_max = tasks_max

            def to_jso(self):
                return {
                    "memory_accounting": self.__memory_accounting,
                    "memory_min": self.__memory_min,
                    "memory_low": self.__memory_low,
                    "memory_high": self.__memory_high,
                    "memory_max": self.__memory_max,
                    "memory_swap_max": self.__memory_swap_max,
                    "tasks_accounting": self.__tasks_accounting,
                    "tasks_max": self.__tasks_max,
                }

        def __init__(self, scope=Scope(), slice=Slice()):
            self.__scope = scope
            self.__slice = slice

        def to_jso(self):
            return {"scope": self.__scope.to_jso(), "slice": self.__slice.to_jso()}


    def __init__(
        self, argv, *, env=Env(), fds={}, systemd_properties=None,
    ):
        self.__argv = tuple( str(a) for a in argv )
        self.__env  = env
        self.__fds = dict(fds)
        self.__systemd_properties = systemd_properties


    def to_jso(self):
        return {
            "argv": self.__argv,
            "env": self.__env.to_jso(),
            "fds": [(n, f.to_jso()) for n, f in self.__fds.items()],
        } | (
            {}
            if self.__systemd_properties is None
            # only insert systemd_properties field if its explicitly set to remain
            # compatible with agents that don't yet support it
            else {"systemd_properties": self.__systemd_properties.to_jso()}
        )



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
        fds["stdout"] = Proc.Fd.Capture("memory", "utf-8")
    if "stderr" not in fds:
        fds["stderr"] = Proc.Fd.Capture("memory", "utf-8")
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


