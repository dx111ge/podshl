"""Vendor-side receipt of user-initiated reports.

Two rules, both of which exist because the alternative destroys the channel:

**Every report gets a state.** Received, duplicate of a known issue, fixed in
version X, or won't-fix *with a reason*. Silence kills the loop after exactly
one attempt — a user who reports and hears nothing never reports again.

**The reward is the receipt, never a token.** Rewarding the *act* of reporting
buys duplicates, noise and farming, which corrupts the one signal the whole
design exists to collect. Rewarding the *outcome* — "you reported this in March,
it shipped in 14.2, 312 others hit it" — is not farmable, because a user cannot
manufacture a fix. It also confirms the motivation people actually have, which
is the hope that something improves, rather than substituting a badge for it.
"""
from __future__ import annotations

import hashlib
import secrets

from .catalog import said

# Below this many independent observations, a combination is not reported as an
# individual event — it is only ever counted. Rare configurations are
# identifying, and the vendor is the only party that can see the population.
K_ANONYMITY = 5

#: Per signature, the *reporters* seen — not the submissions. This used to be a
#: `Counter` of submissions, so one client reporting the same combination five
#: times crossed the floor alone: the threshold that exists because a rare
#: constellation identifies somebody was reached by exactly one somebody. The
#: client has sent `pseudonym` and `epoch` with every report all along, and the
#: operator server has counted distinct pseudonyms since the catch-all was
#: retired; this vendor read neither (`T3a`).
_seen: dict[str, set[bytes]] = {}

#: Pseudonyms are not kept, only keyed digests of them, and the key lives only
#: in this process. The count survives; which pseudonym was which does not, and
#: cannot be tested against a pseudonym later once the process is gone.
_SALT = secrets.token_bytes(32)


def _reporter(report: dict, sig: str) -> bytes | None:
    pseudonym = report.get("pseudonym")
    if not isinstance(pseudonym, str) or not pseudonym:
        return None
    epoch = str(report.get("epoch") or "")
    return hashlib.blake2b(f"{sig}|{epoch}|{pseudonym}".encode(), key=_SALT,
                           digest_size=16).digest()

# Seeded so the full loop is demonstrable: an issue already fixed, and one still
# open, both keyed by the finding a skill produces.
_KNOWN: dict[str, dict] = {
    "torch.precision.consumer-gpu/bf16-emulated": {
        "state": "fixed_in",
        "version": "3.2.0",
        "reports": 312,
        "note": "known_bf16",
    },
    "warranty.rma.precheck/artefakte": {
        "state": "known",
        "reports": 47,
        "note": "known_artefacts",
    },
}


def signature(report: dict) -> str:
    """Stable key over the generalised observation — never over identifiers."""
    parts = [str(report.get("skill_id"))] + [
        f"{k}={v}" for k, v in sorted((report.get("observed") or {}).items())
    ]
    return hashlib.sha256("|".join(parts).encode()).hexdigest()[:16]


def receive(report: dict, lang: str = "en") -> dict:
    """Returns the receipt the user sees. This is the entire reward — in the
    language the client asked for, where the vendor has it."""
    for key, known in _KNOWN.items():
        skill, marker = key.split("/", 1)
        if report.get("skill_id") == skill and (
            marker in str(report.get("observed")).lower()
            or marker in str(report.get("failed_actions")).lower()
            or report.get("resolved_by") == "general_agent"
        ):
            known = {**known, "reports": known["reports"] + 1}
            note = said(lang, known["note"])
            return {
                "state": known["state"],
                "message": (
                    said(lang, "receipt_fixed", n=known["reports"], version=known["version"], note=note)
                    if known["state"] == "fixed_in"
                    else said(lang, "receipt_known", n=known["reports"], note=note)
                ),
                "reports": known["reports"],
            }

    sig = signature(report)
    who = _reporter(report, sig)
    seen = _seen.setdefault(sig, set())
    if who is None:
        # Without a pseudonym there is no way to tell this sender from one
        # already counted, so it cannot count towards the floor at all —
        # counting it would let anyone cross the threshold by leaving the field
        # out. It still gets a receipt: silence kills the channel.
        return {
            "state": "received",
            "message": said(lang, "receipt_no_pseudonym"),
            "reports": len(seen),
            "below_threshold": True,
        }
    repeated = who in seen
    seen.add(who)
    n = len(seen)
    if n < K_ANONYMITY:
        # Counted, but deliberately not surfaced as an individual observation.
        return {
            "state": "received",
            "message": said(lang, "receipt_rare")
                       + (" " + said(lang, "receipt_rare_again") if repeated else ""),
            "reports": n,
            "below_threshold": True,
        }
    return {
        "state": "new",
        "message": said(lang, "receipt_new", n=n),
        "reports": n,
    }
