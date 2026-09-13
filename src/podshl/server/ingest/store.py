"""Writing a mirror version is a supersede, never an overwrite.

**Publish the commit we are serving**, so anyone can check the mirror against
the source. That only means something if what was served yesterday can still be
identified: an UPDATE in place would make the claim unfalsifiable, which is the
same objection as a log that can be quietly edited.

So a new version closes the old row (`valid_to`) and inserts a new one. The
partial unique indexes on `card` and `solution` make "the current version" a
trivial query and make two current versions impossible.
"""
from __future__ import annotations

import json

from .. import log_store
from ..errors import IngestRefused


def _content_hash(fetched) -> bytes:
    h = fetched.content_hash()
    if h is None:
        raise IngestRefused("nothing was fetched")
    return h


def store_card(conn, source_id: int, manifest: dict, content_hash: bytes,
               commit: str | None) -> tuple[int, bool]:
    """Store the card. Returns (id, changed).

    Unchanged content is not a new version — otherwise every ingest cycle would
    append a row and the history would record polling rather than publishing.
    """
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, content_hash FROM card WHERE source_id = %s AND valid_to IS NULL",
            (source_id,))
        current = cur.fetchone()
        if current and bytes(current["content_hash"]) == content_hash:
            return current["id"], False

        if current:
            cur.execute("UPDATE card SET valid_to = now() WHERE id = %s", (current["id"],))

        cur.execute(
            "INSERT INTO card (source_id, json, langs, endpoint_url, commit, content_hash) "
            "VALUES (%s, %s, %s, %s, %s, %s) RETURNING id",
            (source_id, json.dumps(manifest), manifest["langs"],
             manifest["endpoint"], commit, content_hash),
        )
        return cur.fetchone()["id"], True


def store_solution(conn, source_id: int, solution: dict, content_hash: bytes,
                   commit: str | None, *, etag: str | None = None,
                   last_modified: str | None = None) -> bool:
    """Store one solution. Returns whether it changed.

    The validators the origin sent ride along with the row, so the next cycle
    can ask about this file conditionally even when the manifest said 304.
    """
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, content_hash FROM solution "
            "WHERE source_id = %s AND solution_id = %s AND valid_to IS NULL",
            (source_id, solution["id"]))
        current = cur.fetchone()
        if current and bytes(current["content_hash"]) == content_hash:
            # Unchanged content, possibly a new validator: keep what the origin
            # said last, so the next conditional GET can be conditional.
            cur.execute("UPDATE solution SET etag = COALESCE(%s, etag), "
                        "last_modified = COALESCE(%s, last_modified) WHERE id = %s",
                        (etag, last_modified, current["id"]))
            return False
        if current:
            cur.execute("UPDATE solution SET valid_to = now() WHERE id = %s", (current["id"],))

        cur.execute(
            "INSERT INTO solution (source_id, solution_id, answers, proposes, text_by_lang, "
            "  severity, path, commit, content_hash, etag, last_modified) "
            "VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)",
            (source_id, solution["id"], json.dumps(solution["answers"]),
             json.dumps(solution["proposes"]), json.dumps(solution["text_by_lang"]),
             solution.get("severity"), solution["path"], commit, content_hash,
             etag, last_modified),
        )
        return True


def current_card(conn, source_id: int) -> dict | None:
    """The manifest as it was last stored, for a cycle in which the origin said
    the manifest is unchanged and the solutions still have to be asked about."""
    with conn.cursor() as cur:
        cur.execute("SELECT id, json, content_hash, commit FROM card "
                    "WHERE source_id = %s AND valid_to IS NULL", (source_id,))
        return cur.fetchone()


def current_solutions(conn, source_id: int) -> dict[str, dict]:
    """The live solutions by path, each with what the origin last said about
    it, so a conditional GET per file has something to be conditional on."""
    with conn.cursor() as cur:
        cur.execute(
            "SELECT solution_id, answers, proposes, text_by_lang, severity, path, "
            "       content_hash, etag, last_modified "
            "FROM solution WHERE source_id = %s AND valid_to IS NULL", (source_id,))
        return {r["path"]: r for r in cur.fetchall()}


def retract_missing_solutions(conn, source_id: int, kept: list[str]) -> list[str]:
    """Close every live solution this source no longer publishes.

    **Deleting the file was how a maintainer withdrew a solution, and it did
    nothing.** Ingest iterated the manifest's `solutions` list and stored each
    one; the only `valid_to` was the supersede path *inside* `store_solution`,
    which runs only for solutions still listed. So a remedy a maintainer decided
    was harmful — the natural reason to remove one — went on being served from
    the mirror indefinitely, under an anchor re-attested for a manifest that no
    longer declared it.

    Closed rather than deleted, exactly like a card being superseded: the
    history stays, so "what were we serving on the day this went wrong" remains
    an answerable question.

    Returns the ids retracted, so the crawl result can say so rather than
    leaving it to be noticed.
    """
    with conn.cursor() as cur:
        if kept:
            cur.execute(
                "UPDATE solution SET valid_to = now() "
                "WHERE source_id = %s AND valid_to IS NULL AND solution_id <> ALL(%s) "
                "RETURNING solution_id",
                (source_id, kept))
        else:
            cur.execute(
                "UPDATE solution SET valid_to = now() "
                "WHERE source_id = %s AND valid_to IS NULL RETURNING solution_id",
                (source_id,))
        return [r["solution_id"] for r in cur.fetchall()]


def withdraw(conn, source_id: int) -> None:
    """Stop serving, without deleting.

    A takedown degrades rather than deletes: the mirror goes, and the source in
    the developer's repository stays. What is withheld here is precisely the
    state of a project that never registered — not death, un-enrolment.
    """
    with conn.cursor() as cur:
        cur.execute("UPDATE source SET mirror_state = 'withheld' WHERE id = %s", (source_id,))


def record_ingest(conn, source_id: int, fetched, *, changed: bool,
                  declared_status: str | None = None,
                  successor: str | None = None,
                  remember_validators: bool = True) -> None:
    """Update the source's polling state.

    The backoff is not politeness for its own sake: their rate limits become our
    capacity if we ignore them, and a source unchanged for a month does not need
    asking every fifteen minutes.

    `remember_validators` is False when the content was fetched but **refused**.
    Storing the ETag there would mean the next cycle sends `If-None-Match`, gets
    a 304, and reports the source as unchanged — so a document that failed
    validation once would be recorded as fine forever, and the refusal would
    never be re-examined after the developer fixed it.
    """
    with conn.cursor() as cur:
        if fetched.reason.value in ("unreachable", "refused", "internal"):
            cur.execute(
                "UPDATE source SET last_fetched = now(), "
                "  consecutive_silence = consecutive_silence + 1, "
                "  next_fetch_at = now() + (interval '15 minutes' "
                "    * LEAST(power(2, consecutive_silence + 1), 96)) "
                "WHERE id = %s", (source_id,))
            return

        cur.execute(
            "UPDATE source SET last_fetched = now(), consecutive_silence = 0, "
            "  etag = CASE WHEN %s THEN COALESCE(%s, etag) ELSE NULL END, "
            "  last_modified = CASE WHEN %s THEN COALESCE(%s, last_modified) ELSE NULL END, "
            "  content_hash = COALESCE(%s, content_hash), "
            "  last_changed = CASE WHEN %s THEN now() ELSE last_changed END, "
            "  declared_status = COALESCE(%s, declared_status), "
            "  successor_url = COALESCE(%s, successor_url), "
            "  next_fetch_at = now() + interval '15 minutes' "
            "WHERE id = %s",
            (remember_validators, fetched.etag,
             remember_validators, fetched.last_modified,
             fetched.content_hash(), changed, declared_status, successor, source_id),
        )


def attest(conn, source_id: int, anchor_id: int, anchor: dict, commit: str | None,
           content_hash: bytes) -> int:
    """Append the attestation entry.

    Precise about what it claims: not *"this skill is safe"* but *"published by
    whoever controlled this anchor at time T, from commit C"*. That is a weaker
    statement than the enterprise one, and correctly so.
    """
    return log_store.append(
        conn, "attestation_issued",
        {
            "anchor": {"kind": anchor["kind"], "value": anchor["value"]},
            "tier": "oss",
            "commit": commit,
            "content_sha256": content_hash.hex(),
            "claim": "published by whoever controlled this anchor at time T, from commit C",
        },
        anchor_id=anchor_id,
    )
