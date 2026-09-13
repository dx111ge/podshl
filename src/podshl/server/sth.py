"""The signed tree head — the only thing we ask anyone to trust.

Signed with the same scheme as everything else here: EdDSA detached JWS over the
JCS-canonical form. That is deliberate reuse rather than laziness — the client
already verifies exactly this in Rust, so a monitor written against our log
needs no code we have not already shipped and tested on both sides.

Issuing is idempotent per tree size. Re-signing the same size would produce a
second timestamp over the same root, and a monitor comparing two heads has no
way to tell that from a split view.

**What was signed is what is served.** The body used to be rebuilt from the row
on every read, with the *current* key's id in it, so after a rotation every
stored head would have been served with a body its signature never covered. The
signed body is stored beside the signature now; rows from before that column
existed are rebuilt with the key id recorded at signing time, which is the id
the signature actually names.
"""
from __future__ import annotations

import hashlib
import json
import time
from functools import lru_cache

from ..jws import load_or_create_key, public_jwk, sign_detached, verify_detached
from . import log_store
from .config import LOG_KEY, LOG_KEY_CREATE


@lru_cache(maxsize=1)
def key():
    """Read once. The file does not change under a running process — rotation
    is a restart and a log entry — and reading a PEM on every request was the
    hottest thing `/index` did.

    Not created unless `PODSHL_LOG_KEY_CREATE=1`. A host that comes up without
    its key refuses to sign rather than quietly starting a second log.
    """
    return load_or_create_key(LOG_KEY, create=LOG_KEY_CREATE)


@lru_cache(maxsize=1)
def log_id() -> str:
    """SHA-256 of the public key, so a monitor can pin what it is checking. An
    unpinned key makes the whole exercise decorative."""
    jwk = public_jwk(key())
    return hashlib.sha256(json.dumps(jwk, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def body(tree_size: int, root_hash: bytes, timestamp_ms: int, *,
         signed_by: str | None = None) -> dict:
    return {
        "log_id": signed_by or log_id(),
        "schema_version": 1,
        "tree_size": tree_size,
        "root_hash": root_hash.hex(),
        "timestamp": timestamp_ms,
        "hash_algorithm": "SHA-256",
        "signature_algorithm": "EdDSA",
    }


def _from_row(row: dict) -> dict:
    if row.get("body"):
        head = row["body"]
    else:
        # Written before the body was stored. `key_id` is what the signature
        # was made with, so rebuilding under it is rebuilding what was signed.
        head = body(row["tree_size"], bytes(row["root_hash"]), row["timestamp_ms"],
                    signed_by=row["key_id"])
    return {"sth": head, "signature": row["signature"]}


def issue(conn) -> dict:
    """Sign the current head, or return the one already signed for this size."""
    size = log_store.tree_size(conn)
    with conn.cursor() as cur:
        cur.execute("SELECT tree_size, root_hash, timestamp_ms, signature, key_id, body "
                    "FROM sth WHERE tree_size = %s", (size,))
        row = cur.fetchone()
    if row:
        return _from_row(row)

    root = log_store.root(conn)
    ts = int(time.time() * 1000)
    head = body(size, root, ts)
    sig = sign_detached(key(), head, kid=log_id())
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO sth (tree_size, root_hash, timestamp_ms, signature, key_id, body) "
            "VALUES (%s, %s, %s, %s, %s, %s) ON CONFLICT (tree_size) DO NOTHING",
            (size, root, ts, json.dumps(sig), log_id(), json.dumps(head)),
        )
    return {"sth": head, "signature": sig}


def current(conn) -> dict:
    with conn.cursor() as cur:
        cur.execute("SELECT tree_size, root_hash, timestamp_ms, signature, key_id, body "
                    "FROM sth ORDER BY tree_size DESC LIMIT 1")
        row = cur.fetchone()
    if row is None:
        return issue(conn)
    return _from_row(row)


def verify(head: dict, signature: dict, jwk: dict) -> bool:
    ok, _ = verify_detached(jwk, head, signature)
    return ok
