"""Homoglyph detection, and why the outcome is a hold rather than a block.

`SERVER.md` is careful about the line this sits on. We attest **control of an
anchor, never entitlement to a name** — deciding who deserves *NVIDIA* would be
the chokepoint role the whole design rejects, and we are not a trademark
register and must not become one.

But homoglyphs are not a dispute. `exаmple.org` with a Cyrillic а is not a claim
about who owns a name; it is a technical trick with a technical answer, and the
answer is UTS 39 plus punycode display.

**Declining to attest is not blocking.** A held anchor is still fully reachable
as `unknown`, which is the state of everyone who never registered — so we can be
conservative here without becoming a chokepoint, and a false positive costs a
publisher a review rather than their existence. That asymmetry is why this can
be strict at all.

Two things it deliberately does *not* do:

* It does not compare against every other anchor. That would make attestation
  order-dependent and let a squatter who registered first block a legitimate
  anchor — the register role again, arrived at sideways.
* It never looks at a declared problem class. A project that repairs NVIDIA
  driver problems has to be able to say so; a takedown reaches an anchor, never
  a class.
"""
from __future__ import annotations

import json
import unicodedata
from functools import lru_cache
from pathlib import Path

import idna

_DATA = Path(__file__).resolve().parent.parent / "confusables.json"

#: Attacks that survive the Unicode table because both characters are already
#: plain ASCII: the table maps *toward* ASCII, so `rn` and `m` are simply two
#: different ASCII strings to it. These are the ones actually used in the wild.
ASCII_FOLD = (
    ("rn", "m"), ("vv", "w"), ("cl", "d"), ("nn", "m"),
    ("0", "o"), ("1", "l"), ("5", "s"), ("2", "z"), ("8", "b"),
)


@lru_cache(maxsize=1)
def _table() -> dict[str, str]:
    data = json.loads(_DATA.read_text(encoding="utf-8"))
    return data["mappings"]


@lru_cache(maxsize=1)
def table_version() -> str:
    return json.loads(_DATA.read_text(encoding="utf-8"))["unicode_version"]


def normalise_host(raw: str) -> tuple[str, str | None]:
    """(punycode, unicode-for-display-or-None).

    A host that does not round-trip through IDNA is refused outright rather than
    guessed at: if two implementations would read it differently, it is not a
    name we can attest anything about.
    """
    raw = raw.strip().rstrip(".").lower()
    try:
        ascii_form = idna.encode(raw, uts46=True, std3_rules=True).decode()
    except idna.IDNAError as e:
        raise ValueError(f"{raw!r} is not a usable hostname: {e}") from e
    try:
        unicode_form = idna.decode(ascii_form)
    except idna.IDNAError:
        unicode_form = None
    return ascii_form, (unicode_form if unicode_form and unicode_form != ascii_form else None)


def skeleton(label: str) -> str:
    """The UTS 39 skeleton: what this label looks like, rather than what it is.

    Two labels with the same skeleton are visually confusable. The ASCII folds
    are applied after the Unicode mapping, because the table maps toward ASCII
    and cannot see that `rn` and `m` look alike once both are already there.
    """
    label = unicodedata.normalize("NFD", label.lower())
    mapped = "".join(_table().get(c, c) for c in label)
    mapped = unicodedata.normalize("NFD", mapped).lower()
    for pair, replacement in ASCII_FOLD:
        mapped = mapped.replace(pair, replacement)
    return mapped.replace("-", "")


def is_single_script(label: str) -> bool:
    """Mixed scripts in one label are the classic homoglyph shape.

    Latin plus digits and hyphens is the normal case; Latin plus Cyrillic in one
    word is not something anybody does by accident.
    """
    scripts = set()
    for ch in label:
        if ch.isascii() or not ch.isalpha():
            continue
        name = unicodedata.name(ch, "")
        for script in ("LATIN", "CYRILLIC", "GREEK", "ARMENIAN", "HEBREW", "ARABIC"):
            if name.startswith(script):
                scripts.add(script)
                break
    if any(c.isascii() and c.isalpha() for c in label):
        scripts.add("LATIN")
    return len(scripts) <= 1


def repo_labels(value: str) -> list[str]:
    """The parts of a repository identity a claimant chose.

    `https://github.com/owner/name/` gives `["owner", "name"]`. The host is not
    in it, and that is the point: the claimant did not choose `github.com`, it is
    shared by every repository there, and examining it would ask whether *the
    forge* resembles somebody's mark -- a question whose two answers are "no,
    for everyone" and "yes, so hold every project on GitHub".
    """
    tail = value.split("://", 1)[-1]
    parts = [p for p in tail.split("/") if p]
    return [p.lstrip("~") for p in parts[1:]]


def hold_reason(conn, host: str, *, kind: str = "url", value: str = "") -> str | None:
    """A reason to hold this anchor for review, or None.

    Returns a reason rather than a boolean so the operator queue can say *why*,
    and so a publisher told they are held can be told what to change.

    **What is examined depends on what the claimant chose.** For a domain it is
    every label of the host. For a repository it is the owner and the name,
    because the host there is the forge's and is shared -- so a check on it
    would be asking the wrong question of everybody at once. Impersonation on a
    forge happens in the owner: `dx111geo/engram` is a letter out, and the host
    is identical either way.
    """
    if kind == "repo":
        labels = repo_labels(value)
        if not labels:
            return "a repository anchor with no owner or name in it"
        ascii_host = "/".join(labels)
        for label in labels:
            if not is_single_script(label):
                return (f"{label!r} mixes scripts, which is the shape a homoglyph "
                        f"attack takes. On a forge the host is shared, so the owner "
                        f"is where this happens.")
    else:
        try:
            ascii_host, unicode_host = normalise_host(host)
        except ValueError as e:
            return f"not a usable hostname: {e}"

        labels = ascii_host.split(".")
        if unicode_host:
            for label in unicode_host.split("."):
                if not is_single_script(label):
                    return (f"the label {label!r} mixes scripts, which is the shape a "
                            f"homoglyph attack takes. Shown as {ascii_host}.")

    # **Every label, not the second-to-last one.** Guessing the registrable
    # label as `labels[-2]` picks the *hosting platform* on exactly the shared
    # suffixes this project names as its primary supply side: `nvidia.github.io`
    # examines `github`, `nvidia.co.uk` examines `co`, and neither is ever
    # compared against the curated marks. `manifest.py` calls forge hosting the
    # point — "GitHub, GitLab, Codeberg and a bare nginx all work identically" —
    # so the bypass covered the main publishing path.
    #
    # A public-suffix list would name the registrable label correctly and is a
    # dependency, an update cadence and a source of its own staleness. Checking
    # all of them needs none of that and errs the safe way: the failure mode is
    # a *hold on review*, and an anchor held is `unknown` — the state of everyone
    # who never registered. Being conservative here costs a person's afternoon;
    # being wrong the other way costs the thing this check exists for.
    #
    # The mixed-script check directly above already iterated every label.
    def probe_of(label: str) -> str:
        # Guarded. `normalise_host` accepted the host as a whole, which does not
        # promise that every individual label decodes — and an exception raised
        # here escapes `ingest_one`, which catches only `IngestRefused`. Falling
        # back to the encoded form keeps the label in the comparison rather than
        # dropping it, so a label that will not decode is still checked as text.
        if label.startswith("xn--"):
            try:
                label = idna.decode(label)
            except idna.IDNAError:
                pass
        return skeleton(label)

    probes = [probe_of(label) for label in labels if label]

    with conn.cursor() as cur:
        cur.execute("SELECT label, skeleton, owner_host FROM well_known_mark")
        marks = cur.fetchall()

    for mark in marks:
        if mark["owner_host"] and mark["owner_host"] == ascii_host:
            return None  # It is theirs.
    # Known and accepted: a mark's owner is recognised by *host*, so the same
    # vendor's repository on a forge is not recognised as theirs -- NVIDIA's own
    # `github.com/nvidia/...` is held for carrying the mark `nvidia`. Held is not
    # blocked; the anchor stays as reachable as one that never registered, and a
    # person decides. Recognising a mark owner on a forge would mean marks
    # recording forge identities as well as hosts, which is a decision about
    # whose name is whose, and this project has no standing to make it quietly.

    # SV33: the same skeleton is a spoof — it is meant to be mistaken for the
    # mark, which is a technical trick rather than a dispute about a name.
    for mark in marks:
        for probe in probes:
            if probe == mark["skeleton"]:
                return (f"is confusable with the well-known mark {mark['label']!r} — "
                        f"same skeleton {probe!r}")

    # SV34: *carrying* a mark is different from being confusable with it.
    # `nvidia-community-fixes.org` is not pretending to be NVIDIA, and it may
    # well be a legitimate project — but whether a domain may carry somebody
    # else's mark is exactly the question this project has no standing to
    # answer, so it goes to a person rather than being decided here.
    #
    # Held, not blocked. The anchor stays reachable as `unknown`, which is the
    # state of everyone who never registered, so a wrong hold costs a review
    # rather than an existence. That asymmetry is the only reason this can be
    # conservative at all.
    #
    # And this looks at the *anchor* only. A manifest that declares
    # `nvidia.driver.flicker` as a problem class is describing what it repairs,
    # and nothing here ever reads one.
    for mark in marks:
        if len(mark["skeleton"]) < 4:
            continue
        for probe in probes:
            if mark["skeleton"] in probe:
                return (f"carries the well-known mark {mark['label']!r} without being "
                        f"{mark['owner_host'] or 'its owner'} — a person decides this, "
                        f"not us")
    return None


def add_mark(conn, label: str, owner_host: str | None, added_by: str) -> int:
    """Add a mark to the curated list.

    Curated, and added by a person. A list that grew automatically from
    registrations would be a trademark register with extra steps, and this
    project has one sentence about that: we must not become one.
    """
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO well_known_mark (label, skeleton, owner_host, added_by) "
            "VALUES (%s, %s, %s, %s) RETURNING id",
            (label.lower(), skeleton(label), owner_host, added_by))
        return cur.fetchone()["id"]
