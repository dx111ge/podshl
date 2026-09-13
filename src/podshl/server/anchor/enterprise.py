"""Attesting an enterprise anchor: DNS TXT, plus the register.

The two halves are separate on purpose and neither substitutes for the other.

* **The TXT record proves control of the domain.** A DNS record is used rather
  than a file because it is far harder to obtain: subdomain takeover, a CI job or
  a CDN misconfiguration all yield a write into `/.well-known/`, and none of them
  yields a zone edit.
* **The register supplies the name.** It is copied from GLEIF, never typed. There
  is no field anywhere in this system for an enterprise to write its own display
  name, which is what makes the impersonation attack structural rather than
  policed.

And the seam between them is stated rather than hidden: GLEIF publishes no domain
field, so what is attested is *"whoever controls this domain asserts this LEI,
and this is the name the register carries for it"*.

**We are not in the path.** An enterprise anchor gets an attestation and nothing
else — no mirror, no source, no fallback. Silently answering for a vendor whose
endpoint is down would mean answering with knowledge we do not have.
"""
from __future__ import annotations

from .. import log_store
from ..errors import ServerError
from . import dns_txt, gleif, sweep
from .result import Probed, Reason


class NotAttestable(ServerError):
    code = "not_attestable"


def attest(conn, host: str, *, resolver_probe: Probed | None = None) -> dict:
    """Verify the domain, look up the register, attest — or say which half failed.

    Which half failed matters: a DNS timeout is our inability to ask, a wrong
    token is their statement, and a retired LEI is the register's. Collapsing
    those into "could not attest" would leave a vendor with nothing to act on.
    """
    with conn.cursor() as cur:
        cur.execute("SELECT id, kind, value, host, challenge_token, attest_hold "
                    "FROM anchor WHERE host = %s AND kind = 'dns'", (host,))
        anchor = cur.fetchone()
    if not anchor:
        raise NotAttestable(f"no DNS anchor registered for {host}")
    if anchor["attest_hold"]:
        raise NotAttestable(
            f"held for review: {anchor['attest_hold']}. This is not a block — the "
            f"anchor is reachable exactly as one that never registered is.")

    # Explicitly against None. `resolver_probe or ...` is a truthiness test on a
    # Probed, which the type refuses — and it refused this line when it was
    # written that way.
    probed = (resolver_probe if resolver_probe is not None
              else dns_txt.probe(host, anchor["challenge_token"]))
    sweep.record(conn, anchor["id"], probed)
    if not probed.confirmed:
        raise NotAttestable(
            f"the TXT record at _podshl.{host} did not confirm: {probed.reason.value}. "
            + ("That is our failure to ask rather than yours to publish."
               if probed.is_silence else "")
        )

    lei = (probed.detail or {}).get("lei")
    if not lei:
        raise NotAttestable(
            "the TXT record confirms control of the domain but declares no LEI. "
            "Control is half of it; the name has to come from the register, "
            "because there is nowhere here to type one.")

    registered = gleif.lookup(conn, lei)
    if registered.reason is Reason.UNREACHABLE:
        raise NotAttestable(
            f"the register cannot answer for {lei}: "
            f"{registered.detail.get('why')}. Not a statement about you.")
    if not registered.confirmed:
        raise NotAttestable(
            f"the register does not support {lei}: {registered.reason.value} "
            f"{registered.detail}")

    seq = log_store.append(
        conn, "attestation_issued",
        {
            "anchor": {"kind": "dns", "value": anchor["value"]},
            "tier": "enterprise",
            "lei": lei,
            "legal_name": registered.value,
            "lei_file": registered.detail.get("file_id"),
            "claim": "whoever controls this domain asserts this LEI, and this is "
                     "the name the register carries for it",
            "not_claimed": "that the domain and the LEI are bound to each other by "
                           "anything stronger than that assertion",
        },
        anchor_id=anchor["id"],
    )

    with conn.cursor() as cur:
        cur.execute(
            "UPDATE attestation SET withdrawn_at = now(), withdrawn_kind = 'degraded', "
            "withdrawn_reason = 're_attested', withdrawn_seq = %s "
            "WHERE anchor_id = %s AND withdrawn_at IS NULL", (seq, anchor["id"]))
        cur.execute(
            "INSERT INTO attestation (anchor_id, tier, lei, legal_name, lei_file, "
            "  lei_checked, key_jwk, key_thumbprint, issued_seq) "
            "VALUES (%s, 'enterprise', %s, %s, %s, now(), '{}'::jsonb, %s, %s) RETURNING id",
            (anchor["id"], lei, registered.value, registered.detail.get("file_id"),
             __import__("hashlib").sha256(lei.encode()).digest(), seq))
        aid = cur.fetchone()["id"]
        cur.execute("UPDATE anchor SET status = 'live', last_confirmed = now(), "
                    "verified_at = COALESCE(verified_at, now()), failing_since = NULL "
                    "WHERE id = %s", (anchor["id"],))

    return {
        "attestation": aid,
        "tier": "enterprise",
        "lei": lei,
        "legal_name": registered.value,
        "lapsed": registered.detail.get("lapsed", False),
        "log_seq": seq,
        "we_are_not_in_the_path": "no mirror and no fallback. If their endpoint is "
                                  "down a client is told so, rather than given a "
                                  "guess with their name on it.",
    }
