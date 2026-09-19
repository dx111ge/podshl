#!/bin/bash
# Install the PODSHL client as the pacman package `podshl-bin`.
#
# Run by the bar icon, in Omarchy's floating terminal, after the person pressed
# Install on a panel that named this file. Nothing else in the plugin runs it.
#
# From the PKGBUILD that came with this plugin, in `package/`. makepkg downloads
# the released binary, checks it against the sums in that file, and pacman
# installs it, so `pacman -R podshl-bin` removes it like any other package.
# makepkg asks for your password here, in the terminal.
#
# Not from the AUR: registration there is closed at the moment, so `podshl-bin`
# is not published there and this is the only way in.
set -uo pipefail

pkg=podshl-bin
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing PODSHL ($pkg)"

echo "Building the PKGBUILD that came with this plugin:"
echo "  $here/package/PKGBUILD"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp "$here/package/PKGBUILD" "$here/package/podshl-client.desktop" "$here/package/podshl-bin.repairs.json" "$work/" || exit 1
(cd "$work" && makepkg --syncdeps --install --needed --noconfirm) || exit 1

# makepkg can end without an error and without the package.
pacman -Q "$pkg" >/dev/null 2>&1 || { echo "$pkg is not installed." >&2; exit 1; }
