"""Grading a failing anchor — arithmetic over evidence, never over silence.

Losing an anchor is not misconduct. Someone who simply stopped working on
something has done nothing wrong, so this never produces `revoked`: revocation
is an accusation, and the states here are an age.

The last known good content keeps being served through `stale`, because an
abandoned project's solutions are usually still correct — the software did not
change either — and deleting them helps nobody. What the user needs is the age,
not a refusal.
"""
from __future__ import annotations

from datetime import datetime, timedelta, timezone

from .. import log_store
from ..config import STALE_AFTER_DAYS, UNKNOWN_AFTER_DAYS
from .result import Probed

#: Above this share of silence, a sweep run was our outage rather than the
#: internet's, and its results are not held against anybody. Without this, one
#: bad afternoon eventually reads as ten thousand abandoned projects.
DEGRADED_SILENCE_RATE = 0.20

#: Persistent silence is an operator problem, not a state change. A permanent
#: 403 or a firewall that hates our egress must not silently un-enrol a live
#: project.
SILENCE_CEILING_DAYS = 30


def record(conn, anchor_id: int, probed: Probed, *, run_id: int | None = None) -> None:
    """Write the probe and move the clock. The ONLY place `failing_since` moves."""
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO anchor_probe (anchor_id, at, reason, is_evidence, detail, run_id) "
            "VALUES (%s, %s, %s, %s, %s, %s)",
            (anchor_id, probed.at, probed.reason.value, probed.is_evidence,
             __import__("json").dumps(probed.detail), run_id),
        )
        if probed.confirmed:
            # `status` is conditional and the rest is not. A good probe is a true
            # fact about control and is always recorded — but returning the
            # anchor to `live` is an *enrolment* change, and a taken-down anchor
            # must not be re-enrolled by anyone who can republish a file. This is
            # the whole of the fix in 0009: without the CASE, `POST
            # /claim/{host}/verify` reversed a takedown with no authentication.
            cur.execute(
                "UPDATE anchor SET last_checked = %s, last_confirmed = %s, "
                "failing_since = NULL, "
                "status = CASE WHEN taken_down_at IS NULL THEN 'live' ELSE status END, "
                "verified_at = COALESCE(verified_at, %s) WHERE id = %s",
                (probed.at, probed.at, probed.at, anchor_id),
            )
        elif probed.is_evidence:
            cur.execute(
                "UPDATE anchor SET last_checked = %s, "
                "failing_since = COALESCE(failing_since, %s) WHERE id = %s",
                (probed.at, probed.at, anchor_id),
            )
        else:
            # Silence touches `last_checked` and nothing else. Ever.
            cur.execute("UPDATE anchor SET last_checked = %s WHERE id = %s",
                        (probed.at, anchor_id))


def grade(failing_since: datetime | None, status: str, now: datetime | None = None) -> str | None:
    """The status this anchor should be in, or None for no change."""
    if failing_since is None:
        return "live" if status != "live" else None
    now = now or datetime.now(timezone.utc)
    elapsed = now - failing_since
    if elapsed >= timedelta(days=UNKNOWN_AFTER_DAYS):
        return "unknown" if status != "unknown" else None
    if elapsed >= timedelta(days=STALE_AFTER_DAYS):
        return "stale" if status not in ("stale", "unknown") else None
    return None


def apply_grades(conn, now: datetime | None = None) -> list[tuple[int, str, str]]:
    """Move every anchor that has been failing long enough, and log each move.

    `SERVER.md` requires a log entry only for the 90-day transition. The 14-day
    one is logged too: it costs one entry per anchor per lapse and makes the
    whole state machine reconstructible from the log alone, which is what "our
    own honesty is checkable" actually requires.
    """
    moved: list[tuple[int, str, str]] = []
    with conn.cursor() as cur:
        cur.execute(
            "SELECT a.id, a.kind, a.value, a.status, a.failing_since, a.last_confirmed, "
            "       (SELECT count(*) FROM anchor_probe p "
            "        WHERE p.anchor_id = a.id AND p.is_evidence) AS evidence "
            "FROM anchor a WHERE a.failing_since IS NOT NULL AND a.status <> 'unknown'"
        )
        rows = cur.fetchall()

    for row in rows:
        target = grade(row["failing_since"], row["status"], now)
        if target is None or target == row["status"]:
            continue
        seq = log_store.append(
            conn, "anchor_state_changed",
            {
                "anchor": {"kind": row["kind"], "value": row["value"]},
                "from": row["status"], "to": target,
                "last_confirmed": row["last_confirmed"].isoformat() if row["last_confirmed"] else None,
                "evidence_probes": row["evidence"],
            },
            anchor_id=row["id"],
        )
        with conn.cursor() as cur:
            cur.execute("UPDATE anchor SET status = %s WHERE id = %s", (target, row["id"]))
            if target == "unknown":
                # Serving stops, and the attestation is withdrawn as `degraded`
                # — which means exactly `unknown`, the state of a project that
                # never registered. Never `revoked`.
                cur.execute("UPDATE source SET mirror_state = 'withheld' WHERE anchor_id = %s",
                            (row["id"],))
                cur.execute(
                    "UPDATE attestation SET withdrawn_at = now(), withdrawn_kind = 'degraded', "
                    "withdrawn_reason = 'anchor_control_lapsed', withdrawn_seq = %s "
                    "WHERE anchor_id = %s AND withdrawn_at IS NULL",
                    (seq, row["id"]),
                )
        moved.append((row["id"], row["status"], target))
    return moved
