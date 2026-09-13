"""The LEI register, asked one record at a time and remembered for a day.

An enterprise anchor is attested from two halves: a DNS TXT record proves
control of the domain, and the register supplies the name (`enterprise.py`).
This is the second half.

**Asked, not mirrored.** The first version mirrored GLEIF's Golden Copy — the
whole register, 2.7 million records and half a gigabyte, reloaded often enough
that a copy older than a week could still answer. Nothing ever loaded it, and
the reason for it was weak: a lookup happens only when a vendor claims its
domain or is re-verified, never on a user's request; the LEI is public; and the
vendor asked to be checked, so telling GLEIF which LEI we looked up tells them
nothing the vendor has not already published. So each record is fetched from
GLEIF's public API when it is needed and kept in `lei_record` for a day.

**The cache is a cache.** A record younger than `CACHE_FOR` answers without a
request. Older, the register is asked again. If the register cannot be reached,
a record younger than `MAX_AGE` still answers and says it came from the cache;
older than that, nothing answers — an old record producing a confident refusal
would withdraw an attestation for an entity that is fine, which is the resolver
bug in another costume. A 404 is the register's own statement that it has no
such LEI, and is graded as absent.

**The binding, stated out loud.** GLEIF publishes no domain field. What ties a
LEI to a domain is that the TXT record *under that domain* asserts it — so the
attestation claims *"whoever controls this domain asserts this LEI, and this is
the name the register carries for it"*, and nothing stronger. The upgrade path
is vLEI, where a Legal Entity credential signs the assertion.
"""
from __future__ import annotations

import json
import re
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone

from .result import Probed, Reason

API = "https://api.gleif.org/api/v1/lei-records/"

#: A record this young answers without asking the register again.
CACHE_FOR = timedelta(days=1)

#: A record this young still answers when the register cannot be reached.
#: Older, the answer is "cannot tell" — never a refusal.
MAX_AGE = timedelta(days=7)

#: An LEI is 20 characters, letters and digits (ISO 17442). Anything else is
#: not asked about at all: it cannot be one, and it does not go into a URL.
LEI = re.compile(r"[0-9A-Z]{20}")


def is_lei(value: str) -> bool:
    """The shape, and the two check digits at the end (ISO 7064 MOD 97-10, as
    an IBAN): letters count as 10 to 35, and the whole number leaves 1 when
    divided by 97. A string of the right length is not yet an LEI, and a typo
    in a TXT record should be told so here rather than cost a request."""
    if not LEI.fullmatch(value):
        return False
    return int("".join(str(int(c, 36)) for c in value)) % 97 == 1


class Unreachable(Exception):
    """The register did not answer — our inability to ask, not its statement."""


def parse(document: dict) -> dict:
    """The fields we keep, from one API record. Everything else in it — the
    addresses, the officers' events, the relationships — stays with GLEIF: a
    register cache holding more than the name and the standing would be holding
    data we have no use for."""
    a = document["data"]["attributes"]
    entity, registration = a["entity"], a["registration"]
    published = ((document.get("meta") or {}).get("goldenCopy") or {}).get("publishDate") or ""
    return {
        "lei": a["lei"],
        "legal_name": (entity.get("legalName") or {}).get("name") or "",
        "entity_status": entity.get("status") or "",
        "reg_status": registration.get("status") or "",
        "country": ((entity.get("legalAddress") or {}).get("country") or None),
        # Which state of the register this came from, as the log entry names it.
        "file_id": f"api/{published}",
    }


def fetch(lei: str, timeout: int = 10) -> dict | None:
    """One record from GLEIF, or None when the register has no such LEI."""
    request = urllib.request.Request(API + lei, headers={"Accept": "application/vnd.api+json"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as r:
            return parse(json.loads(r.read(1 << 20)))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise Unreachable(f"the register answered HTTP {e.code}") from e
    except (urllib.error.URLError, TimeoutError, OSError, ValueError, KeyError) as e:
        raise Unreachable(f"the register could not be asked: {e}") from e


#: What `lookup` asks the register through. The suite replaces it, so no case
#: depends on GLEIF being up.
FETCH = fetch


def _grade(row: dict, *, from_cache: bool) -> Probed:
    """Failing to renew a LEI is an administrative age, not misconduct — the
    same shape as `stale` — so a lapsed registration is `confirmed` with the
    lapse in the detail. Only a registration the register itself has ended is
    `contradicted`."""
    lei = row["lei"]
    if row["reg_status"] in ("RETIRED", "ANNULLED", "DUPLICATE", "MERGED"):
        return Probed(Reason.CONTRADICTED, {"reg_status": row["reg_status"], "lei": lei})
    if row["entity_status"] not in ("ACTIVE", "NULL", ""):
        return Probed(Reason.CONTRADICTED, {"entity_status": row["entity_status"], "lei": lei})
    return Probed(
        Reason.CONFIRMED,
        {"reg_status": row["reg_status"], "file_id": row["file_id"],
         "lapsed": row["reg_status"] == "LAPSED", "from_cache": from_cache},
        value=row["legal_name"],
    )


def lookup(conn, lei: str) -> Probed:
    """What the register says about one LEI, asked only when the cache is old."""
    lei = (lei or "").strip().upper()
    if not is_lei(lei):
        return Probed(Reason.ABSENT, {"lei": lei, "why": "not an LEI: 20 letters and digits, "
                                                         "ending in two check digits"})

    with conn.cursor() as cur:
        cur.execute("SELECT lei, legal_name, entity_status, reg_status, file_id, loaded_at "
                    "FROM lei_record WHERE lei = %s", (lei,))
        cached = cur.fetchone()
    now = datetime.now(timezone.utc)
    if cached and now - cached["loaded_at"] < CACHE_FOR:
        return _grade(cached, from_cache=True)

    try:
        row = FETCH(lei)
    except Unreachable as e:
        if cached and now - cached["loaded_at"] < MAX_AGE:
            return _grade(cached, from_cache=True)
        why = (f"{e}, and the record we hold is {(now - cached['loaded_at']).days} days old"
               if cached else str(e))
        return Probed(Reason.UNREACHABLE, {"why": why, "lei": lei})

    with conn.cursor() as cur:
        if row is None:
            cur.execute("DELETE FROM lei_record WHERE lei = %s", (lei,))
            return Probed(Reason.ABSENT, {"lei": lei})
        cur.execute(
            "INSERT INTO lei_record (lei, legal_name, entity_status, reg_status, country, "
            "  file_id, loaded_at) VALUES (%s, %s, %s, %s, %s, %s, now()) "
            "ON CONFLICT (lei) DO UPDATE SET legal_name = EXCLUDED.legal_name, "
            "  entity_status = EXCLUDED.entity_status, reg_status = EXCLUDED.reg_status, "
            "  country = EXCLUDED.country, file_id = EXCLUDED.file_id, loaded_at = now()",
            (row["lei"], row["legal_name"], row["entity_status"], row["reg_status"],
             row["country"], row["file_id"]))
    return _grade(row, from_cache=False)
