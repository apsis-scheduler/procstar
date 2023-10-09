import aioconsole
import asyncio
import logging
import shlex
import sys

from   . import ws_service

#-------------------------------------------------------------------------------

def write_prompt(stream):
    stream.write("\x1b[31m>>\x1b[0m ")
    stream.flush()


def print_with_prompt(stream, text):
    stream.write("\b\b\b")
    print(text, file=stream)
    stream.write("\x1b[31m>>\x1b[0m ")
    stream.flush()


def erase_prompt(stream):
    stream.write("\b\b\b")
    stream.flush()


class Handler(logging.StreamHandler):
    """
    Handler that removes the prompt before emitting a record and replaces it
    after.
    """

    def __init__(self, stream):
        super().__init__(stream)
        self.prompting = False


    def write_prompt(self):
        if not self.prompting:
            write_prompt(self.stream)
            self.prompting = True


    def erase_prompt(self):
        if self.prompting:
            erase_prompt(self.stream)
            self.prompting = False


    async def input(self):
        self.write_prompt()
        try:
            text = await aioconsole.ainput()
        except Exception:
            print(file=self.stream)
            raise
        else:
            self.prompting = False
            return text


    def emit(self, record):
        if self.prompting:
            erase_prompt(self.stream)
        super().emit(record)
        self.stream.flush()
        if self.prompting:
            write_prompt(self.stream)



#-------------------------------------------------------------------------------

async def handle(server, command):
    group = "default"
    next_proc = 0

    from . import proto

    command, *args = command.split(" ")
    if command in ("s", "start-command"):
        proc_id = f"proc{next_proc}"
        next_proc += 1

        command = args[0] if len(args) > 0 else "/usr/bin/sleep 5"
        spec = {"argv": shlex.split(command)}

        await server.start(group=group, proc_id=proc_id, spec=spec)

    elif command in ("l", "list-proc-ids"):
        for connection in server.connections.values():
            await connection.send(proto.ProcidListRequest())


LOG_FMT = "%(asctime)s [%(levelname)-7s] %(message)s"
LOG_DATEFMT = "%H:%M:%S"

async def repl(server, *, stream=sys.stdout):
    logger = logging.getLogger()
    old_handlers = logger.handlers
    handler = Handler(stream)
    handler.setFormatter(logging.Formatter(fmt=LOG_FMT, datefmt=LOG_DATEFMT))
    logger.handlers = [handler]
    handler.write_prompt()

    try:
        while True:
            try:
                command = await handler.input()
            except EOFError:
                break
            await handle(server, command)

    finally:
        logger.handlers = old_handlers


async def main():
    logging.basicConfig(
        level   =logging.INFO,
        format  =LOG_FMT,
        datefmt =LOG_DATEFMT,
    )

    server = ws_service.Server()
    async with server.serve():
        await repl(server)


if __name__ == "__main__":
    asyncio.run(main())


