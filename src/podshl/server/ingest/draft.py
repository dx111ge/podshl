"""A maintainer's draft, checked exactly as ingest checks it, and stored nowhere.

The builder on `/publish/build` writes `agent.yaml` and the solution files for a
maintainer, and the question it has to answer is not "does this look right" but
"will the mirror take it". So it does not carry a copy of the rules. It sends
the text it wrote here, and this runs the same functions ingest runs, in the
same order, on the same bytes — `parse_manifest`, `check_manifest`,
`parse_solution`, `check_solution`, and the tree derivation (`tree_build.derive`)
— with the fetching, the anchor probe and the database left out. `SV108` puts
the same drafts through real ingest and through this, and holds the verdicts
equal, so a builder that accepted a file ingest refuses would fail a case
rather than a maintainer.

What it adds is marked as what it is: **warnings** about things ingest accepts
today and nobody would want — a solution for a class the manifest never
declares, which no client can ever ask for; two solutions or two probes with one
id. And **notes** for what is fine and worth knowing: a declared class nothing
answers yet. Neither is ever a refusal here, because a builder stricter than the
mirror would teach a rule the mirror does not have.

**Nothing is kept.** No row, no file, no log line: a draft says what a project
is about to publish, and who is writing it.
"""
from __future__ import annotations

import json

from ..errors import IngestRefused
from . import manifest, tree_build, validate


def _plain(value):
    """As JSON would carry it. YAML can produce a date or a timestamp where a
    maintainer wrote something unquoted, and a response that cannot be encoded
    is a 500 rather than a sentence."""
    return json.loads(json.dumps(value, default=str))


def _dir(url: str) -> str:
    return url if url.endswith("/") else url.rsplit("/", 1)[0] + "/"


def check(agent_yaml: str, solutions: dict[str, str], anchor: str | None) -> dict:
    """What ingest would say about these files, and what it would not.

    `anchor` is where the files will be served from — the URL the operator will
    verify. Without it the endpoint is checked against its own directory, which
    catches a malformed URL and nothing more, and the answer says so.
    """
    out: dict = {"accepted": False, "refused": {}, "trees": [], "no_tree": {},
                 "warnings": [], "notes": [], "anchor_checked": bool(anchor),
                 # What ingest's own parser made of the files, so the builder can
                 # load a project's existing files without a second YAML parser
                 # in the browser that would read them differently.
                 "parsed": {"manifest": None, "solutions": {}}}
    refused = out["refused"]

    try:
        m = manifest.parse_manifest(agent_yaml.encode("utf-8"))
    except IngestRefused as e:
        refused["agent.yaml"] = str(e)
        return out
    out["parsed"]["manifest"] = _plain(m)
    try:
        validate.check_manifest(m, _dir(anchor or m["endpoint"]))
    except IngestRefused as e:
        refused["agent.yaml"] = str(e)
        return out
    if not anchor:
        out["warnings"].append(
            "The endpoint was not checked against where your files will be served. Give that "
            "address, and the check ingest makes — the endpoint must lie under it — is made here too.")

    collect = m.get("collect") or []
    listed = m["solutions"]
    parsed = []
    for rel in listed:
        if rel not in solutions:
            refused[rel] = (f"{rel} is listed in agent.yaml and was not provided, so the mirror "
                            f"would find nothing there and refuse the whole project.")
            continue
        try:
            body = solutions[rel].encode("utf-8")
            validate.check_digest(rel, body, (m.get("solution_sha256") or {}).get(rel))
            sol = manifest.parse_solution(body, rel)
        except IngestRefused as e:
            refused[rel] = str(e)
            continue
        out["parsed"]["solutions"][rel] = _plain(sol)
        try:
            validate.check_solution(sol, collect)
        except IngestRefused as e:
            refused[rel] = str(e)
            continue
        parsed.append(sol)
    for rel in solutions:
        if rel not in listed:
            out["warnings"].append(f"{rel} is not listed under solutions in agent.yaml, so it is never fetched.")

    if refused:
        return out
    out["accepted"] = True

    built, no_tree = tree_build.derive(parsed, collect)
    out["trees"] = sorted(built)
    out["no_tree"] = no_tree

    declared = set(m["problem_classes"])
    answered = {(s.get("answers") or {}).get("problem_class") for s in parsed}
    for s in parsed:
        cls = (s.get("answers") or {}).get("problem_class")
        if cls not in declared:
            out["warnings"].append(
                f"{s['path']} answers {cls!r}, which problem_classes does not declare. A client "
                f"offers the classes you declare, so nobody can ever reach this answer.")
    # Not a warning. A class nothing answers yet is how a gap reaches a
    # maintainer: people can name it and report it, and the dashboard shows it.
    # The Python example declares one on purpose.
    out["notes"] = [f"{cls!r} is declared and nothing answers it yet. People can still name it and "
                    f"report it, and it shows on your dashboard as not answered."
                    for cls in sorted(declared - answered)]
    for kind, ids in (("solution", [s["id"] for s in parsed]),
                      ("probe", [p.get("id") for p in collect if isinstance(p, dict)])):
        for dup in sorted({i for i in ids if ids.count(i) > 1}):
            out["warnings"].append(f"Two {kind}s share the id {dup!r}; only one of them can be told apart.")
    ids = {p.get("id") for p in collect if isinstance(p, dict)}
    for p in collect:
        if isinstance(p, dict) and p.get("when_missing") and p["when_missing"] not in ids:
            out["warnings"].append(
                f"Probe {p.get('id')!r} is asked when {p['when_missing']!r} is missing, and no probe has that id.")
    return out
