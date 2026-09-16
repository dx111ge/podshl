"""The public server.

Built around one sentence:

    We hold no credentials, write into no foreign system, and see no user data.
    What we assert is publicly verifiable; what we serve is a mirror with
    provenance.

The security argument is a single question — *what does an attacker get who owns
our infrastructure?* — and the answer has to stay "a public log, a mirror of
public repositories, and some anonymous counters".

**What is deliberately absent, and must stay absent:**

* No `GET /verify?domain=…`. A per-request lookup would tell us which software
  every client runs — the profile this design refuses to let vendors build,
  gathered from everyone at once. The log is fetched whole, from a CDN.
* No route that returns one vendor's figures to another party. `SV24` is true
  because no such path exists, not because a permission check says no.
* No public ranking. "These vendors are the worst" is the extortion suspicion
  in a chart.
* No unauthenticated gap report. The dashboard is for a claimed anchor and
  nobody else — which is the behaviour the service this replaces got wrong.
* No `POST /reset`.
"""
from __future__ import annotations

import hashlib
import json
import secrets

from fastapi import FastAPI, Header, Query, Request, Response
from fastapi.responses import JSONResponse as _JSONResponse
# A handler that must await the request body cannot itself be `def`, so the
# blocking half goes to a worker explicitly. Measured, on this machine: two
# 0.4 s queries on the event loop cut a trivial route from 206 requests in five
# seconds to 8, and its median from 4 ms to 783 ms. Every other caller waits
# for whichever query is slowest.
from starlette.concurrency import run_in_threadpool

from . import (cluster_tree, clusters, counting, db, explain, index_feed, log_store,
               pages, repartition, sth, takedown)
from .anchor import challenge, forge
from . import config
from .config import K_REPORTERS
from .errors import NotClaimed, ServerError
from .ingest.confusable import normalise_host
from .ingest import tree_build
from .ingest.fetch import under_prefix

# `docs_url`, `redoc_url` and `openapi_url` are switched off, and that is a
# decision rather than tidying. FastAPI turns them on by default, so this server
# was serving Swagger UI to the public because nobody had said not to — and
# Swagger UI loads its script and stylesheet **from a CDN**. On an origin whose
# whole argument is that it makes no external calls and that everything it
# serves can be read, an unreviewed third-party script delivered from our own
# name is the wrong dependency to have acquired by default.
#
# Nothing is hidden by this: every route here is public and documented in
# `SPEC.md` and on `/publish`, and the schema was never the interesting part. The
# interactive page also invites a reader to fire a `POST /notice` or a
# `POST /claim/{host}` at production to see what happens, which is a strange
# thing to put a button on.
class JSONResponse(_JSONResponse):
    """Every JSON answer, with `X-Content-Type-Options: nosniff`.

    A page that loads the shared `/list.js` permits `'self'` in its
    script-src, and `'self'` alone would admit any response from this origin
    as a script if something ever managed to inject a tag naming one — an
    answer carrying values a stranger chose, loaded by URL. With `nosniff` a
    browser refuses anything that is not JavaScript as a script, so no JSON
    answer can stand in for one. A class rather than middleware, which `W5`
    refuses on this app, and the default for every route returning a dict.
    """

    def __init__(self, *args, headers=None, **kwargs):
        super().__init__(*args, headers={"X-Content-Type-Options": "nosniff", **(headers or {})},
                         **kwargs)


app = FastAPI(title="podshl-server", docs_url=None, redoc_url=None, openapi_url=None,
              default_response_class=JSONResponse)

#: The most free text one report carries, in characters — the client's own
#: bound, enforced again here. See `report()`.
MAX_DESCRIPTION = 16 * 1024

#: How large a request body may be, per route, in bytes. A report is a few
#: kilobytes of coarsened facts and at most 16 KiB of text; a notice is a name,
#: a contact and a reason; a claim carries a proof. A diagnosis carries the
#: facts of one machine. Anything larger is not that request, and it is refused
#: before it is parsed rather than parsed and then found to be wrong.
MAX_BODY = 64 * 1024
MAX_BODY_DIAGNOSE = 256 * 1024

#: The outcomes a report may carry. The same list as the CHECK on the column,
#: repeated here so a wrong value is a sentence to the caller and not a
#: constraint violation from the database.
#:
#: `uncovered` is the fourth and the newest (`0020`): nothing the project
#: published covered this at all -- either its rules produced no statement, or
#: the person was shown its problems and said none of them is theirs. The other
#: three all describe what became of an answer, and so had nowhere to put the
#: case where there was none.
OUTCOMES = ("resolved", "unresolved", "escalated", "abstained", "uncovered")

#: How many configurations one dashboard response carries, most people first.
#: The response says how many there are in all, so a cut is visible. Above this
#: a page is not being read, and the explanation of each costs a tree walk.
MAX_CLUSTERS = 1000

#: What `model_class` and `ux_severity` may say. Short labels, not prose: both
#: are grouped on, and a value nobody else will ever send is a group of one.
MAX_LABEL = 64


def _err(e: Exception, status: int = 400, headers: dict | None = None) -> JSONResponse:
    """A refusal. Ours carry a code and a sentence written for the caller; any
    other exception is reported as a bad request without its text, because an
    exception's message is written for us and may name a table, a path or a
    driver — none of which belongs in a reply to a stranger."""
    if isinstance(e, ServerError):
        detail = e.as_dict()
    else:
        detail = {"code": "bad_request", "reason": "the request could not be processed"}
    return JSONResponse(detail, status_code=status, headers=headers)


def _bad(code: str, reason: str, status: int = 400) -> JSONResponse:
    return JSONResponse({"code": code, "reason": reason}, status_code=status)


async def read_json_object(request: Request, cap: int) -> dict | JSONResponse:
    """The request body as a JSON object, or the refusal to send instead.

    Read in chunks against the cap rather than trusted to `Content-Length`: a
    chunked body declares no length, and a body that declares one and sends
    more is exactly the body this exists to refuse. A body that is not a JSON
    object is a 400 — every route here takes one, and a list or a string is a
    caller's mistake worth a sentence rather than a traceback.
    """
    declared = request.headers.get("content-length")
    if declared and declared.isdigit() and int(declared) > cap:
        return _bad("too_large", f"the body is limited to {cap} bytes", 413)
    buf = bytearray()
    async for chunk in request.stream():
        buf += chunk
        if len(buf) > cap:
            return _bad("too_large", f"the body is limited to {cap} bytes", 413)
    if not buf:
        return {}
    try:
        body = json.loads(bytes(buf))
    except (UnicodeDecodeError, ValueError):
        return _bad("not_json", "the body is not JSON")
    if not isinstance(body, dict):
        return _bad("not_an_object", "the body must be a JSON object")
    return body


@app.get("/")
async def index(request: Request):
    """What this server is, and what it does not hold. Served rather than
    promised: the list below is the whole of it.

    Answers a browser with the landing page and everything else with the JSON,
    on the narrowest rule that can do it: the literal `text/html` appearing in
    `Accept`. No q-value parsing — a rule nobody can predict is a rule that
    breaks a machine client at three in the morning. Browsers send it; `httpx`
    and `curl` send `*/*`, so every existing caller stays on the JSON branch.

    **`Vary: Accept` is correctness here, not politeness.** This origin is meant
    to sit behind a CDN, and a cache keyed on URL alone would let the first
    browser hit poison `/` for every monitor behind it.
    """
    if "text/html" in request.headers.get("accept", ""):
        return pages.serve("home.html", vary="Accept",
                           if_none_match=request.headers.get("if-none-match"))
    return JSONResponse({
        "service": "podshl",
        "holds": ["a public attestation log",
                  "a mirror of public repository content, with the commit it came from",
                  "anonymous cluster counters"],
        "never_holds": ["credentials to anyone's system", "user identities",
                        "query logs", "the content of an enterprise diagnosis"],
        "log": "/log/sth",
        # Reported rather than hidden. This being true in production would mean
        # the crawler can be aimed at our own network.
        "loopback_fetching_enabled": config.ALLOW_LOOPBACK,
    }, headers={"Vary": "Accept"})


# ------------------------------------------------------------- the pages
#
# Static files, one explicit route each. Not a `StaticFiles` mount: a mount
# collapses its whole subtree into a single route, and `SV21`/`SV67` enumerate
# `app.routes` to prove no path produces another vendor's figures. A directory
# nobody can enumerate is a directory where dropping a file adds a public URL.

def _page(name: str, request: Request, status: int = 200) -> Response:
    """A page, conditionally. The ETag is the file's hash, so a browser that
    holds the page gets a 304 and the bytes are sent once per change."""
    return pages.serve(name, status=status,
                       if_none_match=request.headers.get("if-none-match"))


@app.get("/publish")
async def page_publish(request: Request):
    return _page("publish.html", request)


@app.get("/publish/build")
async def page_build(request: Request):
    """The builder for `agent.yaml` and the solution files. A form that writes
    the files in the browser and asks `POST /validate` whether the mirror would
    take them."""
    return _page("build.html", request)


@app.get("/register")
async def page_register(request: Request):
    return _page("register.html", request)


@app.get("/dashboard")
async def page_dashboard(request: Request):
    """The maintainer's own figures.

    No path parameter, deliberately. The host lives in the URL *fragment*, which
    a browser never sends — so this server's access log cannot record which
    domain a maintainer was looking at. `/dashboard/{host}` as a *page* would be
    a per-domain lookup written into our logs, which is the thing `SERVER.md`
    refuses when it refuses `GET /verify?domain=`.
    """
    return _page("dashboard.html", request)


@app.get("/security")
async def page_security(request: Request):
    return _page("security.html", request)


@app.get("/projects")
async def page_projects(request: Request):
    return _page("projects.html", request)


@app.get("/log")
async def page_log(request: Request):
    return _page("log.html", request)


@app.get("/notice")
async def page_notice(request: Request):
    return _page("notice.html", request)


@app.get("/imprint")
async def page_imprint(request: Request):
    """503 until an operator identity is configured, and it says which parts
    are missing.

    An unmet imprint duty must not render as a page that looks fine. Nothing
    here is derived — not the hostname, not the `Host` header, not a
    placeholder — because a page that invents a legal identity is worse than one
    that admits it has none. The 503 is `no-store`: a cache holding "not
    configured" past the moment it is configured would keep an unmet duty
    looking unmet.
    """
    if not config.IMPRINT_COMPLETE:
        return pages.serve("imprint-unconfigured.html", status=503)
    return _page("imprint.html", request)


@app.get("/privacy")
async def page_privacy(request: Request):
    """What this server processes about a person, why, for how long, and who
    to ask — in German and English.

    Refused on the same rule as the imprint: a privacy notice has to name the
    controller, and one naming nobody is not a notice. The controller is
    rendered from `/operator`, like the imprint, never copied into the page.
    """
    if not config.IMPRINT_COMPLETE:
        return pages.serve("imprint-unconfigured.html", status=503)
    return _page("privacy.html", request)


@app.get("/operator")
async def operator():
    """The imprint's values, so the page can render what the server says rather
    than a copy of it that drifts."""
    if not config.IMPRINT_COMPLETE:
        return JSONResponse({"configured": False,
                             "missing": [n for n, v in (("PODSHL_IMPRINT_NAME", config.IMPRINT_NAME),
                                                        ("PODSHL_IMPRINT_ADDRESS", config.IMPRINT_ADDRESS),
                                                        ("PODSHL_IMPRINT_EMAIL", config.IMPRINT_EMAIL))
                                          if not v]}, status_code=503,
                            headers={"Cache-Control": "no-store"})
    return {
        "configured": True,
        "name": config.IMPRINT_NAME,
        # One line per part. The page renders them as lines; a `.env` file
        # cannot hold a newline, and an address is not one line.
        "address": [p.strip() for p in (config.IMPRINT_ADDRESS or "").split("|") if p.strip()],
        # Spelled out, never literal. An imprint has to be published; it does not
        # have to be published to a harvester, and this endpoint is as easy to
        # scrape as the page it feeds.
        "email": config.spell_out(config.IMPRINT_EMAIL),
        "register": config.IMPRINT_REGISTER,
        "security_contact": config.spell_out(config.SECURITY_CONTACT),
        "notice_contact": config.spell_out(config.NOTICE_CONTACT),
        # Absent until there is a repository to point at. A page claiming to be
        # open source with a link that 404s is worse than one that says so.
        "source_url": config.SOURCE_URL,
    }


@app.get("/.well-known/security.txt")
async def security_txt():
    """RFC 9116, and only when it can be honest.

    Served only with a contact *and* an expiry, because the format requires the
    second and a security address nobody answers is worse than none at all.
    """
    if not (config.SECURITY_CONTACT and config.SECURITY_EXPIRES):
        return JSONResponse({"code": "no_security_contact",
                             "reason": "none is configured, and publishing one nobody "
                                       "answers is worse than publishing none"},
                            status_code=404)
    # The one place the literal form belongs. RFC 9116 is *for* machines, and a
    # `Contact:` a tool cannot parse defeats the point of the file — so this is
    # served plainly and the human-facing pages are not.
    return pages.text("\n".join([
        f"Contact: mailto:{config.SECURITY_CONTACT}",
        f"Expires: {config.SECURITY_EXPIRES}",
        "Preferred-Languages: en, de",
        "",
    ]))


# The specification's own bytes, so a page that teaches the format cannot drift
# from the format. `index_service/oss_project.py` already serves these on the
# same reasoning — "serving the same bytes the spec publishes means the example
# cannot quietly stop being ingestible". All of `spec/` is public, so this
# discloses nothing new; it only saves a maintainer from retyping it.

SPEC = config.ROOT / "spec"


#: The two worked examples, and they answer different questions.
#:
#: `example` is a project answering for its own package. `example-desktop` is
#: the case the open-source branch actually lives on: a desktop environment
#: publishing fixes for how the NVIDIA proprietary driver behaves under Wayland,
#: owning neither name. Most maintainers who would use this are in the second
#: position rather than the first, and the first example alone does not show
#: them that naming somebody else's software is allowed — which is the thing
#: that stops people before they start.
#:
#: A dict rather than a directory scan: the routes below are enumerable, which
#: is what `SV21`/`SV67` assert against, and a scan would make the served set
#: depend on what happens to be on disk.
EXAMPLES = {
    "": "example",
    "desktop": "example-desktop",
}


def _example_dir(which: str):
    name = EXAMPLES.get(which)
    return None if name is None else SPEC / name / ".podshl"


@app.get("/example/agent.yaml")
async def example_manifest():
    return pages.text((_example_dir("") / "agent.yaml").read_text(encoding="utf-8"),
                      media_type="text/yaml; charset=utf-8")


@app.get("/example/desktop/agent.yaml")
async def example_desktop_manifest():
    """A project that repairs software it did not write.

    Served as its own bytes for the same reason as the first: a page teaching
    the format must not be able to drift from the format. It exists separately
    because the legal shape is what stops maintainers, not the syntax — this one
    names `nvidia.driver` in its problem classes and has nowhere to type a name
    at all, which is the whole answer to "am I allowed to say that".
    """
    return pages.text((_example_dir("desktop") / "agent.yaml").read_text(encoding="utf-8"),
                      media_type="text/yaml; charset=utf-8")


def _solution(which: str, name: str):
    """One published solution, by file name.

    A path parameter, and a file read — so it is bounded to the directory by
    construction rather than by a check somebody could move: the name is
    rejected unless it is exactly one of the files that directory holds.
    """
    here = _example_dir(which) / "solutions"
    allowed = {f.name for f in here.glob("*.md")}
    if name not in allowed:
        return JSONResponse({"code": "no_such_solution",
                             "reason": f"this worked example holds {sorted(allowed)}"},
                            status_code=404)
    return pages.text((here / name).read_text(encoding="utf-8"),
                      media_type="text/markdown; charset=utf-8")


@app.get("/example/solutions/{name}")
async def example_solution(name: str):
    return _solution("", name)


@app.get("/example/desktop/solutions/{name}")
async def example_desktop_solution(name: str):
    return _solution("desktop", name)


@app.get("/vocabulary/{name}")
async def vocabulary(name: str):
    """The two normative vocabularies, verbatim.

    A maintainer has to be able to see exactly what they may ask a machine to
    read, and a copy of that list on a page is a copy that goes stale.
    """
    if name not in ("reads.json", "actions.json"):
        return JSONResponse({"code": "no_such_vocabulary",
                             "reason": "there are two: reads.json and actions.json"},
                            status_code=404)
    return pages.text((SPEC / "vocabulary" / name).read_text(encoding="utf-8"),
                      media_type="application/json")


@app.get("/list.js")
async def list_script():
    return pages.list_script()


@app.get("/podshl.css")
async def stylesheet():
    return pages.stylesheet()


@app.get("/favicon.svg")
async def favicon():
    """Named in every page's head, so `/favicon.ico` is never asked for.

    A route rather than a file the browser guesses at: the argument for
    enumerating these by hand is that every reachable path is countable, and a
    404 in the console of a page that is otherwise clean is a thing a reviewer
    then has to rule out on every visit.
    """
    return pages.icon()


# ----------------------------------------------------------------- the log
#
# Fetched whole, from a CDN. Proofs exist so a *monitor* can check one entry
# without holding the log — never so a client can ask about one domain, which
# would disclose exactly the fact this design refuses to learn.

#: A response pinned to a range that can never change. Entries are append-only
#: and a proof for a named tree size is arithmetic over a prefix that is
#: already sealed, so a cache may keep such an answer for as long as it likes.
IMMUTABLE = {"Cache-Control": "public, max-age=31536000, immutable"}


@app.get("/log/sth")
def log_head():
    """The current head. Cacheable for a minute: a head is issued once per
    tree size, so a cache serving one sixty seconds old is serving one that
    was true sixty seconds ago, which is what a head is."""
    with db.tx() as conn:
        head = sth.issue(conn)
    return JSONResponse(head, headers={"Cache-Control": "public, max-age=60"})


@app.get("/log/key")
async def log_key():
    from ..jws import public_jwk
    return JSONResponse({"key": public_jwk(sth.key()), "log_id": sth.log_id()},
                        headers={"Cache-Control": "public, max-age=3600"})


@app.get("/log/entries")
def log_entries(start: int = 0, end: int | None = None, limit: int = log_store.MAX_PAGE):
    """A page of the log. Immutable when the page is pinned entirely below the
    current tree size — those entries can never change — and a minute otherwise,
    because an open-ended page grows."""
    try:
        log_store.check_page(start, end, limit)
        with db.read() as conn:
            size = log_store.tree_size(conn)
            rows = log_store.entries(conn, start, end, limit)
    except ValueError as e:
        return JSONResponse({"code": "bad_range", "reason": str(e)}, status_code=400)
    sealed = end is not None and end <= size
    return JSONResponse({"entries": rows},
                        headers=IMMUTABLE if sealed else {"Cache-Control": "public, max-age=60"})


@app.get("/log/proof/inclusion")
def log_inclusion(seq: int, size: int | None = None):
    """`size` pins the proof to a signed head the caller already holds; without
    it the proof is for the current tree, which may have grown since. A pinned
    proof is immutable; an unpinned one is good for a minute."""
    try:
        with db.read() as conn:
            proof = log_store.inclusion(conn, seq, size)
    except ValueError as e:
        return JSONResponse({"code": "bad_range", "reason": str(e)}, status_code=400)
    return JSONResponse(proof, headers=IMMUTABLE if size is not None
                        else {"Cache-Control": "public, max-age=60"})


@app.get("/log/proof/consistency")
def log_consistency(first: int, second: int | None = None):
    try:
        with db.read() as conn:
            proof = log_store.consistency(conn, first, second)
    except ValueError as e:
        return JSONResponse({"code": "bad_range", "reason": str(e)}, status_code=400)
    return JSONResponse(proof, headers=IMMUTABLE if second is not None
                        else {"Cache-Control": "public, max-age=60"})


# ------------------------------------------------------------------ the lookup
#
# There was a `GET /cluster/{hash}` here, "step one, served from the edge", and
# it is gone on purpose (`SV104`). It answered how many people had reported an
# exact configuration about a project, unauthenticated, to anybody who could
# guess the configuration — and a signature is a host and a few coarsened
# facts, so guessing is a small search. That is the per-project figure the
# dashboard keeps private. No client called it, and its `solutions` were always
# empty because nothing wrote `link`. The published path is `POST /diagnose`,
# which carries no figure and writes nothing (`SV12`).

#: A draft is one manifest and its solution files, as text. Half a megabyte is
#: far past any real project, and well inside the mirror's own per-file limit.
MAX_BODY_DRAFT = 512 * 1024


@app.post("/validate")
async def validate_route(request: Request):
    body = await read_json_object(request, MAX_BODY_DRAFT)
    if isinstance(body, JSONResponse):
        return body
    return await run_in_threadpool(validate_draft, body)


def validate_draft(body: dict):
    """Would the mirror take these files? The answer ingest would give, and
    nothing is kept.

    Unauthenticated, because a maintainer checks a draft before they have
    claimed anything. It reads no table and writes none, and nothing about the
    request is logged: a draft says what a project is about to publish.
    """
    from .ingest import draft
    from .ingest.fetch import MAX_FILES

    agent_yaml, solutions, anchor = body.get("agent_yaml"), body.get("solutions"), body.get("anchor")
    if not isinstance(agent_yaml, str) or not agent_yaml.strip():
        return _bad("no_manifest", "agent_yaml must be the text of your agent.yaml")
    solutions = {} if solutions is None else solutions
    if not isinstance(solutions, dict) or not all(
            isinstance(k, str) and isinstance(v, str) for k, v in solutions.items()):
        return _bad("bad_solutions", "solutions must map each path, as agent.yaml lists it, to the file's text")
    if len(solutions) > MAX_FILES:
        return _bad("too_many_files", f"{len(solutions)} solution files, more than the {MAX_FILES} "
                                      f"the mirror fetches for one project")
    if anchor is not None and (not isinstance(anchor, str) or not anchor.startswith(("https://", "http://"))):
        return _bad("bad_anchor", "anchor must be the http(s) address your files will be served from")
    return JSONResponse(draft.check(agent_yaml, solutions, anchor or None),
                        headers={"Cache-Control": "no-store"})


@app.post("/diagnose")
async def diagnose_route(request: Request):
    body = await read_json_object(request, MAX_BODY_DIAGNOSE)
    if isinstance(body, JSONResponse):
        return body
    return await run_in_threadpool(diagnose, body)


def diagnose(body: dict):
    """Step two, and only on a miss. Novel configurations only, and rare.

    The endpoint walks the decision tree **exactly**, along the path actually
    taken, and where it cannot proceed it asks rather than guesses. There is no
    fuzzy matching here: a wrong guess would reach a user.

    Three outcomes, and the third is the point — a finding, no statement, or
    "I still need X". The client answers a `need` by collecting or asking, then
    sends again with the enlarged facts. That is the same round loop it already
    runs with the local model, so it needs one mechanism rather than two.
    """
    subject = body.get("subject")
    facts = body.get("facts") or {}
    if not isinstance(facts, dict):
        return _bad("bad_facts", "facts must be an object of fact id to value")
    stated = body.get("stated") or []
    if not isinstance(stated, list) or not all(isinstance(s, str) for s in stated):
        return _bad("bad_stated", "stated must be a list of fact ids")
    problem_class = body.get("problem_class")
    if problem_class is not None and not isinstance(problem_class, str):
        return _bad("bad_problem_class", "problem_class must be text")
    # Which of those facts a person supplied rather than the machine reading.
    # The walk uses them either way — refusing to match on a supplied fact would
    # make the whole point of asking pointless — but an answer that turned on
    # one is a different kind of answer, and saying so is the difference between
    # a finding and a finding somebody can weigh.
    supplied = set(stated)
    if not isinstance(subject, str) or not subject:
        return JSONResponse({"code": "no_subject", "reason": "no subject named"}, status_code=400)

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                # A repository is reached by its **identity** or not at all.
                # Its host is shared with every other repository on the forge, so
                # a bare host would resolve to whichever one happened to be first
                # — somebody else's project, answering for a subject it never
                # claimed. So a subject that is a URL is matched against
                # `anchor.value`, and a bare name stays a host and never reaches
                # a repository.
                #
                # The same string is what a report carries, and therefore what a
                # cluster is keyed on: two projects on one forge must not share a
                # cluster, and with the host as the subject they would have
                # shared every one.
                ("SELECT s.id AS source_id FROM anchor a "
                 "JOIN source s ON s.anchor_id = a.id AND s.mirror_state = 'serving' "
                 + ("WHERE a.value = %s" if subject.startswith(("https://", "http://"))
                    else "WHERE a.host = %s AND a.kind <> 'repo'")), (subject,))
            row = cur.fetchone()
        if not row:
            # Nobody published here. Not an accusation, and not an error.
            return {"outcome": "no_statement",
                    "reason": "nothing is mirrored for this subject",
                    "note": "that says nothing about them"}

        tree_id = (cluster_tree.find_tree(conn, row["source_id"], problem_class)
                   if problem_class else None)
        if tree_id is None:
            return {"outcome": "no_statement",
                    "reason": "no decision tree for this problem class",
                    "note": "without an owner there are no switches — clusters are "
                            "exact signature matches only, which is honest and less useful"}

        root = cluster_tree.load(conn, tree_id)
        out = cluster_tree.walk(root, facts)

        if isinstance(out, cluster_tree.Need):
            return {
                "outcome": "need",
                "need": [out.probe],
                "need_reason": out.reason,
                # Shipped with the question, so "don't know" costs no round trip
                # and cannot dead-end.
                "fallback": ({"solution_id": out.fallback.solution_id}
                             if out.fallback else None),
                "if_you_cannot_answer": (
                    "send `<probe id>.declined: true` and the fallback above applies"),
            }
        if isinstance(out, cluster_tree.Answer):
            with conn.cursor() as cur:
                cur.execute(
                    "SELECT solution_id, text_by_lang, proposes, severity, commit "
                    "FROM solution WHERE source_id = %s AND solution_id = %s "
                    "AND valid_to IS NULL", (row["source_id"], out.solution_id))
                sol = cur.fetchone()
            rested = [f for f in out.decided_on if f in supplied]
            return {
                "outcome": "finding",
                "solution": sol,
                "path": list(out.path),
                # The switches actually taken, not every fact that happened to
                # be present.
                "decided_on": list(out.decided_on),
                "rested_on_supplied": rested,
                "confidence": "measured" if not rested else "rests_on_supplied",
                "reading": None if not rested else (
                    "this answer turned on " + ", ".join(rested) + ", which the person "
                    "supplied rather than the machine reading. If it is wrong, that may be "
                    "the answer rather than the rule"),
            }
        return {"outcome": "no_statement", "reason": out.reason}


@app.post("/report")
async def report_route(request: Request):
    body = await read_json_object(request, MAX_BODY)
    if isinstance(body, JSONResponse):
        return body
    return await run_in_threadpool(report, body)


def _label(value, name: str, default: str | None):
    """A short grouping label, or a refusal. Returns (value, error)."""
    if value is None:
        return default, None
    if not isinstance(value, str) or not value or len(value) > MAX_LABEL:
        return None, _bad(f"bad_{name}", f"{name} must be a short label")
    return value, None


def report(body: dict):
    """Durable, counted, and chosen by the user **after** the answer.

    That ordering is not a detail: a report filed afterwards can carry whether
    the fix worked, which is the outcome label that makes the whole corpus worth
    having.

    Every field is checked for its type here, where the refusal is a sentence.
    Each of these used to reach the database or the signature builder as
    whatever JSON happened to carry, and the error the client got was a 500
    with a driver's or a dict's message in it — a `consent` sent as a string
    raised `AttributeError` on `.get`, an outcome outside the CHECK raised the
    constraint's name.
    """
    pseudonym = body.get("pseudonym")
    if not isinstance(pseudonym, str) or not pseudonym or len(pseudonym) > 256:
        return JSONResponse({"code": "no_pseudonym",
                             "reason": "a report is counted once per pseudonym per epoch, "
                                       "so it must carry one"}, status_code=400)

    subject = body.get("subject")
    if not isinstance(subject, str) or not subject:
        return JSONResponse({"code": "no_subject", "reason": "no subject named"}, status_code=400)
    observed = body.get("observed")
    observed = {} if observed is None else observed
    if not isinstance(observed, dict):
        return _bad("bad_observed", "observed must be an object of fact id to value")
    failed = body.get("failed_actions")
    failed = [] if failed is None else failed
    if not isinstance(failed, list) or not all(isinstance(a, str) for a in failed):
        return _bad("bad_failed_actions", "failed_actions must be a list of action ids")
    outcome = body.get("outcome")
    if outcome is not None and outcome not in OUTCOMES:
        return _bad("bad_outcome", f"outcome must be one of {list(OUTCOMES)}")
    model_class, err = _label(body.get("model_class"), "model_class", "unknown")
    if err:
        return err
    ux_severity, err = _label(body.get("ux_severity"), "ux_severity", None)
    if err:
        return err

    description = body.get("description")
    consent = body.get("description_consent")
    if description is not None and not isinstance(description, str):
        return JSONResponse({"code": "bad_description",
                             "reason": "description must be text"}, status_code=400)
    if description and len(description) > MAX_DESCRIPTION:
        # An excerpt is the lines around a failure. A whole log is a record of
        # somebody's day that nobody reviewed line by line before it was sent,
        # and the client bounds it long before this — so arriving here over the
        # limit means something other than that client sent it.
        return JSONResponse({"code": "too_long",
                             "reason": f"free text is limited to {MAX_DESCRIPTION} characters "
                                       f"— the lines around the failure, not the log"},
                            status_code=413)
    if consent is not None and not isinstance(consent, dict):
        return _bad("bad_consent", "description_consent must be an object")
    if description and not (consent and consent.get("granted") is True
                            and isinstance(consent.get("destination"), str)
                            and consent.get("destination")
                            and isinstance(consent.get("granted_at"), str)
                            and consent.get("granted_at")):
        # Free text travels only under its own explicit consent, with the
        # destination named and the moment it was given. Anything else and it
        # is not consent — the same three fields the database's CHECK demands,
        # asked for here so the refusal is a sentence rather than a constraint.
        return JSONResponse({"code": "no_consent",
                             "reason": "free text needs its own consent naming a destination "
                                       "and when it was granted"},
                            status_code=400)

    # What a person supplied travels in its own map so it cannot be mistaken for
    # a measurement — but it is still part of the shape of the problem. Someone
    # who ran `poetry add` and someone who ran `pip install` are in different
    # situations even with identical readings, so the signature is over both.
    # Splitting them here instead would silently re-cluster every report.
    stated = body.get("stated")
    stated = {} if stated is None else stated
    if not isinstance(stated, dict):
        return JSONResponse({"code": "bad_stated",
                             "reason": "stated must be an object"}, status_code=400)
    sig = clusters.canonical_signature(subject, {**observed, **stated}, failed)
    with db.tx() as conn:
        epoch = counting.open_epoch(conn)
        cid = clusters.ensure(conn, sig, subject_host=subject, epoch=epoch)
        new = counting.record_observation(
            conn, cid, pseudonym, epoch=epoch,
            model_class=model_class, ux_severity=ux_severity, outcome=outcome,
            observed=observed, stated=stated,
            description=description, description_consent=consent,
        )
        n = counting.reporters(conn, cid)
        surfaced = n >= K_REPORTERS
    out = {
        "accepted": True,
        "counted": new,
        "note": None if new else "already counted for this pseudonym this epoch",
        "surfaced": surfaced,
        "why": None if surfaced else
               f"fewer than {K_REPORTERS} independent reporters — counted, not evaluated, "
               f"because a rare constellation is identifying",
    }
    if surfaced:
        # The number is only said once it is at least k. Below that, "you are
        # the third" tells the reporter how many other people share their exact
        # configuration, which is a count about somebody else's machine — and
        # counting up to the floor one report at a time is how the floor would
        # be read from outside.
        out["reporters"] = n
    return out


# ------------------------------------------------------------------ the dashboard
#
# Claiming your own domain unlocks the dashboard about yourself, free. No
# payment, because this is the gift — and the moment it feels like "pay or we
# withhold your own defect list" the company is finished.

#: How long a challenge nonce stands before another call mints a new one. Long
#: enough to publish a file and come back tomorrow.
CHALLENGE_LIVE = "1 hour"



def _host_and_port(raw: str) -> tuple[str, str] | None:
    """Normalise a host that may carry a port, keeping the port.

    `normalise_host` is a host *name* check -- IDNA, punycode, length -- and a
    colon is not part of a name, so it refuses one. A self-hosted forge often
    answers on a port (the Gitea the `gitea` shape was measured against is on
    3141), and refusing those would exclude the case repository anchors exist
    for. So the port is split off, the name is normalised as a name, and the two
    are handed back separately. `None` means the name is not usable.
    """
    bare, _, port = raw.partition(":")
    if port and not (port.isdigit() and 1 <= int(port) <= 65535):
        return None
    try:
        bare, _ = normalise_host(bare)
    except ValueError:
        return None
    if not bare or len(bare) > 253:
        return None
    return bare, port


def _repo_url(authority: str, repo: str) -> str:
    """The candidate identity for a repository, before `forge.parse` judges it.

    `https` except on loopback, which is the same narrow carve-out `0006` made
    for `anchor.value` and for the same reason: without it the suite cannot
    stand up a forge of its own and the repository path could only be exercised
    against somebody else's server. `forge.parse` refuses plain HTTP anywhere
    else, so this cannot widen anything.
    """
    scheme = "http://" if authority.split(":", 1)[0] == "127.0.0.1" else "https://"
    return f"{scheme}{authority}/{repo.strip('/')}"


def _claim_target(host: str, body: dict | None):
    """Which anchor a claim names: a domain, or a repository on a forge.

    Returns `(kind, value, probe_prefix)`, or a `JSONResponse` refusing.

    A repository is named by the forge's host in the path and `repo` in the
    body — `POST /claim/github.com` with `{"repo": "dx111ge/engram"}` — rather
    than by pressing a path into a path parameter. The existing route shapes are
    untouched, nothing has to be encoded twice, and a caller who sends no `repo`
    gets exactly the domain claim they always got.

    `SERVER.md` has named a git forge as an anchor since it was written, and
    until now the code could only anchor a domain: nobody can write
    `https://github.com/.well-known/podshl-challenge`, so every maintainer whose
    project is a repository and who owns no domain was excluded — which is most
    of them. This is that gap, and nothing more: a repository anchor buys the
    right to publish. **It buys no name**: a name here is a word in common
    use rather than property, and nothing decides which project owns one.
    """
    repo = (body or {}).get("repo")
    if repo is None:
        return "url", f"https://{host}/", None
    if not isinstance(repo, str) or len(repo) > 220:
        return _bad("bad_repo", "repo must be a string like 'owner/name'")
    shape = (body or {}).get("forge")
    if shape is not None and not isinstance(shape, str):
        return _bad("bad_forge", "forge must be the name of a forge's URL shape")
    authority = host
    parsed = forge.parse(_repo_url(authority, repo), shape)
    if parsed is None:
        return JSONResponse(
            {"code": "unsupported_forge",
             "reason": f"{authority}/{repo} is not a repository this can anchor"
                       + (f" as forge={shape!r}" if shape else "")
                       + f". {forge.supported()}. A forge's shape goes in when somebody "
                       f"has measured it, not when its documentation has been read — "
                       f"and a host we know is not yours to relabel."},
            status_code=400)
    identity, probe_prefix = parsed
    return "repo", identity, probe_prefix

@app.post("/claim/{host}")
async def claim_start_route(host: str, request: Request):
    body = await read_json_object(request, MAX_BODY)
    if isinstance(body, JSONResponse):
        return body
    return await run_in_threadpool(claim_start, host, body)


def claim_start(host: str, body: dict | None = None):
    """Begin a claim. Two halves, and only one of them is published.

    **A published file proves that somebody controls this host. It does not
    prove that the person asking is that somebody.** This route used to hand an
    anonymous caller the nonce and `verify` used to mint a token for whoever
    asked, so for every project that followed the instruction to leave its
    challenge file published, any stranger could take the dashboard, lock the
    maintainer out and withdraw the project. The nonce was never the weak part:
    it is public by design. The weak part was that *observing* a public file was
    treated as *being* the person who put it there.

    So a claim now has a secret half:

    * `publish` is the hex digest. It goes at the well-known path, it is public,
      and it is what the sweep checks forever after. Liveness is not a
      credential and this half is not one either.
    * `proof` is its preimage. It is returned here, to this caller, once, and
      stored only as a hash. A stranger who reads the published half off the
      public URL cannot run it backwards.

    `verify` requires both: the file must carry the digest, and the caller must
    present the preimage.

    **A confirmed anchor's published value is still never rotated by this
    route.** That was already true and is still the point — re-verification
    probes the stored `challenge_token`, so rotating it on an anonymous call
    would degrade the anchor of anybody who is merely participating. Only a
    successful `verify` moves it, and only to a value the claimant proved they
    could publish.

    **Several claims may stand at once.** There used to be one pending claim
    per host, held for `CHALLENGE_LIVE`, and a second caller was refused for
    the hour — so anybody could keep a maintainer out of their own registration
    by starting a claim every fifty-nine minutes. A claim is a nonce hash and
    nothing else; there is no slot to hold. Each caller gets their own, `verify`
    finds the claim by the hash of whatever proof is presented, and a successful
    proof retires every claim on the anchor. A claim somebody is in the middle
    of is untouched by anyone else's, which is the property the one-slot design
    was reaching for and could only approximate.

    The host is checked as a host name and used in its punycode form. A value
    that does not round-trip through IDNA is refused rather than stored, since
    the database would refuse it anyway and its message would name a CHECK.
    """
    split = _host_and_port(host)
    if split is None:
        return _bad("bad_host", "not a usable host name")
    host, port = split
    authority = f"{host}:{port}" if port else host

    target = _claim_target(authority, body)
    if isinstance(target, JSONResponse):
        return target
    kind, value, probe_prefix = target
    root = probe_prefix or value

    proof = secrets.token_urlsafe(32)
    digest = hashlib.sha256(proof.encode()).hexdigest()
    keep = hashlib.sha256(proof.encode()).digest()
    with db.tx() as conn:
        with conn.cursor() as cur:
            # The anchor row is found by host, oldest first, and only created
            # when there is none — the same row `verify` will read, so a claim
            # cannot start on one row and be checked against another.
            # A domain is still found by host, not by value: a loopback anchor's
            # value carries a port (`http://127.0.0.1:8721/`) and would never
            # match a value built from the host alone. A repository has no such
            # spread — its value *is* its identity and is unique — and host is
            # shared by every repository on the forge, so there it has to be the
            # value or a claim would land on somebody else's row.
            if kind == "repo":
                cur.execute("SELECT id FROM anchor WHERE kind = 'repo' AND value = %s",
                            (value,))
            else:
                cur.execute("SELECT id FROM anchor WHERE host = %s AND kind = 'url' "
                            "ORDER BY id LIMIT 1", (host,))
            row = cur.fetchone()
            if row is None:
                cur.execute(
                    "INSERT INTO anchor (kind, value, host, probe_prefix, challenge_token) "
                    "VALUES (%s, %s, %s, %s, %s) "
                    "ON CONFLICT (kind, value) DO UPDATE SET host = EXCLUDED.host, "
                    "  probe_prefix = EXCLUDED.probe_prefix "
                    "RETURNING id",
                    (kind, value,
                     forge.host_only(value) if kind == "repo" else host,
                     probe_prefix, digest))
                row = cur.fetchone()
            # Claims nobody finished expire on their own; the sweep is the
            # next insert, which costs nothing and needs no timer.
            cur.execute("DELETE FROM claim_pending WHERE anchor_id = %s "
                        "AND issued_at < now() - %s::interval", (row["id"], CHALLENGE_LIVE))
            cur.execute("INSERT INTO claim_pending (anchor_id, nonce_hash) VALUES (%s, %s)",
                        (row["id"], keep))
    return JSONResponse(
        {
            "host": host,
            "anchor": value,
            # Where it is *read* from. For a domain that is the host root and the
            # two sentences are one; for a repository the file is committed into
            # the tree and the forge serves it from somewhere else entirely, so
            # both are said rather than leaving the maintainer to work out that a
            # raw URL is not somewhere they can put anything.
            "put_this_at": challenge.challenge_url(root),
            **({} if kind != "repo" else {
                "commit_this_at": ".well-known/podshl-challenge",
                "note_repo": "commit that file at that path in the repository's "
                             "default branch; the URL above is where it is then read "
                             "from, and there is nothing to configure on the forge.",
            }),
            "publish": digest,
            "proof": proof,
            "then": f"POST /claim/{host}/verify, presenting the proof"
                    + ("" if kind != "repo" else " and the same repo"),
            "note": "publish the first value; keep the second. The published half is "
                    "not a secret - it has to be readable by anyone, which is what "
                    "makes it proof of control, and leaving it there is what keeps "
                    "the anchor live. The half you keep is what makes the claim "
                    "yours rather than any passer-by's, and it is shown once.",
        },
        # The proof appears in this body exactly once. A shared cache holding it
        # would leave a credential in somebody else's memory.
        headers={"Cache-Control": "no-store"},
    )


@app.post("/claim/{host}/verify")
async def claim_verify_route(host: str, request: Request,
                             x_podshl_claim_proof: str | None = Header(default=None)):
    body = await read_json_object(request, MAX_BODY)
    if isinstance(body, JSONResponse):
        return body
    return await run_in_threadpool(claim_verify, host, body, x_podshl_claim_proof)


def claim_verify(host: str, body: dict | None = None,
                       x_podshl_claim_proof: str | None = None):
    """Finish a claim. The file must carry the digest and the caller must hold
    the preimage.

    Either alone is not a claim. The file alone is what a stranger can see; the
    preimage alone is what a stranger cannot obtain, and requiring both is the
    whole of the fix.
    """
    split = _host_and_port(host)
    if split is None:
        return _bad("bad_host", "not a usable host name")
    host, port = split
    target = _claim_target(f"{host}:{port}" if port else host, body)
    if isinstance(target, JSONResponse):
        return target
    kind, value, _ = target
    proof = x_podshl_claim_proof or (body or {}).get("proof") or ""
    if not isinstance(proof, str) or len(proof) > 256:
        proof = ""
    presented = hashlib.sha256(proof.encode()).digest()
    with db.tx() as conn:
        with conn.cursor() as cur:
            # `host` is not unique on `anchor` — two rows can share one when
            # their values differ — and an unordered SELECT would then pick a
            # row at random, which is how a claim lands on one anchor and a
            # verify reads another. Deterministic, oldest first.
            cols = ("SELECT id, value, probe_prefix, challenge_token, taken_down_at, "
                    "       taken_down_seq FROM anchor ")
            if kind == "repo":
                cur.execute(cols + "WHERE kind = 'repo' AND value = %s", (value,))
            else:
                cur.execute(cols + "WHERE host = %s AND kind = 'url' "
                                   "ORDER BY id LIMIT 1", (host,))
            anchor = cur.fetchone()
        if not anchor:
            return JSONResponse({"code": "no_claim", "reason": "start one first"}, status_code=404)

        with conn.cursor() as cur:
            cur.execute("SELECT id, nonce_hash FROM claim_pending WHERE anchor_id = %s "
                        "AND issued_at >= now() - %s::interval ORDER BY id",
                        (anchor["id"], CHALLENGE_LIVE))
            claims = cur.fetchall()
        if not claims:
            return JSONResponse(
                {"code": "no_claim",
                 "reason": "no claim is in progress for this host - start one first"},
                status_code=404)
        # Constant-time, against every claim standing, and before the fetch.
        # A timing difference here is a way to search for the proof, and a
        # fetch performed before the caller is known to hold it is a fetch a
        # stranger can make us perform. Every claim is compared, so the time
        # taken does not say which one matched or whether any did.
        matched = 0
        for c in claims:
            matched |= secrets.compare_digest(bytes(c["nonce_hash"]), presented)
        if not proof or not matched:
            return JSONResponse(
                {"code": "no_proof",
                 "reason": "the proof from POST /claim/{host} was not presented, or "
                           "does not match the claim in progress",
                 "note": "publishing the file is not enough and was never meant to be. "
                         "Anyone can read a public file; only whoever started this "
                         "claim holds the half that was never published."},
                status_code=403)

        expected = hashlib.sha256(proof.encode()).hexdigest()
        # `fetch_root`, not `value`: for a repository the identity a person
        # recognises and the place a file can be read are different, and control
        # is proved at the second one.
        probed = challenge.probe(forge.fetch_root(anchor), expected)
        from .anchor import sweep
        sweep.record(conn, anchor["id"], probed)
        if not probed.confirmed:
            return JSONResponse(
                {"code": "not_confirmed", "reason": probed.reason.value,
                 "detail": probed.detail,
                 "note": "this says what happened, not what you are - a timeout is "
                         "our failure to ask, not your failure to publish"},
                status_code=409)

        token = secrets.token_urlsafe(32)
        with conn.cursor() as cur:
            # The published value the sweep will check from now on is the one
            # just proved, and every claim on this anchor is spent — this one
            # because a claim that could be replayed is a claim somebody can
            # hold on to, and the others because control has just been proved
            # by somebody, and a claim started before that is moot.
            cur.execute("UPDATE anchor SET challenge_token = %s WHERE id = %s",
                        (expected, anchor["id"]))
            cur.execute("DELETE FROM claim_pending WHERE anchor_id = %s", (anchor["id"],))
            # Proving control again supersedes what came before. Control is the
            # account, so live tokens are a cache of it rather than independent
            # credentials - keeping yesterday's alive buys the maintainer nothing
            # they cannot re-obtain in one action, and buys anyone who once took a
            # token another year of it. This is also the whole recovery story:
            # there is no email to send a reset to, and "re-prove control and
            # everything else stops working" is a sentence a maintainer can act on
            # by themselves, with nobody to social-engineer.
            cur.execute(
                "UPDATE dashboard_claim SET revoked_at = now(), revoked_reason = 'superseded' "
                "WHERE anchor_id = %s AND revoked_at IS NULL",
                (anchor["id"],),
            )
            superseded = cur.rowcount
            cur.execute(
                "INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                "VALUES (%s, %s, now() + interval '365 days')",
                (anchor["id"], hashlib.sha256(token.encode()).digest()),
            )
    # Not appended to the transparency log, deliberately: an entry saying who
    # rotated a token publishes a private account fact, and worse, discloses
    # which domains have been claimed at all.
    out = {
        "claimed": host, "token": token, "superseded": superseded,
        "note": "free, private, and unconditional. Keep this token: it is the whole "
                "account. We store only its hash, so we cannot show it to you again.",
        "previous": f"{superseded} token(s) for this domain stopped working just now.",
        "leave_published": "keep the published value where it is - it is how we "
                           "re-confirm you still control the host.",
    }
    if anchor["taken_down_at"]:
        # The token is still issued. Whoever proved control of this domain is
        # the party a takedown was taken against, and the one person who has to
        # be able to read what was decided and on what ground - withholding the
        # dashboard from them would make the statement of reasons harder to
        # reach for its intended recipient than for a passer-by.
        out["still_taken_down"] = True
        out["means"] = ("control is proved and the token is yours, but this anchor "
                        "remains withdrawn. Proving control again does not undo a "
                        "notice-and-action decision - that takes a person, not a "
                        "republished file.")
        out["statement_of_reasons"] = (
            f"/log/entries?start={anchor['taken_down_seq']}"
            f"&end={anchor['taken_down_seq'] + 1}")
        out["contest_it"] = "/notice"
    return JSONResponse(
        out,
        # The plaintext appears in this body exactly once. A shared cache holding
        # it would leave a credential in somebody else's memory.
        headers={"Cache-Control": "no-store"},
    )


@app.post("/claim/{host}/source")
async def claim_source_route(host: str, request: Request,
                             x_podshl_claim: str | None = Header(default=None)):
    body = await read_json_object(request, MAX_BODY)
    if isinstance(body, JSONResponse):
        return body
    return await run_in_threadpool(claim_source, host, body, x_podshl_claim)


def claim_source(host: str, body: dict | None = None,
                       x_podshl_claim: str | None = None):
    """Say where your `.podshl/` is, so the crawler has something to fetch.

    **This route did not exist, and its absence was the hole in the middle of
    the open-source branch.** `/register` proved control and issued a token, and
    then nothing asked where the files were — `INSERT INTO source` appeared only
    in the suite. A maintainer could walk the whole documented path, receive a
    token, and never be mirrored, with no error anywhere to tell them so.

    Authenticated, because it decides what we fetch under somebody's name.
    Separate from `verify`, because proving control and publishing files are
    different acts and a project may do the first while it is still writing the
    second — and because `verify` is also the recovery path, which must not
    quietly re-point a mirror at whatever the caller passed.

    The prefix defaults to the anchor and may be narrower. A project on shared
    hosting publishes under a path, and that path is what the fetch is confined
    to afterwards.

    Enrolling the same prefix twice changes nothing. Enrolling a *different* one
    adds a second source rather than moving the first, because `source` is
    unique on `(anchor_id, manifest_url)` and one anchor is allowed more than one
    `.podshl/`. A maintainer who has moved their files therefore has an old
    source still being crawled, and withdrawing is what stops it.
    """
    prefix = (body or {}).get("prefix") or ""
    try:
        with db.tx() as conn:
            anchor_id = _claimed_anchor(conn, host, x_podshl_claim)
            with conn.cursor() as cur:
                cur.execute("SELECT value, probe_prefix, taken_down_at FROM anchor WHERE id = %s",
                            (anchor_id,))
                anchor = cur.fetchone()
            if anchor["taken_down_at"]:
                return JSONResponse(
                    {"code": "taken_down",
                     "reason": "this anchor is withdrawn by a notice-and-action decision. "
                               "Enrolling it again is not something a republished file can "
                               "do — see /notice."}, status_code=409)

            root = forge.fetch_root(anchor)
            prefix = prefix or root
            if not isinstance(prefix, str) or not prefix.startswith(("https://", "http://")):
                return JSONResponse(
                    {"code": "bad_prefix", "reason": "prefix must be an http(s) URL"},
                    status_code=400)
            if not prefix.endswith("/"):
                prefix += "/"
            if not under_prefix(prefix, root):
                # The same rule the manifest's own endpoint obeys. An anchor
                # proves control of a location and cannot vouch for another one,
                # and that has to be true of what we agree to fetch as well as
                # of what the fetched file then claims.
                return JSONResponse(
                    {"code": "outside_anchor",
                     "reason": f"{prefix!r} does not lie under the verified anchor "
                               f"{anchor['value']!r}"
                               + ("" if root == anchor["value"]
                                  else f", whose files are served from {root!r}")},
                    status_code=400)

            manifest_url = prefix + ".podshl/agent.yaml"
            with conn.cursor() as cur:
                # Idempotent, and due immediately. Re-running it is how a
                # maintainer who moved their files says so.
                cur.execute(
                    "INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                    "VALUES (%s, %s, %s) "
                    "ON CONFLICT (anchor_id, manifest_url) DO UPDATE SET "
                    "  fetch_prefix = EXCLUDED.fetch_prefix, next_fetch_at = now(), "
                    "  mirror_state = 'serving' "
                    "RETURNING id, (xmax = 0) AS created",
                    (anchor_id, manifest_url, prefix))
                row = cur.fetchone()
    except NotClaimed as e:
        return _err(e, status=403)

    return {
        "enrolled": host,
        "manifest_url": manifest_url,
        "fetch_prefix": prefix,
        "created": row["created"],
        "note": "queued for the next crawl pass. A different prefix adds a source "
                "rather than moving this one. Nothing is served until the whole "
                "source validates — a partially valid source is not mirrored at all, "
                "because the half that is missing is invisible to whoever reads the "
                "other half.",
        "then": f"/dashboard#{host}",
    }


@app.post("/claim/{host}/revoke")
def claim_revoke(host: str, x_podshl_claim: str | None = Header(default=None)):
    """Stop every token for this anchor, including the one presented.

    For the maintainer who still holds a token and believes it leaked. It revokes
    **all** of them, because tokens are indistinguishable to whoever holds them —
    each plaintext was shown exactly once — so "revoke the one I am holding" would
    leave the leaked one alive. Revoke everything, then re-prove, is the only
    operation whose outcome a maintainer can reason about.

    Authenticated, and it has to stay that way: an unauthenticated revoke is a
    denial of service against every maintainer at once. `SV21` forbids the word
    `reset` in a public path, which is why this one is `revoke` — the guard named
    it.
    """
    try:
        with db.tx() as conn:
            anchor_id = _claimed_anchor(conn, host, x_podshl_claim)
            with conn.cursor() as cur:
                cur.execute(
                    "UPDATE dashboard_claim SET revoked_at = now(), "
                    "revoked_reason = 'revoked_by_holder' "
                    "WHERE anchor_id = %s AND revoked_at IS NULL",
                    (anchor_id,),
                )
                revoked = cur.rowcount
    except NotClaimed as e:
        return _err(e, status=403)
    return JSONResponse(
        {"revoked": revoked, "host": host,
         "then": f"POST /claim/{host} to prove control again",
         "note": "every token for this domain has stopped working, including the one "
                 "you just used."},
        headers={"Cache-Control": "no-store"},
    )


def _claimed_anchor(conn, host: str, token: str | None) -> int:
    """The dashboard is for a claimed anchor and nobody else.

    `SV19` — the gap report for an unclaimed domain is not accessible to anyone.
    The service this replaces served it to whoever asked, which is the one line
    `SERVER.md` says ends the company rather than costing it a deal.
    """
    if not token:
        raise NotClaimed("this dashboard is private to whoever controls the domain")
    # A self-hosted forge answers on a port and the caller addresses it with one;
    # `host` holds the bare name, because its CHECK admits no colon. Split here
    # rather than in each caller, so source, dashboard, revoke and withdraw
    # cannot disagree about what an address is.
    host = host.split(":", 1)[0]
    with conn.cursor() as cur:
        cur.execute(
            # No `kind` filter here, and that is deliberate: the token names
            # exactly one anchor and is the credential, so the host is a second
            # check rather than the address. Excluding repositories locked them
            # out of their own dashboard — the guard belongs where there is no
            # token to pin the row, which is `/diagnose` and `/mirror`.
            "SELECT a.id FROM dashboard_claim c JOIN anchor a ON a.id = c.anchor_id "
            "WHERE a.host = %s AND c.token_hash = %s "
            "AND c.revoked_at IS NULL AND c.expires_at > now()",
            (host, hashlib.sha256(token.encode()).digest()),
        )
        row = cur.fetchone()
    if not row:
        raise NotClaimed("this dashboard is private to whoever controls the domain")
    return row["id"]


@app.post("/claim/{host}/withdraw")
def claim_withdraw(host: str, x_podshl_claim: str | None = Header(default=None)):
    """Leave. Stop being mirrored, and stop being attested.

    A maintainer who no longer wants to take part should not have to file a
    notice against themselves, and should not have to ask us. Participation was
    always a positive act; withdrawing it is one too.

    It ends in the same place a takedown does — the mirror withheld, the
    attestation withdrawn, the anchor back to `unknown` — because that is what
    un-enrolment *is*: the state of a project that never registered.
    **Nothing of theirs is touched.** The files live in their repository; we
    only ever held a copy.

    Logged, like every other withdrawal, with `self_withdrawn` as the reason.
    That distinction matters for the same reason `superseded` and
    `revoked_by_holder` are kept apart: a project that left is not a project
    that was reported, and a log that cannot tell those apart would make leaving
    look like an accusation.

    The tokens go too. Somebody who has left should not be holding a live
    credential for a dashboard that now has nothing in it.
    """
    try:
        with db.tx() as conn:
            anchor_id = _claimed_anchor(conn, host, x_podshl_claim)
            with conn.cursor() as cur:
                cur.execute("SELECT kind, value FROM anchor WHERE id = %s", (anchor_id,))
                a = cur.fetchone()

            seq = log_store.append(
                conn, "takedown",
                {
                    "anchor": {"kind": a["kind"], "value": a["value"]},
                    "reason_code": "self_withdrawn",
                    "action": "degraded",
                    "means": "the maintainer withdrew. The mirror is withheld and the "
                             "anchor is `unknown` — the state of a project that never "
                             "registered. Nothing in their repository was touched, and "
                             "this is not a finding against them.",
                },
                anchor_id=anchor_id,
            )
            with conn.cursor() as cur:
                cur.execute("UPDATE source SET mirror_state = 'withheld' WHERE anchor_id = %s",
                            (anchor_id,))
                cur.execute(
                    "UPDATE attestation SET withdrawn_at = now(), withdrawn_kind = 'degraded', "
                    "withdrawn_reason = 'self_withdrawn', withdrawn_seq = %s "
                    "WHERE anchor_id = %s AND withdrawn_at IS NULL",
                    (seq, anchor_id))
                cur.execute("UPDATE anchor SET status = 'unknown' WHERE id = %s", (anchor_id,))
                cur.execute(
                    "UPDATE dashboard_claim SET revoked_at = now(), "
                    "revoked_reason = 'revoked_by_holder' "
                    "WHERE anchor_id = %s AND revoked_at IS NULL", (anchor_id,))
                tokens = cur.rowcount
    except NotClaimed as e:
        return _err(e, status=403)
    return JSONResponse(
        {"withdrawn": host, "log_seq": seq, "tokens_revoked": tokens,
         "means": "you are no longer mirrored and no longer attested. Your anchor is "
                  "`unknown`, which is where every project starts and says nothing "
                  "about anybody.",
         "your_files": "untouched — they were always yours, and we only held a copy",
         "coming_back": f"POST /claim/{host} whenever you like. Nothing here holds it "
                        f"against you.",
         "record": f"/log/entries?start={seq}&end={seq + 1}"},
        headers={"Cache-Control": "no-store"},
    )


@app.get("/dashboard/{host}")
def dashboard(host: str, response: Response,
                    x_podshl_claim: str | None = Header(default=None)):
    """What the vendor sees about itself, and the limit stated alongside.

    The limit is the honest sales argument: without a connected agent this shows
    *that* something recurs, never *why*.
    """
    # Private and per-vendor. Without these a shared proxy may hold it, and a
    # cache keyed on URL alone could hand it to the next request for this host.
    # Set on both the page and the refusal: a 403 says whether a claim exists
    # for this host, which is itself a per-vendor fact, and FastAPI drops the
    # injected response's headers when a route returns its own — so the refusal
    # carries them explicitly.
    private = {"Cache-Control": "no-store", "Vary": "X-Podshl-Claim"}
    response.headers.update(private)
    try:
        with db.read() as conn:
            anchor_id = _claimed_anchor(conn, host, x_podshl_claim)
            with conn.cursor() as cur:
                # **Every configuration above the floor, up to a stated cap, and
                # the count of all of them.** This was `LIMIT 100` and said
                # nothing: a project with 263 saw 100 and had no way to know
                # the rest existed (`SV106`).
                cur.execute(
                    "SELECT count(*) AS n FROM cluster WHERE subject_kind = 'domain' "
                    "AND subject_host = %s AND peak_epoch_reporters >= %s", (host, K_REPORTERS))
                total = cur.fetchone()["n"]
                cur.execute(
                    "SELECT id, problem_class, reports_total, peak_epoch_reporters, signature "
                    "FROM cluster WHERE subject_kind = 'domain' AND subject_host = %s "
                    "AND peak_epoch_reporters >= %s "
                    "ORDER BY peak_epoch_reporters DESC, id LIMIT %s",
                    (host, K_REPORTERS, MAX_CLUSTERS),
                )
                rows = cur.fetchall()
                # Their own source, so their own solutions can be walked against
                # what recurs. Scoped to this anchor: the whole page is.
                cur.execute("SELECT s.id, s.manifest_url, s.last_fetched, s.last_changed, "
                            "       s.consecutive_silence, s.last_refusal, "
                            "       s.classes_without_a_tree, "
                            "       c.commit, c.json -> 'collect' AS collect "
                            "FROM source s LEFT JOIN card c ON c.source_id = s.id "
                            "     AND c.valid_to IS NULL "
                            "WHERE s.anchor_id = %s AND s.mirror_state = 'serving' "
                            "ORDER BY s.id LIMIT 1", (anchor_id,))
                src = cur.fetchone()
                trees = []
                if src:
                    cur.execute("SELECT problem_class FROM tree WHERE source_id = %s "
                                "AND valid_to IS NULL ORDER BY problem_class", (src["id"],))
                    trees = [r["problem_class"] for r in cur.fetchall()]
                # The floor applies to every group on the page, not only to the
                # clusters. A model class three people used, or a fact two
                # people typed, is a group below k — and a small group beside
                # the clusters it belongs to is a description of its members.
                # Applied in the query, so there is no row to forget to filter.
                #
                # And counted the way a cluster is (`GR1b`): a `seen_key` is a
                # new key in every cluster and every month, so distinct keys
                # across them made one person with five configurations, or one
                # report a month, into five people. A group clears the floor
                # only if k distinct reporters hold it within one cluster and
                # one month; the figure shown is still the all-time sum.
                cur.execute(
                    "SELECT model_class, sum(n)::int AS reporters FROM ("
                    "  SELECT o.model_class, o.cluster_id, o.epoch, "
                    "         count(DISTINCT o.seen_key) AS n "
                    "  FROM observation o JOIN cluster c ON c.id = o.cluster_id "
                    "  WHERE c.subject_kind = 'domain' AND c.subject_host = %s "
                    "  GROUP BY 1, 2, 3"
                    ") per_month GROUP BY model_class HAVING max(n) >= %s "
                    "ORDER BY reporters DESC",
                    (host, K_REPORTERS))
                model_classes = cur.fetchall()
                # Which facts arrived typed rather than read, and how often. A
                # publisher cannot judge their own rule without this: an outcome
                # that turned on a value somebody supplied says nothing about
                # whether the rule is right.
                cur.execute(
                    "SELECT fact, sum(reports)::int AS reports, sum(n)::int AS reporters FROM ("
                    "  SELECT k AS fact, o.cluster_id, o.epoch, count(*) AS reports, "
                    "         count(DISTINCT o.seen_key) AS n "
                    "  FROM observation o JOIN cluster c ON c.id = o.cluster_id, "
                    "       LATERAL jsonb_object_keys(o.stated) AS k "
                    "  WHERE c.subject_kind = 'domain' AND c.subject_host = %s "
                    "  GROUP BY 1, 2, 3"
                    ") per_month GROUP BY fact HAVING max(n) >= %s "
                    "ORDER BY reports DESC LIMIT 50", (host, K_REPORTERS))
                stated_facts = cur.fetchall()
            # After the cursor block, because explaining a cluster opens cursors
            # of its own — walking this project's trees against what recurs.
            rows = explain.explain(conn, src["id"] if src else None, rows)
            # Where one of their answers helped some configurations and not
            # others. Computed from the rows above and nothing else, so it can
            # name no value and no count those rows do not already show.
            when_by_solution = {}
            if src:
                with conn.cursor() as cur:
                    cur.execute("SELECT solution_id, answers FROM solution "
                                "WHERE source_id = %s AND valid_to IS NULL", (src["id"],))
                    when_by_solution = {r["solution_id"]: (r["answers"] or {}).get("when") or {}
                                        for r in cur.fetchall()}
            suggestions = repartition.forks(
                rows, when_by_solution,
                tree_build.known_facts(src["collect"] or []) if src else None)
            triage = explain.triage(rows)
    except NotClaimed as e:
        return _err(e, status=403, headers=private)

    return {
        "host": host,
        "policy": {
            "free": True, "private": True, "unconditional": True,
            "statement": "This report is free, confidential and conditional on nothing. "
                         "It is not published and it is not used as leverage.",
        },
        "limitation": "Without a connected agent there are no declared probes. This shows "
                      "THAT something recurs, not why. With one it would show the cause "
                      "rather than the frequency.",
        "counter_means": "reported, never occurred — this under-reports reality",
        "total": total,
        "shown": len(rows),
        "clusters": rows,
        "triage": triage,
        # What we could and could not make of their files. Their own, so shown
        # here and nowhere else (`SV105`).
        "files": {
            "manifest_url": src["manifest_url"] if src else None,
            "commit": src["commit"] if src else None,
            "last_fetched": src["last_fetched"].isoformat() if src and src["last_fetched"] else None,
            "last_changed": src["last_changed"].isoformat() if src and src["last_changed"] else None,
            "unanswered_fetches": src["consecutive_silence"] if src else 0,
            "last_refusal": src["last_refusal"] if src else None,
            "trees": trees,
            "classes_without_a_tree": (src["classes_without_a_tree"] or {}) if src else {},
        },
        "suggestions": suggestions,
        "suggestions_means": "a suggestion for you to judge, never a change: which fact separates "
                             "where your answer worked from where it did not, found by comparing "
                             "the configurations above",
        "model_classes": model_classes,
        "stated_facts": stated_facts,
        "stated_means": "supplied by the person, not measured on their machine — an outcome "
                        "that turned on one of these is not evidence about your rule",
        "reading": "A small local model solving something from public knowledge means the "
                   "information was available and the product surface failed to convey it. "
                   "That is a UX defect, not a knowledge gap.",
    }


@app.get("/dashboard/{host}/log")
def dashboard_log(host: str, response: Response,
                        x_podshl_claim: str | None = Header(default=None)):
    """Everything the log says about this anchor.

    This is the other half of the answer to "somebody has to make watching
    easy": every claimed anchor is already an authenticated monitor of its own
    entries, so the population that most cares becomes the population that
    watches. Being the only witness of our own log is the failure mode.
    """
    # Private and per-vendor, on the answer and on the refusal alike — see
    # `dashboard` for why the refusal carries them explicitly.
    private = {"Cache-Control": "no-store", "Vary": "X-Podshl-Claim"}
    response.headers.update(private)
    try:
        with db.read() as conn:
            anchor_id = _claimed_anchor(conn, host, x_podshl_claim)
            return {"host": host, "entries": log_store.for_anchor(conn, anchor_id)}
    except NotClaimed as e:
        return _err(e, status=403, headers=private)


# ----------------------------------------------------------------- discovery
#
# The whole catalogue, signed, fetched rather than queried. There is no
# `/search?q=` and there must never be one: this server promises it holds no
# query logs, and a name query is worse than a domain query because what a user
# types is the problem they have. The client downloads this and searches it on
# the user's own machine, so we learn that somebody fetched the index and never
# what they were looking for.

@app.get("/index")
def discovery_index(response: Response, request: Request):
    """Everything currently served, with the tree head it is consistent with.

    Cacheable on purpose. This is the one request every client makes and none of
    them reveals anything, so it should be cheap to serve and cheap to mirror —
    a CDN in front of this sees all of the traffic and none of the interest.
    """
    with db.read() as conn:
        head, body, tag = index_feed.prepare(conn)

    # The tag is known before anything is signed, and the 304 carries the same
    # cache headers as the body: a conditional request that matches costs no
    # signature, and a cache that gets a 304 without `Cache-Control` treats the
    # entry as expired again on its next look. Explicit headers, because FastAPI
    # drops the injected response's headers when a route returns its own.
    cache = {"ETag": tag, "Cache-Control": "public, max-age=300"}
    if pages.etag_matches(request.headers.get("if-none-match"), tag):
        return Response(status_code=304, headers=cache)
    return JSONResponse(index_feed.sign(head, body), headers=cache)


# ------------------------------------------------------------------ the mirror
#
# Serve from our database. One indexed lookup, no external call — fetching from
# a forge on the request path would make their rate limits our capacity and
# their outage our outage.

@app.get("/mirror/{host}")
def mirror(host: str, response: Response,
           repo: str | None = Query(default=None),
           forge_shape: str | None = Query(default=None, alias="forge")):
    """What we serve for an anchor, and the commit it came from.

    **Publishing the commit is what makes the mirror checkable.** Anyone can
    fetch that commit from the source and diff it against this. A mirror whose
    provenance cannot be checked is just a copy somebody asks you to trust.
    """
    response.headers["Cache-Control"] = "public, max-age=60"

    # A repository is addressed by identity, not by host: `github.com` is shared
    # by every repository there, so a bare host would answer with whichever row
    # was first — somebody else's project, under a name they never claimed. The
    # query string rather than the path, because this is a GET and the identity
    # carries slashes; `forge` is needed only for a host the table does not know,
    # which is the self-hosted case.
    identity = None
    if repo is not None:
        parsed = forge.parse(_repo_url(host, repo), forge_shape)
        if parsed is None:
            return JSONResponse(
                {"host": host, "attested": False,
                 "note": f"{host}/{repo} is not a repository this can address. "
                         f"{forge.supported()}."},
                status_code=404, headers={"Cache-Control": "public, max-age=60"})
        identity = parsed[0]

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT a.host, a.value AS anchor_url, a.kind AS anchor_kind, "
                "       a.status AS anchor_status, a.last_confirmed, "
                "       s.id AS source_id, s.manifest_url, s.declared_status, "
                "       s.successor_url, s.forge_archived, s.forge_last_commit_at, "
                "       c.json, c.commit, c.content_hash, c.langs, c.log_seq "
                "FROM anchor a "
                "JOIN source s ON s.anchor_id = a.id AND s.mirror_state = 'serving' "
                "JOIN card c ON c.source_id = s.id AND c.valid_to IS NULL "
                + ("WHERE a.kind = 'repo' AND a.value = %s"
                   if identity else "WHERE a.host = %s AND a.kind <> 'repo'"),
                (identity or host,))
            row = cur.fetchone()
            if not row:
                # Not an accusation. `unknown` is the state of everyone who never
                # registered, and the client proceeds on the protocol's own trust.
                # Cacheable like the answer it stands in for — a 404 without
                # `Cache-Control` is re-asked on every look, and "not here" is
                # the most common answer this route gives.
                return JSONResponse(
                    {"host": host, **({"anchor": identity} if identity else {}),
                     "attested": False,
                     "note": "not attested here. That says nothing about them — "
                             "discovery works without us, and a vendor who never "
                             "heard of us still works."
                             + ("" if identity else
                                " A repository is addressed as "
                                "?repo=owner/name, because a forge's host is "
                                "shared by everything on it.")},
                    status_code=404, headers={"Cache-Control": "public, max-age=60"})

            cur.execute(
                "SELECT solution_id, answers, proposes, text_by_lang, severity, commit "
                "FROM solution WHERE source_id = %s AND valid_to IS NULL ORDER BY solution_id",
                (row["source_id"],))
            solutions = cur.fetchall()

    # An abandoned project's solutions are usually still correct — the software
    # did not change either — so `stale` keeps being served with its age stated.
    # What the user needs is the age, not a refusal.
    return {
        "host": row["host"],
        # What was verified, in full. For a domain the two say the same thing;
        # for a repository the host is shared and only this identifies it.
        "anchor_url": row["anchor_url"],
        "attested": True,
        "anchor": {
            "status": row["anchor_status"],
            "last_confirmed": row["last_confirmed"],
            "means": "control of the location, last confirmed at that time. Not a "
                     "statement about the project being maintained.",
        },
        "project": {
            # The developer's own word first, the forge's act second, our
            # observation last — and the third never phrased as the first.
            "declared_status": row["declared_status"],
            "successor": row["successor_url"],
            "forge_archived": row["forge_archived"],
            "observed_last_commit": row["forge_last_commit_at"],
        },
        "serving": {
            "commit": row["commit"],
            "content_sha256": bytes(row["content_hash"]).hex(),
            "source": row["manifest_url"],
            "log_seq": row["log_seq"],
            "check_us": "fetch that commit from the source and diff it against this",
        },
        "langs": row["langs"],
        "card": row["json"],
        "solutions": solutions,
    }


@app.post("/notice")
async def notice(request: Request):
    """Notice and action. Two duties survive any risk appetite: a route, and a
    statement of reasons to the affected party — which is the same log entry.

    This route records. It does not act: a takedown is a person's decision,
    made on the operator's own listener, and a route anybody can call must not
    be able to un-enrol anybody. The reply says so.
    """
    body = await read_json_object(request, MAX_BODY)
    if isinstance(body, JSONResponse):
        return body
    notifier = body.get("notifier")
    if not isinstance(notifier, dict):
        return _bad("bad_notifier", "notifier must be an object with a name and a contact")
    def record():
        with db.tx() as conn:
            return takedown.receive(
                conn,
                reason_code=body.get("reason_code", ""),
                notifier=notifier,
                anchor_host=body.get("anchor_host"),
                problem_class=body.get("problem_class"),
            )

    try:
        out = await run_in_threadpool(record)
    except takedown.NoSuchPath as e:
        return _err(e, status=422)
    except ServerError as e:
        return _err(e, status=400)
    return JSONResponse(out, headers={"Cache-Control": "no-store"})


# ------------------------------------------------------------------ public figures

@app.get("/stats")
def stats():
    """Exactly two integers, and there is no parameter that could narrow them.

    It shows the size of the gap without naming anybody, which is the only
    version of this number that helps rather than threatens.
    """
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT * FROM ecosystem_totals")
            totals = cur.fetchone()
    return JSONResponse(totals, headers={"Cache-Control": "public, max-age=300"})
