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
| 1 | `podshl-repairs` downloaded from the 0.1.7 release, checked against `SHA256SUMS`, installed into `~/.local/bin`; the update hook and the agent hook installed | no root, no package; the change to Claude's settings is itself a record |
| 2 | Claude, Omarchy's default agent here, is asked to make the window gaps smaller | the record appears with nobody writing it, by `claude`, with a copy from before |
| 3 | `restore` | the file is back byte for byte |
| 4 | Claude dims unfocused windows; `omarchy-refresh-config` puts Omarchy's default back; the post-update hook runs | the notice stays until clicked; the click opens the review in a terminal; Undo brings back the rounded corners the reset took |
| 5 | PODSHL's client built from its PKGBUILD; the AUR step of `omarchy-update` looks `podshl-bin` up in the AUR, where the name is free | a local pacman repository as the workaround, recorded with `begin`/`done` and its reason; after it nothing is looked up |
| 6 | an older workaround recorded against a Hyprland issue, watched | GitHub is asked because the record says so; the issue is closed, and nothing is removed |
| 7 | Claude edits a file inside a git repository | not recorded: it has a history already |

It ran against the 0.1.7 files as published, from GitHub. Four rehearsals came
before it on the same machine, each stopped by something real — characters lost
by the typing tool, a frame rate too low to read, a missing `OMARCHY_PATH`, and
a check fooled by commented examples — and each is described in the script.

The screen is 3840×1080; the video is its middle, panning to the top right
corner while the notice is the thing to look at (`enc-repairs.sh`).
