import asyncio
from   contextlib import contextmanager

#-------------------------------------------------------------------------------

class Subscribeable:
    """
    Mixin to add support for async subscription to events.

    Call `_publish()` to publish an event to all current subscriptions.
    """

    def __init__(self):
        # Current subscriptions.
        self.__subs = set()


    class Subscription:
        """
        Async iterable and iterator of events sent to one subscription.
        """

        def __init__(self, events):
            self.__events = events

        def __aiter__(self):
            return self

        def __anext__(self):
            return self.__events.get()


    @contextmanager
    def subscription(self):
        # Create a queue for events sent to this subscription.
        events = asyncio.Queue()
        # Register the subscription.
        self.__subs.add(events)
        try:
            yield self.Subscription(events)
        finally:
            # Unregister the subscription.
            self.__subs.remove(events)


    def _publish(self, event):
        for sub in self.__subs:
            sub.put_nowait(event)



