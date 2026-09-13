#!/usr/bin/env bash
# A backup from backup.sh, onto a machine whose volumes are empty.
#
#   ./restore.sh backups/2026-09-13T091500Z
#
# Refuses to overwrite: a state volume that already holds a key, or a database
# that already has tables, stops it. Restoring over a running log is how two
# logs end up signed by one key — decide that by hand, not with this script.
set -euo pipefail
cd "$(dirname "$0")"

src=${1:?usage: restore.sh <backup directory>}
project=${COMPOSE_PROJECT_NAME:-podshl}
dc() { docker compose -p "$project" "$@"; }

(cd "$src" && sha256sum -c SHA256SUMS)

dc up -d --wait db

if dc run --rm --no-deps -T --entrypoint sh server -c 'test -e /var/lib/podshl/log.ed25519'; then
  echo "restore: the state volume already holds a signing key — refusing" >&2
  exit 1
fi
tables=$(dc exec -T db sh -c 'PGPASSWORD="$POSTGRES_PASSWORD" psql -U podshl -d podshl -tAc "select count(*) from information_schema.tables where table_schema = '"'"'public'"'"'"')
if [ "$tables" != "0" ]; then
  echo "restore: the database already has $tables tables — refusing" >&2
  exit 1
fi

dc run --rm --no-deps -T --entrypoint sh server \
  -c 'umask 077 && cat > /var/lib/podshl/log.ed25519' < "$src/log.ed25519"
dc exec -T db sh -c 'PGPASSWORD="$POSTGRES_PASSWORD" pg_restore -U podshl -d podshl --no-owner' < "$src/podshl.dump"

dc up -d
echo "restored from $src — check that /log/sth names the log id you pinned"
