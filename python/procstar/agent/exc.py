#-------------------------------------------------------------------------------

class NoOpenConnectionInGroup(RuntimeError):
    """
    The group contains no open connections.
    """

    def __init__(self, group_id):
        super().__init__(f"no connected agent in group: {group_id}")
        self.group_id = group_id



class NoConnectionError(LookupError):
    """
    No connection with the given connection ID.
    """

    def __init__(self, conn_id):
        super().__init__(f"unknown connection: {conn_id}")
        self.conn_id = conn_id



