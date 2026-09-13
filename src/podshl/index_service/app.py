"""The neutral responsiveness index — deliberately a separate service.

It is separate because neutrality has to be architectural rather than a promise:
this holds no client identity, no incident data and no vendor secrets, so there
is nothing here worth breaching. It accepts one vendor's rate at a time and
publishes a median.
"""
from __future__ import annotations

from collections import defaultdict

from fastapi import FastAPI
from fastapi.responses import JSONResponse

from ..aggregate import publish

app = FastAPI(title="responsiveness-index")

# vendor -> list of rounded rates, one per contributing client. No identities.
_rates: dict[str, list[int]] = defaultdict(list)


@app.post("/contribute")
async def contribute(body: dict):
    vendor, rate = body.get("vendor"), body.get("rate_pct")
    if not isinstance(vendor, str) or not isinstance(rate, int) or not 0 <= rate <= 100:
        return JSONResponse({"accepted": False, "reason": "malformed"}, status_code=400)
    if len(body) > 3 or any(k not in {"vendor", "rate_pct", "weight"} for k in body):
        # Refuse anything carrying more than the three declared fields, so a
        # client cannot accidentally widen what it discloses.
        return JSONResponse({"accepted": False, "reason": "unexpected fields"}, status_code=400)
    _rates[vendor].append(rate)
    return JSONResponse({"accepted": True, **publish(_rates[vendor])})


@app.get("/index/{vendor}")
async def index(vendor: str):
    return JSONResponse({"vendor": vendor, **publish(_rates.get(vendor, []))})


@app.post("/reset")
async def reset():
    """Demo only. A real index has no such endpoint — the history is the asset."""
    _rates.clear()
    return JSONResponse({"reset": True})
