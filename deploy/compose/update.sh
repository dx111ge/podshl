#!/usr/bin/env bash
# Put the current checkout onto a running operator, and say what changed.
#
# `README.md`'s *Routine* row says what this does -- `git archive` the tree over
# /opt/podshl, then `up -d --build`, migrations first. It is a script because
# the order matters and because the one step nobody does by hand is the one
# before it: a migration that fails leaves the database where it was, and the
# way back is a backup that exists.
#
#   deploy/compose/update.sh <user>@<host>
#
# The host is not written down here on purpose: `PUBLISHING.md` forbids an
# address of the maintainer's own network in the published tree, and a usage
# example is exactly where one gets left behind.
#
# Run from the repository root. Uses the committed HEAD, not the working tree,
# so a deployment is always a commit somebody can name.
set -euo pipefail

HOST="${1:?usage: deploy/compose/update.sh <user@host> [--staging] [remote-path]}"

# **Staging is a second operator on the same host**, with its own project, its
# own volumes, its own database and no Caddy. It exists because a deployment
# went from a laptop straight onto the live operator, and on 2026-09-13 a CRLF
# byte in a migration took it down — there was nowhere for that byte to land
# first.
STAGING=""
COMPOSE_FILES=""
if [ "${2:-}" = "--staging" ]; then
  STAGING=1
  COMPOSE_FILES="-f compose.yaml -f compose.staging.yaml"
  shift
fi
REMOTE="${2:-${STAGING:+/opt/podshl-staging}}"
REMOTE="${REMOTE:-/opt/podshl}"
REF="$(git rev-parse --short HEAD)"
DIRTY="$(git status --porcelain | wc -l)"

if [ "$DIRTY" -ne 0 ]; then
  echo "working tree has $DIRTY uncommitted change(s) — this deploys HEAD ($REF)," >&2
  echo "so those would not go out. Commit them or accept that." >&2
fi

# `backup.sh` dumps through the running server, so it cannot run when the stack
# is down -- and the stack being down is exactly when somebody is deploying in a
# hurry. Refusing there would be a gate that only ever fires on the day it must
# not: no backup is taken, no deployment happens, and the operator stays down.
# So: back up if it can, and if it cannot, require that a complete one already
# exists. Complete means all four files, because a half-written backup is the
# kind that is discovered during a restore.
if [ -n "$STAGING" ]; then
  echo "· staging: no backup, because its database is meant to be replaceable"
  echo "  (if a migration destroys it, that is the result this instance exists to produce)"
else
echo "· backing up first"
if ! ssh "$HOST" "cd $REMOTE/deploy/compose && sudo ./backup.sh"; then
  echo "· backup could not run — looking for one that is already complete"
  ssh "$HOST" "sudo find $REMOTE/deploy/compose/backups /root/podshl-backups \
                 -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort | while read -r d; do
                 for f in podshl.dump log.ed25519 log_key.json SHA256SUMS; do
                   sudo test -s \"\$d/\$f\" || continue 2
                 done
                 echo \"\$d\"
               done" | tail -1 | grep -q . || {
    echo "no complete backup exists and none could be taken — not deploying." >&2
    echo "A migration without a way back is not a deployment." >&2
    exit 1
  }
  echo "  using the existing one; nothing has written since the stack stopped"
fi
fi

echo "· copying $REF"
git archive --format=tar HEAD | ssh "$HOST" "sudo tar -x -C $REMOTE"

echo "· building and migrating"
ssh "$HOST" "cd $REMOTE/deploy/compose && sudo docker compose $COMPOSE_FILES up -d --build"

echo "· what the operator says now"
ssh "$HOST" "cd $REMOTE/deploy/compose && sudo docker compose $COMPOSE_FILES logs --tail=12 migrate 2>&1 || true"
echo
if [ -n "$STAGING" ]; then
  echo "deployed $REF to staging. It has no Caddy and no public address;"
  echo "reach it over the tunnel this deployment already used:"
  echo "  ssh -L 8735:127.0.0.1:8735 $HOST"
  echo "  curl -s http://127.0.0.1:8735/index | head -c 120"
  echo
  echo "When the migrations and the ingest are right here, run the same command"
  echo "without --staging."
  exit 0
fi
echo "deployed $REF. Check it from outside, not from the host:"
echo "  curl -s https://sdota.de/index | head -c 120"
echo "  curl -s https://sdota.de/example/desktop/solutions/nvidia-wayland-black-windows.md | grep -A2 'when:'"
echo "  (that must show session.type alone. It said gpu.driver_version: \"< 555\","
echo "   which is false — the prose still discusses 555, and should, so counting"
echo "   the number is not the check. The rule is.)"
