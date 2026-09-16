#!/usr/bin/env bash
# Register the Actions runner that gates the working repository.
#
#   scripts/gitea_runner.sh <gitea url>
#
# The address is not written out here, for the reason
# `deploy/compose/update.sh` states about its own: `PUBLISHING.md` forbids an
# address of the maintainer's own network in the published tree, and a usage
# example is exactly where one gets left behind. This one was -- it stood here
# from 2026-09-15 until the release scan of 2026-09-16 found it, which is the
# scan doing the job the checklist says to repeat "every time, not once".
#
# **Why this is a script.** The runner is one `docker run` with six flags, and
# for a while it was a command in somebody's shell history -- which means the
# machine it runs on is the only record of how it was made, and a machine that
# has been rebuilt is not a record. It is also where two settings live that are
# not obvious and cost a day to find; those are in
# `deploy/gitea-runner/config.yaml`, which this mounts.
#
# The registration token is fetched with the Gitea admin API, using whatever
# credentials git already has for the host -- the same ones used to push. It is
# a one-time token: after `create` the runner holds its own, in its data volume,
# and the token is of no further use.
#
# Re-running this replaces the container. The data volume is kept, so the runner
# keeps its identity; pass --fresh to discard it and register a new one.
set -euo pipefail

cd "$(dirname "$0")/.."

FRESH=""
[ "${1:-}" = "--fresh" ] && { FRESH=1; shift; }
URL="${1:?usage: scripts/gitea_runner.sh [--fresh] <gitea url>}"
NAME="${GITEA_RUNNER_NAME:-podshl-builder}"
HOST=${URL#*://}

say() { printf '\n\033[1m· %s\033[0m\n' "$1"; }

say "asking git for the credentials it uses for $HOST"
CRED=$(printf 'protocol=%s\nhost=%s\n\n' "${URL%%:*}" "$HOST" | git credential fill)
USER=$(printf '%s\n' "$CRED" | sed -n 's/^username=//p')
PASS=$(printf '%s\n' "$CRED" | sed -n 's/^password=//p')
[ -n "$USER" ] && [ -n "$PASS" ] || { echo "git has no credentials for $HOST" >&2; exit 1; }

say "a registration token"
# Instance scope on purpose: this machine builds this project and nothing else,
# and an instance runner is the one shape that does not go stale when the
# repository is renamed or moved between owners.
TOKEN=$(curl -fsS -m 20 -u "$USER:$PASS" -X POST \
  "$URL/api/v1/admin/actions/runners/registration-token" \
  | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')
[ -n "$TOKEN" ] || { echo "no token came back -- is $USER a Gitea administrator?" >&2; exit 1; }

say "replacing the container"
docker rm -f "$NAME" >/dev/null 2>&1 || true
[ -n "$FRESH" ] && docker volume rm "$NAME-data" >/dev/null 2>&1 || true

# `MSYS_NO_PATHCONV=1`: on a Windows shell MSYS rewrites anything that looks
# like an absolute path, so `/var/run/docker.sock` arrives as
# `C:/Program Files/Git/var/run/docker.sock` and the mount is a directory that
# does not exist. The repo has been bitten by this twice before.
CFG="$PWD/deploy/gitea-runner/config.yaml"
MSYS_NO_PATHCONV=1 docker run -d --name "$NAME" --restart unless-stopped \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v "$NAME-data":/data \
  -v "$CFG":/config.yaml:ro \
  -e CONFIG_FILE=/config.yaml \
  -e GITEA_INSTANCE_URL="$URL" \
  -e GITEA_RUNNER_REGISTRATION_TOKEN="$TOKEN" \
  -e GITEA_RUNNER_NAME="$NAME" \
  gitea/act_runner:latest >/dev/null

say "waiting for it to say it registered"
# Not `sleep 10 && echo done`. A runner that failed to register sits there as a
# running container, and "the container is up" is the answer that made ten
# failed runs look like a working CI.
for _ in $(seq 1 30); do
  if docker logs "$NAME" 2>&1 | grep -q 'declare successfully'; then
    docker logs "$NAME" 2>&1 | grep -m1 -A2 'declare successfully' | sed 's/^/  /'
    say "registered as $NAME"
    exit 0
  fi
  sleep 2
done

echo "it did not register within a minute. Its own words:" >&2
docker logs --tail 20 "$NAME" >&2
exit 1
