import collections.abc
import json
import logging
import os
from   pathlib import Path
import subprocess
import tempfile

log = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

PROCSTAR_EXE = Path(__file__).parents[2] / "target/debug/procstar"
SPECS_DIR = Path(__file__).parent / "specs"
SCRIPTS_DIR = Path(__file__).parent / "scripts"


class TemporaryDirectory(tempfile.TemporaryDirectory):

    def __init__(self, *, prefix="procstar-test-tmp-", **kw_args):
        super().__init__(prefix=prefix, **kw_args)


    def __exit__(self, exc_type, exc, tb):
        if exc is None:
            super().__exit__(exc_type, exc, tb)
        else:
            log.warning(f"not cleaning up test tmpdir: {self.name}")
            self._finalizer.detach()



class Errors(Exception):

    def __init__(self, errors):
        super().__init__("\n".join(errors))
        self.errors = tuple(errors)



def _thunk_jso(o):
    if isinstance(o, Path):
        o = str(o)
    elif isinstance(o, bytes):
        o = o.encode()
    elif isinstance(o, str):
        pass
    elif isinstance(o, collections.abc.Mapping):
        o = { str(k): _thunk_jso(v) for k, v in o.items() }
    elif isinstance(o, collections.abc.Sequence):
        o = [ _thunk_jso(i) for i in o ]
    return o


def run(spec):
    spec = _thunk_jso(spec)
    with TemporaryDirectory() as tmp_dir:
        tmp_dir = Path(tmp_dir)
        spec_path = tmp_dir / "spec.json"
        with open(spec_path, "w") as out:
            json.dump(spec, out)
        output_path = tmp_dir / "out.json"
        subprocess.run(
            [
                str(PROCSTAR_EXE),
                "--output", output_path,
                spec_path,
            ],
            stdout=subprocess.PIPE,
            env=os.environ | {"RUST_BACKTRACE": "1"},
        )
        assert output_path.is_file()
        with open(output_path) as file:
            return json.load(file)


def run1(spec, *, proc_id="test"):
    """
    Runs a single process and returns its results, if it ran successfully.

    :param spec:
      Spec for a single process.
    :raise Errors:
      The process had errors.
    """
    proc = run({"specs": {proc_id: spec}})[proc_id]
    if proc["state"] == "error":
        raise Errors(proc["errors"])
    else:
        assert proc["state"] == "terminated"
        return proc


def run_spec(name):
    with open(SPECS_DIR / name) as file:
        spec = json.load(file)
    return run(spec)


