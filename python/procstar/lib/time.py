import datetime

#-------------------------------------------------------------------------------

def now() -> datetime.datetime:
    """
    Returns the current time as an explicit UTC `datetime`.
    """
    return datetime.datetime.utcnow().replace(tzinfo=datetime.timezone.utc)


