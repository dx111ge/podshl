"""The discovery index: the whole catalogue, signed, fetched rather than queried.

A user types the name of **whatever is not working** — a program (`pip`), a
device or its maker (`nvidia`), a product (`datev`) — and the client answers *do
we have anything for this* **on the user's own machine**. Both kinds matter and
neither is the primary one: `problem_classes` name other people's software and
hardware alike, and a project that repairs driver problems has to be able to say
`nvidia` exactly as one that repairs installs says `pip`.

That is the whole design constraint, and it rules out the obvious
implementation.

`GET /search?q=` cannot exist here. This server's `/` promises it holds no query
logs, and `SV21` already forbids a per-domain lookup as the surveillance
endpoint the log avoids. A *name* lookup is worse than a domain lookup, because
what a user types is the problem they have. So discovery is not a question asked
of us: it is an artefact the client already holds and searches offline.

What is searchable is earned rather than claimed. There is no display name here
because `agent.yaml` has none — the display name is the verified anchor, so
there is nowhere to type somebody else's, and a self-asserted name in a
searchable index would put that vector straight back. Two sources instead:

  * **`problem_classes`**, which name other people's software on purpose. That
    is nominative use, and a takedown reaches an anchor, never a class — or a
    vendor could forbid anyone from saying its name out loud.
  * **Tokens from the verified anchor**, so a project confirmed at
    `https://forge.example/psf/requests/` is findable as `requests` because it
    proved control of that URL.

Every entry carries the log sequence its card was attested at, and the whole
document is signed against a tree head. So the index cannot contain a project
that is not in the log, and a monitor can check that offline by walking it.
"""
from __future__ import annotations

import hashlib
import time

from ..jws import sign_detached
from . import log_store, sth

#: Bumped when the shape changes in a way a client cannot read through.
VERSION = 1

#: A token has to be worth typing. One- and two-character fragments match
#: everything and help nobody.
MIN_TOKEN = 3


def _tokens(host: str, anchor_url: str, problem_classes: list[str],
            kind: str = "url") -> list[str]:
    """What a user could type and reasonably expect to land here.

    Every dot segment of every problem class, plus the labels of the verified
    host and the path segments of the verified anchor URL. Segments rather than
    a leading-segment convention: a publisher who writes
    `pip.install.wheel-missing` is findable as `pip`, and one who writes
    `install.wheel-missing` is still findable as `install`, without either being
    forced into a shape the schema does not require.

    **A repository contributes no host labels.** `github.com` is shared by every
    repository on it, so emitting `github` would make one token match every such
    anchor at once -- and it is the forge's name, never the project's. What is
    left is the owner and the repository, which is what somebody would type, and
    the classes the project itself declared.
    """
    out: set[str] = set()
    for cls in problem_classes:
        for part in str(cls).replace("/", ".").replace("-", ".").split("."):
            if len(part) >= MIN_TOKEN:
                out.add(part.lower())
    for label in (host.split(".") if kind != "repo" else []):
        # Not `com`, `org`, `io`: a public suffix matches half the index.
        if len(label) >= MIN_TOKEN and label not in ("com", "org", "net", "www"):
            out.add(label.lower())
    tail = anchor_url.split("://", 1)[-1]
    for seg in tail.split("/")[1:]:
        if len(seg) >= MIN_TOKEN:
            out.add(seg.lower())
    return sorted(out)


def entries(conn) -> list[dict]:
    """Everything currently served, in a stable order.

    Held anchors are excluded. A hold means the name itself is contested — a
    confusable of somebody else's — and putting a contested name into the thing
    users search by name is precisely the wrong moment to be relaxed about it.
    """
    with conn.cursor() as cur:
        cur.execute(
            "SELECT a.host, a.host_unicode, a.value AS anchor_url, a.kind, a.status, "
            "       a.last_confirmed, s.declared_status, s.successor_url, "
            "       c.json, c.langs, c.commit, c.content_hash, c.log_seq "
            "FROM anchor a "
            "JOIN source s ON s.anchor_id = a.id AND s.mirror_state = 'serving' "
            "JOIN card c ON c.source_id = s.id AND c.valid_to IS NULL "
            "WHERE a.attest_hold IS NULL "
            "ORDER BY a.host"
        )
        rows = cur.fetchall()

    out = []
    for r in rows:
        card = r["json"] or {}
        classes = sorted({str(c) for c in (card.get("problem_classes") or [])})
        labels = card.get("class_labels") or {}
        if not isinstance(labels, dict):
            labels = {}
        gloss = card.get("glossary") or {}
        keep = gloss.get("keep") if isinstance(gloss, dict) else None
        keep = [t for t in keep if isinstance(t, str)] if isinstance(keep, list) else []
        out.append({
            "host": r["host"],
            # Display only, and never shown without `host` — the same rule the
            # anchor table states for it.
            "host_unicode": r["host_unicode"],
            "anchor_url": r["anchor_url"],
            # What kind of location was verified, because it is not one claim.
            # A domain holds without a third party -- DNS and TLS say who served
            # the bytes. A repository does not: the forge decides who may write
            # there, so the forge is a trusted third party for every one of
            # these, and a client that showed both as "control confirmed" would
            # be putting one sentence over two strengths of evidence.
            "anchor_kind": r["kind"],
            "problem_classes": classes,
            # The sentence a person picks a class by, where the maintainer
            # wrote one. Only for classes this entry actually declares, so a
            # label cannot smuggle in a class the rules do not answer, and
            # absent entirely when nobody wrote any — a client falls back to
            # the identifier, which is what every client did before.
            "class_labels": {c: labels[c] for c in classes if c in labels},
            # **The glossary travels with the labels, because the labels need
            # it and nothing else can supply it in time.** A client translates
            # `class_labels` to put the question in the reader's language, and
            # that question comes *before* the consent under which the card is
            # fetched -- so at the only moment the terms are wanted, the card
            # that holds them has not been asked for. The result was the
            # failure `LG8` exists to prevent, on the first publisher sentence
            # a person ever reads: engram's `brain` arriving as an organ.
            #
            # Nothing is disclosed by moving it here. These are the project's
            # own published words out of its own `agent.yaml`, and this index
            # is public and signed. Bounded at ingest already -- `MAX_TERMS`
            # and `MAX_TERM` in `ingest/manifest.py` -- so no new limit is
            # needed and none is invented here.
            #
            # Flattened to `glossary_keep` rather than nested, the way
            # `class_labels` is flattened out of the card: an index entry is a
            # flat projection of a card, and `keep` is the only key a glossary
            # has.
            "glossary_keep": keep,
            "search_tokens": _tokens(r["host"], r["anchor_url"] or "", classes, r["kind"]),
            "langs": sorted(r["langs"] or []),
            # The publisher's own word, and the only one that is authoritative.
            "status": r["declared_status"] or "active",
            "successor_url": r["successor_url"],
            # Liveness is stated, never converted into a verdict. A client shows
            # age; it does not withdraw a dormant project.
            "anchor_status": r["status"],
            "last_confirmed": r["last_confirmed"].isoformat() if r["last_confirmed"] else None,
            "commit": r["commit"],
            "content_hash": bytes(r["content_hash"]).hex() if r["content_hash"] else None,
            "log_seq": r["log_seq"],
        })
    return out


def build(conn, *, tree_size: int | None = None, generated_ms: int | None = None) -> dict:
    """The document, before it is signed.

    **Only what the head can prove.** A card attested a moment ago has a
    sequence beyond the last signed head, and an entry a monitor cannot prove
    inclusion for is an entry we are asking to be taken on trust — which is the
    one thing a transparency log exists to stop. Sequences are zero-based and
    `tree_size` is the count, so entry *S* is inside a tree of size *N* exactly
    when `S < N`. Those entries appear in the next index, one head later.

    `generated_ms` is the head's own timestamp when one is given. The document
    used to carry the wall clock, so two requests a second apart produced two
    different signed bodies over identical content — which is a new signature
    on every request and a body no cache could ever match. The head's timestamp
    is the moment the log this document describes was last signed, which is
    what "generated" honestly means here.
    """
    size = log_store.tree_size(conn) if tree_size is None else tree_size
    rows = [e for e in entries(conn)
            if e["log_seq"] is not None and e["log_seq"] < size]
    return {
        "version": VERSION,
        "tree_size": size,
        "generated_ms": int(time.time() * 1000) if generated_ms is None else generated_ms,
        "entries": rows,
    }


def prepare(conn) -> tuple[dict, dict, str]:
    """The head, the unsigned body built to match it, and the body's ETag.

    The head comes first and the document is built to match it, rather than the
    document being built and a head found to fit — those differ exactly when the
    log grew between the two reads, and the second order would publish a claim
    no proof supports.

    Split from `signed` so the ETag is known *before* anything is signed. A
    request that already holds this body gets a 304 and costs no signature.
    """
    head = sth.current(conn)
    if head is None:
        # An operator that has never signed a head has nothing to publish an
        # index against, and must say so rather than serve one unproven. This
        # should not be reachable — migrating issues the first head — so it is
        # a named refusal and not a fallback.
        raise NoSignedHead(
            "this operator has never issued a signed head, so there is nothing "
            "an index could be published against. Run the migrations, which "
            "issue the first one.")
    body = build(conn, tree_size=head["sth"]["tree_size"],
                 generated_ms=head["sth"]["timestamp"])
    return head, body, etag(body)


class NoSignedHead(Exception):
    """Raised rather than serving an index nothing vouches for."""


def sign(head: dict, body: dict) -> dict:
    """Signed with the log key and the same detached-JWS-over-JCS scheme as
    everything else here, so a client that can already check a tree head needs
    no second verifier — and a monitor needs no code we have not shipped."""
    return {
        "index": body,
        "signature": sign_detached(sth.key(), body, kid=sth.log_id()),
        "sth": head["sth"],
        "sth_signature": head["signature"],
        "key_id": sth.log_id(),
    }


def signed(conn) -> dict:
    """The document, the head it is consistent with, and one signature."""
    head, body, _ = prepare(conn)
    return sign(head, body)


def etag(body: dict) -> str:
    """Cheap and content-derived, so a client that already has this index pays
    a 304 rather than the whole catalogue. Over the unsigned body: the same
    content is the same tag whatever the signature bytes happen to be."""
    index = body["index"] if "index" in body and "entries" not in body else body
    seed = f"{index['version']}:{index['tree_size']}:{index['generated_ms']}:" + ",".join(
        f"{e['host']}@{e['log_seq']}:{e['content_hash']}:{e['status']}:{e['anchor_status']}"
        for e in index["entries"]
    )
    return '"' + hashlib.sha256(seed.encode()).hexdigest()[:32] + '"'
