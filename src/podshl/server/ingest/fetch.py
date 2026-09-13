"""Fetching, and the rules that bound it.

**Mirror, never proxy.** Fetching from a forge on the request path would be
wrong twice over: their rate limits become our capacity, and their outage
becomes ours. So this runs on a schedule and everything served comes from our
own database. The property that makes it scale is that ingest load tracks the
number of projects and request load tracks the number of users, and the two are
independent.

**This function fetches URLs an attacker chose**, which is the sharpest hole in
the design and one `SERVER.md` does not mention. Two rules follow:

* The host is resolved first, every resolved address is checked, and the
  connection is made to the address that was checked. Resolving twice — once to
  validate and once to connect — is a DNS-rebinding hole straight into our own
  network.
* Redirects are followed by hand, and only while the host is unchanged and the
  target is still under the recorded prefix. A cross-host redirect would let a
  forge hand us somebody else's bytes, which we would then serve under this
  anchor's name and sign.
"""
from __future__ import annotations

import hashlib
import time
from dataclasses import dataclass, field
import posixpath
from urllib.parse import unquote, urlparse

import httpx

from ..anchor.result import Probed, Reason
from ..anchor.challenge import public_addresses

#: A manifest is a manifest. Anything larger is not one, and a body we would
#: have to stream is a body we should not be storing.
MAX_BYTES = 1 << 20

#: Five hundred solutions in one repository is not a repository.
MAX_FILES = 64

TIMEOUT = httpx.Timeout(connect=5.0, read=10.0, write=5.0, pool=5.0)
MAX_REDIRECTS = 3

#: The whole of one fetch, redirects and body included, in seconds. The
#: per-operation timeouts above bound each read; they do not bound a host that
#: answers one byte every nine seconds for an hour, and a crawler that can be
#: held on one source that long is a crawler one publisher can stall for
#: everyone. The clock starts before the first request and is checked between
#: hops and between chunks.
DEADLINE_S = 20.0


class Overflow(Exception):
    """A body past its cap. Counted after decompression — a gzip body of a few
    kilobytes that inflates to gigabytes would pass a `Content-Length` check
    and still have to be held in memory to be refused."""


class Deadline(Exception):
    """The wall clock ran out."""


def stream(client: httpx.Client, url: str, *, headers: dict, pin: dict,
           cap: int, deadline: float) -> tuple[httpx.Response, bytes | None]:
    """One request, read in chunks against a cap and a deadline.

    Returns the response and its body; the body is None where the status says
    there is nothing to read (a 304, a redirect). The response is closed before
    this returns, so the caller may issue the next hop on the same client.
    """
    if time.monotonic() > deadline:
        raise Deadline()
    with client.stream("GET", url, headers=headers, extensions=pin) as resp:
        if resp.status_code in (304, 301, 302, 303, 307, 308):
            return resp, None
        body = bytearray()
        # `iter_bytes` yields decoded bytes, so the count is what we would have
        # to hold and parse, not what crossed the wire.
        for chunk in resp.iter_bytes():
            body += chunk
            if len(body) > cap:
                raise Overflow()
            if time.monotonic() > deadline:
                raise Deadline()
        return resp, bytes(body)

#: The request extension carrying the address this request may connect to.
#: Per request, never per client: one client serves a whole batch of unrelated
#: sources, so a pin stored on the client would be the wrong host's.
PIN = "podshl_pinned_address"


class PinnedTransport(httpx.HTTPTransport):
    """Connect to the address that was checked, not to the name that was.

    The rule at the top of this file was written and not implemented. Every
    caller validated a *hostname* and then handed that hostname to `httpx`,
    which resolved it again at connect time — so a publisher-controlled name
    with a short TTL answers public for the check and `169.254.169.254` for the
    connection, and nothing in between notices.

    Pinning is done by rewriting the host to the literal address while keeping
    the original name in `Host` and in the TLS SNI. Certificate verification
    then still runs against the *name*, which is what makes this safe rather
    than merely different: an attacker who moves the DNS answer does not thereby
    obtain a certificate.

    A request with no pin is refused outright. Failing closed matters more than
    the convenience: a future caller that forgets the extension gets an error
    instead of quietly resolving twice again.
    """

    def handle_request(self, request: httpx.Request) -> httpx.Response:
        pinned = request.extensions.get(PIN)
        if not pinned:
            raise httpx.TransportError(
                f"refusing to resolve {request.url.host!r} a second time: this "
                f"transport connects only to an address that was already checked. "
                f"Pass extensions={{{PIN!r}: <address>}}."
            )
        if request.url.host != pinned:
            name = request.url.host
            # Keep the name for SNI and for the Host header. httpx verifies the
            # certificate against `sni_hostname` when it is set, so the identity
            # being checked stays the publisher's name and not the address.
            request.extensions = {**request.extensions, "sni_hostname": name}
            request.headers["Host"] = request.url.netloc.decode("ascii")
            request.url = request.url.copy_with(host=pinned)
        return super().handle_request(request)


def pinned_client() -> httpx.Client:
    """The only client anything here should build. Redirects are followed by
    hand, and every request must carry its pin."""
    return httpx.Client(timeout=TIMEOUT, follow_redirects=False,
                        transport=PinnedTransport())


@dataclass(frozen=True, slots=True)
class Fetched:
    """Deliberately the same reason vocabulary as an anchor probe. A 404 on a
    manifest and a 404 on a challenge file mean the same thing about the
    claimant, and using one enum keeps them gradable the same way."""

    reason: Reason
    status: int | None = None
    body: bytes | None = None
    etag: str | None = None
    last_modified: str | None = None
    final_url: str = ""
    not_modified: bool = False
    detail: dict = field(default_factory=dict)

    @property
    def usable(self) -> bool:
        return self.reason is Reason.CONFIRMED

    def __bool__(self):
        raise TypeError(
            "a Fetched has more than two answers — ask .usable, .not_modified, "
            "or compare .reason"
        )

    def content_hash(self) -> bytes | None:
        return hashlib.sha256(self.body).digest() if self.body is not None else None


def decoded_path(path: str) -> str:
    """The path as a server will actually resolve it.

    `urlparse` does not percent-decode and `str.startswith` compares what it was
    given, so `…/.podshl/%2e%2e/%2e%2e/victim/…` *textually* begins with the
    anchor's prefix and passes containment — while any origin or CDN that
    decodes and normalises dot-segments, which RFC 3986 says to do, resolves it
    somewhere else entirely. The mirror would then serve another tenant's file
    under this anchor's attested name.

    Decoded first, then normalised, in that order: normalising an encoded path
    sees no dot-segments to collapse, which is how the encoding evaded the check
    in the first place. `posixpath.normpath` clamps at the root, so a traversal
    that climbs past it lands somewhere the prefix test then rejects.
    """
    decoded = unquote(path)
    cleaned = posixpath.normpath(decoded)
    # `normpath` drops a trailing slash, and that slash is what distinguishes a
    # directory prefix from a sibling whose name merely starts the same way.
    if decoded.endswith("/") and not cleaned.endswith("/"):
        cleaned += "/"
    return cleaned


def under_prefix(url: str, prefix: str) -> bool:
    """Everything fetched has to live under the anchor that was verified.

    Compared on the normalised prefix rather than by string containment:
    `https://example.org/` must not admit `https://example.org.evil/`.
    """
    u, p = urlparse(url), urlparse(prefix)
    if u.scheme != p.scheme or u.hostname != p.hostname or (u.port or 443) != (p.port or 443):
        return False
    base = decoded_path(p.path)
    if not base.endswith("/"):
        base = base.rsplit("/", 1)[0] + "/"
    return decoded_path(u.path).startswith(base)


def get(url: str, *, prefix: str, etag: str | None = None,
        last_modified: str | None = None, client: httpx.Client | None = None) -> Fetched:
    """One conditional GET, bounded."""
    if not url.startswith("https://") and not url.startswith("http://127.0.0.1"):
        # Plain HTTP is refused except against the loopback the demo uses. A
        # mirror served over a channel anyone can rewrite is not provenance.
        return Fetched(Reason.REFUSED, detail={"why": "not https"})
    if not under_prefix(url, prefix):
        return Fetched(Reason.REDIRECTED_AWAY, detail={"why": "outside the anchor", "url": url})

    host = urlparse(url).hostname or ""
    if not host:
        return Fetched(Reason.INTERNAL, detail={"why": "no host"})
    checked = public_addresses(host)
    if not checked:
        return Fetched(Reason.REFUSED, detail={"why": "not a public address", "host": host})
    # Resolved once, here. Every request below connects to this address, so the
    # name cannot answer differently between the check and the connection.
    pin = {PIN: checked[0]}

    headers = {}
    if etag:
        headers["If-None-Match"] = etag
    if last_modified:
        headers["If-Modified-Since"] = last_modified

    owned = client is None
    http = client if client is not None else pinned_client()
    deadline = time.monotonic() + DEADLINE_S
    try:
        resp, body = stream(http, url, headers=headers, pin=pin, cap=MAX_BYTES,
                            deadline=deadline)
        hops = 0
        while resp.status_code in (301, 302, 303, 307, 308) and hops < MAX_REDIRECTS:
            target = str(httpx.URL(url).join(resp.headers.get("location", "")))
            if urlparse(target).hostname != host or not under_prefix(target, prefix):
                return Fetched(Reason.REDIRECTED_AWAY, status=resp.status_code,
                               detail={"to": target})
            url = target
            # Same host by the check above, so the same pin still applies — and
            # reusing it is the point: a redirect that stayed on the host must
            # not become a second opportunity to resolve it.
            resp, body = stream(http, url, headers=headers, pin=pin, cap=MAX_BYTES,
                                deadline=deadline)
            hops += 1

        # The whole point of the conditional GET: ten thousand projects polled
        # every fifteen minutes is about eleven requests a second, nearly all of
        # them this branch.
        if resp.status_code == 304:
            return Fetched(Reason.CONFIRMED, status=304, not_modified=True, final_url=url)
        if resp.status_code in (404, 410):
            return Fetched(Reason.ABSENT, status=resp.status_code, final_url=url)
        if resp.status_code in (401, 403, 429, 451):
            return Fetched(Reason.REFUSED, status=resp.status_code, final_url=url)
        if resp.status_code != 200 or body is None:
            return Fetched(Reason.UNREACHABLE, status=resp.status_code, final_url=url)

        return Fetched(Reason.CONFIRMED, status=200, body=body,
                       etag=resp.headers.get("etag"),
                       last_modified=resp.headers.get("last-modified"),
                       final_url=url)
    except Overflow:
        # Refused as malformed, not as unreachable: the host answered, and what
        # it answered with is not a document this mirror will hold.
        return Fetched(Reason.MALFORMED, status=200, final_url=url,
                       detail={"why": f"larger than {MAX_BYTES} bytes after decompression"})
    except Deadline:
        return Fetched(Reason.UNREACHABLE, final_url=url,
                       detail={"why": f"not complete within {DEADLINE_S:g} s"})
    except httpx.HTTPError as e:
        return Fetched(Reason.UNREACHABLE, detail={"why": type(e).__name__})
    except Exception as e:  # noqa: BLE001
        return Fetched(Reason.INTERNAL, detail={"why": type(e).__name__})
    finally:
        if owned:
            http.close()
