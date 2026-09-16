# Integrating

Two ways in, and they are genuinely different. Read the one that describes you.

|  | Open source | Enterprise |
|---|---|---|
| **What you publish** | One static file at a location you control | A DNS TXT record, and your own endpoint |
| **What proves it is you** | A challenge file at that same location | The DNS record, plus a Legal Entity Identifier |
| **Who answers a diagnosis** | We do, from a mirror of your repository | **You do. We are not in the path** |
| **Where your knowledge lives** | Your repository. We hold pointers | With you. It never leaves |
| **Cost** | Free, permanently | Paid |

The asymmetry is not an inconsistency. Both apply the same question — *what is
actually valuable here?* — and get opposite answers. An open-source project has
no interpretation to protect, so publishing costs it nothing and gains it
review, pull requests and versioning alongside the code. A vendor's
interpretation is its moat, so it keeps it.

**Neither is required to work.** Discovery does not go through us: a vendor who
publishes a valid agent without ever hearing of this still works, and a client
that cannot reach us proceeds on the protocol's own trust. Anything else would
make this a chokepoint rather than a participant.

---

# Open source

## What you get

* **A diagnosis endpoint you do not have to run.** We serve correlation and
  answers from a mirror of your repository.
* **No model, and no inference bill.** What you publish is matched, not
  generated: `problem_classes` is a closed list, every `collect:` entry names an
  op from the read vocabulary, and a solution's `answers.when` is a condition
  over readings. That is rule evaluation, and it runs on our endpoint: the
  user's client sends the readings it was allowed to take, after a second
  consent that names us and shows the values; we walk your conditions against
  them exactly and keep none of them. Nothing here asks you to host a model,
  pay for tokens, or keep a GPU alive — and a user with no model configured
  still gets the readings, the matched solution and the report. A model is
  reached only where *no* project has published anything at all, and it is
  then the user's own.
* **A dashboard about your own project**, free, showing what recurs and what
  people tried.
* **The solutions stay yours.** They live in your repository, so they survive us
  in every clone, they take pull requests from people who are not you, they are
  versioned alongside the code that needs them, and they get reviewed like code
  — which matters, because a solution proposes actions on other people's
  machines.

## What you publish

Three files, at a location you already control. A forge, a bare nginx and a
static host all work identically, because it is fetched as plain HTTPS and there
is no forge API involved.

```
.well-known/podshl-challenge      the digest we give you, and nothing else
.podshl/
  agent.yaml
  solutions/
    wheel-missing-for-python.md
```

A complete, working example is in [`example/`](example/). It is put through the
same validation as anything else, by the test suite, so it cannot quietly stop
being valid.

### `agent.yaml`

```yaml
endpoint: https://example.org/podshl/
commit: 4f2c1a9
status: active
langs: [en, de]

problem_classes:
  - id: pip.install.wheel-missing
    describes: Installing fails with "no matching distribution found"
  - id: pip.install.version-conflict
    describes: pip refuses to install, saying two packages need different versions

collect:
  - id: python.version
    kind: machine
    describes: Python version
    why: Wheels are published per Python minor version
    read: { op: run_tool, tool: python3, args: ["--version"] }

solutions:
  - solutions/wheel-missing-for-python.md
```

**There is no name field, and that is deliberate.** Your display name *is* the
anchor you proved you control, so there is nowhere for anybody to type somebody
else's. It is also why the impersonation attack does not need defending against:
`evil-nvidia-fake.io` appears as `evil-nvidia-fake.io`.

**`describes` is the line a person picks their problem by, and it is yours to
write.** The class itself is an identifier — a rule matches on it, a solution
answers it, and it travels in a report. Nobody outside your project can answer a
question asked in identifiers, and until they had `describes` clients had no
choice but to put three of them in a dropdown and ask which was yours. Say what
the person *sees*, not what is wrong: somebody who already knew it was an
architecture mismatch would not be asking. One sentence, 120 characters, English
— the answer itself belongs in a solution file, where the walk reaches it after
they have consented. A bare string is still valid, and a client then shows the
identifier.

**`problem_classes` names other people's software on purpose.** A project that
repairs `pip` behaviour has to be able to say `pip`. That is nominative use, and
it is what this branch *is* — projects that fix software they did not write. A
takedown reaches an anchor, never a problem class.

**`endpoint` must lie under your anchor.** A manifest for `example.org` pointing
at `google.com` is refused when it arrives, not shown with a warning.

**`glossary.keep` names your own terms.** You owe English and nothing more, so a
reader in another language meets your questions and your answer through their
own model — and a small model turns a product's term into the ordinary word it
resembles: engram's *brain* became *cerveau*. List such words and they reach the
reader as you wrote them:

```yaml
glossary:
  keep: [brain]
```

The client does not ask the model to keep them, because that was measured to
work only by luck of phrasing. It hides each occurrence behind a placeholder
before the text is sent and puts your word back after; a translation that lost
one says so beside it. Only `keep` is read.

**Your terms also travel in the directory, and this is why it matters to you.**
The sentence a person picks their problem by — your `class_labels` — is the
first thing of yours anybody reads, and it is translated *before* the client
fetches your card, because choosing the problem is what leads to the consent
under which your card is fetched. For a while that one sentence was therefore
the only text of yours translated with nothing kept. The directory entry now
carries `glossary.keep` beside the labels, so the terms arrive in time. Nothing
extra is published: the same words you already wrote, in the same public signed
index. A per-language rendering — *write
brain as Wissensspeicher in German* — was measured too, and the models it would
reach did not apply it, so the format has no field for one and a manifest that
writes one is refused. At most 50 terms, each one line of at most 64 characters.

### Where a case goes when nothing you published covers it

```yaml
escalate:
  reason: Nothing published matches these readings, and it may be a defect
          rather than a setup problem
  queue: github-issues
  target: https://github.com/you/your-project/issues
  reply_via: [ticket_url, none]
```

A diagnosis that finds no answer ends with **Markdown the person takes to you**:
everything they were asked and everything the machine read, anonymised, shown
before it is copied and editable first. `target` is the address shown beside it.
Without it the panel says "take this to the project" and leaves somebody holding
a finished bug report with nowhere named — which is what it did until
2026-09-16, to projects that had written the address down.

`reply_via` says what you promise. `ticket_url` means they get a link to follow;
`none` is a real answer and a better one than silence. There is deliberately no
inbound channel on their machine, so nothing here can call them back.

**This is not an automatic path into your tracker, and there is not one yet.**
The person carries it, which is why it works with any tracker at all. `README.md`
under *Your own ticket system* says what is planned and what is excluded.

### A solution

YAML front matter and a human body, so one solution is one pull request — the
machine part small enough to check at a glance, the human part readable.

```markdown
---
id: wheel-missing-for-python
answers:
  problem_class: pip.install.wheel-missing
  when:
    python.version: ">= 3.13"
severity: medium
proposes:
  - action: report_only
    params: {}
    because: The fix is to install a different Python, which the client will not do for you
---
There is no wheel for your Python version yet, so pip fell back to building
from source and the build needs a compiler you probably do not have.
```

`proposes` may only name actions the **client** implements, with parameters
matching its declared patterns. You select; you cannot ship a capability. That
is the whole safety argument, and it is why an arbitrary script would be
strictly worse — a signature over code certifies origin while granting unbounded
effect.

The current list is in [`vocabulary/actions.json`](vocabulary/actions.json).
Today it is three: `report_only`, `set_config_key`, `restore_backup`. It is
short because every entry is an operation somebody had to implement, test on
three platforms, and be willing to have run on a stranger's machine.

### What you may ask to be read

[`vocabulary/reads.json`](vocabulary/reads.json), and it is the same shape one
level down: you name an operation and fill its parameters.

Nine operations:

| Op | Reads |
|---|---|
| `os_fact` | The operating system, the architecture, its version, whether this is a container — with nothing run |
| `env_var` | One environment variable, subject to the deny list |
| `run_tool` | One tool from a short allow list of system tools, with arguments matching that tool's pattern |
| `read_file_key` | One key out of one JSON file inside a granted root |
| `read_ini_key` | One `key = value` line — `.venv/pyvenv.cfg` states the Python a project really uses |
| `read_registry` | One Windows registry value |
| `enumerate_read` | Named keys out of files matching a pattern under a named root, bounded |
| `program_version` | **Your own program's version, by asking it** |
| `container_image_version` | Which version of a Docker image is running |

A tool is on the list or it is not, and its arguments have to match a declared
pattern. `python3 --version` is a reading; `python3 -c "…"` is arbitrary
execution and is refused. There is no `pip list` either — a package inventory is
a fingerprint of the machine.

A relative `path` in `read_file_key` or `read_ini_key` is relative to the project
directory the user grants for this diagnosis, and names nothing until they do.
It is never relative to wherever the client happens to be running.

If you need a reading that is not there, that is a conversation about the
vocabulary rather than something to work around. A vocabulary that grows by
request is doing its job; one that can be bypassed is not a vocabulary.

### Your own version: ask the program, not the person

**Do not ask people which version they run.** A version somebody picks from a
list is a claim, it arrives on your dashboard under *facts people typed*, and an
outcome that turned on it says nothing about your rule. Ask the program:

```yaml
  - id: engram.version
    kind: machine
    describes: Which engram
    why: The .brain format changed between releases
    read: { op: program_version, program: engram }
```

The client finds `engram` on the machine's search path and runs it with
`--version` — or `-V`, `-version` or `version` if you name one of those as
`flag` — and keeps only the version number it prints. Nothing else it prints
leaves the machine — and the number travels **exactly**, `1.2.2` and not
`1.2.x`, because it is your own version and its last component is the fix you
shipped. (Versions of *other* software — a driver, an interpreter — are still
cut to major.minor.) A program with no `--version` flag is usually still fine: anything
that answers an unknown argument with a usage banner naming its version works.

**Where it is not on the search path, the user is asked where it is.** Nothing
is searched for — a client walking the disk to find a program would be reading
far more than it was allowed to. They point at the file or its folder, the file
has to *be* the program you named, and the location counts for that diagnosis
only.

The limits, so none of them surprises you: a bare name and never a path; no
shell, launcher, privilege, power or disk tool, whatever it is called; nothing
inside the operating system's own directories; five seconds, then it is stopped.

If people run your software as a container, ask Docker instead — nothing is run
inside the container, and the tag, or the image's
`org.opencontainers.image.version` label, is the answer:

```yaml
    read: { op: container_image_version, image: ollama/ollama }
```

### The error, and the lines of a log around it

The most useful thing you can receive is the error in the user's own words — and
the one thing no policy can coarsen, so it is withheld unless they agree to send
it. A free-text question may say where its answer usually comes from:

```yaml
  - id: ollama.log
    kind: human
    describes: What Ollama logged when the chat failed
    prompt: Copy the lines Ollama wrote around the failure, if you can
    log: { container: ollama/ollama, file: server.log }
    required: false
```

The client then offers to load the output of a running container of that image,
or the end of a file the user points at — `file` is the name shown as a hint;
where it is on their machine is their answer, and nothing is searched for. What
is loaded is shown only on their machine. They cut it down to the lines that
matter; account and host names, addresses, e-mail, credentials, tokens,
identifiers and times are replaced, they are told what was replaced and how
often, and they can still edit every word before agreeing to send it. At most
120 lines per answer, and you see it on your dashboard under *in their own
words*, for anything that recurs across at least five people.

`log` belongs on a question answered in words — a `kind: human` probe without
`choices`. On anything else it is refused, because it would be text travelling
as a reading.

### The rules a path and a name have to obey

These are refusals you can hit while writing something perfectly reasonable, so
they are worth having in front of you rather than discovering.

| | |
|---|---|
| **No `..` in a path** | A `..` component in a `read_file_key` or `read_ini_key` path is refused outright, and the path is then resolved — symlinks included — and compared against the resolved granted root, component by component. A directory whose name merely begins with two dots is fine |
| **No symlink out of a root** | Both sides of the check are resolved. A link inside a granted root that points outside it is refused, not followed |
| **ASCII only** in `path`, `name`, `key`, `glob` and `keys` | The deny list folds case and nothing else, so a name that merely *looks* permitted would slip past it. Comparing them fairly needs a confusables table the client does not carry |
| **The deny list is not negotiable** | Credentials, keys, browser profiles and wallets are refused whatever you ask and whatever the user clicks. It screens the *field name* as well as the file name, so `api_secret` of an otherwise permitted file is refused too |
| **`enumerate_read` searches where the pattern says** | `telemetry-cache/*.json` reads that directory, not the whole root. The depth is exactly the pattern's depth |
| **`read_registry`** | The hive must be one of `HKLM`, `HKCU`, `HKCR`, `HKU`, `HKCC`, and both the key path and the value name are held to a strict character set |
| **`program_version`** | A bare program name, never a path, and a flag from the closed list. Shells, launchers and power, privilege or disk tools are refused by name |
| **Shell histories are on the deny list** | Anything containing `_history` — `.bash_history`, `.python_history`, PowerShell's `ConsoleHost_history.txt` — every command somebody typed, passwords on a command line included |
| **Solution paths carry no percent-escapes** | Write the file name as it appears in your repository. An escape means the path we check and the path your host resolves are two different paths |
| **At most 24 probes** | That is what the client performs. A manifest declaring more is describing work that would be refused after the user had already consented to it |
| **`commit` and `successor` must be strings** | Quote them. YAML reads `commit: 1234567` as an integer and `commit: yes` as a boolean |

### Where the diagnosis comes from

You do not author a decision tree. **`answers.when` is one**, and it is derived
from what you already wrote.

A set of solutions for one problem class is a decision structure: each `when` is
a conjunction of conditions over facts, and `collect` says how each fact is
acquired. So `POST /diagnose` walks *your solutions*, and the tree cannot
contradict them, because it is a view of them.

Two consequences worth knowing while you write:

* **A reading beats a question.** A switch on a fact with a `read:` is preferred,
  because that value is already in every stored signature and re-partitions
  history, while a question only ever works forward. Where you can decide
  something by reading, do.
* **A question that nothing falls back to is refused.** If the only way to reach
  your solution is a question, and the class has no more general answer, somebody
  who would rather not say is left with nothing. The one exception is a class
  with a single solution decided only by asking — there the class the client
  named is itself the assertion, and the answer stands.

And a new refusal, which is the useful half of the above:

> `solutions/x.md`: answers.when matches on `'engram.speed'`, which no probe in
> `collect` acquires and which the client does not derive. A solution keyed on a
> fact nobody collects can never match.

That one is worth reading twice. A solution guarded by a fact nobody gathers does
not error — it simply never fires, for everybody, for ever.

### Getting it wrong

Everything above is checked **when your files are fetched**, not when a user is
waiting. Your own CI is a convenience that lets a bad pull request fail before
merge; this is the boundary. Refusals name what would have been permitted.

A refused version does not take down the version already being served — a broken
commit must not break a working mirror. A source that fails in some way nobody
planned for costs that source and nothing else: the rest of the crawl is
unaffected.

### Withdrawing a solution

**Delete the file and remove it from `solutions:`.** The next crawl closes it and
the mirror stops serving it. That is the whole procedure, and it is worth stating
because it is the thing you would reach for if you decided a remedy of yours was
harmful.

Closed, not deleted: what was being served on a given day stays answerable. If
you want the *whole* project to stop being mirrored, that is `withdraw` on your
dashboard rather than emptying the list.

## What the user's model does and does not change

Worth knowing, because it decides how much you have to write. Your published
files are matched, never generated, so **the first two rows below are the ones
your users are actually in** — and neither of them costs you anything.

| The user has | What happens to your solutions |
|---|---|
| No model at all | Fully served. Readings are collected under consent, `answers.when` is evaluated against them, your matching solution is shown, and its `proposes:` action runs with dry-run and rollback. Nothing is generated, so nothing is missing |
| A free-tier model | Identical for everything you published. The model is used only where nothing matched, and the report you receive records the size class so you can see it |
| A large paid model | Also identical. A bigger model does not make your solution better, and it never rewrites it |
| Nothing published, by anyone | The only case a model is asked to answer at all, and it is the user's own |

The one consequence for you: **a `problem_class` you did not publish cannot be
matched**, whatever model the user has. Coverage comes from your list, not from
inference. If users keep escalating something, that is a class to add — and the
dashboard tells you which, without you asking anyone.

This is also why `severity` and `proposes:` matter more than prose. The prose is
read by a person; the front matter is what the client can act on with no model
in the loop.

## Saying a project is finished

```yaml
status: deprecated
successor: https://example.org/the-new-one
```

Your own word, and authoritative. Whether your forge says the repository is
archived would be the next best signal, an explicit act by the owner, and the
mirror has a place for it — but nothing reads a forge today, so that field
stays empty and only your declaration counts.

What we will **not** do is call a project outdated because it has not changed.
*"No commit since March 2021"* is something we measured; *"outdated"* is a
verdict we have no standing to reach. A dormant solution is not a wrong one, and
your solutions keep being served with their age stated rather than withdrawn.

## If you stop

Nothing happens for a while, and then it degrades gently:

| Elapsed with the challenge file gone | State | Effect |
|---|---|---|
| One failed check | — | Nothing. Retried next cycle |
| 14 days | `stale` | **Still served.** The client shows when control was last confirmed |
| 90 days | `unknown` | Serving stops |

It never becomes `revoked`. Revocation is an accusation, and somebody who
stopped working on something has done nothing wrong. `unknown` is simply the
state of a project that never registered.

And a failure on *our* side never counts: a timeout or a DNS failure is our
inability to ask, not your absence, and only an actual answer moves that clock.

---

# Enterprise

## What is different

**You run the endpoint.** The interpretation — which is your moat — never leaves
you. We attest identity and nothing else.

**There is no fallback to us when your endpoint is down.** Silently taking over
would mean answering for you with knowledge we do not have. A client is told
you are unreachable, which is true, rather than given a guess with your name on
it.

## What you publish

A DNS TXT record under a domain you control:

```
_podshl.example.com.  TXT  "podshl-challenge=<token>; lei=5493001KJTIIGC8Y1R12"
```

A DNS record is used here rather than a file because it is far harder to obtain:
subdomain takeover, a CI job or a CDN misconfiguration all yield a write into
`/.well-known/`, and none of them yields a zone edit.

Then an Agent Card at `/.well-known/agent-card.json`, signed with a key
published out of band. The details are in [`SPEC.md`](SPEC.md).

## What identity actually means

Your legal name comes from the LEI register, not from anything you type. What is
attested is: *whoever controls this domain asserts this LEI, and this is the
name the register carries for it.*

That is weaker than a cryptographic binding between the two, and it is stated
that way rather than dressed up. It is still categorically stronger than a TLS
certificate, which proves domain control and nothing about who you are — and
that difference is what a person is actually deciding on when they let a
stranger's agent read their machine.

## Key rotation

Rotating your signing key appends a visible entry to the public log, and clients
are shown *"the signing identity for this vendor changed on \<date\>"*.

Not blocked, because a key change at a live anchor is either a legitimate
rotation or a takeover and we cannot tell which. Shown, because a takeover has
to register a new key against an existing anchor, and that is exactly what this
makes visible. **Key continuity is the tripwire; the checking cadence only
catches abandonment.**

## What you serve, and what a client refuses

`SPEC.md` is the protocol. This is the operational half of it: the four things
a client asks of you, in the order it asks them, and the exact conditions under
which it walks away. Every refusal below is one the client implements today, not
one it might.

### 1. The address

Off this machine, **HTTPS or nothing**. A plain-text base is refused before a
byte is sent, and loopback is the only exception — `127.0.0.1`, `localhost`,
`[::1]` and the rest of `127/8`, which have no wire to listen on. A private
address is not an exception: `http://10.0.0.5` and `http://192.168.0.26` are
refused like any other.

A plain connection cannot forge a signed remedy, so this is not about your
answer. It is about the *readings* travelling to you — everything the client
coarsened and anonymised before transmission — being readable by anybody on the
path.

### 2. The card

`GET /.well-known/agent-card.json`. Three distinct outcomes, and only one of
them is your fault:

| What happens | What the client concludes |
|---|---|
| 404, or a body that is not JSON | **no agent here.** Not an error, not a refusal — it says you publish no support agent and moves on |
| Connection refused, DNS failure, timeout | **nobody there.** Reported as unreachable, never as "publishes nothing" |
| A card, but unsigned or unverifiable | **refused.** This is the only one that is a fault, and it is yours |

That first row matters more than it looks. A project with no card and a project
whose host is down were once the same sentence to a user, and they are not the
same thing at all.

### 3. The signature, and where its key comes from

The card carries `signatures`, a detached JWS over the card **with
`signatures` removed**. The client rebuilds that body itself and verifies
against it, so anything you add outside the signature changes nothing and
convinces nobody.

**The key is not in the card.** A card verified against a key it carries proves
internal consistency and nothing else — it is a signature checking itself. The
client resolves a key for your host *out of band* and refuses the card outright
if it has none, naming the source it looked in. Getting a key to that resolver
is the part that costs you something, and that is the point.

The verified `protected` header is where the client reads `org`, `lei` and
`kid`. Not the card body — the body is not what was signed over in the eyes of
anybody checking.

### 4. The exchange

Two messages. `triage` carries the problem and a language and gets back a skill,
or no skill at all, which is a perfectly good answer. **A malformed skill is
your fault and is named as yours**: the client parses it the moment it arrives
rather than letting it surface as an empty probe list after somebody has already
been asked to allow a reading.

`diagnose` carries the facts, and with them two values you must sign back:

```json
{ "kind": "diagnose", "skill_id": "...", "facts": { }, "lang": "en",
  "nonce": "<32 chars, fresh per request>",
  "facts_sha256": "<sha256 of the JCS-canonicalised facts>" }
```

A remedy is accepted **only** as the answer to the request that asked for it.
The client refuses one whose `nonce`, `skill_id` or `facts_sha256` is not the
one it sent, and refuses one carrying **no nonce at all** — which is what a
replayed answer, or a vendor predating the binding, looks like. The refusal is
the same in every case: *this finding is not for this request*.

Sign over what you were asked, not over what you would like to have been asked.

### What you may ask to be read, and what will be refused anyway

Your skill names probes. The client plans every reading **before** it asks
anybody for consent, and refuses out-of-bounds ones there rather than after —
offering somebody a choice you would decline anyway is how people are trained to
click through.

More than **24** machine probes in one skill is refused **as a whole** (`MAX_READS`).
There
is no per-item consent that adds up to enumerating a machine.

Individual readings are refused with a kind, and the kinds are worth knowing
before you write a skill: `absent` (the tool is not on this machine), `denied`
(the client will not read this at all, consent or no consent — an SSH key, an
identifying GPU field, an environment variable off the list), `system` (a
program belonging to the operating system), `outside` (a path that leaves the
project, including by backing out of it), and `invalid` (everything else — an
unknown op, a flag that is not allowed, a tool that does not exist).

Free text a person typed **never travels** to you by default, whatever your
skill asks. It is offered separately, under its own consent, shown verbatim and
editable first. That is not a setting.

### What none of this buys you

Access. A client that cannot reach us proceeds on the protocol's own trust, and
that is deliberate rather than an oversight. See below.

## What you are paying for

Not access. Access cannot be withheld — a client that cannot reach us proceeds
on the protocol's own trust, and that is deliberate. What it buys is the
identity display at the moment somebody decides whether to let your agent read
their machine, protection against impersonation, and the data about your own
products.

---

# Both

## Claiming a dashboard

The same challenge either way, and it is free. **No payment**, because the
moment this feels like *"pay or we withhold your own defect list"* the whole
thing is finished.

### A claim has two halves, and only one of them is published

`POST /claim/{host}` answers with **two** values:

| | |
|---|---|
| `publish` | A hex digest. It goes at `.well-known/podshl-challenge`, it is public, and it is what re-verification checks from then on. Leave it there |
| `proof` | Its preimage. Shown once, to the caller that started the claim, and stored here only as a hash. **Never publish it** |

`POST /claim/{host}/verify` requires both: the file must carry the digest, and
the caller must present the preimage in `X-Podshl-Claim-Proof`.

This is not ceremony. A published file proves that *somebody* controls the host;
it cannot prove that the person asking is that somebody, because anyone can read
it — and on a forge, anyone can read the repository it lives in. Requiring a half
that was never published is what makes the claim yours rather than any
passer-by's.

One consequence worth knowing: **re-proving control means publishing something
new**, not pointing at what is already there. That is the recovery path, and it
is deliberate.

### Then say where your files are

`POST /claim/{host}/source`, with your token in `X-Podshl-Claim` and an optional
`prefix`. Proving control does not say where your `.podshl/` is, and until you
say so nothing is fetched.

The prefix defaults to the bare host and may be narrower — on shared hosting,
one project among several, give the path, and nothing outside it is ever
fetched. Enrolling the same prefix twice keeps the one source and queues an
immediate crawl of it — which is the refresh: there is no other button. Enrolling
a different prefix adds a second source rather than moving the first.

### The dashboard

`GET /dashboard/{host}` with your token in `X-Podshl-Claim`; the page at
`/dashboard` fetches the same JSON, and **Your project** in the navigation of
every page goes there.

On a forge the host in that path is the forge — `github.com` — and it is the
token that names which repository. The answer is scoped to the anchor the token
belongs to: two projects on one forge do not see each other's files or each
other's reports.

**Below five independent reporters a configuration is counted and not shown**,
and the page says so where the figures would be. That is not a delay in
displaying your data, it is the floor: a rare constellation with two reporters
describes the two of them. So a project that has just been enrolled shows its
files and its trees and no clusters, and that is the system working rather than
a page waiting to fill in.

It shows what recurs, with the coarse configuration, what was tried, what
failed, and the distribution of model classes — which is the number that
matters: **a small local model solving something from public knowledge means the
information was available and your product surface failed to convey it.** That
is a UX defect, not a knowledge gap.

It also states its own limit, because that limit is the honest argument for
connecting: without an agent it shows *that* something recurs, never *why*.

## What we hold, and what we cannot

**Held:** public attestations, a mirror of public repository content with the
commit it came from, anonymous counters, and — only where a user explicitly
asked for it — a description they chose to contribute.

**Never held:** credentials to anything of yours, user identities, query logs, or
the content of an enterprise diagnosis.

The counter reads **"47 reported"**, never "47 occurred". It under-reports
reality, and every place it is shown says so.

## Two lines that are not negotiable

* **Your numbers are never shown to anyone else.** Not to a competitor, not in a
  pitch, not anonymised-but-guessable.
* **No public ranking, ever.** Publicly there is exactly one figure: how many
  products are observed and how many have a connected agent. No per-vendor
  breakdown.

## Checking us

The attestation log is public, append-only and signed, and
[`monitor/verify_log.py`](monitor/verify_log.py) checks it using our own
arithmetic. Point it at the log with the key pinned and it will tell you if an
entry was ever altered.

Watch your own anchor. An attestation claiming your domain that you did not ask
for is exactly what this structure exists to make visible — including one we
should not have issued.
