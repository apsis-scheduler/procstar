"""
WebSocket service for incoming connections from procstar instances.
"""

import asyncio
import logging
from   pathlib import Path

from   .server import Server, DEFAULT
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
    parser.add_argument(
        "--tls-cert", metavar="PATH", type=lambda p: Path(p).absolute(),
        help="use TLS cert from PATH")
    parser.add_argument(
        "--tls-key", metavar="PATH", type=lambda p: Path(p).absolute(),
        help="use TLS key from PATH [def: cert path with .key]")
    args = parser.parse_args()

    async def run(server, loc, tls_cert):
        async with server.run(loc=loc, tls_cert=tls_cert):
            # Wait for a connection.
            with server.connections.subscription() as conn_events:
                await anext(conn_events)

            # Start a process.
            proc = await server.start("proc0", {"argv": ["/usr/bin/sleep", "1"]})
            # Show result updates.
            while True:
                async for msg in proc.messages:
                    pass

    tls_cert = DEFAULT if args.tls_cert is None else (
        args.tls_cert,
        # If the key path wasn't given, assume the same path as the cert, except
        # with suffix '.key'.
        args.tls_cert.with_suffix(".key") if args.tls_key is None
        else args.tls_key
    )

    try:
        asyncio.run(run(
            Server(),
            loc=(args.host, args.port),
            tls_cert=tls_cert,
        ))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    logging.basicConfig(
        level=logging.DEBUG,
        format="%(asctime)s [%(levelname)-7s] %(message)s",
    )
    logging.getLogger("websockets.server").setLevel(logging.INFO)

    main()

