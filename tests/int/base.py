import collections.abc
import json
import os
from   pathlib import Path
import subprocess
import tempfile

#-------------------------------------------------------------------------------

PROCSTAR_EXE = Path(__file__).parents[2] / "target/debug/procstar"
SPECS_DIR = Path(__file__).parent / "specs"
SCRIPTS_DIR = Path(__file__).parent / "scripts"


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


def run(specs):
    specs = _thunk_jso(specs)
    with tempfile.TemporaryDirectory() as tmp_dir:
        tmp_dir = Path(tmp_dir)
        spec_path = tmp_dir / "spec.json"
        with open(spec_path, "w") as out:
            json.dump({"procs": specs}, out)
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


def run1(spec):
    """
    Runs a single process and returns its results, if it ran successfully.

    :raise Errors:
      The process had errors.
    """
    proc = run({"test": spec})["test"]
    if len(proc["errors"]) == 0:
        return proc
    else:
        raise Errors(proc["errors"])


def run_spec(name):
    with open((SPECS_DIR / name).with_suffix(".json")) as file:
        spec = json.load(file)
    return run1(spec)


