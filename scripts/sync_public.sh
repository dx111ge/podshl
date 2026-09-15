#!/usr/bin/env bash
# Copy what may be published into the public repository, with the modes intact.
#
# `PUBLISHING.md` says which files stay in the working repository and which go
# out; it does not say how, so the how was a person copying files. That went
# wrong the first three times it mattered on 2026-09-15: `scripts/ci.sh`,
# `build_client.sh` and `walk_client.py` arrived in the public repository
# without their executable bit, because a copy on Windows does not carry one and
# nothing checked. CI runs `scripts/ci.sh` directly, so the first public run
# would have failed with "permission denied" — a broken publication caused by
# the act of publishing.
#
#   scripts/sync_public.sh /path/to/public-checkout
#
# It only writes files. Reviewing the diff, committing, tagging and pushing stay
# a person's job, because what goes out publicly is a decision and not a step.
set -euo pipefail

DEST="${1:?usage: scripts/sync_public.sh <path to the public checkout>}"
cd "$(dirname "$0")/.."

[ -d "$DEST/.git" ] || { echo "$DEST is not a git checkout" >&2; exit 1; }

# The working repository's own notes, named here because `PUBLISHING.md` names
# them. `run_testcases.py`'s `P8` checks that nothing published *refers* to one;
# this is the other half, which is that none of them is published.
INTERNAL="BASELINE.md
BENEFITS.md
DIRECTION.md
DISCOVERY.md
HANDOVER.md
OUTREACH.md"

for f in $INTERNAL; do
  [ -e "$f" ] || { echo "PUBLISHING.md names $f and it does not exist — has it been renamed?" >&2; exit 1; }
done

PUBLISHABLE=$(git ls-files | grep -vxF "$INTERNAL")
COUNT=$(echo "$PUBLISHABLE" | wc -l)
[ "$COUNT" -gt 100 ] || { echo "only $COUNT files to publish — this is not enumerating the tree" >&2; exit 1; }

echo "· copying $COUNT files into $DEST"
echo "$PUBLISHABLE" | while read -r f; do
  mkdir -p "$DEST/$(dirname "$f")"
  cp "$f" "$DEST/$f"
done

# **The modes, from the index rather than from the filesystem.** A Windows
# checkout has no executable bit to copy, so the truth is what git recorded.
echo "· carrying the executable bits"
git ls-files -s | awk '$1=="100755"{ $1=$2=$3=""; sub(/^ +/, ""); print }' | while read -r f; do
  case "$INTERNAL" in *"$f"*) continue ;; esac
  ( cd "$DEST" && git update-index --chmod=+x "$f" 2>/dev/null ) \
    || echo "  could not set +x on $f (not tracked there yet — add it, then run this again)"
done

echo "· what the public repository would gain or lose"
( cd "$DEST" && git add -A && git status --short | head -40 )
echo
echo "Nothing has been committed. Read the diff, then commit and push from $DEST."
