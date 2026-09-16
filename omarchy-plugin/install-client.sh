#!/bin/bash
# Install the PODSHL client as the pacman package `podshl-bin`.
#
# Run by the bar icon, in Omarchy's floating terminal, after the person pressed
# Install on a panel that named this file. Nothing else in the plugin runs it.
#
#   * From the AUR when `podshl-bin` is there, the way Omarchy installs any AUR
#     package (`omarchy-pkg-aur-add`, which is yay).
#   * Until it is, from the PKGBUILD that came with this plugin, in `package/`:
#     the same file the AUR would serve. makepkg downloads the released binary,
#     checks it against the sums in that PKGBUILD, and pacman installs it, so
#     `pacman -R podshl-bin` removes it like any other package.
#
# Either way the package manager asks for the password here, in the terminal.
set -uo pipefail

pkg=podshl-bin
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing PODSHL ($pkg)"

if yay -Si --aur "$pkg" >/dev/null 2>&1; then
  echo "From the AUR."
  omarchy-pkg-aur-add "$pkg" || exit 1
else
  echo "$pkg is not in the AUR yet. Building the PKGBUILD that came with this plugin:"
  echo "  $here/package/PKGBUILD"
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
  cp "$here/package/PKGBUILD" "$here/package/podshl-client.desktop" "$work/" || exit 1
  (cd "$work" && makepkg --syncdeps --install --needed --noconfirm) || exit 1
fi

# makepkg and yay can both end without an error and without the package.
pacman -Q "$pkg" >/dev/null 2>&1 || { echo "$pkg is not installed." >&2; exit 1; }
