"""Parsing `.podshl/agent.yaml` and the solution files it lists.

Plain YAML and Markdown front matter, fetched over plain HTTPS. No forge API,
so GitHub, GitLab, Codeberg and a bare nginx all work identically — and a
developer with no domain, no server and no signing key can still publish, which
is the supply side the whole strategy depends on.

The parsing is deliberately strict and the errors deliberately specific. A
rejection a developer cannot act on is a rejection that gets worked around, and
this is the boundary: their CI is a convenience that lets a bad pull request
fail before merge, but ingest is what actually decides.
"""
from __future__ import annotations

import yaml

from ..errors import IngestRefused

#: Read from the published vocabulary rather than repeated here: the client
#: performs at most this many, so a manifest declaring more is describing work
#: that would be refused after the user had already consented to it.
MAX_READS = 24

#: Not in the vocabulary, because they bound a manifest rather than a client.
#: Both exist so that an oversized document is refused with a sentence instead
#: of reaching the database and failing there.
MAX_CLASSES = 64
#: A class label is the one line a person recognises their own problem by, so it
#: is a sentence and not a page. Longer than this and it is the answer, which
#: belongs in a solution file where the walk can reach it under consent.
MAX_DESCRIBES = 120
MAX_LANGS = 16

#: How many nodes a parsed document may hold, counting every visit. YAML
#: anchors and aliases let a kilobyte of text expand into a structure with
#: millions of leaves — the "billion laughs" shape — and `safe_load` builds it
#: happily, because the bytes were small. The object is walked before anything
#: reads it, and a repeated node is counted every time it is reached, because
#: that is what everything downstream will do with it too.
MAX_NODES = 20_000

#: What a manifest may carry into the database. The parsed document is stored
#: as the card, and a stored document is a served document: `/mirror` hands the
#: card to every client. Keys nobody here reads used to be stored and served
#: verbatim, which made the card a free channel for whatever a publisher put in
#: it. These are the keys the code reads, plus `escalate`, which the client
#: reads off the mirrored card to know where a person is reached.
MANIFEST_KEYS = frozenset({
    "endpoint", "problem_classes", "langs", "solutions", "status", "collect",
    "commit", "successor", "escalate", "glossary", "class_labels",
    "solution_sha256",
})

#: A glossary's bounds. Its terms enter the prompt of the reader's own model, so
#: each is one short line, and there are few of them — a project's own words,
#: not a dictionary.
MAX_TERMS = 50
MAX_TERM = 64


def _glossary(raw) -> dict:
    """`glossary: {keep: [...]}` — the words that are the project's own.

    **Only `keep`, because that is what was measured to work.** `gemma3:4b`
    wrote engram's *brain* as *cerveau* in three French runs of three; told to
    keep it as written, it did so in six of six. Told to write it as a given
    word per language, it ignored that in six of six, and `qwen2.5:7b` followed
    it in German and not once in French. A rendering the reader's model does
    not apply is a promise to the maintainer that nothing keeps, so the format
    has no field for one — and a key it does not have is refused, not ignored,
    so a maintainer who writes one is told (`SV107`).
    """
    if not isinstance(raw, dict):
        raise IngestRefused("agent.yaml: glossary must be a mapping with a `keep` list")
    unknown = sorted(set(raw) - {"keep"})
    if unknown:
        raise IngestRefused(
            f"agent.yaml: glossary has {', '.join(map(repr, unknown))}, and only `keep` is "
            f"read. A reader's model keeps a term as written reliably and renders it as a "
            f"word you choose unreliably, so the glossary names terms to keep and nothing else.")
    keep = raw.get("keep", [])
    if not isinstance(keep, list) or not all(isinstance(t, str) for t in keep):
        raise IngestRefused("agent.yaml: glossary.keep must be a list of terms")
    if len(keep) > MAX_TERMS:
        raise IngestRefused(
            f"agent.yaml: glossary.keep has {len(keep)} terms, more than the {MAX_TERMS} "
            f"accepted — a project's own words, not a dictionary")
    out: list[str] = []
    for term in keep:
        term = term.strip()
        if any(c in term for c in "\r\n") or any(ord(c) < 32 for c in term):
            raise IngestRefused(
                f"agent.yaml: glossary term {term[:20]!r} is not one line. A term reaches the "
                f"reader's model inside its instructions, and one line is all a term needs.")
        if not term or len(term) > MAX_TERM:
            raise IngestRefused(
                f"agent.yaml: glossary term {term[:20]!r} must be 1 to {MAX_TERM} characters")
        if not any(c.isalpha() for c in term):
            raise IngestRefused(
                f"agent.yaml: glossary term {term!r} has no letter in it, and a translation "
                f"never changes a number")
        if term not in out:
            out.append(term)
    return {"keep": out}


def count_nodes(obj, cap: int = MAX_NODES) -> int:
    """The size of a parsed document, as the number of visits a walk makes.

    Iterative rather than recursive, so a deeply nested document is refused by
    the cap rather than by the interpreter's stack. Stops as soon as the cap is
    passed: the point is to refuse, not to measure.
    """
    n = 0
    stack = [obj]
    while stack:
        node = stack.pop()
        n += 1
        if n > cap:
            return n
        if isinstance(node, dict):
            stack.extend(node.keys())
            stack.extend(node.values())
        elif isinstance(node, (list, tuple, set)):
            stack.extend(node)
    return n


def _yaml(raw: bytes, what: str) -> dict:
    try:
        loaded = yaml.safe_load(raw.decode("utf-8"))
    except UnicodeDecodeError as e:
        raise IngestRefused(f"{what} is not UTF-8: {e}") from e
    except yaml.YAMLError as e:
        raise IngestRefused(f"{what} is not valid YAML: {e}") from e
    except RecursionError:
        raise IngestRefused(f"{what} is nested too deeply to be a document") from None
    if count_nodes(loaded) > MAX_NODES:
        raise IngestRefused(
            f"{what} expands to more than {MAX_NODES} nodes. A manifest is a small "
            f"document; one that unfolds into that many values is an expansion attack "
            f"or a mistake, and either way it is not read.")
    if not isinstance(loaded, dict):
        raise IngestRefused(f"{what} is not a mapping")
    return loaded


def _classes(raw) -> tuple[list[str], dict[str, str]]:
    """`problem_classes`, split into the identifiers and the sentences for them.

    A refusal here is a sentence a maintainer can act on, which is the whole
    reason the shape is checked at ingest rather than discovered by a window.
    """
    if not isinstance(raw, list):
        raise IngestRefused("agent.yaml: problem_classes must be a list")
    ids: list[str] = []
    labels: dict[str, str] = {}
    for item in raw:
        if isinstance(item, str):
            ids.append(item)
            continue
        if not isinstance(item, dict):
            raise IngestRefused(
                "agent.yaml: an entry in problem_classes is neither the class itself "
                f"nor a mapping with `id`, but {type(item).__name__}")
        cid = item.get("id")
        if not isinstance(cid, str) or not cid.strip():
            raise IngestRefused("agent.yaml: an entry in problem_classes has no string `id`")
        cid = cid.strip()
        extra = sorted(set(item) - {"id", "describes"})
        if extra:
            raise IngestRefused(
                f"agent.yaml: the problem_classes entry for {cid!r} carries "
                f"{', '.join(extra)}, which means nothing here")
        ids.append(cid)
        describes = item.get("describes")
        if describes is None:
            continue
        if not isinstance(describes, str) or not describes.strip():
            raise IngestRefused(
                f"agent.yaml: `describes` for {cid!r} must be a sentence, not "
                f"{type(describes).__name__}")
        describes = " ".join(describes.split())
        if len(describes) > MAX_DESCRIBES:
            raise IngestRefused(
                f"agent.yaml: `describes` for {cid!r} is {len(describes)} characters, over "
                f"the {MAX_DESCRIBES} permitted. It is the line somebody recognises their "
                f"own problem by, not the answer — the answer belongs in a solution file, "
                f"where the walk reaches it under consent.")
        labels[cid] = describes
    if len(set(ids)) != len(ids):
        raise IngestRefused("agent.yaml: problem_classes names the same class twice")
    return ids, labels


def _solutions(raw) -> tuple[list, dict[str, str]]:
    """The solution paths, and the SHA-256 a manifest states for any of them.

    An entry is a path, or `{path, sha256}`. A digest is a claim, and the claim
    is checked against the bytes when the file is fetched (`SV133`); here only
    its spelling is, because a digest that could never match anything is a
    mistake worth a sentence before anybody fetches a file for it.
    """
    if not isinstance(raw, list):
        return raw, {}
    paths, digests = [], {}
    for entry in raw:
        if isinstance(entry, dict):
            unknown = set(entry) - {"path", "sha256"}
            if unknown or not isinstance(entry.get("path"), str):
                raise IngestRefused(
                    "agent.yaml: a solutions entry is a path, or `path` with an "
                    f"optional `sha256`; this one has {sorted(entry)}")
            digest = entry.get("sha256")
            if digest is not None:
                if (not isinstance(digest, str) or len(digest) != 64
                        or any(c not in "0123456789abcdef" for c in digest)):
                    raise IngestRefused(
                        f"agent.yaml: the sha256 of {entry['path']} must be 64 "
                        f"lower-case hexadecimal characters, not {digest!r}")
                digests[entry["path"]] = digest
            paths.append(entry["path"])
        else:
            paths.append(entry)
    return paths, digests


def parse_manifest(raw: bytes) -> dict:
    """`agent.yaml`, as a dict, with the shape checked but not yet the content.

    Note what is *not* here: an identity field. The display name is the verified
    anchor, so there is nowhere for a manifest to type somebody else's name.
    """
    m = _yaml(raw, "agent.yaml")

    for required in ("endpoint", "problem_classes", "langs", "solutions"):
        if required not in m:
            raise IngestRefused(f"agent.yaml has no {required!r}")

    # **A problem class may carry the sentence a person picks it by.**
    # `engram.llm.model-not-pulled` is an identifier: it is what a rule matches
    # on and what a solution answers. It was also the only thing the window had
    # to put in front of a user, and three identifiers in a dropdown is not a
    # question anybody can answer about their own computer.
    #
    # So an entry is either the identifier alone, as every manifest published so
    # far is, or the identifier with `describes` — one sentence in the user's
    # words. The maintainer writes it because the maintainer is the only person
    # who knows what the symptom looks like from outside. Everything downstream
    # goes on receiving plain identifiers; the sentences travel beside them.
    # Absent, not empty, when nobody wrote a sentence: a manifest that says
    # nothing about labels must come back out of the builder exactly as it went
    # in, and `class_labels: {}` is a difference a maintainer never wrote.
    m["problem_classes"], _labels = _classes(m["problem_classes"])
    if _labels:
        m["class_labels"] = _labels

    # **A solution entry may carry the digest of its file**, so a check of an
    # unchanged project can stop at one conditional request for the manifest
    # (`INGEST-REDESIGN.md`). Optional: a plain path keeps meaning exactly what
    # it meant. The digests travel beside the paths rather than inside them, so
    # everything downstream still reads a list of strings, and are absent -- not
    # empty -- when a manifest names none.
    # Only ever from the entries: a top-level `solution_sha256` a publisher
    # wrote is not a digest anybody spelled next to a file.
    m.pop("solution_sha256", None)
    m["solutions"], _digests = _solutions(m["solutions"])
    if _digests:
        m["solution_sha256"] = _digests

    for name in ("problem_classes", "langs", "solutions"):
        if not isinstance(m[name], list) or not all(isinstance(x, str) for x in m[name]):
            raise IngestRefused(f"agent.yaml: {name} must be a list of strings")
    if not isinstance(m["endpoint"], str):
        raise IngestRefused("agent.yaml: endpoint must be a string")

    status = m.get("status", "active")
    if status not in ("active", "deprecated"):
        raise IngestRefused(
            f"agent.yaml: status {status!r} is not one of active, deprecated. "
            "Deprecation is the developer's own word and has to be spelled the "
            "one way, so it can be told apart from an age we merely observed."
        )
    m["status"] = status

    collect = m.get("collect") or []
    if not isinstance(collect, list):
        raise IngestRefused("agent.yaml: collect must be a list of probes")
    m["collect"] = collect

    # **Typed and bounded, here, where a refusal is a sentence a maintainer can
    # act on.** Everything below used to reach the database untouched, and the
    # error a maintainer got was whatever Postgres said about a parameter — if
    # they got one at all, because an exception raised down there escaped
    # `ingest_one` and took the whole crawl batch with it.
    #
    # YAML types are the trap: `commit: 1234567` is an int, `commit: yes` is a
    # bool, and both bind to a text column as something other than what was
    # written. Refusing is better than coercing, because a commit that is not a
    # string is a mistake worth telling someone about.
    for name in ("commit", "successor"):
        value = m.get(name)
        if value is not None and not isinstance(value, str):
            raise IngestRefused(
                f"agent.yaml: {name} must be a string, not "
                f"{type(value).__name__}. Quote it — YAML reads `{value}` as a "
                f"{type(value).__name__} rather than as text.")

    if len(collect) > MAX_READS:
        raise IngestRefused(
            f"agent.yaml declares {len(collect)} probes; the client performs at most "
            f"{MAX_READS}. A skill demanding more facts than that is an inventory "
            f"sweep rather than a diagnosis, and the client would refuse the rest "
            f"after the user had already consented to them.")
    if len(m["problem_classes"]) > MAX_CLASSES:
        raise IngestRefused(
            f"agent.yaml declares {len(m['problem_classes'])} problem classes, more "
            f"than the {MAX_CLASSES} one project can meaningfully answer for.")
    if len(m["langs"]) > MAX_LANGS:
        raise IngestRefused(
            f"agent.yaml declares {len(m['langs'])} languages, more than the "
            f"{MAX_LANGS} this accepts.")
    for name, values, cap in (("problem_classes", m["problem_classes"], 200),
                              ("langs", m["langs"], 32)):
        for v in values:
            if len(v) > cap:
                raise IngestRefused(
                    f"agent.yaml: an entry in {name} is {len(v)} characters, over "
                    f"the {cap} permitted.")
    if "glossary" in m:
        m["glossary"] = _glossary(m["glossary"])

    # Only what is read is kept. Anything else a publisher wrote stays in their
    # repository, where it was; it is not stored here and not served to anyone.
    return {k: v for k, v in m.items() if k in MANIFEST_KEYS}


def parse_solution(raw: bytes, path: str) -> dict:
    """One solution file: YAML front matter, then the human part.

    The split is what keeps a solution reviewable — the machine part small
    enough to check at a glance, the human part readable — so that one solution
    is one pull request.
    """
    text = raw.decode("utf-8", errors="replace")
    if not text.startswith("---"):
        raise IngestRefused(f"{path}: no front matter (a solution starts with ---)")
    parts = text.split("---", 2)
    if len(parts) < 3:
        raise IngestRefused(f"{path}: front matter is not closed")

    front = _yaml(parts[1].encode(), path)
    body = parts[2].strip()

    if "id" not in front:
        raise IngestRefused(f"{path}: no id")
    answers = front.get("answers")
    if not isinstance(answers, dict) or "problem_class" not in answers:
        raise IngestRefused(f"{path}: answers must name a problem_class")

    proposes = front.get("proposes") or []
    if not isinstance(proposes, list):
        raise IngestRefused(f"{path}: proposes must be a list")

    # The body is the English text unless the file says otherwise. A solution
    # with an empty body is a solution nobody can read.
    text_by_lang = front.get("text_by_lang")
    if text_by_lang is None:
        if not body:
            raise IngestRefused(f"{path}: no text — a solution nobody can read is not one")
        text_by_lang = {front.get("lang", "en"): body}
    if not isinstance(text_by_lang, dict):
        raise IngestRefused(f"{path}: text_by_lang must be a mapping of language to text")

    return {
        "id": str(front["id"]),
        "answers": answers,
        "proposes": proposes,
        "severity": front.get("severity"),
        "text_by_lang": text_by_lang,
        "path": path,
    }
