def elide(string, length, ellipsis="…", pos=1.0):
    """
    Elides characters if necessary to fit `string` in `length` characters.

    :param ellipsis:
      The string to indicate the elided characters.
    :param pos:
      The position of the ellipsis: 0 for left, 1 for right.
    """
    if length < len(ellipsis):
        raise ValueError("max_length less than ellipsis length")
    if not (0 <= pos <= 1):
        raise IndexError(f"bad pos: {pos}")

    if not isinstance(string, bytes):
        string = str(string)
    if len(string) <= length:
        return string

    keep    = length - len(ellipsis)
    left    = int(round(pos * keep))
    right   = keep - left

    result = ellipsis
    if left > 0:
        result = string[: left] + result
    if right > 0:
        result += string[-right :]
    return result


