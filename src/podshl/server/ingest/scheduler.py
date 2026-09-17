"""What drives ingest, and why it is a separate process.

`SERVER.md`'s scaling claim is that **ingest load tracks the number of projects,
request load tracks the number of users, and the two are independent.** That is
only true if they can be scaled and restarted separately, so this is its own
process rather than a background task inside the web application. Putting the
crawler in the request path would make a crawl spike cost request latency, and
scaling the web tier would multiply the crawl.

Sources are claimed with `FOR UPDATE SKIP LOCKED` and a bumped `next_fetch_at`
as the lease. There is no reaper and no leader election: a worker that dies
releases its sources when the lease expires, and N workers scale linearly with
no coordination between them.

**Only what is used is fetched** (`docs/INGEST-REDESIGN.md`). A source is
*hot* while it was used in the last `HOT_DAYS` days, and only hot sources are
on the timer — once a day, file by file. A cold one is checked when somebody
next asks about it, before anything is served (`on_demand.py`).

**Anchors are checked weekly on their own** (`sweep_anchors`), whatever their
project's temperature, and at most daily alongside a check that happens
anyway. That cadence catches abandonment. It does not catch a takeover —
whoever acquires an expired domain can rewrite the challenge file at any
cadence — which is why key continuity is the tripwire and the cadence is not
the security property.
"""
from __future__ import annotations

import json

import httpx

from .. import log_store, sth
from ..anchor import challenge, dns_txt, forge, sweep
from ..anchor.result import Reason
from ..errors import IngestRefused
from . import fetch, manifest, store

#: How many sources one worker takes at a time. Small enough that a crash loses
#: little, large enough that the claim query is not the bottleneck.
BATCH = 200

#: The lease. A worker that dies releases its sources after this.
LEASE_MINUTES = 10

#: A source used within this many days is hot and stays on the timer.
HOT_DAYS = 14

#: A hot source checked longer ago than this is checked again when it is used.
RECHECK_ON_USE_MINUTES = 60

#: A check that reads every file, whatever the manifest's digests say, is due
#: after this. A little under a day, so the timer's daily check is always one.
FULL_CHECK_HOURS = 20

#: The anchor's challenge rides along with a check at most this often.
ANCHOR_ALONGSIDE_HOURS = 24

#: Every anchor is checked at least this often, used or not. Two chances inside
#: the fourteen days after which it is `stale`.
ANCHOR_EVERY_DAYS = 7


def _refused(conn, source_id: int, why: str) -> dict:
    """A version that was not accepted, with the reason kept for its maintainer.

    The previous version keeps being served. Before `0017` the reason went into
    this return value and nowhere else, so the only thing a maintainer saw of a
    refused change was that it never arrived (`SV105`).
    """
    with conn.cursor() as cur:
        cur.execute("UPDATE source SET last_refusal = %s WHERE id = %s", (why, source_id))
    return {"source": source_id, "outcome": "refused", "why": why}


#: What a check needs to know about a source.
SOURCE_COLUMNS = ("id, anchor_id, manifest_url, fetch_prefix, etag, last_modified, "
                  "(last_full_check IS NULL OR last_full_check < now() - "
                  f"interval '{FULL_CHECK_HOURS} hours') AS full_due")


def claim(conn, limit: int = BATCH) -> list[dict]:
    """Take a batch of due **hot** sources. The bumped `next_fetch_at` *is* the lease.

    A cold source is not on the timer at all: nobody asked about it for two
    weeks, and it is checked when somebody next does (`SV125`).
    """
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE source SET next_fetch_at = now() + interval '%s minutes' "
            "WHERE id IN ("
            "  SELECT id FROM source "
            "  WHERE next_fetch_at <= now() AND mirror_state = 'serving' "
            "    AND last_used >= current_date - %s "
            "  ORDER BY next_fetch_at FOR UPDATE SKIP LOCKED LIMIT %s) "
            f"RETURNING {SOURCE_COLUMNS}",
            (LEASE_MINUTES, HOT_DAYS, limit),
        )
        return cur.fetchall()


def probe_anchor(conn, anchor: dict, *, client: httpx.Client | None = None):
    """Check an anchor's challenge where it was proved, and record what came back.

    `fetch_root`, not `value`: a repository's files come from the forge's raw
    prefix, and re-verification has to read the same place the claim was proved
    at or an anchor would go stale for being a repository. A DNS anchor is
    asked through its TXT record, as it was enrolled.
    """
    if anchor["kind"] == "dns":
        probed = dns_txt.probe(anchor["host"], anchor["challenge_token"])
    else:
        probed = challenge.probe(forge.fetch_root(anchor), anchor["challenge_token"],
                                 client=client)
    sweep.record(conn, anchor["id"], probed)
    return probed


def _checked(conn, source_id: int, *, full: bool) -> None:
    """A check that completed, changed or not: the clocks move, the back-off ends."""
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE source SET last_checked = now(), "
            "  last_full_check = CASE WHEN %s THEN now() ELSE last_full_check END, "
            "  check_backoff_until = NULL, check_failures = 0 WHERE id = %s",
            (full, source_id))


def ingest_one(conn, source: dict, *, client: httpx.Client | None = None,
               full: bool | None = None, probe: bool = False) -> dict:
    """Fetch, verify the anchor, validate, store. Returns what happened.

    Nothing is stored unless the whole source validates. A partially valid
    source is not served at all — half a mirror is worse than none, because the
    half that is missing is invisible to whoever reads the other half.

    `full` asks about every solution file whatever the manifest's digests say;
    without it, a manifest that answers `304` and states a digest for every file
    is the whole check. It defaults to what the source row says is due. `probe`
    checks the anchor whatever its clock says; otherwise that rides along at
    most daily, and always before anything changed is attested.
    """
    from . import confusable, tree_build, validate

    owned = client is None
    client = client or fetch.pinned_client()
    try:
        if full is None:
            full = bool(source.get("full_due", True))
        with conn.cursor() as cur:
            cur.execute("SELECT id, kind, value, host, probe_prefix, challenge_token, "
                        "  (last_checked IS NULL OR last_checked < now() - "
                        f"   interval '{ANCHOR_ALONGSIDE_HOURS} hours') AS due "
                        "FROM anchor WHERE id = %s",
                        (source["anchor_id"],))
            anchor = cur.fetchone()

        # The challenge rides along, one extra request, at most daily — the
        # weekly sweep covers every anchor anyway (`SV129`).
        probed = (probe_anchor(conn, anchor, client=client)
                  if probe or anchor["due"] else None)

        fetched = fetch.get(source["manifest_url"], prefix=source["fetch_prefix"],
                            etag=source["etag"], last_modified=source["last_modified"],
                            client=client)

        card = store.current_card(conn, source["id"])
        if fetched.not_modified and card is None:
            # A validator with nothing behind it: the row remembers an ETag but
            # no card was ever stored under it. Ask again without the validator
            # rather than reporting as unchanged a mirror that holds nothing.
            fetched = fetch.get(source["manifest_url"], prefix=source["fetch_prefix"],
                                client=client)

        def anchor_reason():
            return probed.reason.value if probed is not None else "not_due"

        if not fetched.usable:
            store.record_ingest(conn, source["id"], fetched, changed=False)
            return {"source": source["id"], "outcome": fetched.reason.value,
                    "anchor": anchor_reason()}

        digests = {}
        if fetched.not_modified:
            digests = dict(card["json"]).get("solution_sha256") or {}
            listed = dict(card["json"])["solutions"]
            if not full and listed and all(rel in digests for rel in listed):
                # **One request is the whole check.** The manifest did not change,
                # it states the digest of every file, and the stored files were
                # accepted against those digests. A hand-edit that left a digest
                # stale is what the daily full check is for.
                store.record_ingest(conn, source["id"], fetched, changed=False)
                _checked(conn, source["id"], full=False)
                return {"source": source["id"], "outcome": "unchanged", "check": "quick",
                        "anchor": anchor_reason(), "solutions": len(listed),
                        "retracted": [], "log_seq": None}

        if fetched.not_modified:
            # **A 304 on the manifest is not a 304 on the source.** The manifest
            # lists the solution files; it does not change when one of them is
            # edited in place, so "unchanged" here used to mean the solutions
            # were never asked about again. They are asked about below, each
            # with its own validator, and the manifest is the stored one.
            m = dict(card["json"])
            manifest_hash = bytes(card["content_hash"])
        else:
            try:
                m = manifest.parse_manifest(fetched.body)
                # The identity too, because a repository anchor proves two
                # locations at once and a maintainer publishes the one a
                # person can visit. Empty for a domain, where they are the
                # same place.
                validate.check_manifest(
                    m, source["fetch_prefix"],
                    anchor["value"] if anchor["kind"] == "repo" else "")
            except IngestRefused as e:
                # Refused, and the previous version keeps being served. A broken
                # commit must not take a working mirror down with it.
                store.record_ingest(conn, source["id"], fetched, changed=False,
                                    remember_validators=False)
                return _refused(conn, source["id"], str(e))
            manifest_hash = fetched.content_hash()
            digests = m.get("solution_sha256") or {}

        # Each listed file, conditionally where a stored row can be conditional
        # on something. A 304 keeps the stored row; anything else is parsed and
        # validated exactly as a first fetch would be.
        known = store.current_solutions(conn, source["id"])
        solutions = []
        base = source["manifest_url"].rsplit("/", 1)[0] + "/"
        def refuse(why: str) -> dict:
            store.record_ingest(conn, source["id"], fetched, changed=False,
                                remember_validators=False)
            return _refused(conn, source["id"], why)

        for rel in m["solutions"]:
            have = known.get(rel)
            stated = digests.get(rel)
            matches = (have is not None and stated is not None
                       and bytes(have["content_hash"]).hex() == stated)
            if matches and not full:
                # The manifest changed, this file's digest did not, and the stored
                # file is the one the digest names: nothing to ask about.
                sol = {"id": have["solution_id"], "answers": have["answers"],
                       "proposes": have["proposes"], "text_by_lang": have["text_by_lang"],
                       "severity": have["severity"], "path": have["path"]}
                solutions.append((sol, bytes(have["content_hash"]), None, None))
                continue
            # A stored file the digest does not name is asked for in full: a
            # `304` about it would say nothing about the file the digest names.
            conditional = have is not None and (stated is None or matches)
            got = fetch.get(base + rel, prefix=source["fetch_prefix"], client=client,
                            etag=have["etag"] if conditional else None,
                            last_modified=have["last_modified"] if conditional else None)
            if got.not_modified and conditional:
                sol = {"id": have["solution_id"], "answers": have["answers"],
                       "proposes": have["proposes"], "text_by_lang": have["text_by_lang"],
                       "severity": have["severity"], "path": have["path"]}
                solutions.append((sol, bytes(have["content_hash"]), None, None))
                continue
            if not got.usable or got.not_modified:
                return refuse(f"{rel}: {got.reason.value}")
            try:
                # A digest is a claim, checked like any other part of the files.
                validate.check_digest(rel, got.body, stated)
                sol = manifest.parse_solution(got.body, rel)
                validate.check_solution(sol, m.get("collect") or [])
            except IngestRefused as e:
                return refuse(str(e))
            solutions.append((sol, got.content_hash(), got.etag, got.last_modified))

        commit = m.get("commit")
        card_id, card_changed = store.store_card(conn, source["id"], m, manifest_hash, commit)
        changed = card_changed
        for sol, digest, etag, last_modified in solutions:
            changed |= store.store_solution(conn, source["id"], sol, digest, commit,
                                            etag=etag, last_modified=last_modified)

        # Removing the file is how a maintainer withdraws a solution, and until
        # this line it did nothing at all: only listed solutions were touched, so
        # a remedy someone decided was harmful went on being served forever.
        retracted = store.retract_missing_solutions(
            conn, source["id"], [sol["id"] for sol, *_ in solutions])
        changed |= bool(retracted)

        # The decision trees `POST /diagnose` walks. Derived from the solutions
        # rather than authored separately, so every project that has already
        # published gets a diagnosis endpoint without writing anything new — and
        # so the tree cannot contradict the solutions, because it is a view of
        # them. A class whose tree will not build is reported rather than
        # refused: the tree is our derivation of their document, not their
        # document.
        trees = tree_build.rebuild_all(
            conn, source["id"], [sol for sol, *_ in solutions], m.get("collect") or [], commit)

        store.record_ingest(conn, source["id"], fetched, changed=changed,
                            declared_status=m.get("status"), successor=m.get("successor"))
        _checked(conn, source["id"], full=full)
        # Accepted, so no refusal describes it any more; and which classes got
        # no tree, and why, is kept where the maintainer's dashboard reads it.
        with conn.cursor() as cur:
            cur.execute("UPDATE source SET last_refusal = NULL, classes_without_a_tree = %s "
                        "WHERE id = %s", (json.dumps(trees["no_tree"]), source["id"]))

        # A held anchor is mirrored and served like any other; what it does not
        # get is an attestation. Declining to attest is not blocking — the anchor
        # stays exactly as reachable as one that never registered — and that is
        # what lets this be conservative without becoming a chokepoint.
        # For a repository the labels that matter are the owner and the name;
        # the host is the forge's and is shared by everything on it.
        hold = confusable.hold_reason(conn, anchor["host"],
                                      kind=anchor["kind"], value=anchor["value"])
        with conn.cursor() as cur:
            cur.execute("UPDATE anchor SET attest_hold = %s WHERE id = %s",
                        (hold, anchor["id"]))

        # Attest only where the anchor was actually confirmed in this same pass.
        # An anchor that could not be checked is not evidence of anything, and
        # attesting on it would be asserting what we did not verify — so a
        # change is what makes a probe that was not due happen after all.
        seq = None
        if changed and probed is None:
            probed = probe_anchor(conn, anchor, client=client)
        if changed and probed.confirmed and hold is None:
            seq = store.attest(conn, source["id"], anchor["id"], anchor,
                               commit, manifest_hash)
            with conn.cursor() as cur:
                cur.execute("UPDATE card SET log_seq = %s WHERE id = %s", (seq, card_id))

        return {"source": source["id"], "outcome": "stored" if changed else "unchanged",
                "check": "full" if full else "partial",
                "anchor": anchor_reason(), "solutions": len(solutions),
                "retracted": retracted, "trees": trees["trees"],
                "no_tree": trees["no_tree"], "commit": commit, "log_seq": seq,
                "attest_hold": hold}
    finally:
        if owned:
            client.close()


def run_once(conn, limit: int = BATCH) -> list[dict]:
    """One cycle. Called on a timer by the worker, and directly by the suite.

    **The cycle that appends is the cycle that signs.** The discovery index
    publishes nothing its signed head cannot prove, so without a head issued
    here the only issuer would be whoever happens to `GET /log/sth` — and a
    newly published project would stay invisible while ingest kept working
    perfectly. Discovery would depend on an unrelated endpoint being polled,
    which is not a dependency anybody would think to look for.

    Issuing is idempotent per tree size, so signing on every cycle that changed
    something costs one row and never a second timestamp over the same root.
    """
    before = log_store.tree_size(conn)
    out = []
    with fetch.pinned_client() as client:
        for source in claim(conn, limit):
            # **One savepoint per source, and nothing escapes it.**
            #
            # `ingest_one` catches `IngestRefused` and nothing else, and the
            # whole batch ran inside one transaction — so a single unexpected
            # error in source 200 discarded the 199 already ingested *and* the
            # lease bump that `claim` writes, which put the poisoned source back
            # at the front of the next claim. The worker loop has no handler
            # either, so the process died and came back to the same source: a
            # permanent ingest outage for every project, caused by one of them.
            #
            # A savepoint keeps the blast radius at one source, which is the same
            # rule the rest of this file already obeys — a partially valid source
            # is not served at all, and now a *failing* one costs no more than
            # itself.
            try:
                with conn.transaction():
                    out.append(ingest_one(conn, source, client=client))
            except Exception as e:  # noqa: BLE001
                out.append({"source": source["id"], "outcome": "error",
                            "why": f"{type(e).__name__}: {e}"})
    if log_store.tree_size(conn) != before:
        sth.issue(conn)
    return out


def sweep_anchors(conn, limit: int = BATCH, now=None,
                  anchors: list[int] | None = None) -> dict:
    """Check every anchor not checked for a week, then move the ones failing long enough.

    **Independent of how warm the project is**, because `stale` after fourteen
    days and `unknown` after ninety are promises about the anchor (`SV129`).
    Until the redesign the check rode along with ingest, and ingest ran for
    every source every day; with cold sources off the timer, an unused
    project's anchor would otherwise never be asked again.

    **And `apply_grades` is called.** It was written, tested on its own, and
    called by nothing — so an anchor whose challenge disappeared recorded
    `failing_since` faithfully and stayed `live` for ever. The weekly check and
    the grading are one job for that reason: a clock nobody reads is not a
    promise.

    A run in which more than `DEGRADED_SILENCE_RATE` of the answers were
    silence was our outage, and grading is skipped for it — a resolver that
    lies to us for an afternoon must not read as a thousand abandoned projects.

    `anchors` narrows the check (not the grading) to those ids, for the suite
    and for an operator looking at one anchor by hand.
    """
    with conn.cursor() as cur:
        cur.execute("INSERT INTO sweep_run DEFAULT VALUES RETURNING id")
        run_id = cur.fetchone()["id"]
        cur.execute(
            "SELECT id, kind, value, host, probe_prefix, challenge_token FROM anchor "
            "WHERE verified_at IS NOT NULL AND taken_down_at IS NULL AND status <> 'unknown' "
            "  AND (last_checked IS NULL OR last_checked < now() - %s * interval '1 day') "
            "  AND (%s::bigint[] IS NULL OR id = ANY(%s::bigint[])) "
            "ORDER BY last_checked NULLS FIRST FOR UPDATE SKIP LOCKED LIMIT %s",
            (ANCHOR_EVERY_DAYS, anchors, anchors, limit))
        due = cur.fetchall()

    attempted = silent = 0
    with fetch.pinned_client() as client:
        for anchor in due:
            try:
                with conn.transaction():
                    if anchor["kind"] == "dns":
                        probed = dns_txt.probe(anchor["host"], anchor["challenge_token"])
                    else:
                        probed = challenge.probe(forge.fetch_root(anchor),
                                                 anchor["challenge_token"], client=client)
                    sweep.record(conn, anchor["id"], probed, run_id=run_id)
            except Exception:  # noqa: BLE001 - one anchor, not the run
                probed = None
            attempted += 1
            silent += probed is None or probed.is_silence

    degraded = attempted > 0 and silent / attempted > sweep.DEGRADED_SILENCE_RATE
    with conn.cursor() as cur:
        cur.execute("UPDATE sweep_run SET finished_at = now(), attempted = %s, silent = %s, "
                    "degraded = %s WHERE id = %s", (attempted, silent, degraded, run_id))
    before = log_store.tree_size(conn)
    moved = [] if degraded else sweep.apply_grades(conn, now)
    if log_store.tree_size(conn) != before:
        sth.issue(conn)
    return {"checked": attempted, "silent": silent, "degraded": degraded, "moved": moved}


def main() -> None:  # pragma: no cover - the worker loop
    import time

    from .. import db

    # It said "one cycle every 15 minutes" and slept sixty seconds. A cycle a
    # minute is right — each source is fetched only when it is due — and the
    # first line in the journal should say what the process actually does.
    print(f"ingest worker: a cycle every minute; sources used in the last {HOT_DAYS} "
          f"days checked daily, every anchor weekly", flush=True)
    from .. import counting

    while True:
        # Last month's salt goes before anything else is fetched. Its own
        # transaction, so a failing source cannot keep a salt alive.
        try:
            with db.tx() as conn:
                rolled = counting.roll_past(conn)
            if rolled:
                print(f"  epochs closed, salts destroyed: {', '.join(map(str, rolled))}", flush=True)
        except Exception as e:  # noqa: BLE001
            print(f"  rolling past epochs failed, retrying next tick: {type(e).__name__}: {e}", flush=True)
        try:
            with db.tx() as conn:
                results = run_once(conn)
        except Exception as e:  # noqa: BLE001
            # A cycle that fails is a cycle. Dying here is how one bad source
            # became an outage for everybody, and a crash loop retries the same
            # thing at the speed of a restart.
            print(f"  cycle failed, retrying next tick: {type(e).__name__}: {e}", flush=True)
            time.sleep(60)
            continue
        if results:
            print(f"  {len(results)} sources: " +
                  ", ".join(f"{r['source']}={r['outcome']}" for r in results), flush=True)
        try:
            with db.tx() as conn:
                swept = sweep_anchors(conn)
            if swept["checked"] or swept["moved"]:
                print(f"  anchors: {swept['checked']} checked, {swept['silent']} silent"
                      + (", degraded, not graded" if swept["degraded"] else "")
                      + "".join(f", {a} {f}->{t}" for a, f, t in swept["moved"]), flush=True)
        except Exception as e:  # noqa: BLE001
            print(f"  anchor check failed, retrying next tick: {type(e).__name__}: {e}", flush=True)
        time.sleep(60)


if __name__ == "__main__":  # pragma: no cover
    main()
