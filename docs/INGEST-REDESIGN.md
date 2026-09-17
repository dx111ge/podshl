# Ingest redesign: fetch what is used

**Status: agreed on 2026-09-16, built on 2026-09-17** (migration `0022`,
`ingest/on_demand.py`, `SV123`–`SV133`). It replaced the ramp described in
[SERVER.md](SERVER.md) and the case `SV121`. Where the build differs from the text
below, it says so in *As built*.

## Why

Today every enrolled source is fetched on a timer, whether anybody ever asks
about the project or not (`ingest/store.py`, `record_ingest`): the interval is
a twenty-fourth of the time since the last change, at least fifteen minutes,
at most a day. Every pass costs **N + 2** requests (`ingest/scheduler.py`,
`ingest_one`): the anchor's challenge file, the manifest, and each solution file
with its own conditional `GET`, because a manifest that did not change says
nothing about a solution edited in place.

That is fine at the scale of today and wrong at the scale this is meant for:

| | today, per day |
|---|---|
| 10,000 projects, 8 solutions each, all quiet | about 100,000 requests (~1.2/s) |
| 100,000 projects, same shape | about 1,000,000 requests (~12/s) |

Nearly all of it is `304` about projects nobody asked about. A small project
that no person ever looks up still costs ten requests a day, for ever, and at
100,000 projects the operator becomes a noticeable client of
`raw.githubusercontent.com` for no reason.

The design below keeps what makes the mirror trustworthy and stops fetching
what nobody uses.

## What does not change

* **The query path serves from the database.** A person's request never
  depends on a forge being up *for a project that is in use*. The one
  exception, a project nobody has used for two weeks, is spelled out below and
  bounded.
* **Nothing is served until the whole source validates**, and a refusal
  reaches the maintainer's dashboard (`SV105`).
* **The transparency log vouches for exactly the files served.** The client
  keeps checking that a log entry covers what it was given.
* **Conditional requests** (`ETag`, `Last-Modified`) everywhere, through the
  same pinned client and the same prefix confinement.
* **The signed index lists every enrolled project**, used or not, so a project
  can always be found by name.

## The model: hot and cold

A source is **hot** when it was used in the last 14 days, and **cold**
otherwise. "Used" means a request that needed its content: the mirror card
(`GET /mirror/{host}`) or a diagnosis (`POST /diagnose`). Enrolling or
re-enrolling a source (`POST /claim/{host}/source`) also makes it hot, because
the maintainer has just published and needs to see at once whether it
validated.

| | hot | cold |
|---|---|---|
| stored files, card, trees | kept | **kept** |
| fetched on a timer | once a day, file by file (the check a stale digest cannot fool) | no |
| checked when used | if the last check is older than 1 hour — in the background, the stored version is served meanwhile | **always, before anything is served** |
| source unreachable when checked | stored version served, retried later | **nothing served**; the answer says the project's current files cannot be checked right now |
| in the signed index | yes | yes |
| anchor (claim) checked | weekly, and at most daily alongside a use | weekly |

**Cold keeps its rows.** Only the fetching stops. Two reasons:

* `cluster_partition` references `tree_node` with `ON DELETE CASCADE`
  (`sql/0003_clusters.sql`). A maintainer's split of a cluster hangs off the
  tree it was made on; deleting trees when a project goes quiet would delete
  the maintainer's work.
* Storage is not the cost. A project's files are a few kilobytes; the requests
  are what cost. Keeping the stored version and its validators turns the first
  use after a pause into one conditional request, answered `304` in the common
  case, instead of a full reload.

If storage ever matters, content untouched for a long time (a year, say) can be
dropped then, with trees *closed* (`valid_to`) rather than deleted. That is not
part of this change.

**Why a cold project is checked before it is served, and not afterwards.** A
maintainer withdraws a harmful answer by deleting the file, and the mirror
stops serving it on the next ingest (`retract_missing_solutions`). A project
nobody used for two weeks has not been ingested for two weeks; serving its
stored version first would hand out exactly the answer its maintainer took
back. So a cold project fails closed: checked, or not served.

## Checking the claims

An anchor's challenge is re-checked independently of how warm its project is,
because the published promise is about the anchor: **`stale` after 14 days
without a successful check, `unknown` after 90** (`STALE_AFTER_DAYS` and
`UNKNOWN_AFTER_DAYS` in `config.py`, applied in `anchor/sweep.py`).

* **Weekly for every anchor**, one request each. Two chances inside the
  fourteen days.
* **At enrolment** and **when a cold source is loaded**, as today's pass does.
* **At most daily for a hot source**, riding along with a check that is
  happening anyway.

At 100,000 anchors the weekly check is about 14,300 requests a day
(~0.17/s).

## Detecting a change with one request

Today a check costs N + 1 requests after the challenge. The manifest may
instead carry the digest of each solution file:

```yaml
solutions:
  - path: solutions/wrong-archive-for-this-system.md
    sha256: 4f1c…
  - solutions/find-out-which-archive.md     # a plain entry stays valid
```

* **Optional**, and a plain string entry keeps working exactly as today. The
  web builder writes digests; nobody has to compute one by hand.
* With digests, a check is one conditional `GET` of the manifest. A `304` means
  nothing changed; otherwise only files whose digest changed are fetched.
* **A digest is a claim and is checked.** A fetched file whose SHA-256 differs
  from the digest the manifest names is refused like any other invalid file, and
  the maintainer is told on the dashboard which file and which digest.
* **A digest can go stale by hand-editing** a solution without regenerating the
  manifest. So digests speed up the check but do not replace it: every source is
  fully checked, file by file, when it is loaded from cold and at least once a
  day while it is hot.

This is a specification change (`spec/SPEC.md`, the manifest schema, the
builder) as well as an operator change.

## The request path

```
request needs source S
│
├─ S is hot, checked less than 1 hour ago ─────────────► serve stored
│
├─ S is hot, checked longer ago ─────────► serve stored, queue a check of S
│
└─ S is cold ──► check S now (bounded, see below)
                 ├─ unchanged ─────────────────────────► serve stored
                 ├─ changed, valid ─► store, rebuild trees, log entry, serve new
                 ├─ changed, invalid ─► keep stored version out of service,
                 │                      tell the maintainer, answer "cannot
                 │                      use this project's files right now"
                 └─ unreachable / timed out ─► answer "cannot check this
                                               project's files right now"
```

A cold source that is checked successfully becomes hot.

## Protection

Without limits, anybody could walk the signed index and ask about every cold
project in turn, making the operator hammer every forge at once.

* **A bounded pool for on-demand checks**, four at a time to start with. A
  request whose check has not started or finished within about **five seconds**
  gets "this project is being loaded, try again in a moment"; the check carries
  on.
* **One check per source at a time.** Concurrent requests for the same cold
  source wait for the same check.
* **Back-off per source after a failure**: no new on-demand check for 1 minute,
  doubling up to 1 hour, reset by a success. Requests in that window get the
  "cannot check right now" answer without a fetch.
* The existing per-pseudonym `query_budget` stays as it is.

## What the operator learns

`SERVER.md` says today that a query touches no store — "otherwise we would learn
what was asked". This design changes that, deliberately and narrowly:

* **Per source, one date: the day it was last used.** No time of day, no count,
  nothing about who asked or from where.
* **Written at most once per source per day**, in its own short transaction.
  The query itself keeps running read-only (`db.read()`); the date is not written
  from inside it.
* The forge sees the operator fetching a project's files on a day somebody used
  it. It learns nothing about the person.

`SERVER.md` is to state this in those words when the change lands, rather than
keep a sentence that is no longer true.

## Maintainers

* **Publishing** (`POST /claim/{host}/source`, re-POSTed after a change) loads
  and validates at once and makes the source hot. That is how a maintainer sees
  a refusal immediately.
* **A ready-made GitHub Action** that re-POSTs after a push to `.podshl/` is
  worth shipping with the builder; with it, a maintainer's change is live within
  a minute without anybody polling.
* **The dashboard** reads what is stored and does not make a source hot.
  Looking at your own dashboard is not a person with a problem.

## The log and the index

* **Unchanged content writes nothing to the log.** A cold source that answers
  `304`, or whose files hash to what the last entry records, is served under
  that entry.
* **Changed content appends an entry**, exactly as ingest does today.
* **Cooling writes nothing** — to the log or to the index. The index lists every
  enrolled project whatever its temperature, so its ETag does not change because
  a project went quiet.

## Data model

A migration (`0022`) adds to `source`:

| column | meaning |
|---|---|
| `last_used` | `date`, the last day the source was used; set to the migration day for every existing source, so everything starts hot and cools over two weeks |
| `last_checked` | `timestamptz`, the last completed check of its files |
| `check_backoff_until` | `timestamptz`, the per-source back-off after a failed on-demand check |

Hot and cold are derived (`last_used >= current_date - 14`), not stored. The
timer-driven `next_fetch_at` stays for two jobs only: the daily full check of
hot sources and the weekly anchor check.

## Load

100,000 projects, 5 % of them used in any two weeks, 8 solutions each:

| | today | redesigned |
|---|---|---|
| projects nobody uses | up to ~1,000,000 requests/day | 0 |
| anchor checks | inside the daily pass | ~14,300/day |
| hot projects | inside the daily pass | at most ~120,000/day, and only if every one of them is used every hour; with digests one request each |
| cold projects | — | one check per actual first use after a pause |

At worst about a seventh of today; in practice far less, because hardly any
project is used every hour.

## Rollout

1. Specification: the optional `sha256` on solution entries, the schema, the
   builder writing it (`W19`/`W20` extended).
2. Migration `0022`, the request-path check, the pool and back-off, the weekly
   anchor job; the timer pass reduced to hot sources.
3. `SERVER.md` rewritten for the new schedule and the new sentence about what
   the operator learns; `SV121` replaced by the cases below.
4. **Staging first**, as for every migration: `deploy/compose/update.sh --staging`.
   Watch the requests per hour against the forge before and after.
5. The GitHub Action for maintainers, documented in `ONBOARDING.md`.

## Cases

Written into [TESTCASES-SERVER.md](TESTCASES-SERVER.md) as `open`:

| # | Case |
|---|---|
| SV123 | A hot source checked less than an hour ago is served without a fetch |
| SV124 | A hot source checked longer ago is served at once and checked in the background |
| SV125 | A source nobody used for 14 days is not fetched on any timer |
| SV126 | A cold source is checked before anything is served, and becomes hot |
| SV127 | A cold source whose files cannot be checked is not served, and the answer says why |
| SV128 | A withdrawn solution is never served from a cold source |
| SV129 | Every anchor is checked weekly; `stale` and `unknown` still arrive on time |
| SV130 | On-demand checks are bounded: pool, one per source, back-off after failure |
| SV131 | A maintainer's cluster partitions survive their project going cold |
| SV132 | The operator records only the day a source was last used, at most once a day, outside the query's read-only transaction |
| SV133 | A solution whose content does not match its manifest digest is refused and the maintainer is told |

## As built

* **Two more columns than the table above**: `last_full_check`, because a check
  that stopped at the manifest's `304` must not count as the daily file-by-file
  one, and `check_failures`, which the doubling back-off needs.
* **The digests travel beside the paths.** The parsed manifest keeps
  `solutions` a list of strings and carries `solution_sha256` next to it, so the
  card, the builder's import and every reader downstream see what they saw.
* **"Serve stored, queue a check"** sets the source's `next_fetch_at` to now; the
  worker takes it within a minute. The request never waits on the forge.
* **A cold check that fails does not make the source hot.** `last_used` is
  written for a cold source only after its check succeeded, so the next request
  asks again instead of being served what nobody vouched for.
* **`cluster_partition` no longer exists** (`0015` dropped it with `link`).
  What a project holds today is its files, its trees and the clusters of reports
  about it (`cluster.source_id`); the reason to keep rows is the same, and
  `SV131` holds it for those.
* **The weekly anchor job also grades.** `sweep.apply_grades` had no caller; it
  runs after every weekly check unless that run was mostly silence.
* **Enrolment waits up to five seconds** for its check and returns the outcome
  as `check`; a longer one carries on and the dashboard shows it.
* The GitHub Action for maintainers (rollout step 5) is a workflow to copy,
  `examples/github-action/podshl.yml`, with curl and nothing from the
  marketplace. It waits for `raw.githubusercontent.com` to serve the pushed
  files before it re-enrols, because that cache is minutes long and the
  operator would otherwise read the old copy and call it unchanged (`GA1`).

## Settled

Agreed on 2026-09-16: 14 days to go cold, one hour before a hot source is
checked again on use, a cold source fails closed, rows are kept when a source
cools, anchors are checked weekly, digests are optional.
