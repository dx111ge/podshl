"""Appending is the only write.

An entry, once in, is never updated and never deleted. There is no code path
here that could, and that is deliberate: if we can remove things quietly, the
transparency is decorative. A takedown is itself an append.

Appends are serialised by an advisory lock rather than by a sequence. A sequence
leaves gaps on rollback, and a **gapless** index is what every proof is
arithmetic over — one missing number and every inclusion proof past it is wrong.

The leaves are JCS JSON rather than RFC 6962's TLS structs. That is a deliberate
deviation: this project already has one canonicalisation and one signature
scheme, both already implemented in Rust for the client, so a second byte format
would mean a second verifier in the client. The cost is that an off-the-shelf CT
monitor cannot read this log, and it is paid for by shipping
`spec/monitor/verify_log.py`.
"""
from __future__ import annotations

import json
from typing import Any

from .. import jcs
from . import merkle
from .errors import LogSealed

# One lock id for the whole log. Two appends racing would both compute the same
# next sequence and one would lose its place in the tree.
_LOG_LOCK = 8_721_000_1

KINDS = (
    "attestation_issued",
    "attestation_withdrawn",
    "anchor_state_changed",
    "key_changed",
    "takedown",
    "tier_flag_changed",
    "log_policy",
)


def canonical(payload: dict) -> bytes:
    """The exact bytes that get hashed and stored. JCS, so the ordering and
    escaping are fixed rather than whatever the serialiser felt like."""
    return jcs.canonicalize(payload)


def tree_size(conn) -> int:
    """`max(seq) + 1`, not `count(*)`. The two are equal because the index is
    gapless — that is the invariant every proof rests on — and only one of them
    is an index probe rather than a scan of every row on every request."""
    with conn.cursor() as cur:
        cur.execute("SELECT coalesce(max(seq) + 1, 0) AS n FROM log_entry")
        return cur.fetchone()["n"]


#: The most entries one page of the log returns. A page is a page.
MAX_PAGE = 1000


def _node_reader(conn):
    """Reads complete internal nodes. The ragged right edge is never stored, so
    a request for one is a bug rather than a cache miss."""
    def node(level: int, index: int) -> bytes:
        with conn.cursor() as cur:
            cur.execute("SELECT hash FROM log_node WHERE level = %s AND idx = %s",
                        (level, index))
            row = cur.fetchone()
        if row is None:
            raise LogSealed(f"log node ({level}, {index}) is missing — the tree is incomplete")
        return bytes(row["hash"])
    return node


def append(conn, kind: str, payload: dict, *, anchor_id: int | None = None) -> int:
    """Append one entry and return its sequence.

    Called inside the caller's transaction on purpose: an attestation row and
    the entry announcing it are one fact.
    """
    if kind not in KINDS:
        raise LogSealed(f"unknown log entry kind {kind!r} — permitted: {list(KINDS)}")

    with conn.cursor() as cur:
        cur.execute("SELECT pg_advisory_xact_lock(%s)", (_LOG_LOCK,))
        cur.execute("SELECT coalesce(max(seq) + 1, 0) AS n FROM log_entry")
        seq = cur.fetchone()["n"]

        entry = {"seq": seq, "v": 1, "kind": kind, **payload}
        data = canonical(entry)
        leaf = merkle.leaf_hash(data)

        cur.execute(
            "INSERT INTO log_entry (seq, kind, data, leaf_hash, anchor_id) "
            "VALUES (%s, %s, %s, %s, %s)",
            (seq, kind, data, leaf, anchor_id),
        )
        # Level 0 is the leaf itself.
        cur.execute("INSERT INTO log_node (level, idx, hash) VALUES (0, %s, %s)", (seq, leaf))

        # Then every block that just completed. Amortised O(1), worst case
        # O(log n): a node is written once, when its range fills, and never
        # touched again.
        level, n = 1, seq + 1
        while n % (1 << level) == 0:
            idx = (n >> level) - 1
            with conn.cursor() as c2:
                c2.execute("SELECT hash FROM log_node WHERE level = %s AND idx IN (%s, %s) "
                           "ORDER BY idx", (level - 1, idx * 2, idx * 2 + 1))
                pair = [bytes(r["hash"]) for r in c2.fetchall()]
            if len(pair) != 2:
                raise LogSealed(f"cannot complete node ({level}, {idx}): children missing")
            cur.execute("INSERT INTO log_node (level, idx, hash) VALUES (%s, %s, %s)",
                        (level, idx, merkle.node_hash(pair[0], pair[1])))
            level += 1
    return seq


def root(conn) -> bytes:
    n = tree_size(conn)
    return merkle.root(_node_reader(conn), n) if n else merkle.empty_root()


def check_page(start: int, end: int | None, limit: int) -> None:
    """The ranges a page may ask for. A negative start, an end before the
    start, or a limit outside 1..MAX_PAGE is a caller's mistake and is told so,
    rather than being passed to the database to fail with a message about
    something else."""
    if start < 0:
        raise ValueError(f"start {start} is negative")
    if end is not None and end < start:
        raise ValueError(f"end {end} is before start {start}")
    if not 1 <= limit <= MAX_PAGE:
        raise ValueError(f"limit {limit} is not between 1 and {MAX_PAGE}")


def entries(conn, start: int = 0, end: int | None = None, limit: int = MAX_PAGE) -> list[dict]:
    """A page of the log, as it was written. `data` is returned verbatim; a
    re-serialisation differing by one byte would invalidate every proof."""
    check_page(start, end, limit)
    with conn.cursor() as cur:
        if end is None:
            cur.execute("SELECT seq, kind, data FROM log_entry WHERE seq >= %s "
                        "ORDER BY seq LIMIT %s", (start, limit))
        else:
            cur.execute("SELECT seq, kind, data FROM log_entry WHERE seq >= %s AND seq < %s "
                        "ORDER BY seq LIMIT %s", (start, end, limit))
        return [
            {"seq": r["seq"], "kind": r["kind"], "entry": json.loads(bytes(r["data"]))}
            for r in cur.fetchall()
        ]


def leaf_hashes(conn, start: int = 0, end: int | None = None) -> list[bytes]:
    with conn.cursor() as cur:
        if end is None:
            cur.execute("SELECT leaf_hash FROM log_entry WHERE seq >= %s ORDER BY seq", (start,))
        else:
            cur.execute("SELECT leaf_hash FROM log_entry WHERE seq >= %s AND seq < %s "
                        "ORDER BY seq", (start, end))
        return [bytes(r["leaf_hash"]) for r in cur.fetchall()]


def inclusion(conn, seq: int, size: int | None = None) -> dict[str, Any]:
    """PATH(seq, D[size]). `size` defaults to the current tree.

    A client that fetched a signed head and then asked for a proof got the proof
    for whatever the tree had grown to in between — against a root it had no
    signature for. Asking for the head's own size is what makes the two agree.
    Every complete block of a prefix is already stored, so any size up to the
    current one can be proved.
    """
    n = tree_size(conn)
    if size is not None:
        if not 0 < size <= n:
            raise ValueError(f"tree size {size} is not between 1 and {n}")
        n = size
    path = merkle.inclusion_path(_node_reader(conn), seq, n)
    return {"leaf_index": seq, "tree_size": n, "path": [p.hex() for p in path]}


def consistency(conn, first: int, second: int | None = None) -> dict[str, Any]:
    """PROOF(first, D[second]). Both sizes are checked against the tree here,
    because `merkle` only knows the arithmetic: a `second` past the current
    size would ask the node reader for nodes that do not exist and surface as
    a sealed-log error, which is the wrong accusation for a typo."""
    current = tree_size(conn)
    n = current if second is None else second
    if first < 0 or n < 0:
        raise ValueError("tree sizes are not negative")
    if n > current:
        raise ValueError(f"tree size {n} is past the current size {current}")
    if first > n:
        raise ValueError(f"cannot prove {first} is a prefix of {n}")
    path = merkle.consistency_path(_node_reader(conn), first, n)
    return {"first": first, "second": n, "path": [p.hex() for p in path]}


def for_anchor(conn, anchor_id: int) -> list[dict]:
    """Everything the log says about one anchor. This is what makes every
    claimed anchor an authenticated monitor of its own entries — the population
    that most cares becomes the population that watches."""
    with conn.cursor() as cur:
        cur.execute("SELECT seq, kind, data FROM log_entry WHERE anchor_id = %s ORDER BY seq",
                    (anchor_id,))
        return [
            {"seq": r["seq"], "kind": r["kind"], "entry": json.loads(bytes(r["data"]))}
            for r in cur.fetchall()
        ]
