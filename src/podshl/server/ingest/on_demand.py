"""What a request does before it is served from a source (`INGEST-REDESIGN.md`).

The query path still serves from the database. What changed is that the
database is no longer kept warm for projects nobody asks about, so a request
has to say it asked:

* **hot, checked within the hour** — served, nothing else happens;
* **hot, checked longer ago** — served at once, and the source is put at the
  front of the worker's queue (`next_fetch_at` at the epoch);
* **cold** — checked here, before anything is served, in a bounded pool. A
  withdrawn solution is removed by that check rather than handed out, which is
  the reason a cold source fails closed (`SV128`).

**What this leaves behind** is one date per source, the day it was last used,
written at most once a day in its own short transaction — never from inside the
query's read-only one (`SV132`). No time, no count, nobody's address. The
check a use triggers has a time of its own, and that says to within the hour
when *somebody* used the project; `SERVER.md` says so in those words.

**Protection.** Anybody can walk the signed index and ask about every cold
project in turn, so: `POOL_SIZE` checks at a time, one per source (concurrent
requests wait for the same one), a request waits `WAIT_S` and is then told the
project is being loaded, and a failed check backs off per source — a minute,
doubling to an hour — during which a request is answered without a fetch
(`SV130`). The pool is per process; an operator running several web workers
has that many pools, which is still a bound.
"""
from __future__ import annotations

import threading
from concurrent.futures import Future, ThreadPoolExecutor
from concurrent.futures import TimeoutError as FutureTimeout
from dataclasses import dataclass

from .. import db, log_store, sth
from . import fetch, scheduler

POOL_SIZE = 4
WAIT_S = 5.0
BACKOFF_FIRST_S = 60
BACKOFF_MAX_S = 3600

_pool = ThreadPoolExecutor(max_workers=POOL_SIZE, thread_name_prefix="podshl-check")
_inflight: dict[int, Future] = {}
_lock = threading.Lock()

#: What counts as a check that succeeded.
OK = ("stored", "unchanged")


@dataclass(frozen=True)
class Unavailable:
    """Why a request is not served from this source right now.

    `code` is one of `loading` (a check is running, ask again shortly),
    `cannot_check` (the project's files could not be read) and `cannot_use`
    (they were read and refused). None of them says the project publishes
    nothing — that is a different answer, and a worse one to give wrongly.
    """
    code: str
    reason: str
    retry_after: int

    def body(self) -> dict:
        return {"code": self.code, "reason": self.reason, "retry_after": self.retry_after,
                "note": "this is about reading the project's current files, not about "
                        "whether the project publishes anything"}


def _check(source_id: int) -> dict:
    """Check one source now, in its own transaction, and write what it means.

    Takes the row with `SKIP LOCKED`: a worker already checking it is the same
    check, and waiting for its batch to commit could take minutes.
    """
    with db.tx() as conn:
        with conn.cursor() as cur:
            cur.execute(f"SELECT {scheduler.SOURCE_COLUMNS} FROM source "
                        "WHERE id = %s AND mirror_state = 'serving' "
                        "FOR UPDATE SKIP LOCKED", (source_id,))
            source = cur.fetchone()
        if source is None:
            return {"source": source_id, "outcome": "busy"}
        before = log_store.tree_size(conn)
        try:
            with conn.transaction():
                # Full and with the anchor, as a load from cold always is: the
                # stored version is two weeks old and nothing has vouched for
                # it since.
                with fetch.pinned_client() as client:
                    result = scheduler.ingest_one(conn, source, client=client,
                                                  full=True, probe=True)
        except Exception as e:  # noqa: BLE001 - one source, not the request
            result = {"source": source_id, "outcome": "error",
                      "why": f"{type(e).__name__}: {e}"}
        with conn.cursor() as cur:
            if result["outcome"] in OK:
                # Checked successfully, so it is hot from today.
                cur.execute("UPDATE source SET last_used = current_date "
                            "WHERE id = %s AND last_used < current_date", (source_id,))
            else:
                cur.execute(
                    "UPDATE source SET check_failures = check_failures + 1, "
                    "  check_backoff_until = now() + LEAST(%s * power(2, check_failures), %s) "
                    "    * interval '1 second' "
                    "WHERE id = %s", (BACKOFF_FIRST_S, BACKOFF_MAX_S, source_id))
        if log_store.tree_size(conn) != before:
            sth.issue(conn)
    return result


def _submit(source_id: int) -> Future:
    with _lock:
        running = _inflight.get(source_id)
        if running is not None:
            return running
        future = _pool.submit(_check, source_id)
        _inflight[source_id] = future

    def done(_):
        with _lock:
            if _inflight.get(source_id) is future:
                del _inflight[source_id]
    future.add_done_callback(done)
    return future


def check_now(source_id: int, wait: float = WAIT_S) -> dict | None:
    """Check a source in the pool and wait up to `wait` seconds for the result.

    None when it has not finished; it carries on regardless.
    """
    try:
        return _submit(source_id).result(timeout=wait)
    except FutureTimeout:
        return None


def _unavailable(result: dict | None) -> Unavailable:
    if result is None or result["outcome"] == "busy":
        return Unavailable("loading", "this project's files are being loaded; "
                                      "ask again in a moment", 5)
    if result["outcome"] == "refused" and result.get("why"):
        return Unavailable("cannot_use", "this project's current files could not be "
                                         "accepted, so nothing from it is served until "
                                         "they are; its maintainer is told why", BACKOFF_FIRST_S)
    return Unavailable("cannot_check", "this project's current files cannot be checked "
                                       "right now, so nothing from it is served", BACKOFF_FIRST_S)


def before_serving(source_id: int) -> Unavailable | None:
    """None when the stored version may be served; otherwise why not."""
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT last_used >= current_date - %s AS hot, "
                "       last_used < current_date AS touch, "
                "       COALESCE(last_checked >= now() - %s * interval '1 minute', false) AS recent, "
                "       COALESCE(check_backoff_until > now(), false) AS backing_off, "
                "       last_refusal IS NOT NULL AS refused, "
                "       GREATEST(0, EXTRACT(EPOCH FROM check_backoff_until - now()))::int AS wait "
                "FROM source WHERE id = %s",
                (scheduler.HOT_DAYS, scheduler.RECHECK_ON_USE_MINUTES, source_id))
            row = cur.fetchone()
    if row is None:
        return _unavailable({"outcome": "unreachable"})

    if row["hot"]:
        if row["touch"] or not row["recent"]:
            # Its own transaction, after the read one has closed. The date is
            # written once a day; the queueing is a no-op once queued.
            with db.tx() as conn:
                with conn.cursor() as cur:
                    if row["touch"]:
                        cur.execute("UPDATE source SET last_used = current_date "
                                    "WHERE id = %s AND last_used < current_date", (source_id,))
                    if not row["recent"]:
                        # The front of the queue: ahead of anything a timer
                        # made due, which can wait for a person who is asking.
                        cur.execute("UPDATE source SET next_fetch_at = 'epoch' "
                                    "WHERE id = %s AND next_fetch_at > 'epoch'", (source_id,))
        return None

    # Cold. `last_used` is not touched until a check succeeds, so a check that
    # failed leaves the source cold, and the next request asks again rather
    # than being served what nobody has vouched for in two weeks.
    if row["backing_off"]:
        u = _unavailable({"outcome": "refused" if row["refused"] else "unreachable"})
        return Unavailable(u.code, u.reason, max(1, row["wait"]))
    result = check_now(source_id)
    if result is not None and result["outcome"] in OK:
        return None
    return _unavailable(result)
