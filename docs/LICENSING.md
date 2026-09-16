# Licensing

Copyright (C) 2026 Sven Andreas.

Two licences, and the boundary between them is the point rather than an
accident.

| What | Licence | File |
|---|---|---|
| All code — the client, the server, the tooling, the test suites | **AGPL-3.0-or-later** | [`LICENSE`](../LICENSE) |
| `spec/` — the protocol, its schemas, its vocabularies, its example and its reference monitor | **Apache-2.0** | [`spec/LICENSE-APACHE-2.0`](../spec/LICENSE-APACHE-2.0) |
| `spec/` prose — the specification documents themselves | **CC-BY-4.0** | [`spec/LICENSE-CC-BY-4.0`](../spec/LICENSE-CC-BY-4.0) |

## Why the specification is not copyleft

**A specification has to be implementable without infecting the
implementation.** If reading `spec/vocabulary/reads.json` or copying four lines
out of `spec/example/agent.yaml` obliged a vendor to relicense their support
tooling, then participating in this protocol would be a licensing decision
before it was a technical one — and their legal review would answer it with no.

That is not a minor cost. This protocol's central claim is that *unknown is not
untrusted*: a project nobody has heard of is in exactly the state a project
starts in, and the way anybody leaves that state is by publishing two files. A
licence that makes publishing those two files a legal event would put an
approval queue in front of the one action the whole design depends on being
free.

So Apache-2.0 for anything in `spec/` that is executed or parsed — schemas, the
vocabularies, the worked example, `spec/monitor/verify_log.py` — because a
monitor is only useful if strangers run it and nobody negotiates first. It also
carries an explicit patent grant, which matters for a wire format more than for
an application.

CC-BY-4.0 for the prose, because a specification document is a document. A
software licence applied to written text is a category error that produces
questions no one can answer.

## Why the code is copyleft

The intent is that everything comes back. That is what copyleft means, and the
**A** in AGPL is the part that matters here: a hosted service is how this
software would otherwise be used without ever being distributed. An operator who
improves the log, the anchor verification or the ingest validator and runs it as
a service owes those improvements to the people relying on them.

It is **not** a non-commercial clause, deliberately. "Non-commercial" is not
open source under the OSI definition, distributions will not package it, the
term is vague enough that a legal department answers it with no, and it would
deny the free client to precisely the companies the paid side exists for.

## Everything is published

Decided 2026-09-13: the whole server is public — the dashboard, the aggregation
and the operator view included, under the same AGPL as the rest. It had been
held back as the operator's business. For the people this is for, that was the
wrong trade: a maintainer asked to trust what an operator does with their
users' reports has to be able to read all of it, not only the parts a monitor
can check. What stays out of the repository is internal notes and the
commercial reasoning, not code.

What had to be open in any case is the *verification*. Certificate
Transparency works with closed log software because the proofs are
independently checkable and the monitors are independent, which is exactly what
`spec/monitor/verify_log.py` is. So the parts a participant depends on to keep
working — the log with its append and proof paths, anchor verification, the
ingest validator, the mirror's serving path — are AGPL, because their
correctness is a security claim and a security claim nobody can read is a
promise.

## The name

**PODSHL is a trademark, and that is the real control point.** Anyone may fork
this, run it, and build a competitor from it. Nobody may present their fork as
this. A fork is welcome; an impersonation of the operator whose entire product
is a claim about what it does and does not hold is not the same thing.

The licences above grant no trademark rights, which both of them say for
themselves — Apache-2.0 §6 explicitly, and the GPL family by granting copyright
permissions and nothing else.

## Contributions

By opening a pull request you licence your contribution under the licence of
the file you are changing, and you confirm you have the right to do so. There is
no Contributor Licence Agreement, and that is a decision with a cost attached:
copyright stays with each author, so relicensing later needs every one of them,
and in practice that means it cannot be done. A CLA would keep a commercial
exception saleable to companies whose legal review refuses AGPL. It also asks
every contributor to sign a document assigning rights to a one-person company
before their first patch, which is the surest way to have no contributors at
all.
