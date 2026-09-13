"""Clusters: the class comes from the signature, never from free text.

An earlier draft of the catch-all took a truncated problem sentence as the
class. That breaks the rule the rest of the design obeys — two reports with the
same readings and the same failed actions are the same class, and a sentence is
neither. Free text may travel, but only under its own explicit consent and with
the destination named.

The signature's hash is the cluster's identity, and nothing more public than
that. It was once also the path of `GET /cluster/<hash>`, a cacheable lookup
meant to keep common queries away from the origin; it answered a project's
report counts to anybody who could guess a configuration, which is a small
search, and it was removed (`SV104`).
"""
from __future__ import annotations

import hashlib
import json

from .. import jcs


def canonical_signature(subject: str, observed: dict, failed_actions: list | None = None) -> dict:
    """What two reports must share to be the same problem.

    Sorted, coarsened values only — the client has already applied the
    generalisation policy, so nothing here needs to know that a serial exists.
    """
    return {
        "subject": subject,
        "observed": {k: observed[k] for k in sorted(observed)},
        "failed_actions": sorted(failed_actions or []),
    }


def signature_hash(signature: dict) -> bytes:
    """The cluster's identity, unique in the table. JCS so it stays stable if a
    second implementation ever has to compute it."""
    return hashlib.sha256(jcs.canonicalize(signature)).digest()


def derive_class(signature: dict) -> str:
    """The problem class, derived rather than supplied.

    Deliberately opaque and deliberately not a sentence: it is an identity for
    grouping, and anything human-readable here would be a free-text field
    wearing a different hat.
    """
    shape = "+".join(sorted(signature.get("observed", {})))
    return f"sig.{hashlib.sha256(shape.encode()).hexdigest()[:12]}"


def shape(signature: dict) -> list[str]:
    """The field names carried. Two reports are comparable only if their shapes
    match — a shape change is a new cluster, not a merge into an old one."""
    return sorted(signature.get("observed", {}))


def find(conn, sig_hash: bytes) -> dict | None:
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, subject_kind, subject_host, source_id, problem_class, "
            "       reports_total, peak_epoch_reporters "
            "FROM cluster WHERE signature_hash = %s", (sig_hash,))
        return cur.fetchone()


def ensure(conn, signature: dict, *, subject_host: str | None = None,
           source_id: int | None = None, epoch: int) -> int:
    """Find or create the cluster for this signature."""
    sig_hash = signature_hash(signature)
    found = find(conn, sig_hash)
    if found:
        return found["id"]

    kind = "source" if source_id is not None else "domain"
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO cluster (subject_kind, source_id, subject_host, problem_class, "
            "  signature_shape, signature_hash, signature, first_epoch, last_epoch) "
            "VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s) "
            "ON CONFLICT (signature_hash) DO UPDATE SET last_epoch = EXCLUDED.last_epoch "
            "RETURNING id",
            (kind, source_id, subject_host, derive_class(signature), shape(signature),
             sig_hash, json.dumps(signature), epoch, epoch),
        )
        return cur.fetchone()["id"]
