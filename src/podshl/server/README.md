# The server

The operator side of [SERVER.md](../../../docs/SERVER.md). Internal — this does not
go public with the client.

    mise run db && mise run migrate && mise run server

`:8725` is the public application. `:8726` is the operator's, and it is a
**separate ASGI application on a separate listener** rather than a guarded route
— `SV24` says one vendor's figures are refused because no path exists to produce
them, and that is only true if the code cannot be reached.

## One principle

> We hold no credentials, write into no foreign system, and see no user data.
> What we assert is publicly verifiable; what we serve is a mirror with
> provenance.

The security argument is one question — *what does an attacker get who owns our
infrastructure?* — and the answer has to stay "a public log, a mirror of public
repositories, and some anonymous counters".

## What is built

| | |
|---|---|
| `merkle.py` | RFC 6962. No database access, so `spec/monitor/verify_log.py` vendors it verbatim and checks us with our own arithmetic |
| `log_store.py` | Appending is the only write. Serialised by an advisory lock, because a gapless index is what every proof is arithmetic over |
| `sth.py` | Signed tree heads, EdDSA over JCS — the same scheme the client already verifies, so a monitor needs no code we have not shipped |
| `anchor/result.py` | `Probed`, which **refuses to be a boolean** |
| `anchor/challenge.py` | The OSS path, plus the SSRF guard `SERVER.md` does not mention |
| `anchor/dns_txt.py` | The enterprise path |
| `anchor/sweep.py` | `stale` at 14 days, `unknown` at 90, never `revoked` |
| `counting.py` | `seen_key`, epochs, and a threshold that counts people |
| `clusters.py` | The class derived from the signature, never supplied |
| `ingest/fetch.py` | Conditional `GET`, and the guard for URLs an attacker chose |
| `ingest/manifest.py` | `.podshl/agent.yaml` and solution front matter |
| `ingest/validate.py` | The gate: endpoint under anchor, English, action and read vocabularies |
| `ingest/store.py` | Supersede, never overwrite — the served commit stays checkable |
| `ingest/scheduler.py` | The claim loop, in its own process |
| `cluster_tree.py` | The walk: exact along the path, `need`, and a fallback carried downward |
| `repartition.py` | Where an answer helped some configurations and not others, and what a switch would do to history — from the dashboard's own rows, saved nowhere |
| `ingest/confusable.py` | UTS 39, and why a hold is not a block |
| `anchor/gleif.py` | The register, asked per LEI through GLEIF's API and cached for a day |
| `anchor/enterprise.py` | DNS TXT plus the register, and the seam between them |
| `takedown.py` | Degrade to `unknown`, with a reason from a closed vocabulary |
| `app.py` / `ops_app.py` | The two applications |

## Three things worth reading the code for

**A probe result has three answers, not two.** Every resolver in this project
used to return `None` for a missing file, a malformed record, a timeout and an
NXDOMAIN alike — so "they published nothing" and "we could not ask" arrived as
the same value, and the message asserted the first. `SERVER.md`'s grading is
arithmetic over exactly that difference, so `Probed.__bool__` raises rather than
letting `if result:` compile. Only evidence advances the clock; a sweep whose
silence rate exceeds 20 % is discarded entirely, because otherwise one bad
afternoon on our side reads as ten thousand abandoned projects.

**The epoch salt is not in the database.** A column would be in every base
backup and every WAL archive, so *"discarded when the epoch rolls"* would be
true of the live row and false of the archive — the promise honest only until
the first restore. It is a 0600 file, overwritten before it is unlinked, and the
database holds only its digest so a restarted process can prove it loaded the
right one.

**Re-partitioning reads history; it never moves it.** `seen_key` is salted per
cluster, so reattaching an observation would need an HMAC over a pseudonym we
deliberately do not have. And a partition is not stored either: the tree is
derived from `answers.when`, so where an answer should fork is shown to the
maintainer as a suggestion for their own solution file (`SV32`).

## What the cases caught

`SV22` failed on its first run: free text was stored with no consent at all. A
Postgres `CHECK` passes when it evaluates to **NULL**, not only when it
evaluates to true — so with `description_consent` NULL the condition was NULL
and the constraint was decorative in exactly the case it existed for. Migration
`0005` fixes it and strips the rows that got in, keeping the observation and
deleting the words nobody agreed to share.

Every `CHECK` guarding a nullable column needs an explicit `IS NOT NULL`, or it
is weakest exactly where the data is.

## The guard that reading the standard library would not have given you

`is_public_address` refuses loopback, RFC 1918, link-local, reserved, multicast
and unspecified — and then a list of ranges Python's own flags miss.
`100.64.0.0/10`, carrier-grade NAT, returns False for `is_private`,
`is_link_local` *and* `is_reserved`. On any CGNAT network it would have gone
straight through. A case asserting it should be refused found that; reading the
`ipaddress` documentation did not.

## A question that cannot be answered must still lead somewhere

`SERVER.md` says "don't know" falls back to the parent and never dead-ends. That
is easy to write and easy to get subtly wrong: if the parent has no answer
either, the user who cannot say is left with nothing.

So the fallback is computed on the way **down** — it ships with the question, so
"don't know" costs no second round trip — and a tree whose question has nothing
above it to fall back to is **refused when it is authored**. A dead end is a
defect in the tree, not something for a user to discover by being unable to
answer. `SV28a` covers that half.

## The type that caught its own author

`Probed.__bool__` raises. Writing `enterprise.py` I typed
`resolver_probe or dns_txt.probe(...)` — a truthiness test on a Probed — and the
type refused it at the first run. That is exactly the collapse it exists to
prevent, and it caught the person who wrote the guard.

## What is not built

All 45 of `SERVER.md`'s `SV` cases are covered. What remains is not code:

* **The authoring tool.** Trees are *derived* from the solutions now
  (`ingest/tree_build.py`), so nothing stands between a publisher and a
  diagnosis. What the tool is still for is the fuzzy half — proposing a
  merge for a human to approve.
* **The ingest worker on a real host.** `run_once` is driven by the suite, and
  `main()` is the loop `deploy/systemd/podshl-ingest.service` runs, supervised
  (`DP1`). Nobody has started that unit on a host yet, because there is no host.
* **A second log watcher.** One monitor proves a log self-consistent; two
  comparing roots at the same tree size is what detects a split view.

`catchall/` is retired. It ran alongside with the two behaviours `SERVER.md`
contradicts — an unauthenticated gap report and a free-text problem class — and
this server does neither. The client was pointed here first, so the anonymous
branch never stopped recording: `report_without_vendor` posts a `subject` and a
per-subject pseudonym to `/report`, and the class is derived from the signature
rather than sent. `GR1`, `GR1a`, `GR2` and `GR3` moved with it, and **`GR1a`
closed in the move** — it was `open` precisely because the old floor counted
posts rather than people.
