"""Where a decision tree comes from.

`cluster_tree` could walk a tree, validate one and load one, and nothing
anywhere built one: `INSERT INTO tree` existed only in the test suite. So
`POST /diagnose` answered `no_statement` on every real deployment whatever it
was sent — the endpoint the enterprise branch is named after, returning nothing
by construction.

**The tree is derived from what a maintainer already writes.** A solution's
`answers.when` is a conjunction of conditions over facts, and a set of solutions
for one problem class is therefore already a decision structure; `collect` says
how each fact is acquired. Asking publishers to author a tree as well would be
a second document describing the same thing, and the two would drift.

Three properties fall out of deriving it rather than authoring it, and each one
matters more than the convenience:

* **Every project that has already published gets a diagnosis endpoint**, with
  no change to their files and nothing new to learn.
* **The tree cannot contradict the solutions**, because it is a view of them.
* **Ambiguity becomes visible.** Two solutions for one class that cannot be told
  apart is a defect a publisher wants to know about, and building the tree is
  what notices.

The ordering rule is the one from `cluster_tree`'s own header: *a switch on an
already-collected fact is worth more than a new question*, because a reading is
in every stored signature and re-partitions history, while a question only ever
works forward. So readings are chosen first, and a question is reached only when
no reading separates what is left.
"""
from __future__ import annotations

import json
import re
from dataclasses import dataclass

from ... import spec_gate
from ..cluster_tree import COMPARATORS, Node, validate_tree
from ..errors import IngestRefused

#: `>=` before `>`, or a prefix test takes the shorter one and leaves an `=`
#: dangling in the value.
_PREFIXES = ((">=", "ge"), ("<=", "le"), ("==", "eq"), (">", "gt"), ("<", "lt"), ("=", "eq"))

_VERSIONISH = re.compile(r"\d+(\.\d+)+")
_NUMERIC = re.compile(r"-?\d+(\.\d+)?")


@dataclass
class _Rule:
    """One solution, reduced to what decides whether it applies."""

    solution_id: str
    when: dict
    path: str

    @property
    def specificity(self) -> int:
        return len(self.when)


def _condition(fact: str, raw) -> tuple[str, object]:
    """`when` value to (operator, value).

    An operator may be written into the value — `">= 3.13"` — which is how the
    specification's own example writes it, so it is part of the format whether
    or not it was ever named as one.
    """
    if isinstance(raw, (list, tuple)):
        return "in", list(raw)
    if isinstance(raw, str):
        s = raw.strip()
        for prefix, op in _PREFIXES:
            if s.startswith(prefix):
                return op, s[len(prefix):].strip()
        return "eq", s
    if isinstance(raw, bool):
        return "eq", "true" if raw else "false"
    return "eq", raw


def _comparator(fact: str, values: list) -> str:
    """One comparator for a switch, decided from the fact and every value on it.

    It lives on the *parent*, so all of a node's branches share it — which means
    it cannot be decided per branch, and a set of values that disagree about
    what they are is a question for the publisher rather than something to
    average.
    """
    flat: list[str] = []
    for v in values:
        flat.extend(str(x) for x in (v if isinstance(v, (list, tuple)) else [v]))
    if "version" in fact or (flat and all(_VERSIONISH.fullmatch(s) for s in flat)):
        return "version"
    if flat and all(_NUMERIC.fullmatch(s) for s in flat):
        return "number"
    return "string"


def _probe_index(collect: list) -> dict[str, dict]:
    return {p["id"]: p for p in collect if isinstance(p, dict) and p.get("id")}


def _derived_ids() -> set[str]:
    derived = spec_gate.vocabulary("reads").get("derived") or []
    out = set()
    for entry in derived:
        if isinstance(entry, dict) and entry.get("id"):
            out.add(entry["id"])
        elif isinstance(entry, str):
            out.add(entry)
    return out


def known_facts(collect: list) -> set[str]:
    """Every fact a solution may match on: a probe in `collect`, or one the
    client derives. The ingest gate and the dashboard's fork suggestions ask the
    same question, so they ask it here — a suggestion that disagreed would offer
    a snippet the gate then refuses."""
    return set(_probe_index(collect)) | _derived_ids()


def check_when_is_answerable(solution: dict, collect: list) -> None:
    """Every fact a solution matches on has to be one the client can obtain.

    A solution keyed on a fact nobody collects can never match anybody — it is
    dead on arrival, and silently so. Refused here rather than discovered as an
    absence of results months later, and the message names the probe ids that
    do exist.
    """
    when = (solution.get("answers") or {}).get("when") or {}
    if not when:
        return
    known = known_facts(collect)
    for fact in when:
        if fact not in known:
            raise IngestRefused(
                f"{solution.get('path', solution.get('id'))}: answers.when matches on "
                f"{fact!r}, which no probe in `collect` acquires and which the client "
                f"does not derive. A solution keyed on a fact nobody collects can never "
                f"match. Declared probes: {sorted(known) or 'none'}")


def _switch_fact(rules: list[_Rule], decided: set[str], probes: dict[str, dict]) -> str | None:
    """The next fact to branch on.

    Readings before questions, because a reading is already in every stored
    signature and re-partitions history while a question only works forward.
    Then the fact that separates the most solutions, then alphabetically — the
    last is not a preference, it is so that the same manifest always produces
    the same tree.
    """
    counts: dict[str, int] = {}
    values: dict[str, list] = {}
    for rule in rules:
        for fact, raw in rule.when.items():
            if fact not in decided:
                counts[fact] = counts.get(fact, 0) + 1
                values.setdefault(fact, []).append(_condition(fact, raw)[1])
    if not counts:
        return None

    def rank(fact: str):
        probe = probes.get(fact) or {}
        # Same test as the node's own `switch_kind`, and it has to be the same
        # or the ordering promise is about a different thing than the label. A
        # probe that carries both a reading and a question is only *decided* by
        # the reading when the values branched on are ones a reading can
        # produce; a value taken from the probe's own `choices` is not.
        choices = {str(c) for c in (probe.get("choices") or [])}
        decided_by_reading = bool(probe.get("read")) and not (
            choices and all(str(v) in choices for v in values[fact]))
        return (0 if decided_by_reading else 1, -counts[fact], fact)

    return sorted(counts, key=rank)[0]


def _build(rules: list[_Rule], decided: set[str], probes: dict[str, dict],
           counter: list[int], depth: int, problem_class: str, above: bool = False) -> Node:
    counter[0] += 1
    node = Node(id=counter[0], parent_id=None, depth=depth)

    # Solutions all of whose conditions are already decided apply *here*. The
    # most specific of them becomes this node's answer — which is also what its
    # children's "don't know" branch lands on, computed on the way down exactly
    # as `cluster_tree` requires.
    settled = [r for r in rules if all(f in decided for f in r.when)]
    if settled:
        best = max(r.specificity for r in settled)
        winners = [r for r in settled if r.specificity == best]
        if len(winners) > 1:
            raise IngestRefused(
                f"{problem_class}: {', '.join(sorted(w.solution_id for w in winners))} "
                f"cannot be told apart — they match on the same facts with the same "
                f"values, so a user would be waiting on a coin flip. Give them a "
                f"condition that differs, or merge them.")
        node.solution_id = winners[0].solution_id

    remaining = [r for r in rules if any(f not in decided for f in r.when)]
    fact = _switch_fact(remaining, decided, probes)
    if fact is None:
        return node

    probe = probes.get(fact) or {}
    node.switch_fact = fact
    node.probe = probes.get(fact)
    branches: dict[str, tuple[str, object]] = {}
    for rule in remaining:
        if fact in rule.when:
            op, value = _condition(fact, rule.when[fact])
            branches.setdefault(json.dumps([op, value], sort_keys=True), (op, value))

    # A probe may carry **both** a reading and a question — the reading is
    # tried, and the question is asked only if it comes back empty. Whether such
    # a switch is decided by reading or by asking is therefore not a property of
    # the probe; it is a property of the *values branched on*. A value drawn
    # from the probe's own `choices` is one only the question can produce, so a
    # switch whose every branch is a choice is a question however it is read.
    #
    # This is not bookkeeping. It decides whether "I would rather not say" is
    # allowed to fall back, and getting it from the probe alone would call a
    # switch a reading that no reading can ever satisfy.
    choices = {str(c) for c in (probe.get("choices") or [])}
    values = [v for _, v in branches.values()]
    all_from_choices = bool(choices) and bool(values) and all(
        str(v) in choices for v in values)
    node.switch_kind = ("reading" if probe.get("read") and not all_from_choices
                        else "question")
    node.comparator = _comparator(fact, values)
    if node.comparator not in COMPARATORS:  # pragma: no cover - defensive
        raise IngestRefused(f"{problem_class}: no comparator for {fact!r}")

    ordered = sorted(branches.values(), key=lambda ov: (ov[0] != "eq", json.dumps(ov[1], sort_keys=True)))
    # **A solution that says nothing about this switch applies under every value
    # of it, including the ones nobody named.** It used to apply only under the
    # values somebody else's rule happened to create, because those were the
    # only children there were — so a reading outside them fell off the tree and
    # took that solution with it.
    #
    # engram published `wrong-archive-for-this-system`: the operating system and
    # the download, nothing about the architecture. Another rule named
    # `os.arch: aarch64`, so `os.arch` became the switch and grew one child; on
    # an ordinary x86_64 Linux desktop nothing matched, the walk stopped at the
    # answer above, and somebody holding the Windows archive was told to go and
    # find out which archive they had. The right answer was published, mirrored,
    # signed, and unreachable.
    #
    # So the branches the author named are followed by one for everything else,
    # carrying exactly the rules that did not constrain this fact. Last, because
    # a named value must always win over it.
    unconstrained = [r for r in remaining if fact not in r.when]
    for op, value in ordered:
        # A solution that does not constrain this fact still applies on every
        # branch of it — it simply has nothing to say about this switch.
        subset = [r for r in remaining
                  if fact not in r.when or _condition(fact, r.when[fact]) == (op, value)]
        child = _build(subset, decided | {fact}, probes, counter, depth + 1, problem_class,
                       above=above or node.solution_id is not None)
        child.parent_id = node.id
        child.match_op = op
        child.match_value = value
        child.edge_label = f"{fact} {op} {value}"
        node.children.append(child)

    # **A question the problem class already answers** (`SV109`).
    #
    # The shape a maintainer writes first — the person says what goes wrong, the
    # machine reads which OS it is — puts the question below the reading, since
    # readings are switched on first, with no answer above it. A question that
    # dead-ends on "I would rather not say" is refused, so the whole class had no
    # tree and nobody was answered from it.
    #
    # Where every solution below agrees on one value for the question, the class
    # the person named when they asked already says it. So a skip falls back to
    # that branch's answer — marked `fallback_only`, as `0011` marks one, so a
    # value that contradicts the rule still gets nothing. Deliberately narrow:
    # two values under the question and the class no longer says which; an answer
    # already above and the more general one stays the fallback, rather than one
    # that assumes a value nobody gave.
    if (node.switch_kind == "question" and node.solution_id is None and not above
            and len(node.children) == 1 and node.children[0].match_op == "eq"
            and node.children[0].solution_id and not node.children[0].fallback_only):
        node.solution_id = node.children[0].solution_id
        node.fallback_only = True

    # Appended after that check rather than before it, so a class whose single
    # named value the problem class already implies keeps its `SV109` fallback:
    # the catch-all is about values, and that rule is about a question nobody
    # needs to be asked.
    if unconstrained:
        other = _build(unconstrained, decided | {fact}, probes, counter, depth + 1,
                       problem_class, above=above or node.solution_id is not None)
        other.parent_id = node.id
        other.match_op = "any"
        other.match_value = None
        other.edge_label = f"{fact} — any other value"
        node.children.append(other)
    return node


def _only_questions(node: Node) -> bool:
    """No switch anywhere in this tree is a reading.

    The distinction matters because a declined *reading* and a declined
    *question* are not the same event. A reading that could not be taken says
    nothing about the machine; a question the person would rather not answer,
    on a class they themselves named, still leaves the class named.
    """
    stack = [node]
    while stack:
        n = stack.pop()
        if n.switch_fact is not None and n.switch_kind != "question":
            return False
        stack.extend(n.children)
    return True


def build(problem_class: str, solutions: list[dict], collect: list) -> Node:
    """The tree for one problem class, or `IngestRefused` saying why not."""
    rules = [
        _Rule(solution_id=s["id"],
              when=dict((s.get("answers") or {}).get("when") or {}),
              path=s.get("path", s["id"]))
        for s in solutions
    ]
    probes = _probe_index(collect)
    root = _build(rules, set(), probes, [0], 0, problem_class)

    # **A class that only a question can decide still has to answer somebody who
    # will not answer it.**
    #
    # `validate_tree` refuses a question with nothing above it to fall back to,
    # and `SV28a` says why: *refused when the tree is authored*. That is right
    # for a tree a person wrote, where the dead end is theirs to fix before
    # shipping. It is the wrong test for a tree derived from somebody's
    # solutions, and it fires on a shape that is both common and honest — a
    # project whose failures are indistinguishable from outside, separated only
    # by asking. Nothing in the read vocabulary can tell "search returns
    # nothing" from "ingest is slow"; only the person can.
    #
    # So exactly one fallback is supplied, under conditions narrow enough that
    # it is not a guess:
    #
    #   * the class has **one** solution — there is nothing to choose between,
    #   * and **no reading** discriminates anywhere in it, so the only thing
    #     that can be declined is the question itself,
    #   * and the client named this problem class when it asked, which is
    #     already an assertion about what went wrong.
    #
    # Where a reading discriminates, no fallback is invented: a declined reading
    # must not be answered as though it had matched, which is the difference
    # between this and guessing.
    if len(rules) == 1 and root.solution_id is None and _only_questions(root):
        root.solution_id = rules[0].solution_id
        # And marked as what it is. Without the mark the walk returned this
        # answer on *any* dead end — including a reading that contradicted the
        # very rule it belongs to, which is a confident wrong answer.
        root.fallback_only = True
    # The same validation an authored tree would face. Deriving it is not a
    # reason to trust it less.
    validate_tree(root)
    return root


def store(conn, source_id: int, problem_class: str, root: Node, commit: str | None) -> int:
    """Supersede the current tree for this class and write the new one."""
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE tree SET valid_to = now() WHERE source_id = %s AND problem_class = %s "
            "AND valid_to IS NULL", (source_id, problem_class))
        cur.execute(
            "INSERT INTO tree (source_id, problem_class, commit) VALUES (%s, %s, %s) "
            "RETURNING id", (source_id, problem_class, commit))
        tree_id = cur.fetchone()["id"]

        # Breadth-first, so a parent always has its database id before a child
        # needs it.
        queue: list[tuple[Node, int | None]] = [(root, None)]
        while queue:
            node, parent_db_id = queue.pop(0)
            cur.execute(
                "INSERT INTO tree_node (tree_id, parent_id, depth, match_op, match_value, "
                "  edge_label, switch_fact, switch_kind, comparator, probe, solution_id, "
                "  fallback_only) "
                "VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s) RETURNING id",
                (tree_id, parent_db_id, node.depth, node.match_op,
                 json.dumps(node.match_value) if node.match_value is not None else None,
                 node.edge_label, node.switch_fact, node.switch_kind, node.comparator,
                 json.dumps(node.probe) if node.probe else None, node.solution_id,
                 node.fallback_only))
            db_id = cur.fetchone()["id"]
            queue.extend((child, db_id) for child in node.children)
    return tree_id


def derive(solutions: list[dict], collect: list) -> tuple[dict[str, Node], dict[str, str]]:
    """Every problem class's tree, or why it has none. Pure.

    The one derivation both ingest and a draft's check (`draft.check`) run, so
    the builder a maintainer uses cannot promise a tree ingest would not build.
    """
    by_class: dict[str, list[dict]] = {}
    for sol in solutions:
        cls = (sol.get("answers") or {}).get("problem_class")
        if cls:
            by_class.setdefault(cls, []).append(sol)
    built: dict[str, Node] = {}
    refused: dict[str, str] = {}
    for cls, sols in sorted(by_class.items()):
        try:
            built[cls] = build(cls, sols, collect)
        except IngestRefused as e:
            refused[cls] = str(e)
    return built, refused


def rebuild_all(conn, source_id: int, solutions: list[dict], collect: list,
                commit: str | None) -> dict:
    """Every problem class this source answers for. Returns what happened.

    **A class whose tree will not build does not fail the source.** The tree is
    our derivation of their document rather than their document, so refusing to
    mirror a project because we could not derive something from it would be the
    wrong way round. The reason is returned instead, so it can reach the person
    who can act on it.

    The exception is an ambiguity that is unambiguously theirs — two solutions
    for one class that cannot be told apart — which `build` raises and which
    `check_when_is_answerable` has already made unlikely.
    """
    derived, refused = derive(solutions, collect)
    built = []
    for cls, root in derived.items():
        store(conn, source_id, cls, root, commit)
        built.append(cls)

    # A class that no longer builds must not go on being answered from the tree
    # it had before. Closing it returns that class to exact-signature matching,
    # which is honest and less useful — and never a stale answer.
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE tree SET valid_to = now() WHERE source_id = %s AND valid_to IS NULL "
            "AND problem_class <> ALL(%s)", (source_id, built or [""]))
        closed = cur.rowcount
    return {"trees": built, "no_tree": refused, "closed": closed}
