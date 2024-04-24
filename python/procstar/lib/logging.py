import logging
from   logging import log, debug, info, warning, error, critical
import logging.handlers
import ora

#-------------------------------------------------------------------------------

def set_log_levels():
    # Quiet some noisy stuff.
    # logging.getLogger("asyncio").setLevel(logging.WARNING)
    logging.getLogger("urllib3.connectionpool").setLevel(logging.INFO)
    logging.getLogger("requests.packages.urllib3.connectionpool").setLevel(logging.INFO)
    # logging.getLogger("websockets.protocol").setLevel(logging.INFO)


#-------------------------------------------------------------------------------

class Formatter(logging.Formatter):

    def formatMessage(self, rec):
        time = ora.UNIX_EPOCH + rec.created
        level = rec.levelname
        return (
            f"{time:%Y-%m-%dT%.3C} {rec.name:24s} {level[0]} {rec.message}"
        )


def configure(*, level="WARNING"):
    if isinstance(level, str):
        level = getattr(logging, level.upper())
    logging.basicConfig(
        level=level,
        format="%(message)s",
        datefmt="%Y-%m-%dT%H:%M:%S",
    )
    # Root logger's formatter.
    logging.getLogger().handlers[0].formatter = Formatter()

    set_log_levels()


