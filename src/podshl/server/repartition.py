"""Where one of a maintainer's answers needs a distinction it does not make.

> A solution whose reports say it worked for some and not others is two
> problems.

The outcome label exists because the report comes after the attempt, and this
is where it pays: the failure of an answer points at the place the tree needs to
fork. What this module returns is a **suggestion for a person** — which fact
separates the configurations the answer helped from the ones it did not, what a
switch on it would do to history, and the `answers.when` that would say so. It
is fuzzy on purpose, and fuzziness is allowed here and nowhere at runtime: a
wrong guess on this page costs a maintainer a minute, a wrong guess in
`/diagnose` reaches a user.

**It compares clusters, never the reports inside one.** A cluster's signature
is every fact a report carried (`app.report`), so the reports in one cluster
agree on every value by construction. The first version of this looked for a
separating fact *inside* a cluster, joined on a `tried_link_id` that no report
ever carried, and was proved by a suite that wrote rows `/report` cannot
produce. It could never have found anything in production. What does differ is
the clusters one answer covers — and `explain` already knows which answer
covers which cluster, by walking the maintainer's own trees.

**It reads nothing the page does not already show.** Its input is the rows
`explain` built, each already at or above the floor, each outcome already
floored on its own (`GR1b`). So there is no query here to forget a floor in: a
value it names is a value on one of those cards, and a count it gives is a sum
of counts on them.

**Nothing is saved.** A tree is derived from the solutions at ingest, so the
only place a fork can live is the maintainer's own `answers.when`. A partition
stored here would be a second document that drifts from the first.
"""
from __future__ import annotations

from .cluster_tree import _coerce
from .ingest.tree_build import _comparator

#: How many candidate facts one answer shows. The first one that separates
#: completely is the suggestion; a list of forty is a spreadsheet.
MAX_CANDIDATES = 5

#: What stands in for a fact a configuration did not carry. Absence separates
#: as well as a value does — a fact read only where it failed is a finding.
ABSENT = None


def _verdict(row: dict) -> str | None:
    """What this configuration says about its answer, from the floored counts."""
    worked, did_not = row.get("worked") or 0, row.get("did_not") or 0
    if worked and did_not:
        return "mixed"
    if worked:
        return "worked"
    if did_not:
        return "did_not"
    return None


def _facts(row: dict) -> dict[str, tuple[str, object]]:
    out = {k: ("measured", v) for k, v in (row.get("measured") or {}).items()}
    out.update({k: ("typed", v) for k, v in (row.get("typed") or {}).items()})
    return out


def _values(rows: list[dict], fact: str) -> list:
    seen = []
    for r in rows:
        v = _facts(r).get(fact, (None, ABSENT))[1]
        if v not in seen:
            seen.append(v)
    return sorted(seen, key=lambda v: (v is not ABSENT, str(v)))


def _splits(rows: list[dict], fact: str) -> list[dict]:
    """What a switch on this fact does to history: past reports, per value.

    A reading is in every stored signature, so saving the switch re-partitions
    everything already reported. The counts are the cards' own counts summed.
    """
    by_value: dict[str, dict] = {}
    for r in rows:
        v = _facts(r).get(fact, (None, ABSENT))[1]
        slot = by_value.setdefault(repr(v), {"value": v, "reports": 0, "people": 0,
                                             "worked": 0, "did_not": 0})
        slot["reports"] += r.get("reports") or 0
        slot["people"] += r.get("reporters") or 0
        slot["worked"] += r.get("worked") or 0
        slot["did_not"] += r.get("did_not") or 0
    return sorted(by_value.values(), key=lambda s: -s["reports"])


def _range(fact: str, worked: list, did_not: list) -> str | None:
    """`">= 2.6"` rather than a list, where that is what the values say.

    Only for versions and numbers, only where every value it worked with lies
    on one side of every value it did not, and only with at least two values on
    the side the range describes — one value is a point, and a range drawn
    through a point claims releases nobody has reported on.
    """
    comparator = _comparator(fact, worked + did_not)
    if comparator == "string" or len(worked) < 2:
        return None
    try:
        w = sorted(_coerce(v, comparator) for v in worked)
        f = sorted(_coerce(v, comparator) for v in did_not)
    except ValueError:
        return None
    by_key = {_coerce(v, comparator): v for v in worked + did_not}
    if f[-1] < w[0]:
        return f">= {by_key[w[0]]}"
    if w[-1] < f[0]:
        return f"< {by_key[f[0]]}"
    return None


def _when(current: dict, fact: str, values: list, did_not: list) -> dict | None:
    """The `answers.when` that would narrow this answer to where it worked.

    Only when that is expressible: a configuration that simply did not carry the
    fact cannot be matched on it, so a side made of absences has no snippet.
    """
    present = [v for v in values if v is not ABSENT]
    if not present or len(present) != len(values) or ABSENT in did_not:
        return None
    out = dict(current or {})
    out[fact] = _range(fact, present, did_not) or (present[0] if len(present) == 1 else present)
    return out


def forks(rows: list[dict], when_by_solution: dict[str, dict] | None = None,
          acquired: set[str] | None = None) -> list[dict]:
    """For every answer that helped some configurations and not others, what
    separates them — best evidenced first.

    `rows` are `explain.explain`'s output. `when_by_solution` is each current
    solution's own `answers.when`, so a suggestion can be pasted rather than
    reconstructed. `acquired` is every fact the project's current `collect`
    can obtain (`tree_build.known_facts`): a fact outside it may still be in
    old reports, but a `when` on it is refused at ingest, so no snippet is
    offered for it.
    """
    when_by_solution = when_by_solution or {}
    by_solution: dict[str, list[dict]] = {}
    for row in rows:
        answer = row.get("answer")
        if answer and _verdict(row):
            by_solution.setdefault(answer["solution_id"], []).append(row)

    out = []
    for solution_id, covered in sorted(by_solution.items()):
        worked = [r for r in covered if _verdict(r) == "worked"]
        did_not = [r for r in covered if _verdict(r) == "did_not"]
        mixed = [r for r in covered if _verdict(r) == "mixed"]
        if not (worked and did_not) and not mixed:
            continue

        candidates = []
        if worked and did_not:
            names = set()
            for r in worked + did_not:
                names |= set(_facts(r))
            for fact in sorted(names):
                w, f = _values(worked, fact), _values(did_not, fact)
                if w == f:
                    continue
                kinds = {_facts(r)[fact][0] for r in worked + did_not if fact in _facts(r)}
                separates = not any(v in f for v in w)
                can_match = acquired is None or fact in acquired
                candidates.append({
                    "fact": fact,
                    # A fork on a fact somebody typed is a fork on their answer,
                    # which may be wrong; one on a reading is a fork on the
                    # machine. The page says which, as it does for a match.
                    "provenance": "typed" if "typed" in kinds else "measured",
                    "when_it_worked": w,
                    "when_it_did_not": f,
                    "separates_completely": separates,
                    "splits_history": _splits(worked + did_not, fact),
                    "acquired": can_match,
                    "when": (_when(when_by_solution.get(solution_id), fact, w, f)
                             if separates and can_match else None),
                })
            candidates.sort(key=lambda c: (not c["separates_completely"], not c["acquired"],
                                           c["provenance"] != "measured", c["fact"]))

        first = covered[0]["answer"]
        out.append({
            "solution_id": solution_id,
            "problem_class": first["problem_class"],
            "worked": {"configurations": len(worked),
                       "people": sum(r.get("worked") or 0 for r in worked)},
            "did_not": {"configurations": len(did_not),
                        "people": sum(r.get("did_not") or 0 for r in did_not)},
            "candidates": candidates[:MAX_CANDIDATES],
            "history": (
                "Every value named here is already in the reports you have, so a switch "
                "on it re-partitions history the moment you publish it."
                if candidates else None),
            # The same configuration, both outcomes. Nothing collected tells those
            # people apart, so no reading can — only a new question, and a question
            # answers nothing about anybody who already reported.
            "same_configuration_both_ways": [
                {"measured": r.get("measured") or {}, "typed": r.get("typed") or {},
                 "worked": r.get("worked"), "did_not": r.get("did_not")}
                for r in mixed],
            "forward_only": (
                "Where it worked for some and not others in one configuration, nothing you "
                "collect separates them. A new question would apply forward only: nobody who "
                "already reported has answered it."
                if mixed else None),
            "attributed_by": "walking your current solutions — a report filed against an "
                             "earlier version of them is counted against today's",
        })
    # The answer with the most people it failed first: that is the afternoon.
    out.sort(key=lambda s: (-s["did_not"]["people"], s["solution_id"]))
    return out
