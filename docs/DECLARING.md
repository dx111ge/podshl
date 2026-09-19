# Declaring what your package does to a machine

**For maintainers of packages, installers and Omarchy plugins.**

Your package sometimes has to leave the ordinary path. It is built from a
PKGBUILD because it cannot be in the AUR. It patches a file outside itself to
work around a bug upstream has not fixed yet. It ships a copy of a component
that stands in front of the packaged one. You know why — and the person whose
machine it is does not, and will not a year from now, when the upstream bug is
fixed and the workaround has quietly become the problem.

So say it, once, in a small file your package installs. The
[record of local fixes](REPAIRS.md) reads it on every review, keeps it as a
record with your reason, and looks at it again after every update. When the
workaround is no longer needed, your next version withdraws it, and the person
is told — in your words — what they can now clean up.

The reference for the format is the man page
[`podshl-repairs.d(5)`](../packaging/repairs/podshl-repairs.d.5); this page is
the guide.

---

- [What you get, and what it costs](#what-you-get-and-what-it-costs)
- [Where the file goes](#where-the-file-goes)
- [The format](#the-format)
- [Examples](#examples)
- [Updating and withdrawing](#updating-and-withdrawing)
- [How the declaration is attributed](#how-the-declaration-is-attributed)
- [What a declaration cannot do](#what-a-declaration-cannot-do)
- [Testing your declaration](#testing-your-declaration)
- [Checklist](#checklist)

---

## What you get, and what it costs

**It costs one file.** No dependency on PODSHL, no service, no account, no
build step. If `podshl-repairs` is not installed, the file is inert: nothing
reads it and nothing breaks. If it is, the person sees:

```
$ podshl-repairs list
[1789821805-2] package — podshl-bin (declared by podshl-bin) — applied
```

and `podshl-repairs show` gives your reason in full:

```
$ podshl-repairs show 1789821805-2
[1789821805-2] package — podshl-bin (declared by podshl-bin)
  state: applied
  why: podshl-bin is built from its PKGBUILD, not installed from the AUR: ...
  declared by: podshl-bin
  installed when recorded: 0.1.8-1
  no copy to go back to
  recorded: 2026-09-19
```

With your reason attached, and — once you name an upstream issue or the version
that fixes it — a notice when that moment comes:

```
$ podshl-repairs review
[1789821805-3] file — /etc/foo.conf (declared by foo-patched)
  - upstream says 2.5.0 has the fix, and 2.5.1 is installed
```

**What you get** is that the reason for your workaround survives on the machine
as long as the workaround does, attributed to your package, and that you can
take it back in the same place you put it: your package.

## Where the file goes

| You ship | Install the declaration to | Attributed to |
|---|---|---|
| a system package (pacman, `.deb`, …) | `/usr/share/podshl/repairs.d/<name>.json` | the package that owns the file, asked of `pacman -Qo` or `dpkg -S` |
| an Omarchy plugin | `repairs.d/<name>.json` inside the plugin's own repository | `omarchy plugin <id>`, from the directory Omarchy installed it into, which is named after the plugin's `id` |
| an installer that writes into the home directory (`curl … \| bash`) | `~/.local/share/podshl/repairs.d/<name>.json` | nobody: the record is named after the file |

`<name>` is yours to choose; use your package's name so two packages never
collide. The directories are read afresh on every review, so a package or
plugin installed since the last one is found, and one removed is noticed.

## The format

One JSON object with a list of entries:

```json
{
  "records": [
    {
      "id": "outside-the-aur",
      "kind": "package",
      "package": "podshl-bin",
      "reason": "podshl-bin is built from its PKGBUILD, not installed from the AUR ...",
      "until": { "issue": "https://github.com/owner/repo/issues/1" }
    }
  ]
}
```

| Field | Required | What it is |
|---|---|---|
| `id` | yes | The entry's identity across versions of your package. Letters, digits, `-`, `_`, `.`; at most 64. **Keep it stable**: a new id is a new record. |
| `kind` | yes | `file`, `package` or `overlay` (below). |
| `reason` | yes | Why, in the words somebody will read a year from now. At most 2000 characters. An entry without one is refused. |
| `path` | for `file` and `overlay` | An absolute path. For `file`, the file your package changes; for `overlay`, the copy that stands in front of something. **It must exist** when the declaration is read, or the entry is refused (and taken on a later review once it does). |
| `package` | for `package` | The package this is about. With `file` or `overlay` it is optional and names the package whose version `until.fixed_in` is compared against. |
| `original` | for `overlay` (this or `original_package`) | The path of what the copy stands in front of. |
| `original_package` | for `overlay` (this or `original`) | The package that provides it. |
| `until.issue` | no | The upstream issue or pull request the workaround is for — an `https://` URL. Shown to the person; GitHub is asked about it only if *they* switch watching on. |
| `until.fixed_in` | no | The first version with the fix. Once the installed version of `package` reaches it, the record says the workaround may no longer be needed. |
| `retired` | — | Instead of all the above: withdraws the entry. See [Updating and withdrawing](#updating-and-withdrawing). |

**The three kinds**, and what is watched for each:

| `kind` | Use it for | What the review watches |
|---|---|---|
| `file` | a file outside your package that your package, its install script or its installer changes | the file changing again or disappearing, from the moment it was first read |
| `package` | your package itself, when *how* it is installed is the workaround — built outside the AUR, pinned, held back | its installed version against `until.fixed_in`; a package the official repositories offer a newer version of is reported as held behind it |
| `overlay` | a copy standing in front of a packaged component | what it stands in front of changing underneath it, and the copy changing |

## Examples

### A package built outside the AUR

This is the one that started it. `podshl-bin` cannot be in the AUR while AUR
registration is closed, so it is built from its PKGBUILD. An AUR helper looks
every such package up on each update, and whoever registered the free name
could have their package installed in its place. The person has to know that,
and has to know when it stops being true.

[`packaging/aur/podshl-bin/podshl-bin.repairs.json`](../packaging/aur/podshl-bin/podshl-bin.repairs.json):

```json
{
  "records": [
    {
      "id": "outside-the-aur",
      "kind": "package",
      "package": "podshl-bin",
      "reason": "podshl-bin is built from its PKGBUILD, not installed from the AUR: AUR registration is closed, so it cannot be published there yet, and the name is not registered. An AUR helper looks every such package up on each update, and whoever registered the name could have their package installed in its place. Keep it out of the helper's reach until it is in the AUR under its maintainer — a local pacman repository, or IgnorePkg in /etc/pacman.conf."
    }
  ]
}
```

In the PKGBUILD it is one more source and one more line in `package()`:

```bash
source=(
  # ...
  "$pkgname.repairs.json"
)

package() {
  # ...
  install -Dm644 "$pkgname.repairs.json" "$pkgdir/usr/share/podshl/repairs.d/$pkgname.json"
}
```

### A package that patches a system file

A driver package that has to set a kernel parameter in `/etc` until upstream
releases a fix:

```json
{
  "records": [
    {
      "id": "modeset-workaround",
      "kind": "file",
      "path": "/etc/modprobe.d/foo-modeset.conf",
      "package": "foo-driver",
      "reason": "Sets modeset=0 for foo: 2.4 freezes on resume with it on (upstream #812). Remove the file once 2.5 is installed.",
      "until": {
        "issue": "https://github.com/foo/foo-driver/issues/812",
        "fixed_in": "2.5.0"
      }
    }
  ]
}
```

When `foo-driver` 2.5.0 or later is installed, the review says so. If somebody
edits or removes the file in the meantime, the review says that too.

### A copy in front of a packaged component

A patched plugin in `/usr/local` that shadows the distribution's:

```json
{
  "records": [
    {
      "id": "patched-foo-plugin",
      "kind": "overlay",
      "path": "/usr/local/share/app/plugins/foo",
      "original": "/usr/share/app/plugins/foo",
      "original_package": "app-plugin-foo",
      "reason": "A copy of app-plugin-foo with the fix for #99 applied; the packaged one crashes on start. Remove this copy once app-plugin-foo carries the fix.",
      "until": { "issue": "https://github.com/app/foo/pull/100" }
    }
  ]
}
```

When `app-plugin-foo` is updated, the review says what the copy stands in front
of has changed — the moment to check whether it is still needed.

### An Omarchy plugin

A plugin is a package by other means: it arrives, is updated and is removed
without the package manager, and can change the machine as much as a package
can. It declares from inside itself:

```
my-plugin/
├── manifest.json
├── BarWidget.qml
└── repairs.d/
    └── my-plugin.json
```

Omarchy installs the plugin into `~/.config/omarchy/plugins/<id>/`, named after
the `id` in its `manifest.json`; the record reads `repairs.d/` there and names
the entries `declared by omarchy plugin <id>`. When the plugin is updated, the
declaration follows; when it is removed, the record says so.

### An installer into the home directory

A script that installs into `~/.local` can leave its declaration beside it:

```bash
mkdir -p ~/.local/share/podshl/repairs.d
cat > ~/.local/share/podshl/repairs.d/my-tool.json <<'EOF'
{ "records": [ {
    "id": "shell-hook",
    "kind": "file",
    "path": "/home/USER/.bashrc",
    "reason": "my-tool adds one line to ~/.bashrc to put itself on PATH. my-tool uninstall removes it."
} ] }
EOF
```

Nothing owns such a file, so the record is named after it
(`declared by my-tool`), not after a package.

### A Debian package

The same file, installed by your packaging. Keep it as `debian/foo.json` and
list it in `debian/foo.install`:

```
debian/foo.json usr/share/podshl/repairs.d/
```

`dpkg -S` then names `foo` as its owner.

## Updating and withdrawing

**Updating:** change the entry and ship the new version. The record follows —
a new reason, a new `until` — and keeps its identity, as long as the `id` is the
same. Your package's own update is not reported as news about the record.

**Withdrawing:** when a later version no longer needs an entry, keep the `id`
and replace the entry with `retired` and a reason that tells the person what
they can now clean up:

```json
{
  "records": [
    {
      "id": "outside-the-aur",
      "retired": "podshl-bin is in the AUR now; the local repository or IgnorePkg entry can go."
    }
  ]
}
```

The next review says, once:

```
[1789821805-2] package — podshl-bin (declared by podshl-bin)
  - the package's maintainer withdrew this: podshl-bin is in the AUR now; the local repository or IgnorePkg entry can go.
```

and is quiet after the person has looked at it.

| What your next version does | What the record says |
|---|---|
| changes the entry, same `id` | nothing new; the record follows the declaration |
| replaces it with `retired` and a reason | "the package's maintainer withdrew this: *your reason*", once |
| drops the entry from the file | "the package no longer declares this", once — prefer `retired`, which says why |
| the package is removed | "the package that declared this is no longer installed" |
| declares a new `id` | a new record |

**Nothing is ever removed from the record** by a declaration, a withdrawal or
a removal: the person decides. That is deliberate. The workaround you shipped
may still be on the machine after your package is gone, and the record is
where the person finds out it was yours.

## How the declaration is attributed

The record never takes a name from inside the file — anybody can write any
name into a file. It asks:

- for a file under `/usr/share`, the package manager: `pacman -Qqo` on Arch and
  Omarchy, `dpkg-query -S` on Debian and Ubuntu;
- for a file in an Omarchy plugin, the directory Omarchy installed the plugin
  into;
- for a file nobody owns, nobody — the record is named after the file.

## What a declaration cannot do

A declaration is information. It **cannot**:

- undo, change, delete or create anything on the machine;
- run anything;
- make the record ask GitHub — `until.issue` is only shown until the person
  switches watching on;
- remove a record, its own or anybody else's.

The worst a malicious declaration can do is add a line to a list.

## Testing your declaration

1. Install your package (or plugin) on a machine with
   [`podshl-repairs`](REPAIRS.md#install).
2. Run `podshl-repairs list`. Your entry should be there as
   `declared by <your package>`.
3. If it is not, the reason is in the client log:

   ```
   grep 'repairs declared' ~/.local/state/podshl/client.log
   ```

   Every declaration taken, followed, withdrawn or refused leaves a line there.

| The log says | What to fix |
|---|---|
| `declaration … is not readable, left as it is` | the file is not valid JSON, or has no `records` list |
| `…: an entry has no usable id` | the `id` is missing, empty, too long, or has other characters than letters, digits, `-`, `_`, `.` |
| `…: a declaration says why, and this one does not` | add `reason` |
| `…: kind "…" is not file, package or overlay` | use one of the three |
| `…: The change cannot be recorded: … is missing or not in the expected form.` | a field the kind needs is missing (`package` for `package`, `original` or `original_package` for `overlay`), or `until` holds something other than an `https://` issue and a version |
| `…: The change cannot be recorded: … does not exist.` | the `path` is not there when the review runs; it is taken on a later review once it is |
| nothing at all | the file is not in one of the [three places](#where-the-file-goes), or its name does not end in `.json` |

4. Ship a version that withdraws it, and run `podshl-repairs review`: the
   withdrawal should be said once, with your reason.

## Checklist

- [ ] The file is in the right place for what you ship, and ends in `.json`.
- [ ] Every entry has a stable `id` and a `reason` a stranger can act on.
- [ ] `until` names the issue and, if you know it, the version with the fix.
- [ ] Your next version withdraws the entry with `retired` when it is no longer
      needed, instead of dropping it silently.
- [ ] `podshl-repairs list` shows it as `declared by <your package>` after
      installing.

See also: [`podshl-repairs.d(5)`](../packaging/repairs/podshl-repairs.d.5),
[`podshl-repairs(1)`](../packaging/repairs/podshl-repairs.1),
[REPAIRS.md](REPAIRS.md).
