"""One error taxonomy, so a refusal can say which rule it broke.

A rejection a developer cannot act on is a rejection that gets worked around.
Every error here carries a code from a closed vocabulary, because the codes are
counted: `SERVER.md` requires every takedown to be logged with a public reason,
and a reason that is free text cannot be counted or compared.
"""
from __future__ import annotations


class ServerError(Exception):
    code = "error"

    def __init__(self, message: str, **detail):
        super().__init__(message)
        self.message = message
        self.detail = detail

    def as_dict(self) -> dict:
        return {"code": self.code, "reason": self.message, **self.detail}


class IngestRefused(ServerError):
    """A document that does not conform. Refused at the boundary, not at
    execution — a solution proposing an action nobody implements should be
    turned away when it arrives, not when a user has already consented to a plan
    built around it."""
    code = "ingest_refused"


class NotAttested(ServerError):
    """Asked for something only an attested anchor has. This is not an
    accusation and must never read as one: `unknown` is the state of everyone
    who never registered."""
    code = "not_attested"


class NotClaimed(ServerError):
    """The dashboard for a domain nobody has proved they control. Refused for
    everyone, including the operator's own curiosity."""
    code = "not_claimed"


class LogSealed(ServerError):
    """An attempt to alter the log. There is no path to this; if it is raised,
    something is wrong that a reason code will not fix."""
    code = "log_sealed"
