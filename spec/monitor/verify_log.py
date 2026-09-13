#!/usr/bin/env python3
"""Watch a PODSHL transparency log. Depends on nothing the operator controls.

    python verify_log.py https://log.example --key <log_id>

Needs `cryptography` for the Ed25519 check — the one dependency, and it is not
one the operator controls.

Certificate Transparency works because third parties watch. A log only its
operator can verify is decorative, so this exists and is deliberately small
enough to read in one sitting: it vendors the operator's own Merkle arithmetic
and uses it to check the operator.

What it checks, in the order that matters:

1. **The head is signed by the key you pinned.** An unpinned key makes the whole
   exercise theatre — whoever serves the log would simply serve a key too.
2. **The new tree extends the old one.** This is the check a naive monitor
   forgets, and it is the one that catches a rewrite: an inclusion proof only
   says an entry is in *some* tree.
3. **The entries served are the entries committed to.** Folding the leaf hashes
   back to the root catches a log that proves one thing and serves another.
4. **Nothing about your anchor happened that you did not do.** Especially
   `attestation_issued` and `key_changed` — an attestation naming your domain
   that you did not ask for is the case this whole structure exists to make
   visible.

State lives in a local file, because a monitor that forgets the last root it saw
cannot detect anything at all.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import sys
import urllib.request
from pathlib import Path

# ---------------------------------------------------------------- vendored
# Copied verbatim from the operator's src/podshl/server/merkle.py, so this
# checks them with their own arithmetic. If the two ever disagree, that is
# itself the finding.

LEAF_PREFIX, NODE_PREFIX = b"\x00", b"\x01"


def leaf_hash(data: bytes) -> bytes:
    return hashlib.sha256(LEAF_PREFIX + data).digest()


def node_hash(left: bytes, right: bytes) -> bytes:
    return hashlib.sha256(NODE_PREFIX + left + right).digest()


def _lpo2(n: int) -> int:
    k = 1
    while k * 2 < n:
        k *= 2
    return k


def root_from_leaves(leaves) -> bytes:
    if not leaves:
        return hashlib.sha256(b"").digest()
    if len(leaves) == 1:
        return leaves[0]
    k = _lpo2(len(leaves))
    return node_hash(root_from_leaves(leaves[:k]), root_from_leaves(leaves[k:]))


def verify_consistency(old_size, new_size, old_root, new_root, path) -> bool:
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


# ---------------------------------------------------------------- the monitor

def get(base: str, path: str) -> dict:
    with urllib.request.urlopen(f"{base.rstrip('/')}{path}", timeout=30) as r:
        return json.loads(r.read())


def canonical(entry: dict) -> bytes:
    """The bytes the operator hashed. JCS: keys by UTF-16 code unit, no spaces,
    no escaped solidus, integers only."""
    return json.dumps(entry, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False).encode()


def b64u_decode(s: str) -> bytes:
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def verify_head(head: dict, jwk: dict) -> bool:
    """The detached JWS over the head, against the key you pinned.

    This was missing, and its absence made everything below it decorative. The
    monitor compared the `log_id` the head *said* it had against the one you
    pinned — a string in a document the operator serves, so an operator serving
    a fabricated head would simply put your own id in it. Nothing then stopped
    a self-consistent invented log: the entries fold to its root because they
    are its entries, and consistency proofs hold because it is consistent with
    itself.

    The signature is what a fabricated head cannot have. Detached JWS, the
    signing input `b64u(JCS(protected)) . b64u(JCS(body))` — the same two lines
    the client runs, vendored here for the same reason the Merkle arithmetic is.
    """
    try:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError:
        print("REFUSED: `cryptography` is not installed, so the head's signature "
              "cannot be checked — and everything else here is only worth as much "
              "as that check. pip install cryptography")
        return False
    try:
        sig = head["signature"]
        if isinstance(sig, str):            # stored heads carry it as JSON text
            sig = json.loads(sig)
        pub = Ed25519PublicKey.from_public_bytes(b64u_decode(jwk["x"]))
        signing_input = (sig["protected"] + "."
                         + base64.urlsafe_b64encode(canonical(head["sth"]))
                           .decode().rstrip("=")).encode()
        pub.verify(b64u_decode(sig["signature"]), signing_input)
        return True
    except Exception:
        return False


def check(base: str, expect_key: str | None, watch_host: str | None,
          state_path: Path) -> int:
    head = get(base, "/log/sth")
    sth, size = head["sth"], head["sth"]["tree_size"]
    root = bytes.fromhex(sth["root_hash"])

    if expect_key and sth["log_id"] != expect_key:
        print(f"REFUSED: the log identifies as {sth['log_id']}, you pinned {expect_key}.")
        print("         A different key is a different log, whatever the URL says.")
        return 2
    if not expect_key:
        print(f"warning: no key pinned. Pin --key {sth['log_id']} or this proves nothing.")

    # The key the operator serves, held to the id you pinned — and then the
    # head held to that key. Both halves are needed: the id alone is a string
    # in a document they wrote, and a key alone is whatever they felt like
    # serving today.
    served = get(base, "/log/key")
    key_id = hashlib.sha256(canonical(served["key"])).hexdigest()
    if expect_key and key_id != expect_key:
        print(f"REFUSED: the key served hashes to {key_id}, you pinned {expect_key}.")
        print("         The head names your id and the key behind it is not yours.")
        return 2
    if not verify_head(head, served["key"]):
        print("ALARM: the signed head does not verify under that key. A head that "
              "cannot be checked is not a head; everything below this would be "
              "the operator agreeing with themselves.")
        return 2
    print(f"head signed by {key_id[:16]}… at size {size}")

    state = json.loads(state_path.read_text()) if state_path.exists() else {}
    old_size = state.get("tree_size", 0)
    old_root = bytes.fromhex(state["root_hash"]) if state.get("root_hash") else b""

    if old_size and size < old_size:
        print(f"ALARM: the log shrank, {old_size} -> {size}. A log cannot shrink.")
        return 2

    if old_size and size > old_size:
        proof = get(base, f"/log/proof/consistency?first={old_size}&second={size}")
        if not verify_consistency(old_size, size, old_root, root,
                                  [bytes.fromhex(h) for h in proof["path"]]):
            print(f"ALARM: {old_size} is not a prefix of {size}. The log was rewritten.")
            return 2
        print(f"consistent: {old_size} -> {size}")

    # What is served must be what was committed to.
    leaves, start = [], 0
    while start < size:
        page = get(base, f"/log/entries?start={start}&limit=1000")["entries"]
        if not page:
            break
        for row in page:
            leaves.append(leaf_hash(canonical(row["entry"])))
        start += len(page)
    if len(leaves) != size:
        print(f"ALARM: the head claims {size} entries, {len(leaves)} were served.")
        return 2
    if root_from_leaves(leaves) != root:
        print("ALARM: the entries served do not fold to the signed root.")
        return 2
    print(f"served entries match the signed root ({size} entries)")

    if watch_host:
        mine = [row for row in
                (get(base, f"/log/entries?start=0&limit={max(size,1)}")["entries"])
                if watch_host in json.dumps(row["entry"])]
        print(f"\nentries naming {watch_host}: {len(mine)}")
        for row in mine:
            kind = row["entry"]["kind"]
            flag = "  <-- check this was you" if kind in (
                "attestation_issued", "key_changed") else ""
            print(f"  seq {row['seq']:>6}  {kind}{flag}")

    state_path.write_text(json.dumps({"tree_size": size, "root_hash": sth["root_hash"]}))
    print(f"\nstate saved: size {size}")
    print("Publish this root somewhere others can see it. Two monitors comparing "
          "roots at the same size is what detects a split view; consistency proofs "
          "alone only prove the log is self-consistent for you.")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("base", help="the log's base URL")
    p.add_argument("--key", help="the log_id you pinned. Without it this proves nothing.")
    p.add_argument("--host", help="an anchor to watch for entries about")
    p.add_argument("--state", default="podshl-monitor.json")
    a = p.parse_args()
    return check(a.base, a.key, a.host, Path(a.state))


if __name__ == "__main__":
    sys.exit(main())
