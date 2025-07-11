import functools
import logging
import os
from   pathlib import Path
import subprocess

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

@functools.cache
def get_procstar_path() -> Path:
    """
    Returns the path to the procstar binary.

    Uses the env var `PROCSTAR`, if set.
    Otherwise builds the binary and uses the debug version.
    """
    try:
        path = Path(os.environ["PROCSTAR"])
    except KeyError:
        # Build the binary to ensure it's up to date
        project_root = Path(__file__).parents[3]
        path = project_root / "target" / "debug" / "procstar"

        logger.info("building procstar binary...")
        subprocess.run(
            ["cargo", "build"],
            cwd=project_root,
            check=True,
            capture_output=True
        )

        if not path.exists():
            raise FileNotFoundError(f"debug binary not found after build: {path}")

    assert os.access(path, os.X_OK), f"missing exe {path}"
    logger.info(f"using {path}")
    return path


# Use a self-signed cert for localhost for integration tests.
TLS_CERT_PATH = Path(__file__).parent / "localhost.crt"
TLS_KEY_PATH = TLS_CERT_PATH.with_suffix(".key")

