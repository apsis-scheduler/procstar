import asyncio
import logging
from typing import Any, Callable, Iterable, Type


logger = logging.getLogger(__name__)


class Interval:
    """
    Closed-open interval.

      >>> i = Interval(10, 20)
      >>> assert i == i
      >>> assert tuple(i) == (10, 20)
      >>> print(i)
      [10, 20)

    """

    def __init__(self, start, stop=None):
        self.start = start
        self.stop = stop

    def __eq__(self, other):
        return other is self or (other.start == self.start and other.stop == self.stop)

    def __str__(self):
        return f"[{self.start}, {self.stop})"

    def __iter__(self):
        return iter((self.start, self.stop))


def format_call(__fn, *args, **kw_args) -> str:
    """
    Formats a function call, with arguments, as a string.

      >>> format_call(open, "data.csv", mode="r")
      "open('data.csv', mode='r')"

    :param __fn:
      The function to call, or its name.
    """
    try:
        name = __fn.__name__
    except AttributeError:
        name = str(__fn)
    args = [repr(a) for a in args]
    args.extend(n + "=" + repr(v) for n, v in kw_args.items())
    return f"{name}({', '.join(args)})"


def format_ctor(obj, *args, **kw_args):
    return format_call(obj.__class__, *args, **kw_args)


async def retry_exception(
    fn: Callable[[], Any],
    exc_types: Iterable[Type[BaseException]],
    *,
    retries,
    interval,
):
    for retry in range(retries + 1):
        try:
            return await fn()
        except tuple(exc_types) as exc:
            if retry == retries:
                logger.error(f"failed after {retries} retries")
                raise
            else:
                logger.warning(str(exc))
                logger.debug(f"retrying in {interval}s")
                await asyncio.sleep(interval)
