"""
Processes on connected procstar instances.
"""

import asyncio
from   collections.abc import Mapping
from   dataclasses import dataclass
from   functools import cached_property
import logging

from   procstar import proto
from   procstar.lib.asyn import iter_queue
from   procstar.lib.py import Interval
import procstar.lib.json

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

class ProcessUnknownError(RuntimeError):
    """
    The process is unknown to the remote agent.
    """

    def __init__(self, proc_id):
        super().__init__(f"process unknown: {proc_id}")
        self.proc_id = proc_id



class ProcessDeletedError(RuntimeError):
    """
    The process was deleted.
    """

    def __init__(self, proc_id):
        super().__init__(f"process deleted: {proc_id}")
        self.proc_id = proc_id



# Derive from Jso to convert dict into object semantics.
class Result(procstar.lib.json.Jso):
    """
    The proc res dictionary produced by the agent.
    """



@dataclass
class FdData:
    """
    The fd name.
    """
    fd: str

    """
    The interval of output bytes contained in this update.
    """
    interval: Interval = Interval(0, 0)

    """
    The output encoding.
    """
    encoding: str | None = None

    """
    The output data.
    """
    data: bytes = b""



#-------------------------------------------------------------------------------

class Process:
    """
    A process running under a connected procstar instance.
    """

    proc_id: str
    conn_id: str

    # FIXME
    errors: list[str]

    # FIXME: What happens when the connection is closed?

    def __init__(self, conn, proc_id):
        self.__conn = conn
        self.proc_id = proc_id
        # FIXME: Receive proc-specific errors.
        self.errors = []
        self.__msgs = asyncio.Queue()


    @property
    def conn_id(self):
        return self.__conn.conn_id


    def _on_message(self, msg):
        self.__msgs.put_nowait(msg)


    @cached_property
    async def updates(self):
        """
        A singleton async iterator over updates for this process.

        The iterator may:
        - yield a `Result` instance
        - yield a `FdData` instance
        - terminate if the process is deleted
        - raise `ProcessUnknownError` if the proc ID is unknown

        """
        async for msg in iter_queue(self.__msgs):
            assert msg.proc_id == self.proc_id
            match msg:
                case proto.ProcResult(_, result):
                    yield Result(result)

                case proto.ProcFdData(_, fd, start, stop, encoding, data):
                    # FIXME: Process data.
                    yield FdData(
                        fd      =fd,
                        interval=Interval(start, stop),
                        encoding=encoding,
                        data    =data,
                    )

                case proto.ProcDelete(_):
                    # Done.
                    break

                case proto.ProcUnknown(_):
                    raise ProcessUnknownError(self.proc_id)

                case _:
                    assert False, f"unexpected msg: {msg!r}"


    def request_result(self):
        """
        Returns a coro that sends a request for updated result.
        """
        return self.__conn.send(proto.ProcResultRequest(self.proc_id))


    def request_fd_data(self, fd, *, interval=Interval(0, None)):
        """
        Returns a coro that requests updated output data,
        """
        return self.__conn.send(proto.ProcFdDataRequest(
            proc_id =self.proc_id,
            fd      =fd,
            start   =interval.start,
            stop    =interval.stop,
        ))


    def send_signal(self, signum):
        """
        Returns a coro that sends a signal to the proc.
        """
        return self.__conn.send(proto.ProcSignalRequest(self.proc_id, signum))


    def request_delete(self):
        """
        Returns a coro that requests deletion of the proc.
        """
        return self.__conn.send(proto.ProcDeleteRequest(self.proc_id))


    async def delete(self):
        """
        Requests deletion of the proc and awaits confirmation.
        """
        await self.request_delete()
        # The update iterator exhausts when the proc is deleted.
        async for update in self.updates:
            pass



#-------------------------------------------------------------------------------

class Processes(Mapping):
    """
    Processes running under connected procstar instances.

    Maps proc ID to `Process` instances.
    """

    def __init__(self):
        self.__procs = {}


    def create(self, conn, proc_id) -> Process:
        """
        Creates and returns a new process on `connection` with `proc_id`.

        `proc_id` must be unknown.
        """
        assert proc_id not in self.__procs
        self.__procs[proc_id] = proc = Process(conn, proc_id)
        return proc


    def on_message(self, procstar_info, msg):
        """
        Processes `msg` to the corresponding process.

        :param procstar_info:
          About the procstar instance from which the message was received.
        """
        def get_proc(proc_id):
            """
            Looks up or creates, if necessary, the `Process` object.
            """
            try:
                return self.__procs[proc_id]
            except KeyError:
                conn_id = procstar_info.conn.conn_id
                logger.info(f"new proc on {conn_id}: {proc_id}")
                return self.create(self, proc_id)

        match msg:
            case proto.ProcidList(proc_ids):
                # Make sure we track a proc for each proc ID the instance knows.
                for proc_id in proc_ids:
                    _ = get_proc(proc_id)

            case proto.ProcResult(proc_id):
                # Attach Procstar server and connection info to the result.
                msg.res["procstar"] = procstar_info
                get_proc(proc_id)._on_message(msg)

            case proto.ProcFdData(proc_id):
                get_proc(proc_id)._on_message(msg)

            case proto.ProcDelete(proc_id) | proto.ProcUnknown(proc_id):
                self.__procs.pop(proc_id)._on_message(msg)

            case proto.Register:
                # We should receive this only immediately after connection.
                logger.error(f"msg unexpected: {msg}")

            case proto.IncomingMessageError():
                # FIXME: Proc-specific errors.
                logger.error(f"msg error: {msg.err}")

            case _:
                logger.error(f"unknown msg: {msg}")



    # Mapping methods

    def __contains__(self, proc_id):
        return self.__procs.__contains__(proc_id)


    def __getitem__(self, proc_id):
        return self.__procs.__getitem__(proc_id)


    def __len__(self):
        return self.__procs.__len__()


    def __iter__(self):
        return self.__procs.__iter__()


    def values(self):
        return self.__procs.values()


    def items(self):
        return self.__procs.items()



