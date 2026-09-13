"""A static host for engram's `.podshl/`, standing in for GitHub Pages.

Deliberately dumb, and the same shape as the specification's own example host:
a challenge file at a fixed path, the manifest and the solutions under
`.podshl/`, an ETag so the conditional GET has something to be conditional
about, and nothing else. A forge, a bare nginx and a static host all serve this
identically, which is the property that lets a project with no server publish.

The one line that must differ is the canonical base. engram's manifest declares
`https://dx111ge.github.io/engram/`, which is what belongs in the repository;
served from here it has to point at here, or ingest refuses it — correctly,
because an endpoint outside its anchor is exactly what that check exists for.
So it is substituted on the way out and named rather than hidden.
"""
from __future__ import annotations

import hashlib
import os
from pathlib import Path

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse, PlainTextResponse

app = FastAPI(title="engram-host")

ROOT = Path("/app/var/engram")
CANONICAL_BASE = "https://dx111ge.github.io/engram/"
CHALLENGE_TOKEN = os.environ["ENGRAM_CHALLENGE_TOKEN"]


def _etag(body: bytes) -> str:
    return '"' + hashlib.sha256(body).hexdigest()[:32] + '"'


@app.get("/")
async def home():
    return PlainTextResponse(
        "engram — AI intelligence platform in a single binary.\n"
        "Support metadata at /.podshl/agent.yaml\n"
    )


@app.get("/.well-known/podshl-challenge")
async def challenge():
    return PlainTextResponse(CHALLENGE_TOKEN)


@app.get("/.podshl/{path:path}")
async def podshl(path: str, request: Request):
    target = (ROOT / ".podshl" / path).resolve()
    if not str(target).startswith(str((ROOT / ".podshl").resolve())) or not target.is_file():
        return JSONResponse({"detail": "Not Found"}, status_code=404)
    body = target.read_bytes().replace(CANONICAL_BASE.encode(), str(request.base_url).encode())
    etag = _etag(body)
    if request.headers.get("if-none-match") == etag:
        return PlainTextResponse("", status_code=304, headers={"ETag": etag})
    return PlainTextResponse(body.decode("utf-8"), headers={"ETag": etag})
