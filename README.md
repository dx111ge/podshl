# PODSHL

**One support client for every project — instead of one diagnostic tool per
project, and instead of pasting logs into a forum and waiting three days.**

A project publishes three static files to the repository it already has. The
same client, on anybody's machine, reads them for every project that does: it
takes the readings that project asked for — each one shown and agreed to
separately — matches them against that project's own rules, and gives the
answer at the moment the thing broke. Nobody writes a diagnostic tool, and
nobody installs one per game, driver or application.

The mechanism in one line: support knowledge reaches the user's own agent at
the moment of need, discovered and cryptographically verified with no prior
relationship, executes on the user's machine under the user's consent, and
returns only a conclusion.

**The data never moves. The instructions cannot go stale.**

---

## Start here

| You are | Read |
|---|---|
| **An open-source maintainer** | **[ONBOARDING.md](ONBOARDING.md)** — publish three static files, run no server, pay for no model |
| **Someone with a broken machine** | **[INSTALL.md](INSTALL.md)** — install the client, and what it does and does not send |
| A developer working on this | [SERVER.md](SERVER.md) and [TESTCASES.md](TESTCASES.md), then *Run it* below |

---

## What each side gets

### If you maintain an open-source project

* **Once people use the client, the problems they hit start reaching you — with
  nobody opening a ticket.** Each time somebody works through a problem with your
  project, what their machine read, what they told it, and whether your answer
  helped can come back to your dashboard. It shows you which setups break, which
  of your answers do not work, and which problems nothing you published covers
  yet: the gaps in your install and setup instructions, from the people who fell
  into them. Only what each person agrees to send, after they tried; names,
  addresses and tokens taken out on their machine; and nothing shown until five
  different people report the same thing.
* **No server and no model.** Three static files in your repository —
  `.podshl/agent.yaml` and the solutions it names. Nothing to host, nothing to
  pay for, nothing to keep running. They stay yours: they are in the
  repository, in every clone, and they outlive us.
* **The round trip stops.** What you would have asked for — which card, which
  driver, which version, what the log said — is read on the user's machine
  before you are involved at all, against a list *you* wrote.
* **You answer with rules, not prose.** `answers.when` is matched exactly, so
  the same configuration gets the same answer every time and you can see which
  fact decided it.
* **It is yours alone.** The dashboard is private to whoever controls the
  domain. There is no public ranking and no route that shows your figures to
  anyone else.

Start at **[ONBOARDING.md](ONBOARDING.md)**.

### If your machine is broken

* **An answer now**, from the people who wrote the software, instead of a
  multi-day round trip or a forum thread about a different version.
* **Nothing is read or sent without you seeing it first.** Three separate
  consents — read, send, change — each in plain language, each showing the
  actual values. The refusing button holds focus, so a stray Return can never
  agree to anything.
* **What leaves is what you were shown**, with your account name, addresses,
  tokens and timestamps already taken out of it, and still editable.
* **A change is shown, dry-run, and reversible.** The client will only do three
  things at all — state a finding, set a key in a configuration file, or put
  back the copy it made first — because a publisher may choose an operation and
  may not invent one.
* **It is worth having with nobody participating.** If a project has published
  nothing, the readings still stop at your machine and any model involved is
  one you chose and can run locally.

Start at **[INSTALL.md](INSTALL.md)**.

### What it is not, yet

Honesty is the product here, so: this is an **early beta**. One operator runs at
**https://sdota.de**, on its maintainer's own hardware; no project has adopted
it yet; the client packages for Windows, Linux and macOS are **not signed**, so
each system will warn before running them. The whole path has been walked end to
end on a real machine against a local operator. `TESTCASES.md` and
`TESTCASES-SERVER.md` say, case by case, what is verified and what is `open`.

---

## The problem

The largest problem in support is not missing knowledge. It is **not knowing
what is on the machine** — today answered by asking the user to run `dxdiag` or
paste `journalctl` output, over a multi-day round trip, and answered wrong.

Measured, not assumed: **82.4 % of 20,321 German ticket resolutions contain
nothing actionable** ([FINDINGS.md](FINDINGS.md)). The knowledge is not in the
tickets. It is in the maintainers' heads, and it never reaches the user at the
moment it would help.

## How it works

```
   maintainer                     operator                      user
   ----------                     --------                      ----
   .podshl/agent.yaml   --ingest-->  mirror + log   --fetch-->  client
   .podshl/solutions/                (verified,                    |
   on any static host                 with commit)                 |
                                                                   v
                                                        reads locally, under
                                                        per-item consent
                                                                   |
                                   <--readings, after a transmit-----+
                                      consent naming the operator
                                      and showing the values
                                   walks answers.when exactly,
                                   stores nothing
                                   -----the fix, or "I still need X"-->
                                                                   |
                                                                   v
                                                        shows the fix
                                                        dry-runs, then applies
                                                                   |
                                        <--optional report---------+
                                          (coarsened readings, pseudonym;
                                           free text only on its own consent)
```

Matching is **rule evaluation over closed vocabularies**, not generation. That
is why a maintainer needs no model and no server, and why a user with no model
configured still gets the full path. Where it happens depends on who published:
for a project's files it is the operator's `POST /diagnose`, for a vendor it is
the vendor's own endpoint, and in both cases the readings travel there as read,
after a consent that names the recipient and shows the values. The operator
uses them to walk the project's rules and does not keep them. Only where nobody
published anything does a model see them, and it is then the one the user
chose.

## Why it is safe

**The publisher may only choose, never invent.** The action vocabulary and the
read vocabulary both live on the client. A publisher names an operation and
fills declared parameters; it cannot ship a capability. A hallucinating or
compromised publisher can mis-parameterise a tested operation — which validation
and the dry-run catch — but cannot introduce one.

**Nothing happens without being shown first.** Read, transmit, change: three
separate consents, in plain language, with the evidence one disclosure behind.
The *refusing* button holds focus, so a stray Return can never grant. Whatever
the user withholds becomes a question rather than a dead end.

**Unknown is not untrusted.** A project that publishes a perfectly good agent
without ever hearing of us still works. Anything else would make this a
chokepoint rather than a participant.

### What the operator holds, and never holds

The security argument is one question — *what does an attacker get who owns our
infrastructure?* — and the answer has to stay boring. The running server reports
it at `/`:

| Holds | Never holds |
|---|---|
| A public attestation log | Credentials to anyone's system |
| A mirror of public repository content, with the commit it came from | User identities |
| Anonymous cluster counters | Query logs |
| | The content of an enterprise diagnosis |

Counting is **distinct people, not submissions** — the same client reporting five
times counts once — and nothing surfaces below five distinct reporters, because
a rare constellation is identifying. Epoch salts and the log's signing key are
deliberately *not* in the database: a column would be in every base backup and
every WAL archive, so a salt that is meant to be destroyed when its epoch ends
would survive in the archive. Today no worker rolls the epoch — the salt files
stay until an operator removes them — and observation rows are kept until
deleted, so that promise is a property of the design and not yet of the running
service.

## Nobody is asked to trust a model

Where a project has published, a generative model is not in the path at all.
Its answers are its own sentences, matched against a closed vocabulary, so
there is nothing to invent: the same readings get the same answer, and the
answer names the fact that decided it. That is not a position on models — it
is that an answer nobody wrote is an answer nobody can be asked about, and
support is where that costs the most.

*A model is optional* below says what happens in the one case where nobody
published, and what it costs.

Two figures a maintainer sees are never anybody else's, either: no public
ranking, ever, and no per-project figure shown to another project.

## Ports

| Port | What | Public? |
|---|---|---|
| `8721` | Vendor support agent — the counterparty | demo |
| `8722` | A plain website, no agent — the negative case | demo |
| `8723` | The responsiveness index | demo |
| `8725` | **The operator server** | yes |
| `8726` | **The operator's own view** | **no — separate app, separate listener** |
| `8727` | An example OSS project, serving `.podshl/` | demo |

`:8726` is a different ASGI application rather than a guarded route: one
project's figures are refused because *no path exists to produce them*, which is
only true if the code cannot be reached. Do not route it publicly.

`:8724` was the catch-all and is gone — it served an unauthenticated per-project
gap report, which [SERVER.md](SERVER.md) names as the line whose crossing ends
the company.

## The pages on `:8725`

The operator server answers a browser as well as a machine. `GET /` is
content-negotiated — the page to `Accept: text/html`, the JSON self-description
to anything else, and `Vary: Accept` on both so a cache cannot serve one to the
other.

| | |
|---|---|
| `/` | Why this exists: the measured finding, why written instructions rot, why bug reports are disappearing into assistants, and how the three sides fit together |
| `/publish` | What a maintainer writes, with **both** worked examples served as the same bytes `spec/` ships — the second answers the question that actually stops people: whether you may publish fixes for software you did not write |
| `/register` | Claim a host, say where your files are, and restore a lost token. A claim has two halves — a digest you publish and a preimage you keep — because a public file shows that *somebody* controls a host and cannot show that the person asking is that somebody |
| `/dashboard` | A maintainer's own project, in sentences: what recurs, how many people, whether their own answer worked, and what was measured as against what somebody typed. Host in the URL fragment, token in a header |
| `/security` | What the client can and cannot do to a machine, and what travels |
| `/projects` | Who publishes. Names and problem classes, never a figure |
| `/log` | The transparency log, with the head to pin. No search box, deliberately |
| `/notice` | Notice and action |
| `/imprint` | 503 until an operator identity is configured — see `.env.example` |

They are static files, one explicit route each, fetching the same JSON the
client fetches. **Not a `StaticFiles` mount:** a mount collapses its subtree
into one route, and the cases that prove no path produces another vendor's
figures work by enumerating the route table.

`scripts/screenshot_pages.sh` opens them on a virtual screen and photographs
them. That is not a nicety — these pages carry a Content-Security-Policy naming
a hash per inline script, and a wrong hash blocks the script *silently*: the
layout draws and nothing fills in. Three separate visual defects this session
were invisible in the markup and obvious in a screenshot.

## Run it

```bash
mise run services     # counterparty: :8721 :8722 :8723 · OSS project :8727
mise run db           # the server's Postgres cluster, in var/
mise run migrate      # its schema
mise run server       # the operator server :8725  (ops view :8726)
mise run client       # the native client
mise run client-test  # the client suite, against the shipped binary
mise run testcases    # all 272 cases; fails if the docs drift
mise run demo         # the whole argument in five acts
mise run doctor       # what the client can actually do on this machine
mise run release      # the Linux artefacts, checksummed — see RELEASING.md
mise run trust-stub   # var/ans_stub.json, the out-of-band key
```

### On a machine that is not Linux

`compose.yaml` puts the whole Linux side — Postgres 18, the operator server, the
counterparty and the Rust toolchain — in one container, so one command still
answers for the whole list:

```bash
docker compose -f compose.yaml -f compose.gpu.yaml up -d
docker compose -f compose.yaml -f compose.gpu.yaml run --rm --no-deps podshl testcases
docker compose -f compose.yaml -f compose.gpu.yaml run --rm --no-deps podshl release
```

One container rather than several on purpose: the suite reaches the counterparty
over loopback and `PODSHL_ALLOW_LOOPBACK` exists so the crawler may follow it
there. Split apart, "loopback" stops meaning what those cases mean by it. The GPU
override is separate because a machine without an NVIDIA device cannot start a
service that reserves one — and `C1` needs one, because it proves the consented
read differs from the withheld one by actually reading `gpu.name`.

The client is deliberately **not** in the container. It is a native binary on the
user's own machine, which is the claim. On Windows it is started with

```powershell
cd client-rs; cargo build; cd ..
pwsh scripts\run_client.ps1                 # add -Release, or -DebugPort 9333
```

rather than by hand, and the reason is small but wearing: a **debug** build is a
console subsystem binary — `main.rs` asks for `windows_subsystem = "windows"`
only under `not(debug_assertions)`, so that stdout is there while developing —
and its console opens *in front of* the window you meant to look at. The script
starts the console minimised and raises the application window. A release
binary has no console and the same script starts it the same way.

## A model is optional

For any project that has published support files, everything is matched against
rules — on the operator's endpoint, against readings the user agreed to send —
and no model is involved. A model is reached in exactly one case: **nobody
published anything**, and there is then nobody to own what a model would
produce, so it falls to one the user chose. On that path the readings go to the
model provider the user picked, and a report afterwards carries them to the
operator coarsened, under the same generalisation policy as every other report.

| | |
|---|---|
| **Nothing configured** | A full path, not a degraded one: readings, the overview, and a structured report to attach to an issue |
| **Free cloud tier** | GitHub Models, Google AI Studio, Cerebras, Groq, Mistral, OpenRouter `:free` — presets, a free key, no card |
| **Local** | Ollama, LM Studio, llama.cpp — the only option where the question stays on the device |
| **Paid cloud** | Anthropic, OpenAI, DeepSeek, Together |

A cloud model means the question *and the readings* do leave the device, to the
provider the user picked. The settings screen says so rather than letting "your
own model" imply "stays local". Keys go to the OS credential store, never to a
file, and a key is only ever sent to the host its preset names.

## What is here

| | |
|---|---|
| `client-rs/` | **The client.** Rust + Tauri v2, a binary of under 7 MB |
| `spec/` | **The protocol.** Prose, both normative vocabularies, two worked examples, an integration guide, and a log monitor |
| `src/podshl/server/` | **The operator side.** A signed RFC 6962 transparency log, anchor verification that distinguishes silence from absence, and counting that counts people |
| `src/podshl/` | The counterparty — a vendor's own endpoint, the index, an example OSS project — and the conformance gate |
| `run_testcases.py` | The suite. Case ids map 1:1 to `TESTCASES.md` and `TESTCASES-SERVER.md` |

## Read in this order

1. **[FINDINGS.md](FINDINGS.md)** — what was measured, and the three things that
   were discarded because of it. Start here: it explains why the rest exists.
2. **[spec/SPEC.md](spec/SPEC.md)** — the protocol itself, and the two
   vocabularies that bound what may be asked for.
   [spec/INTEGRATING.md](spec/INTEGRATING.md) is the normative publisher's side;
   [ONBOARDING.md](ONBOARDING.md) is the friendly one.
3. **[SERVER.md](SERVER.md)** — the operator side, built around one security
   principle, and what it may hold. `src/podshl/server/` is the part of it that
   runs; [deploy/](deploy/README.md) is how it is run.
4. **[TESTCASES.md](TESTCASES.md)** and
   **[TESTCASES-SERVER.md](TESTCASES-SERVER.md)** — every case, with what
   *should* happen rather than what the code does today.
5. [examples/engram/](examples/engram/README.md) — a real project taken from
   nothing to a dashboard, with screenshots and screencasts.

[RELEASING.md](RELEASING.md) says how the clients are built and what each build
has and has not been tested on. [PUBLISHING.md](PUBLISHING.md) says what is in
this repository and what is not.

## Licence, and reporting a finding

**AGPL-3.0** for the code, **Apache-2.0** for `spec/` code and schemas,
**CC-BY-4.0** for its prose. Two licences on purpose: a specification has to be
implementable without infecting the implementation, or participating in this
protocol becomes a licensing decision before it is a technical one — and the
whole design rests on publishing three static files being free. [LICENSING.md](LICENSING.md)
says where the boundary runs and why; the name is a trademark and that is the
real control point.

**No warranty.** All of this is provided as is, without warranty of any kind.
Support content is written by the projects themselves — we mirror what a project
publishes in its own repository, we do not author or review it, and the project
is named in every answer so you can see whose advice you are taking. The client
shows, dry-runs and asks before every change, which bounds the *effect* and
promises nothing about the *outcome*. Back up what matters before acting on
advice, from here or anywhere.

Found something? [SECURITY.md](SECURITY.md). This client reads a stranger's
machine and changes things on it under instructions fetched from a third party,
which is why that document lists what is actually worth attacking rather than
asking you to guess. There is no bounty, and saying so is more useful than a
page implying otherwise.
