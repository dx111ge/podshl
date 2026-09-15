"""Notice and action, and why a takedown degrades rather than deletes.

Removing on every claim without review makes the takedown path a free weapon,
and the ground it clears is the supply side the whole thing depends on. The
answer is not courage — the operator takes no financial risk and a German
cease-and-desist costs money to answer even when one complies at once. The
answer is **reach**:

* Exposure exists only where we host for others. An enterprise vendor talks to
  its own endpoint with no fallback to us, so we hold nothing there and there is
  nothing to remove.
* Solutions live in the developer's repository and we are the mirror. A takedown
  reaches the mirror, not the source.
* So it degrades `attested` to `unknown` — precisely the state of a project that
  never registered. Not death, un-enrolment, and the client already handles that
  without anything new being built.

Two duties survive any risk appetite: a route for a notice, and a **statement of
reasons to the affected party**. The second is the same log entry, which is why
removing quietly is impossible here rather than merely discouraged.

**A notice is not a decision.** `receive` used to perform the takedown in the
same unauthenticated request that filed the notice, so anybody could un-enrol
any mirrored project by typing a name and a reason code — the weapon the first
paragraph says this path must not be. A notice is recorded as `pending` now, and
a person acts on it from the operator's own listener (`act`), or reverses a
decision they made (`reinstate`). Both are log entries with a public reason.
"""
from __future__ import annotations

import json

from . import log_store
from .errors import ServerError

#: A closed vocabulary, so weaponised claims are visible and countable. Free
#: text here would make the reason unreadable in aggregate, and counting is the
#: point: a notifier who sends fifty is a fact worth being able to state.
REASON_CODES = (
    "trademark_claim",
    "copyright_claim",
    "impersonation",
    "malware",
    "court_order",
    "other",
)

#: How much a notifier may say about themselves. A notice names a person and a
#: way to reply; it is not a place to store a document.
MAX_NOTIFIER_FIELD = 512


class NoSuchPath(ServerError):
    """SV39. A notice aimed at a problem class rather than an anchor.

    There is no path to act on one, and that is deliberate. A manifest declares
    the problem classes it handles, and those name other people's products — a
    community project that fixes NVIDIA driver problems has to be able to say
    so. If a takedown could reach a problem class, a vendor could forbid anyone
    from saying their name out loud.
    """

    code = "no_such_path"


class NoSuchNotice(ServerError):
    code = "no_such_notice"


class NotPending(ServerError):
    """Acting on a notice that is not in the state the action needs. A decision
    cannot be made twice, and a reversal needs a decision to reverse."""

    code = "not_pending"


def _check_notifier(notifier) -> dict:
    """Typed here, so a body shaped wrong is a sentence and not a traceback."""
    if not isinstance(notifier, dict):
        raise ServerError("notifier must be an object with a name and a contact")
    name, contact = notifier.get("name"), notifier.get("contact")
    if not isinstance(name, str) or not name.strip() \
            or not isinstance(contact, str) or not contact.strip():
        raise ServerError("a notice must say who is sending it and how to reply")
    # The form has always sent this and the route has always thrown it away, so
    # the one thing the DSA asks a notifier to assert was collected by a
    # checkbox and discarded by the server - which looks like a safeguard and
    # is not one. Filing stays free, remote and unauthenticated, because a
    # notice comes from a stranger; what can be asked of a stranger is that
    # they say in the request that they mean it (`0014`).
    if notifier.get("statement_of_good_faith") is not True:
        raise ServerError(
            "a notice must carry notifier.statement_of_good_faith = true - the "
            "statement that you believe what the notice says is accurate. It is "
            "what the law asks of whoever files one, and it is what makes this a "
            "statement somebody made rather than a button somebody pressed.")
    for k, v in notifier.items():
        if not isinstance(k, str) or not isinstance(v, (str, int, float, bool)) \
                or (isinstance(v, str) and len(v) > MAX_NOTIFIER_FIELD):
            raise ServerError(f"notifier.{k} is not a short text value")
    return notifier


def _anchor_for(conn, host: str) -> dict | None:
    with conn.cursor() as cur:
        # `host` is not unique on `anchor`. Without an ORDER BY a notice could
        # land on one row today and its decision on another tomorrow.
        cur.execute("SELECT id, kind, value FROM anchor WHERE host = %s "
                    "ORDER BY id LIMIT 1", (host,))
        return cur.fetchone()


def receive(conn, *, reason_code: str, notifier: dict,
            anchor_host: str | None = None,
            problem_class: str | None = None) -> dict:
    """Record a notice. Nothing is withdrawn here.

    Refused outright where nothing could ever be done — a problem class, or an
    anchor we hold nothing for — because a person's time is the scarce thing
    and a queue full of the undoable spends it. Everything else waits for one.
    """
    if not isinstance(reason_code, str) or reason_code not in REASON_CODES:
        raise ServerError(
            f"reason code {reason_code!r} is not one of {list(REASON_CODES)}. "
            f"The codes are closed so that claims can be counted.")
    notifier = _check_notifier(notifier)
    if anchor_host is not None and not isinstance(anchor_host, str):
        raise ServerError("anchor_host must be a host name")
    if problem_class is not None and not isinstance(problem_class, str):
        raise ServerError("problem_class must be text")

    if problem_class and not anchor_host:
        # Recorded, then refused. "No such path exists" should be demonstrable
        # rather than asserted, so the refusal is written down and countable.
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO notice (reason_code, notifier, acted_at, action, refused_reason, "
                "                    good_faith_stated) "
                "VALUES (%s, %s, now(), 'refused', %s, true) RETURNING id",
                (reason_code, json.dumps(notifier),
                 f"aimed at the problem class {problem_class!r} rather than at an anchor"),
            )
            nid = cur.fetchone()["id"]
        raise NoSuchPath(
            "a notice can name an anchor, never a problem class. A manifest that "
            "says it handles a product is describing what it repairs, not claiming "
            "to be it — and a takedown that reached a class would let a vendor "
            "forbid anyone from naming their software.",
            notice=nid,
        )
    if not anchor_host:
        raise ServerError("a notice names the anchor it is about")

    anchor = _anchor_for(conn, anchor_host)
    if not anchor:
        # Enterprise, or simply somebody we never mirrored. Nothing of theirs is
        # here, so there is nothing to remove.
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO notice (reason_code, notifier, acted_at, action, refused_reason, "
                "                    good_faith_stated) "
                "VALUES (%s, %s, now(), 'refused', %s, true) RETURNING id",
                (reason_code, json.dumps(notifier), "nothing is mirrored for this anchor"),
            )
            nid = cur.fetchone()["id"]
        return {"notice": nid, "action": "refused",
                "why": "we hold nothing for this anchor. Where a vendor runs its own "
                       "endpoint we are not in the path, so there is nothing here to remove."}

    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO notice (anchor_id, reason_code, notifier, action, good_faith_stated) "
            "VALUES (%s, %s, %s, 'pending', true) RETURNING id",
            (anchor["id"], reason_code, json.dumps(notifier)))
        nid = cur.fetchone()["id"]

    return {
        "notice": nid,
        "action": "pending",
        "why": "recorded, and waiting for a person. Nothing is withdrawn by filing a "
               "notice: a route that acted on its own would let anyone un-enrol "
               "anyone. When a decision is made it is a public log entry with a "
               "reason code, and the affected party is told where to read it.",
    }


def _notice(conn, notice_id: int) -> dict:
    with conn.cursor() as cur:
        cur.execute("SELECT n.id, n.anchor_id, n.reason_code, n.action, n.log_seq, "
                    "       a.kind, a.value "
                    "FROM notice n LEFT JOIN anchor a ON a.id = n.anchor_id "
                    "WHERE n.id = %s", (notice_id,))
        row = cur.fetchone()
    if not row:
        raise NoSuchNotice(f"no notice {notice_id}")
    return row


def act(conn, notice_id: int) -> dict:
    """Perform the takedown a pending notice asks for. The operator's decision.

    The mirror is withheld, the attestation withdrawn as `degraded`, and the
    anchor marked taken down — a fact the liveness machinery cannot overwrite,
    so a republished challenge file does not undo it (`0009`).
    """
    n = _notice(conn, notice_id)
    if n["action"] != "pending":
        raise NotPending(f"notice {notice_id} is {n['action']}, not pending")
    if n["anchor_id"] is None:
        raise NotPending(f"notice {notice_id} names no anchor")

    seq = log_store.append(
        conn, "takedown",
        {
            "anchor": {"kind": n["kind"], "value": n["value"]},
            "reason_code": n["reason_code"],
            "action": "degraded",
            "means": "the mirror is withdrawn; the source in the developer's "
                     "repository is untouched, and the anchor is now `unknown` — "
                     "the state of a project that never registered",
        },
        anchor_id=n["anchor_id"],
    )
    with conn.cursor() as cur:
        cur.execute("UPDATE source SET mirror_state = 'withheld' WHERE anchor_id = %s",
                    (n["anchor_id"],))
        cur.execute(
            "UPDATE attestation SET withdrawn_at = now(), withdrawn_kind = 'degraded', "
            "withdrawn_reason = %s, withdrawn_seq = %s "
            "WHERE anchor_id = %s AND withdrawn_at IS NULL",
            (n["reason_code"], seq, n["anchor_id"]))
        # `status` alone was not enough. It is the liveness column, and the next
        # good probe — including one anybody can trigger through
        # `POST /claim/{host}/verify` — used to set it back to `live`. The
        # takedown is recorded as its own fact, pointing at the statement of
        # reasons, and nothing automatic clears it.
        cur.execute(
            "UPDATE anchor SET status = 'unknown', taken_down_at = now(), "
            "taken_down_seq = %s WHERE id = %s",
            (seq, n["anchor_id"]))
        cur.execute(
            "UPDATE notice SET acted_at = now(), action = 'degraded', log_seq = %s "
            "WHERE id = %s", (seq, notice_id))

    return {
        "notice": notice_id,
        "action": "degraded",
        "log_seq": seq,
        "statement_of_reasons": f"/log/entries?start={seq}&end={seq + 1}",
        "why": "the mirror is withdrawn and the anchor is now `unknown`. The source "
               "in the developer's repository is untouched — a takedown reaches the "
               "mirror, not the source.",
    }


def reinstate(conn, notice_id: int) -> dict:
    """Reverse a takedown. A person reversing a decision a person made.

    The anchor's takedown mark is cleared, its sources go back to serving and
    are queued for an immediate crawl so liveness is re-established by a real
    probe rather than assumed, and the reversal is a log entry pointing at the
    decision it reverses. Nothing about the original entry changes: a log that
    could forget a takedown could forget anything.
    """
    n = _notice(conn, notice_id)
    if n["action"] != "degraded":
        raise NotPending(f"notice {notice_id} is {n['action']}; only a takedown can be reinstated")

    seq = log_store.append(
        conn, "takedown",
        {
            "anchor": {"kind": n["kind"], "value": n["value"]},
            "reason_code": n["reason_code"],
            "action": "reinstated",
            "reverses": n["log_seq"],
            "means": "the decision at the sequence named is reversed; the mirror "
                     "serves again and the anchor may return to `live` on its next "
                     "confirmed probe",
        },
        anchor_id=n["anchor_id"],
    )
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE notice SET action = 'reinstated', reinstated_at = now(), "
            "reinstated_seq = %s WHERE id = %s", (seq, notice_id))
        # Another takedown may stand against the same anchor. The mark is
        # cleared only when this was the last one.
        cur.execute(
            "SELECT count(*) AS n FROM notice WHERE anchor_id = %s AND action = 'degraded'",
            (n["anchor_id"],))
        standing = cur.fetchone()["n"]
        if standing == 0:
            cur.execute(
                "UPDATE anchor SET taken_down_at = NULL, taken_down_seq = NULL WHERE id = %s",
                (n["anchor_id"],))
            cur.execute(
                "UPDATE source SET mirror_state = 'serving', next_fetch_at = now(), "
                "etag = NULL, last_modified = NULL WHERE anchor_id = %s",
                (n["anchor_id"],))

    return {
        "notice": notice_id,
        "action": "reinstated",
        "log_seq": seq,
        "reverses": n["log_seq"],
        "still_taken_down": standing > 0,
        "record": f"/log/entries?start={seq}&end={seq + 1}",
    }


def pending(conn, limit: int = 100, offset: int = 0) -> list[dict]:
    """What is waiting for a person, oldest first. The notifier is included:
    this is the operator's own listener, and a decision needs to know who is
    asking and how to answer them.

    **Oldest first and a hundred at a time hid every new notice behind a
    backlog.** The order is right, because a queue is worked from the front,
    but without an offset the hundred-and-first notice was not on a later page
    — it was on no page at all, and an operator holding a hundred undecided
    notices would never see one filed today. Found on 2026-09-15 because two
    cases that file a notice and then look for it stopped finding it, which is
    the same defect wearing a test's clothes: they were the operator, and the
    operator could not see it either.
    """
    with conn.cursor() as cur:
        cur.execute(
            "SELECT n.id, n.received_at, n.reason_code, n.notifier, n.good_faith_stated, "
            "       a.host, a.value "
            "FROM notice n JOIN anchor a ON a.id = n.anchor_id "
            "WHERE n.action = 'pending' ORDER BY n.received_at, n.id "
            "LIMIT %s OFFSET %s",
            (max(1, min(limit, 500)), max(0, offset)))
        return cur.fetchall()


def pending_count(conn) -> int:
    """How many are waiting, whatever one page of them shows. A list that
    cannot say how long it is cannot be paged through by whoever reads it."""
    with conn.cursor() as cur:
        cur.execute("SELECT count(*) AS n FROM notice WHERE action = 'pending'")
        return cur.fetchone()["n"]
