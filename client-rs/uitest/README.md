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

Layer 2 was planned as a headless page with `window.__TAURI__.core.invoke`
bridged to `podshl-client invoke`, so a test could drive the real Rust in
seconds without a window. It is not buildable as described, and the reason is
worth writing down rather than rediscovering: **eight of the commands the
published path calls are not on the `invoke` surface**, and several are absent
on purpose — `perform_reads` runs readings on the machine, `send_published_report`
sends somebody's data, `llm_translate` can spend their money. Widening that
surface to make a test possible would be widening it for everybody. Recording
the answers instead was ruled out by the same plan that asked for the bridge,
and rightly: a recording is a claim that the shape has not changed.

So the choice is a decision somebody has to make — widen the surface, or accept
that the fast loop stops before the readings — and it is not one to make by
writing a test that quietly assumes it.

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

They do not run in CI: a desktop session is needed and GitHub's runners have
none. `scripts/ci.sh` gates on layers 1 and 2's Rust half, which is everything
that can be checked without one.
