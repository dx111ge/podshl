"""A vendor with no A2A presence — which is every vendor today.

Exists so the demo can show the difference honestly rather than describing it:
the site is up, the company is real, and there is simply no agent to talk to.
"""
from fastapi import FastAPI
from fastapi.responses import HTMLResponse, JSONResponse

app = FastAPI(title="plain-vendor-website")


@app.get("/", response_class=HTMLResponse)
def home():
    return "<h1>Beispiel GmbH</h1><p>Support: FAQ, Kontaktformular, Hotline Mo-Fr 9-17 Uhr.</p>"


@app.get("/.well-known/agent-card.json")
def no_card():
    return JSONResponse({"detail": "Not Found"}, status_code=404)


@app.get("/nonjson/.well-known/agent-card.json", response_class=HTMLResponse)
def not_json():
    """200, and a login page rather than a card — a captive portal, a CDN error
    page, an SSO redirect that landed. Treated as "nobody there": the status
    said yes and the body is not a card, which is not a trust failure and must
    never be reported as one.

    It has its own route because otherwise the case is indistinguishable from
    the 404 and nothing actually exercises the parse."""
    return "<!doctype html><title>Anmelden</title><h1>Bitte anmelden</h1>"


@app.get("/broken/.well-known/agent-card.json")
def broken():
    """A vendor whose endpoint is misconfigured rather than absent. Still
    "nobody there" from the client's side — the distinction that matters is
    trust, and a 500 says nothing about identity."""
    return JSONResponse({"detail": "Internal Server Error"}, status_code=500)
