import functools
import logging
import os
from   pathlib import Path
import shutil

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

@functools.cache
def get_procstar_path() -> Path:
    """
    Returns the path to the procstar binary.

    Uses the env var `PROCSTAR`, if set.
    """
    try:
        path = os.environ["PROCSTAR"]
    except KeyError:
        path = shutil.which("procstar")
        if path is None:
            path = Path(__file__).parents[3] / "target" / "debug" / "procstar"

    assert os.access(path, os.X_OK), f"missing exe {path}"
    logging.info(f"using {path}")
    return path


# Use a self-signed cert for localhost for integration tests.
TLS_CERT_PATH = Path(__file__).parent / "localhost.crt"
TLS_KEY_PATH = TLS_CERT_PATH.with_suffix(".key")

