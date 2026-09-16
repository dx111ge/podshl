# PODSHL for Omarchy

One bar icon. Click it and the PODSHL client asks the projects you actually run
what is wrong with your machine — and asks you before it reads anything.
Right-click opens the project page.

## Install

    omarchy plugin add https://github.com/dx111ge/omarchy-podshl --enable

Then click the icon. **The first click installs the client** if it is not on
this machine yet:

1. a panel under the icon says the client is missing and shows what it would
   run: `bash ~/.config/omarchy/plugins/podshl.diagnose/install-client.sh`,
   a short script that came with this plugin — read it first if you like;
2. **Install** opens Omarchy's floating terminal on that script. It installs
   the pacman package `podshl-bin`: from the AUR when the package is there,
   and until then from the same PKGBUILD, shipped in `package/`, built with
   `makepkg`, which checks the downloaded release against the sums in it. The
   package manager asks for your password there;
3. when it succeeds, PODSHL starts. When it does not, the terminal says
   *PODSHL was not installed* above Omarchy's closing "Done!" prompt, which
   Omarchy shows whatever happened.

**Not now** or Escape closes the panel and nothing runs. The keyboard works too:
left and right choose, Enter confirms.

### Why the first click, and not the plugin install

`omarchy plugin add` clones files and runs nothing — no install hook, no sudo,
by design ("It never runs anything from the plugin, never executes an install
hook, and never asks for sudo", Omarchy manual, *Shell Plugins*). A plugin
therefore cannot bring a package with it. What it can do is offer the
installation the first time it is needed, in the open, with the command shown
before anything runs. That is what this one does.

### The package

`podshl-bin` is the released Linux binary from this project's GitHub release,
with its desktop entry, icons and licence. Its PKGBUILD lives in the main
repository under `packaging/aur/podshl-bin/` and is copied into `package/` here
on every release. The binary is built for the operator at https://sdota.de,
with the public key of its transparency log compiled in.

It is **not in the AUR yet**: registration there is paused, so there is no
account to publish it with. Once it is, `yay -S podshl-bin` works on its own,
and the script above takes that path by itself. Until then, without the plugin:

    cp -r ~/.config/omarchy/plugins/podshl.diagnose/package /tmp/podshl-bin
    cd /tmp/podshl-bin && makepkg -si

In a copy, because building inside the plugin's checkout would leave build files
in a git repository that `omarchy plugin update` pulls into.

## Keyboard

    omarchy-shell shell summon podshl.diagnose

does what a click does, so a Hyprland binding can start a diagnosis — or offer
the installation — without the mouse.

## Where to look, because the icon is small

It is **one monochrome bug glyph in a 26-pixel bar**, and on a wide screen that
is genuinely hard to spot. It lands in the **centre** section by default, which
on most setups puts it right of the clock. If you want it somewhere you will
actually notice, move it:

    omarchy bar move podshl.diagnose --section right --index 0
    omarchy bar move podshl.diagnose --after omarchy.clock

## Updating, and one thing Omarchy 4.0.4 does not do

    omarchy plugin update podshl.diagnose
    omarchy restart shell

**The restart is needed.** Measured on Omarchy 4.0.4: after an update, and after
editing the plugin's files, the shell logs *"Local plugin changed, reloading"*
and keeps running the previous version of the widget until it is restarted.

## Removing it

    omarchy plugin remove podshl.diagnose
    omarchy pkg drop podshl-bin

The first removes the icon, the second the client. The client keeps its settings
in `~/.config/podshl`, its log in `~/.local/state/podshl` and its window data in
`~/.local/share/de.podshl.client`; delete those as well for nothing to remain.

## What this plugin does, and what it deliberately does not

It starts a program, and offers to install that program when it is missing.
That is all of it.

Plugins **run unsandboxed** inside the Omarchy shell, and this one is for a
product whose entire argument is bounded effect. So it reads no files, runs no
diagnosis, makes no network call and keeps no state. The one thing it runs on
its own is `command -v podshl-client`, at the moment you click, to decide
between starting the client and offering it. Everything that touches your
machine lives in the client, behind a consent panel that shows each reading
before it is taken and an anonymiser that strips paths, accounts and addresses
before anything travels.

## What the client is for

A project publishes three files on a host it controls: a challenge, a manifest
of what it needs to know, and its answers. Your machine reads only what that
project asked for, one consent at a time, and matches locally.

From 0.1.5 on, the client uses **the desktop's default agent** as its model when
you have not chosen another one — `~/.config/omarchy/defaults/agent`, called
headless with its file, shell and web tools denied, from an empty directory,
leaving no session behind. Only Claude Code has been measured for that so far.
It is a cloud service, and every panel that is about to hand it readings says
so.

When nothing published covers your problem, you get the report you would have
had to assemble by hand — anonymised, with measured and supplied facts kept
apart — to paste into the project's issue tracker.

## Licence

AGPL-3.0-or-later, the same as the client.
