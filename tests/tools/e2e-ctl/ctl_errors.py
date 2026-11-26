"""The typed error of every e2e-ctl action: an HTTP status and a code, answered as `{"error": ...}`."""


class ActionError(Exception):
    def __init__(self, status, code, message):
        super().__init__(message)
        self.status = status
        self.code = code
