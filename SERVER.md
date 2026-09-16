# The server

Companion to the client in `client-rs/` (what runs on the user's machine) and
the protocol in `spec/`. This is the other side of the
wire: what PODSHL operates, what it may hold, and what it must never be able to
do.

Status: **built.** This was written as architecture before the server existed;
the code is `src/podshl/server/`, and the cases at the end run against a real
database as `TESTCASES-SERVER.md`. Where the prose below still speaks in the
future tense, the case table is the authority.

## One principle

> **We hold no credentials, write into no foreign system, and see no user data.
> What we assert is publicly verifiable; what we serve is a mirror with
> provenance.**

Everything else follows from that sentence, and where a feature cannot be built
inside it, it is not built. The security argument is a single question — *what
does an attacker get who owns our infrastructure?* — and the answer has to stay
"a public log, a mirror of public repositories, and some anonymous counters".

## Three branches

| | Enterprise, own agent | OSS, hosted endpoint | Nobody there |
|---|---|---|---|
| **Anchor** | DNS TXT + Legal Entity Identifier | Challenge file at a controlled URL | none |
| **Card** | Self-signed, key from DNS | From `agent.yaml`, signed by us, logged | none |
| **Endpoint** | Theirs — **we are not in the path** | Ours, served from our mirror | none |
| **Solutions** | With them, never leave | Their repository; we hold pointers | none |
| **What accumulates here** | Nothing | Clusters and counters | Clusters and counters |

The third column is not a failure state. It is **today's entire market**, and it
is where the case for participating gets made.

## What we hold, and what we cannot

**Held:** public attestations, a mirror of public repository content with the
commit it came from, anonymous cluster counters, and — only where a user
explicitly asked for it — a free-text description they chose to contribute.

**Never held:** credentials to anyone's system, user identities, query logs,
the content of an enterprise diagnosis.

**Separated:** billing carries real personal data and lives in its own system.
Its only output into the public side is a tier flag, which is itself logged and
therefore auditable.

**The compromise test.** An attacker who owns everything gets a log that was
already public, a mirror of repositories that were already public, and counters
that identify nobody. They gain no write access to any customer system, because
none exists.

## Attestation

### Anchors

An anchor is **a location the claimant demonstrably controls** and from which we
can fetch a file. A git forge, a plain web server, a domain. "Repository" is a
recommendation, not a requirement — git is preferred only because it brings
history, review and pull requests with it.

This is deliberately also the storage requirement: **anyone who cannot serve a
challenge file cannot be verified anyway**, so there is no separate "developer
without a repository" case to solve. The bar is a single static file.

> **Open, and the code does not do this yet — 2026-09-14.** The paragraph above
> names a git forge first, and the implementation cannot anchor one. A claim
> stores `value = https://<host>/` and asks for the challenge at that host's
> root, and `anchor_host_is_punycode` admits nothing but a bare hostname — so
> the only anchor that can exist is a **domain**. A maintainer whose project is
> `github.com/owner/repo` cannot publish, because nobody can write
> `https://github.com/.well-known/podshl-challenge`. That is most of the
> audience this branch exists for.
>
> What is being built instead: the anchor's identity is the full `owner/repo`,
> which the forge does guarantee is unique; the challenge is a file **inside the
> repository**, fetched through the forge's raw URL; and **no name is ever
> claimed**, because a name is a word in common use rather than property —
> GitHub holds 1872 repositories with `engram` in the name. The index therefore
> shows candidates and does not pick one, ordered by relevance to the problem
> the person actually has, then by corroboration as a **weight and never a
> gate**, then by this operator's own distinct-reporter counts, then by the age
> of the anchor. Stars and downloads appear nowhere: ranking the measured
> example by stars puts the wrong project first.
>
> **And the trust story is not the same for the two.** A domain holds without a
> third party — DNS and TLS say who served the bytes. A forge path does not: the
> forge decides who may write there, so for a repo anchor the forge is a trusted
> third party and an anchor is no better than its word. That difference has to
> reach the user rather than hiding behind one sentence about control being
> confirmed. `SECURITY.md` carries what is a finding here and what is not.

| | OSS | Enterprise |
|---|---|---|
| Challenge | File at the controlled URL | **DNS TXT** |
| Why | Lowest possible barrier | A DNS record is far harder to obtain than a write into `/.well-known/` — subdomain takeover, a CI job or a CDN misconfiguration all yield the latter |
| Identity | The verified URL, verbatim | The Legal Entity Identifier, from the register |

### No free-text fields, ever

The impersonation attack is *"I register the name NVIDIA and point it at some
other JSON."* It is prevented by removing the fields it needs:

* The display name **is** the verified anchor. For enterprise it is the legal
  name from the LEI register. There is nowhere to type "NVIDIA".
* The endpoint **must lie under the verified anchor**. A card for `example.org`
  pointing at `google.com` is rejected at ingest, not rendered with a warning.
* A brand name resolves only from the apex domain, and only through LEI-backed
  enterprise verification.

Search therefore returns *anchors that name themselves*. `evil-nvidia-fake.io`
appears as `evil-nvidia-fake.io`, and the client already states how it arrived
at each match.

### The log, and why it is a log

Attestations are published as an **append-only, signed transparency log**, not
as a lookup API. Two reasons, and the second is the important one:

1. **No surveillance.** A per-request `GET /verify?domain=…` would tell us which
   software every client runs — precisely the profile we refuse to let vendors
   build, gathered from everyone at once. The log is fetched whole, from a CDN.
2. **Our own honesty is checkable.** A domain owner monitoring the log sees any
   attestation claiming their domain, including one we should not have issued.
   Certificate Transparency solved this exact problem; we are not going to
   improve on it.

### Three states, and the middle one is load-bearing

| State | Means | Client |
|---|---|---|
| **attested** | We verified control, and for enterprise the legal identity | Full trust signal shown |
| **unknown** | Never registered. **Says nothing about them.** | Proceeds on the protocol's own trust — signed card, key from DNS — with the status shown plainly |
| **revoked** | We attested and withdrew **for cause**: compromise or abuse | Refused |

### Anchor liveness and project liveness are different things

Two questions get confused because both end in *"is this still any good?"*:

| | Asks | Signal |
|---|---|---|
| **Anchor liveness** | Does the claimant still control the location? | Challenge file, DNS TXT |
| **Project liveness** | Has anything moved in the project? | `last_commit`, forge state, the developer's own word |

They are independent. A repository can be actively maintained while a check
fails transiently, and an anchor can verify perfectly for a project nobody has
touched since 2021. **One is a security property, the other is an age.**

#### The cadence is not the security property

Re-verification rides along with ingest — the challenge file is fetched in the
same conditional `GET` pass as the manifest, so it costs one extra request per
project per cycle and needs no separate crawler. For enterprise it is a DNS
query plus a bulk LEI comparison against the register's published files.

But checking more often does not catch a **takeover**. Whoever acquires an
expired domain can rewrite the challenge file and set the TXT record; the anchor
then verifies correctly at any cadence. What they cannot do is produce the
private key, because cards are signed by the vendor and not by us. A takeover
therefore has to register a **new** key against an existing anchor, and that
appends a visible entry to the log.

**Key continuity is the tripwire; the cadence only catches abandonment.** A key
change at a live anchor is either legitimate rotation or a takeover, and we
cannot tell which — so we do not block, we show it: *"the signing identity for
this vendor changed on <date>"*. The same line as `unknown != blocked`.

#### Losing an anchor is not misconduct

| Elapsed | State | Effect |
|---|---|---|
| One failed check | — | Nothing. Retry next cycle. |
| >= 14 days | `stale` | Still served. The client shows when control was last confirmed. |
| >= 90 days | `unknown` | Serving stops. The transition goes into the log. |

It never becomes `revoked`. Revocation is an accusation, and someone who simply
stopped working on something has done nothing wrong. The last known good content
keeps being served through `stale` because an abandoned project's solutions are
usually still correct — the software did not change either — and deleting them
helps nobody. What the user needs is the age, not a refusal.

#### Deprecation is declared, not deduced

For OSS, saying so is the ecosystem norm. Three signals, and the order matters:

1. **The developer declares it** — `status: deprecated` in `.podshl/agent.yaml`,
   optionally naming a successor. Authoritative.
2. **The forge reports it archived.** An explicit act by the owner, so nearly as
   good, and worth reading rather than guessing around. The schema has the
   column (`source.forge_archived`) and `/mirror` serves it; nothing fills it
   yet, because reading a forge is not built.
3. **We observe the age.** `last_commit` two years back is an *observation* —
   and for the same reason it is not yet observed.

The third never gets phrased as the first. *"No change since March 2021"* is
something we measured; *"outdated"* is a verdict we have no standing to reach,
and dressing the one up as the other is exactly the plausible-looking assertion
this project refuses elsewhere. A dormant solution is not a wrong solution.

**If "unknown" behaved like "blocked", we would be a chokepoint rather than a
participant**, and a vendor who publishes a perfectly good A2A agent without
ever hearing of us would be broken by us. Discovery must work without us:
ANS/DNS first, well-known probing second, our directory only as a hint that
labels itself as one.

Which raises the fair question of what enterprise pays for. Not access — access
cannot be withheld. It pays for the identity display at the moment someone
decides whether to let a stranger's agent read their machine, for protection
against impersonation, and for the data about their own products.

## The OSS path

An OSS developer has a repository. They usually have **no domain, no server and
no signing key**, and a design that requires any of those excludes the supply
side the whole strategy depends on.

* **They publish `.podshl/agent.yaml`** at their anchor: identity, the problem
  classes they handle, what to collect for each, the endpoint, the escalation
  declaration, and the list of solution files.
* **We sign the card** and log it. The attestation is precise about what it
  claims: not *"this skill is safe"* but *"published by whoever controlled this
  anchor at time T, from commit C"*. That is a weaker statement than the
  enterprise one, and correctly so.
* **We serve the endpoint** — correlation and answers — from our mirror.

### Where solutions live

**In the developer's repository.** We hold pointers.

If the solutions lived with us we would own the knowledge, which is poison for a
product whose adoption depends on the open-source community. In the repository
they gain four properties we could not otherwise give them: no lock-in (the
knowledge survives us in every clone), **pull requests** (other people can
contribute solutions, which is how open source actually scales), versioning
alongside the code that needs them, and review — a solution that proposes
actions on other people's machines passes the same review as code.

### Format

```
.podshl/
  agent.yaml              ← card: identity, problem classes, what to collect,
                            endpoint, escalation, and the list of solutions
  solutions/
    flicker-scrolling.md
```

Fetched as **plain HTTPS**: the manifest, then the files it lists. No forge API,
so GitHub, GitLab, Codeberg and a bare nginx all work identically.

A solution keeps its machine part small and its human part readable, so that one
solution is one pull request:

```markdown
---
id: flicker-scrolling
answers:
  problem_class: display.flicker
  when: { app.version: "< 1.4.0" }
severity: medium
proposes:
  - action: set_config_key
    params: { file: settings.toml, key: vsync, value: "on" }
---
Until 1.3.2 VSync was not set on multi-monitor systems. Fixed in 1.4.0;
until then the setting above works around it.
```

The `answers` block is **applicability metadata, not a rule engine** on the
client. Nothing is downloaded and evaluated there; the client sends its consented
readings to `POST /diagnose`, and matching happens on our side as an **exact
walk** of the tree derived from those blocks — along the path the readings
select, asking for the fact it still needs rather than guessing at the nearest
branch. The readings walk the tree and are not stored.

### Validation is a gate, not a hope

The validator runs **at ingest**, not only in the developer's CI. CI is a
convenience that lets a bad pull request fail before merge; ingest is the
boundary. It checks the schema, that every proposed action exists in the
client's vocabulary, that every referenced reading exists in the catalogue, and
that English is present.

## The enterprise path

They run their own endpoint, so the interpretation — which is their moat — never
leaves them. We attest identity and nothing else. **There is no fallback to us
when their endpoint is down**: silently taking over their role would mean
answering for a vendor with knowledge we do not have.

The asymmetry between the paths is not an inconsistency. Both apply the same
question — *what is actually valuable here?* — and get opposite answers. An
open-source project has no interpretation to protect, so it publishes; a vendor
does, so it keeps it.

## The anonymous branch, and the dashboard

Where nobody publishes an agent, observations still accumulate — keyed by the
domain the user named, and by the product strings that came out of the readings.

**No free text by default.** An earlier draft of the catch-all took a truncated
problem sentence as the class, which breaks the rule the rest of the design
obeys. The class is derived from the **signature**: two reports with the same
readings and the same failed actions are the same class. A description may
travel, but only under its own explicit consent and with the destination named.

### What the vendor sees, and how they get to see it

**Claiming your own domain unlocks the dashboard about yourself, free.** The
same challenge as any other anchor. No payment, because this is the gift, and
the moment it feels like *"pay or we withhold your own defect list"* the company
is finished.

The dashboard shows clusters by frequency, each with its coarse configuration,
what was tried, what failed, and the distribution of model classes — which is
the load-bearing number: **a small local model solving something from public
knowledge means the information was available and the product surface failed to
convey it.** That is a UX defect, not a knowledge gap.

And it states its own limit, because that limit is the honest sales argument:
without a connected agent it shows *that* something recurs, never *why*.

### What is public and what is not

| | |
|---|---|
| **Public** | The attestation log — a **positive list** of who participates. An absent name is not an accusation. |
| **Private, free** | The gap dashboard for a claimed domain |
| **Outreach** | We may put a report in front of a vendor unsolicited — free, confidential, unconditional |
| **Never public** | Any per-vendor defect list |

### Whom to approach — the operator's own view

Running the commons means we can see, per unclaimed domain, how much has piled
up. That is not a contradiction of the rule above: **private to the public is
not invisible to the operator.** The rule was that no per-vendor defect list is
ever *published*.

This is the outreach list, and it prioritises itself. The domain with the most
accumulated observations has the most users in pain, is the one where a report
lands hardest, and is therefore both the best sales call and the most deserving
of one — the signal and the value are the same number. It doubles as the
capacity-planning figure, since it is also where the traffic is.

What legitimises using it is what we carry when we go: **their own report,
free, confidential and unconditional.** A user who reported did so hoping
something would improve; us delivering that report to the vendor is the
mechanism of that hope, not a repurposing of it. The client already says as
much before anything is sent.

Two lines that must not be crossed, because crossing either ends the company
rather than costing it a deal:

* **One vendor's numbers are never shown to anyone else.** Not to a competitor,
  not in a pitch, not anonymised-but-guessable.
* **No public ranking, ever.** *"These vendors are the worst"* is the
  extortion suspicion in a chart.

Publicly there is exactly one figure: **ecosystem totals** — how many products
are observed and how many have a connected agent. No per-vendor breakdown. It
shows the size of the gap without naming anybody, which is the only version of
this number that helps rather than threatens.

## Query and report are different acts

Asking *"does anyone know this?"* and saying *"this happened to me"* carry the
same payload — the coarsened signature — and must not be the same decision.

* **Query** is ephemeral. It touches **no store at all**; abuse protection is a
  counter per pseudonym, never a record of what was asked.
* **Report** is durable and counted, and the user chooses it **after** the
  answer. That ordering is not a detail: a report filed afterwards can carry
  *whether it worked*, which is the outcome label that makes the whole corpus
  worth having.
* Consequently the counter reads **"47 reported"**, never "47 occurred". It
  under-reports reality and the dashboard has to say so.

### The lookup is one step, and it carries no figure

`POST /diagnose` with the facts, which the server walks down the project's
decision tree — returning an answer, a `need`, or nothing. **No fuzzy matching
happens here**; the tree is walked exactly, and where it cannot proceed it asks
rather than guesses. It runs in a read-only transaction the database enforces
(`SV12`), so "queries are not counted" is a property of the connection rather
than a policy.

**There was a first step, and it was removed.** `GET /cluster/<hash>`, the hash
of the exact canonical signature, was meant to be served from a CDN so that
common queries never reached the origin. It answered how many people had
reported that configuration about that project, without authentication — and a
signature is a host and a few coarsened facts, so its hash is a small search
rather than a secret. That is the per-project figure the dashboard keeps
private, readable one guess at a time (`SV104`). No client ever called it. If
caching in front of a host comes back, it comes back as a cacheable answer with
no count in it.

## Similarity: a switch, not a metric

Grouping "the same problem" across configurations that differ slightly is the
one piece that looked genuinely hard. It is not, because the question is wrong.

Both failure directions are expensive. Merge too eagerly and two distinct bugs
share a cluster, the solution is wrong for half of them, and the counter lies.
Split too eagerly and forty-seven users become forty-seven clusters of one,
nothing crosses the k-threshold, and nobody ever sees anything. A threshold on a
fuzzy score gets to be wrong in both directions at once, and cannot explain
itself either way.

**So do not estimate whether A and B are the same. Acquire the fact that
decides it.**

That fact is either a **reading** or a **question for the user** — and both
already exist in the client, with consent, validation and the round loop. There
is nothing new to build there.

### A cluster is a decision tree, not a bag

```
display.flicker
├─ gpu.driver_version < 610 ?
│  ├─ yes → solution A
│  └─ no  → how many monitors ?        ← asked of the user
│           ├─ 1          → solution B
│           ├─ more       → solution C
│           └─ don't know → falls back to the parent
```

The endpoint therefore has a **third outcome** beside "finding" and "no
statement": **"I still need X."** The client reads or asks, and sends again.

That is the same iterative loop it already runs with the local model, now driven
by the vendor's tree instead — which means **a vendor's endpoint may ask
follow-up questions exactly as the local model does**, and the client needs one
mechanism rather than two.

*Client change this requires: the diagnose response gains a `need: [Probe]`
outcome, and the vendor path reuses the existing round loop instead of a single
shot.*

### Two rules that come out of use rather than theory

**"Don't know" is a valid answer and must never dead-end.** Not everyone knows
whether their card is overclocked. That branch falls back to the parent node and
takes whatever answer applies there.

**A switch on an already-collected fact is worth more than a new question.** A
switch on `gpu.driver_version` **re-partitions history**, because the value is
already in every stored signature. A new question only works forward.

That rule is now enforced rather than advised: the tree is **derived** from the
solutions' own `answers.when` conditions, and the derivation prefers a switch on
a reading over a switch on a question. A publisher therefore authors no tree at
all, which is what makes a diagnosis available to every project that has already
published — and means the two documents cannot drift, because there is only one.

One subtlety the derivation had to get right. A probe may carry *both* a reading
and a question, and whether such a switch is decided by reading or by asking is
a property of the **values branched on**, not of the probe: a value drawn from
the probe's own `choices` is one only the question can produce. Deciding it from
the probe alone would call a switch a reading that no reading can ever satisfy.

### A dashboard is for an anchor, and on a forge that is not a host

A report carries its subject as the client sends it: the **repository URL** for a
repository anchor, the bare host for a domain one. The dashboard looked up
clusters by the host in its own path, so on a forge it looked for `github.com` —
a string no report has ever carried. Every report about a project on a forge was
stored correctly, counted correctly, and shown to nobody; the maintainer's own
page said nothing recurs while their clusters sat in the table.

Matching the bare host would be the opposite mistake and the worse one: every
project on that forge shares it, so *no route produces another vendor's figures*
would fall to whoever claimed a repository there first. The subject comes from
the claimed anchor, and `SV122` checks both directions — its own reports arrive,
the neighbour's do not.

### Where a switch is missing announces itself

> **A cluster with a solution whose reports say it worked for some and not
> others is two problems.**

The outcome label exists precisely because the report comes *after* the attempt,
and it points at the place a distinction is missing. The failure of a solution
is the signal for where the tree needs to fork.

**And `uncovered` is the label for no solution at all.** The other four are
about an answer: it worked, it did not, it went to a person, the walk declined
to state anything. A run that reached the end of what a project published and
found nothing had no word, so it could not be reported — and `/diagnose` runs in
a read-only transaction, so asking leaves no trace either. The gap was the one
event a maintainer could never learn about. Two things arrive under it and
deliberately share a label, because a maintainer reading the dashboard is
answering one question — *is there an answer here I have not written?* — but
they are told apart by what they carry: rules that matched nothing were reached
*after* the readings, so that report holds the constellation they failed on,
while a person saying "none of these is my problem" says so at the class picker,
before anything is read, so that report holds no readings at all.

### A solution that ignores a switch still applies under every value of it

A branch is grown for each value some rule *names*. A rule that says nothing
about the switch applies whatever the value is — and it used to live only inside
the branches other rules had created, so a reading outside those fell off the
tree and took that answer with it. Published, mirrored, signed, and unreachable.

engram found it on somebody's desktop on 2026-09-16. Its answer for the wrong
archive names the operating system and the download and says nothing about the
processor; another rule names `os.arch: aarch64`. So `os.arch` became the switch,
grew one child, and on an ordinary x86_64 Linux machine holding the Windows
archive nothing matched — the walk stopped at the answer above and the person was
told to go and find out which archive they had.

Each node now ends with one more child, `any`, carrying exactly the rules that
did not constrain the switch. It is always last, so a named value is always
preferred, and `validate_tree` refuses a tree where it is not.

### Fuzziness belongs in the authoring tool, never at runtime

| | |
|---|---|
| **Runtime** | Exact, along the path actually walked. A wrong guess would reach a user. |
| **Authoring tool** | As fuzzy as we like — *"200 singletons that look related, merge?"* — because a human approves it |

### Without an owner, exact only

The anonymous branch has no developer, so it has no switches: clusters are exact
signature matches. That is honest — *"these 47 have identical configurations"* —
and less useful, which is the argument for connecting stated as a property
rather than a pitch. **Similarity is curated capital, and curation needs an
owner.**

## Serving: mirror, never proxy

Fetching from a forge on the request path would be wrong twice over — their rate
limits become our capacity, and their outage becomes ours.

* **Ingest on a schedule** with conditional `GET`, and the schedule follows how
  quiet a project is. Fifteen minutes for every source is eleven requests a
  second at ten thousand projects, and almost all of it is `304` about files
  nobody has touched since last year — so the interval is a twenty-fourth of
  the time since the last change, floored at fifteen minutes and capped at a
  day. A project that changed an hour ago is still asked promptly, because right
  after a change is when the next one is likely; one untouched for a month is
  asked daily. The cap is load-bearing: anchor re-verification rides along with
  ingest and `stale` is a promise at fourteen days, which a daily floor keeps
  with a factor of fourteen to spare. `SV121`.
  A maintainer who wants a crawl now re-POSTs `/claim/{host}/source`, which
  queues an immediate one; there is no separate refresh button and no webhook.

  **Not "fetch when somebody asks."** That would break three things at once:
  their rate limits become our capacity, their outage becomes ours, and our own
  fetch log becomes a record of who asked about which project and when. Asking
  leaves no trace at the operator — `/diagnose` runs in a read-only transaction
  — and it must not start leaving one at the forge instead.
* **Serve from our database.** One indexed lookup, no external call.
* **Publish the commit** we are serving, so anyone can check the mirror against
  the source — **and our own**, which is the same sentence turned on ourselves.
  `GET /` and the footer of every page say what this operator is running, as
  `git describe --tags --always` reported at the moment the image was built.
  Baked in as a build argument rather than read at run time, so the string
  travels with the code it describes; a build nobody told says nothing rather
  than claiming a version (`W22`). Until 2026-09-16 the only way to know what an
  operator ran was to ask whoever runs it, which is the shape of claim this
  whole project exists to replace.

The property that makes this scale: **ingest load tracks the number of projects,
request load tracks the number of users, and the two are independent.**

### Serving other people's content has consequences

* The **action vocabulary remains the real defence** — a malicious solution can
  still only propose operations the client implements, with validated parameters
  and a dry-run the user sees.
* **Re-verify anchors along with ingest**, and grade the failure: `stale` at 14
  days, `unknown` at 90, never `revoked` — see *Anchor liveness and project
  liveness are different things*.
* **Follow no cross-host redirects.** Fetch the recorded URL or nothing.

## Names we do not own

We attest **control of an anchor**, never entitlement to a name. Deciding who
deserves *NVIDIA* would be the chokepoint role rejected at `unknown != blocked`.
We are not a trademark register and must not become one.

The obvious impersonation is already impossible: the display name **is** the
verified anchor, and there is no name field to abuse (`SV4`). Homoglyph anchors
are a spoofing problem with a technical answer — punycode display and confusable
detection at ingest — not a dispute. What remains is harder.

### Naming a product is not claiming to be it

A manifest declares *the problem classes they handle*, and those classes name
other people's products. A community project that fixes NVIDIA driver problems
has to be able to say so. That is nominative use, and **it is the entire OSS
branch**: projects that repair software they did not write.

So identity and subject stay separate fields — anchor verified, problem classes
declared — and nothing in the display lets a problem class read as an identity.
**A takedown reaches the anchor, never a problem class**, or a vendor could
forbid anyone from saying their name out loud.

### Jurisdiction: Germany, and no appetite for a fight

The operator is in Germany and takes no financial risk. That is not a
compromise here — it points the same way the law does. Because we mirror, we are
a hosting service: the liability privilege holds only while we have no
knowledge, and once notified we must act without delay or become liable
ourselves. **Removing on notice is the requirement, not the surrender.**

Two duties survive any risk appetite: a notice-and-action route, and a
**statement of reasons to the affected party**. The second is already built — it
is the same log entry. *(Whether the internal complaint route falls away for a
micro-enterprise is a question for a lawyer, not for this document.)*

### Therefore a takedown degrades; it does not delete

Removing on every claim without review makes the takedown path a free weapon,
and the ground it clears is the supply side the whole strategy depends on. The
answer is not courage. It is **reach**:

* **Exposure exists only where we host for others.** Enterprise clients talk to
  the vendor's own endpoint with no fallback to us (`SV9`); we hold nothing
  there, so there is nothing to remove.
* **Solutions live in the developer's repository and we are the mirror.** A
  takedown therefore reaches the mirror, not the source.
* **So it degrades `attested` to `unknown`** — precisely the state of a project
  that never registered. Not death, un-enrolment, and the client already handles
  it without anything new being built.
* **Prevention is cheaper than process.** At ingest, refuse to attest anchors
  that are confusable with a well-known mark or carry one without owning it.
  Declining to attest is not blocking, so we can be conservative here without
  becoming a chokepoint.
* **Every takedown is logged with a public reason code.** If we can remove
  things quietly, the transparency is decorative — and the duty to state reasons
  makes weaponised claims visible and countable.
* **A notice is not an action.** `POST /notice` records it as pending and
  nothing changes on receipt; a person reads it in the operator's own view on
  `:8726`, behind `PODSHL_OPS_TOKEN`, and acts from there — degrading the
  anchor, refusing the notice, or later reinstating an anchor that was
  degraded. The log entry is written when the person acts, and it is the
  statement of reasons. A route that acted on its own would be the free weapon
  above, automated.
* **A notice says that whoever filed it means it.** `notifier.statement_of_good
  _faith` must be `true` — not truthy, `true` — or the notice is refused before
  anything is recorded. The form has always asked; the route used to throw the
  answer away, which is a safeguard on one page and nowhere else. It stops
  nobody, and that is not what it is for: it makes a careless notice a
  statement somebody made rather than a button somebody pressed, and it is the
  record the operator holds when a decision is questioned a year later.

### What this does not buy

**Zero is not reachable; low is.** A German cease-and-desist costs money to
answer even when one complies at once, and a wrongly signed undertaking binds
permanently with a contractual penalty. Operating a public service here also
means a real name and postal address on the site — which is the address the
letter arrives at. Both belong in the operating plan, not in the architecture,
but neither should come as a surprise.

### Hosting abroad moves nothing

**Liability follows the person, not the machine.** The DSA attaches to the
market rather than the location: it applies to services directed at users in the
EU wherever the provider sits, and a provider established outside it has to
designate an EU representative on top. Trademark claims follow the same logic,
and a court here reaches a resident here however far away the servers are.

What changes the exposure is the **legal person**. Operating personally means
unlimited liability against private assets and a home address in the Impressum;
a limited company — a UG is the small form, one euro of capital — caps the
liability at what was put in and puts the company's address on the site instead.
A foreign entity is often sold as protection and rarely is: the EU duties
remain, and management from Germany pulls German tax law along anyway. The
numbers are a question for an accountant, not for this document.

The location does decide two things. Hosting outside the EU makes data
protection harder rather than easier, and it changes **who processes the
complaints** — a US host may act on a single notice with no procedure at all,
where an EU host works under defined ones. For staying online, hosting outside
the EU is the worse choice.

**And all of it hangs on one design decision:** we are a hosting service only
because we mirror. The enterprise branch holds nothing of ours and is
correspondingly unreachable. Dropping the mirror was rejected under *Serving:
mirror, never proxy* for capacity reasons, and that argument still holds — but
the entire legal exposure sits on that single choice, which is worth knowing if
the trade-off ever shifts.

## How many callers at once

Measured, on one developer machine: the container has four cores, the server is
one `uvicorn` worker, and the generator ran on the host so it was not competing
for the same CPU.

| | |
|---|---|
| Peak | **~1,000 requests a second**, at about four to eight requests in flight, p95 under 15 ms |
| Past that | throughput falls and latency grows — but a service in the same container that touches **no** database shows the same curve, so that ceiling is four cores behind Docker's forwarder rather than anything here |
| Failures | none at any level. It queues; it does not fall over |

A user makes a handful of requests per *diagnosis*, not per second, so request
serving is not the limit that will be hit first — the crawler and the database
are. Most of the hot routes are cacheable (`max-age=300`, and `immutable` on
anything pinned below the tree size), and the deployment puts a TLS proxy in
front, which is where that should be absorbed.

**What did matter was not throughput.** Every handler that touched the database
was `async def` with a blocking `psycopg` call inside, so the process was one
thread doing one thing and the pool's eight connections could never be more
than one. With two 0.4-second queries in flight, a trivial route served 8
requests in five seconds at a median of 783 ms; with those handlers off the
loop, 206 requests at 4 ms. One slow query froze every other caller, and a slow
query needs no bug — a lock, a cold index, a dashboard over a large project.
`SV102` holds it: a handler that awaits nothing is `def` and Starlette gives it
a worker, and one that must await the request body hands the blocking half to
`run_in_threadpool`.

## Data model

```
anchor          (id, kind[url|dns], value, host, challenge_token, verified_at,
                 last_checked, last_confirmed, status[live|stale|unknown],
                 taken_down_at, taken_down_seq)
claim_pending   (anchor_id, nonce_hash, issued_at)      -- several may stand at once;
                                                        --   verify finds one by the
                                                        --   hash of what is presented
attestation     (anchor_id, tier, subject_name, lei, key_jwk, issued_at,
                 revoked_at, log_seq)
dashboard_claim (anchor_id, token_hash, issued_at, expires_at,   -- 365 days
                 revoked_at, revoked_reason)             -- the token is never stored
source          (anchor_id, manifest_url, fetch_prefix, etag, last_modified,
                 next_fetch_at, mirror_state, declared_status, successor_url,
                 forge_archived, forge_last_commit_at)   -- the forge columns stay empty
card            (source_id, json, langs, commit, content_hash, valid_from, valid_to, log_seq)
solution        (source_id, solution_id, answers, text_by_lang, proposes,
                 severity, commit, valid_from, valid_to,
                 etag, last_modified)                    -- its own validators, so a file
                                                         --   edited in place is refetched
tree            (source_id, problem_class, …)            -- derived at ingest from
tree_node       (tree_id, …, fallback_only)              --   answers.when and collect
cluster         (id, subject_kind, subject_host, source_id, problem_class,
                 signature, signature_shape, signature_hash, first_epoch,
                 last_epoch, reports_total, reporters_this_epoch, peak_epoch_reporters)
observation     (cluster_id, epoch, model_class, ux_severity, outcome,
                 observed,                               -- the coarsened readings, verbatim
                 stated,                                 -- what a person supplied
                 description, description_consent,      -- free text, only with its consent
                 seen_key)      -- HMAC(HKDF(epoch_salt, cluster), pseudonym); no timestamp
notice          (anchor_id, received_at, reason_code, notifier, good_faith_stated,
                 acted_at, action[pending|degraded|refused|reinstated],
                 refused_reason, log_seq, reinstated_at, reinstated_seq)
                                                        -- pending until a person acts;
                                                        --   NULL good faith means filed
                                                        --   before it was required
```

`seen_key` is how the k-threshold counts **distinct** pseudonyms without
storing any: it is salted per cluster and per epoch, so it cannot be linked
across either, and once the salt file is gone it cannot be tested against any
pseudonym. `observation` is the one table that holds what people sent —
coarsened readings, list answers and, with consent, free text — and a backup of
it is a backup of that. `notice.notifier` is the only personal data on the
public side, and it is never served.

## What we do not build

Rebuilding an ITSM is the mistake [FINDINGS.md](FINDINGS.md) warns about, and it
would be a worse one than the original.

| We do | We do not |
|---|---|
| Intake — the incident arrives complete | Workflow, assignment, queues |
| Deduplication — structured, not text-similarity | SLA clocks, escalation tiers |
| The trail, including the dead ends | The ticket's state model |
| Reporting — gap and responsiveness | Time tracking, billing of work |

The ticket's state belongs to the system where the work happens. We only ask
after it — and that produces the most valuable field of all: **closed and not
reopened** is the real outcome label, rather than an agent's click.

## Test cases

Both branches, and the cases that sit between them. The server is built, and
every one of these runs against a real database from
[TESTCASES-SERVER.md](TESTCASES-SERVER.md), which also carries the cases added
since this table was written (`SV40` onwards).

| # | Case | Expected |
|---|---|---|
| SV1 | OSS anchor: challenge file present | Attested `tier: oss`, entry appended to the log |
| SV2 | Challenge file removed, one cycle | No state change; retried next cycle |
| SV2a | Challenge file gone 14 days | `stale`; still served, last confirmation shown |
| SV2b | Challenge file gone 90 days | `unknown`; serving stops, transition logged |
| SV2c | Anchor verifies but the key changed | Served, key change surfaced, never blocked |
| SV2d | `status: deprecated` in the manifest | Shown as the developer's own statement |
| SV2e | Forge reports the repository archived | Shown as an owner action |
| SV2f | No commit for two years, anchor live | Age stated as an observation, never a verdict |
| SV3 | Card whose endpoint points outside its anchor | Rejected at ingest |
| SV4 | Card with a display name that is not the anchor | Impossible — there is no such field |
| SV5 | Solution proposing an action outside the vocabulary | Rejected at ingest, not at execution |
| SV6 | Solution referencing an unknown reading | Rejected at ingest |
| SV7 | `agent.yaml` without English | Rejected — English is the one obligation |
| SV8 | Enterprise anchor via DNS TXT + LEI | Attested `tier: enterprise` with the register's name |
| SV9 | Enterprise endpoint unreachable | **No fallback to us**; the client is told the vendor is unreachable |
| SV10 | A domain we never attested | `unknown` — the client proceeds on protocol trust, never blocked |
| SV11 | Revoked domain | Refused |
| SV12 | Query | Touches no store; no record of what was asked |
| SV13 | Report | Counted once per pseudonym per epoch, never twice |
| SV14 | Fewer than k distinct pseudonyms | Cluster not surfaced to anyone |
| SV15 | Report after a solution was tried | Carries the outcome |
| SV16 | Exact-signature query | Served from the edge; never reaches the origin |
| SV17 | Forge unreachable at request time | Serving unaffected — the mirror answers |
| SV18 | Mirror content vs source | The served commit is published and verifiable |
| SV19 | Dashboard for an unclaimed domain | Not accessible to anyone |
| SV20 | Dashboard after claiming the domain | Accessible, free, private |
| SV21 | Any per-vendor defect list | Never published |
| SV22 | Free-text description | Only with its own consent, destination named |
| SV23 | The operator's outreach view | Ranks unclaimed domains by accumulated observations — internal only |
| SV24 | One vendor's figures requested by another party | Refused; no path exists to produce them |
| SV25 | Public ecosystem figures | Totals only, no per-vendor breakdown derivable |
| SV26 | A signature that reaches a node needing a switch | Endpoint answers `need`, not a guess |
| SV27 | The client answering a `need` | Same round loop as the local-model path, consented per round |
| SV28 | "Don't know" on a switch | Falls back to the parent node; never a dead end |
| SV29 | A switch added on an already-collected fact | Re-partitions stored history |
| SV30 | A switch added as a new question | Applies forward only, and the tool says so |
| SV31 | Runtime matching | Exact along the walked path — no fuzzy match ever reaches a user |
| SV32 | A cluster whose solution reports mixed outcomes | Surfaced to the developer as a missing distinction |
| SV33 | Anchor confusable with a well-known mark | Not attested at ingest; still reachable as `unknown` |
| SV34 | Anchor carrying a mark it does not own | Held, not silently attested |
| SV35 | Manifest naming another vendor's product as a problem class | Accepted — nominative use is the OSS branch |
| SV36 | Takedown notice against an OSS anchor | Mirror withdrawn, `attested` → `unknown`, source untouched |
| SV37 | Takedown notice against an enterprise vendor | Nothing to remove; we hold none of their content |
| SV38 | Any takedown | Log entry with a public reason code; affected party told why |
| SV39 | Takedown aimed at a problem class rather than an anchor | Refused — no such path exists |

## Open questions

1. **Monitoring the log.** Certificate Transparency works because third parties
   watch it. Somebody has to make watching easy, or the transparency is
   decorative.
