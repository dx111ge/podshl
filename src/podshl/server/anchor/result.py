"""The distinction every resolver in this project used to destroy.

`trust.py`, `collectors.py`, `trust.rs` — all of them returned `None` for a
missing file, a malformed record, a timeout, a SERVFAIL and an NXDOMAIN alike.
So "the claimant published nothing" and "we could not ask" arrived as the same
value, and the user-facing message asserted the first.

That is not merely untidy. `SERVER.md` grades anchor failures — *one failed
check: nothing; 14 days: `stale`; 90 days: `unknown`* — and **that grading is
impossible to implement on top of an Option**. A check that cannot say why it
failed cannot decide whether the failure counts.

The failure mode this prevents is specific and bad: with an Option-shaped
check, one resolver outage on our side marches *every* anchor toward `unknown`
at the same time — a self-inflicted mass un-enrolment of a service explicitly
designed never to revoke.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum


class Reason(str, Enum):
    CONFIRMED = "confirmed"             # asked, answered, matched
    CONTRADICTED = "contradicted"       # asked, answered, wrong token
    ABSENT = "absent"                   # asked, answered "no such thing"
    MALFORMED = "malformed"             # asked, answered, unparseable
    REDIRECTED_AWAY = "redirected_away"  # told to go elsewhere; we did not
    UNREACHABLE = "unreachable"         # not asked: timeout, TLS, SERVFAIL
    REFUSED = "refused"                 # not asked: 403, 429, 451, DNS REFUSED
    INTERNAL = "internal"               # not asked: our bug, our outage


#: The claimant answered, and the answer was no. The clock runs.
EVIDENCE = frozenset({
    Reason.CONTRADICTED, Reason.ABSENT, Reason.MALFORMED, Reason.REDIRECTED_AWAY,
})

#: We could not ask, or were not allowed to. The clock does not run, because
#: "we had an outage" must never be spelled "they abandoned it".
SILENCE = frozenset({Reason.UNREACHABLE, Reason.REFUSED, Reason.INTERNAL})


@dataclass(frozen=True, slots=True)
class Probed:
    reason: Reason
    detail: dict = field(default_factory=dict)
    value: str | None = None
    at: datetime = field(default_factory=lambda: datetime.now(timezone.utc))

    @property
    def confirmed(self) -> bool:
        return self.reason is Reason.CONFIRMED

    @property
    def is_evidence(self) -> bool:
        return self.reason in EVIDENCE

    @property
    def is_silence(self) -> bool:
        return self.reason in SILENCE

    def __bool__(self):
        raise TypeError(
            "a Probed has three answers, not two — confirmed, evidence of absence, "
            "and silence. `if result:` is exactly the bug this type exists to "
            "prevent: it collapses 'they published nothing' into 'we could not "
            "ask'. Ask .confirmed, .is_evidence or .is_silence."
        )
