# The record of local fixes

A local fix outlives its reason. The bug it works around gets fixed upstream, an
update overwrites it, or a cloned plugin keeps running while the official one
moves on, and a year later nobody knows why it is there.

`podshl-repairs` keeps a record of such changes — made by you, by a script, by a
coding agent, or declared by the packages and plugins that make them — and
looks at them again after every update. It **never removes or undoes anything
by itself**: a newer version is a reason to look, not proof that a fix can go,
because a backport and a fix that did not hold look the same from here.

**Watch it:** [`examples/repairs/podshl-repairs.mp4`](../examples/repairs/podshl-repairs.mp4)
— installed with one line on a clean Omarchy, a change asked of Claude recorded
without anybody writing it, undone, a kept change overwritten by Omarchy and
noticed, and what a package declares about itself. What each part shows is in
[`examples/repairs/README.md`](../examples/repairs/README.md).

The same record is also part of the PODSHL client (`podshl-client repairs …`,
and a panel in its window); everything here applies to both.

---

- [Install](#install)
- [First steps](#first-steps)
- [Where records come from](#where-records-come-from)
  - [You, or a script](#you-or-a-script)
  - [A coding agent](#a-coding-agent)
  - [Packages and plugins](#packages-and-plugins)
  - [The PODSHL client](#the-podshl-client)
- [Looking again](#looking-again)
- [What each finding means](#what-each-finding-means)
- [Deciding](#deciding)
- [After updates, or daily](#after-updates-or-daily)
- [Watching an upstream issue](#watching-an-upstream-issue)
- [What leaves this machine](#what-leaves-this-machine)
- [Removing a record](#removing-a-record)
- [Command reference](#command-reference)
- [Files and environment](#files-and-environment)
- [Troubleshooting](#troubleshooting)
- [How this is tested](#how-this-is-tested)

---

## Install

**Linux, Omarchy first.** One line installs the program and sets up its hooks:

```
curl -fsSL https://raw.githubusercontent.com/dx111ge/podshl/main/packaging/repairs/install.sh | bash
```

**GLIBC 2.39 or newer** — the published build is made on Debian trixie. Debian
13, Ubuntu 24.04, Arch and Omarchy are new enough; Debian 12 and Ubuntu 22.04
are not, and the installer says so and stops rather than letting the loader
report `version GLIBC_2.39 not found` after a download it just said was checked.
On an older system, build it: `cargo build --release -p podshl-client --bin
podshl-repairs`.

[`packaging/repairs/install.sh`](../packaging/repairs/install.sh):

1. finds the newest release;
2. downloads `podshl-repairs`, its man pages and the release's `SHA256SUMS`;
3. **installs nothing unless each file matches its sum** (`RR23`);
4. puts the program into `~/.local/bin` and the man pages into
   `~/.local/share/man` — no root, no package, so no package manager and no AUR
   helper is ever asked about it;
5. sets up the review after every update (Omarchy) or once a day (elsewhere),
   and on Omarchy the hook that records what your default agent writes.

It says each step as it does it. Read it first if you would rather: it is
short. Then `man podshl-repairs`.

**By hand**, without piping anything into a shell:

```
cd "$(mktemp -d)"
curl -fLO https://github.com/dx111ge/podshl/releases/download/v0.1.8/podshl-repairs-0.1.8-linux-x86_64
curl -fLO https://github.com/dx111ge/podshl/releases/download/v0.1.8/SHA256SUMS
sha256sum -c --ignore-missing SHA256SUMS
install -Dm755 podshl-repairs-0.1.8-linux-x86_64 ~/.local/bin/podshl-repairs
podshl-repairs install-hook
podshl-repairs install-agent-hook    # Omarchy: your default agent
```

**Taking it out again:**

```
podshl-repairs remove-agent-hook
podshl-repairs remove-hook
rm ~/.local/bin/podshl-repairs ~/.local/share/man/man1/podshl-repairs.1 ~/.local/share/man/man5/podshl-repairs.d.5
```

The record itself stays in `~/.config/podshl` until you remove that too.

**Other systems.** On Windows and macOS the record is part of the PODSHL
client ([INSTALL.md](INSTALL.md)); `podshl-repairs` on its own is offered on
Linux first. On Omarchy one agent is the defined one, so what it writes can be
recorded through its hooks and walked end to end; Windows has no such default,
and the standalone record comes there later, as a whole. The build already
writes a console `podshl-repairs.exe` (`RR18` checks it is one); nothing
installs it yet.

## First steps

```
$ podshl-repairs review
Nothing recorded wants another look.

$ id=$(podshl-repairs begin --kind file --by me --path ~/.config/app/app.conf)
$ $EDITOR ~/.config/app/app.conf
$ podshl-repairs done "$id"

$ podshl-repairs list
[1789821805-2] file — ~/.config/app/app.conf (me) — applied

$ podshl-repairs show "$id"
[1789821805-2] file — ~/.config/app/app.conf (me)
  state: applied
  copy to go back to: ~/.config/podshl/backups/1789821805-2/app.conf
  recorded: 2026-09-19

$ podshl-repairs restore "$id"
Put back: ~/.config/app/app.conf
```

`begin` keeps a copy before you change the file; `done` takes the file's digest
afterwards; from then on every review compares the file with that digest.
`restore` puts the copy back. Nothing is ever undone without you asking.

## Where records come from

### You, or a script

A change you are about to make, or have made:

```
podshl-repairs begin --kind file --by me --path P [options]    # keeps a copy; prints the id
podshl-repairs done ID
podshl-repairs add --kind file|package|overlay --by NAME [options]   # already made; no copy
```

| Kind | What it is | What is kept | Flagged when |
|---|---|---|---|
| `file` | a file edited in place | SHA-256 of the file after the change; with `begin`, a copy from before | the file changed again or is gone; the copy is gone |
| `package` | a package built or pinned locally | the installed version, and the version the package manager offered then | the official package moved on (the local one is frozen), or it was updated |
| `overlay` | a local copy standing in front of a component (a cloned plugin, a patched script) | digest of the copy; digest of the original path, or the original package's version | what the copy overrides has changed; the copy changed |

Any record can name the upstream issue or pull request it works around
(`--issue URL`) and the first version that fixes it (`--fixed-in V`). Once that
version is installed, the record says the change may no longer be needed — or
the fix did not hold.

```
podshl-repairs add --kind package --by me --package hyprland \
      --issue https://github.com/hyprwm/Hyprland/pull/1234 --watch
podshl-repairs add --kind overlay --by my-agent --path ~/.local/share/app/plugins/foo \
      --original /usr/share/app/plugins/foo --original-package app-plugin-foo
```

Package versions are read with `pacman` (Arch, Omarchy), `dpkg-query` and
`apt-cache` (Debian, Ubuntu), `brew` (macOS), and on Windows from the programs'
uninstall entries (the installed version only; Windows has no package manager
to ask for a newer one).

**For an agent or a script that changes a file**, the order is `begin`, the
change, `done`:

```
id=$(podshl-repairs begin --kind file --by my-script --path ~/.config/app/app.conf \
      --issue https://github.com/owner/app/issues/42)
# ... change the file ...
podshl-repairs done "$id"
```

### A coding agent

An agent that fixes something on this machine does not write it down, and
neither does the person at midnight. So the agent's own hooks do it:

```
podshl-repairs install-agent-hook            # on Omarchy: the default agent
podshl-repairs install-agent-hook --agent claude
podshl-repairs install-agent-hook --print    # what it would write, and nothing else
podshl-repairs remove-agent-hook
```

Then you open the agent the way you always do and ask for a change in your own
words. Before it writes a file, a copy is kept; after, the record is finished
under the agent's name. Nobody types anything for the record.

* **One record per file per session.** The copy is from before the agent first
  touched the file, so Undo goes back to before all of it. A file the agent
  creates is recorded without a copy — there was nothing before.
* **Not recorded:** anything inside a git working tree (it has a better history
  already, and every edit to source code would bury the one change to
  `~/.config` that matters), temporary files, the agent's own directory, and
  this record itself.
* **Shell commands too.** Agents change config files with a shell command as
  often as with their file tools (`cat >> file`, `sed -i`, `tee`), and
  Omarchy's instructions for agents do it that way. Before the command runs,
  every file it names is copied aside; afterwards each one that changed becomes
  a record with that copy, and each one it created becomes a record without.
  Files it only read leave nothing, and the copies are gone again.
* **Not seen:** a file a command reaches without naming it — through a
  variable other than `$HOME`, a glob, or a script it calls — and a new file
  named by a bare word (`echo x > new.conf`; `./new.conf` is seen). Hand-made
  copies next to a file (`file.bak.123`, `file.orig`, `file~`) are not
  recorded as changes of their own.
* **It never stops the agent** and never changes what it writes; anything that
  goes wrong is a line on stderr and exit 0.
* Installing writes two entries into the agent's settings and changes nothing
  else there (the file's keys come back in alphabetical order); installing
  twice leaves one, and `remove-agent-hook` takes out only its own. **That
  change to the settings file is itself recorded**, with a copy, so it can be
  undone like any other.

**Measured for Claude Code only** (`~/.claude/settings.json`, or
`CLAUDE_CONFIG_DIR`). Walked on Omarchy with Claude Code 2.1.278 as the default
agent: a real edit to a file in `~/.config` was recorded and then restored, and
the same edit in a git repository left no record (`RR21`, `RR22`).

### Another agent

Every agent has its own hook format and its own settings file. Yours is not
refused, and it is not guessed at either.

**Measure it.** Nothing here knows where your agent keeps its hooks, and that
is half of what is missing:

```
podshl-repairs measure-agent-hook --agent opencode   # prints the two commands
podshl-repairs measure-agent-hook                    # what is being measured
podshl-repairs measure-agent-hook --stop
```

Put the two printed commands into your agent's own hook configuration and work
as you normally would. While measuring, **nothing is recorded** — the format is
not known yet — and every call is kept exactly as it arrived in
`agent-samples/` next to the record. Those files hold the paths and the shell
commands your agent used, which is your machine written down: nothing sends
them anywhere, and reading one before you send it is the point of keeping them
as files.

**Then let it read the format off them:**

```
podshl-repairs measure-agent-hook --write-format
```

That writes a draft entry into `agent-formats.json` beside the record, read off
the calls your agent actually made. It works on one distinction the samples
carry by themselves: a field belonging to the *session* is in every call, and a
field belonging to the *tool* is only in the calls that used it. That is also
what keeps the agent's own transcript out of it — `transcript_path` is a
perfectly good-looking path in every single call, and a record claiming you
repaired your agent's transcript would be a wrong record made confidently.

Use the draft with `podshl-repairs install-agent-hook --agent NAME --guessed`.
A format that was not walked is never treated as measured, whatever the file
says:

* the two hook commands are **printed to put in by hand** — a settings file
  whose shape nobody here has seen is not written;
* **every record it makes says so** in its reason: `recorded by the NAME hook,
  session … (this agent's hook format was read rather than measured, from read
  off 7 calls collected on this machine)`;
* **every call it cannot read is kept** as a sample while measuring is on, so
  the place the reading is wrong shows itself instead of failing quietly.

If it works on your machine, your samples and your `agent-formats.json` are
what turn it into a measured one for everybody else. You can also write the
entry by hand — see [`examples/agent-formats.json`](../examples/agent-formats.json)
for the fields.

### Gemini CLI

Walked on an Omarchy desktop with Gemini CLI 0.60.0 on 2026-09-21, so it is
measured and needs no `--guessed`:

```
podshl-repairs install-agent-hook --agent gemini
```

It prints the two commands and the block to paste, because this program does
not write another agent's settings file. **Two things are off by default, and
both cost an afternoon to find:**

* **`tools.enableHooks` is false** unless you set it. Without it your hooks are
  read, accepted, and never run — the record stays empty and nothing says why.
* **The folder must be trusted.** Gemini CLI refuses a headless run in an
  untrusted directory, which is the property that stops a cloned repository
  from running commands at you. Trust it in interactive mode, or set
  `GEMINI_CLI_TRUST_WORKSPACE=true`.

Two ways it differs from Claude Code, both found by measuring and neither
guessable: `file_path` arrives **relative** to the session's `cwd`, and there
is **no call id** at all, so a shell command's staging falls back to the
session and the command text.

One gap, named rather than papered over: `run_shell_command` can carry a
`dir_path` that moves where the command runs, and no call in the walk had one.
It is not in the format, because a key nobody has seen is a guess. A command
that uses it will leave no record rather than a record of the wrong file.

### An agent that calls a function instead of a command

Some agents do not run a command before a tool — they call a function inside
their own process. opencode is one. There is nothing there for `agent-hook` to
be wired to, so the joint is a small plugin that hands what it was given to the
same program every other agent's hook calls:
[`examples/agents/opencode/`](../examples/agents/opencode/). It forwards the
payload whole rather than picking the file out of it, because what the file is
called is the thing being measured.

Walked with opencode 1.18.31 against a local Ollama model: the hooks fire, the
calls are kept, and the draft read off them records and restores. Not walked: a
shell command, so `command` and `shell` in that draft are empty and **a change
opencode makes through a shell command leaves no record**. Named here rather
than filled in from documentation.

### Packages and plugins

A package that deliberately leaves the ordinary path knows why — built outside
the AUR, patching a system file until upstream fixes a bug, shipping a copy in
front of a packaged component. It says so in a small file it installs, and the
record takes it in on every review:

```
$ podshl-repairs list
[1789821805-2] package — podshl-bin (declared by podshl-bin) — applied
```

| Declared by | The file is in |
|---|---|
| a package | `/usr/share/podshl/repairs.d/*.json` |
| an Omarchy plugin | `~/.config/omarchy/plugins/<id>/repairs.d/*.json` |
| an installer into the home directory | `~/.local/share/podshl/repairs.d/*.json` |

* **Who declared it** is asked of the package manager (`pacman -Qo`,
  `dpkg -S`) or taken from the plugin's directory — never from the file.
* **An update** of the package follows the declaration: no second record, and
  the package's own update is not news.
* **The maintainer can withdraw it** in a later version, with a reason; the
  record says so once. A package removed leaves its records saying so.
* **A declaration never does anything**: it cannot undo, change or run
  anything, and it cannot make the record ask GitHub.

How to write one: [DECLARING.md](DECLARING.md), and the reference
[`podshl-repairs.d(5)`](../packaging/repairs/podshl-repairs.d.5).

### The PODSHL client

Every change the PODSHL client's window makes is written down before it runs,
or it does not run. Those records are in the same list, with the vendor or
project the window was talking to as who proposed them.

## Looking again

```
podshl-repairs review            # what wants another look, and why
podshl-repairs review --offline  # the same, asking nobody
podshl-repairs review --json     # for scripts
```

The review reads the machine — the files, the package manager, the
declarations — and changes nothing on it. It asks the network about one thing
only: an upstream issue you chose to watch, at most once a day. `--offline`
asks nobody.

A finding stays until you have looked at it (`keep`), and comes back when
something new happens: a later update, or a finding you were not shown.

## What each finding means

| The review says | What happened | What you might do |
|---|---|---|
| the file changed since the fix | the file is not what it was after the change was made — an update, a migration, a tool or a person rewrote it | look at the file; `restore` puts back the copy from before the change, or make the change again |
| the changed file is gone | the target no longer exists | `keep`, or `forget` a record that no longer means anything |
| the copy to go back to is gone | the backup under `~/.config/podshl/backups` was removed | the change can no longer be undone from here |
| the setting is no longer in the file | a setting the client wrote is not there any more | as above |
| the setting is no longer what the fix set | a setting outside any file (a service, a variable) was changed back | as above |
| updated from *A* to *B* since the fix | the package is at another version than when the change was made | check the fix still holds, then `keep` |
| upstream says *V* has the fix, and *W* is installed | the version named as the fix is installed | try without the workaround; a backport and a fix that did not hold look the same from here |
| held at *A*, the official package is now *B* | a package built or pinned locally has fallen behind | rebuild, or keep holding it |
| what this copy overrides has changed | the component an overlay overrides was updated | check the copy is still needed |
| the upstream issue is closed | a watched issue was closed | see whether the workaround can go |
| the upstream fix is merged, not yet released | a watched pull request was merged | wait for the release |
| the upstream fix is in release *T* | the merge was found in a release | compare with what is installed |
| the package's maintainer withdrew this: *reason* | a package or plugin withdrew what it declared | follow the maintainer's reason — often it says what to clean up |
| the package no longer declares this | the entry disappeared from a package that is still installed | as above, without a reason |
| the package that declared this is no longer installed | the declaring package or plugin was removed | the workaround may still be on the machine; decide whether it goes |
| The package manager does not know *P*, so its version cannot be compared. | the package is not installed any more, or never was | look by hand |
| *V* is a build from a repository's head; no release number is before or after it. | a `-git` build or similar | look by hand |

## Deciding

| Command | What it does |
|---|---|
| `podshl-repairs show ID` | everything about one record first: why, who declared it, upstream, whether there is a copy to go back to (`RR27`) |
| `podshl-repairs keep ID` | looked at, kept: what you were shown is not raised again until something changes |
| `podshl-repairs restore ID` | puts back the copy `begin` kept; the record becomes `undone` |
| `podshl-repairs watch ID on\|off` | ask GitHub about the record's issue |
| `sudo podshl-repairs forget ID` | remove what the record said ([below](#removing-a-record)) |

## After updates, or daily

`podshl-repairs install-hook` sets up a review that shows a desktop
notification when something wants a look. `--print` shows what it would do
without doing it, and `remove-hook` undoes it.

| System | What `install-hook` sets up | Tested |
|---|---|---|
| Omarchy | `~/.config/omarchy/hooks/post-update.d/podshl-repairs`, run by Omarchy after every update | **on a real Omarchy desktop, 2026-09-18**: installed (chosen over systemd because `~/.config/omarchy` exists), a real `omarchy-update` ran it, the notification reached the desktop, the update finished with exit code 0, `remove-hook` took the file and left the other hooks alone |
| Other Linux | systemd user service and timer `podshl-repairs` (daily, `Persistent=true`), notification via `notify-send` | **on a live systemd user session, 2026-09-18**: plain Debian with systemd as pid 1 and a lingering user (`docker/systemd-walk.Dockerfile`). The units are accepted, the timer is enabled and scheduled, a review that found something exits 3 and systemd records `Result=success` rather than a failed unit, a missing notification daemon does not break the run, and `remove-hook` leaves no timer and no unit files |
| Windows | scheduled task `PODSHL\Repairs review`, daily at 12:00, without administrator rights; Windows notification | **on Windows 11, 2026-09-18**: created, run (`schtasks /Run`, result 3), and the toast confirmed in the notification database under PODSHL's own name; removed, and removed again quietly |
| macOS | `~/Library/LaunchAgents/de.podshl.repairs.plist`, at login and daily at 12:00; notification via `osascript` | **on a hosted macOS runner, 2026-09-18** (`.github/workflows/macos-walk.yml`): the client runs, a fix is recorded, flagged and restored, a package's version is read through `brew`, and the plist is written, accepted by `plutil`, loaded and listed by `launchctl`, started, and removed again. **Not shown there:** that a notification *banner appears* — `osascript` is invoked and returns 0, but a runner has no logged-in session |

**On Omarchy the notice is one you can act on** (`RR20`). It goes through
Omarchy's own notification sender, **stays until you dismiss it** — Omarchy
shows an ordinary notice for eight seconds, and the hook runs in the middle of
an update while you are watching the terminal — and **a click opens the review
in a floating terminal**. The next notice replaces the last, so updates nobody
looked after leave one notice rather than a pile. It names the program that
raised it. Do-not-disturb still holds it back.

On Windows the notification is raised under PODSHL's own identity
(`de.podshl.client`), not PowerShell's. Uninstalling the client removes that
identity and the daily task, whether or not you ran `remove-hook` first.

On Linux without a systemd user session (a container, a plain SSH login),
`install-hook` stops with the error `systemctl` gave and exit code 1.

## Watching an upstream issue

**Off unless you switch it on, per record** (`--watch`, `watch ID on`, or the
button in the client's window). A lookup tells GitHub, from your address, which
issue this computer follows — which is why it is opt-in, and why a declaration
cannot switch it on. It goes straight to GitHub's public API, without an
account or token, and never through anybody else. A watched record is asked at
most once a day, and never again once the answer is final.

* An issue: open or closed.
* A pull request: open, closed or merged. If merged, **the first release that
  contains the merge commit** is found by asking GitHub, for each published
  release after the merge (at most eight, drafts and pre-releases skipped),
  whether the commit is part of it. That release is then compared with the
  installed version like `--fixed-in`.

Only `https://github.com/<owner>/<repo>/issues/<n>` and `…/pull/<n>` links can
be watched.

## What leaves this machine

**Nothing, except the GitHub lookup for an issue you chose to watch.** The
record, its copies, the declarations and the log stay in your home directory.
There is no account, no telemetry, and no server of ours involved.

**What it did is written down.** Every command that changes something adds a
line to `~/.local/state/podshl/client.log` — what was recorded, what a review
concluded and which flags it raised, what was restored, what a hook set up or
took away, which declarations were taken or refused, and every GitHub lookup
with what it answered, so a check made while you were not looking is not
invisible afterwards. The log is anonymised the same way a PODSHL report is,
and `PODSHL_LOG=0` turns it off.

## Removing a record

`sudo podshl-repairs forget ID` removes what a record said — the path, the
digest, who made it, the note, the upstream issue — and deletes the copy it
kept. What stays is a stub: that a record existed, when the change was recorded,
what kind it was, and when it was forgotten. A record with a hole in it should
not read like one that never had the entry.

It is deliberately awkward. **It needs administrator or root rights**, and that
is not about file permissions — `repairs.json` is in your own configuration
directory and you can edit it by hand. It is about who *cannot*: the agents,
skills and scripts this record exists to keep track of run as you, so a removal
an ordinary process can perform is a removal the thing being recorded can
perform. **And it needs you to type the record's id** at a real terminal; a
pipe is not a person, and no option skips either gate. There is no way to
remove a record from the client's window, because the window does not run as
root. The removal is written to the log; after it, that is the only place that
still says what the record held.

## Command reference

The full reference is [`podshl-repairs(1)`](../packaging/repairs/podshl-repairs.1)
(`man podshl-repairs`); every command also answers `--help` without running.

```
podshl-repairs review [--json] [--notify] [--offline]
podshl-repairs list [--json]
podshl-repairs show ID [--json]
podshl-repairs add --kind file|package|overlay --by NAME [--path P]
      [--package NAME] [--original P] [--original-package NAME]
      [--issue URL] [--fixed-in V] [--watch] [--note TEXT]
podshl-repairs begin --kind file --by NAME --path P [same options]
podshl-repairs done ID
podshl-repairs keep ID
podshl-repairs watch ID on|off
podshl-repairs restore ID
podshl-repairs forget ID
podshl-repairs install-hook [--print]
podshl-repairs remove-hook [--print]
podshl-repairs install-agent-hook [--agent NAME] [--print] [--guessed]
podshl-repairs remove-agent-hook [--agent NAME] [--print]
podshl-repairs measure-agent-hook [--agent NAME] [--stop] [--write-format]
podshl-repairs --version
```

| Exit code | Meaning |
|---|---|
| `0` | nothing to look at, or the command did what it was asked |
| `3` | `review` found something to look at |
| `1` | an error; the reason is on standard error |

**`podshl-client repairs …`** takes the same commands with `repairs` in front,
runs the same code, and reads and writes the same record: a fix recorded by one
program is seen by the other (`RR17`). `podshl-repairs` also accepts the word
`repairs` in front, so a hook written by either program runs with either
(`RR19`).

On Windows the installed client is a window program. It prints into the
terminal it was started from, but `cmd` and PowerShell do not wait for a window
program, so a script that needs the exit code starts it with
`Start-Process podshl-client -ArgumentList 'repairs','review' -Wait -PassThru`
and reads `ExitCode`.

## Files and environment

| Path | What it holds |
|---|---|
| `~/.config/podshl/repairs.json` | the record (`%APPDATA%\podshl` on Windows, `~/Library/Application Support/podshl` on macOS) |
| `~/.config/podshl/backups/` | the copies `begin` and the agent hook keep |
| `~/.config/podshl/agent-sessions.json` | which record belongs to which file in which agent session |
| `~/.local/state/podshl/client.log` | what every command that changed something did |
| `~/.config/omarchy/hooks/post-update.d/podshl-repairs` | the Omarchy update hook |
| `/usr/share/podshl/repairs.d/`, `~/.local/share/podshl/repairs.d/`, `~/.config/omarchy/plugins/*/repairs.d/` | declarations |

| Variable | Effect |
|---|---|
| `VS_ROOT` | keep the record somewhere else |
| `PODSHL_LOG=0` | write no log |
| `CLAUDE_CONFIG_DIR` | where Claude Code keeps its settings, when not `~/.claude` |

Every field a decision reads is written by code: versions come from the
package manager, paths are resolved, digests are computed. A note, or a
maintainer's reason, is kept and shown, and nothing reads it.

## Troubleshooting

**The agent changed a file and nothing was recorded.** Was the file inside a
git repository, in a temporary directory, or in the agent's own directory?
Those are skipped on purpose. Did the agent change it through a shell command
that did not name the file — a variable, a glob, a script? Those are not seen.
Was the hook installed before 0.1.8? Then it only sees the file tools: run
`podshl-repairs install-agent-hook` again. Is the hook installed at all —
`grep agent-hook ~/.claude/settings.json`?

**`install-agent-hook` says there is no hook format for my agent.** Claude Code
and Gemini CLI have been walked; anything else has not. Two ways on, both above under
[Another agent](#another-agent): `measure-agent-hook --agent NAME` keeps what
your agent sends so the format can be walked, and an entry in
`agent-formats.json` plus `--guessed` uses a format read from documentation,
saying so in every record it makes. Either way the record itself never needed
the hook: `begin`/`done`, `add`, `list`, `review` and `restore` work with any
agent, or none.

**The update ran and no notice appeared.** Is do-not-disturb on? A silenced
notice goes to Omarchy's notification history. Is the hook there —
`ls ~/.config/omarchy/hooks/post-update.d/`? Did the review find anything —
`podshl-repairs review`?

**A package's declaration does not show up.** `grep 'repairs declared'
~/.local/state/podshl/client.log` says what was taken or refused and why
([DECLARING.md](DECLARING.md#testing-your-declaration)).

**`review` exits with 3 in my script.** That is "something to look at", not
an error; errors are 1.

**`podshl-repairs: command not found` after installing.** `~/.local/bin` is not
on your `PATH`; the installer says so when it is not. Add it, or call
`~/.local/bin/podshl-repairs`.

## How this is tested

Every claim above is a case in [TESTCASES.md](TESTCASES.md) (`RR1`–`RR26`),
run by the suite on every change, and the ones that need a real desktop were
walked on one: Omarchy for the update hook, the notice and its click, the
agent hook with a real Claude, the installer, and a package declaration
installed, withdrawn and removed with a real `pacman`; Windows 11 for the
scheduled task and its toast; a live systemd session; a hosted macOS runner.
