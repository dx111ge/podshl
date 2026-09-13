"""The enterprise path: a DNS TXT record under the domain.

A DNS record is far harder to obtain than a write into `/.well-known/` —
subdomain takeover, a CI job or a CDN misconfiguration all yield the latter.
That is the entire reason the two tiers use different challenges.

A DNSSEC-bogus answer maps to UNREACHABLE, never ABSENT. Treating a forged
answer as the claimant's statement would let an on-path attacker walk any
anchor to `unknown` in ninety days.
"""
from __future__ import annotations

from .result import Probed, Reason

RECORD = "_podshl.{domain}"
PREFIX = "podshl-challenge="


def parse(strings: list[str], token: str) -> Probed:
    for txt in strings:
        if not txt.startswith(PREFIX):
            continue
        rest = txt[len(PREFIX):]
        found, _, extra = rest.partition(";")
        found = found.strip()
        lei = None
        for part in extra.split(";"):
            part = part.strip()
            if part.startswith("lei="):
                lei = part[4:].strip()
        if found != token:
            return Probed(Reason.CONTRADICTED, {"lei": lei}, value=found)
        return Probed(Reason.CONFIRMED, {"lei": lei}, value=found)
    return Probed(Reason.ABSENT, {"records": len(strings)})


def probe(domain: str, token: str) -> Probed:
    try:
        import dns.resolver
        import dns.exception
    except ImportError:
        # Not "no record": we could not ask. The Python client used to return
        # None here, which reported a missing dependency as a vendor with no key.
        return Probed(Reason.INTERNAL, {"why": "dnspython is not installed"})

    name = RECORD.format(domain=domain)
    try:
        answers = dns.resolver.resolve(name, "TXT")
    except dns.resolver.NXDOMAIN:
        return Probed(Reason.ABSENT, {"rcode": "NXDOMAIN"})
    except dns.resolver.NoAnswer:
        return Probed(Reason.ABSENT, {"rcode": "NODATA"})
    except dns.resolver.NoNameservers:
        return Probed(Reason.UNREACHABLE, {"rcode": "SERVFAIL"})
    except dns.exception.Timeout:
        return Probed(Reason.UNREACHABLE, {"why": "timeout"})
    except Exception as e:  # noqa: BLE001
        return Probed(Reason.INTERNAL, {"why": type(e).__name__})

    strings = [b"".join(rr.strings).decode("utf-8", "replace") for rr in answers]
    return parse(strings, token)
