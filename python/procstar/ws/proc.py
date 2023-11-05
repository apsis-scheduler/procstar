"""
Processes on connected procstar instances.
"""

import asyncio
from   collections.abc import Mapping
import logging
from   typing import Optional

from   procstar import proto
from   procstar.lib.json import Jso

logger = logging.getLogger(__name__)

#-------------------------------------------------------------------------------

class Process:
    """
    A process running under a connected procstar instance.
    """

    class Results:
        """
        Process result updates.

        Acts as a one-shot async iterable and iterator of result updates.  The
        `latest` property always returns the most recent, which may be none if
        no result has yet been received.
        """

        def __init__(self):
            self.__latest = None
            self.__updates = asyncio.Queue()


        @property
        def latest(self) -> Optional[Jso]:
            """
            Most recent received process result.
            """
            return self.__latest


        def __aiter__(self):
            return self


        def __anext__(self):
            return self.__updates.get()


        def _update(self, result):
            self.__latest = result
            self.__updates.put_nowait(result)


    proc_id: str
    conn_id: str

    results: Results

    # FIXME
    errors: list[str]

    # FIXME: What happens when the connection is closed?

    def __init__(self, conn_id, proc_id):
        self.proc_id = proc_id
        self.conn_id = conn_id
        self.results = self.Results()
        # FIXME: Receive proc-specific errors.
        self.errors = []


    async def wait_for_completion(self) -> Jso:
        """
        Awaits and returns a completed result.
        """
        # Is it already completed?
        res = self.results.latest
        if res is not None and res.status is not None:
            return res

        while True:
            res = await anext(self.results)
            if res.status is not None:
                return res



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

            case proto.ProcResult(proc_id, res):
                proc = get_proc(proc_id)
                logger.debug(f"msg proc result: {proc_id}")
                # Attach procstar info to the result.
                result = Jso(res)
                result.procstar = procstar_info
                proc.results._update(result)

            case proto.ProcDelete(proc_id):
                proc = get_proc(proc_id)
                logger.debug(f"msg proc delete: {proc_id}")
                del self.__procs[proc_id]

            case proto.Register:
                # We should receive this only immediately after connection.
                logger.error(f"msg unexpected: {msg}")

            case proto.IncomingMessageError():
                # FIXME: Implement.
                # FIXME: Proc-specific errors.
                logger.error(f"msg error: {msg.err}")


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



