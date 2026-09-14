"""The OSS path: one static file at a location the claimant controls.

The bar is deliberately a single static file, and that is also the storage
requirement — **anyone who cannot serve a challenge file cannot be verified
anyway**, so there is no separate "developer without a repository" case to
solve. Git is preferred only because it brings history, review and pull
requests with it, not because it is required.

This rides in the same conditional-GET pass as the manifest, so re-verification
costs one extra request per project per cycle and needs no separate crawler.
"""
from __future__ import annotations

import ipaddress
import socket
import time
from urllib.parse import urlparse

import httpx

from ..config import ALLOW_LOOPBACK
from .result import Probed, Reason

CHALLENGE_PATH = ".well-known/podshl-challenge"
TIMEOUT = httpx.Timeout(connect=5.0, read=10.0, write=5.0, pool=5.0)
MAX_BYTES = 4096


def challenge_url(prefix: str) -> str:
    return f"{prefix.rstrip('/')}/{CHALLENGE_PATH}"


#: Ranges Python's own flags do not catch. `100.64.0.0/10` is carrier-grade NAT
#: and returns False for is_private, is_link_local and is_reserved alike — so on
#: any CGNAT network it would sail straight through a guard that trusts those
#: three. Found by a test asserting it should be refused, not by reading the
#: standard library.
EXTRA_REFUSED = (
    ipaddress.ip_network("100.64.0.0/10"),   # CGNAT (RFC 6598)
    ipaddress.ip_network("192.0.0.0/24"),    # IETF protocol assignments
    ipaddress.ip_network("192.0.2.0/24"),    # TEST-NET-1
    ipaddress.ip_network("198.51.100.0/24"),  # TEST-NET-2
    ipaddress.ip_network("203.0.113.0/24"),  # TEST-NET-3
    ipaddress.ip_network("64:ff9b::/96"),    # NAT64
)


def public_addresses(host: str) -> list[str]:
    """Resolve once, check every answer, and **return the addresses checked**.

    This is the sharpest hole in the whole design and `SERVER.md` does not
    mention it: ingest fetches URLs an attacker chose. Without this, an anchor
    pointing at 169.254.169.254 or 127.0.0.1 turns the crawler into a request
    forgery engine aimed at our own network.

    Returning the addresses rather than a verdict is the other half, and it was
    missing. A caller that asks "is this host public?" and then hands the *name*
    to an HTTP client has resolved twice: once to decide and once to connect.
    Between those two lookups a publisher-controlled name with a one-second TTL
    can answer with a public address and then with `169.254.169.254`, and every
    check passed. `fetch.py`'s own docstring already forbade this — "the
    connection is made to the address that was checked" — and the code did it
    anyway.

    Empty means refused. A host with no addresses is not reachable either, so
    the two cases collapse safely into one.
    """
    try:
        infos = socket.getaddrinfo(host, None)
    except socket.gaierror:
        return []
    out: list[str] = []
    for info in infos:
        literal = info[4][0]
        try:
            addr = ipaddress.ip_address(literal)
        except ValueError:
            return []
        if addr.is_loopback and ALLOW_LOOPBACK:
            # Deliberately switched on, and reported at `/` so it is visible.
            out.append(literal)
            continue
        if (addr.is_loopback or addr.is_private or addr.is_link_local
                or addr.is_reserved or addr.is_multicast or addr.is_unspecified):
            return []
        if any(addr in net for net in EXTRA_REFUSED if addr.version == net.version):
            return []
        out.append(literal)
    return out


def is_public_address(host: str) -> bool:
    """Every resolved address is acceptable. Kept as the readable form of the
    question; anything that then *connects* must use `public_addresses` and
    connect to what it got back."""
    return bool(public_addresses(host))


def classify(status: int, body: str, token: str, *, redirected_off_host: bool) -> Probed:
    """The mapping IS the design, so it is one readable function.

    `451` maps to REFUSED rather than ABSENT on purpose: a blocking
    intermediary is not the claimant's statement about their own file.
    """
    if redirected_off_host:
        return Probed(Reason.REDIRECTED_AWAY, {"status": status})
    if status in (404, 410):
        return Probed(Reason.ABSENT, {"status": status})
    if status in (401, 403, 429, 451):
        return Probed(Reason.REFUSED, {"status": status})
    if status >= 500:
        return Probed(Reason.UNREACHABLE, {"status": status})
    if status != 200:
        return Probed(Reason.UNREACHABLE, {"status": status})

    found = body.strip()
    if not found or len(found) > 512 or "\n" in found:
        return Probed(Reason.MALFORMED, {"status": status, "length": len(body)})
    if found != token:
        return Probed(Reason.CONTRADICTED, {"status": status}, value=found)
    return Probed(Reason.CONFIRMED, {"status": status}, value=found)


def probe(prefix: str, token: str, *, client: httpx.Client | None = None) -> Probed:
    url = challenge_url(prefix)
    host = urlparse(url).hostname or ""
    if not host:
        return Probed(Reason.INTERNAL, {"why": "no host in the recorded URL"})
    checked = public_addresses(host)
    if not checked:
        return Probed(Reason.REFUSED, {"why": "not a public address", "host": host})
    # Resolved once. Every request below connects to what was checked, so the
    # name cannot answer differently between the check and the connection.
    from ..ingest.fetch import DEADLINE_S, PIN, Deadline, Overflow, pinned_client, stream
    pin = {PIN: checked[0]}

    owned = client is None
    client = client or pinned_client()
    # The same bounds as a manifest fetch, for the same reason: this follows a
    # URL somebody else chose, and neither a body that never ends nor a host
    # that answers a byte a minute may hold the crawler.
    deadline = time.monotonic() + DEADLINE_S
    try:
        resp, body = stream(client, url, headers={}, pin=pin, cap=MAX_BYTES, deadline=deadline)
        # Redirects are followed by hand, and only while the host is unchanged.
        # A cross-host redirect would let a forge hand us somebody else's bytes,
        # which we would then serve under this anchor's name.
        hops = 0
        while resp.status_code in (301, 302, 303, 307, 308) and hops < 3:
            target = resp.headers.get("location", "")
            # `str(...)`, and that is the whole of a bug that made this branch
            # unreachable: `httpx.URL.join` returns a `URL`, `urlparse` wants a
            # string, and it raised `AttributeError` into the catch-all below —
            # so every redirect answered INTERNAL, and the cross-host guard two
            # lines down, which is a security check, had never run at all.
            # Nothing redirected until a forge did: Codeberg answers `/raw/HEAD/`
            # with a 303 to `/raw/branch/<name>/`, which is how it resolves HEAD.
            new_host = urlparse(str(httpx.URL(url).join(target))).hostname
            if new_host != host:
                return classify(resp.status_code, "", token, redirected_off_host=True)
            url = str(httpx.URL(url).join(target))
            resp, body = stream(client, url, headers={}, pin=pin, cap=MAX_BYTES,
                                deadline=deadline)
            hops += 1
        text = (body or b"").decode(resp.charset_encoding or "utf-8", errors="replace")
        return classify(resp.status_code, text, token, redirected_off_host=False)
    except Overflow:
        # A challenge file is one line. A body past the cap is not a challenge
        # file, and it is the claimant's file, so this is evidence.
        return Probed(Reason.MALFORMED, {"why": f"larger than {MAX_BYTES} bytes"})
    except Deadline:
        return Probed(Reason.UNREACHABLE, {"why": f"not complete within {DEADLINE_S:g} s"})
    except httpx.HTTPError as e:
        return Probed(Reason.UNREACHABLE, {"why": type(e).__name__})
    except Exception as e:  # noqa: BLE001
        return Probed(Reason.INTERNAL, {"why": type(e).__name__})
    finally:
        if owned:
            client.close()
