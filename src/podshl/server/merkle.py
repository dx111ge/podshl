"""RFC 6962 Merkle tree arithmetic. No database, on purpose.

`spec/monitor/verify_log.py` vendors this file verbatim, so a third party checks
us with our own code. That is the point of the exercise: Certificate
Transparency works because other people watch, and a log only its operator can
verify is decorative.

The hashing is RFC 6962 §2.1 exactly:

    MTH({})   = SHA-256()                                -- the empty string
    MTH(d[0]) = SHA-256(0x00 || d[0])
    MTH(D[n]) = SHA-256(0x01 || MTH(D[0:k]) || MTH(D[k:n]))

where **k is the largest power of two strictly less than n**. That rule, rather
than splitting in half, is what makes the tree append-only-friendly: every left
subtree is a complete, aligned block, so a node once written never changes and
the only work an append does is on the right-hand edge.

The prefixes are not decoration. Without them a leaf and an internal node could
hash identically, and an attacker could present a subtree as a leaf.
"""
from __future__ import annotations

import hashlib
from typing import Callable, Sequence

LEAF_PREFIX = b"\x00"
NODE_PREFIX = b"\x01"


def leaf_hash(data: bytes) -> bytes:
    return hashlib.sha256(LEAF_PREFIX + data).digest()


def node_hash(left: bytes, right: bytes) -> bytes:
    return hashlib.sha256(NODE_PREFIX + left + right).digest()


def empty_root() -> bytes:
    return hashlib.sha256(b"").digest()


def _largest_power_of_two_below(n: int) -> int:
    """k such that k < n <= 2k. Undefined for n < 2, and callers never ask."""
    k = 1
    while k * 2 < n:
        k *= 2
    return k


# A NodeReader answers "what is the hash of the complete block at (level, index)".
# Level 0 is the leaves. Node (L, i) covers leaves [i*2^L, (i+1)*2^L).
NodeReader = Callable[[int, int], bytes]


def range_root(node: NodeReader, lo: int, hi: int) -> bytes:
    """MTH of leaves [lo, hi).

    Decomposed into maximal aligned complete blocks, folded right-associatively.
    That set is provably the same one the recursive definition produces, which
    is why every block asked for here is already stored and none has to be
    recomputed from leaves.
    """
    if hi <= lo:
        return empty_root()

    blocks: list[bytes] = []
    i = lo
    while i < hi:
        size = 1
        # The largest aligned block starting at i that still fits in [i, hi).
        while i % (size * 2) == 0 and i + size * 2 <= hi:
            size *= 2
        level = size.bit_length() - 1
        blocks.append(node(level, i // size))
        i += size

    acc = blocks[-1]
    for h in reversed(blocks[:-1]):
        acc = node_hash(h, acc)
    return acc


def root(node: NodeReader, size: int) -> bytes:
    return range_root(node, 0, size)


def inclusion_path(node: NodeReader, index: int, size: int) -> list[bytes]:
    """RFC 6962 PATH(m, D[n]), iteratively.

    Returned root-last, because the RFC's `:` concatenation builds it that way
    and a verifier folds from the leaf upward.
    """
    if not 0 <= index < size:
        raise ValueError(f"leaf {index} is not in a tree of {size}")

    path: list[bytes] = []
    offset, m, n = 0, index, size
    while n > 1:
        k = _largest_power_of_two_below(n)
        if m < k:
            path.append(range_root(node, offset + k, offset + n))
            n = k
        else:
            path.append(range_root(node, offset, offset + k))
            m -= k
            offset += k
            n -= k
    path.reverse()
    return path


def consistency_path(node: NodeReader, old: int, new: int) -> list[bytes]:
    """RFC 6962 PROOF(m, D[n]) — that the tree of `old` is a prefix of `new`.

    This is the check a naive monitor forgets, and it is the one that catches a
    rewrite: an inclusion proof only says an entry is in *some* tree.
    """
    if old == 0:
        return []
    if old > new:
        raise ValueError(f"cannot prove {old} is a prefix of {new}")
    if old == new:
        return []
    return _subproof(node, old, 0, new, True)


def _subproof(node: NodeReader, m: int, offset: int, n: int, is_root: bool) -> list[bytes]:
    if m == n:
        # The old tree is exactly this subtree. Its root is already known to the
        # verifier when this is the whole tree, and needed otherwise.
        return [] if is_root else [range_root(node, offset, offset + n)]

    k = _largest_power_of_two_below(n)
    if m <= k:
        return _subproof(node, m, offset, k, is_root) + [
            range_root(node, offset + k, offset + n)
        ]
    return _subproof(node, m - k, offset + k, n - k, False) + [
        range_root(node, offset, offset + k)
    ]


# ------------------------------------------------------------------ verifying
#
# These take no NodeReader: they are pure arithmetic over a proof, which is what
# lets a monitor run them with no access to anything of ours.


def verify_inclusion(index: int, size: int, leaf: bytes,
                     path: Sequence[bytes], root_hash: bytes) -> bool:
    """Fold the leaf upward with its siblings and compare.

    `fn`/`sn` track the node's index and the last index at the current level.
    The `fn == sn` case is the ragged right edge: there the node has no right
    sibling, so the next sibling supplied is on the left, and levels are skipped
    until the index is odd again. Getting that wrong verifies everything except
    the right edge, which is exactly where an append lands.
    """
    if not 0 <= index < size:
        return False
    fn, sn = index, size - 1
    acc = leaf
    for sibling in path:
        if sn == 0:
            return False
        if fn & 1 or fn == sn:
            acc = node_hash(sibling, acc)
            while fn != 0 and not (fn & 1):
                fn >>= 1
                sn >>= 1
        else:
            acc = node_hash(acc, sibling)
        fn >>= 1
        sn >>= 1
    return sn == 0 and acc == root_hash


def verify_consistency(old_size: int, new_size: int, old_root: bytes,
                       new_root: bytes, path: Sequence[bytes]) -> bool:
    """That the tree of `old_size` is a prefix of the tree of `new_size`.

    This is the check that catches a rewrite, and the one a naive monitor
    forgets: an inclusion proof only says an entry is in *some* tree. Both roots
    are recomputed from the same proof — the old one is not taken on trust.
    """
    if old_size > new_size:
        return False
    if old_size == new_size:
        return not path and old_root == new_root
    if old_size == 0:
        return not path
    if not path:
        return False

    remaining = list(path)
    fn, sn = old_size - 1, new_size - 1
    # Climb out of the old tree's right spine. If that lands at zero the old
    # tree is a complete subtree, so its root is not carried in the proof.
    while fn & 1:
        fn >>= 1
        sn >>= 1

    if fn == 0:
        old_acc = new_acc = old_root
    else:
        old_acc = new_acc = remaining.pop(0)

    for sibling in remaining:
        if sn == 0:
            return False
        if fn & 1 or fn == sn:
            old_acc = node_hash(sibling, old_acc)
            new_acc = node_hash(sibling, new_acc)
            while fn != 0 and not (fn & 1):
                fn >>= 1
                sn >>= 1
        else:
            new_acc = node_hash(new_acc, sibling)
        fn >>= 1
        sn >>= 1

    return sn == 0 and old_acc == old_root and new_acc == new_root


def root_from_leaves(leaves: Sequence[bytes]) -> bytes:
    """The whole tree from its leaf hashes. Slow by design — this is what a
    monitor uses to check that the entries we *serve* are the ones we committed
    to, and what the tests measure the incremental path against."""
    if not leaves:
        return empty_root()
    if len(leaves) == 1:
        return leaves[0]
    k = _largest_power_of_two_below(len(leaves))
    return node_hash(root_from_leaves(leaves[:k]), root_from_leaves(leaves[k:]))
