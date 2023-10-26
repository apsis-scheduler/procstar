"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
import logging

from   .server import Server
from   procstar import proto

# Timeout to receive an initial login message.
TIMEOUT_LOGIN = 60

# FIXME: What is the temporal scope of a connection?

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

def main():
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--host", metavar="ADDR", default=None,
        help="serve from interface bound to ADDR [def: all]")
    parser.add_argument(
        "--port", metavar="PORT", type=int, default=proto.DEFAULT_PORT,
        help=f"serve from PORT [def: {proto.DEFAULT_PORT}]")
    args = parser.parse_args()

    async def run(server, loc):
        async with server.run(loc):
            # Wait for a connection.
            with server.connections.subscription() as conn_events:
                await anext(conn_events)

            # Start a process.
            proc = await server.start("proc0", {"argv": ["/usr/bin/sleep", "1"]})
            # Show result updates.
            while True:
                async for msg in proc.results:
                    pass


    logging.basicConfig(
        level=logging.DEBUG,
        format="%(asctime)s [%(levelname)-7s] %(message)s",
    )
    logging.getLogger("websockets.server").setLevel(logging.INFO)
    try:
        asyncio.run(run(Server(), loc=(args.host, args.port)))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    logging.getLogger().setLevel(logging.DEBUG)
    main()

