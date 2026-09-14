# Server test cases

    mise run db          # the cluster in var/pgdata
    mise run migrate     # the schema
    mise run testcases   # every row marked `auto`, by id

**Internal.** `PUBLISHING.md` puts the client cases on the public side and these
on this one, so they live in their own file: the split is a cut rather than a
disentangling when the client is published.

Same rules as `TESTCASES.md`. Every case states what *should* happen, coverage is
marked honestly, and the runner fails if an `auto` row has no implementation.
These need Postgres; when it is absent they fail with an instruction rather than
being skipped, because a case that passes without running proves nothing.

**All 45 `SV` cases in `SERVER.md` have an implementation.** The rows below carry
more than 45 ids, because several of that document's cases needed splitting once
they were written down — `SV8` and `SV8a` fail differently and a vendor needs to
know which half, `SV47` and `SV47a` guard two separate code paths, and `SV28a`
is the authoring-time half of a rule `SV28` only checks at runtime.

## The log

`SERVER.md` commits to an append-only signed transparency log, and to
Certificate Transparency's reason for one: **our own honesty is checkable**. A
domain owner monitoring the log sees any attestation claiming their domain,
including one we should not have issued.

| # | Case | Expected | Cover |
|---|---|---|---|
| SV1 | An OSS anchor is attested | Entry appended to the log; the sequence is gapless, because every proof is arithmetic over it | auto |
| SV40 | The log's root | Computed incrementally, and equal to the root computed from the leaves. If those differ, every proof ever issued is wrong | auto |
| SV41 | Inclusion and consistency proofs | Verify for every entry and every prefix — consistency is the one that catches a rewrite | auto |
| **SV42** | **An entry altered in place** | **Changes the root. Visible from outside with no cooperation from us — otherwise the log is unfalsifiable and the transparency is decorative** | auto |
| SV43 | The signed tree head | Verifies under its own key, and not under any other | auto |

## Anchors, and the difference between silence and absence

| # | Case | Expected | Cover |
|---|---|---|---|
| SV2 | Challenge file gone once | No state change. Retry next cycle | auto |
| SV2a | Gone 14 days, then 90 | `stale`, then `unknown`. **Never `revoked`** — revocation is an accusation, and someone who stopped working on something has done nothing wrong | auto |
| **SV44** | **A probe result used as a boolean** | **Refused by the type. `if result:` collapses "they published nothing" into "we could not ask", which is the bug every resolver in this project had** | auto |
| SV45 | 404, 500, 451, a wrong token, a cross-host redirect | Five different answers, not one. A blocking intermediary is not the claimant's statement | auto |
| **SV46** | **A sweep during our own outage** | **Ages nothing. Otherwise one bad afternoon reads as ten thousand abandoned projects — a self-inflicted mass un-enrolment of a service designed never to revoke** | auto |
| SV47 | The anchor probe aimed at a private address | Refused. The crawler follows URLs an attacker chose, so refusing loopback and private ranges is what stops it becoming a request-forgery engine aimed at our own network | auto |

## What cannot be typed in

| # | Case | Expected | Cover |
|---|---|---|---|
| SV4 | A display name that is not the anchor | Impossible — the column does not exist | auto |
| SV51 | An OSS attestation carrying a legal name | Refused. The legal name exists only for enterprise and only from the register | auto |
| SV50 | The problem class | Derived from the signature. Key order does not change a problem's identity, and the class carries no prose | auto |

## Counting people, not posts

The service this replaces counted submissions, so five presses of the button
from one person crossed a threshold named for anonymity. The client had been
sending a pseudonym all along and nothing read it.

| # | Case | Expected | Cover |
|---|---|---|---|
| **SV13** | **The same client reporting five times** | **Counted once. Submissions and reporters are two figures with two names, and only the second is ever compared to k** | auto |
| SV14 | Fewer than k distinct pseudonyms | Not surfaced to anyone — not to the vendor, not to the operator | auto |
| SV48 | The epoch roll | Destroys the salt. Afterwards a stored `seen_key` cannot be tested against any pseudonym by anyone, including us, including under an order | auto |
| SV49 | The same pseudonym in two clusters or two epochs | Different keys. Nothing joins on either axis | auto |
| SV16 | The same signature twice | One cluster, found by one indexed lookup — which is why the common case never reaches the origin | auto |

## Ingest — the gate, not the hope

The developer's CI is a convenience that lets a bad pull request fail before
merge. This is the boundary: a solution proposing an action nobody implements
must be refused when it arrives, not when a user has already consented to a plan
built around it.

`spec/example/` is a worked `.podshl/agent.yaml` for an ordinary Python project,
and it goes through this same gate in the suite — an example the implementation
would reject is worse than none, because a developer copies it and the first
thing that happens is a rejection they did not cause.

| # | Case | Expected | Cover |
|---|---|---|---|
| SV52 | The published example | Passes its own gate, manifest and solutions | auto |
| SV3 | An endpoint outside the verified anchor | Refused at ingest, not rendered with a warning — including the lookalike `example.org.evil` | auto |
| **SV6** | **A read outside the vocabulary** | **Refused. The sharp case is not an unknown tool but a known one asked to do something else: `python3 --version` is a reading, `python3 -c ...` is arbitrary execution, and one argument separates them** | auto |
| SV7 | A manifest declaring no English | Refused at ingest — English is the one obligation | auto |
| SV7a | A card without English reaching the database | Refused by a `CHECK` as well, so a bug in the validator still cannot store one | auto |
| SV5 | A solution proposing an unknown action, or a real one with a bad parameter | Refused at ingest, not at execution | auto |
| **SV35** | **A manifest naming another vendor's product as a problem class** | **Accepted. Nominative use is the entire OSS branch: a project that repairs somebody else's software has to be able to say whose. A takedown reaches an anchor, never a class, or a vendor could forbid anyone from saying their name out loud** | auto |
| SV53 | A solution path leaving the manifest's directory | Refused | auto |
| SV47a | The *manifest fetch* aimed at a private address | Refused too. The anchor probe and the manifest fetch are separate code paths, and a guard on one is not a guard on the other | auto |
| SV54 | A fetch result used as a boolean | Refused by the type, for the same reason as an anchor probe | auto |

## The mirror

Fetching from a forge on the request path would be wrong twice over — their rate
limits become our capacity, and their outage becomes ours. So ingest runs on a
schedule and everything served comes from our own database.

| # | Case | Expected | Cover |
|---|---|---|---|
| SV55 | A project ingested from a real host | Anchor confirmed in the same pass, manifest and both solutions validated and stored, an entry appended to the log | auto |
| **SV18** | **The commit we publish** | **Reproduces the bytes we serve. Publishing a commit whose content cannot be checked against it is a claim nobody can verify — the same objection as a log that can be quietly edited** | auto |
| SV17 | The source goes offline | The mirror keeps answering. Nothing on the request path touches the network | auto |
| **SV56** | **A source that failed validation** | **Does not remember its ETag. Otherwise the next cycle sends `If-None-Match`, gets a 304, and records a document that failed once as fine forever — and the refusal is never revisited after the developer fixes it** | auto |
| SV57 | Loopback fetching | Off unless deliberately switched on, and reported at `/` when it is. A crawler that follows a URL to 127.0.0.1 is a request-forgery engine aimed at whatever else is listening | auto |

## Similarity: a switch, not a metric

Both failure directions are expensive. Merge too eagerly and two bugs share a
cluster, the solution is wrong for half of them, and the counter lies. Split too
eagerly and forty-seven users become forty-seven clusters of one. A threshold on
a fuzzy score is wrong in both directions at once and cannot explain itself
either way — so the endpoint does not estimate whether two reports are the same.
It acquires the fact that decides.

| # | Case | Expected | Cover |
|---|---|---|---|
| SV26 | A signature reaching a node that needs a switch | The endpoint answers `need`, not a guess | auto |
| SV27 | The client answering a `need` | The same round loop it already runs with the local model — one mechanism, not two | auto |
| **SV28** | **"Don't know"** | **Falls back to the parent. Structural rather than hopeful: the fallback is computed on the way *down*, so it ships with the question and nothing walks back up looking for one** | auto |
| **SV28a** | **A question with nothing above it to fall back to** | **Refused when the tree is authored. A dead end is a defect in the tree, not something a user should discover by being unable to answer** | auto |
| SV29 | A switch on an already-collected fact | Re-partitions stored history — the value is in every signature already — and the suggestion says what it does to the reports already held, in the dashboard cards' own counts | auto |
| SV30 | A configuration where an answer went both ways | No reading can separate those people, because their reports agree on every fact. The suggestion says a new question would apply forward only | auto |
| SV31 | Runtime matching | Exact along the walked path. An unbranched value falls back rather than resolving to the nearest branch, and a value the comparator cannot read stops the walk instead of counting as a non-match | auto |
| **SV32** | **An answer that helped some configurations and not others** | **Shown on the maintainer's dashboard as a missing distinction: the fact that separates the two sides, and the `answers.when` that would say so. Computed from the clusters the page already shows and nothing else, so a configuration below the floor contributes no value and no count. Walked from published files and real reports — the first version compared reports *inside* one cluster, which agree on every fact by construction, and passed only on rows the suite wrote itself** | auto |
| SV58 | Two branches matching the same value | Refused when authored. Not a coin flip while a user waits | auto |

**Observations are never moved.** `seen_key` is salted per cluster, so
reattaching a row would need an HMAC over a pseudonym we deliberately do not
have — and moving rows would either break the uniqueness that makes the
threshold count people, or force us to keep the pseudonym. Nor is a partition
stored: a tree is derived from `answers.when`, so a fork lives in the
maintainer's own solution file or nowhere. `0015` dropped `cluster_partition`,
which nothing had ever written.

**Trees are derived, not authored.** `ingest/tree_build.py` builds one per
problem class from the solutions' own `answers.when` conditions, using `collect`
for the probes — so a publisher writes no tree and there is no second document
to drift. `SV91` walks that path end to end from published files.

**The authoring tool is a section of the dashboard**, and it only ever returns a
suggestion. Not writing trees, but the *fuzzy* half: where one of a
maintainer's answers helped some configurations and not others, which fact
separates them (`repartition.forks`, `SV32`). It is not yet the other fuzzy
proposal — that two hundred singletons which look related might be one
cluster.

## What stays private

| # | Case | Expected | Cover |
|---|---|---|---|
| **SV19** | **The dashboard for an unclaimed domain** | **Refused to everyone. The service this replaces served any product's gap report to whoever asked, which `SERVER.md` calls the line whose crossing ends the company** | auto |
| SV20 | After the domain is claimed | Open, free, private. The same challenge as any other anchor | auto |
| SV21 | Another vendor's figures | No route produces them. The operator's view is a different application on a different listener, so this is a property of the routing table rather than of a guard | auto |
| **SV105** | **Ingest could not make something of a project's files** | **The maintainer is told, on their own dashboard. A class whose decision tree did not derive, and why, in terms of the path they wrote; a version refused outright while the previous one keeps being served, until one is accepted. Both reasons were returned by the ingest worker and stored nowhere — found with a seeded project where none of 34 solutions matched anything, and the page said only "no decision tree could be derived"** | auto |
| **SV106** | **A project with hundreds of configurations** | **Grouped by the solution file a maintainer would edit and graded — did not help, needs a distinction, nobody said, working — with what nothing answers alongside, by people. Every configuration above the floor is returned up to a stated cap, and the count of all of them with it: this was `LIMIT 100` and silent, found at 263. A suggestion on a fact the project's current `collect` does not read offers no `answers.when`, and one where versions split cleanly offers a range** | auto |
| **SV109** | **A solution that holds when the person names the symptom and the machine reads the OS** | **Derives a tree. Readings are switched on first, so the question sat below the reading with nothing to fall back to, and the whole class was refused — the shape a maintainer writes first. Where every solution below the question agrees on one value, the class the person named already says it, so a skipped question falls back to that branch's answer, marked fallback-only (`0011`): a matching answer finds it, a declined or missing one falls back to it, a contradicting one gets nothing. Two values under one OS are still refused, and a tree with a more general answer above keeps that one. Walked over `/diagnose`** | auto |
| **SV108** | **A maintainer's draft sent to `POST /validate`** | **Judged as ingest judges it — the same functions on the same bytes, with the fetching, the anchor probe and the database left out. Seven drafts go through real ingest and through `/validate` with the address they are served from as the anchor: accepted by one is accepted by the other, a refusal names the same file with the same sentence, and a class with no tree has the same reason. Warnings name what ingest accepts and nobody would want — a solution for an undeclared class, duplicate ids — and notes what is fine and worth knowing, a declared class nothing answers yet; neither is ever a refusal. It also returns ingest's own parse of the files, which is how the builder loads existing ones (`W20`). Nothing is stored. Proved able to fail by removing one check from the draft path** | auto |
| **SV107** | **A manifest that names the project's own terms** | **`glossary.keep` survives ingest into the card the mirror serves — anything a manifest carries that ingest does not know is dropped on purpose, and this was. Its terms enter a reader's prompt, so each is one line of at most 64 characters with a letter in it, at most 50, and a key the format does not have is refused rather than ignored: a per-language rendering was measured and not applied by the models it would reach, so there is no field for one** | auto |
| **SV110** | **A month ends** | **Its salt is destroyed without anybody asking. `roll` existed and was tested, and nothing in production called it, so every month's salt stayed on disk and "cannot be tested against any pseudonym once the epoch rolls" was true of the suite only. The ingest worker runs `roll_past` each cycle: every epoch before the current month is closed and its salt overwritten and removed, a salt file with no epoch row as well, and the current month is untouched** | auto |
| **SV111** | **Somebody wants to know what is done with their data** | **`/privacy` answers in German and English, rendered from `/operator` like the imprint and refused with 503 on the same rule when no controller is configured. Every page that links the imprint links it too. It names each thing the server holds with its retention — and the access log it describes is the one the Caddyfile writes: first path segment only, no request headers, addresses masked** | auto |
| **SV104** | **A stranger who guesses a configuration of somebody else's project** | **Learns nothing. `GET /cluster/{hash}` answered how many people had reported that exact configuration, unauthenticated — and a signature is a host and a few coarsened facts, so the hash is a small search, not a secret. No client called it and its `solutions` were always empty, so it is removed rather than guarded, and checked on the routing table as well as over the wire** | auto |
| SV22 | Free text with no consent | Refused by the database. **This one was decorative for a while** — see below | auto |
| SV25 | The public figures | Two integers, with no parameter that could narrow them | auto |
| **SV64** | **A user types whatever is not working** | **Found, whether it is a program (`pip`) or a device and its maker (`nvidia`) — `problem_classes` name other people's software and hardware alike. The token comes from those classes, as nominative use, and from the verified anchor** | auto |
| **SV65** | **A self-asserted name in the index** | **There is no field to assert one. The display name is the verified anchor, and a name in the thing people search *by name* would put the impersonation vector straight back** | auto |
| SV66 | The index's provenance | Signed with the log key against a tree head, and **nothing is published that the head cannot prove**: an entry whose sequence is beyond the signed size waits for the next index rather than being taken on trust | auto |
| **SV67** | **`/search?q=`** | **Does not exist and must not. We promise no query logs, and a name query is worse than the per-domain lookup `SV21` forbids: what a user types is the problem they have. The catalogue is fetched whole** | auto |
| SV68 | A held anchor | Not indexed. A hold means the name itself is contested, and the index is what users search by name | auto |
| **SV70** | **A project published after the last signed head** | **Appears, provable under the head the index was signed with. The cycle that appends is the cycle that signs — otherwise the only issuer is whoever happens to `GET /log/sth`, and discovery quietly freezes while ingest works perfectly** | auto |
| **SV72** | **A diagnosis that turned on a fact the person supplied** | **Still answered — refusing would make asking the question pointless — but it names the switches it took and which of those were supplied, and grades itself `rests_on_supplied`. A rule that failed on a measured fact is a defect worth the publisher's time; one that failed on an answered fact may be nothing wrong at all** | auto |
| **SV71** | **A report whose deciding fact was typed rather than read** | **The dashboard says so, per fact and per reporter. A solution that matched on a measurement and failed is a defect in the rule; one that matched on a value the person supplied may be nothing of the sort, and before the split those arrived identical** | auto |
| **SV73** | **`GET /` asked by a browser and by a monitor** | **A page to the first, the JSON to the second, and `Vary: Accept` on both — without it a CDN keyed on URL alone lets the first browser hit poison `/` for every monitor behind it** | auto |
| SV74 | A page's body | Byte-identical to the file on disk. There is no server-side rendering and therefore no injection surface, asserted rather than argued | auto |
| **SV75** | **A page's inline script against its own Content-Security-Policy** | **Permitted, by a hash per block. A wrong hash does not error — the browser blocks the script and the page comes up with its layout drawn and nothing filled in. A page with no script must still say `'none'`; an empty `script-src` is an illegal header value and the connection dies** | auto |
| **SV76** | **The imprint with no operator configured** | **503, naming the settings that are missing. Nothing is derived — not the hostname, not the `Host` header, not a placeholder — because a page that invents a legal identity is worse than one that admits it has none. `security.txt` is 404 rather than publishing a contact nobody answers** | auto |
| **SV77** | **A maintainer who lost their token proves control again** | **The new token works and every earlier one stops. There is no email to send a reset to, so re-proof is the whole recovery story — and it only works as a sentence somebody can act on if it is complete. Otherwise a leaked token is a 365-day problem no route can end** | auto |
| **SV78** | **A revoked token, an expired one, and none at all** | **One identical answer. A different message for a revoked token is an oracle telling a stranger that a host has been claimed, which is a per-vendor fact** | auto |
| **SV79** | **A stranger starts a claim on a project that is already attested** | **The published value is not rotated. Re-verification probes it, so moving it would make that project's file stop matching and its attestation would be lost — an unauthenticated route that un-enrols anybody. Only a successful `verify` moves it, and only to a value the claimant proved they could publish. A claim already in progress cannot be restarted from outside either, so a stranger cannot pull the ground out from under one** | auto |
| **SV80** | **A maintainer who no longer wants to take part** | **Withdraws, themselves. The mirror is withheld, the attestation withdrawn, the anchor back to `unknown`, and their tokens revoked with it — nothing in their repository is touched, because we only ever held a copy. Logged as `self_withdrawn` and kept distinct from a takedown: a project that left is not a project that was reported, and a log that cannot tell those apart makes leaving look like an accusation. Refused without a token, so nobody can un-enrol anyone else** | auto |
| SV69 | The index across the wire | The document the server signs is the one the client verifies, against a key pinned out of band — fetched from the running server, because a fixture proves the two agree with a file rather than with each other | auto |
| GR1 | Fewer reporters than the floor | Nothing is reported about that vendor. The floor is applied in the query, so there is no result to filter afterwards and forget to | auto |
| **GR1a** | **Fewer distinct reporters than the floor, from repeated submissions** | **Nothing is reported — below it a constellation is identifying, and *identifying* is about people rather than posts. The catch-all counted rows in a list, so k submissions from one client crossed. The floor is `peak_epoch_reporters`, so they now count once** | auto |
| **GR1b** | **A group inside the page — an outcome, a model class, a fact somebody typed — held by one key a month, or one key in each of k clusters** | **Not shown. A `seen_key` is a new key every month and in every cluster, so distinct keys over all time counted one persistent person as k people. A group clears the floor the way a cluster does: k distinct reporters within one cluster and one month** | auto |
| GR2 | At the floor | The report states its own policy (free, private, unconditional) **and its own limits**: without a connected agent it shows *that* something recurs, not *why* | auto |
| GR3 | A weak local model solved it | Read as a UX defect rather than a knowledge gap — the information was available and the product surface failed to convey it | auto |

### SV22 caught a real defect, which is the point of writing cases this way

The constraint was:

```sql
description IS NULL OR (description_consent ? 'destination' AND ...)
```

A `CHECK` passes when it evaluates to NULL, not only when it evaluates to true.
With `description_consent` NULL the right side is NULL, so the whole expression
was `false OR NULL` = NULL — which Postgres accepts. Free text with **no consent
at all**, the case the constraint exists for, went straight in.

Migration `0005` fixes it and strips the rows that got in: the text is deleted
and the observation kept, so the count a user contributed to survives while the
words they never agreed to share do not. The lesson generalises — every `CHECK`
guarding a nullable column needs an explicit `IS NOT NULL`, or it is weakest
exactly where the data is.

## Takedown, and the states an anchor can be in

Removing on every claim without review makes the takedown path a free weapon,
and the ground it clears is the supply side the whole thing depends on. The
answer is not courage — it is reach: we are the mirror, so a takedown reaches
the mirror and not the source.

| # | Case | Expected | Cover |
|---|---|---|---|
| **SV39** | **A notice aimed at a problem class** | **Refused, and the refusal recorded. A manifest that says it handles a product describes what it repairs, not what it is — a takedown reaching a class would let a vendor forbid anyone from naming their software** | auto |
| SV37 | A notice against an anchor we mirror nothing for | Nothing to remove. Where a vendor runs its own endpoint we are not in the path | auto |
| SV36 | A takedown against a mirrored anchor | `attested` → `unknown`, mirror withheld, source untouched. Withdrawal is `degraded`, never `revoked` | auto |
| SV38 | Any takedown | A log entry with a public reason code, and the affected party told where to read it — the same entry | auto |
| SV59 | The reason code | From a closed vocabulary, so weaponised claims are countable | auto |
| **SV81** | **A taken-down anchor is probed and the file is still published** | **It stays `unknown`. The probe is recorded, because control is a true fact — but returning to `live` is an *enrolment* change and `POST /claim/{host}/verify` needs no authentication, so anybody at all could have undone a takedown by republishing a file. Liveness and enrolment were sharing one column; a takedown now writes its own fact, pointing at its statement of reasons, and no automatic path clears it** | auto |
| **SV2c** | **A key change at a live anchor** | **Served, surfaced, never blocked. Rotation and takeover look identical, so it is made visible rather than guessed at — key continuity is the tripwire, the cadence only catches abandonment** | auto |
| SV2d | Deprecation | Three signals kept separate: the developer's word, the forge's act, and an age we measured. The schema has no column for a verdict we have no standing to reach | auto |
| SV10 | A domain we never attested | `unknown`, and the reply says it means nothing about them | auto |
| **SV11** | **A revoked anchor** | **Refused. Revocation is the one state that *is* an accusation — attested and withdrawn for cause, meaning compromise or abuse. Nothing reaches it by accident: every other path produces `degraded`, which means `unknown`** | auto |
| SV2b | Gone 90 days | `unknown`. Serving stops, the transition is logged, and it is still not `revoked` | auto |
| SV2e | The forge reports the repository archived | Shown as an owner's explicit act, kept separate from our own observation | auto |
| SV2f | No commit for two years, anchor live | The age is stated. "Outdated" is a verdict the schema has no column for | auto |
| SV9 | An enterprise endpoint that is down | **No fallback.** Structural: an enterprise anchor has no mirrored source, so nothing here could answer in their place even if it tried | auto |
| SV24 | One vendor's figures requested by another party | No route produces them | auto |

## The enterprise tier

The TXT record proves control of the domain — used rather than a file because a
zone edit is far harder to obtain than a write into `/.well-known/`. The register
supplies the name, copied from GLEIF and never typed: there is no field anywhere
in this system for an enterprise to write its own display name.

The seam between them is stated rather than hidden. GLEIF publishes no domain
field, so what is attested is *"whoever controls this domain asserts this LEI,
and this is the name the register carries for it"* — and the log entry carries a
`not_claimed` line saying so.

| # | Case | Expected | Cover |
|---|---|---|---|
| SV8 | DNS TXT plus a live LEI | Attested `tier: enterprise` with the register's name, and a log entry that does not overstate what was verified | auto |
| **SV8a** | **Each half failing on its own** | **Different answers. A DNS timeout is our inability to ask, a wrong token is their statement, and a retired LEI is the register's — collapsed into "could not attest" a vendor has nothing to act on** | auto |
| SV62 | The register, asked and cached | A record younger than a day answers without a request. When GLEIF cannot be reached, a record up to a week old still answers and says it came from the cache; older, or none at all, says it cannot answer — an old record producing confident refusals would withdraw attestations for entities that are fine. No case depends on GLEIF being up | auto |
| SV63 | The register client's fields | Read from a record exactly as GLEIF's API served it — name, entity and registration status, country, and which publication it came from. A string that is not an LEI, check digits included, is never sent to the register. Replaced a 502 MB Golden Copy mirror nothing had ever loaded (operator, 2026-09-13) | auto |

## Names we do not own

We attest **control of an anchor, never entitlement to a name.** Deciding who
deserves *NVIDIA* would be the chokepoint role this design rejects, and we are
not a trademark register.

A homoglyph is different: `exаmple.org` with a Cyrillic а is not a claim about
who owns a name, it is a technical trick, and UTS 39 is the technical answer.
`scripts/fetch_confusables.py` vendors the hostname-relevant 1,565 mappings of
the real table — approximating it with a hand-written list of lookalikes would
be the kind of plausible assertion this project refuses elsewhere.

**The outcome is a hold, never a block.** A held anchor stays exactly as
reachable as one that never registered. A wrong hold costs a publisher a review
rather than their existence, and that asymmetry is the only reason this can be
conservative at all.

| # | Case | Expected | Cover |
|---|---|---|---|
| SV33 | An anchor confusable with a well-known mark | Not attested; still reachable as `unknown` | auto |
| SV33a | The effect of a hold | It withholds the attestation and nothing else — the source is still mirrored and still served | auto |
| **SV34** | **An anchor carrying a mark it does not own** | **Held, not silently attested. `nvidia-community-fixes.org` may be entirely legitimate — whether a domain may carry somebody else's mark is the question we have no standing to answer, so it goes to a person and meanwhile the anchor works** | auto |
| **SV35a** | **The name check and a declared problem class** | **It never reads one. A project that repairs NVIDIA driver problems has to be able to say so, and if a takedown could reach a class a vendor could forbid anyone from naming their software** | auto |
| SV61 | The confusables table | The real UTS 39 one, with its Unicode version recorded — Cyrillic а folds to a, Greek omicron to o | auto |

## Query, report, and the operator's view

| # | Case | Expected | Cover |
|---|---|---|---|
| **SV12** | **A query** | **Touches no store, and the read path cannot write — enforced by the database, not by a convention** | auto |
| SV15 | A report filed after a solution was tried | Carries the outcome, through `/report`. Which answer it is about is not reported — the protocol carries no solution id — but found by walking the project's own solutions against its facts (`SV32`) | auto |
| SV23 | The operator's outreach view | Ranks unclaimed domains over the threshold. Private to the public is not invisible to the operator | auto |
| **SV82** | **Interactive API documentation** | **Not served, on either listener. FastAPI turns `/docs`, `/redoc` and `/openapi.json` on by default, so Swagger UI — which loads its script from a CDN — was being served because nobody had said not to. Nothing was hidden by turning it off; every route here is public. What was wrong is an unreviewed third-party script delivered under our own name, on an origin whose argument is that it makes no external calls** | auto |
| SV60 | The integration guide | Matches the code — every action named, the 14/90-day schedule, and the claims a publisher reads before they ever see a refusal | auto |
| **SV83** | **Both worked examples** | **Served as the bytes `spec/` publishes, over HTTP, solutions included — a page teaching the format cannot drift from the format. The second exists for a different reason from the first: a desktop project answering for the NVIDIA driver under Wayland, owning neither name. It names `nvidia.driver` in its problem classes and has no field in which to claim to be them, which is the answer to the question that actually stops maintainers** | auto |
| **SV112** | **A repository URL is one identity** | **Four spellings of one repository — bare, trailing slash, `.git`, and another case — collapse to a single anchor identity, because a forge cannot hold two repositories that differ only in case and keeping the typed case would create a confusable pair we invented ourselves. A deep link into the forge's interface, a query, a fragment, plain HTTP, `..` in a segment, an owner with no repository, and a forge nobody has measured are each refused rather than guessed at** | auto |
| **SV113** | **A repository is anchored by repository, not by forge** | **`SERVER.md` has named a git forge as an anchor since it was written and the code could only anchor a domain — nobody can write `https://github.com/.well-known/podshl-challenge`, which excluded every maintainer who owns a repository and no domain. A claim now stores the identity (`https://github.com/owner/repo/`) and the place a forge actually serves file contents (`raw.githubusercontent.com/owner/repo/HEAD/`) as two columns, tells the maintainer where to **commit** the file rather than only where it will be read, and gives two repositories on one forge two anchors — `host` is shared there, so a claim resolved by host would land on whichever row was oldest, which is somebody else's** | auto |
| **SV114** | **A redirect is followed only while the host is the same** | **`probe` follows redirects by hand in order to refuse a cross-host one — a forge handing us somebody else's bytes would have them served under this anchor's name. The guard passed an `httpx.URL` to `urlparse`, which wants a string, so every redirect raised `AttributeError` into the catch-all and answered INTERNAL: the branch never ran, and neither did the check inside it. Unseen because nothing redirected until a forge did — Codeberg answers `/raw/HEAD/` with a `303` to `/raw/branch/<name>/`, which is how Forgejo resolves HEAD. Walked against a real server, because the bug sat between the fetch and the classification and a case calling the classifier would have passed throughout** | auto |
| **SV115** | **A repository goes from claim to served card** | **The whole chain through the public routes — claim, commit the digest, verify with the kept half, say where the files are, ingest, read the card back — with nothing seeded into the database, because every step between them is where this could break. The forge is a local server laid out as a Gitea (`/{owner}/{repo}/raw/HEAD/`), the shape measured against Codeberg and a self-hosted Gitea 1.26.1. A prefix pointing at the repository next door is refused as `outside_anchor`: on a forge, next door belongs to somebody else. And the card comes back under its **identity** while the bare forge host is not an address at all — every repository on `github.com` shares that host, so a mirror answering by host would hand a stranger whichever project was first, under a name it never claimed** | auto |
| **SV84** | **A read instruction naming a path** | **Cannot walk out of a granted root. The root check is a prefix test on both sides and a prefix test does not see `..`, so `<config>/../../.npmrc` starts with `<config>` and reads outside every granted root — `enumerate_read` has refused `..` since it was written and these two never did. The deny list did not close it either: it read the file name and not the field taken out of it, so `api_secret` of any accepted file went through. A directory merely beginning with two dots stays legal, so a substring test is the wrong fix** | auto |
| **SV85** | **A maintainer who proved control** | **Is not thereby mirrored. Proving control and saying where the files are are two acts, and only the first had a route: `INSERT INTO source` existed nowhere but the suite, so the whole documented path ended in a token and silence. `POST /claim/{host}/source` is authenticated, refuses a prefix outside the verified anchor, is idempotent, and queues the source due immediately** | auto |
| **SV86** | **A stranger who can read the published challenge file** | **Cannot claim the host with it. `verify` took no authentication and checked only that the file was there — and the file is public by design, is handed to anonymous callers on purpose, and maintainers are told to leave it published forever. So any passer-by could take the dashboard of any project that followed the instructions, lock out the maintainer, and withdraw the project. A claim now has two halves: the digest is published, the preimage is returned once to whoever started the claim and stored only as a hash. Both are required, the proof is checked in constant time before any fetch, and a spent claim cannot be replayed** | auto |
| **SV87** | **A publisher-controlled name with a short TTL** | **Cannot answer public for the check and `169.254.169.254` for the connection. `fetch.py`'s own docstring forbade resolving twice and the code did it: the host was resolved to be validated, the answers were discarded, and the *name* was handed to `httpx`, which resolved it again at connect time. The check now returns the addresses it checked and the connection is pinned to one of them, with the name kept for SNI so certificate verification still runs against the publisher's identity. A request with no pin fails closed. The case performs the rebinding against two loopback servers and asserts which one received it** | auto |
| **SV88** | **A solution path that is percent-encoded** | **Cannot leave the anchor. `check_manifest` looked for a literal `..` and `under_prefix` compared the still-encoded path, so `%2e%2e/` passed both — while the origin that eventually serves it decodes and normalises, as RFC 3986 says to. On shared forge hosting, where every project is a path under one host, that is another tenant's file mirrored under this anchor's attested name. The encoding is refused outright in a solution path, and containment is compared on the decoded, normalised path** | auto |
| **SV89** | **A solution the maintainer deleted** | **Stops being served. Ingest iterated the manifest's list and stored each entry, and the only `valid_to` was the supersede path inside `store_solution` — which runs only for solutions still listed. So deleting the file, which is how a maintainer withdraws a remedy they have decided is harmful, did nothing at all: the mirror went on serving it under an anchor re-attested for a manifest that no longer declared it. Closed rather than deleted, so what was being served on a given day stays answerable** | auto |
| **SV90** | **One source that fails unexpectedly** | **Costs one source. `ingest_one` caught `IngestRefused` and nothing else and the whole claimed batch ran in one transaction, so an error in the last source discarded every source already ingested *and* the lease that `claim` had written — putting the poisoned source back at the head of the next claim, with no handler in the worker loop to survive it. One savepoint per source, and a cycle that fails is retried rather than fatal** | auto |
| **SV91** | **A published project with no tree authored** | **Gets a diagnosis anyway. `cluster_tree` could walk, validate and load a tree and nothing built one — `INSERT INTO tree` existed only in the suite, so `/diagnose` answered `no_statement` on every real deployment whatever it was sent, and the suite did not notice because the suite inserted the trees itself. The tree is now derived from `answers.when` and `collect`, which every publisher already writes, so it cannot contradict the solutions and needs nothing new learned. This case ingests published files and asks over HTTP: a finding, the same walk graded `rests_on_supplied` when the fact was typed, a `need` carrying a probe that can actually be performed, and `no_statement` where nothing matches rather than the nearest branch** | auto |
| **SV92** | **The dashboard** | **Says which of the maintainer's own solutions answers each thing that recurs, whether it helped, and what was measured as against what somebody typed. It used to show a derived hash, a row of `key=value` and two integers — all true, none of it a statement about a problem — and it left out the outcome label, which had been recorded on every report since the beginning and displayed nowhere. The answer is found by walking their own trees, the same walk `/diagnose` does. A class whose tree answers without any fact deciding anything is not listed as also matching: it matches every configuration equally, and saying so against each of them is noise** | auto |

| **SV93** | **A version to be read, a log to be offered** | **At the gate, as narrow as the client: `program_version` names a bare program, never a path — where it lives is the user's machine's answer — with a flag from a closed list, and never a shell, launcher, privilege, power or disk tool; `container_image_version` names a repository without a tag, because the tag is the answer. `log` belongs only on a question a person answers in their own words — on a machine probe it would be text travelling as a reading — and its `file` is a name shown as a hint, never a path the client would open** | auto |
| **SV98** | **A served head verifies under the key that signed it** | **`sth.issue` rebuilt the head body from the row on every read, with the *current* key's id in it — so after a rotation every stored head would be served with a body its signature never covered, and every monitor would report the log broken on a day nothing was wrong with it. A monitor that cannot tell a rotation from a fork has to treat both as a fork, which is the one alarm the exercise exists to raise. The body that was signed is stored beside the signature, and that is what is served** | auto |
| **SV99** | **A solution edited in place is fetched again** | **The manifest carried an ETag and the solutions did not, so a 304 on the manifest meant "unchanged" for the whole source — and a solution file edited in place, which is what fixing the wording of a remedy looks like, was never fetched again. The mirror served the old text indefinitely and the maintainer could not find out, because everything visible to them said the source was up to date. Each solution remembers its own validators, and a 304 on the manifest is followed by a conditional GET per solution** | auto |
| SV96 | An inclusion proof for a signed head the caller already holds | Answered for that head's size, not the current tree's. The route knew only the current tree, so an append between fetching the head and fetching the proof left the client with a path to a root it had no signature for — a check that fails for the wrong reason teaches people to ignore it. Sizes past the tree, and leaves past the size, are refused | auto |
| **SV95** | **A reading that contradicts the only rule of a class** | **Gets no answer — not that rule's answer. A class whose single solution is decided only by asking carries it on its root so that "I would rather not say" still lands somewhere, and the walk could not tell that fallback from a settled answer: on "no branch matches" it returned whatever it carried, three lines below a comment saying this is not an opportunity to pick the nearest. engram's `ollama.endpoint.unreachable` told a machine that had read `OLLAMA_HOST=0.0.0.0` and Ollama 0.33 that no Ollama was running. The fallback is marked now (`0011`): a missing or declined fact lands on it, a contradicting value does not, and a settled answer above a switch still applies because its own conditions were met on the way. Found by walking the published path in the real window** | auto |
| **SV94** | **Words a person agreed to send** | **Reach the maintainer. Free text was withheld by default, attached only under its own consent naming the recipient, refused by a CHECK without it — and then shown nowhere, so the error message a maintainer can get no other way stopped in the database. Now on their dashboard, for clusters at or above the floor only, as it arrived. Bounded: a report carrying more than 16 KiB of text is refused before anything is stored** | auto |

## Running it somewhere

| # | Case | Expected | Cover |
|---|---|---|---|
| **DP1** | **The units a host installs** | **Start what exists, and are supervised. The ingest worker was a loop nobody supervised, and there was nothing to install on a host at all. `deploy/systemd/` is checked against the code: every `module:object` imports, every process restarts when it dies, is confined, and both listeners are on loopback — the public one is reached only through the TLS proxy, the operator's own view never — and none of them sets the development loopback exception** | auto |

## Takedown

| # | Case | Expected | Cover |
|---|---|---|---|
| **SV103** | **The published monitor refuses a log it cannot verify** | **`spec/monitor/verify_log.py` is what this project hands a third party and says *check us with this*. Nothing ran it, and it did not do the first thing its own docstring claims: it compared the `log_id` the head *said* it had against the one you pinned — a string in a document the operator serves. Everything else it checked holds perfectly inside an invented log, because an invented log is consistent with itself. Demonstrated rather than argued: the case stands up a forged log one entry short, carrying its own honest root, the genuine key and the pinned id. The version that shipped reported "served entries match the signed root" and exited 0. The signature is the one thing a forged head cannot have, and it is checked now — against the served key, held to the pinned id** | auto |
| **SV102** | **No route waits on the database from the event loop** | **Every handler that touched the database was `async def` with a blocking `psycopg` call inside, so the whole server was one thread doing one thing and the pool's eight connections could never be more than one. Measured here before the fix: with two 0.4-second queries in flight, a trivial route served **8 requests in five seconds** with a median of **783 ms**; with the same handlers off the loop, **206 requests** and **4 ms** — what an idle server gives. Not a throughput argument, and peak throughput barely moved: it is that one slow query froze everybody, and a slow query needs no bug — a lock, a cold index, a dashboard over a large project. Two shapes are allowed: a handler that awaits nothing is `def` and Starlette gives it a worker; one that must await the request body stays `async def` and hands the blocking half to `run_in_threadpool`** | auto |
| **SV101** | **A notice states that whoever filed it means it** | **`notice.html` has always sent `statement_of_good_faith: true` and the route always threw it away, so the one thing the law asks a notifier to assert was collected by a checkbox and discarded by the server — a safeguard on the page and nowhere else, and nothing at all for a notice filed through anything but that page. Filing stays free, remote and unauthenticated, because a notice comes from a stranger; what can be asked of a stranger is that they say in the request that they mean it. A truthy value is not a statement, so `1` and `"yes"` are refused alongside `false` and absence. The queue shows it to whoever decides, and a CHECK refuses a row that records a denial** | auto |
| **SV100** | **The decision is made on the operator's own listener** | **Both halves over HTTP, against both listeners at once. The public one records and takes no authentication, because a notice is filed by a stranger. The operator's one decides — `/notices`, `/notice/{id}/act`, `/notice/{id}/reinstate` — on a different application on a different port, and every route there needs the bearer token: loopback is a wall, not a person, and a browser tab on the operator's machine is already inside it. A `Host` the listener was not told about is refused *before* the token is looked at, so a request arriving under a name that merely resolves to 127.0.0.1 never reaches a comparison it could time. The notifier is on the queue and on no public route; the decision and its reversal are public log entries that do not name them** | auto |
| **SV97** | **A notice is recorded; a person decides** | **`POST /notice` takes no authentication and cannot — a notice is filed by a stranger — and it used to perform the takedown in the same request: mirror withheld, attestation withdrawn, anchor marked. That made notice-and-action a free, remote, unauthenticated un-enrolment of any mirrored project, which is the weapon this path must not be. "Removing on notice" is about a *person* removing on notice. A notice is `pending` until somebody on the operator's own listener acts; the decision is a public log entry with a reason code and the affected party is told where to read it; and the same person can reverse it, which is another entry naming the one it reverses. The original entry never changes — a log that could forget a takedown could forget anything** | auto |

## What is not covered here

Every `SV` case in `SERVER.md` now has an implementation. What remains is not a
gap in the cases but a gap in the operation:

| | |
|---|---|
| Proposing clusters to merge | Trees are **derived**, and where an answer forks is on the dashboard (`SV32`). The other fuzzy half — that many singletons which look related might be one problem — is not built |
| The ingest worker on a real host | `deploy/systemd/podshl-ingest.service` supervises it (`DP1`); nobody has run it on a host yet, because there is no host |
| A second log watcher | One monitor proves a log self-consistent. Two comparing roots at the same tree size is what detects a split view |
| The client's attestation display | `SV10` and `SV11` are checked here, on the server's side of the wire. The client now shows a published answer's confirmation date, staleness and log entry (`AT1`); it does not yet verify an inclusion proof for that entry itself |
