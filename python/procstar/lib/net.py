def make_netloc(loc):
    """
    Constructs the "netloc" part of a URL from a host, port pair.
    """
    host, port = loc
    assert host is not None
    netloc = f"[{host}]" if ":" in host else host
    if port is not None:
        netloc += f":{port}"
    return netloc


