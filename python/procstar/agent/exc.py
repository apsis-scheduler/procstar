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



class NotConnectedError(RuntimeError):
    """
    The connection isn't actually connected.

    When an agent disconnects, we keep the connection around for a while, rather
    than tossing it immediately, to give the agent an opportunity to reconnect.
    """

    def __init__(self, conn_id):
        super().__init__(f"not connected, but may reconnect: {conn_id}")
        self.conn_id = conn_id



class WebSocketNotOpen(RuntimeError):
    """
    The connection's Web Socket connection is not open.
    """

    def __init__(self, conn_id):
        super().__init__(f"connection web socket not open: {conn_id}")
        self.conn_id = conn_id



