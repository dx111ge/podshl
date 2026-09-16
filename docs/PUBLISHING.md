# What is published, and how

**Decided 2026-09-13: everything that runs is public.** The client, the whole
server — the dashboard, the aggregation and the operator view included — the
protocol, the deployment, the tests and the measured results, in a public
repository on GitHub under the licences in `LICENSING.md`.

It had been drawn the other way: the client and the verification paths open, the
operator's business closed. For the people PODSHL asks to trust it that was the
wrong line. An open-source maintainer is asked to point their users at an
operator that receives those users' reports; they have to be able to read what
that operator does with them — all of it, and not only the parts a monitor can
check from outside. Somebody could copy it; the name is the control point
(`LICENSING.md`, *The name*), not the source.

## The history goes nowhere

**The public repository starts with a fresh initial commit.** Not a filtered
history, not a rewrite. The working repository's history carries internal notes,
discarded approaches and commercial reasoning in its commit messages, and
filtering that is never quite complete. Nobody who wants to use or check PODSHL
needs to read how it was argued into existence.

## What is in the public repository

| | |
|---|---|
| **Code** | `client-rs/`, `src/` (the operator server with everything it serves, and the development counterparty), `docker/`, `deploy/`, `scripts/`, `mise.toml`, the compose files |
| **Protocol** | `spec/` in full — prose, both vocabularies, `INTEGRATING.md`, the worked examples, and `monitor/verify_log.py` |
| **Documents** | `README.md`, `INSTALL.md`, `ONBOARDING.md`, `SECURITY.md`, `LICENSING.md`, `SERVER.md`, `RELEASING.md`, this file |
| **Tests** | `TESTCASES.md`, `TESTCASES-SERVER.md`, `run_testcases.py` |
| **Results** | `FINDINGS.md`, `examples/engram/` with its screenshots and screencasts, `docs/shots/` |
| **Releases** | `release/<operator>/log_key.json` — the public log key a client release is built with — and the release artefacts on GitHub |

## What stays in the working repository only

Internal notes and commercial reasoning, not code: the session handover, the
direction and go-to-market document, the benefits case, outreach, the hardware
baseline, and the discovery design notes.

## Before every publication

- [x] No hostname, IP or internal address of the maintainer's own network in
      the published tree. Test fixtures use documentation addresses and a
      made-up account (`jdoe`); the anonymiser's cases hold to those
- [x] No link from a published document to one that stays internal
- [x] The operator's identity is configuration (`.env`), never in the tree —
      except where it is published anyway: the security contact in `SECURITY.md`
      is the imprint's address
- [x] A licence is chosen and applied: AGPL-3.0 for the code; Apache-2.0 for
      `spec/` code and schemas and CC-BY-4.0 for its prose
- [x] `SECURITY.md` with a way to report a finding
- [ ] Re-run the scan in `RELEASING.md`, *Cutting one*, on the tree about to be
      copied — every time, not once
