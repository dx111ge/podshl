"""Enrol engram: prove control, say where the files are, and run one crawl pass.

Everything here is the shipped path except one line, and that line is named
rather than hidden.

`POST /claim/{host}` mints an anchor whose value is `https://{host}/`, and this
demo host speaks plain HTTP on loopback — which the database's own carve-out
permits and that route cannot produce. So the claim is seeded directly onto the
loopback anchor row. Everything after it is real: the probe fetches the file
over HTTP, `verify` checks the proof and issues the token, `POST
/claim/{host}/source` enrols the mirror, and the scheduler validates and stores.

Run it with the challenge server *not yet started*: it prints the value to
publish, waits for you to serve it, and then verifies.

    python harness/enrol.py
"""
from __future__ import annotations

import hashlib
import json
import secrets
import sys
import urllib.request

sys.path.insert(0, "/app/src")

from podshl.server import db  # noqa: E402

SERVER = "http://127.0.0.1:8725"
HOST = "engram.localhost"
BASE = "http://127.0.0.1:8728/"


def post(path: str, headers: dict | None = None, body: dict | None = None) -> dict:
    req = urllib.request.Request(
        SERVER + path,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Content-Type": "application/json", **(headers or {})},
        method="POST")
    with urllib.request.urlopen(req, timeout=15) as r:
        return json.load(r)


# 1 — the claim, in two halves. The digest is published; the preimage is kept
#     and is what makes this claim ours rather than any passer-by's.
proof = secrets.token_urlsafe(32)
digest = hashlib.sha256(proof.encode()).hexdigest()

with db.tx() as conn:
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM anchor WHERE host = %s AND value = %s",
                    (HOST, BASE))
        row = cur.fetchone()
        if row:
            aid = row["id"]
        else:
            cur.execute(
                "INSERT INTO anchor (kind, value, host, challenge_token) "
                "VALUES ('url', %s, %s, %s) RETURNING id", (BASE, HOST, digest))
            aid = cur.fetchone()["id"]
        # A claim is a row of its own, and several may stand for one anchor:
        # seeding one here does not disturb anybody else's (`0012`).
        cur.execute("INSERT INTO claim_pending (anchor_id, nonce_hash) VALUES (%s, %s)",
                    (aid, hashlib.sha256(proof.encode()).digest()))

print(f"anchor {aid}")
print()
print("  publish this at /.well-known/podshl-challenge :")
print(f"    {digest}")
print()
print("  start the host with it, then press Return:")
print(f"    ENGRAM_CHALLENGE_TOKEN={digest} \\")
print("      python -m uvicorn serve:app --host 0.0.0.0 --port 8728")
input()

# 2 — verify. The file must carry the digest and this caller must hold the
#     preimage; either one alone is not a claim.
claimed = post(f"/claim/{HOST}/verify", headers={"X-Podshl-Claim-Proof": proof})
token = claimed["token"]
print(f"claimed: {claimed['claimed']}  (superseded {claimed['superseded']})")

# 3 — say where the files are. Proving control does not say that, and until it
#     is said nothing is fetched.
enrolled = post(f"/claim/{HOST}/source", headers={"X-Podshl-Claim": token},
                body={"prefix": BASE})
print(f"enrolled: {enrolled['manifest_url']}  (new: {enrolled['created']})")

# 4 — one crawl pass.
from podshl.server.ingest import scheduler  # noqa: E402

with db.tx() as conn:
    with conn.cursor() as cur:
        cur.execute("UPDATE source SET next_fetch_at = now(), etag = NULL, "
                    "last_modified = NULL WHERE anchor_id = %s", (aid,))
    for r in scheduler.run_once(conn):
        print("ingest:", r)

print()
print(f"dashboard token (shown once): {token}")
print(f"  open {SERVER}/dashboard#{HOST}")
