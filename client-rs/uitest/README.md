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

**Layer 2 is done.** `ui/flow.js` holds the decisions that were wrong twice, or
written out two and three times:

* what a search hit publishes, what to say when nothing is published, and what
  to do with each outcome the published path returns;
* what a round of answers means — a skip is `<id>.declined`, an unanswered
  *required* field is not a skip, a publisher's `pattern` is anchored and a
  broken one constrains nothing. All three panels that ask a person a question
  go through it;
* what editing a value before it goes means — only a person's own words are
  editable, emptying a box withdraws the answer rather than sending an empty
  one, and an edit naming something the panel did not show is ignored;
* who may receive what. `Flow.consent()` is a gate: a panel that showed a
  snapshot and got a yes grants it, naming who for, and a send asks the gate.
  A send with no panel in front of it throws instead of sending quietly;
* report assembly. `preview_report` in Rust decides what a report holds and
  what is withheld, so the open question was whether a decision was left in the
  page at all. There were three, and they are here now: which withheld facts
  still have words to offer — `stated[id] === null`, never `dropped`, because
  that also holds machine readings and a box to retype one of those is what the
  reading panel refuses; what an emptied free-text box means; and what the
  footer checkbox does, with the footer's words coming from the binary that
  wrote them rather than a second copy of the sentence living here.

Each move was covered by 3a, which is what made starting safe.

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
