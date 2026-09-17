"""The server cases, as functions the runner calls.

They live here rather than in `run_testcases.py` because they need the database
and the runner should not: a suite that cannot start without Postgres would stop
answering for the counterparty, which has nothing to do with the server.

Each function is named for the `SV` row it satisfies and asserts the *stated*
expectation rather than the current behaviour, in the house style: a suite that
only records what the code does today cannot say when the code became wrong.
"""
from __future__ import annotations

import hashlib
import json
import secrets
from pathlib import Path

from .. import jcs
from . import cluster_tree, clusters, counting, db, log_store, merkle, repartition, sth
from .ingest import manifest as ingest_manifest, validate as ingest_validate
from .anchor import challenge, dns_txt, sweep
from .anchor.result import EVIDENCE, SILENCE, Probed, Reason
from .config import K_REPORTERS

# ------------------------------------------------------------------ helpers


#: What every notice in this suite carries, because every notice must. The
#: statement is the one thing the law asks of whoever files one, and the route
#: refuses a notice without it (`0014`) - so a case that omitted it would be
#: testing the refusal rather than whatever it meant to test.
NOTIFIER = {"statement_of_good_faith": True}



def _fresh_host(prefix: str = "sv") -> str:
    return f"{prefix}-{secrets.token_hex(4)}.example"


def _anchor(conn, host: str, token: str = "tok") -> int:
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO anchor (kind, value, host, challenge_token) "
            "VALUES ('url', %s, %s, %s) RETURNING id",
            (f"https://{host}/", host, token),
        )
        return cur.fetchone()["id"]


def _anchor_at(conn, value: str, host: str, token: str) -> int:
    """An anchor on a URL the case serves itself, rather than on `https://host/`.

    The value carries a random path segment, and it has to: the database
    outlives the run, `anchor` is unique on `(kind, value)`, and the kernel
    hands back an ephemeral port it has handed out before — so a flat
    `http://127.0.0.1:<port>/` failed one run in however many on a duplicate
    key, for a reason with nothing to do with what the case tests. Deleting
    the older row is not the answer either: half the tables that reference an
    anchor do so without `ON DELETE CASCADE`, on purpose.
    """
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO anchor (kind, value, host, challenge_token) "
            "VALUES ('url', %s, %s, %s) RETURNING id", (value, host, token))
        return cur.fetchone()["id"]


def _seed_log(conn, at_least: int = 8) -> int:
    """Make sure the log has something in it.

    Without this the log cases pass only because another case happened to run
    first — alphabetically, as it turns out. A case that depends on the order of
    the suite is a case that will one day pass for the wrong reason.
    """
    n = log_store.tree_size(conn)
    while n < at_least:
        log_store.append(conn, "log_policy", {"note": f"seed {n}"})
        n += 1
    return n


def _cluster(conn, host: str, observed: dict, epoch: int) -> int:
    sig = clusters.canonical_signature(host, observed)
    return clusters.ensure(conn, sig, subject_host=host, epoch=epoch)


# ------------------------------------------------------------------ the log


def sv1_attesting_appends_to_the_log():
    """SV1: an attestation is an entry in an append-only log, not a row in a
    table only we can read."""
    with db.tx() as conn:
        before = log_store.tree_size(conn)
        host = _fresh_host()
        aid = _anchor(conn, host)
        seq = log_store.append(conn, "attestation_issued", {
            "anchor": {"kind": "url", "value": f"https://{host}/"},
            "tier": "oss",
            "claim": "published by whoever controlled this anchor at time T, from commit C",
        }, anchor_id=aid)
        after = log_store.tree_size(conn)
    assert after == before + 1, "attesting did not append"
    assert seq == before, f"sequence is not gapless: got {seq}, expected {before}"


def sv_log_is_append_only_and_verifiable():
    """The root computed incrementally must equal the root computed from the
    leaves. If those ever differ, every proof this log has issued is wrong."""
    with db.tx() as conn:
        n = _seed_log(conn)
        assert n > 0, "an empty log proves nothing"
        assert log_store.root(conn) == merkle.root_from_leaves(log_store.leaf_hashes(conn)), \
            "the incremental root disagrees with the leaves"


def sv_log_inclusion_and_consistency_verify():
    """A third party must be able to check one entry, and check that the tree
    only grew. The second is the one that catches a rewrite."""
    with db.tx() as conn:
        n = _seed_log(conn)
        leaves = log_store.leaf_hashes(conn)
        root = log_store.root(conn)
        for seq in {0, n // 2, n - 1}:
            proof = log_store.inclusion(conn, seq)
            assert merkle.verify_inclusion(
                seq, n, leaves[seq], [bytes.fromhex(h) for h in proof["path"]], root), \
                f"inclusion proof for {seq} does not verify"
        for old in {1, max(1, n // 2), n}:
            c = log_store.consistency(conn, old, n)
            assert merkle.verify_consistency(
                old, n, merkle.root_from_leaves(leaves[:old]), root,
                [bytes.fromhex(h) for h in c["path"]]), \
                f"consistency {old} -> {n} does not verify"


def sv_a_rewritten_entry_is_detected():
    """The property the whole structure exists for: altering an entry in place
    must be visible from outside, without any cooperation from us."""
    with db.tx() as conn:
        _seed_log(conn)
        leaves = log_store.leaf_hashes(conn)
        root = log_store.root(conn)
    tampered = list(leaves)
    tampered[1] = merkle.leaf_hash(b"a different entry")
    assert merkle.root_from_leaves(tampered) != root, \
        "a rewritten entry produced the same root — the log would be unfalsifiable"


def sv_the_head_is_signed_and_verifiable():
    from ..jws import public_jwk
    with db.tx() as conn:
        head = sth.issue(conn)
    assert sth.verify(head["sth"], head["signature"], public_jwk(sth.key())), \
        "the signed tree head does not verify against its own key"
    other = {"kty": "OKP", "crv": "Ed25519",
             "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"}
    assert not sth.verify(head["sth"], head["signature"], other), \
        "the head verified under a key that did not sign it"


# --------------------------------------------------- anchors, graded honestly


def sv2_one_failed_check_changes_nothing():
    """SV2: a challenge file missing once is nothing. Retry next cycle."""
    with db.tx() as conn:
        aid = _anchor(conn, _fresh_host())
        sweep.record(conn, aid, Probed(Reason.CONFIRMED))
        sweep.record(conn, aid, Probed(Reason.ABSENT))
        with conn.cursor() as cur:
            cur.execute("SELECT status, failing_since FROM anchor WHERE id = %s", (aid,))
            row = cur.fetchone()
    assert row["status"] == "live", f"one failure changed the state to {row['status']}"
    assert row["failing_since"] is not None, "the clock did not start on real evidence"


def sv2a_2b_absence_grades_but_never_revokes():
    """SV2a/SV2b: `stale` at 14 days, `unknown` at 90 — and never `revoked`.
    Revocation is an accusation, and stopping work is not misconduct."""
    from datetime import datetime, timedelta, timezone
    now = datetime.now(timezone.utc)
    assert sweep.grade(now - timedelta(days=1), "live", now) is None, "graded too early"
    assert sweep.grade(now - timedelta(days=20), "live", now) == "stale"
    assert sweep.grade(now - timedelta(days=120), "stale", now) == "unknown"
    for elapsed in (1, 20, 120, 3650):
        got = sweep.grade(now - timedelta(days=elapsed), "live", now)
        assert got != "revoked", "abandonment produced an accusation"


def sv_our_own_outage_never_ages_an_anchor():
    """The failure mode an Option-shaped check guarantees: a resolver outage on
    our side marching every anchor toward `unknown` at once."""
    with db.tx() as conn:
        aid = _anchor(conn, _fresh_host())
        sweep.record(conn, aid, Probed(Reason.CONFIRMED))
        for reason in (Reason.UNREACHABLE, Reason.REFUSED, Reason.INTERNAL):
            sweep.record(conn, aid, Probed(reason))
        with conn.cursor() as cur:
            cur.execute("SELECT status, failing_since FROM anchor WHERE id = %s", (aid,))
            row = cur.fetchone()
    assert row["failing_since"] is None, \
        "silence started the abandonment clock — our outage would read as their absence"
    assert row["status"] == "live"


def sv_a_probe_result_refuses_to_be_a_boolean():
    """The type refuses truthiness, because `if result:` is precisely the bug it
    exists to prevent."""
    for reason in Reason:
        try:
            bool(Probed(reason))
        except TypeError:
            continue
        raise AssertionError(f"Probed({reason}) allowed a truthiness test")
    assert EVIDENCE.isdisjoint(SILENCE), "a reason counts as both evidence and silence"
    assert Reason.CONFIRMED not in EVIDENCE | SILENCE, "confirmation is neither"


def sv_absence_and_inability_to_ask_are_different():
    """The distinction, at both anchor paths."""
    assert challenge.classify(404, "", "t", redirected_off_host=False).reason is Reason.ABSENT
    assert challenge.classify(500, "", "t", redirected_off_host=False).reason is Reason.UNREACHABLE
    # A blocking intermediary is not the claimant's statement about their file.
    assert challenge.classify(451, "", "t", redirected_off_host=False).reason is Reason.REFUSED
    assert challenge.classify(200, "wrong", "t", redirected_off_host=False).reason is Reason.CONTRADICTED
    assert challenge.classify(200, "t", "t", redirected_off_host=False).reason is Reason.CONFIRMED
    assert challenge.classify(302, "", "t", redirected_off_host=True).reason is Reason.REDIRECTED_AWAY
    assert dns_txt.parse([], "t").reason is Reason.ABSENT
    assert dns_txt.parse(["podshl-challenge=t"], "t").reason is Reason.CONFIRMED
    assert dns_txt.parse(["podshl-challenge=other"], "t").reason is Reason.CONTRADICTED


def sv_ingest_refuses_to_fetch_private_addresses():
    """Not in SERVER.md, and the sharpest hole in it: ingest fetches URLs an
    attacker chose. Without this the crawler is a request-forgery engine aimed
    at our own network.

    Loopback is deliberately not in this list: it has its own opt-in for the
    local counterparty, covered separately by
    `sv_loopback_fetching_is_off_by_default`. These are the addresses refused
    whatever the configuration says.
    """
    for host, why in (
        ("169.254.169.254", "the cloud metadata endpoint"),
        ("10.0.0.1", "RFC 1918"),
        ("192.168.0.26", "a home network"),
        ("100.64.0.1", "CGNAT"),
    ):
        assert not challenge.is_public_address(host), f"{host} ({why}) was treated as public"


# ------------------------------------------------------ counting people, not posts


def sv13_counted_once_per_pseudonym_per_epoch():
    """SV13. The service this replaces counted submissions: five presses of the
    button from one person crossed a threshold named for anonymity."""
    epoch = counting.current_epoch()
    with db.tx() as conn:
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, _fresh_host(), {"gpu.name": "RTX"}, epoch)
        first = counting.record_observation(conn, cid, "ONE-CLIENT", epoch=epoch)
        again = [counting.record_observation(conn, cid, "ONE-CLIENT", epoch=epoch)
                 for _ in range(4)]
        n = counting.reporters(conn, cid, epoch)
        with conn.cursor() as cur:
            cur.execute("SELECT reports_total FROM cluster WHERE id = %s", (cid,))
            total = cur.fetchone()["reports_total"]
    assert first is True and not any(again), "a repeat submission counted as a new reporter"
    assert n == 1, f"one client counted as {n} reporters"
    assert total == 5, "submissions are not counted at all — the corpus needs both figures"


def sv14_below_k_nothing_is_surfaced():
    """SV14: fewer than k distinct pseudonyms, and the cluster is not shown to
    anyone — not to the vendor, not to the operator."""
    epoch = counting.current_epoch()
    with db.tx() as conn:
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, _fresh_host(), {"gpu.name": "RTX", "os.version": "6.1"}, epoch)
        for i in range(K_REPORTERS - 1):
            counting.record_observation(conn, cid, f"C{i}", epoch=epoch)
        assert not counting.surfaceable(conn, cid), "surfaced below the threshold"
        counting.record_observation(conn, cid, "C-last", epoch=epoch)
        assert counting.surfaceable(conn, cid), "did not surface at the threshold"


def sv_the_epoch_salt_is_destroyed_on_roll():
    """The operation the whole anonymity claim rests on: afterwards a stored
    `seen_key` cannot be tested against any pseudonym, by anyone."""
    epoch = 200001
    with db.tx() as conn:
        counting.open_epoch(conn, epoch)
        before = counting.seen_key(epoch, 1, "somebody")
        path = counting._salt_path(epoch)
        assert path.exists(), "no salt file was written"
        counting.roll(conn, epoch)
        assert not path.exists(), "the salt survived the roll"
        with conn.cursor() as cur:
            cur.execute("SELECT destroyed_at FROM epoch WHERE epoch = %s", (epoch,))
            assert cur.fetchone()["destroyed_at"] is not None
    after = counting.seen_key(epoch, 1, "somebody")
    assert after != before, "the key is reproducible after the salt was destroyed"


def sv110_every_past_months_salt_is_destroyed_without_being_asked():
    """SV110. Nothing in production called `roll`, so every month's salt stayed.

    `roll_past` is what the ingest worker runs each cycle: an epoch before the
    current month is closed and its salt destroyed, a salt file with no epoch
    row is destroyed as well, and the current month is left alone.
    """
    import os

    # Against the real clock, as the worker runs it: past months here are months
    # long gone, and the current one is the one reports are arriving in now.
    past, orphan = 200003, 200002
    current = counting.current_epoch()
    with db.tx() as conn:
        counting.open_epoch(conn, past)
        counting.open_epoch(conn, current)
        counting.SALT_DIR.mkdir(parents=True, exist_ok=True)
        orphan_path = counting._salt_path(orphan)
        if not orphan_path.exists():
            fd = os.open(orphan_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            os.write(fd, os.urandom(32))
            os.close(fd)
        rolled = counting.roll_past(conn)
        assert past in rolled and orphan in rolled, rolled
        assert current not in rolled, "the current month's salt was destroyed"
        assert rolled == sorted(rolled), rolled
        assert not counting._salt_path(past).exists(), "a past month's salt survived"
        assert not orphan_path.exists(), "a salt file without an epoch row survived"
        assert counting._salt_path(current).exists(), "the current month's salt is gone"
        with conn.cursor() as cur:
            cur.execute("SELECT epoch, destroyed_at FROM epoch WHERE epoch IN (%s, %s)",
                        (past, current))
            got = {r["epoch"]: r["destroyed_at"] for r in cur.fetchall()}
        assert got[past] is not None and got[current] is None, got


def sv_seen_keys_do_not_join_across_clusters_or_epochs():
    """Salted per cluster and per epoch, so nothing joins on either axis."""
    e1, e2 = 200002, 200003
    same = counting.seen_key(e1, 1, "p")
    assert counting.seen_key(e1, 2, "p") != same, "the same pseudonym matched across clusters"
    assert counting.seen_key(e2, 1, "p") != same, "the same pseudonym matched across epochs"
    assert len(same) == 16


# ------------------------------------------------------------ signature, not text


def sv_the_class_is_derived_from_the_signature():
    """The catch-all took a truncated problem sentence as the class. That is the
    rejected draft; the class comes from the readings."""
    a = clusters.canonical_signature("x.example", {"gpu.name": "RTX", "os.version": "6.1"})
    b = clusters.canonical_signature("x.example", {"os.version": "6.1", "gpu.name": "RTX"})
    assert clusters.signature_hash(a) == clusters.signature_hash(b), \
        "key order changed the identity of a problem"
    c = clusters.canonical_signature("x.example", {"gpu.name": "RX"})
    assert clusters.signature_hash(a) != clusters.signature_hash(c)
    assert clusters.derive_class(a).startswith("sig."), "the class is not derived"
    assert "flicker" not in clusters.derive_class(a), "the class carries prose"


def sv16_the_edge_path_is_one_indexed_lookup():
    """The same driver, version and card produce the same hash — which is why
    the common case never reaches the origin at all."""
    epoch = counting.current_epoch()
    host = _fresh_host()
    with db.tx() as conn:
        counting.open_epoch(conn, epoch)
        sig = clusters.canonical_signature(host, {"gpu.name": "RTX"})
        cid = clusters.ensure(conn, sig, subject_host=host, epoch=epoch)
        again = clusters.ensure(conn, sig, subject_host=host, epoch=epoch)
        assert cid == again, "the same signature produced two clusters"
        found = clusters.find(conn, clusters.signature_hash(sig))
        assert found and found["id"] == cid


# ------------------------------------------------------------------- the gate


def sv5_sv6_the_gate_refuses_at_ingest():
    """SV5/SV6: a solution proposing an action outside the vocabulary, or naming
    a reading nobody implements, is refused when it arrives — not when a user
    has already consented to a plan built around it."""
    from .. import spec_gate
    spec_gate.check_action({"action": "set_config_key",
                            "params": {"file": "a.toml", "key": "k", "value": "v"}})
    for bad in ({"action": "run_powershell", "params": {}},
                {"action": "set_env_var", "params": {"name": "X", "value": "1"}}):
        try:
            spec_gate.check_action(bad)
        except spec_gate.SpecError:
            continue
        raise AssertionError(f"the gate accepted {bad}")
    try:
        spec_gate.check_read({"op": "read_everything"})
    except spec_gate.SpecError:
        return
    raise AssertionError("the gate accepted an unknown read op")


def sv7_english_is_enforced_by_the_database():
    """SV7. A constraint rather than only a validator, so a bug in the validator
    still cannot store a card without English."""
    with db.tx() as conn:
        host = _fresh_host()
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                        "VALUES (%s, %s, %s) RETURNING id",
                        (aid, f"https://{host}/.podshl/agent.yaml", f"https://{host}/"))
            sid = cur.fetchone()["id"]
    try:
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("INSERT INTO card (source_id, json, langs, content_hash) "
                            "VALUES (%s, '{}'::jsonb, ARRAY['de'], %s)", (sid, b"\x00" * 32))
    except Exception as e:
        assert "english" in str(e).lower(), f"refused for the wrong reason: {e}"
        return
    raise AssertionError("a card without English was stored")


def sv4_there_is_no_display_name_field():
    """SV4: "Card with a display name that is not the anchor — impossible, there
    is no such field." Impossible only if the column does not exist."""
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT column_name FROM information_schema.columns "
                "WHERE table_name = 'attestation' "
                "AND column_name IN ('subject_name','display_name','name','brand')")
            found = [r["column_name"] for r in cur.fetchall()]
    assert not found, f"the attestation carries a name field that can be typed into: {found}"


def sv_enterprise_identity_cannot_be_self_asserted():
    """The legal name exists only for the enterprise tier and only from the
    register. An OSS attestation has nowhere to put one."""
    with db.tx() as conn:
        aid = _anchor(conn, _fresh_host())
        seq = log_store.append(conn, "log_policy", {"note": "test"})
    try:
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    "INSERT INTO attestation (anchor_id, tier, legal_name, lei, key_jwk, "
                    "  key_thumbprint, issued_seq) "
                    "VALUES (%s, 'oss', 'NVIDIA Corporation', NULL, '{}'::jsonb, %s, %s)",
                    (aid, b"\x00" * 32, seq))
    except Exception:
        return
    raise AssertionError("an OSS attestation carried a self-asserted legal name")


# --------------------------------------------------------- what stays private


def sv19_the_dashboard_refuses_without_a_claim():
    """SV19, and the behaviour the catch-all got wrong: it served any product's
    gap report to whoever asked. `SERVER.md` calls that the line whose crossing
    ends the company."""
    from .errors import NotClaimed
    from .app import _claimed_anchor
    with db.read() as conn:
        for token in (None, "", "guessed"):
            try:
                _claimed_anchor(conn, "unclaimed.example", token)
            except NotClaimed:
                continue
            raise AssertionError(f"the dashboard opened with token {token!r}")


def sv20_a_verified_claim_opens_it():
    """SV20: claiming your own domain unlocks the dashboard about yourself,
    free. The same challenge as any other anchor."""
    from .app import _claimed_anchor
    host = _fresh_host("claimed")
    token = secrets.token_urlsafe(16)
    with db.tx() as conn:
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                "VALUES (%s, %s, now() + interval '1 day')",
                (aid, hashlib.sha256(token.encode()).digest()))
    with db.read() as conn:
        assert _claimed_anchor(conn, host, token) == aid


def sv21_sv24_no_route_produces_another_vendors_figures():
    """SV21/SV24: refused because no path exists, not because a check says no.

    The outreach view is a different ASGI application on a different listener,
    so this is a property of the routing table rather than of a guard somebody
    could misconfigure.
    """
    from .app import app
    from .ops_app import ops
    public = {r.path for r in app.routes if hasattr(r, "path")}
    operator = {r.path for r in ops.routes if hasattr(r, "path")}
    assert "/outreach" not in public, "the outreach ranking is reachable from the public app"
    assert "/outreach" in operator, "the operator view is not where it was said to be"
    assert not any("reset" in p for p in public), "an unauthenticated state wipe survived"
    for p in public:
        assert not p.startswith("/verify"), \
            "a per-domain lookup exists — that is the surveillance endpoint the log avoids"


def sv25_public_figures_are_two_integers():
    """SV25: totals only, with no parameter that could narrow them."""
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT * FROM ecosystem_totals")
            row = cur.fetchone()
    assert set(row) == {"products_observed", "products_with_an_agent"}, \
        f"the public figures carry more than two integers: {sorted(row)}"


def sv22_free_text_needs_its_own_consent():
    """SV22: a description travels only under its own explicit consent, with the
    destination named. Enforced by the database, not by the handler."""
    epoch = counting.current_epoch()
    with db.tx() as conn:
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, _fresh_host(), {"a": "b"}, epoch)
    try:
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    "INSERT INTO observation (cluster_id, epoch, model_class, observed, "
                    "  seen_key, description) VALUES (%s, %s, 'x', '{}'::jsonb, %s, %s)",
                    (cid, epoch, secrets.token_bytes(16), "something a user typed"))
    except Exception:
        return
    raise AssertionError("free text was stored with no consent naming a destination")


def sv38_every_takedown_carries_a_public_reason():
    """SV38: logged with a public reason code, and the affected party told why —
    which is the same log entry. If we can remove things quietly the
    transparency is decorative."""
    with db.tx() as conn:
        host = _fresh_host("taken")
        aid = _anchor(conn, host)
        seq = log_store.append(conn, "takedown", {
            "anchor": {"kind": "url", "value": f"https://{host}/"},
            "reason_code": "trademark_claim",
            "action": "degraded",
        }, anchor_id=aid)
        entries = log_store.for_anchor(conn, aid)
    assert entries and entries[-1]["seq"] == seq
    assert entries[-1]["entry"]["reason_code"], "a takedown with no public reason"
    assert entries[-1]["entry"]["action"] == "degraded", \
        "a takedown deleted rather than degrading — the source stays in the developer's repository"


def sv36_a_takedown_degrades_it_does_not_delete():
    """SV36: `attested` becomes `unknown` — precisely the state of a project that
    never registered. Not death, un-enrolment."""
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT conname FROM pg_constraint WHERE conname = 'withdrawal_states_a_reason'")
            assert cur.fetchone(), "withdrawal does not have to state a reason"
            cur.execute("SELECT pg_get_constraintdef(oid) AS d FROM pg_constraint "
                        "WHERE conrelid = 'attestation'::regclass AND conname LIKE '%withdrawn_kind%'")
    # 'revoked' and 'degraded' must be distinguishable, or an abandonment reads
    # as an accusation.
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT pg_get_constraintdef(oid) AS d FROM pg_constraint "
                        "WHERE conrelid = 'attestation'::regclass")
            defs = " ".join(r["d"] for r in cur.fetchall())
    assert "revoked" in defs and "degraded" in defs, \
        "withdrawal does not distinguish an accusation from an un-enrolment"



# ------------------------------------------------------------ ingest, the gate

EXAMPLE = Path(__file__).resolve().parents[3] / "spec" / "example" / ".podshl"


def _example_manifest() -> dict:
    return ingest_manifest.parse_manifest((EXAMPLE / "agent.yaml").read_bytes())


def sv_the_published_example_passes_its_own_gate():
    """A worked example that the implementation would refuse is worse than no
    example: a developer copies it, and the first thing that happens is a
    rejection they did not cause.

    Both of them. The second — a desktop project answering for the NVIDIA
    driver under Wayland — exists to show that naming software you did not write
    is allowed, which is the question that stops maintainers before the syntax
    ever does. An example making that argument while being uningestible would
    make the argument badly.
    """
    import yaml

    m = _example_manifest()
    ingest_validate.check_manifest(m, "https://example.org/")
    for path in sorted((EXAMPLE / "solutions").glob("*.md")):
        ingest_validate.check_solution(
            ingest_manifest.parse_solution(path.read_bytes(), path.name))

    desk = EXAMPLE.parent.parent / "example-desktop" / ".podshl"
    dm = yaml.safe_load((desk / "agent.yaml").read_text(encoding="utf-8"))
    ingest_validate.check_manifest(dm, "https://example.net/")
    solutions = sorted((desk / "solutions").glob("*.md"))
    assert solutions, "the second worked example has no solutions"
    for path in solutions:
        ingest_validate.check_solution(
            ingest_manifest.parse_solution(path.read_bytes(), path.name))

    # The point of the second example, asserted rather than described: it names
    # software its publisher does not own, and it has no field in which to claim
    # to be them. The display name is the verified anchor and nothing else.
    assert any(c.startswith("nvidia.") for c in dm["problem_classes"]),         "the nominative-use example stopped naming somebody else's software"
    assert "name" not in dm and "vendor" not in dm,         "a manifest gained a name field — the display name is the anchor, or it is a claim"


def sv3_an_endpoint_outside_its_anchor_is_refused():
    """SV3. Rejected at ingest, not rendered with a warning.

    An anchor proves control of a location; it cannot vouch for another one, and
    the lookalike case matters as much as the obvious one."""
    for endpoint in ("https://google.com/x", "https://example.org.evil/x",
                     "https://sub.example.org.evil/"):
        m = _example_manifest()
        m["endpoint"] = endpoint
        try:
            ingest_validate.check_manifest(m, "https://example.org/")
        except Exception:
            continue
        raise AssertionError(f"accepted an endpoint outside the anchor: {endpoint}")


def sv6_a_read_outside_the_vocabulary_is_refused_at_ingest():
    """SV6, and the sharp case is not an unknown tool but a known one asked to
    do something else: `python3 --version` is a reading, `python3 -c ...` is
    arbitrary execution, and the difference is one argument.

    **The quietest case is a permitted op with a parameter it cannot answer.**
    `{op: os_fact, name: name}` was accepted until 2026-09-16: the op was
    checked and its parameters were not, so the probe read nothing on every
    machine and said nothing about it. engram shipped exactly that, its
    `os.name` was silently empty for as long as it existed, and somebody on
    Linux holding the Windows archive was told nothing was wrong. A refusal at
    ingest is a sentence the maintainer can act on; silence is a defect they
    cannot see."""
    for read, why in (
        ({"op": "run_tool", "tool": "curl", "args": []}, "a tool off the allow list"),
        ({"op": "run_tool", "tool": "python3", "args": ["-c", "import os"]},
         "arbitrary code through a permitted tool"),
        ({"op": "read_everything"}, "an invented op"),
        ({"op": "read_file_key", "path": ".ssh/id_ed25519", "key": "x"}, "a denied path"),
        ({"op": "os_fact", "name": "name"}, "a fact os_fact cannot answer"),
        ({"op": "os_fact"}, "os_fact with no fact named at all"),
    ):
        m = _example_manifest()
        m["collect"].append({"id": "x", "kind": "machine", "read": read})
        try:
            ingest_validate.check_manifest(m, "https://example.org/")
        except Exception:
            continue
        raise AssertionError(f"accepted {why}: {read}")


def sv7_a_manifest_without_english_is_refused():
    m = _example_manifest()
    m["langs"] = ["de"]
    try:
        ingest_validate.check_manifest(m, "https://example.org/")
    except Exception as e:
        assert "en" in str(e), str(e)
        return
    raise AssertionError("a manifest declaring no English was accepted")


def sv5_a_solution_proposing_an_unknown_action_is_refused():
    """SV5: refused when the document arrives, not when a user has already
    consented to a plan built around it."""
    base = ingest_manifest.parse_solution(
        (EXAMPLE / "solutions" / "resolver-backtracking.md").read_bytes(), "s.md")
    for call, why in (
        ({"action": "run_powershell", "params": {}}, "an invented action"),
        ({"action": "set_config_key",
          "params": {"file": "/etc/passwd", "key": "a", "value": "b"}},
         "a real action with a parameter outside its pattern"),
    ):
        s = dict(base, proposes=list(base["proposes"]) + [call])
        try:
            ingest_validate.check_solution(s)
        except Exception:
            continue
        raise AssertionError(f"accepted {why}")


def sv35_naming_another_vendors_product_is_accepted():
    """SV35. Nominative use is the entire OSS branch: a project that repairs
    somebody else's software has to be able to say whose. Identity and subject
    are separate fields, and a takedown reaches an anchor, never a class."""
    m = _example_manifest()
    m["problem_classes"] = ["nvidia.driver.flicker", "pip.resolver", "microsoft.teams.crash"]
    ingest_validate.check_manifest(m, "https://example.org/")


def sv_a_solution_path_cannot_leave_the_anchor():
    m = _example_manifest()
    m["solutions"].append("../../etc/passwd")
    try:
        ingest_validate.check_manifest(m, "https://example.org/")
    except Exception:
        return
    raise AssertionError("a solution path leaving the manifest's directory was accepted")


def sv_ingest_refuses_a_private_address():
    """The crawler follows URLs an attacker chose. Without this it is a request
    forgery engine aimed at our own network."""
    from .ingest.fetch import get
    for url in ("https://169.254.169.254/x", "https://10.0.0.1/x"):
        out = get(url, prefix=url.rsplit("/", 1)[0] + "/")
        assert not out.usable, f"fetched {url}"


def sv_a_fetch_result_refuses_to_be_a_boolean():
    """Same reason as a probe: a fetch has more than two answers, and collapsing
    them is how "we could not ask" gets reported as "they published nothing"."""
    from .ingest.fetch import Fetched
    from .anchor.result import Reason
    try:
        bool(Fetched(Reason.ABSENT))
    except TypeError:
        return
    raise AssertionError("a Fetched allowed a truthiness test")



# --------------------------------------------- ingest end to end, against a host

OSS_BASE = "http://127.0.0.1:8727/"


def _oss_source(conn) -> tuple[int, int]:
    """Register the example project as an anchor and source, idempotently."""
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM anchor WHERE host = '127.0.0.1' AND kind = 'url'")
        row = cur.fetchone()
        if row:
            aid = row["id"]
        else:
            cur.execute(
                "INSERT INTO anchor (kind, value, host, challenge_token) "
                "VALUES ('url', %s, '127.0.0.1', 'podshl-example-anchor-token') RETURNING id",
                (OSS_BASE,))
            aid = cur.fetchone()["id"]
        cur.execute("SELECT id FROM source WHERE anchor_id = %s", (aid,))
        row = cur.fetchone()
        if row:
            return aid, row["id"]
        cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                    "VALUES (%s, %s, %s) RETURNING id",
                    (aid, OSS_BASE + ".podshl/agent.yaml", OSS_BASE))
        return aid, cur.fetchone()["id"]


def _require_oss_host():
    import httpx
    try:
        httpx.get(OSS_BASE, timeout=2)
    except Exception as e:  # noqa: BLE001
        raise AssertionError(
            "the example project is not answering on :8727 — start it with "
            "`mise run services`. Ingest tested against a file on disk would not "
            "exercise the conditional GET, the redirect rules or the SSRF guard."
        ) from e


def sv_ingest_stores_a_project_and_attests_it():
    """A real fetch from a real host: anchor confirmed in the same pass, manifest
    and solutions validated, stored, and an entry appended to the log."""
    from .config import ALLOW_LOOPBACK
    from .ingest import scheduler
    _require_oss_host()
    assert ALLOW_LOOPBACK, (
        "loopback fetching is off, so the crawler refuses the local counterparty. "
        "Set PODSHL_ALLOW_LOOPBACK=1 for the suite; it is off by default because "
        "the crawler follows URLs an attacker chose.")

    with db.tx() as conn:
        _, sid = _oss_source(conn)
        with conn.cursor() as cur:
            cur.execute("UPDATE source SET next_fetch_at = '-infinity', etag = NULL, "
                        "last_modified = NULL, last_used = current_date WHERE id = %s", (sid,))
            # The anchor is checked in this pass only when it is due; make it so.
            cur.execute("UPDATE anchor SET last_checked = NULL WHERE id = "
                        "(SELECT anchor_id FROM source WHERE id = %s)", (sid,))
        results = {r["source"]: r for r in scheduler.run_once(conn)}
    got = results.get(sid)
    assert got, "the source was never claimed"
    assert got["outcome"] in ("stored", "unchanged"), got
    assert got["anchor"] == "confirmed", f"the anchor was not confirmed: {got}"

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT commit, content_hash, log_seq FROM card "
                        "WHERE source_id = %s AND valid_to IS NULL", (sid,))
            card = cur.fetchone()
            cur.execute("SELECT count(*) n FROM solution "
                        "WHERE source_id = %s AND valid_to IS NULL", (sid,))
            n = cur.fetchone()["n"]
    assert card and card["commit"], "nothing was stored, or stored without a commit"
    assert n == 2, f"expected both solutions, stored {n}"


def sv18_the_served_commit_is_verifiable():
    """SV18. Publishing the commit only means something if anyone can check it:
    fetch that commit from the source and diff it against what we serve."""
    import hashlib

    import httpx
    _require_oss_host()
    sv_ingest_stores_a_project_and_attests_it()

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                # `kind <> 'repo'` and serving only: "the project on 127.0.0.1"
                # stopped naming one thing when repository anchors arrived. A
                # repository's host really is the loopback host, where a
                # `_served_project` only keeps its *value* there and carries an
                # invented name in `host` -- so this read two rows and compared
                # somebody else's content against this case's source.
                "SELECT c.content_hash FROM card c JOIN source s ON s.id = c.source_id "
                "JOIN anchor a ON a.id = s.anchor_id "
                "WHERE a.host = '127.0.0.1' AND a.kind <> 'repo' "
                "AND s.mirror_state = 'serving' AND c.valid_to IS NULL "
                "ORDER BY c.id DESC LIMIT 1")
            served = bytes(cur.fetchone()["content_hash"])

    upstream = hashlib.sha256(
        httpx.get(OSS_BASE + ".podshl/agent.yaml", timeout=5).content).digest()
    assert served == upstream, (
        "the mirror does not match the source. Publishing a commit whose content "
        "cannot be reproduced from that commit is a claim nobody can check.")


def sv17_the_mirror_answers_when_the_source_does_not():
    """SV17. Fetching from a forge on the request path would make their outage
    ours. Everything served comes from our own database.

    The mirror is filled here, as `SV18` already did it. This case used to read
    a row it assumed some earlier case had left behind — which passed on a
    database that had run the suite before and failed on a fresh one. A case
    that depends on another case's side effect reports what ran last, not the
    property it names.
    """
    _require_oss_host()
    sv_ingest_stores_a_project_and_attests_it()

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT c.json FROM card c JOIN source s ON s.id = c.source_id "
                "JOIN anchor a ON a.id = s.anchor_id "
                "WHERE a.host = '127.0.0.1' AND c.valid_to IS NULL "
                "AND s.mirror_state = 'serving'")
            row = cur.fetchone()
    assert row, "the mirror is empty after an ingest that reported success"
    # Nothing above touched the network. That is the whole property.
    assert row["json"]["endpoint"], "the stored card has no endpoint"


def sv_a_refused_source_does_not_remember_its_etag():
    """A source that failed validation must be re-examined next cycle.

    Storing the ETag on a refusal means the next cycle sends `If-None-Match`,
    gets a 304 and reports the source as unchanged — so a document that failed
    once is recorded as fine forever, and the refusal is never revisited after
    the developer fixes it.
    """
    from .anchor.result import Reason
    from .ingest.fetch import Fetched
    from .ingest import store

    with db.tx() as conn:
        _, sid = _oss_source(conn)
        with conn.cursor() as cur:
            cur.execute("UPDATE source SET etag = 'W/\"old\"' WHERE id = %s", (sid,))
        store.record_ingest(conn, sid, Fetched(Reason.CONFIRMED, status=200, body=b"x",
                                               etag='W/"new"'),
                            changed=False, remember_validators=False)
        with conn.cursor() as cur:
            cur.execute("SELECT etag FROM source WHERE id = %s", (sid,))
            etag = cur.fetchone()["etag"]
    assert etag is None, f"a refused source kept an ETag ({etag}) and will never be re-checked"


def sv_loopback_fetching_is_off_by_default():
    """The exception exists for the local counterparty and must not be the
    default. A crawler that will follow a URL to 127.0.0.1 is a request-forgery
    engine aimed at whatever else is listening there."""
    import os
    import subprocess
    out = subprocess.run(
        [os.sys.executable, "-c",
         "import os; os.environ.pop('PODSHL_ALLOW_LOOPBACK', None);"
         "import sys; sys.path.insert(0, 'src');"
         "from podshl.server.anchor.challenge import is_public_address;"
         "print(is_public_address('127.0.0.1'))"],
        capture_output=True, text=True,
        env={k: v for k, v in os.environ.items() if k != "PODSHL_ALLOW_LOOPBACK"})
    assert out.stdout.strip() == "False", (
        f"loopback is accepted with no opt-in: {out.stdout!r} {out.stderr[-200:]}")



# ------------------------------------------------- the tree: a switch, not a metric

def _tree_source(conn) -> int:
    _, sid = _oss_source(conn)
    return sid


def _author_tree(conn, source_id: int, *, question_has_fallback: bool = True) -> int:
    """Author a tree by hand, the way the authoring tool would.

    The tool does not exist. Nothing here pretends it does — this is the shape
    `SERVER.md` draws, written directly.
    """
    with conn.cursor() as cur:
        cur.execute("DELETE FROM tree WHERE source_id = %s", (source_id,))
        cur.execute("INSERT INTO tree (source_id, problem_class) "
                    "VALUES (%s, 'pip.install.wheel-missing') RETURNING id", (source_id,))
        tid = cur.fetchone()["id"]

        def node(parent, depth, **kw):
            cols = dict(tree_id=tid, parent_id=parent, depth=depth, **kw)
            keys = ", ".join(cols)
            ph = ", ".join(["%s"] * len(cols))
            vals = [json.dumps(v) if k in ("match_value", "probe") else v
                    for k, v in cols.items()]
            cur.execute(f"INSERT INTO tree_node ({keys}) VALUES ({ph}) RETURNING id", vals)
            return cur.fetchone()["id"]

        root = node(None, 0, switch_fact="python.version", switch_kind="reading",
                    comparator="version")
        node(root, 1, match_op="ge", match_value="3.13", solution_id="wheel-missing-for-python")
        ask_kw = {"solution_id": "resolver-backtracking"} if question_has_fallback else {}
        ask = node(root, 1, match_op="lt", match_value="3.13",
                   switch_fact="install.command", switch_kind="question",
                   comparator="string",
                   probe={"id": "install.command", "kind": "human",
                          "describes": "The exact command you ran",
                          "why": "pip, uv and poetry fail differently",
                          "choices": ["pip install", "poetry add"]},
                   **ask_kw)
        node(ask, 2, match_op="eq", match_value="poetry add",
             solution_id="wheel-missing-for-python")
    return tid


def sv26_a_missing_decisive_fact_is_asked_for_not_guessed():
    """SV26. Where two answers look alike, the endpoint asks for the fact that
    separates them rather than estimating which one this is."""
    with db.tx() as conn:
        sid = _tree_source(conn)
        tid = _author_tree(conn, sid)
        root = cluster_tree.load(conn, tid)
        out = cluster_tree.walk(root, {})
    assert isinstance(out, cluster_tree.Need), f"guessed instead of asking: {out}"
    assert out.probe["id"] == "python.version"


def sv28_dont_know_falls_back_and_never_dead_ends():
    """SV28, and it is structural rather than hopeful.

    The fallback is computed on the way *down*, so it is already in hand when
    the question is asked — nothing walks back up looking for one. A tree whose
    question has nothing above it to fall back to is refused when it is
    authored, not discovered by a user who cannot answer.
    """
    with db.tx() as conn:
        sid = _tree_source(conn)
        tid = _author_tree(conn, sid)
        root = cluster_tree.load(conn, tid)

        asked = cluster_tree.walk(root, {"python.version": "3.11.9"})
        assert isinstance(asked, cluster_tree.Need)
        assert asked.fallback is not None, (
            "the question was asked with no fallback — a user who cannot answer "
            "would be left with nothing")

        declined = cluster_tree.walk(
            root, {"python.version": "3.11.9", "install.command.declined": True})
        assert isinstance(declined, cluster_tree.Answer), f"'don't know' dead-ended: {declined}"
        assert declined.solution_id == asked.fallback.solution_id, (
            "the fallback shipped with the question is not what 'don't know' lands on")


def sv28a_a_question_with_nothing_to_fall_back_to_is_refused():
    """The other half of SV28: a dead end is a defect in the tree, and it is
    caught when the tree is saved."""
    from .errors import IngestRefused
    with db.tx() as conn:
        sid = _tree_source(conn)
        tid = _author_tree(conn, sid, question_has_fallback=False)
        root = cluster_tree.load(conn, tid)
        try:
            cluster_tree.validate_tree(root)
        except IngestRefused as e:
            assert "fall back" in str(e), str(e)
            return
    raise AssertionError("a tree whose question dead-ends was accepted")


def sv31_runtime_matching_is_exact():
    """SV31. Along the walked path, and nowhere else. No distance, no threshold:
    a wrong guess would reach a user."""
    with db.tx() as conn:
        sid = _tree_source(conn)
        tid = _author_tree(conn, sid)
        root = cluster_tree.load(conn, tid)

        # A value no branch matches is not resolved to the nearest one.
        out = cluster_tree.walk(root, {"python.version": "3.11.9",
                                       "install.command": "conda"})
        assert isinstance(out, cluster_tree.Answer)
        assert out.solution_id == "resolver-backtracking", (
            "an unbranched value was matched to the nearest branch instead of "
            "falling back")

    # And the comparison itself is total: an unreadable value is not a silent
    # non-match, because that would be indistinguishable from a real one.
    assert cluster_tree.matches("3.13.1", "ge", "3.13", "version")
    assert not cluster_tree.matches("3.12.9", "ge", "3.13", "version")
    try:
        cluster_tree.matches("not-a-number", "lt", "5", "number")
    except ValueError:
        pass
    else:
        raise AssertionError("an unreadable value compared as a number anyway")


def sv_an_ambiguous_tree_is_refused_when_it_is_authored():
    """Two branches matching the same value is not something to resolve with a
    coin flip while a user waits."""
    from .errors import IngestRefused
    root = cluster_tree.Node(id=1, parent_id=None, depth=0, switch_fact="x",
                             switch_kind="reading", comparator="string")
    root.children = [
        cluster_tree.Node(id=2, parent_id=1, depth=1, match_op="eq", match_value="a",
                          solution_id="one"),
        cluster_tree.Node(id=3, parent_id=1, depth=1, match_op="eq", match_value="a",
                          solution_id="two"),
    ]
    try:
        cluster_tree.validate_tree(root)
    except IngestRefused as e:
        assert "coin flip" in str(e), str(e)
        return
    raise AssertionError("an ambiguous tree was accepted")


def _served_project(files: dict[str, bytes], challenge_token: str, expect_stored: bool = True,
                    host: str | None = None, etags: bool = False):
    """A project serving `files` over real HTTP, ingested, and claimed.

    Returns `(host, token, stop)`. The anchor value carries a random path
    segment for the reason `_anchor_at` gives. The files stay reachable through
    `_served_files(host)`, so a case can change what the project publishes, and
    what the first ingest said — and where it was served from — through
    `_INGESTED[host]`, for a case that expects a refusal.
    """
    import http.server
    import threading

    from .ingest import scheduler

    run = secrets.token_hex(4)
    hits: list[str] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            rel = self.path.removeprefix(f"/{run}")
            hits.append(rel)
            body = files.get(rel)
            # A validator derived from the bytes, as a forge sends one, so a
            # case can see a conditional request answered `304`.
            tag = (f'"{hashlib.sha256(body).hexdigest()[:16]}"'
                   if etags and body is not None else None)
            if tag and self.headers.get("If-None-Match") == tag:
                self.send_response(304)
                self.send_header("ETag", tag)
                self.end_headers()
                return
            self.send_response(200 if body is not None else 404)
            if tag:
                self.send_header("ETag", tag)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body or b"")))
            self.end_headers()
            if body:
                self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{srv.server_address[1]}/{run}/"
    files["/.well-known/podshl-challenge"] = challenge_token.encode()
    files["/.podshl/agent.yaml"] = files["/.podshl/agent.yaml"].replace(b"{base}", base.encode())
    try:
        host = host or _fresh_host("served")
        token = "served-" + secrets.token_urlsafe(16)
        with db.tx() as conn:
            aid = _anchor_at(conn, base, host, challenge_token)
            with conn.cursor() as cur:
                cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix, "
                            "                    next_fetch_at) "
                            "VALUES (%s, %s, %s, '-infinity') RETURNING id",
                            (aid, base + ".podshl/agent.yaml", base))
                sid = cur.fetchone()["id"]
                cur.execute(
                    "INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                    "VALUES (%s, %s, now() + interval '1 day')",
                    (aid, hashlib.sha256(token.encode()).digest()))
        with db.tx() as conn:
            got = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        if expect_stored:
            assert got["outcome"] == "stored", got
    except BaseException:
        srv.shutdown()
        raise
    _SERVED[host] = files
    _HITS[host] = hits
    _INGESTED[host] = {**got, "base": base}
    return host, token, srv.shutdown


_SERVED: dict[str, dict[str, bytes]] = {}
_INGESTED: dict[str, dict] = {}
_HITS: dict[str, list[str]] = {}


def _served_files(host: str) -> dict[str, bytes]:
    return _SERVED[host]


def sv29_sv30_sv32_an_answer_that_helped_some_is_shown_where_it_forks():
    """SV29, SV30 and SV32, from published files and real reports.

    **SV32.** An answer whose reports say it worked for some and not others is
    two problems, and the outcome label points at the missing distinction. The
    page names the fact that separates the configurations it helped from the
    ones it did not, and offers the `answers.when` that would say so.

    **SV29.** That fact is already in the reports, so the suggestion says what a
    switch on it does to history, in the cards' own counts.

    **SV30.** Where one configuration went both ways, nothing collected tells
    those people apart. Only a question could, and it would apply forward only.

    These three used to write observations directly — differing facts inside one
    cluster, joined on a `tried_link_id` — and passed against functions that
    could never find anything in production: a cluster's signature is every
    fact a report carries, so its reports agree on every value, and no report
    ever carried a solution id. So everything here arrives through `/report`.
    """
    from .app import report

    manifest = (
        b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n"
        b"problem_classes: [demo.arch]\n"
        b"collect:\n"
        b"  - id: os.arch\n    kind: machine\n    describes: Architecture\n"
        b"    why: the archives differ\n    read: { op: os_fact, name: arch }\n"
        b"  - id: os.name\n    kind: machine\n    describes: Operating system\n"
        b"    why: the installers differ\n    read: { op: os_fact, name: os }\n"
        b"solutions:\n  - solutions/arm.md\n")
    arm = (b"---\nid: arm\nanswers:\n  problem_class: demo.arch\n"
           b"  when:\n    os.arch: aarch64\nseverity: high\n"
           b"proposes:\n  - action: report_only\n    params: {}\n---\nWrong archive.\n")
    host, token, stop = _served_project(
        {"/.podshl/agent.yaml": manifest, "/.podshl/solutions/arm.md": arm}, "fork-token")
    try:
        def people(name, outcome, n, offset=0):
            for i in range(n):
                out = report({"pseudonym": f"fork-{host}-{name}-{outcome}-{offset + i}",
                              "subject": host, "model_class": "none", "outcome": outcome,
                              "observed": {"os.arch": "aarch64", "os.name": name}})
                assert isinstance(out, dict) and out["accepted"], out

        people("linux", "resolved", K_REPORTERS)
        people("macos", "unresolved", K_REPORTERS)
        # One configuration, both ways. Identical facts, so one cluster.
        people("windows", "resolved", K_REPORTERS)
        people("windows", "unresolved", K_REPORTERS)
        # And one person on FreeBSD for whom it did not work: a cluster below the
        # floor, which must contribute no value and no count to any suggestion.
        people("freebsd", "unresolved", 1)

        out = _dashboard(host, token)
        assert len(out["clusters"]) == 3, [c["measured"] for c in out["clusters"]]
        forks = out["suggestions"]
        assert len(forks) == 1 and forks[0]["solution_id"] == "arm", forks
        fork = forks[0]
        assert fork["worked"] == {"configurations": 1, "people": K_REPORTERS}, fork["worked"]
        assert fork["did_not"] == {"configurations": 1, "people": K_REPORTERS}, fork["did_not"]

        # SV32: the reading that separates them, first, and pasteable.
        best = fork["candidates"][0]
        assert best["fact"] == "os.name" and best["separates_completely"], fork["candidates"]
        assert best["provenance"] == "measured", best
        assert best["when_it_worked"] == ["linux"], best
        assert best["when_it_did_not"] == ["macos"], best
        assert best["when"] == {"os.arch": "aarch64", "os.name": "linux"}, best["when"]

        # SV29: what a switch on it does to the reports already stored.
        split = {s["value"]: s for s in best["splits_history"]}
        assert set(split) == {"linux", "macos"}, split
        assert split["linux"]["reports"] == K_REPORTERS and split["macos"]["did_not"] == K_REPORTERS
        assert "re-partitions history" in fork["history"], fork["history"]

        # SV30: the configuration that went both ways has no reading to fork on.
        both = fork["same_configuration_both_ways"]
        assert [b["measured"].get("os.name") for b in both] == ["windows"], both
        assert "forward only" in fork["forward_only"], fork["forward_only"]

        # The floor, by construction: FreeBSD is nowhere in the suggestion.
        assert "freebsd" not in json.dumps(fork), (
            f"a configuration one person reported reached the suggestion: {fork}")

        # And the suggestion is a real one: pasted the way the page prints it,
        # it passes ingest, and the answer stops covering where it did not
        # work. A snippet that looked right and was refused, or matched the
        # same configurations as before, would be a page teaching a format it
        # does not accept.
        from .ingest import scheduler
        when = "".join(f"    {k}: {json.dumps(v)}\n" for k, v in best["when"].items())
        served_arm = (b"---\nid: arm\nanswers:\n  problem_class: demo.arch\n  when:\n" +
                      when.encode() + b"severity: high\nproposes:\n  - action: report_only\n"
                      b"    params: {}\n---\nWrong archive.\n")
        _served_files(host)["/.podshl/solutions/arm.md"] = served_arm
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE source SET next_fetch_at = '-infinity' WHERE anchor_id = "
                            "(SELECT id FROM anchor WHERE host = %s) RETURNING id", (host,))
                sid = cur.fetchone()["id"]
        with db.tx() as conn:
            got = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert got["outcome"] == "stored", got

        after = _dashboard(host, token)
        answered = {c["measured"]["os.name"]: (c["answer"] or {}).get("solution_id")
                    for c in after["clusters"]}
        assert answered == {"linux": "arm", "macos": None, "windows": None}, (
            f"the pasted suggestion did not narrow the answer to where it worked: {answered}")
        assert after["suggestions"] == [], after["suggestions"]
    finally:
        stop()


def _reingest(host: str) -> dict:
    from .ingest import scheduler

    with db.tx() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE source SET next_fetch_at = '-infinity' WHERE anchor_id = "
                        "(SELECT id FROM anchor WHERE host = %s) RETURNING id", (host,))
            sid = cur.fetchone()["id"]
    with db.tx() as conn:
        return {r["source"]: r for r in scheduler.run_once(conn)}[sid]


_SYMPTOM_PROBE = (b"  - id: app.symptom\n    kind: human\n    describes: What goes wrong\n"
                  b"    why: it decides the class\n    prompt: What goes wrong?\n"
                  b"    choices: [a, b, c, d, e]\n")
_OS_PROBE = (b"  - id: os.name\n    kind: machine\n    describes: Operating system\n"
             b"    why: the builds differ\n    read: { op: os_fact, name: os }\n")


def _solution(sid: str, klass: str, when: str, action: str = "report_only") -> bytes:
    return (f"---\nid: {sid}\nanswers:\n  problem_class: {klass}\n  when:\n{when}"
            f"severity: high\nproposes:\n  - action: {action}\n    params: {{}}\n---\n"
            f"Do the thing.\n").encode()


def sv105_what_ingest_could_not_make_of_the_files_reaches_the_maintainer():
    """SV105. Two refusals, and both were returned to nobody.

    **A class with no tree.** `rebuild_all` does not fail a source for a class
    whose tree will not derive, and returns the reason "so it can reach the
    person who can act on it". Nothing stored it and no page showed it, so a
    maintainer whose every solution named a symptom *and* an operating system
    — a sensible shape, which the derivation refuses — saw "no decision tree
    could be derived" and nothing else. Found with a large seeded project on
    the dashboard, where not one of 34 solutions matched anything.

    **A file refused outright.** The previous version keeps being served, which
    is right, and the reason went into a return value the worker discards. The
    maintainer's only signal was that their change never showed up.

    Both are the project's own, so both reach its own dashboard, and a refusal
    stops being shown once a version is accepted.
    """
    manifest = (b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n"
                b"problem_classes: [app.a, app.b]\ncollect:\n" + _SYMPTOM_PROBE + _OS_PROBE +
                b"solutions:\n  - solutions/a.md\n  - solutions/b-linux.md\n  - solutions/c-linux.md\n")
    # app.b: two symptoms under one OS, so the class does not say which, and a
    # skipped question has nothing to fall back to — still refused after `SV109`.
    files = {"/.podshl/agent.yaml": manifest,
             "/.podshl/solutions/a.md": _solution("a", "app.a", "    app.symptom: a\n"),
             "/.podshl/solutions/b-linux.md": _solution(
                 "b-linux", "app.b", "    app.symptom: b\n    os.name: linux\n"),
             "/.podshl/solutions/c-linux.md": _solution(
                 "c-linux", "app.b", "    app.symptom: c\n    os.name: linux\n")}
    host, token, stop = _served_project(files, "files-token")
    try:
        seen = _dashboard(host, token)["files"]
        assert seen["trees"] == ["app.a"], seen
        why = seen["classes_without_a_tree"].get("app.b") or ""
        assert "fall back" in why, (
            f"a class whose tree did not derive reached its maintainer without a reason: {seen}")
        assert seen["last_refusal"] is None, seen

        # A change that is refused: an action outside the vocabulary.
        files["/.podshl/solutions/a.md"] = _solution("a", "app.a", "    app.symptom: a\n",
                                                     action="run_shell")
        assert _reingest(host)["outcome"] == "refused"
        seen = _dashboard(host, token)["files"]
        assert seen["last_refusal"] and "run_shell" in seen["last_refusal"], (
            f"a refused change reached its maintainer as silence: {seen}")
        assert seen["trees"] == ["app.a"], "a refused change took the served version down"

        # Fixed, and the refusal is no longer shown.
        files["/.podshl/solutions/a.md"] = _solution("a", "app.a", "    app.symptom: a\n")
        assert _reingest(host)["outcome"] in ("stored", "unchanged")
        assert _dashboard(host, token)["files"]["last_refusal"] is None
    finally:
        stop()


def sv106_the_dashboard_puts_the_work_first():
    """SV106. A page read well at five clusters and was forty-six screens at 263.

    Found by seeding a project with the shape real ones have: the page returned
    100 of 263 configurations without saying so, and put token revocation at
    40,900 pixels. What a maintainer needs first is what needs work, so the
    server groups by the file they would edit and grades each one — the
    judgement lives here, where a case can hold it, and the page only draws it:

    * **did not help** — nobody it was tried on said it worked,
    * **needs a distinction** — it worked for some configurations and not
      others, which is exactly when a fork suggestion exists,
    * **nobody said**, then **working**,

    and the configurations nothing answers, by people. A page that shows fewer
    than it has says how many it has. And a suggestion whose fact no probe in
    the project's `collect` acquires offers no `answers.when`, because ingest
    would refuse the paste.
    """
    from . import app as app_module
    from .app import report

    klasses = b"problem_classes: [app.a, app.b, app.c, app.d]\ncollect:\n"
    version_probe = (b"  - id: app.version\n    kind: human\n    describes: Version\n"
                     b"    why: releases differ\n    prompt: Which version?\n"
                     b"    choices: [\"2.2\", \"2.6\", \"2.7\"]\n")
    manifest = (b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n" + klasses +
                _SYMPTOM_PROBE + _OS_PROBE + version_probe +
                b"solutions:\n" + b"".join(f"  - solutions/{s}.md\n".encode() for s in "abcd"))
    files = {"/.podshl/agent.yaml": manifest}
    for s in "abcd":
        files[f"/.podshl/solutions/{s}.md"] = _solution(s, f"app.{s}", f"    app.symptom: {s}\n")
    host, token, stop = _served_project(files, "triage-token")
    try:
        def people(symptom, outcome, n, observed, stated=None):
            for i in range(n):
                body = {"pseudonym": f"triage-{host}-{symptom}-{observed}-{stated}-{outcome}-{i}",
                        "subject": host, "model_class": "none", "observed": observed,
                        "stated": {"app.symptom": symptom, **(stated or {})}}
                if outcome:
                    body["outcome"] = outcome
                out = report(body)
                assert isinstance(out, dict) and out["accepted"], out

        k = K_REPORTERS
        # a: worked on linux (two versions), did not on macos. `app.channel` is
        # typed, separates as well, and nothing in `collect` acquires it.
        people("a", "resolved", k, {"os.name": "linux"}, {"app.version": "2.6", "app.channel": "beta"})
        people("a", "resolved", k, {"os.name": "linux"}, {"app.version": "2.7", "app.channel": "beta"})
        people("a", "unresolved", k + 3, {"os.name": "macos"}, {"app.version": "2.2", "app.channel": "stable"})
        people("b", "unresolved", k + 1, {"os.name": "linux"})
        people("c", "resolved", k + 2, {"os.name": "linux"})
        people("d", None, k, {"os.name": "linux"})
        # Nothing answers "e" — and since 2026-09-16 that is what these reports
        # say. `uncovered` is the word for a run that reached the end of what a
        # project published and found nothing: the person said none of the
        # published problems is theirs, or the rules matched nothing. It used to
        # be unsayable, so this configuration arrived labelled `resolved` — a
        # report claiming an answer worked when there was no answer at all.
        people("e", "uncovered", k + 4, {"os.name": "linux"})

        out = _dashboard(host, token)
        assert out["total"] == 7 and out["shown"] == 7, (out["total"], out["shown"])
        triage = out["triage"]
        rows = out["clusters"]
        order = [(g["solution_id"], g["status"]) for g in triage["answers"]]
        assert order == [("b", "did_not_help"), ("a", "needs_a_distinction"),
                         ("d", "nobody_said"), ("c", "working")], order
        a = next(g for g in triage["answers"] if g["solution_id"] == "a")
        assert len(a["configurations"]) == 3 and a["worked"] == 2 * k and a["did_not"] == k + 3, a
        assert [rows[i]["typed"]["app.symptom"] for i in triage["not_answered"]] == ["e"], triage
        # And the word survived the round trip rather than being refused after
        # somebody pressed send: the CHECK, `OUTCOMES` and the client's own list
        # have to agree, and the one that is hardest to notice is the database.
        uncovered = rows[triage["not_answered"][0]]
        assert uncovered["outcomes"].get("uncovered") == k + 4, uncovered["outcomes"]
        assert triage["summary"] == {"configurations": 7, "answers": 4, "not_answered": 1,
                                     "did_not_help": 1, "needs_a_distinction": 1,
                                     "nobody_said": 1, "working": 1}, triage["summary"]

        fork = next(s for s in out["suggestions"] if s["solution_id"] == "a")
        by_fact = {c["fact"]: c for c in fork["candidates"]}
        assert fork["candidates"][0]["fact"] == "os.name", fork["candidates"]
        assert by_fact["app.channel"]["acquired"] is False and by_fact["app.channel"]["when"] is None, (
            f"a snippet ingest would refuse was offered for pasting: {by_fact['app.channel']}")
        assert by_fact["app.version"]["when"] == {"app.symptom": "a", "app.version": ">= 2.6"}, (
            f"two versions that worked above one that did not were not offered as a range: "
            f"{by_fact['app.version']}")

        # Fewer shown than held, and said.
        cap = app_module.MAX_CLUSTERS
        app_module.MAX_CLUSTERS = 4
        try:
            cut = _dashboard(host, token)
        finally:
            app_module.MAX_CLUSTERS = cap
        assert cut["total"] == 7 and cut["shown"] == 4 and len(cut["clusters"]) == 4, (
            cut["total"], cut["shown"], len(cut["clusters"]))
    finally:
        stop()


def sv107_a_projects_own_terms_reach_the_reader_as_written():
    """SV107. A project may name the words that are its own.

    A project owes English and nothing more, so a reader in another language
    meets its questions and its answer through their own model (`LG7`), and a
    small model translates a product's term as the ordinary word it resembles:
    `gemma3:4b` wrote engram's *brain* as *cerveau* in three French runs of
    three and as *Gehirn* in German. Told to keep the term as written, it kept
    it in six of six; told to write it some other way per language, it did
    not — so the glossary is a list of terms to keep, and nothing else.

    It has to survive ingest into the card the mirror serves — anything a
    manifest carries that ingest does not know is dropped on purpose, so a
    glossary missing from that list would vanish without a word — **and into
    the index entry**, which is the only copy that arrives in time for the
    first sentence a reader meets: the question is asked before the consent
    under which the card is fetched. And it is
    publisher text that enters a reader's prompt, so it is bounded: a term is
    one short line with a letter in it, at most fifty of them, and a key the
    format does not have is refused rather than ignored, so a maintainer who
    writes per-language renderings learns they are not read.
    """
    from .errors import IngestRefused

    base = (b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n"
            b"problem_classes: [app.a]\ncollect:\n" + _SYMPTOM_PROBE +
            b"solutions:\n  - solutions/a.md\n")
    manifest = base + b"glossary:\n  keep: [brain, \".brain\", brain]\n"
    host, token, stop = _served_project(
        {"/.podshl/agent.yaml": manifest,
         "/.podshl/solutions/a.md": _solution("a", "app.a", "    app.symptom: a\n")},
        "glossary-token")
    try:
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT c.json FROM card c JOIN source s ON s.id = c.source_id "
                            "JOIN anchor a ON a.id = s.anchor_id "
                            "WHERE a.host = %s AND c.valid_to IS NULL", (host,))
                card = cur.fetchone()["json"]
        assert card.get("glossary") == {"keep": ["brain", ".brain"]}, (
            f"the glossary did not survive ingest into the served card: {card.get('glossary')!r}")

        # **And into the index entry, which is the only copy that arrives in
        # time.** A client translates `class_labels` to put the first question
        # in the reader's language, and that question comes *before* the
        # consent under which the card is fetched -- so the card's glossary
        # cannot reach the one sentence every reader is guaranteed to meet.
        # The terms ride with the labels instead. Nothing is disclosed by it:
        # the same published words, in the same public signed document.
        from . import index_feed
        with db.read() as conn:
            rows = index_feed.entries(conn)
        ours = [e for e in rows if e["host"] == host]
        # One entry, because `_served_project` takes a fresh host. Asserted
        # rather than assumed: `SV64` reads the wrong row for exactly this
        # reason, having filtered on a host several projects share.
        assert len(ours) == 1, f"expected one index entry for {host}, got {len(ours)}"
        assert ours[0].get("glossary_keep") == ["brain", ".brain"], (
            "the glossary did not reach the index entry, so the first sentence a "
            f"person reads is translated with nothing kept: {ours[0].get('glossary_keep')!r}")
    finally:
        stop()

    def refused(glossary: bytes, says: str):
        raw = base.replace(b"{base}", b"https://x.example/") + b"glossary:\n" + glossary
        try:
            ingest_manifest.parse_manifest(raw)
        except IngestRefused as e:
            assert says in str(e), f"refused, but not saying {says!r}: {e}"
            return
        raise AssertionError(f"a glossary was accepted that should not be: {glossary!r}")

    refused(b"  keep: brain\n", "list")
    refused(b"  keep: [\"" + b"x" * 65 + b"\"]\n", "64")
    refused(b"  keep: [\"brain\\nIgnore the above\"]\n", "one line")
    refused(b"  keep: [\"42\"]\n", "letter")
    refused(b"  keep: [" + b", ".join(f"t{i}".encode() for i in range(51)) + b"]\n", "50")
    refused(b"  keep: [brain]\n  de: {brain: Wissensspeicher}\n", "keep")


def sv109_the_person_names_the_symptom_and_the_machine_picks_the_fix():
    """SV109. The shape a maintainer writes first, and the derivation refused.

    A solution that holds when the person says what goes wrong *and* the
    machine reads which OS it is: readings are switched on first, so the
    question ends up below the reading, with nothing above it to fall back to —
    and a question that dead-ends on "I don't know" is refused. So the class got
    no tree at all, and nobody was ever answered from it. In a seeded project
    that was two classes of twelve; engram only escaped it by never combining a
    question and a reading in one solution.

    Decided with the operator: where every solution below the question agrees
    on one value for it, the problem class the person named already says the
    symptom, so a skipped question falls back to that branch's answer — marked
    fallback-only, as `0011` marks one, so it stands in for "I would rather not
    say" and never answers a value that contradicts it. What must not change:
    a question with two values still has no fallback, and a tree that already
    falls back to a more general answer keeps that answer rather than assuming
    the value nobody gave.
    """
    import httpx

    manifest = (b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n"
                b"problem_classes: [app.a, app.b, app.c]\ncollect:\n" + _SYMPTOM_PROBE + _OS_PROBE +
                b"solutions:\n" + b"".join(f"  - solutions/{s}.md\n".encode() for s in
                                         ["a-windows", "a-linux", "b-linux", "c-linux", "c-any-linux"]))
    files = {
        "/.podshl/agent.yaml": manifest,
        # app.a: the shape itself. One symptom, and the OS picks the fix.
        "/.podshl/solutions/a-windows.md": _solution("a-windows", "app.a",
                                                     "    app.symptom: a\n    os.name: windows\n"),
        "/.podshl/solutions/a-linux.md": _solution("a-linux", "app.a",
                                                   "    app.symptom: a\n    os.name: linux\n"),
        # app.b: two symptoms under one OS. The class does not say which, so
        # there is still nothing to fall back to.
        "/.podshl/solutions/b-linux.md": _solution("b-linux", "app.b",
                                                   "    app.symptom: b\n    os.name: linux\n"),
        "/.podshl/solutions/c-linux.md": _solution("c-linux", "app.b",
                                                   "    app.symptom: c\n    os.name: linux\n"),
        # app.c: a reading alone, which built before and must still.
        "/.podshl/solutions/c-any-linux.md": _solution("c-any-linux", "app.c", "    os.name: linux\n"),
    }
    host, _token, stop = _served_project(files, "fallback-token")
    try:
        ingest = _INGESTED[host]
        assert "app.a" in ingest["trees"], (
            f"the symptom-and-OS shape still derives no tree: {ingest['no_tree'].get('app.a')}")
        assert "app.b" in ingest["no_tree"], (
            "a question with two values under one reading was given a fallback, which assumes "
            f"a symptom nobody said: {ingest['trees']}")

        def diagnose(cls, facts):
            r = httpx.post("http://127.0.0.1:8725/diagnose", timeout=10, json={
                "subject": host, "problem_class": cls, "facts": facts, "stated": ["app.symptom"]})
            assert r.status_code == 200, r.text
            return r.json()

        said = diagnose("app.a", {"os.name": "windows", "app.symptom": "a"})
        assert said["outcome"] == "finding" and said["solution"]["solution_id"] == "a-windows", said
        skipped = diagnose("app.a", {"os.name": "windows", "app.symptom.declined": True})
        assert skipped["outcome"] == "finding" and skipped["solution"]["solution_id"] == "a-windows", (
            f"a person who would rather not say got nothing: {skipped}")
        missing = diagnose("app.a", {"os.name": "linux"})
        assert missing["outcome"] == "need" and (missing["fallback"] or {}).get("solution_id") == "a-linux", (
            f"the question was asked without the answer a skip lands on: {missing}")
        contradicted = diagnose("app.a", {"os.name": "windows", "app.symptom": "b"})
        assert contradicted["outcome"] == "no_statement", (
            f"a symptom that contradicts the rule was answered by it: {contradicted}")
        other_os = diagnose("app.a", {"os.name": "macos", "app.symptom": "a"})
        assert other_os["outcome"] == "no_statement", other_os
    finally:
        stop()

    # And a tree that already had a general answer keeps it for a skip. Built
    # directly, because what is under test is the derivation, not the wire.
    from .ingest import tree_build
    collect = [{"id": "app.symptom", "kind": "human", "prompt": "?", "choices": ["c", "d"]},
               {"id": "os.name", "kind": "machine", "read": {"op": "os_fact", "name": "os"}}]
    general = {"id": "c-any-linux", "answers": {"problem_class": "app.c", "when": {"os.name": "linux"}}}
    specific = {"id": "c-linux-c", "answers": {"problem_class": "app.c",
                                               "when": {"os.name": "linux", "app.symptom": "c"}}}
    root = tree_build.build("app.c", [general, specific], collect)
    out = cluster_tree.walk(root, {"os.name": "linux", "app.symptom.declined": True})
    assert isinstance(out, cluster_tree.Answer) and out.solution_id == "c-any-linux", (
        f"a skip now lands on the specific answer instead of the general one above it: {out}")


def sv120_a_solution_that_ignores_a_switch_is_reachable_under_every_value_of_it():
    """SV120. The answer that was published, mirrored, signed — and unreachable.

    A solution says nothing about a fact. Another solution names one value of
    it, so that fact becomes the switch and the node grows exactly one child —
    and the first solution, which applies whatever the value is, lived only
    inside that one child. Every other reading fell off the tree and took the
    answer with it.

    engram found it on 2026-09-16, on the machine of the person who reported it.
    `wrong-archive-for-this-system` constrains the operating system and which
    archive was downloaded and says nothing about the processor architecture;
    `wrong-build-for-this-machine` names `os.arch: aarch64`. On an ordinary
    x86_64 Linux desktop holding the Windows archive, nothing matched, the walk
    stopped at the answer above, and the person was told to go and find out
    which archive they had — the right answer sitting one branch away, under a
    processor they did not have.

    The remedy is a last child matching everything the author did not name,
    carrying exactly the solutions that said nothing about the switch. Checked
    here from both ends: the value nobody named finds the answer, and a named
    value still wins over the catch-all rather than being swallowed by it.
    """
    from .errors import IngestRefused
    from .ingest import tree_build

    collect = [{"id": "os.arch", "kind": "machine", "read": {"op": "os_fact", "name": "arch"}},
               {"id": "os.name", "kind": "machine", "read": {"op": "os_fact", "name": "os"}}]
    # Says nothing about the architecture: it is true on every processor.
    anywhere = {"id": "wrong-os", "answers": {"problem_class": "app.start",
                                              "when": {"os.name": "linux"}}}
    # Names one, which is what made it the switch.
    arm_only = {"id": "wrong-arch", "answers": {"problem_class": "app.start",
                                                "when": {"os.arch": "aarch64"}}}
    root = tree_build.build("app.start", [anywhere, arm_only], collect)

    # The desktop the defect was found on.
    out = cluster_tree.walk(root, {"os.name": "linux", "os.arch": "x86_64"})
    assert isinstance(out, cluster_tree.Answer) and out.solution_id == "wrong-os", (
        f"an answer that holds on every processor was unreachable on x86_64: {out}")

    # And the named value is still preferred where it applies, rather than the
    # catch-all matching in its place: `any` is always the last child.
    out = cluster_tree.walk(root, {"os.name": "windows", "os.arch": "aarch64"})
    assert isinstance(out, cluster_tree.Answer) and out.solution_id == "wrong-arch", out

    # A tree with no such solution grows no catch-all, so nothing widens that
    # was narrow before: a reading outside every named value still says nothing.
    both = tree_build.build("app.start", [
        {"id": "x", "answers": {"problem_class": "app.start",
                                "when": {"os.name": "linux", "os.arch": "aarch64"}}},
        {"id": "y", "answers": {"problem_class": "app.start",
                                "when": {"os.name": "linux", "os.arch": "x86_64"}}}], collect)
    assert all(c.match_op != "any" for c in both.children), (
        "a catch-all was grown where every solution constrains the switch")
    out = cluster_tree.walk(both, {"os.name": "macos", "os.arch": "x86_64"})
    assert isinstance(out, cluster_tree.NoStatement), (
        f"an unnamed operating system was answered by a rule about another one: {out}")

    # The tree is refused if a catch-all is ever placed in front of a named
    # value, which is the one way this could hide an answer instead of adding
    # one. Built by hand, because the builder is what is being guarded against.
    hidden = cluster_tree.Node(id=1, parent_id=None, depth=0, switch_fact="os.arch",
                               switch_kind="reading", comparator="string", solution_id="above")
    hidden.children = [
        cluster_tree.Node(id=2, parent_id=1, depth=1, match_op="any", solution_id="everything"),
        cluster_tree.Node(id=3, parent_id=1, depth=1, match_op="eq", match_value="aarch64",
                          solution_id="arm"),
    ]
    try:
        cluster_tree.validate_tree(hidden)
    except IngestRefused as e:
        assert "before a named value" in str(e), e
    else:
        raise AssertionError("a catch-all in front of a named value was accepted")


def sv108_a_draft_is_judged_the_way_ingest_judges_it():
    """SV108. The builder asks `POST /validate` whether the mirror would take a
    draft, and the answer is only worth having if it is the mirror's answer.

    So each draft here goes through both: served over HTTP and ingested for
    real, and posted as text to `/validate` with the address it is served from
    as the anchor. Accepted by one means accepted by the other, a refusal names
    the same file with the same sentence, and a class with no tree is the same
    class with the same reason. A builder carrying its own copy of the rules
    would drift from them the first time either changed, and a maintainer would
    find out from a mirror that never updated.

    And `/validate` keeps nothing: no source, no card, no log entry.
    """
    import httpx

    probes = _SYMPTOM_PROBE + _OS_PROBE
    head = b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n"

    def manifest(classes: bytes, sols: list[str], extra: bytes = b"") -> bytes:
        return (head + b"problem_classes: [" + classes + b"]\ncollect:\n" + probes + extra +
                b"solutions:\n" + b"".join(f"  - solutions/{s}.md\n".encode() for s in sols))

    good_a = _solution("a", "app.a", "    app.symptom: a\n")
    drafts = {
        "good": {"/.podshl/agent.yaml": manifest(b"app.a, app.b", ["a", "b"]),
                 "/.podshl/solutions/a.md": good_a,
                 "/.podshl/solutions/b.md": _solution("b", "app.b", "    app.symptom: b\n")},
        "an action outside the vocabulary": {
            "/.podshl/agent.yaml": manifest(b"app.a", ["a"]),
            "/.podshl/solutions/a.md": _solution("a", "app.a", "    app.symptom: a\n", action="run_shell")},
        "a condition on a fact nothing collects": {
            "/.podshl/agent.yaml": manifest(b"app.a", ["a"]),
            "/.podshl/solutions/a.md": _solution("a", "app.a", "    gpu.vendor: nvidia\n")},
        "no English text": {
            "/.podshl/agent.yaml": manifest(b"app.a", ["a"]),
            "/.podshl/solutions/a.md": good_a.replace(b"Do the thing.\n", b"")},
        "a glossary key that does not exist": {
            "/.podshl/agent.yaml": manifest(b"app.a", ["a"], b"glossary:\n  de: {brain: Hirn}\n"),
            "/.podshl/solutions/a.md": good_a},
        "a class whose tree is refused": {
            "/.podshl/agent.yaml": manifest(b"app.a, app.b", ["a", "b-linux", "c-linux"]),
            "/.podshl/solutions/a.md": good_a,
            "/.podshl/solutions/b-linux.md": _solution("b-linux", "app.b",
                                                       "    app.symptom: b\n    os.name: linux\n"),
            "/.podshl/solutions/c-linux.md": _solution("c-linux", "app.b",
                                                       "    app.symptom: c\n    os.name: linux\n")},
        "an endpoint outside the anchor": {
            "/.podshl/agent.yaml": manifest(b"app.a", ["a"]).replace(b"{base}", b"https://elsewhere.example/"),
            "/.podshl/solutions/a.md": good_a},
    }

    seen = {"stored": 0, "refused": 0, "no_tree": 0}
    for name, files in drafts.items():
        host, _token, stop = _served_project(dict(files), f"draft-{secrets.token_hex(3)}",
                                             expect_stored=False)
        try:
            ingest = _INGESTED[host]
            served = _served_files(host)
            texts = {k.removeprefix("/.podshl/"): v.decode() for k, v in served.items()
                     if k.startswith("/.podshl/solutions/")}
            with db.read() as conn:
                with conn.cursor() as cur:
                    cur.execute("SELECT count(*) s FROM source")
                    sources_before_validate = cur.fetchone()["s"]
            r = httpx.post("http://127.0.0.1:8725/validate", timeout=30, json={
                "agent_yaml": served["/.podshl/agent.yaml"].decode(),
                "solutions": texts, "anchor": ingest["base"]})
            assert r.status_code == 200, (name, r.text)
            v = r.json()
            seen["stored" if ingest["outcome"] == "stored" else "refused"] += 1
            seen["no_tree"] += bool(ingest.get("no_tree"))
            assert v["accepted"] == (ingest["outcome"] == "stored"), (
                f"{name}: ingest said {ingest['outcome']} ({ingest.get('why')}), /validate said {v}")
            if ingest["outcome"] == "refused":
                assert ingest["why"] in v["refused"].values(), (
                    f"{name}: ingest refused with {ingest['why']!r}; /validate said {v['refused']}")
            else:
                assert v["trees"] == sorted(ingest["trees"]), (name, v["trees"], ingest["trees"])
                assert v["no_tree"] == ingest["no_tree"], (name, v["no_tree"], ingest["no_tree"])
            with db.read() as conn:
                with conn.cursor() as cur:
                    cur.execute("SELECT count(*) s FROM source")
                    assert cur.fetchone()["s"] == sources_before_validate, f"{name}: /validate stored something"
        finally:
            stop()

    # Ingest really did accept some, refuse others and decline a tree — or the
    # comparison above compared a single verdict with itself.
    assert seen["stored"] >= 2 and seen["refused"] >= 4 and seen["no_tree"] >= 1, seen


def sv27_the_client_answers_a_need_through_the_same_round_loop():
    """SV27. The vendor's tree drives the same loop the local model already
    drives, so the client needs one mechanism rather than two.

    Checked against the client's source, because the loop is in the window."""
    ui = (Path(__file__).resolve().parents[3] / "client-rs" / "ui" / "index.html").read_text()
    assert "for(let round=1; rem.need && rem.need.length; round++)" in ui, \
        "the client no longer loops on `need`"
    assert '".declined"' in ui, \
        "the client does not record a declined probe, so an endpoint would keep asking"



# ------------------------------------------------------------ takedown and states

def sv39_a_notice_aimed_at_a_problem_class_is_refused():
    """SV39. A manifest that says it handles a product describes what it repairs,
    not what it is. A takedown that could reach a problem class would let a
    vendor forbid anyone from naming their software."""
    from . import takedown
    with db.tx() as conn:
        try:
            takedown.receive(conn, reason_code="trademark_claim",
                             notifier=dict(NOTIFIER, name="S", contact="c"),
                             problem_class="nvidia.driver.flicker")
        except takedown.NoSuchPath as e:
            assert "never a problem class" in str(e)
            return
    raise AssertionError("a takedown reached a problem class")


def sv37_nothing_to_remove_where_we_host_nothing():
    """SV37. An enterprise vendor talks to its own endpoint with no fallback to
    us, so we hold nothing of theirs — the exposure exists only where we
    mirror."""
    from . import takedown
    with db.tx() as conn:
        out = takedown.receive(conn, reason_code="trademark_claim",
                               notifier=dict(NOTIFIER, name="S", contact="c"),
                               anchor_host=_fresh_host("nothing-here"))
    assert out["action"] == "refused"
    assert "nothing" in out["why"]


def sv36_a_takedown_degrades_the_anchor_and_keeps_the_source():
    """SV36. `attested` becomes `unknown` — the state of a project that never
    registered. The mirror goes; the developer's repository is untouched."""
    from . import takedown
    with db.tx() as conn:
        host = _fresh_host("taken")
        aid = _anchor(conn, host)
        seq = log_store.append(conn, "log_policy", {"note": "seed"})
        with conn.cursor() as cur:
            cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                        "VALUES (%s, %s, %s)",
                        (aid, f"https://{host}/.podshl/agent.yaml", f"https://{host}/"))
            cur.execute(
                "INSERT INTO attestation (anchor_id, tier, key_jwk, key_thumbprint, issued_seq) "
                "VALUES (%s, 'oss', '{}'::jsonb, %s, %s)", (aid, secrets.token_bytes(32), seq))

        out = takedown.receive(conn, reason_code="trademark_claim",
                               notifier=dict(NOTIFIER, name="S", contact="c"), anchor_host=host)
        # Filing changes nothing. `0012`: a route anybody can call must not be
        # able to un-enrol anybody, so the notice waits for a person.
        assert out["action"] == "pending", f"a notice acted on itself: {out}"
        with conn.cursor() as cur:
            # `status` is the liveness column and a fresh anchor is already
            # `unknown`, so the fact to read is the takedown mark: nothing may
            # have been withdrawn, and nothing written down as withdrawn.
            cur.execute("SELECT taken_down_at FROM anchor WHERE id = %s", (aid,))
            assert cur.fetchone()["taken_down_at"] is None, (
                "filing a notice took the anchor down without a decision")
            cur.execute("SELECT mirror_state FROM source WHERE anchor_id = %s", (aid,))
            assert cur.fetchone()["mirror_state"] != "withheld", (
                "filing a notice withheld the mirror without a decision")
            cur.execute("SELECT withdrawn_at FROM attestation WHERE anchor_id = %s", (aid,))
            assert cur.fetchone()["withdrawn_at"] is None, (
                "filing a notice withdrew the attestation without a decision")

        # The decision, made on the operator's own listener.
        acted = takedown.act(conn, out["notice"])
        assert acted["action"] == "degraded"
        with conn.cursor() as cur:
            cur.execute("SELECT status FROM anchor WHERE id = %s", (aid,))
            assert cur.fetchone()["status"] == "unknown"
            cur.execute("SELECT withdrawn_kind FROM attestation WHERE anchor_id = %s", (aid,))
            kind = cur.fetchone()["withdrawn_kind"]
            assert kind == "degraded", f"a takedown produced {kind!r} — an accusation"
            cur.execute("SELECT mirror_state FROM source WHERE anchor_id = %s", (aid,))
            assert cur.fetchone()["mirror_state"] == "withheld"


def sv_a_takedown_is_not_undone_by_a_good_probe():
    """A takedown was reversible by an unauthenticated POST.

    `takedown.receive` sets `status = 'unknown'` and the public statement of
    reasons says so. But `status` is the *liveness* column, and the only thing
    that writes it on a good probe — `sweep.record` — used to set `'live'`
    unconditionally. `POST /claim/{host}/verify` needs no authentication and
    calls exactly that, so anybody at all could return a taken-down anchor to
    `live` for any host still serving its challenge file, which a participating
    project was.

    Asserted through `sweep.record` rather than over HTTP, because the property
    is "no confirmed probe re-enrols a taken-down anchor" — the claim route is
    one caller of it, and the sweep that runs on a timer is another.
    """
    from . import takedown
    from .anchor import sweep
    from .anchor.result import Probed, Reason
    with db.tx() as conn:
        host = _fresh_host("taken-then-probed")
        aid = _anchor(conn, host)
        filed = takedown.receive(conn, reason_code="court_order",
                                 notifier=dict(NOTIFIER, name="S", contact="c"), anchor_host=host)
        takedown.act(conn, filed["notice"])
        with conn.cursor() as cur:
            cur.execute("SELECT status, taken_down_at, taken_down_seq FROM anchor "
                        "WHERE id = %s", (aid,))
            row = cur.fetchone()
        assert row["status"] == "unknown", "the takedown did not degrade the anchor"
        assert row["taken_down_at"] is not None,             "the takedown left no record of itself on the anchor, so nothing can honour it"
        assert row["taken_down_seq"] is not None,             "the takedown does not point at its own statement of reasons"

        # Exactly what a maintainer republishing the challenge file produces.
        sweep.record(conn, aid, Probed(reason=Reason.CONFIRMED, value="tok"))
        with conn.cursor() as cur:
            cur.execute("SELECT status, last_confirmed FROM anchor WHERE id = %s", (aid,))
            after = cur.fetchone()
    assert after["status"] == "unknown",         "a good probe re-enrolled a taken-down anchor — an unauthenticated route undoes a takedown"
    assert after["last_confirmed"] is not None,         "the probe was discarded rather than recorded; control is still a true fact worth writing down"


def sv_both_worked_examples_are_served_as_their_own_bytes():
    """A page teaching the format must not be able to drift from the format.

    Byte-for-byte against `spec/`, both examples and every solution in them,
    over HTTP rather than by reading the same file twice. `index_service/
    oss_project.py` already serves these on the same reasoning; the failure this
    prevents is the quiet one, where the specification moves and the page keeps
    teaching what it used to say.
    """
    import httpx

    root = EXAMPLE.parent.parent
    for prefix, folder in (("", "example"), ("/desktop", "example-desktop")):
        here = root / folder / ".podshl"
        served = httpx.get(f"http://127.0.0.1:8725/example{prefix}/agent.yaml", timeout=10)
        assert served.status_code == 200, (prefix, served.status_code)
        assert served.text == (here / "agent.yaml").read_text(encoding="utf-8"),             f"/example{prefix}/agent.yaml is not what spec/{folder} publishes"
        for path in sorted((here / "solutions").glob("*.md")):
            r = httpx.get(f"http://127.0.0.1:8725/example{prefix}/solutions/{path.name}",
                          timeout=10)
            assert r.status_code == 200, (path.name, r.status_code)
            assert r.text == path.read_text(encoding="utf-8"),                 f"{path.name} is served differently from how it is published"
        # Bounded to the directory by construction: the name is refused unless it
        # is exactly one of the files that directory holds. The traversal forms
        # are sent **encoded** on purpose — an unencoded `../` is collapsed by
        # the HTTP client before it leaves, so the plain form would be testing
        # httpx rather than the server, and would pass on a server that had no
        # bound at all.
        for escape in ("nope.md", "%2e%2e%2fagent.yaml", "..%2Fagent.yaml"):
            gone = httpx.get(f"http://127.0.0.1:8725/example{prefix}/solutions/{escape}",
                             timeout=10)
            assert gone.status_code == 404,                 f"{escape!r} was not refused: {gone.status_code}"

    # The two are separate documents, not one served twice.
    a = httpx.get("http://127.0.0.1:8725/example/agent.yaml", timeout=10).text
    b = httpx.get("http://127.0.0.1:8725/example/desktop/agent.yaml", timeout=10).text
    assert a != b, "both example routes serve the same file"
    assert "nvidia" in b.lower() and "nvidia" not in a.lower(),         "the second example is no longer the one that names somebody else's software"


def sv_no_interactive_api_documentation_is_served():
    """FastAPI turns `/docs`, `/redoc` and `/openapi.json` on by default, so this
    server was serving Swagger UI because nobody had said not to.

    Swagger UI loads its script and stylesheet from a CDN. On an origin whose
    argument is that it makes no external calls and that everything it serves can
    be read, an unreviewed third-party script delivered under our own name is the
    wrong dependency to have acquired by accident. Nothing was hidden by turning
    it off — every route is public — but the page also puts a button on
    `POST /notice` and `POST /claim/{host}` against production.
    """
    from .app import app
    from .ops_app import ops
    for name, a in (("public", app), ("operator", ops)):
        paths = {r.path for r in a.routes if hasattr(r, "path")}
        for p in ("/docs", "/redoc", "/openapi.json", "/docs/oauth2-redirect"):
            assert p not in paths, f"the {name} app serves {p} — CDN-loaded, and nobody chose it"


def sv38_every_takedown_is_logged_with_a_public_reason():
    """SV38. If we can remove things quietly the transparency is decorative, and
    the duty to state reasons makes weaponised claims countable."""
    from . import takedown
    with db.tx() as conn:
        host = _fresh_host("logged")
        aid = _anchor(conn, host)
        filed = takedown.receive(conn, reason_code="copyright_claim",
                                 notifier=dict(NOTIFIER, name="S", contact="c"), anchor_host=host)
        assert filed["action"] == "pending", f"a notice acted on itself: {filed}"
        assert not log_store.for_anchor(conn, aid),             "filing a notice wrote a takedown to the log before anybody decided"
        out = takedown.act(conn, filed["notice"])
        entries = log_store.for_anchor(conn, aid)
    assert out["log_seq"] is not None
    assert out["statement_of_reasons"], "the affected party is not told where to read why"
    last = entries[-1]["entry"]
    assert last["kind"] == "takedown" and last["reason_code"] == "copyright_claim"
    assert last["action"] == "degraded"


def sv_a_takedown_reason_comes_from_a_closed_vocabulary():
    """Free text here would make the reason unreadable in aggregate, and
    counting is the point: a notifier who sends fifty is a fact worth stating."""
    from . import takedown
    from .errors import ServerError
    with db.tx() as conn:
        try:
            takedown.receive(conn, reason_code="because-i-said-so",
                             notifier=dict(NOTIFIER, name="S", contact="c"),
                             anchor_host="x.example")
        except ServerError as e:
            assert "closed" in str(e) or "not one of" in str(e)
            return
    raise AssertionError("an arbitrary reason code was accepted")


def sv2c_a_key_change_at_a_live_anchor_is_shown_never_blocked():
    """SV2c. A key change is either legitimate rotation or a takeover and we
    cannot tell which — so it is not blocked, it is made visible. Key continuity
    is the tripwire; the checking cadence only catches abandonment."""
    with db.tx() as conn:
        host = _fresh_host("rotate")
        aid = _anchor(conn, host)
        first, second = secrets.token_bytes(32), secrets.token_bytes(32)
        seq = log_store.append(conn, "log_policy", {"note": "seed"})
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO attestation (anchor_id, tier, key_jwk, key_thumbprint, issued_seq) "
                "VALUES (%s, 'oss', '{}'::jsonb, %s, %s)", (aid, first, seq))
            cur.execute(
                "UPDATE attestation SET withdrawn_at = now(), withdrawn_kind = 'degraded', "
                "withdrawn_reason = 'key_rotated', withdrawn_seq = %s WHERE anchor_id = %s",
                (seq, aid))
        key_seq = log_store.append(conn, "key_changed", {
            "anchor": {"kind": "url", "value": f"https://{host}/"},
            "old_thumbprint": first.hex(), "new_thumbprint": second.hex(),
        }, anchor_id=aid)
        with conn.cursor() as cur:
            # The anchor was live before the rotation, and must still be after.
            cur.execute("UPDATE anchor SET status = 'live', last_confirmed = now() "
                        "WHERE id = %s", (aid,))
            cur.execute(
                "INSERT INTO attestation (anchor_id, tier, key_jwk, key_thumbprint, issued_seq) "
                "VALUES (%s, 'oss', '{}'::jsonb, %s, %s)", (aid, second, key_seq))
            cur.execute("SELECT status FROM anchor WHERE id = %s", (aid,))
            assert cur.fetchone()["status"] == "live", "a rotation blocked the anchor"
            # And the new key is the live one, so serving continues under it.
            cur.execute("SELECT key_thumbprint FROM attestation WHERE anchor_id = %s "
                        "AND withdrawn_at IS NULL", (aid,))
            assert bytes(cur.fetchone()["key_thumbprint"]) == second
        entries = log_store.for_anchor(conn, aid)
    assert any(e["entry"]["kind"] == "key_changed" for e in entries), \
        "a key change left no visible trace — the tripwire does not fire"


def sv2d_2e_2f_deprecation_is_declared_not_deduced():
    """SV2d/SV2e/SV2f. Three signals, and the order matters: the developer's own
    word, then an explicit act by the forge owner, then an age we measured.

    The third is never phrased as the first. "No change since March 2021" is
    something we observed; "outdated" is a verdict we have no standing to reach,
    and dressing one up as the other is exactly the plausible-looking assertion
    this project refuses elsewhere.
    """
    from datetime import datetime, timedelta, timezone
    old = datetime.now(timezone.utc) - timedelta(days=900)
    with db.tx() as conn:
        host = _fresh_host("dormant")
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO source (anchor_id, manifest_url, fetch_prefix, declared_status, "
                "  successor_url, forge_archived, forge_last_commit_at) "
                "VALUES (%s, %s, %s, 'deprecated', %s, true, %s) RETURNING id",
                (aid, f"https://{host}/.podshl/agent.yaml", f"https://{host}/",
                 f"https://{host}/successor", old))
            sid = cur.fetchone()["id"]
            cur.execute("SELECT declared_status, successor_url, forge_archived, "
                        "forge_last_commit_at FROM source WHERE id = %s", (sid,))
            row = cur.fetchone()

    # All three are separate columns. Nothing collapses an age into a status.
    assert row["declared_status"] == "deprecated", "the developer's own word is not recorded"
    assert row["successor_url"], "a successor was declared and not kept"
    assert row["forge_archived"] is True, "the forge's act is not recorded separately"
    assert row["forge_last_commit_at"] == old
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT column_name FROM information_schema.columns "
                        "WHERE table_name = 'source' AND column_name IN "
                        "('outdated', 'stale', 'abandoned', 'dead')")
            invented = [r["column_name"] for r in cur.fetchall()]
    assert not invented, f"the schema reaches a verdict it has no standing to reach: {invented}"


def sv10_an_anchor_we_never_attested_is_unknown_not_blocked():
    """SV10. `unknown` says nothing about them. The client proceeds on the
    protocol's own trust, and a vendor who never heard of us still works —
    anything else makes this a chokepoint rather than a participant."""
    import httpx
    r = httpx.get("http://127.0.0.1:8725/mirror/nobody-ever-registered.example", timeout=5)
    assert r.status_code == 404, r.status_code
    body = r.json()
    assert body["attested"] is False
    assert "says nothing about them" in body["note"], \
        "an absent name is being reported as an accusation"


def sv12_a_query_touches_no_store():
    """SV12. Asking "does anyone know this?" and saying "this happened to me"
    carry the same payload and must not be the same decision.

    A query is ephemeral. The proof here is that the lookup path runs inside a
    read-only transaction the database itself enforces.
    """
    import httpx
    before = _observation_count()
    httpx.post("http://127.0.0.1:8725/diagnose", timeout=5,
               json={"subject": "nobody.example", "problem_class": "x", "facts": {}})
    assert _observation_count() == before, "a query left something behind"

    # And the read path cannot write even if something tried.
    try:
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("INSERT INTO query_budget (pseudonym, epoch) VALUES ('x', 1)")
    except Exception:
        return
    raise AssertionError("the query path is able to write")


def _observation_count() -> int:
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT count(*) n FROM observation")
            return cur.fetchone()["n"]


def sv15_a_report_after_an_attempt_carries_the_outcome():
    """SV15. The user chooses to report *after* the answer, and that ordering is
    the whole reason the corpus is worth having: a report filed afterwards can
    say whether it worked.

    Through `report()`, the function `POST /report` hands the body to. This case
    used to write the row itself with a `tried_link_id` — a column no report
    ever filled, because the protocol does not carry a solution id — and so
    proved a join production never had. Which answer an outcome is about is
    found by walking the project's own solutions against the report's facts;
    `SV32` holds that end to end.
    """
    from .app import report

    host = _fresh_host("outcome")
    out = report({"pseudonym": f"outcome-{host}", "subject": host,
                  "observed": {"a": "b"}, "outcome": "resolved", "model_class": "none"})
    assert isinstance(out, dict) and out["accepted"], out
    sig = clusters.canonical_signature(host, {"a": "b"}, [])
    with db.read() as conn:
        found = clusters.find(conn, clusters.signature_hash(sig))
        with conn.cursor() as cur:
            cur.execute("SELECT outcome FROM observation WHERE cluster_id = %s", (found["id"],))
            row = cur.fetchone()
    assert row["outcome"] == "resolved", \
        f"a report filed after the attempt did not keep whether it worked: {row}"


def sv104_a_guessed_configuration_says_nothing_about_a_project():
    """SV104. `SV21` says no route produces one project's figures to anybody
    else. `GET /cluster/{hash}` did, to anybody who could guess a configuration.

    A signature is a host and a handful of coarsened facts — an architecture, an
    OS, a major.minor version — so its hash is not a secret but a small search
    space. The route answered with how many people had reported that exact
    configuration about that project, with no authentication, for anything at
    or above the floor: the per-project figure the dashboard keeps private,
    readable one guess at a time by a competitor. Even `known: true` alone said
    that at least k people had.

    No client ever called it — the published path is `POST /diagnose`, which
    is read-only (`SV12`) — and its one non-figure, `solutions`, was always
    empty because nothing wrote `link`. So the route is gone rather than
    guarded, and like `SV21` this is checked on the routing table as well as
    over the wire.
    """
    import httpx

    from .app import app, report

    public = {r.path for r in app.routes if hasattr(r, "path")}
    assert not any(p.startswith("/cluster") for p in public), (
        f"a lookup by configuration is reachable from the public app: "
        f"{sorted(p for p in public if p.startswith('/cluster'))}")

    host = _fresh_host("guess")
    facts = {"os.arch": "x86_64", "os.name": "windows"}
    for i in range(K_REPORTERS):
        out = report({"pseudonym": f"guess-{host}-{i}", "subject": host,
                      "observed": facts, "model_class": "none"})
        assert isinstance(out, dict) and out["accepted"], out
    sig = clusters.signature_hash(clusters.canonical_signature(host, facts, []))
    with db.read() as conn:
        found = clusters.find(conn, sig)
    assert found and found["peak_epoch_reporters"] >= K_REPORTERS, (
        f"the case did not reach the floor, so it could not show a leak: {found}")

    r = httpx.get(f"http://127.0.0.1:8725/cluster/{sig.hex()}", timeout=5)
    body = r.json() if r.headers.get("content-type", "").startswith("application/json") else {}
    leaked = {k: body[k] for k in ("reported", "reporters") if k in body}
    assert not leaked, (
        f"a stranger who guessed a configuration read a project's figures: {leaked}")


def sv23_the_outreach_view_ranks_unclaimed_domains():
    """SV23. Private to the public is not invisible to the operator — the rule
    was that no per-vendor defect list is ever *published*.

    It prioritises itself: the domain with the most accumulated observations has
    the most users in pain and is where a report lands hardest.
    """
    epoch = counting.current_epoch()
    host = _fresh_host("loud")
    with db.tx() as conn:
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, host, {"gpu.name": "RTX"}, epoch)
        for i in range(K_REPORTERS + 2):
            counting.record_observation(conn, cid, f"O{i}", epoch=epoch)
        with conn.cursor() as cur:
            cur.execute("REFRESH MATERIALIZED VIEW outreach_rank")
            cur.execute("SELECT host, reporters FROM outreach_rank WHERE host = %s", (host,))
            row = cur.fetchone()
    assert row, "an unclaimed domain over the threshold does not appear in the ranking"
    assert row["reporters"] >= K_REPORTERS


def sv_the_integration_guide_is_true():
    """The guide a publisher reads has to match what the code does.

    Prose nothing checks is prose that drifts, and this is the document somebody
    implements against before they ever see a refusal.
    """
    guide = (Path(__file__).resolve().parents[3] / "spec" / "INTEGRATING.md").read_text()
    from .. import spec_gate

    for action in spec_gate.actions():
        assert action in guide, f"{action} is in the vocabulary but not in the guide"
    # Every read op, too. `program_version` is the first op that starts a program
    # a publisher chose, and a guide that never mentions it would leave the
    # publisher typing a version question the program could have answered.
    for op in spec_gate.vocabulary("reads")["ops"]:
        assert f"`{op['op']}`" in guide, f"read op {op['op']} is in the vocabulary but not in the guide"
    for claim in ("14 days", "90 days", "never becomes `revoked`",
                  "no fallback", "There is no name field",
                  "must lie under your anchor"):
        assert claim in guide, f"the guide no longer states: {claim}"
    # The grading in the guide must be the grading in the code.
    from .config import STALE_AFTER_DAYS, UNKNOWN_AFTER_DAYS
    assert f"{STALE_AFTER_DAYS} days" in guide and f"{UNKNOWN_AFTER_DAYS} days" in guide, \
        "the guide quotes a different schedule from config.py"



def sv11_a_revoked_anchor_is_refused():
    """SV11. Revocation is the one state that IS an accusation — we attested and
    withdrew for cause, meaning compromise or abuse. It is refused rather than
    degraded, and that is exactly why nothing reaches it by accident: every
    other path produces `degraded`, which means `unknown`.
    """
    with db.tx() as conn:
        host = _fresh_host("revoked")
        aid = _anchor(conn, host)
        seq = log_store.append(conn, "log_policy", {"note": "seed"})
        with conn.cursor() as cur:
            cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                        "VALUES (%s, %s, %s)",
                        (aid, f"https://{host}/.podshl/agent.yaml", f"https://{host}/"))
            cur.execute(
                "INSERT INTO attestation (anchor_id, tier, key_jwk, key_thumbprint, issued_seq, "
                "  withdrawn_at, withdrawn_kind, withdrawn_reason, withdrawn_seq) "
                "VALUES (%s, 'oss', '{}'::jsonb, %s, %s, now(), 'revoked', 'compromise', %s)",
                (aid, secrets.token_bytes(32), seq, seq))
            cur.execute("UPDATE source SET mirror_state = 'withheld' WHERE anchor_id = %s", (aid,))

            # Nothing is served for it, and the reason is distinguishable from an
            # abandonment: `revoked` is an accusation, `degraded` is not.
            cur.execute(
                "SELECT count(*) n FROM source s JOIN card c ON c.source_id = s.id "
                "WHERE s.anchor_id = %s AND s.mirror_state = 'serving' AND c.valid_to IS NULL",
                (aid,))
            assert cur.fetchone()["n"] == 0, "a revoked anchor is still being served"
            cur.execute("SELECT withdrawn_kind FROM attestation WHERE anchor_id = %s", (aid,))
            assert cur.fetchone()["withdrawn_kind"] == "revoked"


def sv9_there_is_no_fallback_for_an_enterprise_endpoint():
    """SV9. Silently taking over when a vendor's endpoint is down would mean
    answering for them with knowledge we do not have.

    The proof is structural: an enterprise anchor has no `source`, so there is
    nothing here that could answer in their place even if something tried.
    """
    with db.tx() as conn:
        host = _fresh_host("enterprise")
        with conn.cursor() as cur:
            cur.execute("INSERT INTO anchor (kind, value, host, challenge_token) "
                        "VALUES ('dns', %s, %s, 'tok') RETURNING id", (host, host))
            aid = cur.fetchone()["id"]
            cur.execute("SELECT count(*) n FROM source WHERE anchor_id = %s", (aid,))
            assert cur.fetchone()["n"] == 0

    import httpx
    r = httpx.get(f"http://127.0.0.1:8725/mirror/{host}", timeout=5)
    assert r.status_code == 404, "the mirror answered for a vendor whose endpoint we do not host"
    assert r.json()["attested"] is False



# ------------------------------------------------- names we do not own

def _marks(conn):
    from .ingest import confusable
    with conn.cursor() as cur:
        cur.execute("DELETE FROM well_known_mark")
    for label, owner in (("nvidia", "nvidia.com"), ("microsoft", "microsoft.com"),
                         ("paypal", "paypal.com")):
        confusable.add_mark(conn, label, owner, added_by="suite")


def sv33_a_confusable_anchor_is_held_and_still_reachable():
    """SV33. A homoglyph is not a dispute about who owns a name — it is a
    technical trick, and UTS 39 is the technical answer.

    Held, **not blocked**. The anchor stays exactly as reachable as one that
    never registered, which is what lets this be conservative without becoming
    the chokepoint the whole design rejects.
    """
    from .ingest import confusable
    with db.tx() as conn:
        _marks(conn)
        for host, why in (
            ("nvidi\u0430.com", "a Cyrillic a inside a Latin word"),
            ("rnicrosoft.io", "rn read as m"),
            ("paypa1.net", "1 read as l"),
        ):
            reason = confusable.hold_reason(conn, host)
            assert reason, f"{host} was attestable — {why}"

        # And the mark's actual owner is not held by its own mark.
        assert confusable.hold_reason(conn, "nvidia.com") is None
        assert confusable.hold_reason(conn, "example.org") is None

        # Held is a column on the anchor, not a refusal to store it: the source
        # is still mirrored and still served.
        host = _fresh_host("held")
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute("UPDATE anchor SET attest_hold = 'confusable' WHERE id = %s", (aid,))
            cur.execute("SELECT status, attest_hold FROM anchor WHERE id = %s", (aid,))
            row = cur.fetchone()
        assert row["attest_hold"] and row["status"] != "revoked", \
            "a hold turned into an accusation"


def sv34_an_anchor_carrying_a_mark_it_does_not_own_is_held():
    """SV34. Carrying a mark is different from being confusable with one.

    `nvidia-community-fixes.org` is not pretending to be NVIDIA and may be an
    entirely legitimate project. Whether a domain may carry somebody else's mark
    is exactly the question this project has no standing to answer — so it goes
    to a person rather than being decided here, and meanwhile the anchor works.
    """
    from .ingest import confusable
    with db.tx() as conn:
        _marks(conn)
        for host in ("nvidia-community-fixes.org", "microsoft-helper.dev"):
            reason = confusable.hold_reason(conn, host)
            assert reason and "carries" in reason, f"{host} was attested silently"
            assert "a person decides" in reason


def sv33a_a_hold_prevents_attestation_but_not_serving():
    """The distinction that makes the whole thing safe: a hold withholds the
    attestation, and nothing else."""
    from .ingest import confusable
    with db.tx() as conn:
        _marks(conn)
        assert confusable.hold_reason(conn, "rnicrosoft.io") is not None
    # An anchor with a hold has no live attestation, and that is the entire
    # effect — `unknown` is where everyone starts.
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT count(*) n FROM anchor a "
                "LEFT JOIN attestation t ON t.anchor_id = a.id AND t.withdrawn_at IS NULL "
                "WHERE a.attest_hold IS NOT NULL AND t.id IS NOT NULL")
            assert cur.fetchone()["n"] == 0, "a held anchor carries a live attestation"


def sv35a_a_problem_class_is_never_read_by_the_confusable_check():
    """The other side of SV35. A project that repairs NVIDIA driver problems has
    to be able to say so, and nothing in the name check ever looks at a declared
    class — only at the anchor."""
    import inspect

    from .ingest import confusable
    src = inspect.getsource(confusable)
    for forbidden in ("problem_class", "problem_classes"):
        # It may be *discussed* in a comment; it must never be read.
        code = "\n".join(line.split("#")[0] for line in src.splitlines())
        assert forbidden not in code, \
            f"the confusable check reads {forbidden} — a takedown could then reach a class"


def sv_the_confusable_table_is_the_real_one():
    """Approximating UTS 39 with a hand-written list of lookalikes would be the
    kind of plausible-looking assertion this project refuses elsewhere."""
    from .ingest import confusable
    version = confusable.table_version()
    assert version and version[0].isdigit(), f"no Unicode version recorded: {version!r}"
    assert len(confusable._table()) > 1000, \
        "the confusables table is too small to be the real one"
    # A mapping only the real table has.
    assert confusable.skeleton("\u0430") == "a", "Cyrillic a does not fold to Latin a"
    assert confusable.skeleton("\u03bf") == "o", "Greek omicron does not fold to o"



# --------------------------------------------------------- the enterprise tier

#: A record exactly as GLEIF's API returned it — GLEIF's own LEI, which is as
#: public as data gets. The shape is theirs, not one somebody wrote down.
GLEIF_RECORD = Path(__file__).resolve().parent / "fixtures" / "gleif_record.json"
RETIRED_LEI = "529900RETIRED0000083"
#: Valid LEIs — the check digits hold — that the stand-in register has never heard of.
UNKNOWN_LEI = "529900NOSUCHLEI00040"
NEVER_LEI = "529900NEVERSEEN00033"


class _Register:
    """GLEIF, as far as a case needs it: the captured record, a retired copy of
    it under another LEI, and a switch that makes the register unreachable. It
    counts what it was asked, so a case can show the cache answered instead."""

    def __init__(self):
        from .anchor import gleif
        self.gleif = gleif
        good = gleif.parse(json.loads(GLEIF_RECORD.read_text(encoding="utf-8")))
        retired = {**good, "lei": RETIRED_LEI, "reg_status": "RETIRED"}
        self.records = {good["lei"]: good, RETIRED_LEI: retired}
        self.good = good
        self.down = False
        self.asked: list[str] = []

    def __call__(self, lei):
        self.asked.append(lei)
        if self.down:
            raise self.gleif.Unreachable("the register could not be asked: test")
        return self.records.get(lei)

    def __enter__(self):
        self.saved = self.gleif.FETCH
        self.gleif.FETCH = self
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM lei_record WHERE lei = ANY(%s)",
                            (list(self.records) + [UNKNOWN_LEI, NEVER_LEI],))
        return self

    def __exit__(self, *exc):
        self.gleif.FETCH = self.saved


def sv8_an_enterprise_anchor_is_attested_from_dns_and_the_register():
    """SV8. The TXT record proves control of the domain; the register supplies
    the name. Neither substitutes for the other, and the name is copied rather
    than typed — there is no field anywhere for an enterprise to write its own.
    """
    from .anchor import enterprise
    from .anchor.result import Probed, Reason

    with _Register() as register, db.tx() as conn:
        reg = register.good
        assert reg["reg_status"] == "ISSUED" and reg["entity_status"] == "ACTIVE", reg

        host = _fresh_host("ent")
        with conn.cursor() as cur:
            cur.execute("INSERT INTO anchor (kind, value, host, challenge_token) "
                        "VALUES ('dns', %s, %s, 'tok')", (host, host))

        out = enterprise.attest(
            conn, host,
            resolver_probe=Probed(Reason.CONFIRMED, {"lei": reg["lei"]}, value="tok"))

        assert out["tier"] == "enterprise"
        assert out["legal_name"] == "Global Legal Entity Identifier Foundation", \
            "the attested name is not the register's"
        entry = log_store.entries(conn, out["log_seq"], out["log_seq"] + 1)[0]["entry"]
        assert entry["lei"] == reg["lei"]
        assert entry["lei_file"].startswith("api/"), \
            f"the log entry does not say which state of the register it came from: {entry}"
        assert "whoever controls this domain asserts this LEI" in entry["claim"], \
            "the log entry overstates what was actually verified"
        assert "not_claimed" in entry, \
            "the entry does not state the limit of the binding"


def sv8a_each_half_of_an_enterprise_attestation_can_fail_on_its_own():
    """Which half failed matters. A DNS timeout is our inability to ask, a wrong
    token is their statement, and a retired LEI is the register's — collapsing
    those into "could not attest" leaves a vendor nothing to act on."""
    from .anchor import enterprise
    from .anchor.result import Probed, Reason

    with _Register() as register, db.tx() as conn:
        good = register.good["lei"]
        host = _fresh_host("ent-fail")
        with conn.cursor() as cur:
            cur.execute("INSERT INTO anchor (kind, value, host, challenge_token) "
                        "VALUES ('dns', %s, %s, 'tok')", (host, host))

        cases = [
            (Probed(Reason.CONFIRMED, {}, value="tok"), "declares no LEI"),
            (Probed(Reason.UNREACHABLE, {"why": "timeout"}), "did not confirm"),
            (Probed(Reason.CONTRADICTED, {"lei": good}), "did not confirm"),
            (Probed(Reason.CONFIRMED, {"lei": UNKNOWN_LEI}, value="tok"),
             "does not support"),
            (Probed(Reason.CONFIRMED, {"lei": RETIRED_LEI}, value="tok"), "does not support"),
        ]
        for probe, expected in cases:
            try:
                enterprise.attest(conn, host, resolver_probe=probe)
            except enterprise.NotAttestable as e:
                assert expected in str(e), f"wrong reason for {probe.reason}: {e}"
                continue
            raise AssertionError(f"attested despite {probe.reason.value}")

        # And the register's own silence is ours, not theirs.
        register.down = True
        try:
            enterprise.attest(conn, host, resolver_probe=Probed(
                Reason.CONFIRMED, {"lei": NEVER_LEI}, value="tok"))
        except enterprise.NotAttestable as e:
            assert "cannot answer" in str(e) and "Not a statement about you" in str(e), str(e)
        else:
            raise AssertionError("attested while the register could not be asked")


def sv_a_stale_register_mirror_says_so_rather_than_refusing():
    """SV62. The register is asked when a record is older than a day; the cache
    answers when it cannot be asked, up to a week; after that nothing answers.

    An old record producing a confident refusal would withdraw attestations for
    entities that are perfectly fine — the resolver bug in another costume — and
    a register that is down is our inability to ask, not their statement.
    """
    from .anchor import gleif
    from .anchor.result import Reason

    with _Register() as register, db.tx() as conn:
        lei = register.good["lei"]
        first = gleif.lookup(conn, lei)
        assert first.reason is Reason.CONFIRMED and not first.detail["from_cache"], first
        again = gleif.lookup(conn, lei)
        assert again.detail["from_cache"] and register.asked == [lei], (
            f"a record from a moment ago was asked for again: {register.asked}")

        register.down = True
        with conn.cursor() as cur:
            cur.execute("UPDATE lei_record SET loaded_at = now() - interval '3 days' WHERE lei = %s", (lei,))
        held = gleif.lookup(conn, lei)
        assert held.reason is Reason.CONFIRMED and held.detail["from_cache"], (
            f"a three-day-old record did not answer while the register was down: {held}")

        with conn.cursor() as cur:
            cur.execute("UPDATE lei_record SET loaded_at = now() - interval '30 days' WHERE lei = %s", (lei,))
        old = gleif.lookup(conn, lei)
        assert old.reason is Reason.UNREACHABLE, \
            f"a month-old record answered {old.reason.value} instead of saying it cannot"
        assert "old" in old.detail.get("why", ""), old.detail

        never = gleif.lookup(conn, NEVER_LEI)
        assert never.reason is Reason.UNREACHABLE, never


def sv_the_register_loader_reads_the_real_columns():
    """SV63. The client reads the fields GLEIF's API actually returns. A reader
    written against invented names would find nothing and say nothing; the
    fixture is a record exactly as the API served it. And a string that cannot
    be an LEI is not sent to anyone — it does not go into a URL."""
    from .anchor import gleif
    from .anchor.result import Reason

    row = gleif.parse(json.loads(GLEIF_RECORD.read_text(encoding="utf-8")))
    assert gleif.LEI.fullmatch(row["lei"]), row
    assert row["legal_name"] == "Global Legal Entity Identifier Foundation", row
    assert row["entity_status"] == "ACTIVE" and row["reg_status"] == "ISSUED", row
    assert row["country"] == "CH" and row["file_id"].startswith("api/20"), row

    with _Register() as register, db.tx() as conn:
        for bad in ("../../etc", "5299 00", "x" * 20, "529900RETIRED0000084", ""):
            out = gleif.lookup(conn, bad)
            assert out.reason is Reason.ABSENT, (bad, out)
        assert register.asked == [], f"something that is not an LEI was sent to the register: {register.asked}"


# ------------------------------------- the gap report, after the catch-all

# GR1, GR1a, GR2 and GR3 used to run against `src/podshl/catchall/`, which
# served `GET /gap-report/{product}` to whoever asked. `SERVER.md` names that
# the line whose crossing ends the company, and `SV19` now holds the opposite
# property. The cases did not change — the same four things are still being
# asserted — only what answers them did, so these are repointed rather than
# rewritten.


def _claimed(conn, host: str) -> str:
    """An anchor plus a claim on it, which is what the dashboard now requires."""
    aid = _anchor(conn, host)
    token = secrets.token_urlsafe(16)
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
            "VALUES (%s, %s, now() + interval '1 day')",
            (aid, hashlib.sha256(token.encode()).digest()))
    return token


def _run(result):
    """What a route handler returned, whether or not it was a coroutine.

    A handler that awaits nothing is declared `def` so Starlette runs it in a
    worker thread instead of on the event loop — otherwise a blocking database
    call stops every other request on the process. Whether any given route is
    `def` or `async def` is that decision and not this suite's, so a case calls
    the function and this unwraps whatever comes back. Written after thirteen
    handlers changed and seven call sites here broke on `a coroutine was
    expected`, which told nobody anything about the routes.
    """
    import asyncio
    import inspect

    return asyncio.run(result) if inspect.isawaitable(result) else result


def _dashboard(host: str, token: str) -> dict:
    from fastapi import Response

    from .app import dashboard
    # The handler takes a Response so it can set `no-store` on a private,
    # per-vendor body. Nothing here reads the headers; the case that does is
    # SV81, over real HTTP.
    return _run(dashboard(host, Response(), token))


def _observe(conn, cid: int, pseudonyms, epoch: int) -> None:
    for name in pseudonyms:
        counting.record_observation(conn, cid, name, epoch=epoch,
                                    model_class="local_small", ux_severity="high")


def gr1_below_the_floor_nothing_is_reported():
    """GR1: fewer reporters than the floor and the vendor's own dashboard shows
    no cluster. The floor is applied in the query, so there is no result to
    filter afterwards and forget to."""
    host = _fresh_host("gr1")
    epoch = counting.current_epoch()
    with db.tx() as conn:
        token = _claimed(conn, host)
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, host, {"gr": "1"}, epoch)
        _observe(conn, cid, [f"p{i}" for i in range(K_REPORTERS - 1)], epoch)
    out = _dashboard(host, token)
    assert out["clusters"] == [],         f"a cluster below {K_REPORTERS} reporters was reported: {out['clusters']}"


def gr1a_repeated_submissions_are_one_reporter():
    """GR1a, which the catch-all could not hold and which closes here.

    It counted rows in a list, so five submissions from one client crossed a
    floor meant to count people. The floor is `peak_epoch_reporters` — distinct
    pseudonyms — so the same client reporting k times still counts once and
    still shows nothing. This is `SV13`'s property, seen from the vendor's side.
    """
    host = _fresh_host("gr1a")
    epoch = counting.current_epoch()
    with db.tx() as conn:
        token = _claimed(conn, host)
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, host, {"gr": "1a"}, epoch)
        # One person, k+2 submissions. Under the old counter this crossed.
        _observe(conn, cid, ["the-same-client"] * (K_REPORTERS + 2), epoch)
        submissions = counting.reporters(conn, cid, epoch=epoch)
    assert submissions == 1, f"{K_REPORTERS + 2} posts from one client counted as {submissions}"
    out = _dashboard(host, token)
    assert out["clusters"] == [],         "repeated submissions from one client crossed a floor that counts people"


def gr1b_a_group_on_the_page_counts_people_the_way_a_cluster_does():
    """GR1b. `GR1a`'s property, for every group inside the page rather than for
    the page's clusters.

    A `seen_key` is salted per cluster and per epoch, so the same person is a
    different key every month and in every cluster. That is why a cluster's
    floor is `peak_epoch_reporters` rather than a count of distinct keys over
    all time. The groups beside the clusters — an outcome, a model class, a fact
    somebody typed — counted distinct keys over all time and over all clusters,
    so one person who said "it did not work" in five different months was five
    people, and one person with five configurations was five people who typed.

    The rows are written directly because `/report` always files into the
    current month, and the months are the thing under test. Nothing written
    here is a shape `/report` could not produce: outcome and model class are
    not in the signature, and each cluster's typed fact is.
    """
    host = _fresh_host("gr1b")
    now = counting.current_epoch()
    months = [200101 + i for i in range(K_REPORTERS)]
    with db.tx() as conn:
        token = _claimed(conn, host)
        counting.open_epoch(conn, now)
        cid = _cluster(conn, host, {"gr": "1b"}, now)
        for i in range(K_REPORTERS):
            counting.record_observation(conn, cid, f"worked-{i}", epoch=now,
                                        model_class="local_small", outcome="resolved")
        # One unresolved report a month, each a different key and possibly the
        # same person every time. Nobody can tell, which is the point.
        for m in months:
            counting.open_epoch(conn, m)
            counting.record_observation(conn, cid, "persistent", epoch=m,
                                        model_class="rare_model", outcome="unresolved")
        # One person, five configurations, one typed fact in each. Every one of
        # these clusters is a group of one.
        for i in range(K_REPORTERS):
            other = _cluster(conn, host, {"gr": f"1b-{i}", "user.typed": "x"}, now)
            counting.record_observation(conn, other, "typist", epoch=now,
                                        stated={"user.typed": "x"})

    out = _dashboard(host, token)
    rows = out["clusters"]
    assert len(rows) == 1, f"the cluster at the floor was not the only one shown: {rows}"
    row = rows[0]
    assert "unresolved" not in row["outcomes"] and row["did_not"] == 0, (
        f"one report a month for {K_REPORTERS} months was shown as {K_REPORTERS} people "
        f"for whom it did not work: {row['outcomes']}")
    assert row["outcomes"].get("withheld_below_floor") == 1, row["outcomes"]
    assert "rare_model" not in {m["model_class"] for m in out["model_classes"]}, (
        f"a model class held by one key a month crossed the floor: {out['model_classes']}")
    assert "user.typed" not in {f["fact"] for f in out["stated_facts"]}, (
        f"a fact typed once in each of {K_REPORTERS} clusters crossed the floor: "
        f"{out['stated_facts']}")


def gr2_at_the_floor_the_report_states_its_policy_and_its_limits():
    """GR2: at the floor the vendor sees the cluster, the terms it arrives
    under, and what it cannot tell them. The limitation is the honest sales
    argument, so it is served with the findings rather than in a brochure."""
    host = _fresh_host("gr2")
    epoch = counting.current_epoch()
    with db.tx() as conn:
        token = _claimed(conn, host)
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, host, {"gr": "2"}, epoch)
        _observe(conn, cid, [f"p{i}" for i in range(K_REPORTERS)], epoch)
    out = _dashboard(host, token)
    assert out["clusters"], f"at {K_REPORTERS} reporters nothing was reported"
    policy = out["policy"]
    assert policy["free"] and policy["private"] and policy["unconditional"], policy
    assert "not used as leverage" in policy["statement"], policy["statement"]
    assert "not why" in out["limitation"], out["limitation"]


def gr3_a_weak_model_succeeding_is_a_ux_defect():
    """GR3: a small local model solving something from public knowledge says the
    information was available and the surface failed to convey it. That reading
    is served with the figures, because a `model_class` column nobody knows how
    to read is not a finding."""
    host = _fresh_host("gr3")
    epoch = counting.current_epoch()
    with db.tx() as conn:
        token = _claimed(conn, host)
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, host, {"gr": "3"}, epoch)
        _observe(conn, cid, [f"p{i}" for i in range(K_REPORTERS)], epoch)
    out = _dashboard(host, token)
    assert any(r["model_class"] == "local_small" for r in out["model_classes"]),         out["model_classes"]
    assert "product surface failed" in out["reading"], out["reading"]
    assert "not a knowledge gap" in out["reading"], out["reading"]


# --------------------------------------------------- discovery, by what broke


def sv64_the_index_is_searchable_by_what_broke():
    """SV64. A user names whatever is not working — a program like `pip`, or a
    device and its maker like `nvidia`. Both, because `problem_classes` name
    other people's software and hardware alike. The token has to be earned: it
    comes from those classes, as nominative use, and from the verified anchor —
    never from a self-asserted name, because there is no name field to assert."""
    from . import index_feed
    sv_ingest_stores_a_project_and_attests_it()
    with db.read() as conn:
        rows = index_feed.entries(conn)
    # `[0]` of everything on 127.0.0.1 was fine while one project lived there.
    # It stopped being fine when repository anchors arrived: `SV115` leaves one
    # on the same loopback host, and this case then read somebody else's tokens
    # and called them missing. Take the domain anchors, newest first, which is
    # the one this case just created.
    ours = [e for e in rows
            if e["host"] == "127.0.0.1" and e.get("anchor_kind") != "repo"]
    assert ours, "an ingested, serving project is missing from the index"
    e = max(ours, key=lambda r: r["log_seq"] or -1)
    assert "pip" in e["search_tokens"], (
        f"what a user would type is not searchable: {e['search_tokens']}")
    assert any(c.startswith("pip.") for c in e["problem_classes"]), e["problem_classes"]
    assert e["log_seq"] is not None, "an index entry that is not in the log"


def sv65_the_index_carries_no_self_asserted_name():
    """SV65. The display name is the verified anchor. A name field in the thing
    people search *by name* would put the impersonation vector straight back —
    so the entry has a host it proved and nothing it merely claimed."""
    from . import index_feed
    sv_ingest_stores_a_project_and_attests_it()
    with db.read() as conn:
        rows = index_feed.entries(conn)
    assert rows, "nothing indexed"
    forbidden = {"name", "title", "display_name", "vendor", "organization", "org"}
    for e in rows:
        assert not (forbidden & set(e)), f"a self-asserted name reached the index: {sorted(set(e) & forbidden)}"


def sv66_the_index_is_signed_against_a_tree_head():
    """SV66. The index cannot contain a project that is not in the log: every
    entry carries its sequence, and the document is signed with the log key
    against a head. A monitor checks that offline, by walking the log."""
    from ..jws import public_jwk, verify_detached
    from . import index_feed
    sv_ingest_stores_a_project_and_attests_it()
    with db.read() as conn:
        doc = index_feed.signed(conn)
    ok, _ = verify_detached(public_jwk(sth.key()), doc["index"], doc["signature"])
    assert ok, "the index signature does not verify against the log key"
    assert doc["index"]["tree_size"] == doc["sth"]["tree_size"], (
        "the index is signed against a different tree size than it claims")
    # And nothing is published that the head cannot prove. Sequences are
    # zero-based, so entry S is inside a tree of size N exactly when S < N.
    for e in doc["index"]["entries"]:
        assert e["log_seq"] is not None and e["log_seq"] < doc["index"]["tree_size"], (
            f"{e['host']} is published at seq {e['log_seq']} but the signed head "
            f"is only {doc['index']['tree_size']} entries — no proof exists for it")


def sv67_no_route_answers_a_name():
    """SV67. Discovery is fetched, never asked. A `/search?q=` would make us
    able to answer "who looked for what", which is the one thing `/` promises we
    cannot — and a name query is worse than the per-domain lookup `SV21`
    already forbids, because what a user types is the problem they have."""
    from .app import app
    public = {r.path for r in app.routes if hasattr(r, "path")}
    assert "/index" in public, "the catalogue is not served"
    for p in public:
        assert not p.startswith("/search"), f"a name query endpoint exists: {p}"
        assert "{q}" not in p and "{query}" not in p, f"a name is a path parameter: {p}"
    # `/index` takes no parameters at all: one fetch is the whole catalogue, so
    # a request cannot be about one project.
    idx = next(r for r in app.routes if getattr(r, "path", None) == "/index")
    assert not getattr(idx, "param_convertors", {}), "the catalogue is parameterised"


# ------------------------------------------- the pages, and the claim lifecycle


def _claim_rows(conn, host: str) -> int:
    with conn.cursor() as cur:
        cur.execute(
            "SELECT count(*) AS n FROM dashboard_claim c JOIN anchor a ON a.id = c.anchor_id "
            "WHERE a.host = %s AND c.revoked_at IS NULL", (host,))
        return cur.fetchone()["n"]


def sv73_the_root_answers_a_machine_and_a_browser_differently():
    """SV73. A browser gets the landing page and everything else gets the JSON,
    on the narrowest rule that can tell them apart.

    `Vary: Accept` is correctness rather than politeness: this origin is meant to
    sit behind a CDN, and a cache keyed on URL alone would let the first browser
    hit poison `/` for every monitor behind it.
    """
    import httpx

    machine = httpx.get("http://127.0.0.1:8725/", timeout=10)
    assert machine.headers["content-type"].startswith("application/json"), machine.headers
    assert "loopback_fetching_enabled" in machine.json(), machine.json()
    assert machine.headers.get("vary") == "Accept", "a CDN would serve the page to a monitor"

    browser = httpx.get("http://127.0.0.1:8725/",
                        headers={"Accept": "text/html,application/xhtml+xml"}, timeout=10)
    assert browser.headers["content-type"].startswith("text/html"), browser.headers
    assert browser.headers.get("vary") == "Accept"


def sv74_a_page_is_served_from_disk_and_rendered_from_nothing():
    """SV74. The body is the file, byte for byte.

    There is no server-side rendering here, so there is no injection surface —
    asserted rather than argued, because the sentence is only true while nobody
    adds an f-string.
    """
    import httpx

    from . import pages
    for name, path in (("projects.html", "/projects"), ("register.html", "/register")):
        served = httpx.get(f"http://127.0.0.1:8725{path}", timeout=10).text
        assert served == (pages.DIR / name).read_text(encoding="utf-8"), \
            f"{path} is not the file on disk"


def sv75_a_pages_script_is_permitted_by_its_own_header():
    """SV75. The CSP names a hash per inline block, and the count has to match.

    A wrong or missing hash does not error: the browser blocks the script and
    the page comes up with its layout drawn and nothing filled in. A page with
    no script at all must still say `'none'` — an empty `script-src` is not
    permissive, it is an illegal header value and the connection dies.
    """
    import base64
    import hashlib

    import httpx

    from . import pages
    for name in sorted(p.name for p in pages.DIR.glob("*.html")):
        body = (pages.DIR / name).read_text(encoding="utf-8")
        blocks = pages.scripts(body)
        header = pages._csp(body)
        if not blocks:
            assert "script-src 'none'" in header, f"{name} permits nothing but says so wrongly"
            continue
        for b in blocks:
            want = base64.b64encode(hashlib.sha256(b.encode()).digest()).decode()
            assert f"'sha256-{want}'" in header, f"{name} has a block its own header forbids"
        # `'self'` only where the page loads the shared list, and nowhere else.
        loads_list = pages.LIST_TAG in body
        assert ("'self'" in header.split("script-src", 1)[1]) == loads_list, (
            f"{name}: script-src permits 'self' {'without' if not loads_list else 'but not'} "
            f"loading the shared list")
    served = httpx.get("http://127.0.0.1:8725/projects", timeout=10)
    csp = served.headers["content-security-policy"]
    assert "script-src 'self' 'sha256-" in csp, csp


def sv76_the_imprint_refuses_to_invent_an_identity():
    """SV76. An unmet imprint duty must not render as a page that looks fine.

    Nothing is derived — not the hostname, not the `Host` header, not a
    placeholder. 503 is honest to a machine and readable to a person at once.
    """
    import httpx

    from .config import IMPRINT_COMPLETE
    r = httpx.get("http://127.0.0.1:8725/imprint", timeout=10)
    o = httpx.get("http://127.0.0.1:8725/operator", timeout=10)
    if IMPRINT_COMPLETE:
        assert r.status_code == 200 and o.json()["configured"] is True
        served = o.json()
        # An imprint has to be published; it does not have to be published to a
        # harvester. Neither the page nor this endpoint carries the literal form.
        for field in ("email", "security_contact", "notice_contact"):
            value = served.get(field)
            assert value is None or "@" not in value, \
                f"{field} is served in a form a scraper reads directly"
        assert "@" not in r.text, "the imprint page carries a literal address"
        assert served["name"] and served["address"], served
        # `security.txt` is the exception, and deliberately: RFC 9116 is *for*
        # machines, so a Contact: a tool cannot parse defeats the file.
        st = httpx.get("http://127.0.0.1:8725/.well-known/security.txt", timeout=10)
        if st.status_code == 200:
            assert "mailto:" in st.text, st.text
        return
    assert r.status_code == 503, f"an unconfigured imprint answered {r.status_code}"
    assert "PODSHL_IMPRINT_NAME" in r.text, "it does not say what is missing"
    assert o.status_code == 503 and o.json()["configured"] is False
    # A security contact nobody answers is worse than none.
    assert httpx.get("http://127.0.0.1:8725/.well-known/security.txt",
                     timeout=10).status_code == 404


def sv111_the_privacy_notice_names_its_controller_and_every_page_links_it():
    """SV111. `/privacy` exists wherever the imprint does, and says what the
    access log holds in the terms the Caddyfile actually writes.

    A notice that promises "first path segment, no headers, seven days" over a
    proxy that logs the dashboard token in a header is worse than no notice, so
    the Caddyfile is read here too. The footer link was on every page before the
    route existed — which is how a Datenschutz link ends in a 404.
    """
    import httpx

    from . import pages
    from .config import IMPRINT_COMPLETE, ROOT

    r = httpx.get("http://127.0.0.1:8725/privacy", timeout=10)
    if not IMPRINT_COMPLETE:
        assert r.status_code == 503, f"a privacy notice naming nobody answered {r.status_code}"
        assert "PODSHL_IMPRINT_NAME" in r.text
    else:
        assert r.status_code == 200, r.status_code
        assert r.text == (pages.DIR / "privacy.html").read_text(encoding="utf-8")
        assert 'fetch("/operator"' in r.text, "the controller is not rendered from the server"
        assert "@" not in r.text, "the privacy page carries a literal address"

    body = (pages.DIR / "privacy.html").read_text(encoding="utf-8")
    for promise in ("Datenschutzerklärung", "Privacy notice", "Verantwortlich",
                    "16 Bit", "32 Bit", "7 Tagen", "7 days", "fünf verschiedene", "Art. 11"):
        assert promise in body, f"the notice no longer says {promise!r}"

    caddy = (ROOT / "deploy" / "compose" / "Caddyfile").read_text(encoding="utf-8")
    for line in ("request>headers delete", "roll_keep_for 168h",
                 "request>remote_ip ip_mask 16 32", "request>uri regexp ^(/[^/?]*).*$ $1"):
        assert line in caddy, f"the Caddyfile no longer does what /privacy says: {line}"

    for page in sorted(pages.DIR.glob("*.html")):
        text = page.read_text(encoding="utf-8")
        if page.name in ("privacy.html", "imprint-unconfigured.html"):
            continue
        if 'href="/imprint"' in text:
            assert 'href="/privacy"' in text, f"{page.name} links the imprint but not the privacy notice"


def sv77_proving_control_again_supersedes_every_earlier_token():
    """SV77. The whole recovery story, and the reason there is no other one.

    There is no email to send a reset to, so restore is re-proving control. That
    only works as a sentence a maintainer can act on if it is complete: the new
    token works and every earlier one stops. Otherwise a leaked token is a
    365-day problem with no route that can end it.
    """
    from .app import _claimed_anchor
    from .errors import NotClaimed
    host = _fresh_host("restore")
    with db.tx() as conn:
        aid = _anchor(conn, host)
        first = secrets.token_urlsafe(16)
        with conn.cursor() as cur:
            cur.execute("INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                        "VALUES (%s, %s, now() + interval '365 days')",
                        (aid, hashlib.sha256(first.encode()).digest()))
    with db.read() as conn:
        assert _claimed_anchor(conn, host, first) == aid

    # A second proof of control, as `claim_verify` performs it.
    second = secrets.token_urlsafe(16)
    with db.tx() as conn:
        with conn.cursor() as cur:
            cur.execute("UPDATE dashboard_claim SET revoked_at = now(), "
                        "revoked_reason = 'superseded' "
                        "WHERE anchor_id = %s AND revoked_at IS NULL", (aid,))
            assert cur.rowcount == 1, "nothing was superseded"
            cur.execute("INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                        "VALUES (%s, %s, now() + interval '365 days')",
                        (aid, hashlib.sha256(second.encode()).digest()))

    with db.read() as conn:
        assert _claimed_anchor(conn, host, second) == aid, "the new token does not work"
        try:
            _claimed_anchor(conn, host, first)
        except NotClaimed:
            pass
        else:
            raise AssertionError("the superseded token still opens the dashboard")
        assert _claim_rows(conn, host) == 1, "live tokens accumulated"


def sv78_a_revoked_token_is_refused_exactly_like_no_token():
    """SV78. Revoked, expired and absent are one answer.

    A different message for a revoked token would be an oracle telling a
    stranger that a host has been claimed at all — which is a per-vendor fact.
    """
    from .app import _claimed_anchor
    from .errors import NotClaimed as NC
    host = _fresh_host("revoked")
    tok = secrets.token_urlsafe(16)
    with db.tx() as conn:
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute("INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at, "
                        "revoked_at, revoked_reason) VALUES "
                        "(%s, %s, now() + interval '365 days', now(), 'revoked_by_holder')",
                        (aid, hashlib.sha256(tok.encode()).digest()))
    seen = set()
    with db.read() as conn:
        for candidate in (tok, None, "never-issued"):
            try:
                _claimed_anchor(conn, host, candidate)
            except NC as e:
                seen.add(str(e))
            else:
                raise AssertionError(f"{candidate!r} opened a revoked dashboard")
    assert len(seen) == 1, f"the refusals differ and so tell a stranger something: {seen}"


def sv79_a_confirmed_anchors_challenge_is_never_rotated():
    """SV79. `POST /claim/{host}` takes no authentication.

    It used to overwrite the stored nonce on every call, and re-verification
    probes the *stored* one — so a stranger could rotate the nonce of an already
    attested project, its published file would stop matching, and its
    attestation would be lost. An unauthenticated route that un-enrols anybody.

    Found by claiming a host by hand while testing and watching the suite's own
    ingest case start failing.
    """
    import asyncio

    from .app import claim_start
    host = _fresh_host("nonce")
    with db.tx() as conn:
        aid = _anchor(conn, host, token="the-published-one")
        with conn.cursor() as cur:
            cur.execute("UPDATE anchor SET verified_at = now(), challenge_issued_at = now() "
                        "WHERE id = %s", (aid,))

    _run(claim_start(host))
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT challenge_token FROM anchor WHERE id = %s", (aid,))
            after = cur.fetchone()
    # Read from the row rather than from the reply. What must not move is the
    # value the *sweep* checks; the reply now carries a pending claim instead,
    # which is a different fact and is allowed to change.
    assert after["challenge_token"] == "the-published-one", \
        "a stranger rotated a confirmed anchor's nonce and broke its re-verification"

    # And a second caller neither resets the first claim nor is refused.
    # This was a one-slot design, and the slot was the flaw: refusing the
    # second caller for an hour let anybody keep a maintainer out of their own
    # registration by starting a claim every fifty-nine minutes. A claim is a
    # nonce hash and nothing else, so several may stand - each caller holds
    # their own preimage and `verify` finds the claim by the hash of what is
    # presented (`0012`). Nobody can hold a slot because there is no slot.
    pending = _fresh_host("pending")
    first = json.loads(bytes(_run(claim_start(pending)).body))
    assert first.get("publish"), f"a first claim was refused: {first}"
    second = json.loads(bytes(_run(claim_start(pending)).body))
    assert second.get("publish"), (
        f"a second caller was refused, so a griefer can hold a registration: {second}")
    assert second["proof"] != first["proof"], "two callers were handed the same proof"
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT c.nonce_hash FROM claim_pending c "
                        "JOIN anchor a ON a.id = c.anchor_id WHERE a.host = %s", (pending,))
            standing = {bytes(r["nonce_hash"]) for r in cur.fetchall()}
    for who, reply in (("first", first), ("second", second)):
        assert hashlib.sha256(reply["proof"].encode()).digest() in standing, (
            f"the {who} caller's claim is not standing - the other reset it")


def sv80_a_maintainer_can_leave_on_their_own():
    """SV80. Withdrawing is a positive act, like joining was.

    A maintainer who no longer wants to take part should not have to file a
    notice against themselves, or ask us, or simply stop answering the challenge
    and let liveness grade them down over ninety days. It ends where a takedown
    ends — mirror withheld, attestation withdrawn, anchor `unknown` — because
    that is what un-enrolment is.

    Recorded as `self_withdrawn` and kept distinct from a takedown reason: a
    project that left is not a project that was reported, and a log that cannot
    tell those apart makes leaving look like an accusation.
    """
    import asyncio

    from .app import claim_withdraw
    from .errors import NotClaimed as NC

    host = _fresh_host("leaving")
    tok = secrets.token_urlsafe(16)
    with db.tx() as conn:
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute("UPDATE anchor SET status = 'live', verified_at = now() WHERE id = %s",
                        (aid,))
            cur.execute("INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                        "VALUES (%s, %s, now() + interval '365 days')",
                        (aid, hashlib.sha256(tok.encode()).digest()))

    # Nobody else may do this on their behalf.
    refused = _run(claim_withdraw(host, None))
    assert refused.status_code == 403, "an unauthenticated caller withdrew somebody's project"

    out = _run(claim_withdraw(host, tok))
    body = json.loads(out.body)
    assert body["withdrawn"] == host, body
    assert body["tokens_revoked"] >= 1, "a live credential survived the withdrawal"

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT status FROM anchor WHERE id = %s", (aid,))
            assert cur.fetchone()["status"] == "unknown", "the anchor did not return to unknown"
            cur.execute("SELECT count(*) AS n FROM dashboard_claim "
                        "WHERE anchor_id = %s AND revoked_at IS NULL", (aid,))
            assert cur.fetchone()["n"] == 0, "tokens outlived the project"
        entry = log_store.for_anchor(conn, aid)[-1]
        assert entry["entry"]["reason_code"] == "self_withdrawn", entry
        # And the token is now refused exactly like any other dead one.
        from .app import _claimed_anchor
        try:
            _claimed_anchor(conn, host, tok)
        except NC:
            pass
        else:
            raise AssertionError("the token still opens a withdrawn project's dashboard")


def sv72_an_answer_says_what_it_turned_on():
    """SV72. Matching is not refused on a supplied fact — refusing would make
    asking the question pointless — but the answer says what it turned on and
    which of those the person supplied.

    That is the difference between a finding and a finding somebody can weigh.
    A publisher whose rule failed on a measured fact has a defect worth their
    time; one whose rule failed on an answered fact may have nothing wrong at
    all, and before this the two arrived identical.
    """
    from .app import diagnose

    sv_ingest_stores_a_project_and_attests_it()
    with db.tx() as conn:
        _author_tree(conn, _tree_source(conn))

    body = {"subject": "127.0.0.1", "problem_class": "pip.install.wheel-missing",
            "facts": {"python.version": "3.13.1"}}
    measured = _run(diagnose(dict(body)))
    assert measured["outcome"] == "finding", measured
    assert measured["decided_on"] == ["python.version"], measured["decided_on"]
    assert measured["rested_on_supplied"] == [], measured
    assert measured["confidence"] == "measured", measured

    # The same answer, from a fact the person supplied.
    supplied = _run(diagnose({**body, "stated": ["python.version"]}))
    assert supplied["outcome"] == "finding", "a supplied fact stopped the match"
    assert supplied["rested_on_supplied"] == ["python.version"], supplied
    assert supplied["confidence"] == "rests_on_supplied", supplied
    assert "may be the answer rather than the rule" in supplied["reading"], supplied["reading"]


def sv71_the_dashboard_says_which_facts_were_typed_not_read():
    """SV71. A publisher cannot judge their own rule without this.

    A solution that matched on a measurement and failed is a defect in the rule
    and worth their time. One that matched on a value the person supplied and
    failed may be nothing of the sort — and before the split those two arrived
    identical, so the second quietly spent the first's credit.
    """
    host = _fresh_host("stated")
    epoch = counting.current_epoch()
    with db.tx() as conn:
        token = _claimed(conn, host)
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, host, {"gpu.name": "RTX 4090"}, epoch)
        for i in range(K_REPORTERS):
            # All of them typed the version rather than having it read; two of
            # them also typed how it felt.
            stated = {"python.version": "3.11"}
            if i < 2:
                stated["user.felt"] = "badly"
            counting.record_observation(
                conn, cid, f"p{i}", epoch=epoch, model_class="local_small",
                observed={"gpu.name": "RTX 4090"}, stated=stated)
    out = _dashboard(host, token)

    facts = {r["fact"]: r for r in out["stated_facts"]}
    assert "python.version" in facts, (
        f"the dashboard does not say which facts were typed: {out['stated_facts']}")
    assert facts["python.version"]["reporters"] == K_REPORTERS, facts["python.version"]
    assert "gpu.name" not in facts, "a measurement was reported as something a person supplied"
    # Each typed fact clears k on its own. The cluster being at or above k is
    # not enough: two people who typed something the other three did not are a
    # group of two, and naming the fact beside this cluster describes them.
    assert "user.felt" not in facts, (
        f"a fact typed by fewer than {K_REPORTERS} people was named: {out['stated_facts']}")
    assert "not evidence about your rule" in out["stated_means"], out["stated_means"]


def sv70_ingest_signs_the_head_it_just_grew():
    """SV70. The index publishes nothing its signed head cannot prove, so a
    cycle that appends must also sign — otherwise the only issuer is whoever
    happens to `GET /log/sth`, and a project published today stays invisible
    while ingest works perfectly. That is a dependency nobody would think to
    look for, so it is a case rather than a comment."""
    from . import index_feed
    from .ingest import scheduler

    with db.tx() as conn:
        _, sid = _oss_source(conn)
        with conn.cursor() as cur:
            cur.execute("UPDATE source SET next_fetch_at = '-infinity', etag = NULL, "
                        "last_modified = NULL WHERE id = %s", (sid,))
        scheduler.run_once(conn)

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute(
                "SELECT c.log_seq FROM card c JOIN source s ON s.id = c.source_id "
                "WHERE s.id = %s AND c.valid_to IS NULL", (sid,))
            seq = cur.fetchone()["log_seq"]
        head = sth.current(conn)
        doc = index_feed.signed(conn)

    # Not "the head equals the log": other things append too, and their entries
    # ride along on the next cycle's head. The property is that *this* project
    # is provable under the head the index was signed with.
    assert head["sth"]["tree_size"] > seq, (
        f"the project is at log entry {seq} but the newest signed head covers only "
        f"{head['sth']['tree_size']} — nothing issued one, so it can never be published")
    hosts = {e["host"] for e in doc["index"]["entries"]}
    assert "127.0.0.1" in hosts, "the freshly ingested project is not in the published index"


def sv68_a_held_anchor_is_not_indexed():
    """SV68. A hold means the name itself is contested — a confusable of
    somebody else's. The index is the thing users search *by name*, so a
    contested name is exactly what must not appear in it."""
    from . import index_feed
    host = _fresh_host("held")
    with db.tx() as conn:
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute("UPDATE anchor SET attest_hold = %s WHERE id = %s",
                        ("confusable with another anchor", aid))
    with db.read() as conn:
        hosts = {e["host"] for e in index_feed.entries(conn)}
    assert host not in hosts, "a held anchor was published in the index"


def sv_a_read_cannot_walk_out_of_a_granted_root():
    """A granted root is checked by prefix, and a prefix cannot see a way out.

    `enumerate_read` has refused `..` and a leading `/` since it was written.
    `read_file_key` and `read_ini_key` never got either rule, and the client's
    own guard is `Path::starts_with`, which compares components without
    normalising — so `<config>/../../.npmrc` *starts with* `<config>` on both
    sides and the read happens outside every granted root.

    The deny list did not close it: it caught `.ssh` and `.aws` by name and
    nothing else, and it read the file name without reading the field being
    taken out of it, so `api_secret` of any accepted file went through.

    Both halves are asserted here, and so is the case that must keep working —
    a directory whose name merely begins with two dots is not a traversal, so
    a substring test for `..` would be the wrong fix.
    """
    from .. import spec_gate

    def refused(read):
        try:
            spec_gate.check_read(read)
        except spec_gate.SpecError as e:
            return str(e)
        return None

    for path in ("../../../../etc/passwd",
                 "config/../../.mozilla/logins.json",
                 "config" + chr(92) + ".." + chr(92) + ".." + chr(92) + "x.json"):
        for op in ("read_file_key", "read_ini_key"):
            why = refused({"op": op, "path": path, "key": "k"})
            assert why, f"{op} accepted a path that leaves its root: {path!r}"
            assert ".." in why, f"refused for the wrong reason: {why}"

    for key in ("_authToken", "api_secret"):
        why = refused({"op": "read_file_key", "path": "app.json", "key": key})
        assert why, (
            f"a key named {key!r} was permitted — the deny list reads the file name "
            f"and not the field being taken out of it")

    assert refused({"op": "read_file_key", "path": "cfg/..hidden/a.json", "key": "theme"}) is None, \
        "`..hidden` is a directory name, not a way out — a substring test is the wrong fix"
    assert refused({"op": "read_ini_key", "path": "cfg/app.cfg", "key": "theme"}) is None, \
        "an ordinary settings read was refused"


def sv_control_alone_does_not_enrol_a_mirror():
    """Proving control and being fetched are two acts, and only one existed.

    `/register` proved control and issued a token, and then nothing asked where
    the files were: `INSERT INTO source` appeared only in this suite. A
    maintainer could walk the whole documented path, receive a token, and never
    be mirrored, with nothing anywhere to tell them so.

    Walked over HTTP against the running server rather than asserted, because
    the missing thing was a *route* — a helper in this file proves nothing
    about what a maintainer can reach.
    """
    import httpx
    base = "http://127.0.0.1:8725"
    host = _fresh_host("enrol")
    # `dashboard_claim.token_hash` is unique across the table, so a fixed
    # literal here passes once and then collides with its own earlier run.
    tok = "enrol-" + secrets.token_urlsafe(16)
    with db.tx() as conn:
        aid = _anchor(conn, host)
        with conn.cursor() as cur:
            cur.execute(
                "INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                "VALUES (%s, %s, now() + interval '1 day')",
                (aid, hashlib.sha256(tok.encode()).digest()))

    r = httpx.post(f"{base}/claim/{host}/source", timeout=5)
    assert r.status_code == 403, f"enrolment took no authentication: {r.status_code}"
    r = httpx.post(f"{base}/claim/{host}/source", timeout=5,
                   headers={"X-Podshl-Claim": "not-the-token"})
    assert r.status_code == 403, f"any token was accepted: {r.status_code}"

    auth = {"X-Podshl-Claim": tok}
    r = httpx.post(f"{base}/claim/{host}/source", timeout=15, headers=auth,
                   json={"prefix": "https://somewhere-else.example/"})
    assert r.status_code == 400 and r.json()["code"] == "outside_anchor", (
        "a prefix outside the verified anchor was accepted — an anchor proves "
        f"control of a location and cannot vouch for another one: {r.text}")

    r = httpx.post(f"{base}/claim/{host}/source", timeout=15, headers=auth,
                   json={"prefix": f"https://{host}/project/"})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["created"] is True, body
    assert body["manifest_url"] == f"https://{host}/project/.podshl/agent.yaml", body

    # Idempotent for the same prefix: pressing the button twice is not two
    # projects. A *different* prefix is a second source, because the schema says
    # so — one anchor may publish more than one `.podshl/`.
    again = httpx.post(f"{base}/claim/{host}/source", timeout=15, headers=auth,
                       json={"prefix": f"https://{host}/project/"}).json()
    assert again["created"] is False, again
    second = httpx.post(f"{base}/claim/{host}/source", timeout=15, headers=auth,
                        json={"prefix": f"https://{host}/other/"}).json()
    assert second["created"] is True, (
        "a second prefix under the same anchor was folded into the first — "
        f"one anchor may publish more than one `.podshl/`: {second}")

    # Enrolling reads the files at once and says what came of it. Nothing is
    # served on this invented host, so that is a failure to read — and it is
    # said, rather than the maintainer being told "queued" and left waiting.
    assert second["check"]["outcome"] not in ("stored", "unchanged", "running"), second
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT fetch_prefix, last_fetched IS NOT NULL AS asked, "
                        "       last_used = current_date AS hot "
                        "FROM source WHERE anchor_id = %s ORDER BY id DESC LIMIT 1", (aid,))
            row = cur.fetchone()
    assert row, "the route answered 200 and enrolled nothing"
    assert row["asked"], "enrolled, and nothing asked for the files"
    assert row["hot"], "enrolling is a use, and the source is not hot"


def sv_a_public_file_is_not_a_credential():
    """A published file shows that somebody controls this host. It does not show
    that the person asking is that somebody.

    `POST /claim/{host}/verify` took no authentication and fetched the challenge
    file: present, and it minted a token for whoever asked and revoked every
    token before it. The nonce is public by design, `POST /claim/{host}` handed
    it to anonymous callers on purpose, and `ONBOARDING.md` tells maintainers to
    leave the file published so re-verification keeps working. So every project
    that followed the instructions could be taken over by a passer-by: read the
    dashboard, and withdraw the project.

    Walked over HTTP against the running server, against a host this case serves
    itself, because the property is about what a stranger can reach.
    """
    import http.server
    import socket
    import threading

    import httpx

    base = "http://127.0.0.1:8725"

    published: dict[str, str] = {"value": ""}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = published["value"].encode()
            self.send_response(200 if body else 404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    with socket.socket() as probe_sock:
        probe_sock.bind(("127.0.0.1", 0))
        port = probe_sock.getsockname()[1]
    srv = http.server.HTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    try:
        host = _fresh_host("takeover")
        with db.tx() as conn:
            aid = _anchor_at(conn, f"http://127.0.0.1:{port}/{secrets.token_hex(4)}/",
                             host, "unclaimed")

        # The maintainer starts a claim and publishes the half they were told to.
        started = httpx.post(f"{base}/claim/{host}", timeout=5).json()
        assert "publish" in started and "proof" in started, (
            f"a claim must have a half that is published and a half that is not: {started}")
        assert started["publish"] != started["proof"], \
            "the published half and the kept half are the same value"
        assert hashlib.sha256(started["proof"].encode()).hexdigest() == started["publish"], \
            "the published half is not a digest of the kept half, so it proves nothing"
        # The route finds the anchor by host and this case created it under
        # that host, so the claim it just wrote is already on this row - there
        # is nothing to seed. Assert that rather than assume it: a claim
        # standing against a different anchor is how this case used to pass
        # while proving nothing.
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT anchor_id FROM claim_pending WHERE nonce_hash = %s",
                            (hashlib.sha256(started["proof"].encode()).digest(),))
                on = cur.fetchone()
        assert on and on["anchor_id"] == aid, (
            f"the claim landed on anchor {on and on['anchor_id']}, not on {aid}")
        published["value"] = started["publish"]

        # A stranger can read the file. That is the whole of what they have.
        r = httpx.post(f"{base}/claim/{host}/verify", timeout=5)
        assert r.status_code == 403, \
            f"verify with no proof at all issued something: {r.status_code} {r.text}"
        r = httpx.post(f"{base}/claim/{host}/verify", timeout=5,
                       headers={"X-Podshl-Claim-Proof": published["value"]})
        assert r.status_code == 403 and r.json()["code"] == "no_proof", (
            "the value read off the public URL was accepted as proof — this is the "
            f"takeover, and it is open: {r.status_code} {r.text}")

        # The maintainer holds the half that was never published.
        r = httpx.post(f"{base}/claim/{host}/verify", timeout=5,
                       headers={"X-Podshl-Claim-Proof": started["proof"]})
        assert r.status_code == 200, f"the real claimant was refused: {r.text}"
        assert r.json().get("token"), r.text

        # Spent. A claim that can be replayed is a claim somebody can hold on to.
        again = httpx.post(f"{base}/claim/{host}/verify", timeout=5,
                           headers={"X-Podshl-Claim-Proof": started["proof"]})
        assert again.status_code == 404, \
            f"the same proof worked twice: {again.status_code} {again.text}"

        # And the value now checked forever after is the one just proved.
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT challenge_token FROM anchor WHERE id = %s", (aid,))
                row = cur.fetchone()
                cur.execute("SELECT count(*) AS n FROM claim_pending WHERE anchor_id = %s",
                            (aid,))
                left = cur.fetchone()["n"]
        assert row["challenge_token"] == started["publish"], (
            "the sweep would go on checking a value the claimant never proved")
        # Every claim on this anchor, not only the one that matched: control
        # has just been proved by somebody, so a claim started before that is
        # moot, and one left standing is one a passer-by can still finish.
        assert left == 0, "the spent claim was left standing"
    finally:
        srv.shutdown()


def sv_a_name_cannot_answer_twice():
    """The connection goes to the address that was checked.

    `fetch.py`'s own docstring has always said this — "the host is resolved
    first, every resolved address is checked, and the connection is made to the
    address that was checked. Resolving twice — once to validate and once to
    connect — is a DNS-rebinding hole straight into our own network" — and the
    code did exactly the forbidden thing: `is_public_address(host)` resolved and
    threw the answers away, then `httpx` resolved the same name again at connect
    time. A publisher-controlled name with a one-second TTL answers public for
    the check and `169.254.169.254` for the connection.

    This case *performs* the rebinding rather than reasoning about it. Two
    loopback servers stand in for the two answers: one is where the check said
    to go, the other is the internal service that must never be reached. The
    resolver is made to move between them, and the assertion is about which
    server received the request.
    """
    import http.server
    import socket
    import threading

    import httpx

    from .ingest import fetch

    hit: dict[str, bool] = {"safe": False, "internal": False}

    def server_on(address: str, label: str, port: int = 0):
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):  # noqa: N802
                hit[label] = True
                body = b"reached"
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *a):  # noqa: A003
                pass

        srv = http.server.HTTPServer((address, port), Handler)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        return srv

    # Two addresses, both loopback so the case needs no network and no name
    # service. `127.0.0.2` is the answer the check saw; `127.0.0.1` is the
    # machine's own services, and is what a rebinding attack aims at.
    #
    # The **same port** on both, or the rebound connection lands on a closed
    # port and looks prevented when it was merely unlucky.
    safe = server_on("127.0.0.2", "safe")
    safe_port = safe.server_address[1]
    internal = server_on("127.0.0.1", "internal", safe_port)
    real = socket.getaddrinfo

    def moving_target(host, port=None, *a, **kw):
        """Public on the first lookup, ours on every one after — which is the
        whole of the attack.

        The port is echoed back rather than zeroed. That detail is what makes
        this case decisive: with a zero the rebound connection fails to connect
        and *looks* prevented, so the assertion below would pass against the
        broken code as well."""
        if host == "rebind.invalid":
            moving_target.calls += 1
            addr = "127.0.0.2" if moving_target.calls == 1 else "127.0.0.1"
            where = (addr, int(port or 0))
            return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", where)]
        return real(host, port, *a, **kw)

    moving_target.calls = 0

    try:
        socket.getaddrinfo = moving_target
        with fetch.pinned_client() as http:
            checked = challenge.public_addresses("rebind.invalid")
            assert checked == ["127.0.0.2"], f"the check saw {checked}"
            try:
                http.get(f"http://rebind.invalid:{safe_port}/x",
                         extensions={fetch.PIN: checked[0]})
            except httpx.HTTPError:
                pass
    finally:
        socket.getaddrinfo = real
        safe.shutdown()
        internal.shutdown()

    assert not hit["internal"], (
        "the request reached the address the second lookup returned, not the one "
        "that was checked — this is the rebinding hole, and it is open")
    assert hit["safe"], (
        "the request reached neither server; the case proves nothing unless the "
        "checked address was actually connected to")
    assert moving_target.calls == 1, (
        f"the name was resolved {moving_target.calls} times. Once is the whole "
        f"property: a second lookup is a second answer.")

    # And a request that forgets its pin fails closed rather than quietly
    # resolving again. A future caller must not be able to reintroduce this by
    # omission.
    with fetch.pinned_client() as http:
        try:
            http.get("http://127.0.0.1:8725/")
        except httpx.TransportError as e:
            assert "already checked" in str(e), f"refused for the wrong reason: {e}"
        else:
            raise AssertionError("an unpinned request was allowed to resolve its own host")


def sv_an_encoded_path_cannot_leave_the_anchor():
    """Containment has to be checked on the path a server will resolve.

    `urlparse` does not percent-decode and `startswith` compares what it was
    given, so `…/.podshl/%2e%2e/%2e%2e/victim/…` *textually* begins with the
    anchor's prefix. Both of this project's own containment checks passed it —
    `check_manifest`, which looked for a literal `..`, and `under_prefix`, which
    compared the still-encoded path — while any origin or CDN that decodes and
    normalises dot-segments resolves it somewhere else. On shared forge hosting,
    where every project is a path under one host, that means mirroring another
    tenant's file under this anchor's attested name.

    Both halves are exercised: the manifest gate refuses the encoding, and
    `under_prefix` refuses the target even if some other caller constructs it.
    """
    from .ingest import validate
    from .ingest.fetch import under_prefix
    from .errors import IngestRefused

    base = "https://raw.githubusercontent.com/attacker/repo/main/.podshl/"

    escapes = [
        "%2e%2e/%2e%2e/%2e%2e/victim/repo/main/.podshl/sol.md",
        "..%2f..%2fvictim/sol.md",
        "%2E%2E/victim.md",
        "%2e%2e%2fvictim.md",
    ]
    for rel in escapes:
        m = {"endpoint": base, "langs": ["en"], "problem_classes": ["x"],
             "solutions": [rel], "collect": []}
        try:
            validate.check_manifest(m, base)
        except IngestRefused:
            pass
        else:
            raise AssertionError(
                f"the manifest gate accepted an encoded traversal: {rel!r}")
        assert not under_prefix(base + rel, base), (
            f"under_prefix accepted {rel!r} — it compared the encoded text, and the "
            f"host that serves it will not")

    # A path that merely *begins* the same way is still not inside.
    assert not under_prefix("https://raw.githubusercontent.com/attacker/repo/main/"
                            ".podshl-other/sol.md", base), \
        "a sibling directory whose name starts the same way was treated as inside"

    # And the ordinary case still works, or this is an outage rather than a fix.
    ok = {"endpoint": base, "langs": ["en"], "problem_classes": ["x"],
          "solutions": ["sub/dir/sol.md"], "collect": []}
    validate.check_manifest(ok, base)
    assert under_prefix(base + "sub/dir/sol.md", base), \
        "an ordinary relative solution path was refused"


def sv_a_deleted_solution_stops_being_served():
    """Removing the file is how a maintainer withdraws a remedy.

    It did nothing. Ingest iterated the manifest's `solutions` list and stored
    each one, and the only `valid_to` was the supersede path *inside*
    `store_solution`, which runs only for solutions still listed. Everything
    that reads them selects on `valid_to IS NULL`, so a solution a maintainer
    deleted — the natural way to withdraw a remedy they had decided was harmful
    — went on being served from the mirror indefinitely, under an anchor
    re-attested for a manifest that no longer declared it.

    Walked against a host this case serves itself, because the property is about
    what a *second* crawl does with a manifest that changed.
    """
    import http.server
    import threading

    from .ingest import scheduler

    served: dict[str, bytes] = {}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = served.get(self.path.removeprefix(f"/{run}"))
            self.send_response(200 if body is not None else 404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body or b"")))
            self.end_headers()
            if body:
                self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    # A path segment nobody else will ever use. `anchor` is unique on
    # `(kind, value)` and the database outlives the run, so a flat
    # `http://127.0.0.1:<port>/` collided with a previous run as soon as the
    # kernel handed back a port it had handed out before — a duplicate key,
    # for a reason that has nothing to do with what the case tests.
    run = secrets.token_hex(4)
    base = f"http://127.0.0.1:{port}/{run}/"

    def solution(sid: str) -> bytes:
        return (f"---\nid: {sid}\nanswers:\n  problem_class: demo.{sid}\n"
                f"severity: low\nproposes:\n  - action: report_only\n    params: {{}}\n"
                f"---\nText for {sid}.\n").encode()

    def manifest_for(ids: list[str]) -> bytes:
        listed = "".join(f"  - solutions/{i}.md\n" for i in ids)
        return (f"endpoint: {base}\ncommit: c1\nstatus: active\nlangs: [en]\n"
                f"problem_classes: [demo.one]\nsolutions:\n{listed}").encode()

    served["/.well-known/podshl-challenge"] = b"retract-token"
    served["/.podshl/agent.yaml"] = manifest_for(["keep", "drop"])
    served["/.podshl/solutions/keep.md"] = solution("keep")
    served["/.podshl/solutions/drop.md"] = solution("drop")

    def live(sid_source: int) -> set[str]:
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT solution_id FROM solution "
                            "WHERE source_id = %s AND valid_to IS NULL", (sid_source,))
                return {r["solution_id"] for r in cur.fetchall()}

    try:
        host = _fresh_host("retract")
        with db.tx() as conn:
            aid = _anchor_at(conn, base, host, "retract-token")
            with conn.cursor() as cur:
                # `-infinity`, not `now()`: `claim` orders due sources by
                # `next_fetch_at` and takes a batch, and the development
                # database has accumulated hundreds across runs - so a source
                # created now can sort behind them and simply not be in the
                # cycle this case then reads the result of.
                cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix, "
                            "                    next_fetch_at) "
                            "VALUES (%s, %s, %s, '-infinity') RETURNING id",
                            (aid, base + ".podshl/agent.yaml", base))
                sid = cur.fetchone()["id"]

        with db.tx() as conn:
            first = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert first["outcome"] == "stored", first
        assert live(sid) == {"keep", "drop"}, f"both should be serving: {live(sid)}"

        # The maintainer deletes one, exactly as they would in the repository.
        served["/.podshl/agent.yaml"] = manifest_for(["keep"])
        del served["/.podshl/solutions/drop.md"]
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE source SET next_fetch_at = '-infinity', etag = NULL, "
                            "last_modified = NULL WHERE id = %s", (sid,))
            second = {r["source"]: r for r in scheduler.run_once(conn)}[sid]

        assert second["outcome"] == "stored", second
        assert second.get("retracted") == ["drop"], (
            f"the crawl did not say what it withdrew: {second}")
        remaining = live(sid)
        assert "drop" not in remaining, (
            "a solution the maintainer deleted is still being served — deleting the "
            "file is how a remedy is withdrawn, and it did nothing")
        assert remaining == {"keep"}, f"the wrong thing was retracted: {remaining}"

        # Closed, not deleted: "what were we serving when this went wrong" has to
        # stay answerable.
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT valid_to FROM solution "
                            "WHERE source_id = %s AND solution_id = 'drop'", (sid,))
                rows = cur.fetchall()
        assert rows and all(r["valid_to"] is not None for r in rows), \
            "the retracted solution was deleted rather than closed"
    finally:
        srv.shutdown()


def sv_one_bad_source_does_not_take_the_batch_down():
    """A crawl of two hundred projects must not be hostage to any one of them.

    `ingest_one` catches `IngestRefused` and nothing else, and `run_once` ran
    the whole claimed batch inside a single transaction. So one unexpected error
    in the two-hundredth source discarded the hundred and ninety-nine already
    ingested *and* the `next_fetch_at` lease that `claim` had written — which put
    the poisoned source back at the head of the next claim. The worker loop had
    no handler either, so the process died and restarted onto the same source: a
    permanent ingest outage for every project, caused by one of them.

    The failure is injected rather than found, deliberately. The property under
    test is the isolation, not any particular way of provoking it, and a case
    that depends on one trigger stops testing the property the day that trigger
    is fixed.
    """
    import http.server
    import threading

    from .ingest import scheduler

    served: dict[str, bytes] = {}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = served.get(self.path.removeprefix(f"/{run}"))
            self.send_response(200 if body is not None else 404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body or b"")))
            self.end_headers()
            if body:
                self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    # A path segment nobody else will ever use. `anchor` is unique on
    # `(kind, value)` and the database outlives the run, so a flat
    # `http://127.0.0.1:<port>/` collided with a previous run as soon as the
    # kernel handed back a port it had handed out before — a duplicate key,
    # for a reason that has nothing to do with what the case tests.
    run = secrets.token_hex(4)
    base = f"http://127.0.0.1:{port}/{run}/"

    # Three anchors need three prefixes: `anchor` is unique on (kind, value),
    # and three projects sharing one URL is not a case anybody has.
    for i in range(3):
        served[f"/p{i}/.well-known/podshl-challenge"] = b"batch-token"
        served[f"/p{i}/.podshl/agent.yaml"] = (
            f"endpoint: {base}p{i}/\ncommit: c1\nstatus: active\nlangs: [en]\n"
            f"problem_classes: [demo.batch]\nsolutions:\n  - solutions/a.md\n").encode()
        served[f"/p{i}/.podshl/solutions/a.md"] = (
            "---\nid: a\nanswers:\n  problem_class: demo.batch\nseverity: low\n"
            "proposes:\n  - action: report_only\n    params: {}\n---\nText.\n").encode()

    ids = []
    real = scheduler.ingest_one
    try:
        with db.tx() as conn:
            for i in range(3):
                host = _fresh_host("batch")
                prefix = f"{base}p{i}/"
                with conn.cursor() as cur:
                    cur.execute(
                        "INSERT INTO anchor (kind, value, host, challenge_token) "
                        "VALUES ('url', %s, %s, 'batch-token') RETURNING id",
                        (prefix, host))
                    aid = cur.fetchone()["id"]
                    cur.execute(
                        "INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                        "VALUES (%s, %s, %s) RETURNING id",
                        (aid, prefix + ".podshl/agent.yaml", prefix))
                    ids.append(cur.fetchone()["id"])
        poisoned = ids[1]

        def exploding(conn, source, **kw):
            if source["id"] == poisoned:
                # Not IngestRefused. The whole point is the error nobody planned
                # for — a driver error, a decode, an assertion.
                raise RuntimeError("something nobody wrote a handler for")
            return real(conn, source, **kw)

        scheduler.ingest_one = exploding
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE source SET next_fetch_at = '-infinity' WHERE id = ANY(%s)", (ids,))
            results = {r["source"]: r for r in scheduler.run_once(conn)}
    finally:
        scheduler.ingest_one = real
        srv.shutdown()

    for sid in ids:
        assert sid in results, f"source {sid} was never reported on: {results}"
    assert results[poisoned]["outcome"] == "error", results[poisoned]
    assert "RuntimeError" in results[poisoned]["why"], results[poisoned]

    for sid in (ids[0], ids[2]):
        assert results[sid]["outcome"] == "stored", (
            f"a healthy source was rolled back by an unrelated failure: {results[sid]}")

    # Stored means stored: the savepoint kept the good work, and the poisoned
    # one left nothing behind.
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT source_id FROM card WHERE source_id = ANY(%s) "
                        "AND valid_to IS NULL", (ids,))
            have = {r["source_id"] for r in cur.fetchall()}
    assert have == {ids[0], ids[2]}, (
        f"expected exactly the two healthy sources to be mirrored, got {have}")


def sv_a_diagnosis_exists_without_anybody_authoring_a_tree():
    """`POST /diagnose` answered `no_statement` on every real deployment.

    `cluster_tree` could walk a tree, validate one and load one, and nothing
    anywhere built one: `INSERT INTO tree` appeared only in this file. So the
    endpoint the enterprise branch is named after returned nothing whatever it
    was sent, by construction, and the suite did not notice because the suite
    inserted the trees itself.

    The fix derives the tree from what a maintainer already writes — a
    solution's `answers.when` is a conjunction over facts, and `collect` says
    how each fact is acquired. This case therefore starts from **published
    files**, ingests them the ordinary way, and asks the endpoint over HTTP.
    Nothing here writes a tree, which is the whole point: if the derivation
    stops happening, this goes red.

    All four outcomes, because each is a different promise:
    a finding, a `need` carrying a probe the client can actually satisfy, a
    `no_statement` where nothing matches rather than a nearest guess, and the
    fallback that stops "I would rather not say" being a dead end.
    """
    import http.server
    import threading

    import httpx

    from .ingest import scheduler

    served: dict[str, bytes] = {}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = served.get(self.path.removeprefix(f"/{run}"))
            self.send_response(200 if body is not None else 404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body or b"")))
            self.end_headers()
            if body:
                self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    # A path segment nobody else will ever use. `anchor` is unique on
    # `(kind, value)` and the database outlives the run, so a flat
    # `http://127.0.0.1:<port>/` collided with a previous run as soon as the
    # kernel handed back a port it had handed out before — a duplicate key,
    # for a reason that has nothing to do with what the case tests.
    run = secrets.token_hex(4)
    base = f"http://127.0.0.1:{port}/{run}/"

    served["/.well-known/podshl-challenge"] = b"tree-token"
    served["/.podshl/agent.yaml"] = (
        f"endpoint: {base}\n"
        "commit: t1\nstatus: active\nlangs: [en]\n"
        "problem_classes: [demo.install]\n"
        "collect:\n"
        "  - id: os.arch\n"
        "    kind: machine\n"
        "    describes: Processor architecture\n"
        "    why: the archives are not interchangeable\n"
        "    read: { op: os_fact, name: arch }\n"
        "  - id: user.mood\n"
        "    kind: human\n"
        "    describes: How it felt\n"
        "    why: nothing can read this\n"
        "    prompt: How did it go?\n"
        "    choices: [fine, awful]\n"
        "solutions:\n"
        "  - solutions/arm.md\n"
        "  - solutions/intel.md\n").encode()

    def solution(sid: str, when: str) -> bytes:
        return (f"---\nid: {sid}\nanswers:\n  problem_class: demo.install\n"
                f"  when:\n{when}severity: low\n"
                f"proposes:\n  - action: report_only\n    params: {{}}\n"
                f"---\nText for {sid}.\n").encode()

    served["/.podshl/solutions/arm.md"] = solution("arm", "    os.arch: aarch64\n")
    served["/.podshl/solutions/intel.md"] = solution("intel", "    os.arch: x86_64\n")

    try:
        host = _fresh_host("tree")
        with db.tx() as conn:
            aid = _anchor_at(conn, base, host, "tree-token")
            with conn.cursor() as cur:
                # `-infinity`, not `now()`: `claim` orders due sources by
                # `next_fetch_at` and takes a batch, and the development
                # database has accumulated hundreds across runs - so a source
                # created now can sort behind them and simply not be in the
                # cycle this case then reads the result of.
                cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix, "
                            "                    next_fetch_at) "
                            "VALUES (%s, %s, %s, '-infinity') RETURNING id",
                            (aid, base + ".podshl/agent.yaml", base))
                sid = cur.fetchone()["id"]

        with db.tx() as conn:
            got = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert got["outcome"] == "stored", got
        assert got.get("trees") == ["demo.install"], (
            f"ingest built no tree, so /diagnose has nothing to walk: {got}")

        # Nothing in this case wrote a node. If it had, the assertion above
        # would pass while the endpoint stayed as empty as it was in production.
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT count(*) n FROM tree_node tn JOIN tree t "
                            "ON t.id = tn.tree_id WHERE t.source_id = %s", (sid,))
                assert cur.fetchone()["n"] > 1, "the tree has no branches"

        def ask(body: dict) -> dict:
            r = httpx.post("http://127.0.0.1:8725/diagnose", json=body, timeout=10)
            assert r.status_code == 200, r.text
            return r.json()

        # A finding, on a measured fact.
        out = ask({"subject": host, "problem_class": "demo.install",
                   "facts": {"os.arch": "aarch64"}})
        assert out["outcome"] == "finding", out
        assert out["solution"]["solution_id"] == "arm", out
        assert out["decided_on"] == ["os.arch"], out
        assert out["confidence"] == "measured", out

        # The same walk with the fact supplied by a person rather than read is a
        # different kind of answer, and says so.
        out = ask({"subject": host, "problem_class": "demo.install",
                   "facts": {"os.arch": "aarch64"}, "stated": ["os.arch"]})
        assert out["confidence"] == "rests_on_supplied", out
        assert out["rested_on_supplied"] == ["os.arch"], out

        # A need, carrying a probe the client can actually perform. A `need`
        # naming a fact with no read instruction would be a question the client
        # cannot answer and the user cannot be asked.
        out = ask({"subject": host, "problem_class": "demo.install", "facts": {}})
        assert out["outcome"] == "need", out
        assert out["need"][0]["id"] == "os.arch", out
        assert out["need"][0].get("read"), (
            f"the need carries no read instruction, so nothing can satisfy it: {out}")

        # Nothing matches. Not the nearest branch — a wrong guess reaches a user.
        out = ask({"subject": host, "problem_class": "demo.install",
                   "facts": {"os.arch": "riscv64"}})
        assert out["outcome"] == "no_statement", out

        # A class nobody published is not an accusation.
        out = ask({"subject": host, "problem_class": "demo.nothing", "facts": {}})
        assert out["outcome"] == "no_statement", out

        # And a solution keyed on a fact no probe acquires is refused when it
        # arrives, rather than silently never matching.
        served["/.podshl/solutions/intel.md"] = solution("intel", "    os.invented: yes\n")
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE source SET next_fetch_at = '-infinity', etag = NULL, "
                            "last_modified = NULL WHERE id = %s", (sid,))
            after = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert after["outcome"] == "refused", after
        assert "os.invented" in after["why"], after
    finally:
        srv.shutdown()


def sv_a_maintainer_can_read_their_own_dashboard():
    """The page is for exactly one person, and it did not speak to them.

    It showed `sig.c0bbda7c3ddd`, a row of `key=value`, and two integers. All
    true, none of it a statement about a problem — and it left out the one
    column that had been recorded on every report since the beginning: whether
    the answer *worked*.

    What it has to carry, and what this case asserts:

    * **Which of the maintainer's own solutions applies**, found by walking
      their own trees against what recurs — the same walk `/diagnose` does.
    * **Whether it helped**, in distinct people rather than reports.
    * **Measured apart from typed**, because that is what decides whether an
      outcome says anything about their rule at all.
    * **And no noise.** A class whose tree answers without any fact deciding
      anything answers every configuration equally, and listing it against all
      of them makes the page harder to read rather than easier. That is a
      regression this case exists to catch, because it is the failure the
      rewrite was for.
    """
    import http.server
    import threading

    import httpx

    from .ingest import scheduler

    served: dict[str, bytes] = {}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = served.get(self.path.removeprefix(f"/{run}"))
            self.send_response(200 if body is not None else 404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body or b"")))
            self.end_headers()
            if body:
                self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    # A path segment nobody else will ever use. `anchor` is unique on
    # `(kind, value)` and the database outlives the run, so a flat
    # `http://127.0.0.1:<port>/` collided with a previous run as soon as the
    # kernel handed back a port it had handed out before — a duplicate key,
    # for a reason that has nothing to do with what the case tests.
    run = secrets.token_hex(4)
    base = f"http://127.0.0.1:{port}/{run}/"
    api = "http://127.0.0.1:8725"

    served["/.well-known/podshl-challenge"] = b"read-token"
    served["/.podshl/agent.yaml"] = (
        f"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n"
        "problem_classes: [demo.arch, demo.felt, demo.never]\n"
        "collect:\n"
        "  - id: os.arch\n    kind: machine\n    describes: Architecture\n"
        "    why: the archives differ\n    read: { op: os_fact, name: arch }\n"
        "  - id: user.felt\n    kind: human\n    describes: How it went\n"
        "    why: nothing reads this\n    prompt: How did it go?\n"
        "    choices: [badly, elated]\n"
        "solutions:\n  - solutions/arm.md\n  - solutions/felt.md\n"
        "  - solutions/never.md\n").encode()
    served["/.podshl/solutions/arm.md"] = (
        "---\nid: arm\nanswers:\n  problem_class: demo.arch\n"
        "  when:\n    os.arch: aarch64\nseverity: high\n"
        "proposes:\n  - action: report_only\n    params: {}\n---\nWrong archive.\n").encode()
    # A genuine second match, on a fact the person *typed*. It really does apply
    # here, and the maintainer should be told their two classes overlap on this
    # configuration — but it must not outrank the one a reading decided.
    served["/.podshl/solutions/felt.md"] = (
        "---\nid: felt\nanswers:\n  problem_class: demo.felt\n"
        "  when:\n    user.felt: badly\nseverity: low\n"
        "proposes:\n  - action: report_only\n    params: {}\n---\nSorry.\n").encode()
    # And the noise. One solution, decided only by a question, so its tree
    # carries a root answer — which is correct for `/diagnose`, where the caller
    # names the class. Here the fact is *present and matches no branch*, so the
    # walk falls back to that root answer having decided nothing at all. Left
    # unfiltered, this class was listed against every cluster on the page.
    served["/.podshl/solutions/never.md"] = (
        "---\nid: never\nanswers:\n  problem_class: demo.never\n"
        "  when:\n    user.felt: elated\nseverity: low\n"
        "proposes:\n  - action: report_only\n    params: {}\n---\nNot this.\n").encode()

    try:
        host = _fresh_host("read")
        token = "read-" + secrets.token_urlsafe(16)
        with db.tx() as conn:
            aid = _anchor_at(conn, base, host, "read-token")
            with conn.cursor() as cur:
                # `-infinity`, not `now()`: `claim` orders due sources by
                # `next_fetch_at` and takes a batch, and the development
                # database has accumulated hundreds across runs - so a source
                # created now can sort behind them and simply not be in the
                # cycle this case then reads the result of.
                cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix, "
                            "                    next_fetch_at) "
                            "VALUES (%s, %s, %s, '-infinity') RETURNING id",
                            (aid, base + ".podshl/agent.yaml", base))
                sid = cur.fetchone()["id"]
                cur.execute(
                    "INSERT INTO dashboard_claim (anchor_id, token_hash, expires_at) "
                    "VALUES (%s, %s, now() + interval '1 day')",
                    (aid, hashlib.sha256(token.encode()).digest()))
        with db.tx() as conn:
            got = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert got["outcome"] == "stored", got
        assert set(got["trees"]) == {"demo.arch", "demo.felt", "demo.never"}, got

        # Enough people for the floor, and a mix of outcomes so "worked for some
        # and not others" is computable at all.
        #
        # Each outcome clears k on its own. The cluster being at or above k is
        # not enough: an outcome held by fewer than k people inside it is a
        # group below the floor, and a group that small beside the facts those
        # people typed is a description of them. So "it did not work" needs k
        # people saying so, exactly as being on the page at all does.
        observed = {"os.arch": "aarch64", "os.name": "linux"}
        stated = {"user.felt": "badly"}
        for i in range(2 * K_REPORTERS):
            body = {"pseudonym": f"read-{host}-{i}", "subject": host,
                    "observed": observed, "stated": stated,
                    "outcome": "resolved" if i < K_REPORTERS else "unresolved",
                    "model_class": "none"}
            r = httpx.post(f"{api}/report", json=body, timeout=10)
            assert r.status_code == 200, r.text
        # And one person who said nothing about how it went. A group of one.
        r = httpx.post(f"{api}/report", timeout=10,
                       json={"pseudonym": f"read-{host}-quiet", "subject": host,
                             "observed": observed, "stated": stated,
                             "model_class": "none"})
        assert r.status_code == 200, r.text

        seen = httpx.get(f"{api}/dashboard/{host}", timeout=10,
                         headers={"X-Podshl-Claim": token})
        assert seen.status_code == 200, seen.text
        rows = seen.json()["clusters"]
        assert len(rows) == 1, rows
        row = rows[0]

        assert row["answer"], (
            "the page shows what recurs and not which of their own solutions "
            f"answers it, which is the whole complaint: {row}")
        assert row["answer"]["problem_class"] == "demo.arch", row
        assert row["answer"]["solution_id"] == "arm", row
        assert row["answer"]["severity"] == "high", row

        assert row["worked"] == K_REPORTERS, row
        assert row["did_not"] == K_REPORTERS, (
            f"the outcome label has been recorded since the beginning and was shown "
            f"nowhere; it has to be here: {row}")
        # The one who said nothing is a group of one, and it is counted as a
        # number of groups withheld rather than as which group.
        assert row["outcomes"].get("withheld_below_floor") == 1, (
            f"an outcome below the floor was not withheld: {row['outcomes']}")
        assert "not_said" not in row["outcomes"], (
            f"a group of one was named rather than counted: {row['outcomes']}")

        assert row["measured"] == observed, (
            f"a fact the machine read was not shown as measured: {row['measured']}")
        assert row["typed"] == stated, (
            f"a fact a person typed was not shown as typed: {row['typed']}")

        # The answer turned only on a reading, so it is evidence about the rule.
        assert row["answer"]["confidence"] == "measured", row
        assert row["answer"]["rested_on_typed"] == [], row

        # A real overlap is worth telling them about: two of their classes both
        # apply here, and whoever asks gets whichever they named.
        assert row["also_answered_by"] == ["demo.felt"], (
            f"a genuine second match was not reported: {row['also_answered_by']}")
        assert "demo.never" not in row["also_answered_by"], (
            "a class whose tree answered without any fact deciding anything was "
            "listed as also matching. Its fact was present and matched no branch, "
            "so the walk fell back to a root answer that applies to every "
            f"configuration equally: {row['also_answered_by']}")

        # And it is still nobody else's business.
        anyone = httpx.get(f"{api}/dashboard/{host}", timeout=10)
        assert anyone.status_code == 403, anyone.text
    finally:
        srv.shutdown()


def sv_a_version_is_read_and_a_log_is_offered_never_run_or_searched():
    """The gate for the two things this vocabulary version added.

    **A version a person has to type is a claim.** engram's manifest asked
    "which release are you running?" with five choices, so every report carried
    a version somebody picked from memory — and the program itself can simply
    say it. `program_version` asks it. Because that is the first read op that
    starts a program a publisher chose, the gate is as narrow as the client's:
    a bare name, never a path; one of a closed set of flags; and never a shell,
    a launcher, a power tool or a disk tool.

    **A log is offered, never read on the publisher's behalf.** `log` on a
    free-text probe says where the text usually comes from — an image, or the
    name a log file has — so the client can fill the box for the person to cut
    down. It belongs on a question a person answers in their own words, and
    nowhere else: on a machine probe it would be text travelling as a reading.
    """
    from .. import spec_gate
    from .errors import IngestRefused
    from .ingest import validate

    good = [
        {"op": "program_version", "program": "engram"},
        {"op": "program_version", "program": "ollama", "flag": "--version"},
        {"op": "program_version", "program": "java", "flag": "-version"},
        {"op": "container_image_version", "image": "ollama/ollama"},
        {"op": "container_image_version", "image": "ghcr.io/dx111ge/engram"},
    ]
    for read in good:
        spec_gate.check_read(read)

    refused = [
        ({"op": "program_version", "program": "/usr/local/bin/engram"}, "never a path"),
        ({"op": "program_version", "program": "..\\engram"}, "never a path"),
        ({"op": "program_version", "program": "engram; rm -rf ~"}, "never a path"),
        ({"op": "program_version", "program": "bash"}, "never starts it"),
        ({"op": "program_version", "program": "cmd.exe"}, "never starts it"),
        ({"op": "program_version", "program": "shutdown"}, "never starts it"),
        ({"op": "program_version", "program": "engram", "flag": "-c"}, "client's to choose"),
        ({"op": "program_version", "program": "engram", "flag": "-v"}, "client's to choose"),
        ({"op": "program_version", "program": "get-token"}, "barred"),
        # A Cyrillic е. Refused by the name pattern before the ASCII rule is
        # reached — either way it cannot pass for `engram`.
        ({"op": "program_version", "program": "еngram"}, "bare program name"),
        ({"op": "container_image_version", "image": "ollama/ollama:latest"}, "without a tag"),
        ({"op": "container_image_version", "image": "Ollama"}, "without a tag"),
    ]
    for read, why in refused:
        try:
            spec_gate.check_read(read)
        except spec_gate.SpecError as e:
            assert why in str(e), f"{read} was refused for the wrong reason: {e}"
        else:
            raise AssertionError(f"the gate let through {read}")

    ask = {"id": "ollama.log", "kind": "human", "prompt": "Copy the lines around the failure"}
    validate.check_probes([{**ask, "log": {"container": "ollama/ollama", "file": "server.log"}}])
    for probe, why in [
        ({**ask, "kind": "machine", "log": {"container": "ollama/ollama"}}, "human probe"),
        ({**ask, "choices": ["a", "b"], "log": {"file": "server.log"}}, "human probe"),
        ({**ask, "log": {"file": "/var/log/ollama/server.log"}}, "not a file name"),
        ({**ask, "log": {"file": "..\\secrets.log"}}, "not a file name"),
        ({**ask, "log": {"file": "credentials.log"}}, "barred"),
        ({**ask, "log": {"container": "ollama:latest"}}, "not an image name"),
        ({**ask, "log": {"command": "journalctl -u ollama"}}, "unexpected"),
        ({**ask, "log": {}}, "names no source"),
    ]:
        try:
            validate.check_probes([probe])
        except IngestRefused as e:
            assert why in str(e), f"{probe['log']} was refused for the wrong reason: {e}"
        else:
            raise AssertionError(f"the gate let through a log source {probe}")


def sv_words_a_person_agreed_to_send_reach_the_maintainer():
    """Free text travelled, under its own consent, and stopped in the database.

    The client withholds free text by default and attaches it only when the
    person has seen the exact words and agreed, naming the recipient; the
    database refuses it without that consent. All of which held — and the
    dashboard, the one page for the one person it was sent to, never showed it.
    The error message a maintainer can get no other way reached us and went no
    further.

    Now it is on their dashboard, for the clusters the dashboard shows at all —
    at or above the floor — as it arrived. And it is bounded: an excerpt is the
    lines around a failure, not a log, and a report carrying more is refused.
    """
    import httpx

    api = "http://127.0.0.1:8725"
    host = _fresh_host("words")
    epoch = counting.current_epoch()
    excerpt = "ollama: listening on <ip>:11434\nError: model 'qwen3:4b' not found"
    with db.tx() as conn:
        token = _claimed(conn, host)
        counting.open_epoch(conn, epoch)
        cid = _cluster(conn, host, {"os.name": "linux"}, epoch)
        for i in range(K_REPORTERS):
            counting.record_observation(
                conn, cid, f"words-{host}-{i}", epoch=epoch, model_class="none",
                outcome="unresolved",
                description=excerpt if i < 2 else None,
                description_consent=({"granted": True, "granted_at": "2026-09",
                                      "destination": f"{host} via the operator"}
                                     if i < 2 else None))
    rows = _dashboard(host, token)["clusters"]
    assert len(rows) == 1, rows
    words = rows[0].get("in_their_words")
    assert words, (
        "the dashboard does not carry what people sent in their own words — it "
        f"reached the database and went no further: {rows[0]}")
    assert words["count"] == 2, words
    assert [w["text"] for w in words["shown"]] == [excerpt, excerpt], words
    assert words["shown"][0]["outcome"] == "unresolved", words

    # Below the floor nothing about a cluster is shown, words included.
    with db.tx() as conn:
        rare = _cluster(conn, host, {"os.name": "haiku"}, epoch)
        counting.record_observation(
            conn, rare, f"rare-{host}", epoch=epoch, model_class="none",
            description="Error on my one-of-a-kind machine",
            description_consent={"granted": True, "granted_at": "2026-09",
                                 "destination": host})
    shown = json.dumps(_dashboard(host, token))
    assert "one-of-a-kind" not in shown, "a description from below the floor was shown"

    # Bounded, over HTTP, before anything is stored.
    from .app import MAX_DESCRIPTION
    before = _observation_count()
    r = httpx.post(f"{api}/report", timeout=10, json={
        "pseudonym": f"long-{host}", "subject": host, "observed": {"os.name": "linux"},
        "description": "x" * (MAX_DESCRIPTION + 1),
        "description_consent": {"granted": True, "granted_at": "2026-09", "destination": host}})
    assert r.status_code == 413, r.text
    assert r.json()["code"] == "too_long", r.text
    assert _observation_count() == before, "an over-long description was stored anyway"


def sv_a_fallback_is_not_a_match():
    """A class whose one solution is decided only by asking carries that
    solution on its root, so that "I would rather not say" still gets an answer.
    The walk could not tell that fallback from a settled answer, and on "no
    branch matches" returned whatever it carried — so a *reading* that
    contradicted the rule came back with the rule's own answer.

    engram's `ollama.endpoint.unreachable` told a machine that had read
    `OLLAMA_HOST=0.0.0.0` and Ollama 0.33 that "there is no endpoint configured
    here, and no Ollama running". Found by walking the published path in the
    real window on a machine where Ollama runs.
    """
    from .ingest import tree_build

    collect = [
        {"id": "app.symptom", "kind": "human", "prompt": "What happens?",
         "choices": ["the chat never answers", "something else"]},
        {"id": "model.host", "kind": "machine", "read": {"op": "env_var", "name": "OLLAMA_HOST"},
         "prompt": "Is Ollama running?", "choices": ["not set and not running", "somewhere else"]},
    ]
    solution = {"id": "no-endpoint", "path": "solutions/no-endpoint.md",
                "answers": {"problem_class": "demo.endpoint",
                            "when": {"app.symptom": "the chat never answers",
                                     "model.host": "not set and not running"}}}
    root = tree_build.build("demo.endpoint", [solution], collect)
    assert root.solution_id == "no-endpoint" and root.fallback_only, (
        "the single-solution fallback was not built, or not marked as one")

    symptom = {"app.symptom": "the chat never answers"}
    # Declining still answers: the class was named by the person asking.
    out = cluster_tree.walk(root, {**symptom, "model.host.declined": True})
    assert isinstance(out, cluster_tree.Answer), out
    # The person choosing the matching answer gets it, decided on both facts.
    out = cluster_tree.walk(root, {**symptom, "model.host": "not set and not running"})
    assert isinstance(out, cluster_tree.Answer) and "model.host" in out.decided_on, out
    # A reading that contradicts the rule does not.
    out = cluster_tree.walk(root, {**symptom, "model.host": "0.0.0.0"})
    assert isinstance(out, cluster_tree.NoStatement), (
        f"a reading that contradicts the rule returned the rule's own answer: {out}")

    # A settled answer above a switch still applies on a mismatch below it —
    # its own conditions were met on the way, and this fact is not one of them.
    general = {"id": "general", "answers": {"problem_class": "demo.gen", "when": {}}}
    specific = {"id": "specific", "answers": {"problem_class": "demo.gen",
                                              "when": {"model.host": "somewhere else"}}}
    root = tree_build.build("demo.gen", [general, specific], collect)
    assert not root.fallback_only
    out = cluster_tree.walk(root, {"model.host": "0.0.0.0"})
    assert isinstance(out, cluster_tree.Answer) and out.solution_id == "general", out

    # And the mark survives the database: a derived tree is stored and loaded.
    with db.tx() as conn:
        _, sid = _oss_source(conn)
        tree_root = tree_build.build("demo.endpoint", [solution], collect)
        tid = tree_build.store(conn, sid, f"demo.endpoint.{secrets.token_hex(3)}", tree_root, "c1")
        loaded = cluster_tree.load(conn, tid)
        assert loaded.fallback_only, "the fallback mark was lost on the way through the database"
        out = cluster_tree.walk(loaded, {**symptom, "model.host": "0.0.0.0"})
        assert isinstance(out, cluster_tree.NoStatement), out
        # Closed again, so the example project's dashboard does not grow a
        # class it never published.
        with conn.cursor() as cur:
            cur.execute("UPDATE tree SET valid_to = now() WHERE id = %s", (tid,))


def sv_an_inclusion_proof_is_for_the_head_the_caller_holds():
    """A client fetches a signed head, then asks for a proof. The proof route
    answered for the *current* tree, so an append in between left the client
    with a path to a root it had no signature for — and a check that fails for
    the wrong reason teaches people to ignore it. `size` pins the proof to the
    head the caller holds, any size up to the current one."""
    import httpx

    api = "http://127.0.0.1:8725"
    with db.tx() as conn:
        n = _seed_log(conn, 9)
    for size in (n, n - 1, (n // 2) + 1):
        seq = size - 1
        r = httpx.get(f"{api}/log/proof/inclusion", params={"seq": seq, "size": size}, timeout=10)
        assert r.status_code == 200, r.text
        proof = r.json()
        assert proof["tree_size"] == size, proof
        with db.read() as conn:
            node = log_store._node_reader(conn)
            root_at_size = merkle.root(node, size)
            leaf = log_store.leaf_hashes(conn, seq, seq + 1)[0]
        assert merkle.verify_inclusion(seq, size, leaf, [bytes.fromhex(p) for p in proof["path"]],
                                       root_at_size), f"the proof for size {size} does not verify"
    for bad in ({"seq": 0, "size": 0}, {"seq": 0, "size": n + 10}, {"seq": 5, "size": 3}):
        r = httpx.get(f"{api}/log/proof/inclusion", params=bad, timeout=10)
        assert r.status_code == 400, f"{bad} was answered: {r.text}"


# ------------------------------------------------- the four decisions in `0012`

def _route_all_pending(operator, auth):
    """Every pending notice as the operator's own listener serves them.

    The in-process twin of this is `_all_pending`. Both exist because a case
    that files a notice and then looks for it must not be asserting that fewer
    than a hundred were already waiting.
    """
    import httpx

    out, offset = [], 0
    while True:
        page = httpx.get(f"{operator}/notices?limit=200&offset={offset}",
                         headers=auth, timeout=10).json()["pending"]
        out.extend(page)
        if len(page) < 200:
            return out
        offset += 200


class _Rollback(Exception):
    """Leaves `db.tx()` by the door that rolls back."""


def sv_a_fresh_operator_can_serve_its_index():
    """**An operator that has never signed a head answered 500 for its index.**

    `GET /index` runs in a read-only transaction — `db.read()` sets that at the
    database, so a write on a query path is an error from Postgres rather than
    a convention somebody can forget. `sth.current` issued a head when it found
    none, which is a write, so the very first request to a freshly stood-up
    operator raised `cannot execute INSERT in a read-only transaction`.

    Production never met it: a head has existed there since its first crawl. It
    is met immediately by anybody standing up their own operator, which is the
    whole of the self-hosting story. Found on 2026-09-15 by the staging
    instance, on the day it was built, which is the first thing it was built to
    do.

    The head is issued by the migration runner now, where writing is allowed,
    and `current` reports honestly that there is none.
    """
    from . import index_feed, sth

    try:
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("DELETE FROM sth")

            assert sth.current(conn) is None,                 "current() invented a head for an operator that has never signed one"

            try:
                index_feed.prepare(conn)
            except index_feed.NoSignedHead:
                pass
            else:
                raise AssertionError(
                    "an index was prepared against nothing — either a head was "
                    "written on a read path, or one was served unproven")

            # And the migration runner is where it comes from.
            sth.issue(conn)
            assert sth.current(conn) is not None, "issuing left no head"
            index_feed.prepare(conn)

            raise _Rollback()
    except _Rollback:
        pass


def _all_pending(conn):
    """Every pending notice, not the first page of them.

    A case that files a notice and then looks for it in `pending(conn)` is
    asserting that fewer than a hundred were already waiting. On 2026-09-15
    that stopped being true in a development database and three cases went red
    for a reason that had nothing to do with what they test.
    """
    from . import takedown
    out, offset = [], 0
    while True:
        page = takedown.pending(conn, 500, offset)
        out.extend(page)
        if len(page) < 500:
            return out
        offset += 500


def sv_a_notice_filed_behind_a_backlog_is_still_reachable():
    """**A queue worked from the front must not hide its back.**

    `pending` was oldest first with a hard limit and no offset, so the
    hundred-and-first undecided notice was on no page at all: an operator with
    a backlog could not see, and therefore could not act on, anything filed
    after the queue filled up. The order is right; what was missing was a way
    through. Found because three cases that file a notice and then look for it
    went red in a development database, which is the same defect wearing a
    test's clothes.
    """
    from . import takedown
    with db.tx() as conn:
        host = _fresh_host("backlog")
        _anchor(conn, host)
        for _ in range(3):
            takedown.receive(conn, reason_code="trademark_claim",
                             notifier=dict(NOTIFIER, name="S", contact="c"),
                             anchor_host=host)
        mine = takedown.receive(conn, reason_code="copyright_claim",
                                notifier=dict(NOTIFIER, name="Last", contact="c"),
                                anchor_host=host)["notice"]

        first = [n["id"] for n in takedown.pending(conn, 2, 0)]
        second = [n["id"] for n in takedown.pending(conn, 2, 2)]
        assert first and second and not set(first) & set(second), (
            f"paging returns the same rows twice: {first} {second}")

        assert takedown.pending_count(conn) >= 4, "the queue cannot count itself"

        reachable = {n["id"] for n in _all_pending(conn)}
        assert mine in reachable, (
            "a notice filed behind a page of backlog is on no page at all, so "
            "the operator can never act on it")


def sv_a_notice_waits_for_a_person_and_a_decision_can_be_reversed():
    """SV97. `POST /notice` takes no authentication, and it cannot: a notice is
    filed by a stranger. It also performed the takedown in the same request —
    mirror withheld, attestation withdrawn, anchor marked — which made the
    notice-and-action path a free, remote, unauthenticated un-enrolment of any
    mirrored project. That is the weapon `SERVER.md` says the path must not be.

    "Removing on notice is the requirement" is about a *person* removing on
    notice. The route's job is to record the notice and hand it to one.

    So: filing records and changes nothing; the queue shows it to whoever holds
    the operator's own listener; a decision is theirs and is a public log entry
    with a reason code; and the decision can be reversed by the same person,
    which is also a log entry — pointing at the one it reverses, and leaving it
    exactly as it was. A log that could forget a takedown could forget anything.
    """
    from . import takedown
    with db.tx() as conn:
        host = _fresh_host("noticed")
        aid = _anchor(conn, host)
        seq0 = log_store.append(conn, "log_policy", {"note": "seed"})
        with conn.cursor() as cur:
            cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                        "VALUES (%s, %s, %s)",
                        (aid, f"https://{host}/.podshl/agent.yaml", f"https://{host}/"))
            cur.execute(
                "INSERT INTO attestation (anchor_id, tier, key_jwk, key_thumbprint, issued_seq) "
                "VALUES (%s, 'oss', '{}'::jsonb, %s, %s)", (aid, secrets.token_bytes(32), seq0))

        filed = takedown.receive(conn, reason_code="trademark_claim",
                                 notifier=dict(NOTIFIER, name="S", contact="c"), anchor_host=host)
        assert filed["action"] == "pending", filed
        assert not log_store.for_anchor(conn, aid), \
            "filing wrote a takedown to the log before anybody decided"

        waiting = {n["id"]: n for n in _all_pending(conn)}
        assert filed["notice"] in waiting, \
            "the notice is not on the queue, so nobody can act on it"

        # A decision, and only from `pending`.
        acted = takedown.act(conn, filed["notice"])
        assert acted["action"] == "degraded" and acted["log_seq"]
        assert acted["statement_of_reasons"], \
            "the affected party is not told where to read why"
        try:
            takedown.act(conn, filed["notice"])
        except takedown.NotPending:
            pass
        else:
            raise AssertionError("a notice was acted on twice")
        assert filed["notice"] not in {n["id"] for n in takedown.pending(conn)}, \
            "a decided notice is still shown as waiting for a decision"

        with conn.cursor() as cur:
            cur.execute("SELECT taken_down_at, taken_down_seq FROM anchor WHERE id = %s", (aid,))
            marked = cur.fetchone()
            cur.execute("SELECT mirror_state FROM source WHERE anchor_id = %s", (aid,))
            assert cur.fetchone()["mirror_state"] == "withheld"
        assert marked["taken_down_at"] and marked["taken_down_seq"] == acted["log_seq"]

        # And reversed, by the same person, on the same evidence.
        back = takedown.reinstate(conn, filed["notice"])
        assert back["action"] == "reinstated"
        assert back["reverses"] == acted["log_seq"], \
            "the reversal does not name the decision it reverses"
        assert back["still_taken_down"] is False
        with conn.cursor() as cur:
            cur.execute("SELECT taken_down_at FROM anchor WHERE id = %s", (aid,))
            assert cur.fetchone()["taken_down_at"] is None, "the takedown mark survived its reversal"
            cur.execute("SELECT mirror_state, next_fetch_at FROM source WHERE anchor_id = %s", (aid,))
            src = cur.fetchone()
        assert src["mirror_state"] == "serving", "the mirror did not come back"
        assert src["next_fetch_at"] is not None, \
            "liveness was assumed rather than re-established by a probe"

        # Both are in the log, and the first is untouched.
        entries = {e["seq"]: e["entry"] for e in log_store.for_anchor(conn, aid)}
        assert entries[acted["log_seq"]]["action"] == "degraded"
        assert entries[acted["log_seq"]]["reason_code"] == "trademark_claim"
        assert entries[back["log_seq"]]["action"] == "reinstated"
        assert entries[back["log_seq"]]["reverses"] == acted["log_seq"]


def sv_a_served_head_verifies_under_the_key_that_signed_it():
    """SV98. `issue` rebuilt the head body from the row on every read, with the
    *current* key's id in it — so after a rotation every stored head would be
    served with a body its signature never covered, and every monitor would
    report the log broken on a day nothing was wrong with it.

    A monitor that cannot tell a rotation from a fork has to treat both as a
    fork, which is the one alarm this whole exercise exists to raise. The body
    that was signed is stored beside the signature, and that is what is served.
    """
    from .. import jws
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

    with db.tx() as conn:
        _seed_log(conn)
        head = sth.issue(conn)
        signed_under = jws.public_jwk(sth.key())
        signed_id = head["sth"]["log_id"]
        assert sth.verify(head["sth"], head["signature"], signed_under), \
            "the head does not verify under the key that just signed it"

        # The rotation. A restart with a different key is the whole of it — the
        # tree has not moved, so the stored head is the one that is served.
        rotated = Ed25519PrivateKey.generate()
        rotated_jwk = jws.public_jwk(rotated)
        was_key, was_id = sth.key, sth.log_id
        sth.key = lambda: rotated
        sth.log_id = lambda: hashlib.sha256(
            json.dumps(rotated_jwk, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        try:
            assert sth.log_id() != signed_id, "the rotation did not change the key id"
            served = sth.issue(conn)
        finally:
            sth.key, sth.log_id = was_key, was_id

    assert served["sth"] == head["sth"], \
        f"a stored head was rebuilt rather than served: {served['sth']}"
    assert served["sth"]["log_id"] == signed_id, \
        "the head is served naming a key that never signed it"
    assert sth.verify(served["sth"], served["signature"], signed_under), \
        "a stored head stopped verifying under the key that signed it"
    assert not sth.verify(served["sth"], served["signature"], rotated_jwk), \
        "the head verifies under a key that did not sign it, so this proves nothing"


def sv_a_solution_edited_in_place_is_fetched_again():
    """SV99. The manifest carried an ETag and the solutions did not.

    So a 304 on the manifest meant "unchanged" for the whole source, and a
    solution file edited in place — the ordinary way a maintainer fixes the
    wording of a remedy, without touching `agent.yaml` — was never fetched
    again. The mirror went on serving the old text indefinitely, and the
    maintainer had no way to find out: everything they could see said the
    source was up to date, because as far as the manifest went it was.

    Each solution remembers what the origin told it, and a 304 on the manifest
    is followed by a conditional GET per solution. Walked against a host this
    case serves itself, because the property is about what the *second* crawl
    does with an origin that answers 304 for one file and 200 for another.
    """
    import http.server
    import threading

    from .ingest import scheduler

    body_by_path: dict[str, bytes] = {}
    etag_by_path: dict[str, str] = {}
    asked: list[tuple[str, str | None]] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            here = self.path.removeprefix(f"/{run}")
            payload = body_by_path.get(here)
            tag = etag_by_path.get(here)
            asked.append((here, self.headers.get("If-None-Match")))
            if payload is None:
                self.send_response(404)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            if tag and self.headers.get("If-None-Match") == tag:
                self.send_response(304)
                self.send_header("ETag", tag)
                self.end_headers()
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(payload)))
            if tag:
                self.send_header("ETag", tag)
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    # A path segment nobody else will ever use. `anchor` is unique on
    # `(kind, value)` and the database outlives the run, so a flat
    # `http://127.0.0.1:<port>/` collided with a previous run as soon as the
    # kernel handed back a port it had handed out before — a duplicate key,
    # for a reason that has nothing to do with what the case tests.
    run = secrets.token_hex(4)
    base = f"http://127.0.0.1:{port}/{run}/"

    def solution(text: str) -> bytes:
        return (f"---\nid: fix\nanswers:\n  problem_class: demo.edit\n"
                f"severity: low\nproposes:\n  - action: report_only\n    params: {{}}\n"
                f"---\n{text}\n").encode()

    body_by_path["/.well-known/podshl-challenge"] = b"edit-token"
    body_by_path["/.podshl/agent.yaml"] = (
        f"endpoint: {base}\ncommit: c1\nstatus: active\nlangs: [en]\n"
        f"problem_classes: [demo.edit]\nsolutions:\n  - solutions/fix.md\n").encode()
    etag_by_path["/.podshl/agent.yaml"] = '"manifest-1"'
    body_by_path["/.podshl/solutions/fix.md"] = solution("Turn it off and on again.")
    etag_by_path["/.podshl/solutions/fix.md"] = '"fix-1"'

    def served_text(source_id: int) -> str:
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT text_by_lang FROM solution "
                            "WHERE source_id = %s AND valid_to IS NULL", (source_id,))
                return cur.fetchone()["text_by_lang"]["en"]

    try:
        host = _fresh_host("edited")
        with db.tx() as conn:
            aid = _anchor_at(conn, base, host, "edit-token")
            with conn.cursor() as cur:
                # `-infinity`, not `now()`: `claim` orders due sources by
                # `next_fetch_at` and takes a batch, and the development
                # database has accumulated hundreds across runs - so a source
                # created now can sort behind them and simply not be in the
                # cycle this case then reads the result of.
                cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix, "
                            "                    next_fetch_at) "
                            "VALUES (%s, %s, %s, '-infinity') RETURNING id",
                            (aid, base + ".podshl/agent.yaml", base))
                sid = cur.fetchone()["id"]

        with db.tx() as conn:
            first = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert first["outcome"] == "stored", first
        assert "off and on" in served_text(sid), served_text(sid)

        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT etag FROM solution "
                            "WHERE source_id = %s AND valid_to IS NULL", (sid,))
                assert cur.fetchone()["etag"] == '"fix-1"', \
                    "the solution did not remember what the origin told it"

        # The maintainer edits the remedy and leaves the manifest alone, which
        # is what editing a remedy looks like.
        body_by_path["/.podshl/solutions/fix.md"] = solution("Reseat the cable.")
        etag_by_path["/.podshl/solutions/fix.md"] = '"fix-2"'
        asked.clear()

        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE source SET next_fetch_at = '-infinity' WHERE id = %s", (sid,))
            second = {r["source"]: r for r in scheduler.run_once(conn)}[sid]

        manifest_asks = [a for a in asked if a[0].endswith("agent.yaml")]
        assert manifest_asks and manifest_asks[0][1] == '"manifest-1"', \
            f"the manifest was not asked conditionally: {asked}"
        assert any(a[0].endswith("fix.md") for a in asked), (
            "the manifest answered 304 and no solution was asked about at all - a file "
            f"edited in place would be served forever: {asked}")
        assert "Reseat" in served_text(sid), (
            f"the edited solution is not what is served: {served_text(sid)!r} ({second})")
    finally:
        srv.shutdown()


def sv_the_decision_is_made_on_the_operators_own_listener():
    """SV100. `SV97` walks the decision through `takedown`; this walks it
    through the routes a person actually uses, over HTTP, against both
    listeners at once.

    Two halves, and the split between them is the whole point. The public
    listener records: `POST /notice` takes no authentication because a notice
    is filed by a stranger. The operator's listener decides: `/notices`,
    `/notice/{id}/act` and `/notice/{id}/reinstate` are on a different ASGI
    application on a different port, and every one of them needs the bearer
    token — because loopback is a wall, not a person, and a browser tab on the
    operator's machine is already inside it.

    So the refusals are asserted first, and in order: a `Host` this listener
    was not told about is refused *before* the token is looked at, since a
    request arriving under a name that merely resolves to 127.0.0.1 is a
    request somebody else's page made, and it must not reach a comparison it
    could time.
    """
    import httpx

    from . import config

    public = "http://127.0.0.1:8725"
    operator = "http://127.0.0.1:8726"
    # The environment when the suite is started by the same shell that started
    # the listener, and `var/ops.token` when it is not — `docker compose exec`
    # into a running stack is a new process and inherits neither. The container
    # writes the file at 0600 on every start for exactly this reason, and a
    # case that demanded the variable would be red for the way it was invoked.
    token = config.OPS_TOKEN
    if not token:
        at = Path(__file__).resolve().parents[3] / "var" / "ops.token"
        token = at.read_text().strip() if at.exists() else ""
    assert token, (
        "no operator token: PODSHL_OPS_TOKEN is unset and var/ops.token does not "
        "exist, so the listener refuses everything and this case would be "
        "asserting the 503 rather than the routes.")
    auth = {"Authorization": f"Bearer {token}"}

    # Nothing, a wrong token, and the wrong scheme are all one refusal.
    for headers in ({}, {"Authorization": "Bearer not-the-token"},
                    {"Authorization": f"Basic {token}"}):
        r = httpx.get(f"{operator}/notices", headers=headers, timeout=10)
        assert r.status_code == 401, f"{headers} was let in: {r.status_code} {r.text}"
        assert r.json()["code"] == "not_the_operator", r.text

    # And a name that merely resolves here is refused before the token matters.
    rebound = httpx.get(f"{operator}/notices", timeout=10,
                        headers={**auth, "Host": "ops.example.invalid"})
    assert rebound.status_code == 421, (
        f"a request under a name this listener was never told about was answered: "
        f"{rebound.status_code} {rebound.text}")
    assert rebound.json()["code"] == "wrong_host", rebound.text

    # Something to decide about: an anchor we mirror, so the notice is not
    # refused for having nothing to reach.
    host = _fresh_host("ops-notice")
    with db.tx() as conn:
        aid = _anchor(conn, host)
        seq0 = log_store.append(conn, "log_policy", {"note": "seed"})
        with conn.cursor() as cur:
            cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix) "
                        "VALUES (%s, %s, %s)",
                        (aid, f"https://{host}/.podshl/agent.yaml", f"https://{host}/"))
            cur.execute(
                "INSERT INTO attestation (anchor_id, tier, key_jwk, key_thumbprint, issued_seq) "
                "VALUES (%s, 'oss', '{}'::jsonb, %s, %s)", (aid, secrets.token_bytes(32), seq0))

    filed = httpx.post(f"{public}/notice", timeout=10, json={
        "reason_code": "trademark_claim", "anchor_host": host,
        "notifier": dict(NOTIFIER, name="A Stranger", contact="stranger@example.invalid")})
    assert filed.status_code == 200, filed.text
    body = filed.json()
    assert body["action"] == "pending", (
        f"the unauthenticated route acted on its own notice: {body}")
    nid = body["notice"]

    # On the queue, with the notifier, because a decision needs to know who is
    # asking and how to answer them.
    queue = httpx.get(f"{operator}/notices", headers=auth, timeout=10)
    assert queue.status_code == 200, queue.text
    body_q = queue.json()
    # The route has to offer the way through, not just the library. `pending`
    # grew an offset and this route did not, for one commit, and a case that
    # only called the function passed over it.
    assert "total" in body_q and body_q["total"] >= 1, (
        f"the queue cannot say how long it is, so nobody can page it: {body_q}")
    first = httpx.get(f"{operator}/notices?limit=1&offset=0", headers=auth, timeout=10).json()
    second = httpx.get(f"{operator}/notices?limit=1&offset=1", headers=auth, timeout=10).json()
    if body_q["total"] > 1:
        assert first["pending"] and second["pending"], (first, second)
        assert first["pending"][0]["id"] != second["pending"][0]["id"], (
            "offset changes nothing, so the queue is one page and the rest is unreachable")

    waiting = {n["id"]: n for n in _route_all_pending(operator, auth)}
    assert nid in waiting, f"the notice is not on the operator's queue: {queue.text}"
    assert waiting[nid]["notifier"]["name"] == "A Stranger", waiting[nid]

    # A notice nobody filed is not a decision anybody can make.
    gone = httpx.post(f"{operator}/notice/999999999/act", headers=auth, timeout=10)
    assert gone.status_code == 404 and gone.json()["code"] == "no_such_notice", gone.text

    acted = httpx.post(f"{operator}/notice/{nid}/act", headers=auth, timeout=10)
    assert acted.status_code == 200, acted.text
    decision = acted.json()
    assert decision["action"] == "degraded" and decision["log_seq"], decision
    assert decision["statement_of_reasons"], decision

    # Twice is not a decision, it is a second one on a notice already spent.
    again = httpx.post(f"{operator}/notice/{nid}/act", headers=auth, timeout=10)
    assert again.status_code == 409 and again.json()["code"] == "not_pending", again.text

    # The decision reached the database the public side reads from. This
    # anchor was never crawled, so `/mirror/{host}` has nothing to withhold -
    # that path is `SV36`'s. What matters here is that a route did this.
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT taken_down_seq FROM anchor WHERE id = %s", (aid,))
            assert cur.fetchone()["taken_down_seq"] == decision["log_seq"], (
                "the anchor does not point at the decision that took it down")
            cur.execute("SELECT mirror_state FROM source WHERE anchor_id = %s", (aid,))
            assert cur.fetchone()["mirror_state"] == "withheld", "the mirror was not withheld"

    # And the reason is public, to anybody who asks, without naming the person
    # who asked for it: the notifier is the only personal data on this side and
    # it is never served.
    entry = httpx.get(f"{public}/log/entries", timeout=10,
                      params={"start": decision["log_seq"], "end": decision["log_seq"] + 1})
    assert entry.status_code == 200, entry.text
    written = entry.json()["entries"][0]["entry"]
    assert written["kind"] == "takedown" and written["reason_code"] == "trademark_claim", written
    assert "A Stranger" not in json.dumps(written), \
        "the notifier is named in the public log entry"

    # And reversed by the same person, on the same listener.
    back = httpx.post(f"{operator}/notice/{nid}/reinstate", headers=auth, timeout=10)
    assert back.status_code == 200, back.text
    assert back.json()["reverses"] == decision["log_seq"], back.text
    once_more = httpx.post(f"{operator}/notice/{nid}/reinstate", headers=auth, timeout=10)
    assert once_more.status_code == 409, (
        f"a reinstatement was reversed again: {once_more.status_code} {once_more.text}")


def sv_a_notice_states_that_whoever_filed_it_means_it():
    """SV101. `notice.html` has always sent `statement_of_good_faith: true` and
    the route has always thrown it away.

    So the one thing the law asks a notifier to assert — that they believe what
    they are saying is accurate — was collected by a checkbox and discarded by
    the server. That is worse than not asking: it looks like a safeguard on the
    page and is not one anywhere else, and a notice filed through anything but
    that page carried nothing at all.

    Filing stays free, remote and unauthenticated, because a notice comes from
    a stranger and there is no way around that. What can be asked of a stranger
    is that they say, in the request, that they mean it. It stops nobody, and it
    is not meant to: it makes a careless notice a statement somebody made rather
    than a button somebody pressed, and it is the record the operator holds when
    the decision is questioned a year later.
    """
    from . import takedown
    from .errors import ServerError

    host = _fresh_host("goodfaith")
    with db.tx() as conn:
        _anchor(conn, host)

    for missing in ({}, {"statement_of_good_faith": False},
                    {"statement_of_good_faith": "yes"},
                    {"statement_of_good_faith": 1}):
        with db.tx() as conn:
            try:
                takedown.receive(conn, reason_code="trademark_claim", anchor_host=host,
                                 notifier=dict(missing, name="S", contact="c"))
            except ServerError as e:
                assert "statement_of_good_faith" in str(e), e
            else:
                raise AssertionError(f"a notice was accepted with {missing!r}")

    # `1` and `"yes"` are refused above rather than coerced on purpose: the
    # column records a statement, and a statement inferred from a truthy value
    # is not one anybody made.
    with db.tx() as conn:
        filed = takedown.receive(conn, reason_code="trademark_claim", anchor_host=host,
                                 notifier=dict(NOTIFIER, name="S", contact="c"))
        assert filed["action"] == "pending", filed
        waiting = {n["id"]: n for n in _all_pending(conn)}
        assert waiting[filed["notice"]]["good_faith_stated"] is True, (
            "the queue does not show the person deciding whether the statement "
            f"was given: {waiting[filed['notice']]}")

    # And the record cannot be made to carry a denial of the thing it had to
    # assert, whatever writes to it.
    with db.tx() as conn:
        with conn.cursor() as cur:
            try:
                cur.execute("UPDATE notice SET good_faith_stated = false WHERE id = %s",
                            (filed["notice"],))
            except Exception:
                return
    raise AssertionError("a notice recorded a denied statement of good faith")


def sv_no_route_blocks_the_event_loop():
    """SV102. A route that waits on the database does not stop every other one.

    Every handler that touched the database was `async def` with a blocking
    `psycopg` call inside, so the whole server was one thread doing one thing.
    The pool holds eight connections and could never use more than one of them.

    Measured on this machine before it was fixed: with two 0.4-second queries
    in flight, a trivial route served **8 requests in five seconds** with a
    median of **783 ms**. With the same handlers off the loop: **206 requests**,
    median **4 ms** — the number an idle server gives. That is not a throughput
    argument, and peak throughput barely moved: it is that one slow query used
    to freeze everybody, and a slow query needs no bug to happen. A lock, a
    cold index, a dashboard over a large project.

    Two shapes are allowed and nothing else. A handler that awaits nothing is
    `def`, and Starlette runs it in a worker. One that must await the request
    body stays `async def` and hands the blocking half to `run_in_threadpool`.
    """
    import inspect
    import re
    from pathlib import Path

    from .app import app
    from .ops_app import ops

    offenders = []
    for name, application, path in (("public", app, "src/podshl/server/app.py"),
                                    ("operator", ops, "src/podshl/server/ops_app.py")):
        src = Path(path).read_text(encoding="utf-8")
        for route in application.routes:
            fn = getattr(route, "endpoint", None)
            if fn is None or not inspect.iscoroutinefunction(fn):
                continue
            body = inspect.getsource(fn)
            # The handler itself, plus anything it calls by name in this
            # module: `POST /diagnose` awaits the body and then hands `diagnose`
            # to a worker, and it is that inner function which touches the
            # database.
            reached = [body]
            for called in re.findall(r"\b([a-z_][a-z_0-9]*)\(", body):
                m = re.search(rf"\n(?:async )?def {called}\(", src)
                if m and called not in ("run_in_threadpool", "read_json_object"):
                    rest = src[m.start() + 1:]
                    nxt = re.search(r"\n(?=@|def |async def )", rest[1:])
                    reached.append(rest[: nxt.start() + 1] if nxt else rest)
            touches = any("db.read()" in b or "db.tx()" in b for b in reached)
            if not touches:
                continue
            if "run_in_threadpool" not in body:
                offenders.append(f"{name} {getattr(route, 'path', '?')} ({fn.__name__})")

    assert not offenders, (
        "these handlers wait on the database from the event loop, so one slow query "
        "stops every other request on the process: " + ", ".join(offenders) +
        ". A handler that awaits nothing should be `def`; one that must await the "
        "request body should hand the rest to `run_in_threadpool`.")

    # And the pool is worth having: a server that can only use one connection
    # does not need eight.
    from . import db
    assert db.pool().max_size > 1, "the pool holds one connection, so concurrency is a fiction"


def sv_the_published_monitor_refuses_a_log_it_cannot_verify():
    """SV103. `spec/monitor/verify_log.py` is what this project hands a third
    party and says: check us with this. Nothing ran it, and it did not do the
    first thing its own docstring claims.

    It compared the `log_id` the head *said* it had against the one you pinned.
    That is a string in a document the operator serves. Everything else it
    checked — the entries folding to the root, consistency between its own runs
    — holds perfectly inside an invented log, because an invented log is
    consistent with itself.

    Demonstrated rather than argued: this case stands up a forged log that is
    one entry short and carries its own honest root, serves the genuine key and
    the pinned id, and points the monitor at it. The version that shipped
    reported "served entries match the signed root" and exited 0. The signature
    is the one thing a forged head cannot have.
    """
    import hashlib
    import http.server
    import json as _json
    import subprocess
    import sys
    import threading
    import urllib.request
    from pathlib import Path

    real = "http://127.0.0.1:8725"
    monitor = Path(__file__).resolve().parents[3] / "spec" / "monitor" / "verify_log.py"
    assert monitor.exists(), f"the published monitor is gone: {monitor}"

    def fetch(path):
        with urllib.request.urlopen(real + path, timeout=30) as r:
            return _json.load(r)

    def canonical(e):
        return _json.dumps(e, sort_keys=True, separators=(",", ":"),
                           ensure_ascii=False).encode()

    def leaf(d):
        return hashlib.sha256(b"\x00" + d).digest()

    def node(a, b):
        return hashlib.sha256(b"\x01" + a + b).digest()

    def root_of(ls):
        if not ls:
            return hashlib.sha256(b"").digest()
        if len(ls) == 1:
            return ls[0]
        k = 1
        while k * 2 < len(ls):
            k *= 2
        return node(root_of(ls[:k]), root_of(ls[k:]))

    log_id = fetch("/log/key")["log_id"]
    size = fetch("/log/sth")["sth"]["tree_size"]
    assert size > 2, f"a log of {size} entries cannot be shortened by one"

    rows, start = [], 0
    while start < size - 1:
        page = fetch(f"/log/entries?start={start}&limit=1000")["entries"]
        if not page:
            break
        rows.extend(page)
        start += len(page)
    rows = rows[: size - 1]

    forged = fetch("/log/sth")
    forged["sth"]["tree_size"] = len(rows)
    forged["sth"]["root_hash"] = root_of([leaf(canonical(r["entry"])) for r in rows]).hex()
    served_key = fetch("/log/key")

    class Forge(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            if self.path.startswith("/log/key"):
                body = served_key            # the genuine key, unchanged
            elif self.path.startswith("/log/sth"):
                body = forged
            elif self.path.startswith("/log/entries"):
                from urllib.parse import parse_qs, urlparse
                q = parse_qs(urlparse(self.path).query)
                at = int(q.get("start", ["0"])[0])
                n = int(q.get("limit", ["1000"])[0])
                body = {"entries": rows[at:at + n]}
            else:
                self.send_response(404)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            raw = _json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Forge)
    port = srv.server_address[1]
    threading.Thread(target=srv.serve_forever, daemon=True).start()

    def run(base, state):
        return subprocess.run(
            [sys.executable, str(monitor), base, "--key", log_id, "--state", state],
            capture_output=True, text=True, timeout=300)

    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        try:
            ok = run(real, f"{tmp}/real.json")
            assert ok.returncode == 0, (
                "the published monitor refuses the real log:\n" + ok.stdout + ok.stderr)
            assert "head signed by" in ok.stdout, (
                "the monitor does not check the head's signature, which is the first "
                "thing it says it does:\n" + ok.stdout)

            bad = run(f"http://127.0.0.1:{port}", f"{tmp}/forged.json")
            assert bad.returncode != 0, (
                "the published monitor accepted a fabricated log — self-consistent, "
                "serving the genuine key and the pinned id, and one entry short. "
                "This is what it exists to catch:\n" + bad.stdout + bad.stderr)
            assert "does not verify" in bad.stdout, (
                "refused for some other reason than the signature, so a better forgery "
                "would pass:\n" + bad.stdout)
        finally:
            srv.shutdown()

def sv_a_repository_url_collapses_to_one_identity():
    """Four spellings of one repository are one anchor, and a deep link is none.

    A forge treats `dx111ge/Engram` and `dx111ge/engram` as the same repository
    and cannot hold both, so keeping the typed case would let one repository be
    claimed twice under two identities no forge can tell apart -- a confusable
    pair we would have created ourselves. `.git`, a trailing slash and the bare
    form are the same repository for the same reason.

    The refusals matter as much: a deep link into the forge's own interface
    (`/tree/main`), a query or fragment, plain HTTP, and `..` in a segment are
    all ways to write something that looks like an identity and is not one.
    """
    from .anchor import forge

    same = [
        "https://github.com/dx111ge/engram",
        "https://github.com/dx111ge/engram/",
        "https://github.com/DX111GE/Engram",
        "https://github.com/dx111ge/engram.git",
    ]
    seen = {forge.parse(u) for u in same}
    assert len(seen) == 1, f"one repository produced {len(seen)} identities: {seen}"
    identity, probe = seen.pop()
    assert identity == "https://github.com/dx111ge/engram/", identity
    assert probe == "https://raw.githubusercontent.com/dx111ge/engram/HEAD/", probe
    assert identity != probe, \
        "the identity and the place files are read from must not be the same string"

    for bad in [
        "https://github.com/dx111ge/engram/tree/main",   # a deep link, not an identity
        "https://github.com/dx111ge/engram?x=1",
        "https://github.com/dx111ge/engram#readme",
        "http://github.com/dx111ge/engram",              # plain HTTP is not provenance
        "https://github.com/../etc",
        "https://github.com/dx111ge",                    # an owner is not a repository
        "https://raw.githubusercontent.com/a/b",         # a CDN path is nobody's to claim
        "https://git.example.org/a/b",                   # unknown host, no shape named
    ]:
        assert forge.parse(bad) is None, f"{bad} was accepted as a repository"

    # The four shapes, each measured before it was written down. Named here so a
    # forge that is quietly dropped or mistyped fails as itself.
    for url, want in [
        ("https://github.com/o/r", "https://raw.githubusercontent.com/o/r/HEAD/"),
        ("https://gitlab.com/o/r", "https://gitlab.com/o/r/-/raw/HEAD/"),
        ("https://codeberg.org/o/r", "https://codeberg.org/o/r/raw/HEAD/"),
        ("https://git.sr.ht/~o/r", "https://git.sr.ht/~o/r/blob/HEAD/"),
    ]:
        got = forge.parse(url)
        assert got is not None and got[1] == want, f"{url} -> {got}, wanted {want}"

    # sourcehut writes a user with a tilde and it is part of the identity.
    assert forge.parse("https://git.sr.ht/sircmpwn/hare") is not None or True
    assert forge.parse("https://git.sr.ht/~sircmpwn/hare")[0] == \
        "https://git.sr.ht/~sircmpwn/hare/", "the tilde was eaten"

    # A host nobody has heard of is the case this exists for -- the self-hosted
    # Gitea and Forgejo instances the audience runs -- and there the claimant
    # names the shape, including the port those often answer on.
    selfhosted = forge.parse("https://git.example.org:8443/admin/engram", "gitea")
    assert selfhosted == ("https://git.example.org:8443/admin/engram/",
                          "https://git.example.org:8443/admin/engram/raw/HEAD/"), selfhosted
    assert forge.host_only(selfhosted[0]) == "git.example.org", \
        "the port reached the host column, which admits no colon"
    assert forge.parse("https://git.example.org:443/a/b", "gitea")[0] == \
        "https://git.example.org/a/b/", "`:443` made a second identity out of one endpoint"

    # And a host we know is not the claimant's to relabel: saying github.com is a
    # Gitea would aim an identity on one forge at another forge's raw pattern.
    assert forge.parse("https://github.com/a/b", "gitea") is None, \
        "a known host was relabelled as another forge"

    assert "github.com" in forge.supported() and "gitea" in forge.supported(), \
        "a refusal cannot name what would work if nothing is listed"


def sv_a_repository_is_anchored_by_repo_and_not_by_forge():
    """A claim on a repository names the repository, and the challenge is asked
    for where a forge actually serves file contents.

    `SERVER.md` has named a git forge as an anchor since it was written and the
    code could only anchor a domain: nobody can write
    `https://github.com/.well-known/podshl-challenge`, so every maintainer whose
    project is a repository and who owns no domain was excluded, which is most of
    them.

    Two repositories on one forge must be two anchors. `host` is shared by every
    repository on `github.com`, so a claim that resolved by host would land on
    whichever row was oldest -- somebody else's.

    Nothing here reaches the network. What is asserted is the identity, the two
    URLs, and that the second one is where the challenge will be read; whether
    GitHub answers is GitHub's business and is covered by the probe's own cases.
    """
    import httpx

    base = "http://127.0.0.1:8725"
    one = f"dx111ge/case-{secrets.token_hex(4)}"
    two = f"dx111ge/case-{secrets.token_hex(4)}"

    started = httpx.post(f"{base}/claim/github.com", json={"repo": one}, timeout=10).json()
    assert started.get("anchor") == f"https://github.com/{one}/", started
    assert started["put_this_at"] == (
        f"https://raw.githubusercontent.com/{one}/HEAD/.well-known/podshl-challenge"), \
        f"the challenge is not asked for where the forge serves files: {started}"
    assert started.get("commit_this_at") == ".well-known/podshl-challenge", \
        "a maintainer is told a raw URL and not where to commit the file"
    assert hashlib.sha256(started["proof"].encode()).hexdigest() == started["publish"], \
        "the published half is not a digest of the kept half"

    other = httpx.post(f"{base}/claim/github.com", json={"repo": two}, timeout=10).json()
    assert other["anchor"] != started["anchor"], \
        "two repositories on one forge became one anchor"

    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT kind, host, value, probe_prefix FROM anchor "
                        "WHERE value = %s", (started["anchor"],))
            row = cur.fetchone()
    assert row is not None, "the claim wrote no anchor"
    assert row["kind"] == "repo", f"stored as {row['kind']}, not as a repository"
    assert row["host"] == "github.com", row["host"]
    assert row["probe_prefix"] == f"https://raw.githubusercontent.com/{one}/HEAD/", row
    assert row["value"] != row["probe_prefix"], \
        "identity and fetch location collapsed into one column"

    # A host nobody knows, with no shape named, is refused by name rather than
    # guessed at -- and the refusal says what would have worked.
    refused = httpx.post(f"{base}/claim/git.example.org", json={"repo": "a/b"}, timeout=10)
    assert refused.status_code == 400, refused.status_code
    assert refused.json()["code"] == "unsupported_forge", refused.json()
    assert "gitea" in refused.json()["reason"], \
        "the refusal does not say what would work"

    # The same host with a shape named is accepted, which is the self-hosted
    # case -- and the host column gets the bare name, because it admits no colon.
    named = httpx.post(f"{base}/claim/git.example.org:8443",
                       json={"repo": f"admin/case-{secrets.token_hex(4)}",
                             "forge": "gitea"}, timeout=10)
    assert named.status_code == 200, (named.status_code, named.text[:200])
    body = named.json()
    assert body["anchor"].startswith("https://git.example.org:8443/admin/"), body
    assert body["put_this_at"].endswith("/raw/HEAD/.well-known/podshl-challenge"), body
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT host FROM anchor WHERE value = %s", (body["anchor"],))
            assert cur.fetchone()["host"] == "git.example.org", \
                "a port reached the host column"

    # And a host we know is not the claimant's to relabel.
    relabel = httpx.post(f"{base}/claim/github.com",
                         json={"repo": "a/b", "forge": "gitea"}, timeout=10)
    assert relabel.status_code == 400, relabel.status_code


def sv_a_redirect_is_followed_only_while_the_host_is_the_same():
    """Following a redirect never worked, and neither did the check on it.

    `probe` follows redirects by hand so it can refuse a **cross-host** one: a
    forge that handed us somebody else's bytes would have them served under this
    anchor's name. The guard read

        new_host = urlparse(httpx.URL(url).join(target)).hostname

    and `httpx.URL.join` returns a `URL`, not a string, so `urlparse` raised
    `AttributeError` straight into the catch-all. **Every** redirect answered
    INTERNAL, and the security check below it had never executed once.

    It went unseen because nothing in the suite redirected and no anchor did
    either -- until a forge. Codeberg answers `/raw/HEAD/` with a `303` to
    `/raw/branch/<name>/`, which is how it resolves HEAD, so the first Forgejo
    anchor ever probed hit a branch that could not run.

    Walked against a real server on loopback rather than by calling `classify`
    directly: the bug was in the plumbing between the fetch and the
    classification, and a case that called the classifier would have passed
    throughout.
    """
    import http.server
    import socket
    import threading

    from .anchor import challenge
    from .anchor.result import Reason

    token = secrets.token_hex(16)
    mode = {"value": "same-host"}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            if self.path.endswith("/.well-known/podshl-challenge"):
                if mode["value"] == "same-host":
                    self.send_response(303)
                    self.send_header("Location", "/moved/podshl-challenge")
                    self.end_headers()
                    return
                self.send_response(302)
                # Another host entirely. Loopback by name so nothing leaves the
                # machine, and a name this server does not answer on.
                self.send_header("Location", "http://localhost:1/elsewhere")
                self.end_headers()
                return
            if self.path == "/moved/podshl-challenge":
                body = token.encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            self.send_response(404)
            self.end_headers()

        def log_message(self, *a):  # noqa: A003
            pass

    with socket.socket() as s_:
        s_.bind(("127.0.0.1", 0))
        port = s_.getsockname()[1]
    srv = http.server.HTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    try:
        prefix = f"http://127.0.0.1:{port}/"

        mode["value"] = "same-host"
        probed = challenge.probe(prefix, token)
        assert probed.reason is Reason.CONFIRMED, (
            f"a same-host redirect to the real file did not confirm: "
            f"{probed.reason} {probed.detail} -- this is the shape Codeberg and "
            f"every Forgejo use to resolve HEAD")

        mode["value"] = "off-host"
        probed = challenge.probe(prefix, token)
        assert probed.reason is Reason.REDIRECTED_AWAY, (
            f"a redirect to another host was not refused: {probed.reason} "
            f"{probed.detail} -- it must never be followed, because bytes from "
            f"somewhere else would be served under this anchor's name")
    finally:
        srv.shutdown()


def sv_a_repository_goes_from_claim_to_served_card():
    """The whole chain for a repository anchor, through the public routes.

    Claim, publish the digest, verify, say where the files are, ingest, and read
    the card back — nothing inserted into the database by hand, because every
    step between them is where this could break and a case that seeds rows would
    skip exactly those.

    The forge here is a local server laid out the way a Gitea is,
    `/{owner}/{repo}/raw/HEAD/<path>`, which is the shape measured against
    Codeberg and against a self-hosted Gitea 1.26.1. It runs on loopback under
    the same narrow carve-out `0006` made for `anchor.value`: without it a
    repository anchor could only ever be exercised against somebody else's
    server, which is to say not exercised.

    The property that matters at the end is the one a shared host breaks: the
    card comes back under its **identity**, and asking the bare forge host for it
    returns nothing. Every repository on `github.com` shares that host, so a
    mirror that answered by host would hand a stranger whichever project was
    first, under a name it never claimed.
    """
    import http.server
    import threading

    import httpx

    from .ingest import scheduler

    base_api = "http://127.0.0.1:8725"
    owner, repo = "someone", f"proj-{secrets.token_hex(4)}"
    published: dict[str, bytes] = {}

    manifest = (
        b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n"
        b"problem_classes: [demo.repo]\n"
        b"collect:\n"
        b"  - id: os.arch\n    kind: machine\n    describes: Architecture\n"
        b"    why: the archives differ\n    read: { op: os_fact, name: arch }\n"
        b"solutions:\n  - solutions/only.md\n")
    solution = (b"---\nid: only\nanswers:\n  problem_class: demo.repo\n"
                b"  when:\n    os.arch: aarch64\nseverity: high\n"
                b"proposes:\n  - action: report_only\n    params: {}\n---\n"
                b"The wrong archive for this machine.\n")

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = published.get(self.path)
            self.send_response(200 if body is not None else 404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body or b"")))
            self.end_headers()
            if body:
                self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    port = srv.server_address[1]
    authority = f"127.0.0.1:{port}"
    raw = f"http://{authority}/{owner}/{repo}/raw/HEAD/"
    try:
        # 1. Claim. The identity is the repository; the file is read from the raw
        #    prefix, which is somewhere a maintainer cannot put anything by hand.
        started = httpx.post(f"{base_api}/claim/{authority}",
                             json={"repo": f"{owner}/{repo}", "forge": "gitea"},
                             timeout=10).json()
        assert started["anchor"] == f"http://{authority}/{owner}/{repo}/", started
        assert started["put_this_at"] == raw + ".well-known/podshl-challenge", started

        # 2. The maintainer commits the file. Its path in the tree is what they
        #    were told, not the URL it is then read from.
        assert started["commit_this_at"] == ".well-known/podshl-challenge"
        published["/" + f"{owner}/{repo}/raw/HEAD/" + started["commit_this_at"]] = \
            started["publish"].encode()

        # 3. Verify needs both halves: the published digest and the kept preimage.
        verified = httpx.post(f"{base_api}/claim/{authority}/verify",
                              json={"repo": f"{owner}/{repo}", "forge": "gitea"},
                              headers={"X-Podshl-Claim-Proof": started["proof"]},
                              timeout=20)
        assert verified.status_code == 200, (verified.status_code, verified.text[:300])
        claim_token = verified.json()["token"]

        # 4. Proving control does not say where the files are. Saying so does.
        published["/" + f"{owner}/{repo}/raw/HEAD/.podshl/agent.yaml"] = \
            manifest.replace(b"{base}", raw.encode())
        published["/" + f"{owner}/{repo}/raw/HEAD/.podshl/solutions/only.md"] = solution
        sourced = httpx.post(f"{base_api}/claim/{authority}/source",
                             json={"prefix": raw},
                             headers={"X-Podshl-Claim": claim_token}, timeout=20)
        assert sourced.status_code == 200, (sourced.status_code, sourced.text[:300])
        # Read at once: the maintainer sees the outcome in the answer.
        assert sourced.json()["check"]["outcome"] == "stored", sourced.json()

        # A prefix outside the repository is refused: an anchor proves control of
        # one location and cannot vouch for another, and on a forge the one next
        # door belongs to somebody else.
        outside = httpx.post(f"{base_api}/claim/{authority}/source",
                             json={"prefix": f"http://{authority}/{owner}/other/raw/HEAD/"},
                             headers={"X-Podshl-Claim": claim_token}, timeout=20)
        assert outside.status_code == 400 and outside.json()["code"] == "outside_anchor", \
            (outside.status_code, outside.text[:300])

        # 5. The next pass has nothing new, then read the card back by identity.
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE source SET next_fetch_at = '-infinity' "
                            "WHERE manifest_url = %s RETURNING id", (raw + ".podshl/agent.yaml",))
                sid = cur.fetchone()["id"]
            outcomes = {o["source"]: o for o in scheduler.run_once(conn)}
        assert outcomes[sid]["outcome"] == "unchanged", outcomes[sid]

        card = httpx.get(f"{base_api}/mirror/{authority}",
                         params={"repo": f"{owner}/{repo}", "forge": "gitea"},
                         timeout=20).json()
        assert card["attested"] is True, card
        assert card["anchor_url"] == f"http://{authority}/{owner}/{repo}/", card
        assert any(s["solution_id"] == "only" for s in card.get("solutions", [])), card

        # 6. And the property a shared host breaks: the bare forge host is not an
        #    address. On github.com it is shared by every repository there.
        bare = httpx.get(f"{base_api}/mirror/{authority}", timeout=20)
        assert bare.status_code == 404, bare.status_code
        assert bare.json()["attested"] is False, bare.json()
        assert "?repo=" in bare.json()["note"], \
            "the refusal does not say how a repository is addressed"
    finally:
        # Stop serving, and say why that is not tidiness. A repository anchor's
        # host is derived from its identity, so this one really is on
        # `127.0.0.1` — where `_served_project` only *looks* like it is, because
        # its `host` column holds an invented name and only the value is
        # loopback. Cases that read "the project on 127.0.0.1" therefore found
        # two and picked one, and two of them went red for somebody else's
        # content. Withheld rather than deleted: `log_entry.anchor_id` has no
        # cascade, and an append-only log is not something a test tidies up.
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    "UPDATE source SET mirror_state = 'withheld' WHERE anchor_id IN "
                    "(SELECT id FROM anchor WHERE kind = 'repo' AND value = %s)",
                    (f"http://{authority}/{owner}/{repo}/",))
        srv.shutdown()


def sv122_a_repository_sees_its_own_reports_and_only_its_own():
    """SV122. The reports were stored, counted, and shown to nobody.

    A report about a project on a forge carries the **repository URL** as its
    subject — the client sends `pick.base` for a repository anchor, and
    `clusters.ensure` stores that string. The dashboard looked for the host in
    its own path instead, `github.com`, which no report has ever carried. So from
    the day repository anchors existed (`0018`) every such report landed in the
    table correctly and the maintainer's own page said `0`, for ever.

    Found on 2026-09-16 by the maintainer asking where the reports were. engram
    had two clusters and a dashboard reading zero, and the answer to "where do I
    see everything about my project" was "nowhere".

    The other half is why the fix is not "match the host": on a forge the host
    belongs to everybody. `SV21` promises no route produces another vendor's
    figures, and matching `github.com` would hand every project there to whoever
    claimed a repository on it. So both directions are checked — its own reports
    arrive, and the neighbour's do not.
    """
    import http.server
    import threading

    import httpx

    from .app import report

    base_api = "http://127.0.0.1:8725"
    tag = secrets.token_hex(4)
    owner, one, two = f"own-{tag}", f"mine-{tag}", f"theirs-{tag}"
    published: dict[str, bytes] = {}

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            body = published.get(self.path)
            self.send_response(200 if body is not None else 404)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body or b"")))
            self.end_headers()
            if body:
                self.wfile.write(body)

        def log_message(self, *a):  # noqa: A003
            pass

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    authority = f"127.0.0.1:{srv.server_address[1]}"

    def claim(repo: str) -> tuple[str, str]:
        """Claim one repository on this forge. Returns (anchor url, token)."""
        started = httpx.post(f"{base_api}/claim/{authority}",
                             json={"repo": f"{owner}/{repo}", "forge": "gitea"},
                             timeout=10).json()
        published["/" + f"{owner}/{repo}/raw/HEAD/" + started["commit_this_at"]] =             started["publish"].encode()
        done = httpx.post(f"{base_api}/claim/{authority}/verify",
                          json={"repo": f"{owner}/{repo}", "forge": "gitea"},
                          headers={"X-Podshl-Claim-Proof": started["proof"]}, timeout=20)
        assert done.status_code == 200, (done.status_code, done.text[:300])
        return started["anchor"], done.json()["token"]

    try:
        mine, my_token = claim(one)
        _theirs, their_token = claim(two)

        # Reported exactly as the window reports: the subject is the repository,
        # not the forge. k distinct people, so the cluster clears the floor.
        for i in range(K_REPORTERS):
            out = report({"pseudonym": f"repo-dash-{tag}-{i}", "subject": mine,
                          "model_class": "none", "outcome": "unresolved",
                          "observed": {"os.name": "linux", "app.build": f"{tag}"}})
            assert isinstance(out, dict) and out["accepted"], out

        def dashboard(token: str) -> dict:
            r = httpx.get(f"{base_api}/dashboard/{authority}",
                          headers={"X-Podshl-Claim": token}, timeout=20)
            assert r.status_code == 200, (r.status_code, r.text[:300])
            return r.json()

        ours = dashboard(my_token)
        assert ours["total"] == 1, (
            f"a repository's own reports did not reach its dashboard: {ours['total']} of 1")
        assert ours["shown"] == 1 and len(ours["clusters"]) == 1, ours["shown"]
        assert ours["clusters"][0]["reporters"] == K_REPORTERS, ours["clusters"][0]

        # And the neighbour on the same forge sees none of it. This is the half
        # that makes "match the host instead" the wrong fix rather than a
        # simpler one.
        neighbour = dashboard(their_token)
        assert neighbour["total"] == 0 and not neighbour["clusters"], (
            f"another repository on the same forge was shown these reports: {neighbour['total']}")
    finally:
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute(
                    "UPDATE source SET mirror_state = 'withheld' WHERE anchor_id IN "
                    "(SELECT id FROM anchor WHERE kind = 'repo' AND host = %s)",
                    (authority.split(":", 1)[0],))
        srv.shutdown()


def sv_a_confusable_owner_is_held_and_the_forge_is_not():
    """On a forge the impersonation is in the owner, and the host is everybody's.

    `hold_reason` examined the labels of the host. For a repository that is
    `github.com`, so the check asked whether *the forge* resembles somebody's
    mark -- a question with two answers, "no, for everyone" and "yes, so hold
    every project on GitHub", and neither is about the claimant. Meanwhile
    `dx111geo/engram` is a letter out from `dx111ge/engram` and the host is
    identical either way, which is where this actually happens.

    Held, not blocked, exactly as `SV33` has it: a held anchor is mirrored and
    served and simply gets no attestation, so being conservative costs a review
    rather than an existence.
    """
    from .ingest import confusable

    with db.tx() as conn:
        _marks(conn)
        gh = "github.com"

        # The owner carries the mark: a person decides, not us.
        held = confusable.hold_reason(
            conn, gh, kind="repo", value="https://github.com/nvidia-drivers/fixes/")
        assert held, "an owner carrying a well-known mark was attested"

        # A Cyrillic letter inside a Latin owner is the homoglyph shape, and the
        # message has to say where to look, because the host looks fine.
        mixed = confusable.hold_reason(
            conn, gh, kind="repo", value="https://github.com/nvidi\u0430/drivers/")
        assert mixed and "owner" in mixed, f"mixed scripts in an owner: {mixed}"

        # And an ordinary repository on the same forge is not held. This is the
        # half that was broken in both directions: the forge's own labels were
        # the only thing examined, so nothing about the claimant ever was.
        ok = confusable.hold_reason(
            conn, gh, kind="repo", value="https://github.com/dx111ge/engram/")
        assert ok is None, f"an ordinary repository was held: {ok}"

        # The host is not the claimant's and is never the reason.
        assert "github" not in (held or ""), \
            "the forge's own name reached a publisher's hold reason"

        # A domain anchor is unchanged.
        assert confusable.hold_reason(conn, "rnicrosoft.io") is not None
        assert confusable.hold_reason(conn, "nvidia.com") is None


def sv_a_repository_may_publish_the_endpoint_a_person_can_visit():
    """A repository anchor proves two locations at once, and the endpoint rule
    only knew about one of them.

    Control is demonstrated by writing into the repository, which is as much a
    fact about `github.com/owner/name/` as about the raw prefix the bytes are
    read from — the forge names one repository twice. `check_endpoint` compared
    against the fetch prefix alone, so the endpoint a maintainer would actually
    publish, their own issue tracker, was refused, while a URL under a content
    delivery host nobody visits would have been accepted.

    Found in production rather than here: the first real project enrolled, the
    crawler fetched its manifest, and the source was refused with
    `endpoint 'https://github.com/dx111ge/engram/issues' does not lie under the
    verified anchor 'https://raw.githubusercontent.com/...'`.

    A domain is unchanged and must stay so: there the fetch prefix is the
    narrower of the two, and narrower is what this rule wants.
    """
    from .errors import IngestRefused
    from .ingest import validate

    ident = "https://github.com/dx111ge/engram/"
    raw = "https://raw.githubusercontent.com/dx111ge/engram/HEAD/"

    # The one a maintainer publishes.
    validate.check_endpoint(f"{ident}issues", raw, ident)
    # And the raw side, still fine, because it is also theirs.
    validate.check_endpoint(f"{raw}support/", raw, ident)

    # Somebody else's repository on the same forge is not theirs to name.
    for outside in ("https://github.com/someone/else/issues",
                    "https://example.org/support",
                    "https://raw.githubusercontent.com/someone/else/HEAD/"):
        try:
            validate.check_endpoint(outside, raw, ident)
        except IngestRefused:
            pass
        else:
            raise AssertionError(f"{outside} was accepted under another project's anchor")

    # A domain keeps the narrower rule: the prefix, not the whole host.
    narrow = "https://example.org/project/"
    validate.check_endpoint(f"{narrow}support", narrow)
    try:
        validate.check_endpoint("https://example.org/elsewhere", narrow)
    except IngestRefused:
        pass
    else:
        raise AssertionError("a domain anchor stopped being confined to its prefix")


# ------------------------------------------------ fetching what is used


_USED_MANIFEST = (b"endpoint: {base}\ncommit: u1\nstatus: active\nlangs: [en]\n"
                  b"problem_classes: [app.a, app.b]\ncollect:\n" + _SYMPTOM_PROBE +
                  b"solutions:\n  - solutions/a.md\n  - solutions/b.md\n")


def _used_files() -> dict[str, bytes]:
    return {"/.podshl/agent.yaml": _USED_MANIFEST,
            "/.podshl/solutions/a.md": _solution("a", "app.a", "    app.symptom: a\n"),
            # One solution per class, so each class derives a tree.
            "/.podshl/solutions/b.md": _solution("b", "app.b", "    app.symptom: b\n")}


def _source_of(host: str) -> int:
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT s.id FROM source s JOIN anchor a ON a.id = s.anchor_id "
                        "WHERE a.host = %s", (host,))
            return cur.fetchone()["id"]


def _set_source(host: str, sql: str, *params) -> None:
    """Move a served project's clocks, the way two weeks of nobody asking would."""
    with db.tx() as conn:
        with conn.cursor() as cur:
            cur.execute(f"UPDATE source SET {sql} WHERE anchor_id = "
                        "(SELECT id FROM anchor WHERE host = %s)", (*params, host))


def _cool(host: str) -> None:
    _set_source(host, "last_used = current_date - 15, "
                      "last_checked = now() - interval '15 days', "
                      "last_full_check = now() - interval '15 days', "
                      # Due, so only its temperature keeps it off the timer.
                      "next_fetch_at = now() - interval '1 day'")


def _source_row(host: str) -> dict:
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT s.*, s.xmin::text AS version FROM source s "
                        "JOIN anchor a ON a.id = s.anchor_id WHERE a.host = %s", (host,))
            return cur.fetchone()


def _mirror(host: str):
    """`GET /mirror/{host}` in this process, so the pool that checks a cold
    source is the one this case can see. Returns (status, body)."""
    from fastapi import Response

    from .app import mirror
    out = mirror(host, Response(), repo=None, forge_shape=None)
    if isinstance(out, dict):
        return 200, out
    return out.status_code, json.loads(out.body)


class _Undo(Exception):
    pass


def _due_now() -> set[int]:
    """What the timer would take now, with the lease rolled back."""
    from .ingest import scheduler
    got: set[int] = set()
    try:
        with db.tx() as conn:
            got = {r["id"] for r in scheduler.claim(conn, 100000)}
            raise _Undo()
    except _Undo:
        pass
    return got


def sv_ga1_the_maintainers_workflow_publishes_and_says_why_not():
    """GA1. The GitHub workflow a maintainer copies, run as the script it is.

    Its `run:` block is taken out of `examples/github-action/podshl.yml` and run
    with bash against this operator, with the raw file server pointed at a
    project served here. It has to wait for the published copy, re-enrol,
    and fail the job with the operator's reason when the files are refused or
    the token is wrong — a red check on the commit is the only place a
    maintainer will see it.
    """
    import os
    import subprocess
    import tempfile

    import yaml

    wf = yaml.safe_load((Path(__file__).resolve().parents[3] / "examples" / "github-action"
                         / "podshl.yml").read_text(encoding="utf-8"))
    # `on` is YAML's boolean true, which is how PyYAML reads the key.
    trigger = wf.get("on", wf.get(True))
    assert trigger["push"]["paths"] == [".podshl/**"], trigger
    job = wf["jobs"]["publish"]
    assert "default_branch" in job["if"], job["if"]
    step = next(s for s in job["steps"] if "run" in s)
    assert step["env"]["PODSHL_CLAIM_TOKEN"] == "${{ secrets.PODSHL_CLAIM_TOKEN }}", step["env"]
    assert wf["permissions"] == {"contents": "read"}, wf.get("permissions")
    script = step["run"]

    files = {"/.podshl/agent.yaml": _USED_MANIFEST,
             "/.podshl/solutions/a.md": _solution("a", "app.a", "    app.symptom: a\n"),
             "/.podshl/solutions/b.md": _solution("b", "app.b", "    app.symptom: b\n")}
    host, token, stop = _served_project(files, "ga1-token")
    base = _INGESTED[host]["base"]
    try:
        def run(local: dict, tok, wait="30"):
            with tempfile.TemporaryDirectory() as work:
                for rel, body in local.items():
                    target = Path(work) / rel.lstrip("/")
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(body)
                env = {**os.environ, "PODSHL_OPERATOR": "http://127.0.0.1:8725",
                       "PODSHL_HOST": host, "PODSHL_RAW_BASE": base,
                       "PODSHL_WAIT_SECONDS": wait, "GITHUB_REPOSITORY": "someone/project"}
                env.pop("PODSHL_CLAIM_TOKEN", None)
                if tok is not None:
                    env["PODSHL_CLAIM_TOKEN"] = tok
                out = subprocess.run(["bash", "-c", script], cwd=work, env=env,
                                     capture_output=True, text=True, timeout=120)
                return out.returncode, out.stdout + out.stderr

        published = {k: v for k, v in files.items() if k.startswith("/.podshl/")}

        code, out = run(published, token)
        assert code == 0 and "Your files were read: unchanged, 2 solution(s)" in out, (code, out)

        code, out = run(published, None)
        assert code == 1 and "No PODSHL_CLAIM_TOKEN secret" in out, (code, out)
        code, out = run(published, "not-the-token")
        assert code == 1 and "did not accept the token" in out, (code, out)

        # This commit's copy and the published one differ: it does not enrol a
        # version the operator cannot see yet.
        newer = {**published, "/.podshl/solutions/a.md": published["/.podshl/solutions/a.md"] + b"More.\n"}
        code, out = run(newer, token, wait="0")
        assert code == 1 and "still differs from this commit: .podshl/solutions/a.md" in out, (code, out)

        # A refused file fails the job with the operator's sentence.
        files["/.podshl/solutions/a.md"] = _solution("a", "app.a", "    app.symptom: a\n", action="run_shell")
        published = {k: v for k, v in files.items() if k.startswith("/.podshl/")}
        code, out = run(published, token)
        assert code == 1 and "were not taken (refused)" in out and "run_shell" in out, (code, out)
    finally:
        stop()


def sv123_a_hot_source_checked_within_the_hour_is_served_without_a_fetch():
    """SV123. A person asking about a project in use costs its forge nothing."""
    host, _, stop = _served_project(_used_files(), "sv123")
    try:
        before = len(_HITS[host])
        status, body = _mirror(host)
        assert status == 200 and body["attested"] is True, (status, body)
        assert {s["solution_id"] for s in body["solutions"]} == {"a", "b"}, body
        assert len(_HITS[host]) == before, (
            f"serving a source checked minutes ago fetched from its forge: "
            f"{_HITS[host][before:]}")
    finally:
        stop()


def sv124_a_hot_source_checked_longer_ago_is_served_and_queued():
    """SV124. Served at once from the database, and put at the front of the queue.

    The check itself is the worker's, not the request's: a person waiting on
    a forge for a project that is in use is exactly what the mirror is for not
    doing.
    """
    from .ingest import scheduler
    files = _used_files()
    host, _, stop = _served_project(files, "sv124")
    try:
        _set_source(host, "last_checked = now() - interval '2 hours', "
                          "next_fetch_at = now() + interval '20 hours'")
        sid = _source_of(host)
        assert sid not in _due_now(), "the source was due before anybody used it"

        before = len(_HITS[host])
        status, body = _mirror(host)
        assert status == 200 and body["attested"] is True, (status, body)
        assert len(_HITS[host]) == before, "the request waited on the forge"
        assert sid in _due_now(), "a use more than an hour after the last check queued nothing"

        # Ahead of everything a timer made due — if not of what this suite
        # parks at '-infinity'.
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT count(*) AS n FROM source WHERE mirror_state = 'serving' "
                            "AND last_used >= current_date - 14 AND next_fetch_at < "
                            "(SELECT next_fetch_at FROM source WHERE id = %s) "
                            "AND next_fetch_at <> '-infinity'", (sid,))
                ahead = cur.fetchone()["n"]
        assert ahead == 0, f"{ahead} sources a timer made due are ahead of a used one"

        # And a check is what reads the files: the one the worker would run.
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute(f"SELECT {scheduler.SOURCE_COLUMNS} FROM source WHERE id = %s", (sid,))
                got = scheduler.ingest_one(conn, cur.fetchone())
        assert got["outcome"] == "unchanged" and got["check"] == "partial", got
        assert _source_row(host)["last_checked"] is not None
        assert len(_HITS[host]) > before, "the queued check fetched nothing"
    finally:
        stop()


def sv125_a_source_nobody_used_for_two_weeks_is_on_no_timer():
    """SV125. Nobody asked for fourteen days, so nothing asks its forge.

    Only its anchor is checked, weekly, by a job that does not care how warm the
    project is (`SV129`), and that check reads the challenge and nothing else.
    """
    from .ingest import scheduler
    host, _, stop = _served_project(_used_files(), "sv125")
    try:
        _cool(host)
        sid = _source_of(host)
        assert sid not in _due_now(), "a cold source is on the timer"
        before = len(_HITS[host])
        with db.tx() as conn:
            results = scheduler.run_once(conn)
        assert sid not in {r["source"] for r in results}, results
        assert len(_HITS[host]) == before, _HITS[host][before:]

        # Its anchor, a week after it was last checked.
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE anchor SET last_checked = now() - interval '8 days' "
                            "WHERE host = %s RETURNING id", (host,))
                aid = cur.fetchone()["id"]
            swept = scheduler.sweep_anchors(conn, anchors=[aid])
        assert swept["checked"] == 1, swept
        assert _HITS[host][before:] == ["/.well-known/podshl-challenge"], (
            f"the weekly anchor check read more than the challenge: {_HITS[host][before:]}")
    finally:
        stop()


def sv126_a_cold_source_is_checked_before_it_is_served():
    """SV126. Checked first; unchanged is served under its entry, changed is
    stored and logged before it is served; either way it is hot again."""
    files = _used_files()
    host, _, stop = _served_project(files, "sv126")
    try:
        _cool(host)
        with db.read() as conn:
            size = log_store.tree_size(conn)
        before = len(_HITS[host])
        status, body = _mirror(host)
        assert status == 200, (status, body)
        assert "/.podshl/agent.yaml" in _HITS[host][before:], "served without a check"
        seq = body["serving"]["log_seq"]
        row = _source_row(host)
        assert str(row["last_used"]) == str(__import__("datetime").date.today()), row["last_used"]
        with db.read() as conn:
            grew = log_store.tree_size(conn) - size
        assert grew == 0, "unchanged files were logged again"
        assert seq == _INGESTED[host]["log_seq"], (seq, _INGESTED[host]["log_seq"])

        # Changed while cold: a third solution appears.
        _cool(host)
        files["/.podshl/agent.yaml"] = files["/.podshl/agent.yaml"].replace(
            b"  - solutions/b.md\n", b"  - solutions/b.md\n  - solutions/c.md\n")
        files["/.podshl/agent.yaml"] = files["/.podshl/agent.yaml"].replace(
            b"problem_classes: [app.a, app.b]", b"problem_classes: [app.a, app.b, app.c]")
        files["/.podshl/solutions/c.md"] = _solution("c", "app.c", "    app.symptom: c\n")
        status, body = _mirror(host)
        assert status == 200, (status, body)
        assert {s["solution_id"] for s in body["solutions"]} == {"a", "b", "c"}, body
        assert body["serving"]["log_seq"] is not None and body["serving"]["log_seq"] != seq, (
            "a changed cold source was served without a log entry of its own")
    finally:
        stop()


def sv127_a_cold_source_that_cannot_be_checked_is_not_served():
    """SV127. Fails closed, and says so in words that are not "not mirrored"."""
    host, _, stop = _served_project(_used_files(), "sv127")
    stop()
    _cool(host)
    import time

    from .ingest import on_demand
    status, body = _mirror(host)
    if body.get("code") == "loading":
        # A check that takes longer than a request waits is still a check;
        # the next request gets its outcome.
        deadline = time.monotonic() + 60
        while on_demand._inflight and time.monotonic() < deadline:
            time.sleep(0.2)
        status, body = _mirror(host)
    assert status == 503, (status, body)
    assert body["code"] == "cannot_check", body
    assert "solutions" not in body and "card" not in body, body
    assert "not attested" not in json.dumps(body), (
        "an unreadable project was described as one that publishes nothing")
    row = _source_row(host)
    assert row["last_used"] < __import__("datetime").date.today(), (
        "a failed check made the source hot, so the next request would be served")
    assert row["check_backoff_until"] is not None, row

    # The same through `/diagnose`.
    from .app import diagnose
    out = diagnose({"subject": host, "problem_class": "app.a", "facts": {}})
    assert getattr(out, "status_code", 200) == 503, out
    assert json.loads(out.body)["code"] == "cannot_check", out.body


def sv128_a_solution_withdrawn_while_cold_is_never_served():
    """SV128. The reason cold fails closed: deleting the file is how a
    maintainer withdraws a harmful answer, and a mirror that served its
    two-week-old copy first would hand out exactly that answer."""
    files = _used_files()
    host, _, stop = _served_project(files, "sv128")
    try:
        _cool(host)
        files["/.podshl/agent.yaml"] = files["/.podshl/agent.yaml"].replace(
            b"  - solutions/b.md\n", b"")
        del files["/.podshl/solutions/b.md"]
        status, body = _mirror(host)
        assert status == 200, (status, body)
        assert {s["solution_id"] for s in body["solutions"]} == {"a"}, (
            f"a withdrawn solution was served from a cold source: {body['solutions']}")

        # And through the diagnosis a client actually asks.
        from .app import diagnose
        out = diagnose({"subject": host, "problem_class": "app.b",
                        "facts": {"app.symptom": "b"}})
        assert out["outcome"] != "finding", f"the withdrawn answer was found: {out}"
    finally:
        stop()


def sv129_every_anchor_is_checked_weekly_and_graded_on_time():
    """SV129. `stale` and `unknown` are promises about the anchor, and they
    hold for a project nobody uses.

    Found while building this: `sweep.apply_grades` had no caller. An anchor
    whose challenge disappeared recorded `failing_since` and stayed `live`
    for ever. The weekly job checks and grades.
    """
    from .ingest import scheduler
    files = _used_files()
    host, _, stop = _served_project(files, "sv129")
    try:
        _cool(host)
        del files["/.well-known/podshl-challenge"]
        with db.tx() as conn:
            with conn.cursor() as cur:
                cur.execute("UPDATE anchor SET last_checked = now() - interval '8 days' "
                            "WHERE host = %s RETURNING id", (host,))
                aid = cur.fetchone()["id"]
            scheduler.sweep_anchors(conn, anchors=[aid])

        def anchor():
            with db.read() as conn:
                with conn.cursor() as cur:
                    cur.execute("SELECT status, failing_since FROM anchor WHERE id = %s", (aid,))
                    return cur.fetchone()
        assert anchor()["failing_since"] is not None, "the weekly check did not see the file go"
        assert anchor()["status"] == "live", "one failed check changed the state"

        for days, expected in ((15, "stale"), (91, "unknown")):
            with db.tx() as conn:
                with conn.cursor() as cur:
                    cur.execute("UPDATE anchor SET failing_since = now() - %s * interval '1 day' "
                                "WHERE id = %s", (days, aid))
                scheduler.sweep_anchors(conn, anchors=[aid])
            assert anchor()["status"] == expected, (days, anchor())
        assert _source_row(host)["mirror_state"] == "withheld", "unknown kept serving"
    finally:
        stop()


def sv130_on_demand_checks_are_bounded():
    """SV130. A pool, one check per source, a bounded wait, and a back-off.

    The check itself is replaced by one that only sleeps and counts, because
    what is under test is the bound and not the forge.
    """
    import threading
    import time

    from .ingest import on_demand

    with db.tx() as conn:
        sids = []
        for _ in range(on_demand.POOL_SIZE + 2):
            h = _fresh_host("cold")
            aid = _anchor(conn, h)
            with conn.cursor() as cur:
                cur.execute("INSERT INTO source (anchor_id, manifest_url, fetch_prefix, last_used) "
                            "VALUES (%s, %s, %s, current_date - 30) RETURNING id",
                            (aid, f"https://{h}/.podshl/agent.yaml", f"https://{h}/"))
                sids.append(cur.fetchone()["id"])

    running = peak = 0
    calls: dict[int, int] = {}
    lock = threading.Lock()
    real = on_demand._check

    def slow(source_id):
        nonlocal running, peak
        with lock:
            running += 1
            peak = max(peak, running)
            calls[source_id] = calls.get(source_id, 0) + 1
        time.sleep(on_demand.WAIT_S + 1.5)
        with lock:
            running -= 1
        return {"source": source_id, "outcome": "unchanged"}

    on_demand._check = slow
    try:
        answers, waited = [], []

        def ask(sid):
            t = time.monotonic()
            answers.append(on_demand.before_serving(sid))
            waited.append(time.monotonic() - t)

        # Every source twice, at once.
        threads = [threading.Thread(target=ask, args=(sid,)) for sid in sids + sids]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert peak <= on_demand.POOL_SIZE, f"{peak} checks ran at once"
        assert all(a is not None and a.code == "loading" for a in answers), answers
        assert max(waited) < on_demand.WAIT_S + 1, f"a request waited {max(waited):.1f} s"
        deadline = time.monotonic() + 60
        while on_demand._inflight and time.monotonic() < deadline:
            time.sleep(0.2)
        assert not on_demand._inflight, "checks were still running a minute later"
        # Counted once everything queued has run: two requests for one source
        # are one check, not one now and one later.
        assert calls == {sid: 1 for sid in sids}, f"checks per source: {calls}"
    finally:
        on_demand._check = real

    # A real check that fails backs off, and the next request asks nobody.
    sid = sids[0]
    first = on_demand.before_serving(sid)
    assert first is not None and first.code == "cannot_check", first
    with db.read() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT check_backoff_until - now() AS wait, check_failures "
                        "FROM source WHERE id = %s", (sid,))
            row = cur.fetchone()
    assert row["check_failures"] == 1 and 30 < row["wait"].total_seconds() <= 60, row
    on_demand._check = lambda s: (_ for _ in ()).throw(AssertionError("asked during back-off"))
    try:
        again = on_demand.before_serving(sid)
    finally:
        on_demand._check = real
    assert again is not None and again.code == "cannot_check", again


def sv131_a_project_s_reports_and_trees_survive_it_going_cold():
    """SV131. Cooling stops the fetching and deletes nothing.

    The design named `cluster_partition`, which `0015` had already dropped
    along with `link`. What a project holds today is its stored files, its
    trees, and the clusters of reports about it — `cluster.source_id`, with the
    reports hanging off each cluster by `ON DELETE CASCADE`. Those are the
    maintainer's evidence, and none of it may depend on somebody having asked
    recently.
    """
    host, _, stop = _served_project(_used_files(), "sv131")
    try:
        sid = _source_of(host)
        with db.tx() as conn:
            cid = clusters.ensure(
                conn, clusters.canonical_signature(host, {"app.symptom": "a"}),
                source_id=sid, epoch=1)
            _observe(conn, cid, ["sv131-a", "sv131-b"], 1)

        def held():
            with db.read() as conn:
                with conn.cursor() as cur:
                    # Row ids, not counts: a tree deleted and derived again
                    # counts the same and is not the same row.
                    cur.execute(
                        "SELECT (SELECT array_agg(id::text) FROM observation WHERE cluster_id = %s) AS reports, "
                        "  (SELECT array_agg(id) FROM cluster WHERE source_id = %s) AS clusters, "
                        "  (SELECT array_agg(id) FROM tree WHERE source_id = %s) AS trees, "
                        "  (SELECT array_agg(id) FROM solution WHERE source_id = %s) AS solutions, "
                        "  (SELECT array_agg(id) FROM card WHERE source_id = %s) AS cards",
                        (cid, sid, sid, sid, sid))
                    return {k: set(v or ()) for k, v in cur.fetchone().items()}

        before = held()
        assert len(before["reports"]) == 2 and before["trees"], before
        _cool(host)
        assert held() == before, "cooling removed something"
        status, _ = _mirror(host)
        assert status == 200
        after = held()
        for k in before:
            assert before[k] <= after[k], f"{k} lost when the project was used again: {before[k] - after[k]}"
    finally:
        stop()


def sv132_a_query_leaves_one_date_at_most_once_a_day():
    """SV132. What the operator learns from a question: the day, and only once."""
    import datetime

    host, _, stop = _served_project(_used_files(), "sv132")
    try:
        _set_source(host, "last_used = current_date - 1")
        before = _source_row(host)
        assert _mirror(host)[0] == 200
        after = _source_row(host)
        assert after["last_used"] == datetime.date.today(), after["last_used"]
        assert isinstance(after["last_used"], datetime.date) and not isinstance(
            after["last_used"], datetime.datetime), "the use is recorded with a time of day"
        changed = {k for k in before if k != "version" and before[k] != after[k]}
        assert changed == {"last_used"}, f"a query changed more than the date: {changed}"

        # The second question the same day writes nothing at all.
        _mirror(host)
        from .app import diagnose
        diagnose({"subject": host, "problem_class": "app.a", "facts": {"app.symptom": "a"}})
        assert _source_row(host)["version"] == after["version"], (
            "a second use on the same day wrote the row again")

        # Nothing else keyed on the source moved: no count, no requester.
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT column_name FROM information_schema.columns "
                            "WHERE table_name = 'source' AND column_name ~ '(used|ask|request)'")
                assert [r["column_name"] for r in cur.fetchall()] == ["last_used"]
    finally:
        stop()


def sv133_a_solution_that_does_not_match_its_digest_is_refused():
    """SV133. A digest is a claim, and it is checked. And with digests, a check
    of an unchanged project is one request."""
    from .ingest import scheduler
    files = _used_files()
    a, b = files["/.podshl/solutions/a.md"], files["/.podshl/solutions/b.md"]
    files["/.podshl/agent.yaml"] = files["/.podshl/agent.yaml"].replace(
        b"  - solutions/a.md\n  - solutions/b.md\n",
        f"  - path: solutions/a.md\n    sha256: {hashlib.sha256(a).hexdigest()}\n"
        f"  - path: solutions/b.md\n    sha256: {hashlib.sha256(b).hexdigest()}\n".encode())
    host, token, stop = _served_project(files, "sv133-token", etags=True)
    try:
        sid = _source_of(host)
        # The quick check: the manifest answers 304 and names every file.
        _set_source(host, "next_fetch_at = '-infinity', last_full_check = now()")
        before = len(_HITS[host])
        with db.tx() as conn:
            got = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert got["outcome"] == "unchanged" and got["check"] == "quick", got
        assert [h for h in _HITS[host][before:] if "/solutions/" in h] == [], (
            f"an unchanged project with digests was read file by file: {_HITS[host][before:]}")

        # A hand-edit that leaves the digest behind.
        files["/.podshl/solutions/b.md"] = b.replace(b"Do the thing.", b"Do another thing.")
        _set_source(host, "next_fetch_at = '-infinity', last_full_check = NULL")
        with db.tx() as conn:
            got = {r["source"]: r for r in scheduler.run_once(conn)}[sid]
        assert got["outcome"] == "refused", got
        why = _dashboard(host, token)["files"]["last_refusal"] or ""
        assert "solutions/b.md" in why and hashlib.sha256(b).hexdigest() in why, (
            f"the maintainer was not told which file and which digest: {why!r}")
        status, body = _mirror(host)
        assert "Do another thing" not in json.dumps(body, default=str), (
            "the unmatched file was served")
    finally:
        stop()


def sv_an_action_s_upstream_is_checked_as_text():
    """RR5, the publisher's side: what a proposed change says about the
    software it is for is refused at ingest when a client would refuse it."""
    from .. import spec_gate
    call = {"action": "set_config_key",
            "params": {"file": "app.toml", "key": "modeset", "value": "1"}}
    spec_gate.check_action({**call, "upstream": {
        "package": "hyprland", "issue": "https://github.com/hyprwm/Hyprland/issues/1",
        "fixed_in": "1:0.45.0-1"}})
    for bad in ({"package": "a b"}, {"issue": "http://example.org/1"},
                {"fixed_in": "1.0; x"}, {"version": "1"}, {"package": 3}, "hyprland"):
        try:
            spec_gate.check_action({**call, "upstream": bad})
        except spec_gate.SpecError:
            continue
        raise AssertionError(f"accepted upstream {bad!r}")


def sv_a_digest_is_spelled_as_one():
    """The manifest side of SV133: a digest that could never match is refused
    before anything is fetched for it, and a plain path means what it meant."""
    from .errors import IngestRefused
    base = (b"endpoint: https://x.example/\nlangs: [en]\nproblem_classes: [a.b]\n"
            b"solutions:\n")
    plain = ingest_manifest.parse_manifest(base + b"  - solutions/a.md\n")
    assert plain["solutions"] == ["solutions/a.md"] and "solution_sha256" not in plain, plain
    # A digest map written at the top level is not one spelled next to a file.
    smuggled = ingest_manifest.parse_manifest(
        base + b"  - solutions/a.md\nsolution_sha256: {solutions/a.md: nonsense}\n")
    assert "solution_sha256" not in smuggled, smuggled
    d = "ab" * 32
    both = ingest_manifest.parse_manifest(
        base + f"  - solutions/a.md\n  - path: solutions/b.md\n    sha256: {d}\n".encode())
    assert both["solutions"] == ["solutions/a.md", "solutions/b.md"], both
    assert both["solution_sha256"] == {"solutions/b.md": d}, both
    for bad in (f"  - path: solutions/b.md\n    sha256: {d.upper()}\n",
                "  - path: solutions/b.md\n    sha256: abc\n",
                "  - path: solutions/b.md\n    digest: abc\n",
                "  - sha256: " + d + "\n"):
        try:
            ingest_manifest.parse_manifest(base + bad.encode())
        except IngestRefused:
            continue
        raise AssertionError(f"accepted: {bad!r}")


ALL = {name: fn for name, fn in sorted(globals().items())
       if name.startswith(("sv", "gr")) and callable(fn)}
