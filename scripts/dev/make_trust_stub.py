"""The out-of-band key source, for a checkout that has just been cloned.

`var/ans_stub.json` stands in for the ANS/DNS lookup: it is where the client
finds a vendor's key *without* asking the vendor's own card for it, and that
separation is the whole reason a self-asserted card can be refused. Five places
read the file and nothing ever wrote it, so a fresh checkout failed ten client
cases on a missing file rather than on a defect — the same gap HANDOVER records
for `var/test_card.json`, which now heals itself by fetching from the running
vendor.

The key is read from `var/vendor.ed25519`, the vendor's own private key, and
not fetched over HTTP. A stub built from the card it is supposed to
authenticate would verify nothing.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from podshl.jws import load_or_create_key, public_jwk  # noqa: E402

VAR = ROOT / "var"

# The counterparty answers on loopback. `localhost` is here as well because a
# resolver keyed by name cannot know which of the two the operator typed.
HOSTS = ("127.0.0.1", "localhost")


def main() -> int:
    # Idempotent, and the same call the vendor makes: it creates the key on
    # first run and loads it afterwards, so the stub cannot describe a key
    # different from the one that signs the card.
    jwk = public_jwk(load_or_create_key(VAR / "vendor.ed25519"))
    out = VAR / "ans_stub.json"
    out.write_text(json.dumps({h: jwk for h in HOSTS}, indent=2) + "\n")
    print(f"  {out.relative_to(ROOT)}  ({', '.join(HOSTS)})")

    # The log's public key, pinned the same way and for the same reason. The
    # discovery index arrives signed; an index carrying the key it is checked
    # with proves exactly as much as a self-asserted agent card, so the client
    # refuses an index it cannot check against a key it already had.
    log = VAR / "log_key.json"
    log.write_text(json.dumps(public_jwk(load_or_create_key(VAR / "log.ed25519")), indent=2) + "\n")
    print(f"  {log.relative_to(ROOT)}  (the log signing key)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
