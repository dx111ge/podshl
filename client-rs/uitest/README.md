# Driving the window

`client-rs/ui/` is Tauri's `frontendDist` and is bundled into the application, so
a `package.json` or a `node_modules` in it would ship to users. This lives beside
it instead.

No dependencies, and that is not frugality: Node 22 has a `WebSocket` and a test
runner, and the thing being driven is a web view that already speaks the
DevTools protocol. A browser automation stack here would be a second browser.

## What this covers, and what it does not

| | | |
|---|---|---|
| **1** Rust commands | `cargo test`, and `podshl-client invoke` against a live operator | in CI |
| **2** flow decisions | `tests/flow-decisions.test.mjs` over `ui/flow.js` — no page, no network, milliseconds | in CI |
| **3a** the page, headless | `tests/flow-headless.test.mjs` — the shipped page in a browser, `invoke` bridged to the real binary | in CI |
| **3b** the real window | `tests/published-path.test.mjs` — WebView2 over the DevTools protocol | **Windows, by hand** |

**Layer 2 is begun, not finished.** `ui/flow.js` holds the decisions that were
wrong twice: what a hit publishes, what to say when nothing is published, and
what to do with each outcome the published path returns. The consent ordering,
the question rounds and the report assembly are still inside the page's 2200
lines. They belong here too, one at a time, each move covered by 3a — which is
what made this move safe to start.

The `invoke` surface is settled and is not what blocks the rest: three commands
a released client must not have are compiled in only under `--features uitest`,
so a release cannot be asked for them and both build scripts refuse to ship one
that answers.

## Running them

They need a client built for a reachable operator, because they walk the real
one. They fail with an instruction rather than skipping: a suite that quietly
tests nothing is worse than one that is red.

    pwsh ../../scripts/build_client.ps1 sdota.de -Profile debug   # or build_client.sh
    npm test

| variable | |
|---|---|
| `PODSHL_CLIENT` | the binary to drive; otherwise `../target/{debug,release}` |
| `UITEST_QUERY`, `UITEST_PROBLEM`, `UITEST_CLASS` | which project, problem and class |
| `UITEST_PORT` | the debugging port, 9444 by default |

A window opens while they run. Nothing is typed at the operating system — this
talks to the web view's own engine — so a person at the keyboard is not
interrupted, which is the whole reason it is done this way. On 2026-09-14 the
same job was attempted with synthetic keystrokes against the operator's desktop
and broke whenever they typed.

They do not run in CI, for two reasons rather than one. A desktop session is
needed and GitHub's runners have none — and this drives **WebView2**, over the
DevTools protocol, which is Windows. WebKitGTK, which the Linux `.deb` runs on,
has no CDP endpoint at all; it exposes WebKit's own remote inspector and nothing
here speaks that protocol.

So this is a Windows developer's command, not a gate. `scripts/ci.sh` gates on
everything that can be checked without a desktop, and the flow itself is not in
that set — which is the open question, not a detail.
