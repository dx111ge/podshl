#!/usr/bin/env bash
# Install podshl-repairs — the record of local fixes — into ~/.local/bin.
#
#   curl -fsSL https://raw.githubusercontent.com/dx111ge/podshl/main/packaging/repairs/install.sh | bash
#
# What it does, in order, and says as it goes:
#   1. finds the newest release
#   2. downloads podshl-repairs and the release's SHA256SUMS
#   3. checks the one against the other, and installs nothing if they differ
#   4. puts the program into ~/.local/bin — no root, no package, so no package
#      manager and no AUR helper is ever asked about it
#   5. sets up a review after every update (Omarchy) or once a day, and on
#      Omarchy the hook that records what your default agent writes
#
# Nothing here needs root, and nothing outside your home directory changes.
# Taking it out again is printed at the end.
#
# Overrides, for testing and for mirrors:
#   PODSHL_RELEASES      where releases are, default https://github.com/dx111ge/podshl/releases
#   PODSHL_VERSION       a version instead of the newest, e.g. 0.1.7
#   PODSHL_INSTALL_DIR   instead of ~/.local/bin
#   PODSHL_MAN_DIR       instead of ~/.local/share/man
#   PODSHL_NO_HOOKS=1    install the program and set up nothing else
set -euo pipefail

RELEASES=${PODSHL_RELEASES:-https://github.com/dx111ge/podshl/releases}
DEST=${PODSHL_INSTALL_DIR:-$HOME/.local/bin}
MAN=${PODSHL_MAN_DIR:-$HOME/.local/share/man}

say()  { printf '  %s\n' "$*"; }
stop() { printf 'podshl-repairs was not installed: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Linux ] || stop "this installer is for Linux; see docs/REPAIRS.md"
[ "$(uname -m)" = x86_64 ] || stop "there is no build for $(uname -m) yet"
for tool in curl sha256sum install; do
  command -v "$tool" >/dev/null || stop "$tool is not installed"
done

# The newest release is where GitHub's /latest redirects to: .../tag/v<version>.
if [ -n "${PODSHL_VERSION:-}" ]; then
  V=$PODSHL_VERSION
else
  tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$RELEASES/latest") \
    || stop "could not reach $RELEASES"
  V=${tag##*/v}
  case $V in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) stop "could not tell the newest version from $tag" ;;
  esac
fi
BIN=podshl-repairs-$V-linux-x86_64

echo "podshl-repairs $V"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
say "downloading $BIN and SHA256SUMS"
curl -fsSL -o "$tmp/$BIN" "$RELEASES/download/v$V/$BIN" || stop "could not download $BIN"
curl -fsSL -o "$tmp/SHA256SUMS" "$RELEASES/download/v$V/SHA256SUMS" || stop "could not download SHA256SUMS"

# The sums have to name this file: `--ignore-missing` on its own passes a sums
# file that does not mention it at all on older coreutils.
grep -q "  $BIN\$" "$tmp/SHA256SUMS" || stop "SHA256SUMS does not list $BIN"
(cd "$tmp" && sha256sum -c --ignore-missing --status SHA256SUMS) \
  || stop "$BIN does not match the release's SHA256SUMS — nothing was installed"
say "checked against SHA256SUMS"

install -Dm755 "$tmp/$BIN" "$DEST/podshl-repairs"
say "installed $DEST/podshl-repairs"

# The man pages, when the release has them, checked the same way. man finds
# ~/.local/share/man by itself for a program in ~/.local/bin.
pages=0
for page in podshl-repairs.1 podshl-repairs.d.5; do
  grep -q "  $page\$" "$tmp/SHA256SUMS" || continue
  curl -fsSL -o "$tmp/$page" "$RELEASES/download/v$V/$page" || stop "could not download $page"
  (cd "$tmp" && grep "  $page\$" SHA256SUMS | sha256sum -c --status) \
    || stop "$page does not match the release's SHA256SUMS"
  install -Dm644 "$tmp/$page" "$MAN/man${page##*.}/$page"
  pages=$((pages + 1))
done
[ "$pages" -gt 0 ] && say "man podshl-repairs, man podshl-repairs.d"
R=$DEST/podshl-repairs

if [ "${PODSHL_NO_HOOKS:-}" != 1 ]; then
  "$R" install-hook >/dev/null && say "a review after every update (or daily): podshl-repairs install-hook"
  # Only where there is a default agent to take: Omarchy's. Anywhere else the
  # agent has to be named, and guessing one is not this script's to do.
  if command -v omarchy-default-agent >/dev/null 2>&1; then
    if out=$("$R" install-agent-hook 2>&1); then
      say "what your default agent writes is recorded: podshl-repairs install-agent-hook"
    else
      # Every line of it: only Claude Code's hook format has been walked, and
      # what somebody with another agent can do instead is the rest of them.
      say "the agent hook is not set up:"
      echo "$out" | while IFS= read -r line; do say "  $line"; done
    fi
  fi
fi

case ":$PATH:" in
  *":$DEST:"*) ;;
  *) say "note: $DEST is not on your PATH; add it, or call $R directly" ;;
esac
echo
echo "Done. See what is recorded: podshl-repairs list"
echo "Take it out: podshl-repairs remove-agent-hook; podshl-repairs remove-hook; rm $R"
