# Releasing

What is built, where, for which operator — and what each build has and has not
been run on.

## What a release is

**The client**, one package per platform, built for **one operator**: its
address and the public key of its transparency log are compiled in, because an
installed program has nobody to set its environment. `PODSHL_SERVER_URL`,
`PODSHL_INDEX_URL` and `VS_LOG_KEY` still override them at run time.

| Build variable | What |
|---|---|
| `PODSHL_BUILD_SERVER_URL` | the operator, e.g. `https://sdota.de` |
| `PODSHL_BUILD_INDEX_URL` | the responsiveness index; the beta sets the operator's address, which has none, rather than leave a released client asking loopback |
| `PODSHL_BUILD_LOG_KEY` | the text of the operator's **public** log key, `release/<operator>/log_key.json` |

The server is not released as an artefact. It is operated, from `deploy/`.

## The version lives in one place and is checked

`client-rs/Cargo.toml` is the source. `tauri.conf.json` must match it, and
`ui_contract::tests::the_version_is_the_same_everywhere` fails if they drift.
Tags are `v<version>`.

## Where each platform is built

Tauri release builds do not cross-compile; each bundle needs a machine running
its system.

| | Built by | Package | Run and checked |
|---|---|---|---|
| **Windows** x64 | `pwsh scripts/build/build_windows_installer.ps1 -ServerUrl … -IndexUrl … -LogKey …`, on Windows | per-user NSIS setup, no administrator | installed silently, started from an unrelated directory with an empty environment, verified the live operator's signed index with the compiled-in key, uninstalled (`L3`, `L9`, `L10`) |
| **Linux** x86_64 | `mise run release` — on Windows in the container, see below | binary and `.deb` declaring `libwebkit2gtk-4.1-0`, `libgtk-3-0`; the binary also goes into the AUR package `podshl-bin` | the suite runs on Linux in the container; `podshl-bin` was built, installed and started on Omarchy 4.0.4 |
| **macOS** arm64 | `.github/workflows/macos.yml`, GitHub's `macos-14` runners, when a release is published | `.dmg` and a zipped `.app` | **built, never run by us** — there is no Mac here (`L4`, `P5`) |

None of them is signed. Windows SmartScreen names no publisher; macOS Gatekeeper
refuses an unsigned downloaded app until the quarantine attribute is removed
(`xattr -dr com.apple.quarantine /Applications/PODSHL.app`). A signature says
who built something, and there is no certificate yet — that is a decision with a
price, not work.

On Windows an action that needs privilege goes through a separate binary, and
one exists: `podshl-elevate`, built by the installer script and put beside the
client. Three example actions are marked `elevated` and only it performs them —
the client asks Windows to start it with the `runas` verb, so the prompt is
Windows' own. `L5` is `auto`.

### Linux, in the container

    KEY="$(cat release/sdota.de/log_key.json)"
    docker compose run --rm --no-deps \
      -e PODSHL_OPS_TOKEN="$(cat var/ops.token)" \
      -e PODSHL_BUILD_SERVER_URL=https://sdota.de -e PODSHL_BUILD_INDEX_URL=https://sdota.de \
      -e "PODSHL_BUILD_LOG_KEY=$KEY" podshl release

`PODSHL_OPS_TOKEN` is passed so the one-off container does not mint a new token
into `var/ops.token` under the running development container. The task writes
`var/release/<version>/`: the binary, the `.deb`, `SHA256SUMS` and `PLATFORM.md`.
`mise.toml` has to keep LF line endings (`.gitattributes`) — with CRLF its task
dies on `set -e` before building anything.

## Cutting one

1. Both suites green: `docker compose exec podshl python run_testcases.py`.
2. Bump the version in `Cargo.toml` and `tauri.conf.json`.
3. Build Windows and Linux as above, against `release/<operator>/log_key.json`,
   and check that key against `https://<operator>/log/key`.
4. Scan the tree that is about to be published for anything private
   (`PUBLISHING.md`) — the LAN, account names, the operator's address outside
   the imprint, tokens.
5. Publish the release on GitHub with the Windows and Linux files attached; the
   macOS workflow builds and attaches its own.
6. **The AUR package**, once the release's files are downloadable — it points
   at them. See below.
7. **The Omarchy plugin**, on every client release — its `package/PKGBUILD`
   names the client version — and whenever `packaging/omarchy-plugin/` changed: bump
   `version` in its `manifest.json` and publish it to its own repository, below.

## The AUR package, `podshl-bin`

`packaging/aur/podshl-bin/` is the source; the AUR's own git repository
(`ssh://aur@aur.archlinux.org/podshl-bin.git`) receives a copy of its three
files, `PKGBUILD`, `.SRCINFO` and `podshl-client.desktop`. It installs the
**released** Linux binary, so there is nothing to build for it — the operator
and key are already compiled in.

For a new version, on Arch (or in the `archlinux` image, as a user that is not
root, with `base-devel pacman-contrib namcap webkit2gtk-4.1 gtk3`):

    cd packaging/aur/podshl-bin
    # set pkgver, reset pkgrel=1
    updpkgsums
    makepkg -f && namcap PKGBUILD && namcap podshl-bin-*.pkg.tar.zst
    makepkg --printsrcinfo > .SRCINFO

Then check that the binary's sum `updpkgsums` wrote is the one in the release's
`SHA256SUMS`, commit the three files here, and push the same three to the AUR.
`namcap` reports nothing on 0.1.4 and 0.1.5; a new warning is a change to look at, not
noise. Install the built package once and start it before pushing — a package
that builds is not yet a package that runs.

The package is the bare release binary, **not** the one inside the `.deb`: the
two differ in exactly three bytes, Tauri's bundle-type marker (`UNK` against
`DEB`), and this is not a `.deb` install.

## The Omarchy plugin

`omarchy plugin add <url>` clones a repository and looks for `manifest.json`
**at its root** — measured: a subdirectory is refused with *missing
manifest.json*, and there is no option to name one. So the plugin is published
as its own small repository, `github.com/dx111ge/omarchy-podshl`. It is
assembled, never edited:

    scripts/release/publish_omarchy_plugin.sh ../omarchy-podshl

copies `packaging/omarchy-plugin/`, `LICENSE`, and the AUR package's `PKGBUILD` and desktop
entry into `package/`, and refuses when the PKGBUILD's version is not the
client's. `install-client.sh` builds that `package/PKGBUILD` with makepkg. It
used to ask the AUR first and fall back to this; the AUR closed registration in
September 2026, so that question had one answer and the other branch was one
nobody could reach or test. When registration reopens, that is the line to put
back. **So the plugin repository has to be republished
with every client release**, or its PKGBUILD points at the previous one.

Before publishing, on an Omarchy desktop: `omarchy plugin validate`, then add it
from a local clone (`omarchy plugin add file:///…`), **restart the shell** —
4.0.4 keeps running the previous widget after an update — and click through
both paths: client missing, and client installed.

## What a release must not contain

`PUBLISHING.md` is the authority. In short: no internal notes or commercial
documents, no address of the maintainer's own network, no secret — the log key
in `release/` is the public half and nothing else.
