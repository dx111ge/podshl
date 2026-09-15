# Driving the window

`client-rs/ui/` is Tauri's `frontendDist` and is bundled into the application, so
a `package.json` or a `node_modules` in it would ship to users. This lives beside
it instead.

No dependencies, and that is not frugality: Node 22 has a `WebSocket` and a test
runner, and the thing being driven is a web view that already speaks the
DevTools protocol. A browser automation stack here would be a second browser.

## What this covers, and what it does not

| | |
|---|---|
| **1** Rust commands | `cargo test`, and `podshl-client invoke` against a live operator |
| **2** flow logic | *not built* — see below |
| **3** the window | **this**, against the real binary and the real engine |

Layer 2 in the plan is not this and not the bridge either: it is the *flow
logic* pulled out of the DOM code and tested with a measured transport. That is
weeks of untangling 2212 lines, and it is the item worth the most — every defect
this project has seen lived in them.

**The `invoke` surface is settled, and it is not what blocks that.** The
commands the published path needs are on it, except three that a released
client must not have: `perform_reads` runs readings on somebody's machine,
`send_published_report` sends their data, `llm_translate` can spend their money.
From the window each happens behind a consent screen; from a command line none
would. They are compiled in only under `--features uitest`, so a released binary
does not have them at all — a decision made when somebody builds, rather than at
run time by whatever is running, which is what an environment variable would
have been. `build_client.sh` and `build_client.ps1` ask the artefact and refuse
to release one that answers.

So a headless layer 3a is now buildable against the real binary, with nothing
recorded and nothing stubbed. It is not built yet.

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
