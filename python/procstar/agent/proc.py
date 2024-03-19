"""
Processes on connected procstar instances.
"""

import asyncio
from   collections.abc import Mapping
import logging

from   procstar import proto

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



class Results:
    """
    Single-consumer async iterable of results of a process.
    """

    """
    The most recent result received for the process.
    """
    latest: object

    def __init__(self):
        self.latest = None
        self.__queue = asyncio.Queue()


    def __aiter__(self):
        return self


    async def __anext__(self):
        """
        :raise ProcessUnknownError:
          The process is unknown to the remote agent.
        :raise ProcessDeletedError:
          The process was deleted before returning another result.
        """
        match await (msg := self.__queue.get()):
            case proto.ProcResult(_, result):
                self.latest = result
                return result

            case proto.ProcDelete(proc_id):
                raise ProcessDeletedError(proc_id)

            case proto.ProcUnknown(proc_id):
                raise ProcessUnknownError(proc_id)

            case _:
                raise NotImplementedError(f"unknown msg: {msg}")


    def _on_message(self, msg):
        self.__queue.put_nowait(msg)


    async def wait(self):
        """
        Awaits a running process.

        Returns immediately if a non-running result has alread been received.

        :return:
          The process result.
        :raise ProcessDeletedError:
          The process was deleted before returning another result.
        """
        # Is the most recent result completed?  If so, return it immediately.
        if self.latest is not None and self.latest.state != "running":
            return self.result

        # Wait for a completed result.
        async for result in self:
            if result.state != "running":
                return result



#-------------------------------------------------------------------------------

class Process:
    """
    A process running under a connected procstar instance.
    """

    proc_id: str
    conn_id: str

    """
    The most recent result received for this proc.
    """
    results: Results
    # FIXME
    errors: list[str]

    # FIXME: What happens when the connection is closed?

    def __init__(self, conn_id, proc_id):
        self.proc_id = proc_id
        self.conn_id = conn_id
        self.results = Results()
        # FIXME: Receive proc-specific errors.
        self.errors = []



#-------------------------------------------------------------------------------

class Processes(Mapping):
    """
    Processes running under connected procstar instances.

    Maps proc ID to `Process` instances.
    """

    def __init__(self):
        self.__procs = {}


    def create(self, conn_id, proc_id) -> Process:
        """
        Creates and returns a new process on `connection` with `proc_id`.

        `proc_id` must be unknown.
        """
        assert proc_id not in self.__procs
        self.__procs[proc_id] = proc = Process(conn_id, proc_id)
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
                return self.create(conn_id, proc_id)

        match msg:
            case proto.ProcidList(proc_ids):
                logger.debug(f"msg proc_id list: {proc_ids}")
                # Make sure we track a proc for each proc ID the instance knows.
                for proc_id in proc_ids:
                    _ = get_proc(proc_id)

            case proto.ProcResult(proc_id):
                logger.debug(f"msg proc result: {proc_id}")
                msg.res.procstar = procstar_info
                get_proc(proc_id).results._on_message(msg)

            case proto.ProcDelete(proc_id):
                logger.debug(f"msg proc delete: {proc_id}")
                self.__procs.pop(proc_id).results._on_message(msg)

            case proto.ProcUnknown(proc_id):
                logger.debug(f"msg proc unknown: {proc_id}")
                self.__procs.pop(proc_id).results._on_message(msg)

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



