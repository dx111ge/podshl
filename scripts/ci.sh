#!/usr/bin/env bash
# Everything that must pass before a change is pushed, in one command.
#
# **The same command locally and in CI.** `RELEASING.md` and the handover both
# ask for that, and the reason is a day of 2026-09-15: two fixes went in half
# done — a library that grew an option its only caller could not pass, and a
# release task that copied every version it found — and both were caught by
# hand, once minutes before publishing. A gate that only exists on a server is
# a gate nobody runs before pushing; a gate that only exists on a laptop is one
# CI cannot enforce. This is both.
#
#   scripts/ci.sh                      # against the default operator
#   scripts/ci.sh sdota.de https://www.sdota.de
#
# The second argument is the address to compile in when the name does not
# resolve the same everywhere — see `build_client.sh`.
#
# What it gates on:
#
#   * the client builds for a real operator, and the operator and log key are
#     actually in the binary — checked by asking the binary and by fetching the
#     operator's signed index with the key that was compiled in
#   * every Rust test, through `RS1`
#   * every case in `run_testcases.py`, which includes the window's contract,
#     the builder's own writer run in node, and the publication scan
#
# What it reports and does not gate on: `cargo fmt` and `cargo clippy`. There
# are 588 formatting differences and 23 clippy warnings in this tree today.
# Turning either into a gate would make CI red on its first run and every run
# after it, and a suite that is always red teaches people to read past red —
# which is the failure this file exists to prevent, not one to introduce.
set -euo pipefail

cd "$(dirname "$0")/.."

OPERATOR="${1:-sdota.de}"
SERVER_URL="${2:-}"

say() { printf '\n\033[1m· %s\033[0m\n' "$1"; }

say "building the image"
docker compose build podshl

say "building a client for $OPERATOR, and checking what landed in it"
# `--no-deps`: this needs no database, and starting one here only makes the
# failure modes wider.
docker compose run --rm --no-deps --entrypoint sh podshl \
  -c "cd /app && ./scripts/build_client.sh '$OPERATOR' '$SERVER_URL'"

say "the suite"
docker compose run --rm podshl testcases

say "the flow, headless, against the real binary"
# **The one piece of coverage that needed neither a desktop nor Windows.** The
# window test beside it drives WebView2 over the DevTools protocol, which is
# Windows only; this serves the same page to a headless browser under the
# application's own policy and bridges `invoke` to the same Rust. It needs a
# client built with `--features uitest`, which a release must never be, so it
# is built separately and under its own name.
#
# **And into its own target directory.** Both builds write
# `release/podshl-client`; building the test client over the release one
# replaces the artefact that was just checked for *not* having the test
# surface, and the next run then checks what the previous run left behind.
# That happened, and the check caught it — which is the check doing its job
# and not a reason to weaken it. Two builds, two outputs.
# Exported inside the command rather than passed with `-e`: on a Windows shell
# MSYS rewrites anything that looks like an absolute path, so the container was
# handed `C:/Program Files/Git/cargo-target-uitest` — which cargo then tried to
# join into `LD_LIBRARY_PATH` and refused, because of the colon in it.
docker compose run --rm --no-deps --entrypoint sh podshl \
  -c "set -e
      export CARGO_TARGET_DIR=/cargo-target-uitest
      cd /app
      ./scripts/build_client.sh --uitest '$OPERATOR' '$SERVER_URL' >/dev/null
      cd client-rs/uitest
      # Exported, not merely assigned. A line of bare assignments with no
      # command sets shell variables, and \`node\` is a separate command that
      # never sees them — which read as 'no client to drive' about a client
      # that had just been built.
      export PODSHL_CLIENT=\$HOME/.local/bin/podshl-client-uitest
      export UITEST_CHROME=\$(command -v chromium || command -v chromium-browser)
      WS=''
      node -e 'process.exit(typeof WebSocket===\"function\"?0:1)' || WS=--experimental-websocket
      node \$WS --test 'tests/flow-headless.test.mjs'"

say "format and lint, reported"
# The tools have to be there. This step once printed "0 places differ" for a
# tree with 588 of them: `cargo fmt` was not installed, it errored, the grep
# matched nothing and `|| true` made that a number. A report nobody can
# distinguish from a clean result is worse than no report.
docker compose run --rm --no-deps --entrypoint sh podshl -c '
  set -e
  cd /app/client-rs
  cargo fmt --version >/dev/null 2>&1 || { echo "rustfmt is not in this image" >&2; exit 1; }
  cargo clippy --version >/dev/null 2>&1 || { echo "clippy is not in this image" >&2; exit 1; }
  fmt=$(cargo fmt --check 2>/dev/null | grep -c "^Diff in" || true)
  warn=$(cargo clippy --quiet --all-targets 2>&1 | grep -c "^warning" || true)
  echo "  cargo fmt:    $fmt places differ"
  echo "  cargo clippy: $warn warnings"
  echo "  Neither gates. See the header of scripts/ci.sh for why."
'

say "green"
