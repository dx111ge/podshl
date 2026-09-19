# PODSHL

**One support client for every open-source project — instead of pasting logs
into an issue and waiting days for "which version? which GPU? what does the log
say?"**

A project publishes a few static files in the repository it already has. The
client, on anybody's machine, reads what that project asked for — each reading
shown and agreed to separately — matches it against the project's own rules,
and gives the project's own answer at the moment something broke. No server to
run, no model to pay for, nothing to install per project.

![The client showing a project's own answer: a command to run, why it applies,
and a question about whether it worked](examples/engram/shots/client-en/07-their-answer.png)

*The project's own answer, on the user's machine. The amber line marks what the
person said rather than what was measured; every reading behind it was shown and
agreed to first.*

**Watch the whole thing** — a fix still sitting on the machine, a question, the
project's own answer, and the notes to take to the project:
[English, 2:36](examples/engram/videos/podshl-user.mp4) ·
[Deutsch, 2:55](examples/engram/videos/podshl-user-de.mp4) ·
[what a maintainer does](examples/engram/videos/podshl-maintainer.mp4)

**The record of local fixes on its own** — `podshl-repairs` on a clean Omarchy:
an agent's change recorded without anybody writing it, undone, overwritten by an
update and noticed, and a workaround kept with its reason:
[English, 3:54](examples/repairs/podshl-repairs.mp4)

## Try it

**Early beta**, against the operator at **https://sdota.de**.

    # Omarchy: the bar icon installs the client on first click
    omarchy plugin add https://github.com/dx111ge/omarchy-podshl --enable

    # Debian or Ubuntu: the .deb declares what it needs
    sudo apt install ./PODSHL_0.1.7_amd64.deb

    # the bare binary, on any Linux that already has WebKitGTK
    # (libwebkit2gtk-4.1-0 and libgtk-3-0 — it is not self-contained, on purpose)
    chmod +x podshl-client-0.1.7-linux-x86_64 && ./podshl-client-0.1.7-linux-x86_64

Windows, Linux and macOS:
**[download 0.1.7](https://github.com/dx111ge/podshl/releases/latest)**. Nothing
is signed — [INSTALL.md](docs/INSTALL.md) says how to start them anyway, and
exactly what they send.

**Something to try it on:** [`examples/engram/`](examples/engram/) takes a real
project from nothing to an answer — the files a maintainer writes, the
screenshots above, and the walk that produced them. That walkthrough is
recorded against a local host, and engram's published files have moved on since
it was made; it says so, and links the live ones. engram is registered on
`sdota.de`, so the client finds it for real too.

| You are | Start with |
|---|---|
| **Maintaining an open-source project** | **[ONBOARDING.md](docs/ONBOARDING.md)** — publish the files, register, see what your users run into |
| **Someone with a broken machine** | **[INSTALL.md](docs/INSTALL.md)** — install the client, and what it does and does not send |
| Checking how it works | [SERVER.md](docs/SERVER.md), [spec/](spec/SPEC.md), [TESTCASES.md](docs/TESTCASES.md) |

---

## For maintainers

* **No server and no model.** `.podshl/agent.yaml` and the solution files it
  names, in your repository — or built with the form at
  [sdota.de/publish/build](https://sdota.de/publish/build). They stay yours: in
  every clone, and they outlive this service.
* **The round trip stops.** What you would have asked for is read on the user's
  machine before you are involved, against a list *you* wrote.
* **You answer with rules, not prose.** `answers.when` is matched exactly: the
  same configuration gets the same answer, and you can see which fact decided it.
* **What your users hit reaches you, without a ticket.** When people agree to
  report, your private dashboard shows which setups break, whether your answers
  worked, and what nothing you published covers yet — shown only once five
  different people reported the same thing, with names, addresses and tokens
  removed on their machine first.

Two things this does *not* do yet — an automatic path into your tracker, and
feature requests — are written down with the reasoning in
**[MAINTAINERS.md](docs/MAINTAINERS.md)** rather than left as silences.

## For users

* **An answer now**, from the people who wrote the software.
* **Nothing is read or sent without you seeing it first.** Read, send, change:
  separate consents, each showing the actual values. The refusing button holds
  focus, so a stray Return never agrees to anything.
* **What leaves is what you were shown**, with your account name, addresses,
  tokens and times already taken out, and still editable.
* **A change is shown, dry-run first, and reversible.** The client can do three
  things at all — state a finding, set a key in a configuration file, or restore
  the copy it made — because a project may choose an operation and never invent
  one.
* **A fix that is still on your machine is not forgotten.** Local fixes go
  stale in ways nobody notices: an update overwrites one, the upstream bug gets
  fixed and the workaround is now the problem, a copied plugin keeps running
  while the packaged one moves on. Every change is written down before it is
  made, and changes made by *other* tools — an agent, a script, you — can be
  registered too. After an update, or once a day, the client says which ones
  want another look and why; it undoes nothing by itself.
  [REPAIRS.md](docs/REPAIRS.md)

## How it works

```
  maintainer                    operator (sdota.de)                 user
  .podshl/ in the repo  --->  mirror + signed public log  --->  client
                                                                  | reads locally,
                                                                  | item by item
                              matches the project's rules  <-----+ sends only
                              stores nothing of the lookup        | what you agreed
                              ----------- the fix ----------->   +
                                                                  | shows, dry-runs,
                              optional report, pseudonymous <----+ applies
```

Matching is **rule evaluation over closed vocabularies**, not generation, so no
model is in the path where a project has published. Where nothing is published,
the client can still help with a model *you* choose — a local one keeps
everything on your machine.

## Why you can check it

* **Everything that runs is in this repository** — the client, the whole operator
  server including the dashboard, and the deployment that runs sdota.de.
* **The log is public and signed.** Every mirrored file is attested with the
  commit it came from; the client verifies the log with the key compiled into
  it, and [spec/monitor/verify_log.py](spec/monitor/verify_log.py) lets anyone
  watch it.
* **What the operator holds is written down**: [SERVER.md](docs/SERVER.md) and the
  privacy notice at [sdota.de/privacy](https://sdota.de/privacy). Reports count
  distinct people, not submissions; the monthly key that could link a report to
  a pseudonym is destroyed when the month ends.

## Build and run

```bash
# the operator, its database and the test counterparty, in one container
docker compose up -d
docker compose exec podshl python run_testcases.py

# the client, natively
cd client-rs && cargo run
```

How the release packages are built, and what each one has been run on:
[RELEASING.md](docs/RELEASING.md). How sdota.de is deployed:
[deploy/](deploy/README.md).

## What is here

| | |
|---|---|
| `client-rs/` | The client — Rust and Tauri, a binary of under 7 MB |
| `src/podshl/server/` | The operator — mirror, signed transparency log, anchor checks, reports, dashboard |
| `spec/` | The protocol — prose, both vocabularies, schemas, worked examples, the log monitor |
| `deploy/` | Docker Compose, firewall and backup for running an operator |
| `examples/engram/` | A real project taken from nothing to a dashboard, with screenshots and screencasts |
| `packaging/` | The Arch package `podshl-bin` and the Omarchy plugin, whose own repository is assembled from here |
| `docs/` | Installing, onboarding, [what a maintainer gets](docs/MAINTAINERS.md), the operator, releasing, the test cases, [REPAIRS.md](docs/REPAIRS.md) — keeping track of local fixes — and [FINDINGS.md](docs/FINDINGS.md), what was measured before this was built and what was discarded because of it |
| `scripts/` | `build/`, `release/`, `ci/`, `walk/` (driving the real window) and `dev/` (fixtures and measurements) |
| `run_testcases.py` | Every case in `docs/TESTCASES*.md` marked `auto`, by id |

## Licence, warranty, security

**AGPL-3.0** for the code; **Apache-2.0** for `spec/` code and schemas and
**CC-BY-4.0** for its prose, so the protocol can be implemented without the
implementation inheriting a licence — [LICENSING.md](docs/LICENSING.md).

**No warranty.** Support content is written by the projects themselves; the
operator mirrors it and names the project in every answer. The client bounds
what a change can *do*, not whether it fixes anything — back up what matters.

Found something? **[SECURITY.md](SECURITY.md)**.
