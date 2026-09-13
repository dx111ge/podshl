"""A cluster is a decision tree, not a bag.

Grouping "the same problem" across configurations that differ slightly looked
like the hard part. It is not, because the question is wrong.

Both failure directions are expensive. Merge too eagerly and two distinct bugs
share a cluster, the solution is wrong for half of them, and the counter lies.
Split too eagerly and forty-seven users become forty-seven clusters of one,
nothing crosses the threshold, and nobody ever sees anything. A threshold on a
fuzzy score is wrong in both directions at once and cannot explain itself either
way.

**So do not estimate whether A and B are the same. Acquire the fact that decides
it.** That fact is either a reading or a question for the user, and both already
exist in the client with consent, validation and a round loop. There is nothing
new to build on that side.

Three rules that come out of use rather than theory:

* **Exact, along the path actually walked.** No distance, no threshold. A wrong
  guess would reach a user. Fuzziness belongs in the authoring tool, where a
  human approves it.
* **"Don't know" must never dead-end.** Not everyone knows whether their card is
  overclocked. That branch falls back to the parent and takes whatever answer
  applies there — and the fallback is computed on the way *down*, so it is
  already in hand when the question is asked.
* **A switch on an already-collected fact is worth more than a new question.** It
  re-partitions history, because the value is in every stored signature. A new
  question only works forward. The authoring tool has to show that difference,
  or the expensive option gets picked without anyone noticing.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .errors import IngestRefused

#: The whole comparison vocabulary. Deliberately small and total: every operator
#: here has one meaning, and there is no regular expression, no distance and no
#: threshold anywhere in it.
OPS = ("eq", "lt", "le", "gt", "ge", "in")
COMPARATORS = ("string", "number", "version")


@dataclass(frozen=True, slots=True)
class Answer:
    solution_id: str
    node_id: int
    #: The nodes actually walked. Published with the answer so a developer can
    #: see which path produced it rather than inferring it.
    path: tuple[int, ...] = ()
    #: The facts this answer actually turned on — the switches taken, not every
    #: fact that happened to be present. A publisher judging whether an outcome
    #: says anything about their rule needs *these*, because whether the answer
    #: was measured or supplied only matters for the facts that decided it.
    decided_on: tuple[str, ...] = ()


@dataclass(frozen=True, slots=True)
class Need:
    """The third outcome: not an answer and not a refusal, but "I still need X".

    `fallback` is the answer that applies if the user cannot say — computed on
    the way down rather than looked for on the way back up. That is what makes a
    dead end structurally impossible rather than merely unlikely, and it means
    the client can honour "don't know" without a second round trip.
    """

    probe: dict
    node_id: int
    reason: str
    fallback: Answer | None = None


@dataclass(frozen=True, slots=True)
class NoStatement:
    reason: str


Outcome = Answer | Need | NoStatement


@dataclass
class Node:
    id: int
    parent_id: int | None
    depth: int
    match_op: str | None = None
    match_value: Any = None
    edge_label: str | None = None
    switch_fact: str | None = None
    switch_kind: str | None = None
    comparator: str | None = None
    probe: dict | None = None
    solution_id: str | None = None
    #: The solution here stands in for "I would rather not say" and nothing
    #: else: a value that contradicts every branch below must not land on it.
    fallback_only: bool = False
    children: list["Node"] = field(default_factory=list)


def _version_key(value: str) -> tuple:
    parts = []
    for chunk in str(value).replace("-", ".").split("."):
        digits = "".join(c for c in chunk if c.isdigit())
        parts.append(int(digits) if digits else 0)
    return tuple(parts)


def _coerce(value: Any, comparator: str):
    if comparator == "number":
        try:
            return float(str(value).strip())
        except ValueError as e:
            raise ValueError(f"{value!r} is not a number") from e
    if comparator == "version":
        return _version_key(value)
    return str(value)


def matches(value: Any, op: str, expected: Any, comparator: str) -> bool:
    """One comparison, exactly. Raises rather than guessing on a bad value.

    A value the comparator cannot read is not a silent non-match: that would
    make an unreadable reading indistinguishable from one that genuinely did not
    match, and the two lead to different places.
    """
    if op == "in":
        if not isinstance(expected, (list, tuple)):
            raise ValueError("`in` needs a list")
        return any(_coerce(value, comparator) == _coerce(e, comparator) for e in expected)

    left, right = _coerce(value, comparator), _coerce(expected, comparator)
    if op == "eq":
        return left == right
    if op == "lt":
        return left < right
    if op == "le":
        return left <= right
    if op == "gt":
        return left > right
    if op == "ge":
        return left >= right
    raise ValueError(f"unknown operator {op!r} — permitted: {list(OPS)}")


def validate_tree(root: Node) -> None:
    """Reject a tree that could be ambiguous at runtime.

    Two children of one node matching the same value is not something to resolve
    with a coin flip when a user is waiting. It is a defect in the tree, and this
    is where a defect in a tree is supposed to be caught.
    """
    # `carried` is the nearest answer at or above each node — the same value the
    # walk maintains on the way down, computed here so the tree can be refused
    # at authoring time rather than dead-ending at runtime.
    stack = [(root, None, ())]
    while stack:
        node, carried, path = stack.pop()
        carried = node.solution_id or carried

        # "Don't know" is a valid answer and must never dead-end. Not everyone
        # knows whether their card is overclocked, or which command they ran. A
        # question whose unanswered branch leads nowhere is a dead end dressed
        # as a diagnosis, so it is a defect in the tree and it is caught here.
        #
        # Said in the publisher's terms, because since `0017` it reaches them on
        # their dashboard: "node 6" named a row in our table, and the path that
        # leads to the question is something they wrote.
        if node.switch_kind == "question" and carried is None:
            where = (" once " + " and ".join(path)) if path else " first"
            raise IngestRefused(
                f"the question about {node.switch_fact!r} is asked{where}, and no "
                f"solution applies before it is answered. Somebody who cannot answer "
                f"would be left with nothing — 'I don't know' has to fall back to an "
                f"answer above the question, and there is none. A solution whose "
                f"conditions stop short of {node.switch_fact!r} on that path would be "
                f"the answer it falls back to."
            )

        if node.switch_fact is None:
            if node.children:
                raise IngestRefused(
                    f"node {node.id} has children but no switch — nothing would "
                    f"decide between them")
            continue
        if node.comparator not in COMPARATORS:
            raise IngestRefused(f"node {node.id}: comparator {node.comparator!r} is not one "
                                f"of {list(COMPARATORS)}")
        if node.switch_kind == "question" and not node.probe:
            raise IngestRefused(
                f"node {node.id} switches on a question with no probe — there is "
                f"nothing to ask")

        for child in node.children:
            if child.match_op not in OPS:
                raise IngestRefused(f"node {child.id}: operator {child.match_op!r} is not one "
                                    f"of {list(OPS)}")

        # Equality branches must be distinct. Ordered branches are checked at
        # runtime by taking the first match in declaration order, which the
        # author controls — but two `eq` on the same value is unambiguously a
        # mistake rather than a priority.
        seen = []
        for child in node.children:
            if child.match_op == "eq":
                key = str(child.match_value)
                if key in seen:
                    raise IngestRefused(
                        f"node {node.id} has two children matching {key!r} on "
                        f"{node.switch_fact} — a user would be waiting on a coin flip")
                seen.append(key)
        stack.extend(
            (c, carried, path + (c.edge_label or f"{node.switch_fact} {c.match_op} {c.match_value}",))
            for c in node.children)


def walk(root: Node, facts: dict) -> Outcome:
    """Walk the tree against the facts. Exact, along the path actually taken.

    `carried` is the nearest ancestor's answer, maintained on the way down. It
    is what a `Need` hands back as its fallback, and what a "don't know" lands
    on — so nothing ever has to walk back up looking for one.
    """
    node, carried, path, decided = root, None, [], []
    # A carried answer that only stands in for a missing or declined fact. It is
    # still the fallback for "I would rather not say"; it is never the answer to
    # a value that contradicts the rule it belongs to.
    settled = None
    while True:
        path.append(node.id)
        if node.solution_id:
            carried = Answer(node.solution_id, node.id, tuple(path), tuple(decided))
            if not node.fallback_only:
                settled = carried

        if node.switch_fact is None:
            return carried or NoStatement("no solution on this path")

        fact = node.switch_fact

        # An explicitly declined fact is an answer: it means "do not ask me
        # again", and the endpoint must honour that rather than looping.
        if facts.get(f"{fact}.declined") is True:
            return carried or NoStatement(f"{fact} was declined and nothing applies without it")

        if fact not in facts or facts[fact] is None:
            return Need(
                probe=node.probe or {"id": fact, "kind": "machine",
                                     "describes": fact, "why": "it decides between two answers"},
                node_id=node.id,
                reason=f"Two answers look alike here. {fact} separates them.",
                fallback=carried,
            )

        chosen = None
        for child in node.children:
            try:
                if matches(facts[fact], child.match_op, child.match_value, node.comparator):
                    chosen = child
                    break
            except ValueError:
                # The value cannot be read as this comparator expects. Not a
                # non-match — we do not know, so we stop rather than guess.
                return settled or NoStatement(
                    f"{fact}={facts[fact]!r} cannot be compared as {node.comparator}")

        if chosen is None:
            # Nothing matched. This is not an opportunity to pick the nearest —
            # and it used to be exactly that: the carried answer came back even
            # when it was only the fallback for a declined question, so a reading
            # that contradicted a rule returned that rule's own answer. A settled
            # answer above still applies, because its conditions were met on the
            # way here and this fact is not one of them.
            return settled or NoStatement(f"no branch matches {fact}={facts[fact]!r}")
        # Recorded only once a branch was actually taken on it: a fact the walk
        # looked at and could not use did not decide anything.
        decided.append(fact)
        node = chosen


def load(conn, tree_id: int) -> Node:
    """One query, then assembled in memory. A tree is tens of nodes; a recursive
    CTE per request would cost more than the read it replaces."""
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, parent_id, depth, match_op, match_value, edge_label, switch_fact, "
            "       switch_kind, comparator, probe, solution_id, fallback_only "
            "FROM tree_node WHERE tree_id = %s ORDER BY depth, id", (tree_id,))
        rows = cur.fetchall()
    if not rows:
        raise IngestRefused(f"tree {tree_id} has no nodes")

    nodes = {r["id"]: Node(**{k: r[k] for k in
                              ("id", "parent_id", "depth", "match_op", "match_value",
                               "edge_label", "switch_fact", "switch_kind", "comparator",
                               "probe", "solution_id", "fallback_only")}) for r in rows}
    root = None
    for node in nodes.values():
        if node.parent_id is None:
            root = node
        else:
            nodes[node.parent_id].children.append(node)
    if root is None:
        raise IngestRefused(f"tree {tree_id} has no root")
    return root


def find_tree(conn, source_id: int, problem_class: str) -> int | None:
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM tree WHERE source_id = %s AND problem_class = %s "
                    "AND valid_to IS NULL", (source_id, problem_class))
        row = cur.fetchone()
    return row["id"] if row else None
