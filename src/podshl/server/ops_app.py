"""The operator's own view — a second application on a second listener.

`SV24` says one vendor's figures requested by another party are refused because
**no path exists to produce them**. That is only true if the code cannot be
reached, not merely if it is not routed: a handler in the public app guarded by
a permission check is one misconfiguration away from being served. So the
outreach view lives in a different ASGI application, bound to a different port.

Running the commons means we can see, per unclaimed domain, how much has piled
up. That is not a contradiction of "never public": **private to the public is
not invisible to the operator.** The rule was that no per-vendor defect list is
ever *published*.

What legitimises using this is what we carry when we go: their own report, free,
confidential and unconditional. A user who reported did so hoping something
would improve; delivering that report to the vendor is the mechanism of that
hope, not a repurposing of it.

Two lines that must not be crossed, because crossing either ends the company
rather than costing it a deal:

  * One vendor's numbers are never shown to anyone else. Not to a competitor,
    not in a pitch, not anonymised-but-guessable.
  * No public ranking, ever.

**Loopback is a wall, not a credential.** Anything on the same host — a
compromised crawler, a browser tab on the operator's machine coaxed into a
request — reaches a loopback port. So every route here also requires a bearer
token from the environment, compared in constant time, and refuses a `Host`
header it does not expect: a DNS name pointed at 127.0.0.1 is how a page in the
operator's browser would be made to talk to this listener, and it arrives with
that name in `Host`. With no token configured the listener answers 503 to
everything, so a view with no credential is unreachable rather than open.
"""
from __future__ import annotations

import hmac

from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse

from . import config, db, takedown
from .config import K_REPORTERS
from .errors import ServerError

# Off here for the same reason as on the public app, plus one of its own: this
# listener is the operator's, and a self-describing schema of what the operator
# can do is the last thing that should answer an unauthenticated GET.
ops = FastAPI(title="podshl-ops", docs_url=None, redoc_url=None, openapi_url=None)


def gate(headers) -> JSONResponse | None:
    """The refusal for this request, or None to let it through.

    Host first, then the token. The order matters a little: a request from a
    rebinding page never gets as far as a timing comparison, and a listener with
    no token says so to a caller who at least came in through the right name.
    """
    host = (headers.get("host") or "").strip().lower()
    if host not in config.OPS_HOSTS:
        return JSONResponse(
            {"code": "wrong_host",
             "reason": "this listener answers only to the names it was told to; "
                       "a request reaching it under another name is a request "
                       "somebody else's page made"},
            status_code=421, headers={"Cache-Control": "no-store"})
    if not config.OPS_TOKEN:
        return JSONResponse(
            {"code": "no_operator_token",
             "reason": "PODSHL_OPS_TOKEN is not set, so this listener refuses "
                       "everything. It is loopback-only and still needs a credential: "
                       "loopback is a wall, not a person."},
            status_code=503, headers={"Cache-Control": "no-store"})
    presented = headers.get("authorization") or ""
    scheme, _, token = presented.partition(" ")
    ok = (scheme.lower() == "bearer"
          and hmac.compare_digest(token.strip().encode(), config.OPS_TOKEN.encode()))
    if not ok:
        return JSONResponse(
            {"code": "not_the_operator",
             "reason": "a bearer token from PODSHL_OPS_TOKEN is required on every route here"},
            status_code=401,
            headers={"WWW-Authenticate": "Bearer", "Cache-Control": "no-store"})
    return None


@ops.middleware("http")
async def require_operator(request: Request, call_next):
    refused = gate(request.headers)
    if refused is not None:
        return refused
    response = await call_next(request)
    # Nothing served here is for anyone else's cache, whatever the route said.
    response.headers["Cache-Control"] = "no-store"
    return response


def _err(e: ServerError, status: int = 400) -> JSONResponse:
    return JSONResponse(e.as_dict(), status_code=status)


@ops.get("/")
async def index():
    return {"service": "podshl-ops", "audience": "the operator, on a separate listener",
            "never": ["publishing any per-vendor figure", "any ranking that leaves this process"]}


@ops.post("/outreach/refresh")
def refresh():
    with db.tx() as conn:
        with conn.cursor() as cur:
            cur.execute("REFRESH MATERIALIZED VIEW outreach_rank")
    return {"refreshed": True}


@ops.get("/outreach")
def outreach(limit: int = 50):
    """The outreach list, and it prioritises itself.

    The domain with the most accumulated observations has the most users in
    pain, is where a report lands hardest, and is therefore both the best sales
    call and the most deserving of one — the signal and the value are the same
    number. It doubles as the capacity-planning figure, since it is also where
    the traffic is.
    """
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT host, clusters, reporters, reports, last_epoch "
                        "FROM outreach_rank ORDER BY reporters DESC LIMIT %s",
                        (max(1, min(limit, 500)),))
            rows = cur.fetchall()
    return {
        "threshold": K_REPORTERS,
        "rows": rows,
        "carry_when_you_go": "their own report, free, confidential and unconditional",
        "never": "this list is not published, and no figure in it is shown to any other vendor",
    }


@ops.get("/holds")
def holds():
    """Anchors held at ingest for looking like a well-known mark.

    Declining to attest is not blocking — a held anchor is still fully reachable
    as `unknown`, which is the state of everyone who never registered. That is
    exactly why we can be conservative here without becoming a chokepoint, and
    why this queue is reviewed by a person rather than cleared automatically.
    """
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT id, host, host_unicode, attest_hold, created_at FROM anchor "
                        "WHERE attest_hold IS NOT NULL ORDER BY created_at")
            return {"held": cur.fetchall()}


# ------------------------------------------------------------ notice and action
#
# The public route records a notice; the decision is made here. That split is
# the whole of the fix: a route anybody can call must not be able to un-enrol
# anybody, and a decision that a person makes is one a person can reverse.

@ops.get("/notices")
def notices(limit: int = 100, offset: int = 0):
    """What is waiting for a decision, oldest first, with `total` so the rest
    of it can be reached.

    Without the offset a backlog of a hundred made every newer notice
    unreachable from here — not on a later page, on no page — so the operator
    could not act on anything filed after the queue filled up. `pending` grew
    the offset first and this route did not, for one commit, which is its own
    small lesson: a fix in the library that the only caller cannot ask for is
    not a fix.
    """
    with db.read() as conn:
        return {"pending": takedown.pending(conn, limit, offset),
                "total": takedown.pending_count(conn),
                "limit": limit, "offset": offset}


@ops.post("/notice/{notice_id}/act")
def notice_act(notice_id: int):
    """Perform the takedown a pending notice asks for. Logged with its reason
    code; the affected party is told where to read it."""
    try:
        with db.tx() as conn:
            return takedown.act(conn, notice_id)
    except takedown.NoSuchNotice as e:
        return _err(e, 404)
    except takedown.NotPending as e:
        return _err(e, 409)
    except ServerError as e:
        return _err(e, 400)


@ops.post("/notice/{notice_id}/reinstate")
def notice_reinstate(notice_id: int):
    """Reverse a takedown. Also a log entry, pointing at the one it reverses."""
    try:
        with db.tx() as conn:
            return takedown.reinstate(conn, notice_id)
    except takedown.NoSuchNotice as e:
        return _err(e, 404)
    except takedown.NotPending as e:
        return _err(e, 409)
    except ServerError as e:
        return _err(e, 400)
