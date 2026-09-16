#!/usr/bin/env bash
# Brings up the store, then whichever part of the Linux side was asked for.
#
#   all         the store, the operator server (:8725 / ops :8726) and the
#               counterparty (:8721 :8722 :8723 :8727)   [default]
#   server      the store and the operator server only
#   services    the store and the counterparty only
#   testcases   the store and the counterparty, then run_testcases.py
#   release     the Linux artefacts, via mise.toml's own release task
#   migrate     the store, apply migrations, exit
#   shell       the store, then an interactive shell
#   <anything>  run it with the store up and the venv on PATH
set -euo pipefail

PGDATA=${PGDATA:-/pgdata}
PGPORT=${PGPORT:-5433}
PGLOG=/tmp/pg.log

say() { printf '\033[36m·\033[0m %s\n' "$*"; }

# --- the store ---------------------------------------------------------------
# 0700, explicitly, and on every start: the volume outlives the container, and
# postgres refuses a data directory that is group- or world-readable. `install
# -d` without a mode would quietly hand it back 0755 each time.
install -d -m 0700 -o postgres -g postgres "$PGDATA"
# var/ is the host's bind mount; the salts and the log key are written into it.
install -d /app/var /app/var/salt

if [ ! -s "$PGDATA/PG_VERSION" ]; then
  say "initdb $PGDATA"
  gosu postgres initdb -D "$PGDATA" -U podshl --auth=trust -E UTF8 >/dev/null
fi

# The socket lives *in* the data directory, and that is the point: `docker
# compose run` and `up` are different containers with different process tables
# but the same PGDATA volume. A socket under a container-local /var/run makes
# each blind to the other, so the second clears the pid file, starts a second
# postmaster over one data directory, and stops the first one's cluster on its
# way out. Ask the volume, not the process table.
STARTED_HERE=0
if gosu postgres pg_isready -q -h "$PGDATA" -p "$PGPORT" 2>/dev/null; then
  say "postgres is already serving this volume — using it"
else
  # Only now can the pid file be stale: nothing answers on the socket.
  rm -f "$PGDATA/postmaster.pid"
  say "starting postgres on $PGDATA:$PGPORT"
  # `listen_addresses` empty: a unix socket and nothing on the network, which
  # is the property config.py claims for the development database.
  gosu postgres pg_ctl -D "$PGDATA" -l "$PGLOG" -w \
    -o "-p $PGPORT -k $PGDATA -c listen_addresses=''" start >/dev/null
  STARTED_HERE=1
fi

if ! gosu postgres psql -h "$PGDATA" -p "$PGPORT" -U podshl -d postgres -tAc \
     "SELECT 1 FROM pg_database WHERE datname='podshl'" | grep -q 1; then
  say "createdb podshl"
  gosu postgres createdb -h "$PGDATA" -p "$PGPORT" -U podshl podshl
fi

# Stops the listeners by job, not with `kill 0` — the process group includes
# this script, which would take the exit code down with it.
shutdown() {
  trap - TERM INT
  say "stopping"
  for job in $(jobs -p); do kill "$job" 2>/dev/null || true; done
  wait 2>/dev/null || true
  shutdown_store
}
trap 'shutdown; exit 143' TERM INT

# Never stop a cluster this container did not start: another container may be
# serving from the same volume, and taking it down under them is exactly how a
# `run` invocation breaks a running `up`.
shutdown_store() {
  [ "$STARTED_HERE" = "1" ] || return 0
  gosu postgres pg_ctl -D "$PGDATA" -m fast stop >/dev/null 2>&1 || true
}

migrate() {
  say "migrate"
  python -m podshl.server.migrate
}

# The out-of-band key the client verifies the vendor's card against. var/ is
# gitignored, so a fresh clone has none and ten client cases fail on the
# missing file rather than on anything they test.
seed_log() {
  # Three cases ask the client to verify this operator's own proofs, and none
  # of them can hold on an empty log. A database that somebody has been using
  # for a while has entries; a fresh one has none, which is why they passed on
  # developers' machines for months and failed the first time this ran in CI.
  say "seeding the log so the operator's own proofs can be verified"
  python scripts/dev/seed_log.py
}

fixtures() {
  say "trust stub"
  python scripts/dev/make_trust_stub.py
}

# --- the development-only defaults -------------------------------------------
# The log key lives in var/, a bind mount that starts empty on a fresh clone,
# so this container is the one place a missing key may be minted. A production
# host must not: a key created silently on first start forks the log under a
# key nobody pinned. Only set here, and only if the operator did not say
# otherwise.
export PODSHL_LOG_KEY_CREATE="${PODSHL_LOG_KEY_CREATE:-1}"

# The operator's listener refuses everything without a bearer token, and this
# container is the operator's own machine. One is minted per start when none
# was given, and written to var/ so the developer can read it — 0600, on the
# bind mount, like the salts. The suite inherits it through the environment.
if [ -z "${PODSHL_OPS_TOKEN:-}" ]; then
  PODSHL_OPS_TOKEN=$(python -c 'import secrets; print(secrets.token_urlsafe(32))')
  export PODSHL_OPS_TOKEN
  (umask 077 && printf '%s\n' "$PODSHL_OPS_TOKEN" > /app/var/ops.token)
  say "ops token minted for this start — in var/ops.token"
fi

# --- the counterparty --------------------------------------------------------
# Bound to 0.0.0.0 so the published ports reach the host; 127.0.0.1 still
# resolves inside, which is what the crawler cases mean by loopback.
#
# That includes the ops view. compose.yaml publishes it as 127.0.0.1:8726 on
# the host, and Docker's forwarder connects to the container's own address —
# so a listener bound to loopback *inside* the container is unreachable from
# the host, which is the only place a developer on Windows can open it from.
# The host-side mapping is what keeps it off the network here; the token and
# the Host check are what guard it; on a real host the systemd unit binds
# 127.0.0.1 and there is no forwarder.
serve() {  # serve <module:attr> <port>
  uvicorn "$1" --host 0.0.0.0 --port "$2" --log-level warning &
}

counterparty() {
  serve podshl.vendor.app:app                  8721
  serve podshl.index_service.plain_vendor:app  8722
  serve podshl.index_service.app:app           8723
  serve podshl.index_service.oss_project:app   8727
  say "counterparty on 8721 8722 8723 8727"
}

operator() {
  serve podshl.server.app:app      8725
  serve podshl.server.ops_app:ops  8726
  say "operator server on 8725, ops view on 8726"
}

# The suite's own preflight only checks the counterparty, but four SV cases
# reach the operator server on :8725 - so wait for that too, or they fail as
# connection-refused and read like defects.
wait_for_services() {
  for _ in $(seq 1 60); do
    if python - <<'PY' 2>/dev/null
import httpx, sys
for p in (8721, 8722, 8723, 8727, 8725, 8726):
    httpx.get(f"http://127.0.0.1:{p}/", timeout=1)
PY
    then return 0; fi
    sleep 0.5
  done
  echo "services did not come up" >&2
  return 1
}

case "${1:-all}" in
  all)       migrate; fixtures; counterparty; operator; wait ;;
  server)    migrate; operator; wait ;;
  services)  fixtures; counterparty; wait ;;
  testcases) migrate; fixtures; counterparty; operator; wait_for_services; seed_log; rc=0; python run_testcases.py || rc=$?; shutdown; exit $rc ;;
  release)   shutdown_store; exec mise run release ;;
  migrate)   migrate; shutdown ;;
  shell)     exec bash ;;
  *)         exec "$@" ;;
esac
