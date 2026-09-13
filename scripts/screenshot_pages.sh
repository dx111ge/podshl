#!/usr/bin/env bash
# Photograph the operator's pages on a virtual screen.
#
# These pages carry a Content-Security-Policy whose `script-src` is a hash per
# inline block. A wrong hash does not error: the browser blocks the script and
# the page comes up with its layout drawn and nothing filled in — the same
# silent, total failure a syntax error caused in the client's window, which
# stayed green through 201 cases because every check read the file as text.
#
# So looking is not optional here, and it must not need a desktop.
#
#   scripts/screenshot_pages.sh var/shots            # all of them
#   scripts/screenshot_pages.sh var/shots /projects  # one
set -euo pipefail

OUT="${1:-var/shots}"
BASE="${BASE:-http://127.0.0.1:8725}"
SIZE="${SIZE:-1100x900}"
shift || true
PAGES=("$@")
if [ ${#PAGES[@]} -eq 0 ]; then
  PAGES=(/ /publish /register /dashboard /security /projects /log /notice /imprint)
fi

BROWSER=""
for candidate in chromium chromium-browser google-chrome; do
  command -v "$candidate" >/dev/null 2>&1 && { BROWSER="$candidate"; break; }
done
[ -n "$BROWSER" ] || { echo "no chromium on PATH — a page can only be checked by rendering it" >&2; exit 1; }

mkdir -p "$OUT"
export DISPLAY=:99
Xvfb :99 -screen 0 "${SIZE}x24" >/dev/null 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null || true' EXIT
for _ in $(seq 1 40); do xdpyinfo -display :99 >/dev/null 2>&1 && break; sleep 0.25; done

for path in "${PAGES[@]}"; do
  name="$(echo "$path" | sed 's|^/||; s|/|-|g')"
  [ -n "$name" ] || name="home"
  # --virtual-time-budget lets the page's fetches settle before the shot, which
  # is the whole point: an empty table means the script never ran.
  "$BROWSER" --headless --disable-gpu --no-sandbox --hide-scrollbars \
    --window-size="${SIZE/x/,}" \
    --virtual-time-budget=4000 \
    --screenshot="$OUT/$name.png" "$BASE$path" >/dev/null 2>&1 || true
  printf '  %s\n' "$OUT/$name.png"
done
