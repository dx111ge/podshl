"""The human-facing pages, served as files and rendered from nothing.

Every page here is static bytes that fetch the same JSON the client fetches.
Three consequences, and each is the reason for the next:

* **No server-side rendering, so no injection surface.** Nothing on this side
  ever concatenates a value into markup. `SV75` asserts the byte-for-byte
  identity rather than trusting the sentence.
* **The JSON API stays the single source of truth.** A page is a view, never a
  second implementation, so a page cannot show anything the API does not already
  serve to everyone.
* **The route table stays readable.** These are explicit routes, not a
  `StaticFiles` mount. A mount collapses a whole subtree into one `Mount` whose
  `.path` is the prefix, and `SV21`/`SV67` — which enumerate `app.routes` to
  prove no path produces another vendor's figures — would stop being able to see
  what is reachable. Dropping a file into a directory would add a public URL with
  no decorator, no review and no case. "Refused because no path exists" has to
  mean the paths are countable.
"""
from __future__ import annotations

import base64
import hashlib
import re
from pathlib import Path

from fastapi.responses import HTMLResponse, PlainTextResponse, Response

DIR = Path(__file__).parent / "pages"

#: One extractor, used by the CSP hash below and by the syntax check in the
#: suite. If they disagreed, a block the header permits and a block `node`
#: parses would be different blocks, and the disagreement would show up as a
#: blank page rather than as an error.
SCRIPT = re.compile(r"<script>(.*?)</script>", re.S)

#: How a page loads the shared list, exactly. Written once so the CSP and the
#: suite look for the same bytes.
LIST_TAG = '<script src="/list.js"></script>'


def scripts(html: str) -> list[str]:
    return SCRIPT.findall(html)


def _csp(html: str) -> str:
    """`default-src 'none'` and an allow-list from there.

    `script-src` is a hash per inline block rather than `'unsafe-inline'`. The
    cost is a real coupling: reformat a script and the hash stops matching, and
    the page goes **blank with no error a text check would catch** — the exact
    failure that killed the client window and stayed green through 201 cases. It
    is bearable only because the hash is computed here, from these bytes, at
    import; nobody maintains it by hand.
    """
    # base64, not hex. CSP specifies base64 and a browser silently ignores a
    # malformed hash — which does not error, it just blocks the script and the
    # page comes up with its layout drawn and nothing working.
    digests = [
        "'sha256-" + base64.b64encode(hashlib.sha256(s.encode()).digest()).decode() + "'"
        for s in scripts(html)
    ]
    # A page with no script at all must still name a source. An empty
    # `script-src ` is not permissive, it is an illegal header value, and the
    # server closes the connection rather than answering.
    hashes = " ".join(digests) if digests else "'none'"
    # The shared list is a file of its own, so a page that loads it permits
    # `'self'` — and only such a page, so every other one keeps the policy it
    # had. `'self'` would also admit any same-origin response as a script if
    # something ever injected a tag naming one; every JSON answer carries
    # `nosniff` for that reason, and a browser then refuses it as a script.
    if LIST_TAG in html:
        hashes = ("'self' " + hashes) if digests else "'self'"
    return (
        "default-src 'none'; base-uri 'none'; form-action 'none'; "
        "frame-ancestors 'none'; connect-src 'self'; "
        "style-src 'self' 'unsafe-inline'; img-src 'self' data:; "
        f"script-src {hashes}"
    )


def _read(name: str) -> str:
    p = DIR / name
    if not p.is_file():
        raise FileNotFoundError(f"page {name!r} is missing from {DIR}")
    return p.read_text(encoding="utf-8")


#: Read once. A page that changes under a running server is a page nobody
#: reviewed, and the CSP hash would go stale with it.
_CACHE: dict[str, tuple[str, str, str]] = {}


def _load(name: str) -> tuple[str, str, str]:
    if name not in _CACHE:
        body = _read(name)
        etag = '"' + hashlib.sha256(body.encode()).hexdigest()[:32] + '"'
        _CACHE[name] = (body, _csp(body), etag)
    return _CACHE[name]


def etag_matches(if_none_match: str | None, etag: str) -> bool:
    """Whether a conditional request already holds this representation.

    Tolerant on purpose. `If-None-Match` may carry several tags, each may be
    weak (`W/"…"`), and `*` matches anything — a proxy that rewrites a strong
    tag into a weak one on the way through is common, and a comparison that
    only accepted our own exact bytes would make every such client pay for the
    whole body forever while believing it was being conditional.
    """
    if not if_none_match:
        return False
    strong = etag[2:] if etag.startswith("W/") else etag
    for candidate in if_none_match.split(","):
        c = candidate.strip()
        if c == "*":
            return True
        if c.startswith("W/"):
            c = c[2:]
        if c == strong:
            return True
    return False


def serve(name: str, *, status: int = 200, vary: str | None = None,
          if_none_match: str | None = None) -> Response:
    """One page, with the headers every page gets.

    `Referrer-Policy: no-referrer` because a maintainer's dashboard URL carries
    the host they are looking at in its fragment, and a referrer would leak the
    rest of the page's context to anything it links.

    A page is a file, so its ETag is the file's hash and a client that holds
    it gets a 304 with the same cache headers rather than the bytes again. A
    503 — the imprint with nothing configured — is `no-store`: an error page
    describing a missing configuration must not outlive the fix in a cache.
    """
    body, csp, etag = _load(name)
    headers = {
        "Content-Security-Policy": csp,
        "Referrer-Policy": "no-referrer",
        "X-Content-Type-Options": "nosniff",
        "Cache-Control": "no-store" if status >= 500 else "public, max-age=60",
        "ETag": etag,
    }
    if vary:
        headers["Vary"] = vary
    if status == 200 and etag_matches(if_none_match, etag):
        return Response(status_code=304, headers=headers)
    return HTMLResponse(body, status_code=status, headers=headers)


def stylesheet() -> Response:
    return Response(
        _read("podshl.css"),
        media_type="text/css",
        headers={"Cache-Control": "public, max-age=300",
                 "X-Content-Type-Options": "nosniff"},
    )


def list_script() -> Response:
    """The one list every page pages, filters and sorts with (`W18`)."""
    return Response(
        _read("list.js"),
        media_type="text/javascript; charset=utf-8",
        headers={"Cache-Control": "public, max-age=300",
                 "X-Content-Type-Options": "nosniff"},
    )


def icon() -> Response:
    """The tab icon. SVG rather than ICO, and linked rather than guessed at.

    `img-src 'self' data:` in the CSP already permits it; nothing about the
    header needed to change. `nosniff` matters more here than elsewhere — an
    SVG served as anything a browser is willing to reinterpret is a document,
    and a document from this origin would run under this origin's policy.
    """
    return Response(
        _read("favicon.svg"),
        media_type="image/svg+xml",
        headers={"Cache-Control": "public, max-age=86400",
                 "X-Content-Type-Options": "nosniff"},
    )


def text(body: str, media_type: str = "text/plain; charset=utf-8") -> Response:
    return PlainTextResponse(
        body, media_type=media_type,
        headers={"Cache-Control": "public, max-age=300",
                 "X-Content-Type-Options": "nosniff"},
    )
