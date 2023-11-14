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

    proc_id: str
    conn_id: str

    """
    The most recent result received for this proc.
    """
    result: Optional[object]
    # FIXME
    errors: list[str]

    # FIXME: Elsewhere.
    class Messages:

        def __init__(self):
            self.__queue = asyncio.Queue()


        def __aiter__(self):
            return self


        def __anext__(self):
            return self.__queue.get()


        def push(self, msg):
            self.__queue.put_nowait(msg)



    # FIXME: What happens when the connection is closed?

    def __init__(self, conn_id, proc_id):
        self.proc_id = proc_id
        self.conn_id = conn_id
        self.messages = self.Messages()
        self.result = None
        # FIXME: Receive proc-specific errors.
        self.errors = []


    def on_message(self, msg):
        match msg:
            case proto.ProcResult(_proc_id, res):
                res = Jso(res)
                self.result = res
                self.messages.push(("result", res))

            case proto.ProcDelete():
                self.messages.push(("delete", None))


    async def wait_for_completion(self) -> Jso:
        """
        Awaits and returns a completed result.
        """
        # Is the most recent result completed?
        if self.result is None or self.result.status is None:
            # Wait for a result message with a completed status.
            await anext(
                None
                async for t, r in self.messages
                if t == "result" and r.status is not None
            )

        return self.result



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
                msg.res["procstar"] = procstar_info
                get_proc(proc_id).on_message(msg)

            case proto.ProcDelete(proc_id):
                logger.debug(f"msg proc delete: {proc_id}")
                self.__procs.pop(proc_id).on_message(msg)

            case proto.Register:
                # We should receive this only immediately after connection.
                logger.error(f"msg unexpected: {msg}")

            case proto.IncomingMessageError():
                # FIXME: Implement.
                # FIXME: Proc-specific errors.
                logger.error(f"msg error: {msg.err}")

            case proto.Close():
                pass



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



