# The record of local fixes, on a clean Omarchy

[`podshl-repairs.mp4`](podshl-repairs.mp4), English subtitles in a band of
their own under the picture. There is deliberately no `.srt` beside it: a
player that finds one with the same name shows the text twice.

One continuous take on a real Omarchy desktop, recorded on 2026-09-19 with
`scripts/walk/omarchy/take-repairs.sh` and nothing edited afterwards. Every
command is the real one and ran; every step that the subtitles describe was
checked by the script as it happened, and the take would have stopped at the
first that did not.

| | What happens | What it shows |
|---|---|---|
| 1 | one line: `install.sh` finds the 0.1.8 release, checks `podshl-repairs` and its man pages against `SHA256SUMS`, installs them into `~/.local/bin` and `~/.local/share/man`, and sets up the update hook and the agent hook | no root, no package; the change to Claude's settings is itself a record |
| 2 | `claude`, and a request typed in plain words: make the window gaps smaller. Claude changes the file with a shell command, as Omarchy's instructions for agents say | the record appears with nobody writing it, by `claude`, with a copy from before |
| 3 | `restore` | the file is back byte for byte |
| 4 | Claude dims unfocused windows; `omarchy-refresh-config` puts Omarchy's default back; the post-update hook runs | the notice stays until clicked; the click opens the review in a terminal; Undo brings back the rounded corners the reset took |
| 5 | PODSHL's client built from its PKGBUILD; the AUR step of `omarchy-update` looks `podshl-bin` up in the AUR, where the name is free | the package declares that itself: the record is there, attributed by pacman, with its maintainer's reason (`show`) |
| 6 | an older workaround recorded against a Hyprland issue, watched | GitHub is asked because the record says so; the issue is closed, and nothing is removed |
| 7 | Claude edits a file inside a git repository | not recorded: it has a history already |

It ran against the 0.1.8 files as published, from GitHub. The rehearsals
before it stopped on real things, and each is described in the script. One was
in the product: Claude wrote the change with `cp` and `cat >>` instead of its
file tools, and the hook, which then watched only the file tools, recorded
nothing. Since 0.1.8 it records what a shell command names (`RR28`).

The screen is 3840×1080; the video is its middle, panning to the top right
corner while the notice is the thing to look at (`enc-repairs.sh`).
