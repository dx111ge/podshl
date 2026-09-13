"""An open-source project's static host, serving `.podshl/` and nothing else.

The counterparty for the OSS branch. It exists because ingest has to be
exercised against something that behaves like a real publisher rather than
against a fixture read off disk: a static file server, plain HTTPS in
production, with an ETag so the conditional GET has something to be conditional
about.

Deliberately dumb. A forge, a bare nginx and a static host all serve this
identically, which is the property that lets a developer with no domain, no
server and no signing key publish at all — and the reason ingest speaks plain
HTTPS instead of a forge API.
"""
from __future__ import annotations

import hashlib
from pathlib import Path

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, PlainTextResponse

app = FastAPI(title="oss-project")

# The worked example that ships with the specification. Serving the same bytes
# the spec publishes means the example cannot quietly stop being ingestible.
ROOT = Path(__file__).resolve().parents[3] / "spec" / "example"

# What the operator asked this project to put at its anchor. In production this
# is whatever the claimant was told to serve; here it is fixed so the demo can
# verify without a round of manual setup.
CHALLENGE_TOKEN = "podshl-example-anchor-token"


def _etag(body: bytes) -> str:
    return '"' + hashlib.sha256(body).hexdigest()[:32] + '"'


@app.get("/")
async def home():
    return PlainTextResponse(
        "example-project — an ordinary Python library.\n"
        "Support metadata at /.podshl/agent.yaml\n"
    )


@app.get("/.well-known/podshl-challenge")
async def challenge():
    """The whole storage requirement: one static file at a controlled location.

    Anyone who cannot serve this cannot be verified anyway, which is why there
    is no separate "developer without a repository" case to solve."""
    return PlainTextResponse(CHALLENGE_TOKEN)


#: The specification's example declares `https://example.org/`, which is what a
#: real publisher would write and what belongs in a published document. Served
#: from here it has to point at here, or ingest refuses it — correctly, because
#: an endpoint outside its anchor is exactly what SV3 exists to catch.
#:
#: So the one line that must differ is substituted on the way out, and it is
#: named rather than hidden: the bytes a developer copies stay canonical, and
#: the fixture is coherent with the host actually serving it.
CANONICAL_BASE = "https://example.org/"


def _localise(body: bytes, base: str) -> bytes:
    return body.replace(CANONICAL_BASE.encode(), base.encode())


@app.get("/.podshl/{path:path}")
async def podshl(path: str, request: Request):
    target = (ROOT / ".podshl" / path).resolve()
    if not str(target).startswith(str((ROOT / ".podshl").resolve())) or not target.is_file():
        return JSONResponse({"detail": "Not Found"}, status_code=404)

    body = _localise(target.read_bytes(), str(request.base_url))
    etag = _etag(body)
    # The conditional GET is what makes ten thousand projects on a fifteen-minute
    # cycle about eleven requests a second, nearly all of them this branch.
    if request.headers.get("if-none-match") == etag:
        return PlainTextResponse("", status_code=304, headers={"ETag": etag})
    return PlainTextResponse(body.decode("utf-8"), headers={"ETag": etag})
