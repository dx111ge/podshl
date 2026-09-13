"""Vendor-side A2A server: signed Agent Card + one JSON-RPC method.

Implements the parts the product argues about, not the whole of A2A:
  * GET  /.well-known/agent-card.json  (RFC 8615 path, A2A v1.0)
  * POST /a2a                          (JSON-RPC 2.0, method "SendMessage")

The Agent Card is signed with a detached JWS (RFC 7515) over its JCS-canonical
form (RFC 8785) with the `signatures` field removed — as A2A v1.0 specifies. The
protected header carries `lei`, the Legal Entity Identifier, so the client can
show *which registered company* it is talking to rather than merely which domain
answered.
"""
from __future__ import annotations

import hashlib
from pathlib import Path

from fastapi import FastAPI
from fastapi.responses import JSONResponse

from .. import jcs
from ..jws import load_or_create_key, public_jwk, sign_detached
from ..model import Remedy, SkillDescriptor
from . import catalog, reports

KEY = load_or_create_key(Path(__file__).resolve().parents[3] / "var" / "vendor.ed25519")
KID = "acme-support-2026"
LEI = "5493001KJTIIGC8Y1R12"          # illustrative; a real one is GLEIF-registered
VENDOR = "ACME Components GmbH"
PUBLIC_URL = "http://127.0.0.1:8721"

app = FastAPI(title="vendor-support-agent")


def _card_body() -> dict:
    return {
        "protocolVersion": "1.0",
        "name": f"{VENDOR} Support Agent",
        "description": catalog.said("en", "card_description"),
        "url": f"{PUBLIC_URL}/a2a",
        "version": "1.0.0",
        "preferredTransport": "JSONRPC",
        "provider": {"organization": VENDOR, "url": "https://example.invalid"},
        "capabilities": {"streaming": False, "pushNotifications": False},
        "defaultInputModes": ["application/json"],
        "defaultOutputModes": ["application/json"],
        "skills": [
            {"id": s.id, "name": s.title, "description": s.title,
             "tags": sorted(s.applies_to.values()),
             "examples": [p.describes for p in s.probes[:2]]}
            for s in catalog.CATALOG.values()
        ],
        "publicKeyJwk": public_jwk(KEY),
    }


def signed_card() -> dict:
    body = _card_body()
    sig = sign_detached(KEY, body, kid=KID, extra={"lei": LEI, "org": VENDOR})
    return {**body, "signatures": [sig]}


@app.get("/.well-known/agent-card.json")
def agent_card():
    return JSONResponse(signed_card())


def _rpc_result(rid, result):
    return {"jsonrpc": "2.0", "id": rid, "result": result}


def _rpc_error(rid, code, message):
    return {"jsonrpc": "2.0", "id": rid, "error": {"code": code, "message": message}}


@app.post("/a2a")
async def a2a(req: dict):
    rid = req.get("id")
    if req.get("method") != "SendMessage":
        return JSONResponse(_rpc_error(rid, -32601, f"unsupported method {req.get('method')!r}"))
    msg = (req.get("params") or {}).get("message") or {}
    data = next((p.get("data") for p in msg.get("parts", []) if p.get("kind") == "data"), {}) or {}

    kind = data.get("kind")
    # The client states its language with every message, and everything this
    # vendor says back — a skill, a finding, a receipt, a refusal — is in it
    # where the vendor has it, otherwise in English, which every vendor owes.
    want = (data.get("lang") or "en").lower()
    if kind == "triage":
        skill = catalog.triage(data.get("problem", ""), data.get("context") or {})
        if skill is None:
            return JSONResponse(_rpc_result(rid, {
                "status": {"state": "TASK_STATE_FAILED"},
                "reason": catalog.said(want, "no_skill"),
            }))
        # The client is told which language it got, because that decides
        # whether it must translate and show the original alongside.
        try:
            served, lang_served = catalog.serve(skill, want)
        except catalog.MissingEnglish as e:
            return JSONResponse(_rpc_result(rid, {
                "status": {"state": "TASK_STATE_FAILED"}, "reason": str(e),
            }))
        return JSONResponse(_rpc_result(rid, {
            "status": {"state": "TASK_STATE_INPUT_REQUIRED"},
            "skill": served.model_dump(),
            "lang_served": lang_served,
            "lang_requested": want,
        }))

    if kind == "diagnose":
        skill: SkillDescriptor | None = catalog.CATALOG.get(data.get("skill_id"))
        if skill is None:
            return JSONResponse(_rpc_error(rid, -32602, "unknown skill_id"))
        # The same language rule as triage. The request carried no language
        # at all, so a finding was German on every screen whatever the skill
        # had been served in.
        facts = data.get("facts") or {}
        remedy: Remedy = catalog.generate(skill, facts, want)
        payload = remedy.model_dump()
        # The request this answers, signed back with it.
        #
        # A signature proved who wrote a remedy and nothing about what for. An
        # answer to somebody else's readings verified just as well as an answer
        # to these, last month's verified as well as today's, and anybody on
        # the wire could replay one. The nonce is the client's and fresh per
        # request; the hash is recomputed here from the facts actually used,
        # so the answer is bound to the readings it was made from rather than
        # to whatever the caller claimed they were.
        payload["nonce"] = data.get("nonce")
        payload["facts_sha256"] = hashlib.sha256(jcs.canonicalize(facts)).hexdigest()
        claimed = data.get("facts_sha256")
        if claimed is not None and claimed != payload["facts_sha256"]:
            # The client and the vendor canonicalised the same readings
            # differently, which means one of them is not answering about what
            # the other asked. Saying so is the only safe answer.
            return JSONResponse(_rpc_error(
                rid, -32602,
                "facts_sha256 does not match the facts sent - the answer would be "
                "bound to readings neither side agrees on"))
        # A remedy is signed only after the vendor has produced it from the
        # bounded vocabulary. A signature attests validation, not merely origin.
        return JSONResponse(_rpc_result(rid, {
            "status": {"state": "TASK_STATE_COMPLETED"},
            "remedy": payload,
            "signature": sign_detached(KEY, payload, kid=KID, extra={"lei": LEI}),
            "lang_served": "de" if want == "de" else "en",
        }))

    if kind == "report":
        # User-initiated, never a background stream: the button is only worth
        # anything as long as nothing does the same thing silently.
        return JSONResponse(_rpc_result(rid, {
            "status": {"state": "TASK_STATE_COMPLETED"},
            "receipt": reports.receive(data.get("report") or {}, want),
        }))

    if kind == "escalate":
        # Where a human takes over. What makes this worth anything is that the
        # incident record arrives with it: the person starts from machine facts,
        # the user's own answers and the failed attempts, not from "it broken".
        ref = f"{data.get('queue', 'general').upper()}-{abs(hash(str(data.get('payload')))) % 100000:05d}"
        reply = data.get("reply_via") or "none"
        # The return path is what the vendor promised and the user chose. Saying
        # nothing back is allowed; leaving the user to guess is not.
        answer = catalog.said(want, f"reply_{reply}" if reply in ("email", "ticket_url", "none")
                              else "reply_unknown")
        return JSONResponse(_rpc_result(rid, {
            "status": {"state": "TASK_STATE_INPUT_REQUIRED"},
            "reference": ref,
            "queue": data.get("queue"),
            "target": data.get("target"),
            "reply_via": reply,
            "reply_note": answer,
            "ticket_url": (f"https://example.invalid/case/{ref}"
                           if reply == "ticket_url" else None),
            "received_fields": sorted((data.get("payload") or {}).keys()),
        }))

    return JSONResponse(_rpc_error(rid, -32602, f"unknown kind {kind!r}"))
