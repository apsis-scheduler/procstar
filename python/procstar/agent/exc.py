#-------------------------------------------------------------------------------

class NoGroupError(LookupError):
    """
    No group with the given group name.
    """

    def __init__(self, group_id):
        super().__init__(f"no group: {group_id}")
        self.group_id = group_id



class NoOpenConnectionInGroup(RuntimeError):
    """
    The group contains no open connections.
    """

    def __init__(self, group_id):
        super().__init__(f"no connections in group: {group_id}")
        self.group_id = group_id



class NoConnectionError(LookupError):
    """
    No connection with the given connection ID.
    """

    def __init__(self, conn_id):
        super().__init__(f"no connection: {conn_id}")
        self.conn_id = conn_id



