#!/usr/bin/env bash
# Build the client the way a release is built, and refuse to hand back one that
# is not.
#
# `option_env!` is read at compile time, and cargo does not rebuild when only an
# environment variable changed. So a client built once with the operator and the
# log key compiled in, and then rebuilt for any other reason without them, comes
# out silently pointing at loopback with no key — which is not a broken client,
# it is a *plausible* one: it starts, it draws its window, and it quietly cannot
# verify the published directory, so every project falls through to the model.
#
# That cost an afternoon on 2026-09-14. This script is the answer: one way to
# build a client for an operator, and it checks the result rather than trusting
# the build.
#
#   scripts/build_client.sh sdota.de
#   scripts/build_client.sh sdota.de https://www.sdota.de
#
# The second argument is the address to compile in when it is not
# `https://<operator>`, and it exists for one reason: on the network this is
# developed on the router answers `sdota.de` with an address no authoritative
# server claims and nothing listens on, about half the time, alternating with
# the right one. A client built for the apex there cannot reach an operator
# that is perfectly healthy, and every published project falls through to the
# model. `www.sdota.de` is a CNAME to the same name and is answered correctly.
# The key is still the operator own: this changes the address, not the trust.
#
# Releases keep the apex. The authoritative record is right; one network is not.
set -euo pipefail

OPERATOR="${1:?usage: scripts/build_client.sh <operator> [server-url]   (e.g. sdota.de)}"
KEYFILE="release/$OPERATOR/log_key.json"
[ -f "$KEYFILE" ] || { echo "no public log key at $KEYFILE" >&2; exit 1; }

BASE="${2:-https://$OPERATOR}"
BASE="${BASE%/}"
KEY="$(cat "$KEYFILE")"
X="$(python3 -c 'import json,sys;print(json.load(sys.stdin)["x"])' < "$KEYFILE")"

echo "· building for $BASE"
# `touch` so cargo recompiles the crate root: nothing else here changed, and
# without it the previous binary — the one without these values — is returned
# as up to date.
touch client-rs/src/main.rs
( cd client-rs
  PODSHL_BUILD_SERVER_URL="$BASE" \
  PODSHL_BUILD_INDEX_URL="$BASE" \
  PODSHL_BUILD_LOG_KEY="$KEY" \
    cargo build --release )

# `CARGO_TARGET_DIR` moves the output, and the container sets it to a volume.
# Looking in `client-rs/target` regardless is how this script checked a file the
# build had not written — and said the operator was missing from a binary that
# did not exist. It caught itself, which is the only reason this is a note and
# not another afternoon.
BIN="${CARGO_TARGET_DIR:-client-rs/target}/release/podshl-client"
[ -f "$BIN" ] || { echo "no binary at $BIN after building — wrong target directory?" >&2; exit 1; }
echo "· checking what actually landed in the binary"
# The path is named in every message. A check that says what it found without
# saying *where it looked* is the shape of every wasted hour today: it was right
# about the file it read and reading the wrong file.
echo "  checking $BIN"
# Process substitution, not a pipe, and the reason is worth the line: `grep -q`
# stops at the first match and closes the pipe, `strings` then dies of SIGPIPE,
# and `set -o pipefail` reports the pipeline as failed — so the check fails
# *because it succeeded*, and says the operator is missing from a binary that
# has it. It cost an hour, in a script written to stop exactly this kind of
# hour.
grep -qF "$BASE" < <(strings "$BIN") \
  || { echo "$BASE is not in $BIN — it would talk to loopback" >&2; exit 1; }
grep -qF "$X" < <(strings "$BIN") \
  || { echo "the log key is not in $BIN — it could not verify the directory, and every project would fall through to the model" >&2; exit 1; }

# **A release must not carry the test-only surface.** `perform_reads`,
# `send_published_report` and `llm_translate` are on the `invoke` surface only
# under `--features uitest`: from a window they happen behind consent screens,
# from a command line they would not. Asked of the artefact rather than assumed
# from the build.
echo "· checking the test-only surface is not in it"
for cmd in perform_reads send_published_report llm_translate; do
  if ! "$BIN" invoke "$cmd" '{}' 2>&1 | grep -q "is not in this build"; then
    echo "$BIN answers $cmd — it was built with the uitest feature and must not be released" >&2
    exit 1
  fi
done

echo "· asking the binary itself"
"$BIN" invoke endpoints '{}' | python3 -c '
import json,sys
d = json.load(sys.stdin)
op = d.get("operator","")
print(f"  operator: {op}")
raise SystemExit(0 if op.startswith("https://") else "the binary reports a loopback operator")
'
# Install it, because a client that was built and not installed is the same
# problem one step later: the desktop entry, the bar icon and the menu row all
# run `podshl-client` from the PATH, and none of them care what is sitting in a
# target directory. That gap cost an afternoon — the fix was built, verified,
# and never reached the program anybody was actually starting.
DEST="${PODSHL_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$DEST"
install -m755 "$BIN" "$DEST/podshl-client"
installed_op=$("$DEST/podshl-client" invoke endpoints '{}' | python3 -c 'import json,sys;print(json.load(sys.stdin)["operator"])')
[ "$installed_op" = "$BASE" ] \
  || { echo "installed client reports $installed_op, not $BASE" >&2; exit 1; }

echo "built and installed $DEST/podshl-client for $BASE"
