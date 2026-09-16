#!/usr/bin/env bash
# Assemble the Omarchy plugin's own repository from this one.
#
#   scripts/publish_omarchy_plugin.sh /path/to/omarchy-podshl-checkout
#
# `omarchy plugin add` wants manifest.json at the root of what it clones, so the
# plugin cannot be added from a subdirectory here (measured: "missing
# manifest.json"). Its repository is therefore assembled, never edited: the
# plugin from `omarchy-plugin/`, the licence, and the AUR package's PKGBUILD in
# `package/`, which `install-client.sh` builds while `podshl-bin` is not in the
# AUR. One source for each, so the PKGBUILD the plugin builds is the one the
# AUR would serve.
#
# It only writes files. Committing and pushing stay a person's job.
set -euo pipefail

DEST="${1:?usage: scripts/publish_omarchy_plugin.sh <path to the plugin checkout>}"
cd "$(dirname "$0")/.."
[ -d "$DEST/.git" ] || { echo "$DEST is not a git checkout" >&2; exit 1; }

( cd "$DEST" && git ls-files -z | xargs -0 -r rm -f )
mkdir -p "$DEST/package"
cp omarchy-plugin/manifest.json omarchy-plugin/BarWidget.qml omarchy-plugin/README.md \
   omarchy-plugin/install-client.sh LICENSE "$DEST/"
cp packaging/aur/podshl-bin/PKGBUILD packaging/aur/podshl-bin/podshl-client.desktop "$DEST/package/"
# LF everywhere, whatever the publishing machine's git does: a shell script and a
# PKGBUILD with CRLF fail on the desktop they are for.
printf '* text=auto eol=lf\n' > "$DEST/.gitattributes"

# A copy on Windows carries no executable bit; the script is run with `bash`,
# but a reader who runs it directly should not be told "permission denied".
( cd "$DEST" && git add -A && git update-index --chmod=+x install-client.sh )

# The PKGBUILD must name the version this repository's release carries.
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' client-rs/Cargo.toml | head -1)
pkgver=$(sed -n 's/^pkgver=//p' packaging/aur/podshl-bin/PKGBUILD)
[ "$ver" = "$pkgver" ] || { echo "PKGBUILD is $pkgver, the client is $ver" >&2; exit 1; }

echo "· what the plugin repository would gain or lose"
( cd "$DEST" && git status --short )
echo
echo "Nothing has been committed. Read the diff, then commit and push from $DEST."
