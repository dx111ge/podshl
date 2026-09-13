# Findings — AI over ITSM ticket data

Measured 2026-09-07. Three negative results. Reproduce the numbers with
`mise run measure`; every figure below is printed by `scripts/measure_corpus.py`.

Negative results are the ones that get forgotten and then rediscovered
expensively, so each is recorded with the evidence *and* with the condition that
would overturn it.

## What was attempted

The original goal was to finetune a small German LLM on
[`Tobi-Bueck/customer-support-tickets`](https://huggingface.co/datasets/Tobi-Bueck/customer-support-tickets)
as a documented tutorial with quality measurement — a chatbot that suggests
solutions and falls back to a tool call that opens a ticket.

That grew into a plan for a vendor-neutral **ITSM proposal sidecar** over
[engram](https://github.com/dx111ge/engram): ingest resolved tickets, score
resolution quality, propose mandatory fields on arrival with confidence and
evidence. Plan preserved at `~/.claude/plans/you-are-in-plan-mighty-dusk.md`.
It was never approved, and the results below are why.

## Corpus

| | |
|---|---|
| Tobi-Bueck tickets | 61,765 rows — **33,504 de**, 28,261 en |
| Fields | `subject, body, answer, type, queue, priority, language, version, tag_1..8` |
| Labelled block (answer **and** ITIL type) | **20,321** (60.7 %) |
| Neither answer nor type | 13,178 (39.3 %) |
| Types | Incident 8,231 · Request 5,787 · Problem 4,301 · Change 2,007 |
| Priorities / Queues | 5 levels · 52 distinct queues |
| OMQ German helpdesk emails | 627 real emails, 47 numeric categories, **0 resolution texts** |

---

## Result 1 — ticket data does not justify a generative finetune

Not because the data is bad. Because **the good half and the useful half are
different halves.**

**Evidence.** In the 20,321-row labelled block, **82.4 % of answers contain
nothing actionable** — no imperative, no version, no path, no named setting.
The detector is deliberately generous (it matches `klicken|öffnen|prüfen|Version
\d|\d+\.\d+|C:\\|/etc/|Einstellungen|…`), so it over-counts actionability and
the finding is conservative. The typical answer:

> *„Vielen Dank für Ihre Kontaktaufnahme … Unser technisches Team arbeitet
> derzeit an der Analyse der Situation, um die Ursache zu identifizieren …"*

Mean length 428 characters — it is not that answers are short. They are long and
empty. The customer's `body` on the same row, by contrast, is specific:
dashboard load times, data-sync inconsistencies, authentication failures.

**Consequence.**

- **Answer generation:** finetuning reproduces what the corpus contains —
  fluent, polite acknowledgements. It would work and be worthless.
- **Classification** (type / queue / priority): labels are clean and complete on
  20,321 rows, but a generative 4B model is the wrong architecture. A small
  encoder — or logistic regression over embeddings — beats a QLoRA finetune at
  classification, costs orders of magnitude less, and returns calibrated
  confidences, which the generative path does not.

Either way: **no finetune.** The measured hardware baseline (a Turing-generation
consumer GPU) is unaffected and still valid; it just has nothing to train.

**What would overturn this:** a corpus where resolution texts carry actual
repair steps — e.g. a real export from a helpdesk with an enforced solution
field, or one where a knowledge-base article is linked on close.

## Result 2 — the gaps found are real but not essential

Auto-triage, classification, routing and similar-ticket search are shipped today
by ServiceNow (Now Assist), Zendesk, Freshworks (Freddy) and Atlassian
Intelligence. Three candidate differentiators were examined and none holds:

| Candidate | Why it does not hold |
|---|---|
| **Automated Problem Management** — *"these 47 tickets are one problem, no fix documented"* | Vendors ship Problem Management modules. Only automatic problem *detection* from ticket clusters is thin — a feature, not a company. |
| **Measurement** — nobody can answer *"is our helpdesk AI any good?"* with numbers | A genuine deficit, but a consulting finding, not a product. |
| **On-prem / GDPR / vendor-neutral** | A distribution advantage, not a technical one. |

**Both kill-switches in the plan turned out to be unfireable**, which is the
methodological failure underneath:

- *Technical* (buried-gold experiment) needs real tickets. What is on disk is
  synthetic: **81.3 % of answers are unique and the most repeated text appears
  twice.** A real helpdesk shows one boilerplate reply hundreds of times. The
  experiment would test a mechanism against data in which the problem it targets
  does not occur.
- *Commercial* (gap report to a service manager) needs a service manager. There
  is none.

A plan whose falsification tests both fail to run is an assertion, not a
hypothesis.

**What would overturn this:** a service manager who reacts to an
"N occurrences, no documented fix" report from **their own** history.

## Result 3 — the substrate is missing, which is not the same as "AI is overrated"

> **The customer half of a ticket is rich and gets written. The agent half —
> where the actual knowledge would live — is systematically empty.**

This explains the shape of the whole market: every vendor AI feature sits on the
*intake* side — triage, routing, categorisation, similar-ticket search. That is
the side where text exists. Nobody ships usable resolution knowledge because the
data was never created.

And it is **an incentive problem, not a modelling problem.** An agent under SLA
pressure does not write down what they did, because writing costs time and
returns nothing to them. No model repairs that; you cannot learn from data
nobody produced.

Stated this way the finding is useful rather than merely dismissive: it names
what would turn the situation around — **capture at the moment of resolution** —
and simultaneously why that is hard. It is UX and incentive design, not ML.

The plan's one original idea aimed exactly there: the agent's accept-or-correct
click *is* a label, harvested at the moment of maximum context, so quality would
emerge from convergence instead of from data cleaning. But it yields **field**
labels only, never resolution knowledge. It does not solve this either.

---

## Kept / discarded

**Kept.** The mise environment and the measured Turing baseline — fp16 over
bf16, no FlashAttention-2 on sm_75,
~5.3 GiB real VRAM. Independent of this outcome and correct regardless of what
gets trained later. `data/raw/omq/` stays: 627 **real** German support emails
with gold category spans are the only genuine German helpdesk text on this
machine, and the only honest out-of-distribution evaluation set available.

**Discarded.** The sidecar architecture — 5-method ITSM adapter contract,
webhook receiver, `MockITSM`, idempotency and out-of-order handling,
backpressure. That is integration infrastructure for third-party ITSMs, and
there is no customer for it. `bykt` / `agui` are in-house ITSMs; anything of
value here is a feature inside them, not a product beside them.

## Reproduce

```bash
mise run fetch-corpus   # ~15 MB from HF into data/raw/ (not committed)
mise run measure        # prints every number in this document
mise run baseline       # re-verifies the hardware findings
```
