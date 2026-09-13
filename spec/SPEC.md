# The PODSHL protocol

How a vendor's support knowledge reaches a customer's agent, executes on the
customer's machine under the customer's consent, and returns a conclusion.

This document is what a vendor implements against. It describes the wire, not
any particular implementation of it, and it is deliberately narrow: everything
here is either something two independent parties must agree on, or something a
user's safety rests on.

Status: **version 1, and incomplete in one respect that is named** — the
attestation section describes signing and trust as they exist today, and marks
where the operator's directory adds to it.

## The three rules

Everything below follows from these, and where a feature cannot be built inside
them it is not built.

**The vendor may only choose, never invent.** The action vocabulary and the read
vocabulary both live on the client. A vendor names an operation and fills
declared parameters; it cannot ship a capability. A hallucinating or compromised
vendor can mis-parameterise a tested operation — which validation and the
dry-run catch — but cannot introduce one. This is why a signed *script* would be
strictly worse: a signature over arbitrary code certifies origin while granting
unbounded effect.

**Nothing happens without being shown first.** Read, transmit, change: three
separate consents, in plain language.

**Unknown is not untrusted.** A vendor who publishes a valid agent without ever
registering anywhere still works. An agent that cannot be verified is refused;
an agent nobody has heard of is not.

Those last two are opposite states and must never be conflated. *"There is
nobody there"* and *"do not trust this"* lead to opposite behaviour.

## Transport

**A2A across the organisational boundary.** One JSON-RPC method, `SendMessage`,
at the endpoint the agent card declares. The message carries a part of kind
`data`, and that part's `data` object carries a `kind` field selecting one of
four interactions. A client sends exactly one such part; an endpoint reads the
first one it finds and ignores parts of other kinds.

```
POST <endpoint>
{ "jsonrpc": "2.0", "id": "...", "method": "SendMessage",
  "params": { "message": { "role": "user",
    "parts": [ { "kind": "data", "data": { "kind": "triage", ... } } ] } } }
```

A response carries `result`, or `error` in the JSON-RPC sense. Everything
described below lives inside `result`.

| `kind` | Asks | Answers with |
|---|---|---|
| `triage` | *"Which of your skills applies to this problem?"* | `skill`, or a reason there is none |
| `diagnose` | *"Here are the facts. What do you conclude?"* | `remedy` and `signature` |
| `report` | *"This happened to me."* | `receipt` |
| `escalate` | *"This needs a person."* | `reference` and the return path |

## Discovery

**The client asks for a vendor by name; it does not enumerate the machine.**
Scanning a bus to guess a vendor would see only part of the picture — it shows a
graphics chip and a network chip, never a networked printer, an installed
application or an attached peripheral — and it is itself a read, performed
before anyone consented to anything.

Discovery must work without any central party:

1. **ANS/DNS** under the domain the organisation already controls.
2. **Well-known probing** — `GET <base>/.well-known/agent-card.json`.
3. **A directory**, only as a hint, and only where it labels itself as one.

A client must state how it arrived at each candidate, so that a guess at a
domain is visibly a guess rather than a lookup.

### There is nobody there

`404` at the well-known URI means **no agent**, and so does a non-JSON body, a
`5xx`, or an unreachable host — with the reason stated. None of these is a trust
failure. This is the normal case today: almost no vendor publishes an agent, and
a client that treated absence as suspicion would be wrong about nearly everyone.

## Identity

### The agent card

JSON at `/.well-known/agent-card.json` (RFC 8615), declaring identity, endpoint,
transport and skills, plus a `signatures` array.

### Signing

A **detached JWS** (RFC 7515) over the **JCS-canonical** form (RFC 8785) of the
document, with the `signatures` key removed before canonicalisation.

```
signing input = BASE64URL(JCS(protected)) || "." || BASE64URL(JCS(payload))
```

- **Algorithm: `EdDSA` (Ed25519), and nothing else.** Any other `alg` in the
  protected header is refused rather than negotiated.
- **Key format: `{"kty":"OKP","crv":"Ed25519","x":<32 bytes, base64url>}`.**
- Base64url throughout, **unpadded**.
- Verification uses the strict variant, which rejects small-order public keys.
  The permissive variant has no business at a trust boundary.
- The signature object is `{"protected": <b64u>, "signature": <b64u>}`.

The same scheme signs a `remedy`, over the remedy object exactly as it appears
in the response.

> **Numbers must be integers.** JCS as used here refuses any non-integer number.
> A float anywhere in a signed document makes a perfectly valid document report
> *"signature invalid"* — the worst failure mode available, because it looks like
> an attack. Percentages, temperatures, versions and measurements travel as
> **strings**.

Two further canonicalisation details that differ from a naive serialiser, and
each of which breaks a signature by one byte if got wrong:

- Object keys sort by **UTF-16 code unit**, not by UTF-8 byte.
- The solidus `/` is **not** escaped. `\b \t \n \f \r` use short escapes; other
  control characters use `\u00xx`; nothing else is escaped.

### The key comes from outside the card

**A card is never verified against a key it carries.** That would prove the
document is internally consistent and say nothing about who published it. The
key must arrive by a path its publisher does not control — in practice a DNS TXT
record under the domain, which is what ANS provides.

A client that cannot obtain an out-of-band key **refuses**. It does not fall
back to the card's own key material.

> **A resolver must distinguish "no record" from "could not ask."** Absence is a
> statement by the domain owner; a timeout is a fault on the asking side.
> Reporting the second as the first tells a user that a vendor published no key
> when the truth is that we failed to look, and it makes any later grading of
> repeated failures meaningless.

### What identity is, and what it is not

The protected header may carry `kid`, and — where the vendor asserts one — `org`
and `lei`.

> **A name in a signed header is asserted, not verified.** The signature proves
> only that the holder of the key published under that domain wrote it. A client
> must not present such a name as a verified identity on its own. What is
> verified without any third party is the **domain**: the key came from it, and
> the endpoint lies under it.
>
> A verified *legal* identity requires a register lookup by an attesting party
> and is out of scope for this version. Where a client shows a name it has not
> verified, it must show the domain beside it.

## Skills

A `triage` request carries the user's problem text and the language they want:

```json
{"kind": "triage", "problem": "...", "lang": "de", "context": {}}
```

The response carries a **skill descriptor**, or no skill and a reason.

```json
{"id": "...", "version": "3.1.0", "title": "...",
 "applies_to": {"os": "linux"}, "lang": "en", "probes": [ ... ],
 "static_kb_url": "...", "static_kb_says": "..."}
```

`applies_to` is checked by the client **before** anything is read. A skill for
another platform is refused at that point, not after collection.

### Language

**A vendor owes exactly one language beyond its own: English.** Serving
"whatever was asked for" moves the localisation burden onto the vendor, who has
every incentive to support few languages — and a German user of a US vendor
would still read English.

- The response states `lang_served`, which is the language of the text actually
  returned.
- Where that is not the language the user asked for, the client translates
  locally, **marks the result as machine translation**, and keeps the original
  one click away from every consent screen. A consent decision must never rest
  on a translation nobody can check.
- A skill available in neither the requested language nor English is a protocol
  violation and is refused with that reason.

## Probes: what to collect

A probe declares one fact the vendor needs, why it needs it, and how to get it.

```json
{"id": "gpu.driver_version", "kind": "machine",
 "describes": "Installed driver version",
 "why": "bf16 exists in silicon only from 8.0",
 "read": {"op": "run_tool", "tool": "nvidia-smi",
          "args": ["--query-gpu=driver_version", "--format=csv,noheader"]},
 "required": true}
```

| Field | Meaning |
|---|---|
| `id` | The fact's name. **Not free-form — see below.** |
| `kind` | `machine` or `human` |
| `describes`, `why` | Shown to the user. Consent needs a reason |
| `read` | A read instruction from the published vocabulary |
| `derived` | The client computes this from other readings rather than reading it |
| `prompt`, `example`, `pattern`, `choices` | For a human probe |
| `when_missing` | Ask a person only if the machine could not supply it |
| `log` | On a free-text human probe: where the text usually comes from — `container` (an image) and/or `file` (a file name, shown as a hint). The client may load it for the user to cut down; nothing of it travels except under the free-text consent, anonymised |
| `required` | Whether the diagnosis can proceed without it |

**`derived` matters.** A derived fact is computed on the client from readings the
client performed, so a vendor cannot assert an interpretation of a value it did
not observe.

### The read vocabulary

Machine-readable in [`vocabulary/reads.json`](vocabulary/reads.json), which is
normative, at version 3. Nine operations, a tool allow list with per-tool
argument patterns, named roots rather than filesystem paths, and a deny list
that **consent cannot unlock**. A client refuses anything outside it, naming
what is permitted.

A read instruction may be refused by a particular client for a reason that is
not a protocol error — a tool absent from that machine, a platform that has no
registry. That must be **stated**, never returned as a silent empty result.

A relative `path` in `read_file_key` or `read_ini_key` is relative to the project
directory the user granted for the incident; without one it names nothing and
is refused. It is never resolved against the client process's own working
directory.

**`program_version` is the one operation that starts a program a publisher
chose**, and it is bounded accordingly. The publisher names a program, never a
path, and may pick an argument from `programs.flags` only. The client looks on
its search path — never by scanning the disk — and where the program is not
there it asks the user where it is: a file or a folder, which must contain the
program by that exact name, and which counts for the incident only. Refused
whatever anybody consents to: every name on `programs.deny`, and every file
inside the operating system's own directories. The run is bounded in time and
output, and **only the version token comes back**; nothing else the program
prints may leave the client. A client must show the resolved file on the
consent screen, because which file runs is what the user can check.

`container_image_version` reads Docker's own listing: the tag of a running
container of the named image, or the image's `org.opencontainers.image.version`
label. Nothing is executed inside a container, and a publisher names an image,
never a container.

### Reading ids are semantic

> **An id is not a free label.** How a value is coarsened before it may travel
> is decided from its id: an id containing `serial` never travels, one
> containing `version` is reduced to major.minor — unless it was read by
> `program_version` or `container_image_version`, which is the publisher's own
> version and travels exactly — one ending `_mib` is placed in
> a bucket. **Renaming a reading silently changes what leaves the machine.**

The full policy is in `vocabulary/reads.json` under `generalisation`, and it
governs what a **report** carries. A diagnosis round is different: `facts` go
to the endpoint as read, because a rule over `>= 3.13` cannot be walked against
`3.x`. That is why the transmit consent before the first round names the
recipient and shows the values, and why the operator's endpoint uses them for
the walk and does not store them.

## Diagnosis

```json
{"kind": "diagnose", "skill_id": "...", "facts": { ... }, "lang": "de",
 "nonce": "...", "facts_sha256": "..."}
```

`facts` is the **accumulated** map, resent in full on each round. `lang` is the
language asked for at triage; the remedy's human text — findings, reasons,
questions — follows the same rule as the skill: that language, or English.

`nonce` is fresh per request and `facts_sha256` is the SHA-256, hex, of the
JCS-canonical `facts`. The endpoint **must** carry both back inside the signed
remedy, and must compute `facts_sha256` from the facts it actually answered
about rather than echoing the value it was sent — a hash it copied says
nothing. A signature proves who wrote a remedy and nothing about what for:
without this binding an answer to somebody else's readings verifies just as
well as an answer to these, last month's verifies as well as today's, and
anybody on the path can replay one.

The response carries `remedy` and `signature`. A remedy that does not verify is
never acted on, and neither is one that verifies but does not answer *this*
request.

### Three outcomes, and the third is the point

A remedy is one of:

**A finding** — `findings`, and a `plan` of actions, optionally a `verify` list.

**An abstention** — `abstained: true` with `abstain_reason`, optionally
`escalate`. Not knowing is a valid answer and must be available; a vendor that
cannot abstain will guess.

**A need** — `need: [Probe]` with `need_reason`. *"I still need X."* This is how
a decision tree gets walked without guessing: where two problems look alike, the
endpoint asks for the fact that separates them rather than estimating which is
more likely.

The client answers a `need` by collecting or asking, then calls `diagnose` again
with the enlarged `facts`. The same loop, with per-round consent.

> **A vendor must bound its own rounds.** The client is not required to impose a
> limit, and an endpoint that returns `need` indefinitely will loop until the
> user stops it.

### "Don't know" must never dead-end

A user who cannot answer, or declines to, is recorded as
**`"<probe id>.declined": true`** in `facts`. This is a wire convention, not a
UI detail.

An endpoint receiving it **must not ask again for that fact**, and must fall
back to whatever answer applies without it. Not everyone knows whether their
card is overclocked, and a branch that terminates in a question nobody can
answer is a dead end dressed as a diagnosis.

### Actions

A plan entry selects an action and fills its declared parameters:

```json
{"action": "set_config_key",
 "params": {"file": "settings.toml", "key": "vsync", "value": "on"},
 "because": "VSync was not set on multi-monitor systems until 1.3.2"}
```

The vocabulary is machine-readable in
[`vocabulary/actions.json`](vocabulary/actions.json), which is normative.
Parameter patterns are **anchored** by the client — a pattern free to match a
substring is not a validation — and an undeclared parameter is refused along
with an unknown action id.

Version 1 defines three actions. The list is short on purpose: every entry is an
operation somebody had to implement, test on three platforms and be willing to
have run on a stranger's machine, and that cost is what keeps it honest.

| Action | Mutating | Reversible | Parameters |
|---|---|---|---|
| `report_only` | no | — | none. State a finding and change nothing |
| `set_config_key` | yes | yes | `file`, `key`, `value` |
| `restore_backup` | yes | **no** | `file`. This is the undo, so it has no undo of its own |

A client that implements more is free to; a vendor may only select from what the
client it is talking to publishes, and the client names its own vocabulary when
it refuses.

`because` is shown to the user next to the consent prompt. An action without a
reason a person can evaluate is not a request for consent.

Each action declares whether it is `mutating` and whether it is `reversible`. A
client must keep what is needed to reverse a reversible mutating action before
performing it. Not every mutating action is reversible — the action that
*performs* an undo cannot itself be undone — and a client must show that
difference before asking for consent rather than after.

## Reporting

A report is **user-initiated, after the attempt**, and that ordering is not
incidental: a report filed afterwards can carry *whether it worked*, which is
the label that makes the corpus worth having.

```json
{"skill_id": "...", "skill_version": "...", "resolved_by": "vendor_skill",
 "outcome": "resolved", "observed": { ... }, "dropped": [ ... ],
 "failed_actions": [ ... ], "pseudonym": "...", "epoch": "2026-09"}
```

- **No timestamp and no incident id.** A report must not be linkable back to the
  run that produced it. The month is the finest time it carries: `epoch` is a
  year-month, and so is the `granted_at` of a free-text consent.
- `observed` carries readings already coarsened by the generalisation policy;
  `dropped` names what was withheld, so the omission is visible rather than
  silent.
- `resolved_by` is one of `vendor_skill`, `general_agent`, `human`,
  `unresolved`. It is a measurement: a small local model solving something from
  public knowledge means the information was available and the product surface
  failed to convey it.
- `outcome` is the person's answer to *did it work*, asked after they had the
  chance to try. A client must not fill it in on their behalf.
- Free text travels only in `description`, under its own consent naming the
  recipient (`description_consent`), and at most 16 KiB. A client must show the
  exact words before asking, keep them editable, and **replace what has the
  shape of an identifier first** — account and host names, addresses, e-mail,
  credentials, tokens, identifiers and times — telling the user what was
  replaced. Times are on that list because the report carries none by design,
  and one pasted log would restore them.

### Reporting about a published project

Where the answer came from a project's published files rather than a vendor's
agent, the report goes to the operator that mirrors them: `POST /report` with
`subject` (the project's host) in place of `skill_id`, a pseudonym derived per
subject, and the same `observed`, `stated`, `dropped`, `decided_on`,
`outcome` and `description` as above. The project's maintainer sees what
recurs on their own dashboard, for anything at or above five distinct
reporters — including, there, what people agreed to send in their own words.

### The pseudonym

`pseudonym` is derived per vendor and per epoch, so that a recipient can count,
rate-limit and block without being able to link a client across vendors or
across epochs. `epoch` is a year-month.

> **A recipient counting distinct reporters must count distinct pseudonyms.**
> Counting submissions is a different and weaker quantity: it cannot tell five
> people from one person reporting five times, which is precisely the
> distinction any threshold protecting a rare configuration depends on.

The receipt is the entire reward. Rewarding the *act* of reporting buys
duplicates and noise; reporting an *outcome* — "you reported this in March, it
shipped in 14.2" — cannot be farmed, because a user cannot manufacture a fix.

## Escalation

When a case needs a person, the vendor declares where it goes, what it needs,
and how it can answer.

```json
{"reason": "...", "queue": "...", "target": "...",
 "require": [Probe], "reply_via": ["email", "ticket_url", "none"]}
```

`reply_via` is bounded by the client exactly as actions are. The permitted
channels are `email`, `ticket_url` and `none` — and `none` is a real option: a
vendor may promise no reply, and saying so is better than silence.

**There is deliberately no inbound channel on the customer's machine.** A client
does not accept callbacks.

Fields the vendor requires are collected as ordinary probes, with consent, and
validated locally **before** a case is opened — not after an RMA has been raised
against a malformed address.

## Publishing as an open-source project

A developer with no domain, no server and no signing key publishes a single
static file at a location they already control, plus the solutions it lists.
`spec/example/` is a worked one for an ordinary Python project.

```
.podshl/
  agent.yaml                 identity is the anchor, so there is no name field
  solutions/
    wheel-missing-for-python.md
```

Fetched as plain HTTPS — the manifest, then the files it names. No forge API, so
a forge, a bare nginx and a static host all work identically.

Two things about it that are easy to get wrong:

- **The endpoint must lie under the anchor.** A manifest for `example.org`
  pointing at `google.com` is refused, not shown with a warning.
- **`problem_classes` names other people's software on purpose.** A project that
  repairs `pip` behaviour has to be able to say `pip`. That is nominative use,
  and it is what the open-source branch *is*: projects that fix software they
  did not write. Identity and subject are separate fields.

A solution is YAML front matter and a human body, so that one solution is one
pull request — the machine part small enough to check at a glance, the human
part readable.

## Conformance

An implementation conforms if:

1. It verifies signatures as described, refusing any `alg` but `EdDSA`, and
   never against a key the document carries.
2. It refuses any action id or read op outside `vocabulary/`, naming what is
   permitted.
3. It anchors every parameter pattern and refuses undeclared parameters.
4. It never returns a silent empty result where a read was refused.
5. It distinguishes *no agent* from *untrusted*, and *no record* from *could not
   ask*.
6. It honours `<probe id>.declined` without re-asking.
7. It serves English, and states `lang_served`.
8. It emits no non-integer number in any signed document.

The vocabularies in `vocabulary/` are normative and are held to the reference
client by tests; this prose is not a substitute for them where they differ.
