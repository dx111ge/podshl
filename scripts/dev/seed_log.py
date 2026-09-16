"""Give a brand-new operator a log with something in it.

**Why this exists.** Three cases ask the client to verify the operator's own
proofs — `SV69` that its signed index verifies, `AT2` that a real entry proves
against the signed head, `LC2` that consistency between two heads verifies.
None of them can hold on an operator whose log is empty, and a freshly created
database has exactly that. They passed for months on developers' machines
because those databases had months of entries in them; the first time this
suite ran on a clean one — in CI, on 2026-09-15 — all three failed, and so did
`RS1` behind them.

`SV69` says as much in its own message: *"nothing has been registered there, so
there is no entry for this case to verify. Not a client defect — claim an
anchor on that operator."* This is that, for a development operator: real
entries, appended through the server's own log, signed with its own key. It
fakes nothing. What it removes is the assumption that somebody has been using
this database for a while.

**Two heads, not one.** A consistency proof is between two sizes, so a head is
issued, more is appended, and a second head is issued. One head and three
entries would leave `LC2` with nothing to compare.

Development only. It is called from the container's entrypoint, and a
production operator's log gets its entries by attesting anchors.
"""

import sys

sys.path.insert(0, "src")

from podshl.server import db, log_store, sth  # noqa: E402

#: Enough for an inclusion proof and for a consistency proof between two heads.
SEED = [
    ("log_policy", {"note": "development operator seeded so its own proofs can be verified"}),
    ("log_policy", {"note": "a log with one entry cannot demonstrate consistency"}),
    ("log_policy", {"note": "and a proof needs a leaf that is not the head"}),
]


def main() -> int:
    with db.connect() as conn:
        before = log_store.tree_size(conn)
        if before >= len(SEED):
            print(f"  log already has {before} entries — not seeding")
            return 0

        log_store.append(conn, *SEED[0])
        first = sth.issue(conn)["sth"]["tree_size"]
        for kind, payload in SEED[1:]:
            log_store.append(conn, kind, payload)
        second = sth.issue(conn)["sth"]["tree_size"]
        conn.commit()

    print(f"  seeded the log: {before} -> {second} entries, heads at {first} and {second}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
