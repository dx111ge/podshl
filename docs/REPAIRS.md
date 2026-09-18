# Recorded local fixes

A local fix outlives its reason. The bug it works around gets fixed upstream, an
update overwrites it, or a cloned plugin keeps running while the official one
moves on, and a year later nobody knows why it is there. The client keeps a
record of such fixes and looks at them again. It **never removes or undoes
anything by itself**: a newer version is a reason to look, not proof that the
fix can go, because a backport or a fix that did not hold looks the same from
here.

The record is `repairs.json` in the client's settings folder
(`%APPDATA%\podshl` on Windows, `~/Library/Application Support/podshl` on
macOS, `~/.config/podshl` on Linux; `VS_ROOT` overrides it). Every field a
decision reads is written by code: versions come from the package manager,
paths are resolved, digests are computed. A `note` is kept and shown, and
nothing reads it.

## What gets recorded

* **Changes the client makes itself.** Every change the window makes is written
  down before it runs, or it does not run.
* **Changes made by something else**, such as an agent, a script or you,
  registered from the command line:

| Kind | What it is | What is kept | Flagged when |
|---|---|---|---|
| `file` | a file edited in place | SHA-256 of the file after the change; with `begin`, a copy from before | the file changed again or is gone; the copy is gone |
| `package` | a package built or pinned locally | the installed version, and the version the package manager offered then | the official package moved on (the local one is frozen), or it was updated |
| `overlay` | a local copy standing in front of a component (a cloned plugin, a patched script) | digest of the copy; digest of the original path, or the original package's version | what the copy overrides has changed; the copy changed |

Any record can name an upstream issue or pull request and the version that
fixes it (`--fixed-in`). Once that version is installed, the record is flagged
with "may no longer be needed, or the fix did not hold".

## Watching an upstream issue on GitHub

**Off unless you switch it on, per record** (`--watch`, `repairs watch ID on`,
or the button in the window). A lookup tells GitHub, from your address, which
issue this computer follows, which is why it is opt-in. It goes straight to
GitHub's public API, without an account or token, and never through the
operator. A watched record is asked at most once a day, and never again once
the answer is final (an issue closed, a fix found in a release).

* An issue: open or closed.
* A pull request: open, closed or merged. If merged, the client finds **the
  first release that contains the merge commit** by asking GitHub, for each
  published release after the merge (at most eight, drafts and pre-releases
  skipped), whether the commit is part of it. That release is then compared
  with the installed version like `--fixed-in`.

Only `https://github.com/<owner>/<repo>/issues/<n>` and `…/pull/<n>` links can
be watched.

## Looking again

* **In the window**: at every start, a panel lists records that want another
  look, with **Keep it**, **Undo** (where a copy exists) and **Watch the
  upstream issue**. Keeping a record hides what you saw until something new
  happens. The three buttons are reachable by keyboard, and walked that way on
  Windows (WebView2) and on Omarchy (Wayland/WebKitGTK).
* **After updates or daily**: `podshl-client repairs install-hook` sets up a
  review that shows a desktop notification when something wants a look.
  `--print` shows what it would do without doing it, and `remove-hook` undoes
  it.

| System | What `install-hook` sets up | Tested |
|---|---|---|
| Omarchy | `~/.config/omarchy/hooks/post-update.d/podshl-repairs`, run by Omarchy after every update | **on a real Omarchy desktop, 2026-09-18**: installed (chosen over systemd because `~/.config/omarchy` exists), a real `omarchy-update` ran it, the notification reached the desktop, the update finished with exit code 0, `remove-hook` took the file and left the other hooks alone |
| Other Linux | systemd user service and timer `podshl-repairs` (daily, `Persistent=true`), notification via `notify-send` | **on a live systemd user session, 2026-09-18**: plain Debian with systemd as pid 1 and a lingering user (`docker/systemd-walk.Dockerfile`). The units are accepted, the timer is enabled and scheduled, a review that found something exits 3 and systemd records `Result=success` rather than a failed unit, a missing notification daemon does not break the run, and `remove-hook` leaves no timer and no unit files |
| Windows | scheduled task `PODSHL\Repairs review`, daily at 12:00, without administrator rights; Windows notification | **on Windows 11, 2026-09-18**: created, run (`schtasks /Run`, result 3), and the toast confirmed in the notification database under PODSHL's own name; removed, and removed again quietly |
| macOS | `~/Library/LaunchAgents/de.podshl.repairs.plist`, at login and daily at 12:00; notification via `osascript` | **not tested**: no Mac available |

The review changes nothing, and only asks the network about issues you chose to
watch.

The notification is PODSHL's own: on Windows it is raised under the identity
`de.podshl.client`, so it says PODSHL in the notification and in your
notification settings rather than the name of whatever raised it. Uninstalling
removes that identity and the daily task along with the program, whether or not
you ran `remove-hook` first.

**What it did is written down.** Every command that changes something adds a
line to the client log — what was recorded, what a review concluded and which
flags it raised, what was restored, what the hook set up or took away. The one
lookup that leaves your machine, the GitHub check for a watched issue, is
logged with what it answered, so a check made while you were not at the window
is not invisible afterwards. The log is anonymised the same way a report is,
and `PODSHL_LOG=0` turns it off.

On Linux without a systemd user session (a container, a plain SSH login),
`install-hook` stops with the error `systemctl` gave and exit code 1; the unit
files it wrote are removed by `remove-hook`.

## Command line

```
podshl-client repairs review [--json] [--notify] [--offline]
podshl-client repairs list [--json]
podshl-client repairs add --kind file|package|overlay --by NAME [--path P]
      [--package NAME] [--original P] [--original-package NAME]
      [--issue URL] [--fixed-in V] [--watch] [--note TEXT]
podshl-client repairs begin --kind file --by NAME --path P [same options]
podshl-client repairs done ID
podshl-client repairs keep ID
podshl-client repairs watch ID on|off
podshl-client repairs restore ID
podshl-client repairs forget ID
podshl-client repairs install-hook [--print]
podshl-client repairs remove-hook [--print]
```

Exit codes: `0` nothing to look at, `3` something to look at, `1` error. `review
--offline` never asks GitHub.

On Windows the installed client is a window program. It prints into the
terminal it was started from, but `cmd` and PowerShell do not wait for a window
program, so a script that needs the exit code starts it with
`Start-Process podshl-client -ArgumentList 'repairs','review' -Wait -PassThru`
and reads `ExitCode`.

**For an agent or a script that changes a file**, the order is:

```
id=$(podshl-client repairs begin --kind file --by my-agent --path ~/.config/app/app.conf \
      --issue https://github.com/owner/app/issues/42)
# ... change the file ...
podshl-client repairs done "$id"
```

`begin` keeps a copy before the change, so `restore` (or Undo in the window)
can put it back. A change already made is registered with `add`; it has no copy
to go back to.

## Removing a record

`repairs forget ID` removes what a record said — the path, the digest, who made
it, the note, the upstream issue — and deletes the copy it kept. What stays is a
stub: that a record existed, when the change was recorded, what kind it was, and
when it was forgotten. A ledger with a hole in it should not read like a ledger
that never had the entry.

It is deliberately awkward. **It needs administrator or root rights**, and that
is not about file permissions — `repairs.json` is in your own configuration
directory and you can edit it by hand. It is about who *cannot*: the agents,
skills and scripts this record exists to keep track of run as you, so a removal
an ordinary process can perform is a removal the thing being recorded can
perform. **And it needs you to type the record's id** at a real terminal; a
pipe is not a person, and no option skips either gate. There is no way to
remove a record from the window, because the window does not run as root.

The removal is written to the client log. After it, that is the only place that
still says what the record held.

A package pinned locally, and a cloned plugin in front of the packaged one:

```
podshl-client repairs add --kind package --by me --package hyprland \
      --issue https://github.com/hyprwm/Hyprland/pull/1234 --watch
podshl-client repairs add --kind overlay --by my-agent --path ~/.local/share/app/plugins/foo \
      --original /usr/share/app/plugins/foo --original-package app-plugin-foo
```

Package versions are read with `pacman` (Arch, Omarchy), `dpkg-query` and
`apt-cache` (Debian, Ubuntu), `brew` (macOS), and on Windows from the
programs' uninstall entries (the installed version only; Windows has no
package manager to ask for a newer one).
