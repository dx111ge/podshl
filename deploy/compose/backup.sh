#!/usr/bin/env bash
# The two things that must survive the machine, copied off it.
#
#   ./backup.sh [directory]        default: ./backups
#
# Writes a timestamped directory with
#   log.ed25519    the log's signing key — lose it and the log cannot continue
#   log_key.json   its public half, what a client build pins (-LogKey)
#   podshl.dump    the database, pg_dump custom format
#   SHA256SUMS
#
# Not the epoch salts: they are meant to be destroyed when the epoch rolls, and
# a salt in a backup outlives the promise that it was discarded.
#
# The dump holds what people sent (the `observation` table). Keep it under the
# same care as the key, and copy it somewhere that is not this machine.
set -euo pipefail
cd "$(dirname "$0")"

project=${COMPOSE_PROJECT_NAME:-podshl}
dc() { docker compose -p "$project" "$@"; }

out="${1:-backups}/$(date -u +%Y-%m-%dT%H%M%SZ)"
umask 077
mkdir -p "$out"

dc exec -T db sh -c 'PGPASSWORD="$POSTGRES_PASSWORD" pg_dump -U podshl -d podshl -Fc' > "$out/podshl.dump"
dc exec -T server cat /var/lib/podshl/log.ed25519 > "$out/log.ed25519"
dc exec -T server python -c 'import json; from podshl.server import sth; from podshl.jws import public_jwk; print(json.dumps(public_jwk(sth.key())))' > "$out/log_key.json"

# A zero-byte file is a backup that only looks like one.
for f in podshl.dump log.ed25519 log_key.json; do
  [ -s "$out/$f" ] || { echo "backup: $f is empty — nothing here is a backup" >&2; exit 1; }
done
(cd "$out" && sha256sum podshl.dump log.ed25519 log_key.json > SHA256SUMS)
echo "$out"
