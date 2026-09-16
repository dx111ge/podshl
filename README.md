# PODSHL

**One support client for every open-source project — instead of pasting logs
into an issue and waiting days for "which version? which GPU? what does the log
say?"**

A project publishes a few static files in the repository it already has. The
client, on anybody's machine, reads what that project asked for — each reading
shown and agreed to separately — matches it against the project's own rules,
and gives the project's own answer at the moment something broke. No server to
run, no model to pay for, nothing to install per project.

**Early beta.** The operator runs at **https://sdota.de**. Clients for Windows,
Linux and macOS: **[download 0.1.5](https://github.com/dx111ge/podshl/releases/latest)**
(unsigned — [INSTALL.md](docs/INSTALL.md) says how to start them anyway, and what
they send).

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

### Your own ticket system

Today a diagnosis that nothing published covers ends as **Markdown you take to
the project**: anonymised, editable, shown before it is copied, with the
address from your `escalate.target` beside it. That works with every tracker
there is — GitHub, GitLab, Jira, Redmine, Zammad, your own — because the
thing carrying it is a person with a clipboard, and a person needs no
integration.

**An automatic path into your tracker is planned and not built.** If you run
your own ticket system you already have the infrastructure that would make it
worth doing, and "copy this into the browser" is a worse answer for you than
for a project whose tracker is a GitHub tab. It is not built yet because the
obvious ways of building it are all wrong:

* a desktop client holding an API token for your tracker would be standing
  credentials into your systems, on every user's machine;
* relaying through the operator would make it a party to the content **and**
  give it credentials to third-party trackers.

Neither is a thing this project will ship. What it will look at is the shape
that keeps the reporter in the loop — they still press send, nothing holds a
credential it should not, and you receive a case rather than a paste. Until
then, running an A2A endpoint yourself is the supported automatic path, and
`spec/INTEGRATING.md` says what it has to answer.

### "This works, but it should do X" — later

Everything here is built around something being **wrong**: a problem class, a
symptom a person recognises, readings that decide between published answers, an
outcome saying whether the fix worked. A person who thinks your software should
do something it does not have any of that. There is no symptom to read, nothing
on their machine decides anything, and the report that would be assembled is a
report about a machine that is behaving exactly as designed.

**A feature request is a different thing and will get a different path.** It is
written down here so it is a stated plan rather than a silence, and it is
deliberately not next: the diagnosis path is not finished, and the two obvious
shortcuts would spoil what exists.

* Filing it as an outcome would put opinions into a corpus whose value is that
  every row is a measurement. `uncovered` already means *nothing published
  covers this* — adding *and it never will, because it is not a defect* to the
  same table would make the maintainer's dashboard a wish list with readings
  attached.
* Letting a model turn a wish into a bug report is worse than nothing. It
  produces a plausible issue about a defect that does not exist, and somebody
  has to close it.

What it probably looks like: the person's own words, no readings at all, the
same anonymiser and the same consent, counted per project the way reports are
counted so a maintainer can see that forty people asked for the same thing —
and never mixed with the diagnoses. Nothing is designed yet.

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
| `docs/` | Installing, onboarding, the operator, releasing, the test cases, and [FINDINGS.md](docs/FINDINGS.md) — what was measured before this was built, and what was discarded because of it |
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
