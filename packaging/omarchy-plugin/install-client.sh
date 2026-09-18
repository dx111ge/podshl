#!/bin/bash
# Install the PODSHL client as the pacman package `podshl-bin`.
#
# Run by the bar icon, in Omarchy's floating terminal, after the person pressed
# Install on a panel that named this file. Nothing else in the plugin runs it.
#
# From the PKGBUILD that came with this plugin, in `package/`: the same file the
# AUR would serve. makepkg downloads the released binary, checks it against the
# sums in that PKGBUILD, and pacman installs it, so `pacman -R podshl-bin`
# removes it like any other package. makepkg asks for the password here, in the
# terminal.
#
# It used to ask the AUR first and fall back to this. `podshl-bin` is not in the
# AUR and cannot be: registration there has been paused since before the first
# release. So the question had one answer, and asking it meant a branch nobody
# could reach and nobody could test. When registration reopens this is the line
# to change back.
set -uo pipefail

pkg=podshl-bin
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing PODSHL ($pkg)"

echo "Building the PKGBUILD that came with this plugin:"
echo "  $here/package/PKGBUILD"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp "$here/package/PKGBUILD" "$here/package/podshl-client.desktop" "$work/" || exit 1
(cd "$work" && makepkg --syncdeps --install --needed --noconfirm) || exit 1

# makepkg can end without an error and without the package.
pacman -Q "$pkg" >/dev/null 2>&1 || { echo "$pkg is not installed." >&2; exit 1; }
