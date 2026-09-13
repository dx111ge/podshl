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
| **Windows** x64 | `pwsh scripts/build_windows_installer.ps1 -ServerUrl … -IndexUrl … -LogKey …`, on Windows | per-user NSIS setup, no administrator | installed silently, started from an unrelated directory with an empty environment, verified the live operator's signed index with the compiled-in key, uninstalled (`L3`, `L9`, `L10`) |
| **Linux** x86_64 | `mise run release` — on Windows in the container, see below | binary and `.deb` declaring `libwebkit2gtk-4.1-0`, `libgtk-3-0` | the suite runs on Linux in the container; the packaged window has not been walked on a Linux desktop |
| **macOS** arm64 | `.github/workflows/macos.yml`, GitHub's `macos-14` runners, when a release is published | `.dmg` and a zipped `.app` | **built, never run by us** — there is no Mac here (`L4`, `P5`) |

None of them is signed. Windows SmartScreen names no publisher; macOS Gatekeeper
refuses an unsigned downloaded app until the quarantine attribute is removed
(`xattr -dr com.apple.quarantine /Applications/PODSHL.app`). A signature says
who built something, and there is no certificate yet — that is a decision with a
price, not work.

On Windows, **if** an action ever needs privilege it must go through a separate
binary; no published action does, and `L5` fails the day one would.

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

## What a release must not contain

`PUBLISHING.md` is the authority. In short: no internal notes or commercial
documents, no address of the maintainer's own network, no secret — the log key
in `release/` is the public half and nothing else.
