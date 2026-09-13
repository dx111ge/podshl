"""Turning a cluster into something the person it is for can read.

The dashboard showed a maintainer `sig.c0bbda7c3ddd`, a row of `key=value`
facts, and two integers. Every one of those is true and none of them is a
statement about a problem. The operator looked at their own project's dashboard
and could not read it, which is the only review that matters here: this page
exists for exactly one audience and it was not serving them.

What a maintainer is actually asking, in order:

1. **What broke?** Not a hash. The facts, in their own words, and — since
   `/diagnose` now derives a decision tree from their solutions — *which of
   their own answers applies to it*.
2. **How many people, and does that number mean what I think?** It does not:
   it is distinct people who *reported*, never occurrences, and it under-counts
   both ways.
3. **Did the answer work?** The outcome label has been recorded on every report
   since the beginning and was shown nowhere at all. It is the most valuable
   column on the page and it was missing.
4. **Does this tell me anything about my rule?** Only if the answer turned on
   something measured. An outcome that turned on a fact somebody typed may be
   somebody who typed the wrong thing.
5. **What is not covered?** A cluster none of their solutions answers is a gap
   in their own project, on their own dashboard. That is theirs to see — and it
   is the one figure this design will show nobody else, ever.

Nothing here reaches past the source it was asked about. The walk uses that
source's own trees, and a cluster is only ever explained to the anchor it
belongs to.
"""
from __future__ import annotations

from . import cluster_tree
from .config import K_REPORTERS

#: A cluster is explained against at most this many of the source's trees. A
#: project with sixty problem classes and a hundred clusters would otherwise
#: turn one page load into six thousand walks, and the answer for the sixty-first
#: class is not worth that.
MAX_TREES = 40


def _trees(conn, source_id: int) -> list[tuple[str, cluster_tree.Node]]:
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, problem_class FROM tree WHERE source_id = %s AND valid_to IS NULL "
            "ORDER BY problem_class LIMIT %s", (source_id, MAX_TREES))
        rows = cur.fetchall()
    out = []
    for row in rows:
        try:
            out.append((row["problem_class"], cluster_tree.load(conn, row["id"])))
        except Exception:  # noqa: BLE001 - a tree that will not load explains nothing
            continue
    return out


def _typed_facts(conn, ids: list[int]) -> dict[int, set[str]]:
    """Which facts in each cluster arrived typed rather than read.

    The signature merges `observed` and `stated`, deliberately — two people with
    identical readings who ran different commands are in different situations,
    so the shape of the problem includes both. But the *grading* needs them
    apart again, and the observations kept them apart, so this is a lookup
    rather than a guess.

    One query for the page, like `_outcomes` and `_words`. Each was one query
    per cluster, which is nothing at five clusters and eight hundred round trips
    at 263 — cheap on loopback, and not what a database on another machine is.
    """
    out: dict[int, set[str]] = {i: set() for i in ids}
    with conn.cursor() as cur:
        cur.execute(
            "SELECT DISTINCT o.cluster_id, k FROM observation o, "
            "     LATERAL jsonb_object_keys(o.stated) AS k "
            "WHERE o.cluster_id = ANY(%s)", (ids,))
        for r in cur.fetchall():
            out[r["cluster_id"]].add(r["k"])
    return out


def _outcomes(conn, ids: list[int]) -> dict[int, dict]:
    """What happened afterwards, which is why the report comes after the attempt.

    Counted in distinct reporters rather than reports, like everything else on
    this page: one person who tried a fix three times is one person for whom it
    did or did not work.

    **Below the floor, a split is not shown.** The cluster is at or above k, or
    it would not be on the page at all — but an outcome held by one person
    inside it is a group of one, and a group of one next to that person's own
    words and the facts they typed is a description of them. The floor is the
    same k, applied to each group; what falls under it is counted only as a
    number of groups withheld, never as which.

    **The floor is the busiest month, as it is for the cluster** (`GR1b`). A
    `seen_key` is a new key every month, so distinct keys over all time count
    one person who kept saying "it did not work" once per month. The number
    shown is still the all-time one, which over-counts a returning reporter
    exactly as `reporters_means` says; only whether the group is shown at all
    rests on the stricter count.
    """
    with conn.cursor() as cur:
        cur.execute(
            "SELECT cluster_id, outcome, sum(n) AS reporters, max(n) AS peak FROM ("
            "  SELECT cluster_id, coalesce(outcome, 'not_said') AS outcome, epoch, "
            "         count(DISTINCT seen_key) AS n "
            "  FROM observation WHERE cluster_id = ANY(%s) GROUP BY 1, 2, 3"
            ") per_month GROUP BY cluster_id, outcome", (ids,))
        rows = cur.fetchall()
    out: dict[int, dict] = {i: {} for i in ids}
    for r in rows:
        if r["peak"] >= K_REPORTERS:
            out[r["cluster_id"]][r["outcome"]] = int(r["reporters"])
        else:
            shown = out[r["cluster_id"]]
            shown["withheld_below_floor"] = shown.get("withheld_below_floor", 0) + 1
    return out


#: How many excerpts one cluster shows. The point is to see what the failure
#: looks like, and three people's error messages show that; thirty are a pile.
MAX_WORDS = 3


def _words(conn, ids: list[int]) -> dict[int, dict]:
    """What people sent in their own words, where they agreed to send it.

    Free text was recorded since the beginning — withheld by default, attached
    only under its own consent naming a destination, enforced by a CHECK — and
    shown nowhere. So the one thing a maintainer can get no other way, the
    error message or the lines of a log around the failure, reached the
    database and stopped there.

    Only for a cluster the dashboard already shows, which means one at or above
    the floor of distinct reporters; below it nothing about the cluster is
    shown, words included. The text was anonymised on the sender's machine and
    shown to them before they agreed, and it is passed through as it arrived —
    nothing here rewrites what a person consented to.
    """
    out: dict[int, dict] = {i: {"count": 0, "shown": []} for i in ids}
    with conn.cursor() as cur:
        cur.execute(
            "SELECT cluster_id, count(*) AS n FROM observation "
            "WHERE cluster_id = ANY(%s) AND description IS NOT NULL GROUP BY 1", (ids,))
        for r in cur.fetchall():
            out[r["cluster_id"]]["count"] = r["n"]
        # Shortest first: of three excerpts, the tightest one is usually the
        # one somebody cut down to the line that matters.
        cur.execute(
            "SELECT cluster_id, text, outcome FROM ("
            "  SELECT cluster_id, description AS text, outcome, row_number() OVER ("
            "    PARTITION BY cluster_id ORDER BY length(description), description) AS rn "
            "  FROM observation WHERE cluster_id = ANY(%s) AND description IS NOT NULL"
            ") ranked WHERE rn <= %s ORDER BY cluster_id, rn", (ids, MAX_WORDS))
        for r in cur.fetchall():
            out[r["cluster_id"]]["shown"].append({"text": r["text"], "outcome": r["outcome"]})
    return out


def _answers(trees, facts: dict, typed: set[str]) -> list[dict]:
    """Every one of this project's classes whose own tree answers these facts.

    More than one is not an error and is worth seeing: it means two classes the
    maintainer wrote overlap on a real configuration, and somebody is getting
    whichever the client asked for first.
    """
    found = []
    for problem_class, root in trees:
        out = cluster_tree.walk(root, facts)
        # **An answer that decided on nothing is not a match here.**
        #
        # `/diagnose` is told which problem class it is being asked about, so a
        # fallback answer — the one a declined question lands on — is legitimate
        # there: the caller has already asserted the class. This is the opposite
        # situation. Here we are *searching* the classes to see which of them
        # this configuration belongs to, and a class that answers without any
        # fact having decided anything answers every configuration equally.
        #
        # Left in, every cluster listed every such class as also matching, which
        # is noise that makes the page harder to read rather than easier — the
        # exact failure this rewrite exists to fix.
        if isinstance(out, cluster_tree.Answer) and out.decided_on:
            found.append({
                "problem_class": problem_class,
                "solution_id": out.solution_id,
                "decided_on": list(out.decided_on),
            })
    # **Best evidenced first, then most specific.**
    #
    # Alphabetical was the tie-break and it is not a judgement about anything. An
    # answer that turned on facts the *machine read* is stronger evidence than
    # one that turned on what somebody typed about themselves, and where two of
    # a project's classes both match a configuration, the measured one is the
    # one to put at the top of the card.
    def rank(a):
        measured = sum(1 for f in a["decided_on"] if f not in typed)
        return (-measured, -len(a["decided_on"]), a["problem_class"])

    found.sort(key=rank)
    return found


def explain(conn, source_id: int | None, clusters: list[dict]) -> list[dict]:
    """The rows the dashboard renders, each one a sentence rather than a hash."""
    trees = _trees(conn, source_id) if source_id else []
    with conn.cursor() as cur:
        cur.execute("SELECT solution_id, severity, text_by_lang FROM solution "
                    "WHERE source_id = %s AND valid_to IS NULL", (source_id,))
        by_id = {r["solution_id"]: r for r in cur.fetchall()} if source_id else {}

    ids = [c["id"] for c in clusters]
    typed_by, outcomes_by, words_by = _typed_facts(conn, ids), _outcomes(conn, ids), _words(conn, ids)

    out = []
    for c in clusters:
        facts = dict((c.get("signature") or {}).get("observed") or {})
        typed = typed_by[c["id"]] & set(facts)
        answers = _answers(trees, facts, typed)
        outcomes = outcomes_by[c["id"]]

        best = answers[0] if answers else None
        rested = sorted(set(best["decided_on"]) & typed) if best else []
        row = {
            "class": c["problem_class"],
            "reporters": c["peak_epoch_reporters"],
            "reports": c["reports_total"],
            # Split back apart, because the two mean different things to the
            # person reading them.
            "measured": {k: v for k, v in sorted(facts.items()) if k not in typed},
            "typed": {k: v for k, v in sorted(facts.items()) if k in typed},
            "outcomes": outcomes,
            "worked": outcomes.get("resolved", 0),
            "did_not": outcomes.get("unresolved", 0),
            "answer": None,
            "also_answered_by": [a["problem_class"] for a in answers[1:]],
            "in_their_words": words_by[c["id"]],
        }
        if best:
            sol = by_id.get(best["solution_id"]) or {}
            row["answer"] = {
                "problem_class": best["problem_class"],
                "solution_id": best["solution_id"],
                "severity": sol.get("severity"),
                "decided_on": best["decided_on"],
                "rested_on_typed": rested,
                # The grade, in the same words `/diagnose` uses, because a
                # maintainer should not have to learn two vocabularies for one
                # distinction.
                "confidence": "rests_on_supplied" if rested else "measured",
            }
        else:
            row["why_no_answer"] = (
                "nothing you have published answers this configuration"
                if trees else
                "no decision tree could be derived from your solutions for any class")
        out.append(row)
    return out


#: The order a maintainer should read their answers in: the one that helped
#: nobody first, the one that needs a distinction next, and the one working
#: last — a page of good news is not where an afternoon goes.
STATUS_ORDER = ("did_not_help", "needs_a_distinction", "nobody_said", "working")


def triage(rows: list[dict]) -> dict:
    """The page's rows, grouped by the file a maintainer would edit.

    `SV106`. At five clusters a list of cards reads well; at 263 it was
    forty-six screens with the work scattered through it. The grouping is by
    solution because that is the unit a maintainer changes, and it is where a
    fork suggestion belongs. Configurations are referred to by their index in
    `rows`, so nothing is sent twice.

    The grade uses the floored counts the cards already carry, so a group can
    say nothing a card does not:

    * `did_not_help` — people said it did not work, and nobody that it did,
    * `needs_a_distinction` — both, which is exactly when `repartition.forks`
      has something to suggest,
    * `nobody_said`, and `working`.
    """
    groups: dict[str, dict] = {}
    not_answered = []
    for i, row in enumerate(rows):
        answer = row.get("answer")
        if not answer:
            not_answered.append(i)
            continue
        g = groups.setdefault(answer["solution_id"], {
            "solution_id": answer["solution_id"],
            "problem_class": answer["problem_class"],
            "severity": answer.get("severity"),
            "configurations": [], "people": 0, "worked": 0, "did_not": 0})
        g["configurations"].append(i)
        g["people"] += row.get("reporters") or 0
        g["worked"] += row.get("worked") or 0
        g["did_not"] += row.get("did_not") or 0

    for g in groups.values():
        g["status"] = ("needs_a_distinction" if g["worked"] and g["did_not"] else
                       "did_not_help" if g["did_not"] else
                       "working" if g["worked"] else "nobody_said")
    answers = sorted(groups.values(), key=lambda g: (
        STATUS_ORDER.index(g["status"]), -g["people"], g["solution_id"]))
    not_answered.sort(key=lambda i: -(rows[i].get("reporters") or 0))

    summary = {"configurations": len(rows), "answers": len(answers),
               "not_answered": len(not_answered)}
    summary.update({s: sum(1 for g in answers if g["status"] == s) for s in STATUS_ORDER})
    return {"summary": summary, "answers": answers, "not_answered": not_answered}
