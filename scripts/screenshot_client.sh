#!/usr/bin/env bash
# Open the client on a virtual screen and photograph it.
#
# The window is the largest untested surface in this project: every panel is
# covered by a contract test and, for a long time, none had been looked at. A
# screenshot needs a display, a desktop and somebody's attention — so it never
# happened. This needs none of those.
#
#   scripts/screenshot_client.sh var/shots/start.png
set -euo pipefail

OUT="${1:-var/shots/client.png}"
SIZE="${SIZE:-1000x820}"
WAIT="${WAIT:-8}"

mkdir -p "$(dirname "$OUT")"
BIN="${CARGO_TARGET_DIR:-client-rs/target}/release/podshl-client"
[ -x "$BIN" ] || { echo "no binary at $BIN — build it first" >&2; exit 1; }

export DISPLAY=:99
Xvfb :99 -screen 0 "${SIZE}x24" >/dev/null 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null || true' EXIT
for _ in $(seq 1 40); do xdpyinfo -display :99 >/dev/null 2>&1 && break; sleep 0.25; done

# The DMABUF workaround is the binary's own (`L2`) and is not set here. Note
# what this script cannot show: Xvfb is X11, so the failure L2 is about cannot
# happen under it, and every screenshot ever taken here ran past it.
VS_ROOT=../var VS_TRUST=../var/ans_stub.json VS_LOG_KEY=../var/log_key.json \
  bash -c "cd client-rs && exec '$BIN'" &
APP=$!
trap 'kill $APP 2>/dev/null || true; kill $XVFB 2>/dev/null || true' EXIT

sleep "$WAIT"
import -display :99 -window root "$OUT"
echo "  $OUT"
