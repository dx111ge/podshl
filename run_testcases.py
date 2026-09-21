#!/usr/bin/env python3
"""Execute the cases in TESTCASES.md that are marked `auto`.

    mise run testcases

Case ids map 1:1 to the table in TESTCASES.md, so a failure names the row it
broke. A list that claims coverage it does not have is worse than no list —
this is what stops that document being an assertion.

**Scope, stated rather than implied.** This exercises the **counterparty**: the
vendor agent, the plain website, the responsiveness index and the OSS project —
plus the conformance gate that holds a document to `spec/vocabulary/`. It no
longer exercises a client of its own. There used to be a second client here, in
Python, and two thirds of the suite pointed at it: green, plausible, and
measuring an implementation nobody installs. Client cases now delegate to
`cargo test` and are tagged `[rust]` in their description.

What is NOT covered anywhere is the Rust client end to end through its own
window. `cargo run -- demo` exercises the shipped binary without one, which
narrows that gap rather than closing it; the rows that remain uncovered are
marked `manual` or `open` and are not counted as passing.
"""
import json
import os
import re
import subprocess
import sys
import time
import traceback
from pathlib import Path

sys.path.insert(0, "src")
import httpx

from podshl import aggregate
from podshl.vendor import reports as vendor_reports
from podshl.vendor.catalog import CATALOG, MissingEnglish, content, generate, said, serve, triage

# The demo vendor's own words, from its content files: a case that checks what
# it says in German reads the German it wrote, rather than a copy of it here.
_ADVICE = content("de")["skills"]["accounting.booking.guidance"]
_RMA = content("de")["skills"]["warranty.rma.precheck"]
_SYMPTOM = _RMA["probes"]["symptom"]["choices"][1]

ACME, PLAIN, INDEX, OSS = (f"http://127.0.0.1:{p}" for p in (8721, 8722, 8723, 8727))
VAR = Path("var")

CASES = []


def case(cid, desc):
    def deco(fn):
        CASES.append((cid, desc, fn))
        return fn
    return deco


def card():
    return httpx.get(f"{ACME}/.well-known/agent-card.json", timeout=10).json()


def body_and_sig():
    c = card()
    return {k: v for k, v in c.items() if k != "signatures"}, c["signatures"][0]


# ------------------------------------------------------------------ discovery

@case("D1", "Signed card verifies; identity and LEI are exposed [rust]")
def _():
    cargo("flow::tests::a_signed_card_verifies_and_exposes_its_identity")


@case("D2", "No agent card is 'nobody there', not a trust failure [rust]")
def _():
    cargo("flow::tests::every_kind_of_absence_is_nobody_there")


@case("D3", "Unreachable host is the same class as D2 [rust]")
def _():
    cargo("flow::tests::every_kind_of_absence_is_nobody_there")


@case("D4", "Non-JSON at the well-known URI is treated as no agent [rust]")
def _():
    cargo("flow::tests::every_kind_of_absence_is_nobody_there")


@case("D5", "An HTTP 500 at the well-known URI is treated as no agent [rust]")
def _():
    cargo("flow::tests::every_kind_of_absence_is_nobody_there")


@case("D6", "Unsigned card is refused [rust]")
def _():
    cargo("jws::tests::a_card_without_a_signature_cannot_be_verified")


@case("D7", "Tampered card is refused [rust]")
def _():
    cargo("jws::tests::rejects_the_same_card_after_one_byte_changes")


@case("D8", "No out-of-band key means refusal, never a self-asserted card [rust]")
def _():
    cargo("flow::tests::without_an_out_of_band_key_a_card_is_refused_not_accepted")


@case("D9", "A different vendor's key does not verify [rust]")
def _():
    cargo("jws::tests::another_vendors_key_does_not_verify")


@case("D10", "A protected header declaring another alg is refused [rust]")
def _():
    cargo("jws::tests::a_header_declaring_another_algorithm_is_refused")

@case("S1", "A matching problem yields a skill with probes")
def _():
    s = triage("training is slow, bf16?", {})
    assert s and s.probes, "no skill or no probes"


@case("S2", "An unmatched problem routes to a human rather than shrugging [rust]")
def _():
    cargo("flow::tests::an_unmatched_problem_routes_to_a_human")


@case("S3", "A requirement the client cannot check is not a refusal [rust]")
def _():
    cargo("probes::tests::a_non_os_requirement_is_not_a_refusal")


@case("S4", "A skill for another OS is refused before probing [rust]")
def _():
    cargo("probes::tests::a_skill_for_another_os_is_refused_before_probing")

@case("P1", "Everything the catalogue offers here actually reads [rust]")
def _():
    cargo("reads::tests::everything_the_catalogue_offers_here_actually_reads")


@case("P2", "An unavailable machine probe arms the human probe instead of failing [rust]")
def _():
    cargo("flow::tests::an_unreadable_machine_fact_arms_a_question")

@case("C1", "What is not consented to is not read [rust]")
def _():
    cargo("flow::tests::what_is_not_consented_to_is_not_read")


@case("C2", "Planning a read does not perform it [rust]")
def _():
    cargo("flow::tests::planning_a_read_does_not_perform_it")

@case("A1", "A known action dry-runs and then executes [rust]")
def _():
    cargo("actions::tests::a_known_action_dry_runs_and_then_executes")


@case("A2", "An unknown action id is refused, naming the client's vocabulary [rust]")
def _():
    cargo("actions::tests::an_unknown_action_is_refused_naming_the_vocabulary")


@case("A3", "Path traversal in a parameter is refused [rust]")
def _():
    cargo("actions::tests::parameters_are_validated_before_anything_runs")


@case("A4", "Shell metacharacters in a parameter are refused [rust]")
def _():
    cargo("actions::tests::parameters_are_validated_before_anything_runs")


@case("A5", "An unexpected extra parameter is refused [rust]")
def _():
    cargo("actions::tests::parameters_are_validated_before_anything_runs")


@case("A6", "A missing required parameter is refused [rust]")
def _():
    cargo("actions::tests::parameters_are_validated_before_anything_runs")


@case("A7", "A mutating action leaves a rollback copy [rust]")
def _():
    cargo("actions::tests::a_mutating_action_leaves_a_rollback_copy")

@case("R6", "A finding never asserts a fact the endpoint did not have")
def _():
    """The advisory skill's answer says the path was resolved against the
    *installed* version rather than against the manual, which is its entire
    reason to exist. With no version read it formatted Python's `None` into
    that sentence and asserted it anyway.

    Shipping, and invisible: every fixture supplied a version, and the read is
    a file that exists only where the program is installed — so the branch was
    taken on every machine that is not a customer's, and on none that any test
    ran on. Found by walking the path in the window on this machine.

    The rule is general, so this checks the shape rather than the sentence: no
    finding may carry a language's null literal, whichever generator wrote it.
    """
    from podshl.vendor import catalog as _cat

    asked = {"question": "Which account for a supplier invoice?",
             "chart.confirmed": "SKR03"}
    r = generate(CATALOG["accounting.booking.guidance"], asked)
    assert not r.findings, (
        "an answer was given with no installed version read, and it claims to have "
        f"resolved against one: {[f.summary for f in r.findings]}")
    assert r.abstained and r.escalate, "no destination when the deciding fact is missing"

    # With the version present it answers, and says which version.
    ok = generate(CATALOG["accounting.booking.guidance"], {**asked, "app.version": "2026.3"})
    assert ok.findings and not ok.abstained, "a complete question was refused"
    assert "2026.3" in ok.findings[0].summary, ok.findings[0].summary

    # And no generator may put a null literal in front of a person, whatever
    # the facts are missing. `None`, `null` and `undefined` all read to a user
    # as the program admitting it lost track rather than as an answer.
    import re
    holes = re.compile(r"\b(None|null|undefined)\b")
    for sid, skill in CATALOG.items():
        for facts in ({}, asked, {"question": "x"}, {"symptom": "x"}, {"gpu.name": "RTX"}):
            rem = generate(skill, dict(facts))
            shown = [f.summary for f in rem.findings] + [t for t in (rem.abstain_reason,) if t]
            shown += [e for f in rem.findings for e in (f.evidence or [])]
            for line in shown:
                assert not holes.search(line), f"{sid} shows a null literal to a person: {line!r}"


@case("R1", "A validly signed remedy is accepted [rust]")
def _():
    cargo("flow::tests::a_matching_problem_yields_a_verified_remedy")


@case("R2", "A remedy is held to the same rules as the card [rust]")
def _():
    cargo("jws::tests::a_remedy_is_held_to_the_same_rules_as_the_card")


@case("R3", "Abstention produces no action and routes to a human")
def _():
    adv = CATALOG["accounting.booking.guidance"]
    r = generate(adv, {"question": f"how do I {_ADVICE['text']['refuse_words'][1]}",
                       "app.chart_of_accounts": "SKR04"})
    assert r.abstained and r.escalate, "no abstention or no destination"
    assert not r.plan, "an abstaining remedy still planned an action"


@case("R4", "A finding contradicting the vendor's own documentation is marked")
def _():
    s = CATALOG["torch.precision.consumer-gpu"]
    r = generate(s, {"gpu.name": "RTX 2070 SUPER", "gpu.compute_capability": "7.5",
                     "gpu.bf16_native": False})
    assert r.findings[0].contradicts_kb, "contradiction not flagged"
    assert s.static_kb_says, "no published text to contradict"


# ------------------------------------------------------------------ reporting

@case("T1", "Declining the report sends nothing [rust]")
def _():
    cargo("ui_contract::tests::declining_the_report_returns_before_anything_is_sent")


@case("T7", "The whole report path, end to end [rust]")
def _():
    cargo("flow::tests::a_report_is_built_stamped_and_accepted")


@case("T8", "A malformed report is not sent [rust]")
def _():
    cargo("flow::tests::a_malformed_report_is_not_sent")


@case("T2", "A known issue receives a receipt naming the fix")
def _():
    r = vendor_reports.receive({"skill_id": "torch.precision.consumer-gpu",
                                "resolved_by": "general_agent", "observed": {}, "failed_actions": []})
    assert r["state"] == "fixed_in" and "3.2.0" in r["message"], r


@case("T3", "A rare constellation is held below the k-anonymity threshold")
def _():
    uniq = {"skill_id": f"unique.{time.time()}", "observed": {"x": "y"},
            "failed_actions": [], "resolved_by": "human"}
    first = vendor_reports.receive(uniq)
    assert first.get("below_threshold"), "rare report was not held back"


@case("T3a", "One client reporting a combination five times is one reporter, at a vendor too")
def _():
    """The vendor's own report service counted submissions, so one client
    crossed the floor alone. It counts pseudonyms now — and a report without one
    cannot count at all, or leaving the field out would be the way across."""
    combo = {"skill_id": f"t3a.{time.time_ns()}", "observed": {"x": "y"},
             "failed_actions": [], "resolved_by": "human", "epoch": "2026-09"}
    for _ in range(vendor_reports.K_ANONYMITY + 2):
        again = vendor_reports.receive({**combo, "pseudonym": "the-same-client"})
    assert again.get("below_threshold") and again["reports"] == 1, (
        f"one client reporting repeatedly crossed the floor: {again}")

    for i in range(vendor_reports.K_ANONYMITY + 2):
        anon = vendor_reports.receive(dict(combo))
    assert anon.get("below_threshold") and anon["reports"] == 1, (
        f"reports without a pseudonym were counted as independent: {anon}")

    for i in range(1, vendor_reports.K_ANONYMITY):
        last = vendor_reports.receive({**combo, "pseudonym": f"client-{i}"})
    assert not last.get("below_threshold") and last["reports"] == vendor_reports.K_ANONYMITY, (
        f"five distinct reporters were not enough: {last}")


@case("T4", "A serial number and free text never travel [rust]")
def _():
    cargo("report::tests::a_serial_and_free_text_never_travel")


@case("T5", "Version numbers are coarsened [rust]")
def _():
    cargo("report::tests::version_numbers_are_coarsened")


@case("T6", "A report carries no timestamp and no identifier [rust]")
def _():
    cargo("report::tests::a_report_carries_no_timestamp_and_no_identifier")

@case("G1", "Below the contributor floor, no index is published")
def _():
    assert aggregate.publish([80, 80, 90])["published"] is False


@case("G2", "At the floor, median, spread and contributor count are published")
def _():
    r = aggregate.publish([80, 90, 70, 80, 60])
    assert r["published"] and r["rate_pct"] == 80 and r["contributors"] == 5, r


@case("G3", "Ballot stuffing cannot move the median and shows in the spread")
def _():
    r = aggregate.publish([0] * 7 + [100] * 3)
    assert r["rate_pct"] == 0, "forged contributions moved the median"
    assert r["spread_pct"] == [0, 100], "the attempt is not visible in the spread"


@case("G4", "A vendor that never acts loses the report button [rust]")
def _():
    cargo("ledger::tests::a_vendor_that_never_acts_loses_the_button")


@case("G6", "A vendor's standing is checked before the button is offered [rust]")
def _():
    cargo("ui_contract::tests::a_vendors_standing_is_checked_before_the_button_is_offered")


@case("G7", "Contributing to the index is its own decision [rust]")
def _():
    cargo("ui_contract::tests::contributing_to_the_index_is_asked_separately")


@case("G8", "Acknowledgement is not action [rust]")
def _():
    cargo("ledger::tests::only_a_state_that_changed_something_counts_as_acting")


@case("G9", "Too little evidence is stated as such, not quoted as a rate [rust]")
def _():
    cargo("ledger::tests::below_the_basis_it_says_so_rather_than_quoting_a_percentage")


@case("A10", "Dry-run text matches the actual effect [rust]")
def _():
    cargo("report::tests::the_agent_fixes_it_the_fix_holds_and_the_undo_puts_it_back")


@case("RR1", "A change is on record before it is made, written by code [rust]")
def _():
    cargo("repair::tests::the_record_is_written_before_the_change_and_by_code")


@case("RR2", "No record, no change [rust]")
def _():
    cargo("repair::tests::a_record_that_cannot_be_written_stops_the_change")


@case("RR3", "Looking again flags a change and never removes it [rust]")
def _():
    cargo("repair::tests::looking_again_flags_and_never_removes")


@case("RR4", "Versions are ordered as pacman orders them [rust]")
def _():
    cargo("repair::tests::versions_are_ordered_as_pacman_orders_them")


@case("RR5", "What a publisher says about the software is checked as text [rust]")
def _():
    cargo("repair::tests::a_publisher_s_upstream_is_checked_as_text")
    sv("sv_an_action_s_upstream_is_checked_as_text")


@case("RR6", "A file changed by another tool is recorded, watched and can go back [rust]")
def _():
    cargo("repair::tests::a_file_changed_elsewhere_is_recorded_watched_and_can_go_back")


@case("RR7", "A package held back is flagged when the official one moves on [rust]")
def _():
    cargo("repair::tests::a_package_held_back_is_flagged_when_the_official_one_moves_on")


@case("RR8", "An override is flagged when what it overrides changes [rust]")
def _():
    cargo("repair::tests::an_override_is_flagged_when_what_it_overrides_changes")


@case("RR9", "A watched issue is asked seldom, and a merged fix is followed to its release [rust]")
def _():
    cargo("repair::tests::a_watched_issue_is_asked_seldom_and_its_release_is_compared")
    cargo("upstream::tests::a_merged_pull_request_is_followed_to_the_release_that_contains_it")
    cargo("upstream::tests::only_a_github_issue_or_pull_link_is_looked_up")


@case("RR10", "The review after updates runs the review and nothing else [rust]")
def _():
    cargo("repairs_cli::tests::the_hook_runs_the_review_and_nothing_else")
    cargo("repairs_cli::tests::the_command_line_says_what_it_did_not_understand")


@case("RR11", "One record is said in the singular, in every language [rust]")
def _():
    cargo("repairs_cli::tests::one_record_is_said_in_the_singular_in_every_language")


@case("RR12", "The notification is PODSHL's own, and says so [rust]")
def _():
    cargo("repairs_cli::tests::the_windows_toast_is_shown_under_podshls_own_identity")


@case("RR16", "A record can be removed, and almost nothing can remove one [rust]")
def _():
    cargo("repairs_cli::tests::removing_a_record_needs_the_privilege_and_a_person")
    cargo("repairs_cli::tests::a_forgotten_record_leaves_a_stub_and_takes_its_copy")
    # As a process, because the property is about what the command refuses.
    cargo("forget_refuses_when_nobody_is_there_whatever_the_privilege", target="cli")


@case("RR14", "What the repair record did is in the log [rust]")
def _():
    cargo("what_the_repair_record_did_is_in_the_log", target="cli")


@case("RR15", "The uninstaller takes back what was registered outside the folder [rust]")
def _():
    cargo("repairs_cli::tests::the_uninstaller_takes_back_what_was_registered_outside_the_folder")


@case("RR13", "The one repairs command that writes a file says which [rust]")
def _():
    # As a process. A version of this beside the code built the sentence
    # itself, and passed with the printing taken out again.
    cargo("restore_says_which_file_it_put_back", target="cli")


@case("RR17", "A fix recorded by podshl-repairs is the client's record too [rust]")
def _():
    # Each step by the program that did not take the last one.
    cargo("a_fix_recorded_by_one_program_is_seen_by_the_other", target="cli")


@case("RR18", "podshl-repairs carries no window [rust]")
def _():
    # The built file: the libraries it needs on Linux, its subsystem on Windows.
    cargo("the_standalone_record_carries_no_window", target="cli")


@case("RR19", "The hook podshl-repairs writes is one it can run [rust]")
def _():
    # The printed line taken apart and run, not compared with a string.
    cargo("the_hook_podshl_repairs_writes_is_one_it_can_run", target="cli")


@case("RR20", "The notification names the program that raised it, and a click opens the review [rust]")
def _():
    cargo("repairs_cli::tests::the_notification_names_the_program_that_raised_it")


@case("RR24", "What a package or plugin declares becomes a record, attributed by the package manager [rust]")
def _():
    cargo("declared::tests::a_packages_declaration_becomes_a_record_that_follows_its_updates")
    cargo("declared::tests::an_omarchy_plugin_declares_like_a_package")
    cargo("declared::tests::a_broken_declaration_is_said_and_the_rest_still_taken")


@case("RR25", "A maintainer withdraws a declaration in an update, and the record says so once [rust]")
def _():
    cargo("declared::tests::a_maintainer_withdraws_a_declaration_and_the_record_says_so_once")


@case("RR26", "The man pages name every command, flag and declaration field [rust]")
def _():
    cargo("repairs_cli::tests::the_man_pages_name_every_command_flag_and_field")


@case("RR27", "show says why, who declared it, upstream, and whether there is a way back [rust]")
def _():
    cargo("repairs_cli::tests::show_says_why_who_upstream_and_whether_there_is_a_way_back")


@case("RR23", "The one-line install checks the download, installs it with its hook, and refuses a tampered one")
def _():
    # The release is served from a directory, the way GitHub lays it out
    # (download/v<version>/<file>), and the program is a stand-in that writes
    # down what it was asked — so what the installer set up is observable
    # without installing hooks on the machine running the suite.
    import functools, hashlib, http.server, shutil, socket, tempfile, threading
    root = Path(tempfile.mkdtemp(prefix="podshl-install-"))
    try:
        rel = root / "releases" / "download" / "v9.9.9"
        rel.mkdir(parents=True)
        name = "podshl-repairs-9.9.9-linux-x86_64"
        prog = rel / name
        prog.write_text('#!/bin/sh\necho "$@" >> "$HOME/asked"\n')
        good = prog.read_bytes()
        man = rel / "podshl-repairs.1"
        man.write_text(".TH PODSHL-REPAIRS 1\n")
        (rel / "SHA256SUMS").write_text(
            f"{hashlib.sha256(good).hexdigest()}  {name}\n"
            f"{hashlib.sha256(man.read_bytes()).hexdigest()}  podshl-repairs.1\n")
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(root))
        httpd = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        try:
            def install(home):
                home.mkdir(parents=True)
                env = {**os.environ, "HOME": str(home), "PODSHL_VERSION": "9.9.9",
                       "PODSHL_RELEASES": f"http://127.0.0.1:{port}/releases"}
                return subprocess.run(["bash", "packaging/repairs/install.sh"], env=env,
                                      capture_output=True, text=True, timeout=60)

            home = root / "home-a"
            r = install(home)
            assert r.returncode == 0, r.stdout + r.stderr
            placed = home / ".local" / "bin" / "podshl-repairs"
            assert placed.read_bytes() == good, "what was installed is not what was released"
            assert os.access(placed, os.X_OK), "installed without the executable bit"
            asked = (home / "asked").read_text()
            assert "install-hook" in asked, f"the update hook was not set up: {asked!r}"
            assert "checked against SHA256SUMS" in r.stdout, r.stdout
            page = home / ".local" / "share" / "man" / "man1" / "podshl-repairs.1"
            assert page.read_bytes() == man.read_bytes(), "the man page was not installed"

            # The same release with the program changed after the sums were
            # written: refused, and nothing left behind.
            prog.write_bytes(good + b"# changed\n")
            home = root / "home-b"
            r = install(home)
            assert r.returncode != 0, "a program that does not match its sums was installed"
            assert "does not match" in r.stderr, r.stderr
            assert not (home / ".local" / "bin" / "podshl-repairs").exists(), \
                "a refused program was left in ~/.local/bin"
        finally:
            httpd.shutdown()
    finally:
        shutil.rmtree(root, ignore_errors=True)


@case("RR21", "What a coding agent writes outside a repository is recorded, once per session [rust]")
def _():
    cargo("agent_hook::tests::an_agents_edit_outside_a_repository_is_recorded_once_per_session")
    cargo("agent_hook::tests::a_hook_call_it_cannot_read_is_an_error_not_a_stop")


@case("RR28", "What a coding agent changes with a shell command is recorded like its file writes [rust]")
def _():
    cargo("agent_hook::tests::an_agents_shell_command_is_recorded_like_its_file_writes")


@case("RR22", "Installing the agent hook keeps everything else in the agent's settings [rust]")
def _():
    cargo("agent_hook::tests::the_agents_settings_keep_everything_that_is_not_ours")


@case("RR29", "An agent nobody has walked records nothing, and can be measured instead [rust]")
def _():
    cargo("agent_hook::tests::an_unknown_agent_records_nothing_and_can_be_measured_instead")


@case("RR30", "A hook format read rather than walked says so in every record it makes [rust]")
def _():
    cargo("agent_hook::tests::a_format_that_was_read_rather_than_measured_says_so")


@case("A12", "The agent fixes it, the fix holds, and the undo puts it back [rust]")
def _():
    cargo("report::tests::the_agent_fixes_it_the_fix_holds_and_the_undo_puts_it_back")


@case("A11", "A sibling of the sandbox root is not inside it [rust]")
def _():
    cargo("actions::tests::a_sibling_of_the_root_is_not_inside_the_root")


@case("G5", "A contribution carries exactly one vendor and no identity")
def _():
    c = aggregate.contribution("ACME", 12, 10)
    assert set(c) == {"vendor", "rate_pct", "weight"}, c
    assert isinstance(c["vendor"], str), "more than one vendor in a contribution"


# ------------------------------------------------------- conformance to the spec

@case("SP1", "A proposed action outside the vocabulary is refused at the gate")
def _():
    from podshl import spec_gate
    spec_gate.check_action({"action": "set_config_key",
                            "params": {"file": "t.toml", "key": "vsync", "value": "on"}})
    for bad, why in (
        ({"action": "run_powershell", "params": {}}, "an invented action"),
        ({"action": "set_env_var", "params": {"name": "X", "value": "1"}},
         "an action one implementation carried and the other never did"),
        ({"action": "set_config_key", "params": {"file": "t.toml", "key": "vsync"}},
         "a missing required parameter"),
        ({"action": "set_config_key",
          "params": {"file": "t.toml", "key": "vsync", "value": "on", "extra": "1"}},
         "an undeclared parameter"),
        ({"action": "set_config_key",
          "params": {"file": "/etc/passwd", "key": "vsync", "value": "on"}},
         "a parameter that does not match its pattern"),
    ):
        try:
            spec_gate.check_action(bad)
        except spec_gate.SpecError as e:
            assert "vocabulary" in str(e) or "parameter" in str(e), str(e)
            continue
        raise AssertionError(f"accepted {why}: {bad}")


@case("SP2", "A read instruction outside the vocabulary is refused at the gate")
def _():
    from podshl import spec_gate
    spec_gate.check_read({"op": "run_tool", "tool": "nvidia-smi",
                          "args": ["--query-gpu=name", "--format=csv,noheader"]})
    for bad, why in (
        ({"op": "read_everything"}, "an invented op"),
        ({"op": "run_tool", "tool": "curl", "args": []}, "a tool off the allow list"),
        ({"op": "run_tool", "tool": "nvidia-smi", "args": ["; rm -rf /"]},
         "an argument outside the tool's pattern"),
        ({"op": "enumerate_read", "root": "anywhere", "glob": "*", "keys": []},
         "an unnamed root"),
        ({"op": "enumerate_read", "root": "config", "glob": "../*", "keys": []},
         "a traversing glob"),
        ({"op": "read_file_key", "path": ".ssh/id_ed25519", "key": "x"},
         "a denied path"),
    ):
        try:
            spec_gate.check_read(bad)
        except spec_gate.SpecError:
            continue
        raise AssertionError(f"accepted {why}: {bad}")


@case("LC1", "A head is held against the last one this device accepted [rust]")
def _():
    cargo("logproof::tests::a_head_is_held_against_the_last_one_this_client_accepted")
    cargo("logproof::tests::consistency_holds_for_every_prefix_and_nothing_else")


@case("LC2", "The operator's own consistency proofs verify in the client [rust]")
def _():
    cargo("logproof::tests::the_servers_own_consistency_proofs_verify_here")


@case("SP3", "The vendor's own skills conform to the published vocabulary")
def _():
    from podshl import spec_gate
    for skill_id, skill in CATALOG.items():
        for probe in skill.probes:
            if probe.read:
                spec_gate.check_read(probe.read)
    # The catalogue is the counterparty, so this is the CI half of the gate:
    # it fails a bad skill before it is ever served, which is a convenience.
    # The gate that matters runs at ingest, on somebody else's document.


@case("SP6", "The spec's normative claims hold against the implementations")
def _():
    from podshl import jcs, spec_gate
    spec = Path("spec/SPEC.md").read_text()

    # Floats are refused, and the spec says so because getting this wrong looks
    # like an attack rather than like a bug.
    assert "must be integers" in spec, "the spec no longer states the integer rule"
    try:
        jcs.canonicalize({"a": 1.5})
    except Exception:
        pass
    else:
        raise AssertionError("a float canonicalised — the spec's integer rule is false")

    # Every action and read op the spec names in prose must exist in the
    # normative vocabulary, or the two halves of spec/ disagree with each other.
    for action in spec_gate.actions():
        assert action in spec, f"{action} is in the vocabulary but not described in SPEC.md"

    # The declined convention is a wire contract, not a UI detail, so it has to
    # be spelled the same way in the spec and in the client.
    assert ".declined" in spec, "the spec no longer states the declined convention"
    ui = Path("client-rs/ui/index.html").read_text()
    assert '".declined"' in ui, "the client no longer writes <probe id>.declined"

    # The reply channels are bounded by the client; the spec must name the same
    # closed set rather than an aspirational one.
    handover = Path("client-rs/src/handover.rs").read_text()
    declared = re.search(r"REPLY_CHANNELS: &\[&str\] = &\[([^\]]*)\]", handover)
    assert declared, "handover.rs no longer declares REPLY_CHANNELS as a list of ids"
    for channel in ("email", "ticket_url", "none"):
        assert f'"{channel}"' in declared.group(1), f"{channel} is not a client reply channel"
        assert f"`{channel}`" in spec, f"{channel} is not named in SPEC.md"


@case("SP7", "The client's wire structs match the published schema [rust]")
def _():
    cargo("wire::tests::every_struct_matches_its_published_schema")


@case("SP8", "A field added by a vendor does not break the parse [rust]")
def _():
    cargo("wire::tests::an_unknown_field_does_not_break_the_parse")


@case("SP9", "A malformed remedy is refused even when it verifies [rust]")
def _():
    cargo("flow::tests::a_remedy_that_verifies_but_is_malformed_is_still_refused")


@case("SP4", "The client's action vocabulary matches the spec [rust]")
def _():
    cargo("actions::tests::vocabulary_matches_the_spec")


@case("SP5", "The client's read vocabulary matches the spec [rust]")
def _():
    cargo("reads::tests::vocabulary_matches_the_spec")


# ------------------------------------------------------------ interop, platform

#: The Rust suite, run once and read many times.
#:
#: Each `cargo(...)` delegation used to start its own `cargo test --exact`, and
#: there are 153 of them — so the client's suite was compiled-checked and
#: launched 154 times per run, at roughly one and a half cores and up to 830 MB
#: for four minutes. The property that bought was worth keeping and the price
#: was not: scoping to one test is what catches a *renamed* test, because
#: `cargo test --exact` exits 0 when its filter matches nothing.
#:
#: Both are kept by turning it around. The suite runs once per target, every
#: test's own result is read out of the output, and a case asserts that the test
#: it names ran and passed. A renamed test is not in the table, which is the
#: same catch by a cheaper route — and `RS1`'s "every test passes, named by a
#: case or not" is now the same single run rather than a 154th.
_RUST: dict[str, dict[str, str]] = {}

#: `test <name> ... ok`, as `libtest` prints it. Not `--quiet`, which prints
#: dots and would leave nothing to attribute to a case.
_RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.M)


def rust_results(target=None):
    """Every test in one target, by name, with what happened to it.

    With no target, "the client": its binary **and** its library. Since
    2026-09-19 the record of local fixes and what it stands on (`src/lib.rs`)
    is a library the client and `podshl-repairs` share, and a library's tests
    are a target of their own — `--bin podshl-client` alone would find
    `repair::tests::…` missing and fail every case that names one. The two are
    read into one table, because to a case they are one client; a name in both
    would make that table lie, so it is refused.
    """
    key = target or "bin"
    if key in _RUST:
        return _RUST[key]
    runs = [["--test", target]] if target else [["--lib"], ["--bin", "podshl-client"]]
    found, raw = {}, []
    for which in runs:
        out = subprocess.run(["cargo", "test", *which], cwd="client-rs",
                             capture_output=True, text=True, timeout=1800)
        these = {name: how for name, how in _RESULT.findall(out.stdout)}
        assert these, (
            f"the Rust target {' '.join(which)} reported no test at all. That is a build "
            f"failure or a changed output format, and either way nothing below has been "
            f"checked:\n" + (out.stdout[-2000:] + out.stderr[-2000:]))
        both = sorted(set(these) & set(found))
        assert not both, f"a test name is in the library and in the binary: {both}"
        found.update(these)
        raw.append(_rust_detail(out))
    _RUST[key] = found
    _RUST[key + ":raw"] = "\n".join(raw)
    return found


def _rust_detail(out):
    """What to show when a Rust test failed.

    Not `stdout + stderr` sliced from the end, which is what this was. `cargo`
    writes the compiler's output to stderr and `libtest` writes the panic to
    stdout, and the compiler's output is thousands of lines long — so the last
    2000 characters of the two concatenated were reliably 2000 characters of
    "Downloaded tokio v1.53.1". CI went red on 2026-09-15 with a failing case
    and no reason attached to it at all, and the reason was never in the log.
    A red suite that cannot say why teaches people to read past red.

    The panic is what somebody needs. The compiler's last words are kept after
    it, shorter, because sometimes the failure is a build.
    """
    panic = out.stdout
    cut = panic.find("\nfailures:\n")
    if cut != -1:
        panic = panic[cut:]
    return (panic[-3000:].strip()
            + "\n--- cargo's own output, last 800 characters ---\n"
            + out.stderr[-800:].strip())


def cargo(test_name, target=None):
    """Assert that one Rust test ran, and passed.

    Named for the case it satisfies. The test has to be *present*: a case whose
    test was renamed must fail, and it does — the name is simply not in the
    table the run produced.
    """
    results = rust_results(target)
    how = results.get(test_name)
    assert how is not None, (
        f"{test_name} is named by a case and does not exist in the "
        f"{target or 'podshl-client (library and binary)'} target. A renamed test "
        f"is an unrun case.")
    assert how == "ok", f"{test_name} {how}\n" + _RUST[(target or 'bin') + ':raw']


def cargo_all():
    """Every Rust test passes, whether or not a case names it.

    The `cargo(...)` delegations above are the documented cases, and they are
    not all of the tests. Dozens more exist that no case names — internal ones
    about canonicalisation, the ledger, the window contract — and for a long
    time nothing ran them from here at all, so a test could assert the wrong
    thing indefinitely and this suite would report green.

    That is not hypothetical. `report::tests::a_human_answer_from_a_fixed_list_
    does_travel` went on asserting that a bounded human answer travels in
    `observed` after the report deliberately started separating what a person
    supplied from what was measured. It was wrong for two commits and nothing
    here noticed, because no case named it.

    Not solved by demanding a case per test. Most of those are implementation
    detail rather than behaviour a document should promise, and forcing them
    into `TESTCASES.md` would make that document worse. Running all of them is
    the honest fix: a case may go unnamed, it may not go unrun.

    This reads the same run every other Rust case reads, so "all of them" is
    all of them rather than a second opinion from a second process.
    """
    for target in (None, "cli", "elevate_helper"):
        results = rust_results(target)
        bad = sorted(n for n, how in results.items() if how == "FAILED")
        assert not bad, (
            f"{len(bad)} Rust tests failed in {target or 'podshl-client'}: {', '.join(bad)}\n"
            + _RUST[(target or 'bin') + ':raw'])
        # A target that runs nothing is the failure the old guard was reaching
        # for and could not express: it tested the *string* "0 passed", which
        # a run of ten tests hides just as well as a run of none.
        assert len(results) > 0, f"{target or 'podshl-client'} ran no test"


PAGE_DIR = Path("src/podshl/server/pages")

#: Every page a human can reach. Written here as well as in `app.py` on purpose:
#: a page that stops being routed should fail rather than quietly disappear.
PAGES = ["/", "/publish", "/publish/build", "/register", "/dashboard", "/security",
         "/projects", "/log", "/notice", "/imprint"]


def withheld_documents() -> tuple[str, ...]:
    """What `scripts/release/sync_public.sh` refuses to publish, read from the script.

    One list, in the place that applies it. `P8` and `W9` each kept their own
    copy and the three drifted the first time the list grew — a file was
    withheld by the script and still treated as published by the cases, so the
    one that guards against naming an internal document failed for naming one.

    A parse that comes back short is a broken parse, not a short list: the same
    trap as `git ls-files` answering nothing inside the container, which made
    `P8` pass while proving nothing.

    **The public checkout has no list to read**, because the script is itself
    withheld — and publishing the list would publish the names it withholds.
    Both cases failed there on every run for that reason alone. There the
    answer is an empty list, and `unresolved_documents()` carries the check
    instead. The public checkout is recognised by the handover being absent as
    well; a working checkout that lost the script is still a failure.
    """
    script = Path("scripts/release/sync_public.sh")
    if not script.exists():
        assert not Path("internal/HANDOVER.md").exists(), (
            "the sync script is gone from a working checkout — the withheld "
            "list has nowhere to come from")
        return ()
    block = re.search(r'^INTERNAL="([^"]*)"', script.read_text(encoding="utf-8"), re.M)
    assert block, "the sync script has no INTERNAL list to read"
    entries = [line.strip() for line in block.group(1).splitlines() if line.strip()]
    # An entry is a file or a directory, and a directory withholds everything
    # under it — the same rule the script applies.
    names: list[str] = []
    for entry in entries:
        path = Path(entry)
        if path.is_dir():
            names += sorted(f.as_posix() for f in path.rglob("*") if f.is_file())
        else:
            names.append(path.as_posix())
    assert len(names) >= 6 and "internal/HANDOVER.md" in names, (
        f"the withheld list parsed as {names} — that is a broken parse, not a short list")
    return tuple(names)


def withheld_tokens(names) -> set[str]:
    """What a published text must not contain: each withheld path, and its file
    name, because a paragraph is as likely to say `HANDOVER.md` as the path."""
    return {n for name in names for n in (name, Path(name).name)}


#: Names a published file may mention although no file of that name is in the
#: tree, each with the reason. Anything else that looks like one of this
#: project's documents has to exist in the published tree.
DOCUMENTS_NOT_IN_THE_TREE = {
    "PLATFORM.md": "written next to each release's files by the release task",
}


def unresolved_documents(files, published_names):
    """Every `UPPER-CASE.md` a published file names that the published tree does
    not have.

    `withheld_documents()` can only say which names are internal where the list
    exists. This asks the question the other way round and needs no list: a
    reference to a document the reader cannot open is the failure whatever the
    reason, and it is checkable in the public checkout too. Measured there on
    2026-09-16: the only names it finds are the two the cases already allow.
    """
    pattern = re.compile(r"(?<![\w/.-])([A-Z][A-Z0-9_-]+\.md)\b")
    found = []
    for f in files:
        try:
            src = f.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for name in sorted(set(pattern.findall(src))):
            if name not in published_names and name not in DOCUMENTS_NOT_IN_THE_TREE:
                found.append((f, name))
    return found


def page_files():
    return sorted(PAGE_DIR.glob("*.html"))


def shared_scripts():
    """Scripts the pages load as files — `list.js`. Held to the same rules as a
    page's own blocks: they parse, and they write text, never markup."""
    return sorted(PAGE_DIR.glob("*.js"))


def scripts_parse(path, tag):
    """Only a JavaScript parser can say a file is a JavaScript program.

    Shared by `W3` and `W4` rather than written twice: the client's window and
    the operator's pages fail the same way — no handler binds, no label fills,
    and the layout comes up with every word missing while every check that reads
    the file as text keeps passing.
    """
    import re

    blocks = re.findall(r"<script>(.*?)</script>", path.read_text(encoding="utf-8"), re.S)
    # A page with no script is legitimate — the unconfigured imprint is static
    # text, because there is nothing to fetch when there is no operator. `SV75`
    # asserts such a page declares `script-src 'none'` rather than an empty one.
    if not blocks:
        return
    for i, block in enumerate(blocks):
        tmp = VAR / f"{tag}-{i}.js"
        tmp.parent.mkdir(parents=True, exist_ok=True)
        try:
            tmp.write_text(block, encoding="utf-8")
            out = subprocess.run(["node", "--check", str(tmp)],
                                 capture_output=True, text=True, timeout=120)
            assert out.returncode == 0, (
                f"{path} block {i} does not parse — " + out.stderr[:1000])
        finally:
            tmp.unlink(missing_ok=True)


@case("W10", "The server endpoints are configuration, read before they are used")
def _():
    """A released binary pinned to `127.0.0.1` can only ever talk to the machine
    it is running on, which is a shipping defect rather than a development one.

    Two properties, and the second is the one that is easy to lose: the values
    come from the environment, *and* they are read before anything in the boot
    block uses them. Position in the file cannot show the second — every handler
    that uses them is defined above the boot block and called after it — so the
    check is on the ordering inside that block.
    """
    cargo("ui_contract::tests::the_endpoints_are_configuration_and_are_read_before_they_are_used")


@case("W3", "The window's script parses")
def _():
    """The window is a JavaScript program, and only a JavaScript parser can say
    it is one.

    Every other check on this file reads it as text — `W1` matches command
    names, `W2` matches the applicability gate, `I2` matches language filenames
    — and all of them pass happily on a file that cannot run. A syntax error
    there is total: no handler binds, no label is filled, and the user gets a
    window with the layout drawn and every word missing. That is what shipped,
    and 201 cases stayed green through it.

    `node` is a test dependency here and not a build step. The window still has
    none, which is the point of it being one file.
    """
    import re

    window = Path("client-rs/ui/index.html")
    assert re.search(r"<script>", window.read_text(encoding="utf-8")), \
        "the window has no script at all, so there is nothing to parse"
    scripts_parse(window, "ui-syntax-check")


@case("DP1", "The deployment units start what exists, supervised, and nothing private listens publicly")
def _():
    """The ingest worker was a loop nobody supervised, and there was nothing
    to install on a host at all. The units in `deploy/systemd/` are checked
    against the code rather than trusted: every `module:object` they start has
    to import, every process restarts when it dies, both listeners are on
    loopback — the public one is reached only through the TLS proxy, and the
    operator's own view never — and the development-only loopback exception is
    set in none of them."""
    import importlib

    units = sorted(Path("deploy/systemd").glob("*.service"))
    names = {u.name for u in units}
    assert names == {"podshl-server.service", "podshl-ops.service", "podshl-ingest.service"}, names
    for unit in units:
        text = unit.read_text(encoding="utf-8")
        assert "Restart=always" in text, f"{unit.name} is not restarted when it dies"
        assert "PODSHL_ALLOW_LOOPBACK" not in text, f"{unit.name} enables the development loopback exception"
        assert "NoNewPrivileges=yes" in text and "ProtectSystem=strict" in text, f"{unit.name} is not confined"
        exec_line = next(l for l in text.splitlines() if l.startswith("ExecStart="))
        if "uvicorn" in exec_line:
            target = re.search(r"uvicorn (\S+):(\S+)", exec_line)
            module = importlib.import_module(target.group(1))
            assert hasattr(module, target.group(2)), f"{unit.name} starts {target.group(0)}, which does not exist"
            host = re.search(r"--host (\S+)", exec_line).group(1)
            assert host == "127.0.0.1", f"{unit.name} listens on {host}, not loopback"
        else:
            module = re.search(r"-m (\S+)", exec_line).group(1)
            assert hasattr(importlib.import_module(module), "main"), f"{unit.name} starts {module}, which has no main()"
    ops = Path("deploy/systemd/podshl-ops.service").read_text(encoding="utf-8")
    assert "ops_app:ops" in ops and "--port 8726" in ops, "the operator view is not the separate app on 8726"


@case("P3", "An answer that misses its declared pattern is asked again [rust]")
def _():
    cargo("ui_contract::tests::an_answer_that_misses_its_pattern_is_asked_again")


@case("AT1", "A published answer says what is attested about its publisher [rust]")
def _():
    cargo("ui_contract::tests::a_published_answer_says_what_is_attested_about_its_publisher")


@case("LG6", "The finding comes in the language the diagnosis asked for [rust]")
def _():
    cargo("flow::tests::the_finding_comes_in_the_language_asked_for")


@case("V4", "A vendor that is not a chip brand is not contradicted by the chip [rust]")
def _():
    cargo("vendors::tests::a_vendor_that_is_not_a_brand_is_not_contradicted_by_the_chip")
    cargo("ui_contract::tests::switching_vendor_ends_the_run")


@case("AT2", "The client proves a published answer's log entry itself [rust]")
def _():
    cargo("logproof::tests::inclusion_verifies_for_every_leaf_and_nothing_else")
    cargo("logproof::tests::a_real_entry_proves_against_the_signed_head")


@case("SV97", "A notice is recorded; a person decides, and can reverse it")
def _():
    sv("sv_a_notice_waits_for_a_person_and_a_decision_can_be_reversed")


@case("SV103", "The published monitor refuses a log it cannot verify")
def _():
    sv("sv_the_published_monitor_refuses_a_log_it_cannot_verify")


@case("SV104", "A guessed configuration says nothing about a project")
def _():
    sv("sv104_a_guessed_configuration_says_nothing_about_a_project")


@case("SV105", "What ingest could not make of the files reaches the maintainer")
def _():
    sv("sv105_what_ingest_could_not_make_of_the_files_reaches_the_maintainer")


@case("SV106", "The dashboard puts the work first, and says what it cut")
def _():
    sv("sv106_the_dashboard_puts_the_work_first")


@case("SV122", "A repository sees its own reports, and only its own")
def _():
    sv("sv122_a_repository_sees_its_own_reports_and_only_its_own")


@case("GA1", "The maintainers' GitHub workflow publishes, waits for the published copy, and fails with the reason")
def _():
    sv("sv_ga1_the_maintainers_workflow_publishes_and_says_why_not")


@case("SV123", "A hot source checked within the hour is served without a fetch")
def _():
    sv("sv123_a_hot_source_checked_within_the_hour_is_served_without_a_fetch")


@case("SV124", "A hot source checked longer ago is served at once and queued")
def _():
    sv("sv124_a_hot_source_checked_longer_ago_is_served_and_queued")


@case("SV125", "A source nobody used for two weeks is on no timer")
def _():
    sv("sv125_a_source_nobody_used_for_two_weeks_is_on_no_timer")


@case("SV126", "A cold source is checked before it is served")
def _():
    sv("sv126_a_cold_source_is_checked_before_it_is_served")


@case("SV127", "A cold source that cannot be checked is not served")
def _():
    sv("sv127_a_cold_source_that_cannot_be_checked_is_not_served")


@case("SV128", "A solution withdrawn while cold is never served")
def _():
    sv("sv128_a_solution_withdrawn_while_cold_is_never_served")


@case("SV129", "Every anchor is checked weekly and graded on time")
def _():
    sv("sv129_every_anchor_is_checked_weekly_and_graded_on_time")


@case("SV130", "On-demand checks are bounded")
def _():
    sv("sv130_on_demand_checks_are_bounded")


@case("SV131", "A project's reports and trees survive it going cold")
def _():
    sv("sv131_a_project_s_reports_and_trees_survive_it_going_cold")


@case("SV132", "A query leaves one date, at most once a day")
def _():
    sv("sv132_a_query_leaves_one_date_at_most_once_a_day")


@case("SV133", "A solution that does not match its manifest digest is refused")
def _():
    sv("sv_a_digest_is_spelled_as_one")
    sv("sv133_a_solution_that_does_not_match_its_digest_is_refused")


@case("SV120", "A solution that ignores a switch is reachable under every value of it")
def _():
    sv("sv120_a_solution_that_ignores_a_switch_is_reachable_under_every_value_of_it")


@case("SV111", "The privacy notice names its controller, and every page links it")
def _():
    sv("sv111_the_privacy_notice_names_its_controller_and_every_page_links_it")


@case("SV110", "Every past month's salt is destroyed without being asked")
def _():
    sv("sv110_every_past_months_salt_is_destroyed_without_being_asked")


@case("SV109", "The person names the symptom and the machine picks the fix")
def _():
    sv("sv109_the_person_names_the_symptom_and_the_machine_picks_the_fix")


@case("SV108", "A draft is judged the way ingest judges it, and kept nowhere")
def _():
    sv("sv108_a_draft_is_judged_the_way_ingest_judges_it")


@case("SV107", "A project's own terms reach the reader as written")
def _():
    sv("sv107_a_projects_own_terms_reach_the_reader_as_written")


@case("SV102", "No route waits on the database from the event loop")
def _():
    sv("sv_no_route_blocks_the_event_loop")


@case("SV101", "A notice states that whoever filed it means it")
def _():
    sv("sv_a_notice_states_that_whoever_filed_it_means_it")


@case("SV119", "A freshly stood-up operator can serve its index")
def _():
    sv("sv_a_fresh_operator_can_serve_its_index")


@case("SV118", "A notice filed behind a backlog is still reachable")
def _():
    sv("sv_a_notice_filed_behind_a_backlog_is_still_reachable")


@case("SV100", "The decision is made on the operator's own listener")
def _():
    sv("sv_the_decision_is_made_on_the_operators_own_listener")


@case("SV98", "A served head verifies under the key that signed it")
def _():
    sv("sv_a_served_head_verifies_under_the_key_that_signed_it")


@case("SV99", "A solution edited in place is fetched again")
def _():
    sv("sv_a_solution_edited_in_place_is_fetched_again")


@case("SV96", "An inclusion proof can be pinned to the size of a signed head")
def _():
    sv("sv_an_inclusion_proof_is_for_the_head_the_caller_holds")


@case("W11", "The window carries no inline style attribute its CSP would block [rust]")
def _():
    cargo("ui_contract::tests::the_window_carries_no_inline_style_attributes")


@case("W14", "The parts of the window the engine draws follow its theme [rust]")
def _():
    cargo("ui_contract::tests::the_native_parts_of_the_window_follow_its_theme")


@case("C3", "The refusing button is the one that takes focus [rust]")
def _():
    """The source half. The strong half needs a desktop: `drive_window.mjs`
    reads `document.activeElement` on every consent panel it meets, and every
    walk on 2026-09-12 reported none missing. A source check alone would not
    notice a panel that took focus back, which is why both exist."""
    cargo("ui_contract::tests::the_refusing_button_is_the_one_that_takes_focus")


@case("C4a", "The consent panel shows the values as they will be sent [rust]")
def _():
    cargo("ui_contract::tests::the_consent_panel_shows_the_values_as_they_will_be_sent")


@case("C4b", "A person can change their own words before they go [rust]")
def _():
    cargo("ui_contract::tests::a_person_can_change_their_own_words_before_they_go")


@case("C4", "What is shown before sending is what is sent [rust]")
def _():
    cargo("ui_contract::tests::what_is_shown_before_sending_is_what_is_sent")


@case("PV9", "A card reporting 0 for its serial has no serial [rust]")
def _():
    cargo("reads::tests::a_card_reporting_zero_for_its_serial_has_no_serial")


@case("P4", "A question with choices offers no answer of its own [rust]")
def _():
    cargo("ui_contract::tests::a_question_with_choices_offers_no_answer_of_its_own")


@case("W16", "A command that grades an answer is told what was typed [rust]")
def _():
    cargo("ui_contract::tests::every_command_that_grades_an_answer_is_told_what_was_typed")


@case("W15", "The conversation scrolls, and the footer stays [rust]")
def _():
    cargo("ui_contract::tests::the_conversation_scrolls_and_the_footer_stays")


@case("W13", "A sentence shown with no values to fill in carries none [rust]")
def _():
    cargo("i18n::tests::a_sentence_asked_for_without_values_carries_none")


@case("W12", "The window's escaper covers attribute values [rust]")
def _():
    cargo("ui_contract::tests::the_escaper_covers_attribute_values")


@case("I5", "Everything the binary says is a sentence in the language files, said in the user's language [rust]")
def _():
    cargo("msg::tests::every_message_the_binary_says_has_a_sentence")
    cargo("msg::tests::no_sentence_is_kept_for_a_message_nothing_says")
    cargo("msg::tests::every_sentence_can_be_recognised_again")
    cargo("ui_contract::tests::the_binarys_messages_are_shown_through_tr")


@case("I6", "The demo vendor answers in the language asked for, from its content files")
def _():
    """Its receipts, reply notes and refusals were German literals in the
    source, on every screen. They come from `content/<lang>.json` now, in the
    language the client asked for, or English where the vendor has no other."""
    for lang in ("en", "de"):
        r = vendor_reports.receive({"skill_id": f"i6.{time.time_ns()}", "observed": {},
                                    "failed_actions": [], "resolved_by": "human"}, lang)
        assert r["message"] == said(lang, "receipt_no_pseudonym"), r
    assert said("en", "receipt_no_pseudonym") != said("de", "receipt_no_pseudonym")
    assert said("fr", "reply_none") == said("en", "reply_none"), "a language the vendor lacks is not English"
    src = "".join(Path(f"src/podshl/vendor/{m}.py").read_text(encoding="utf-8")
                  for m in ("app", "catalog", "reports"))
    for lang in ("en", "de"):
        for key, text in content(lang)["vendor"].items():
            assert text not in src, f"{key} is written in the source as well as in content/{lang}.json"


@case("LG7", "A published project's words are translated by the reader's own model [rust]")
def _():
    cargo("ui_contract::tests::a_published_projects_words_are_translated_by_the_readers_own_model")


@case("LG8", "A project's own terms are hidden from the model and put back [rust]")
def _():
    cargo("llm::glossary_tests::the_projects_terms_are_hidden_from_the_model_and_put_back")
    cargo("llm::glossary_tests::a_term_the_translation_lost_is_found")
    cargo("ui_contract::tests::a_projects_own_terms_are_kept_and_a_lost_one_is_said")


@case("LG9", "A project's commands reach the reader byte for byte [rust]")
def _():
    cargo("llm::glossary_tests::the_code_in_an_answer_is_hidden_from_the_model_and_put_back")


@case("I4", "The consent text is in the user's language [rust]")
def _():
    cargo("ui_contract::tests::the_consent_text_exists_in_every_language")
    cargo("flow::tests::every_refusal_has_a_kind_the_window_can_translate")


@case("PB7", "A published answer renders its Markdown, and nothing a publisher writes becomes markup")
def _():
    """The answer a project published is Markdown written by a stranger. It was
    shown with blank lines turned into breaks, so the command a solution exists
    to give — an indented block — ran into one line with the next command. The
    renderer escapes every character first and inserts only fixed tags; this
    runs the window's own functions under node against engram's real solution
    and against text written to escape."""
    driver = r"""
const { readFileSync } = require("node:fs");
const html = readFileSync("client-rs/ui/index.html", "utf8");
const grab = (a, b) => { const i = html.indexOf(a); if (i < 0) throw new Error("missing " + a); return html.slice(i, html.indexOf(b, i)); };
const f = new Function(html.match(/const esc = [^\n]+/)[0] + "\n" +
  grab("function inlineMd(", "/// The sentence the user consents to") + "\nreturn {md};")();
const sol = readFileSync("examples/engram/.podshl/solutions/model-endpoint-not-reachable.md", "utf8").split("---").slice(2).join("---");
const ok = f.md(sol), bad = f.md("**<img src=x onerror=alert(1)>**\n\n    <script>alert(2)</script>\n\n* `<b>x</b>` *<i>y</i>*\n```\n</pre><script>3</script>\n```");
const stray = [...bad.matchAll(/<\/?([a-z]+)/g)].map(m => m[1]).filter(t => !["p","b","i","code","pre","ul","li"].includes(t));
console.log(JSON.stringify({ok, bad, stray}));
"""
    out = subprocess.run(["node", "-e", driver], capture_output=True, text=True, timeout=60)
    assert out.returncode == 0, out.stderr[-800:]
    r = json.loads(out.stdout)
    assert not r["stray"], f"a publisher's text became markup: {r['stray']}\n{r['bad']}"
    assert '<pre class="md">ollama pull gemma4:e4b\nollama serve</pre>' in r["ok"], (
        "the command block of engram's solution is not one block of two lines:\n" + r["ok"])
    assert "<b>System → LLM config</b>" in r["ok"], "emphasis arrived as asterisks"


@case("A8", "Undo restores the file from its backup [rust]")
def _():
    cargo("actions::tests::undo_restores_from_the_backup")


@case("A9", "Undo with no backup refuses cleanly [rust]")
def _():
    cargo("actions::tests::undo_without_a_backup_refuses_cleanly")


@case("J1", "The Rust client verifies a card signed by the Python vendor [rust]")
def _():
    cargo("jws::tests::verifies_a_card_signed_by_the_python_vendor")


@case("J2", "One changed byte breaks the signature [rust]")
def _():
    cargo("jws::tests::rejects_the_same_card_after_one_byte_changes")


@case("J3", "Object keys sort by UTF-16 code unit [rust]")
def _():
    cargo("jcs::tests::utf16_ordering")


@case("J4", "The solidus is not escaped [rust]")
def _():
    cargo("jcs::tests::solidus_is_not_escaped")


@case("J5", "Control characters use the short escapes [rust]")
def _():
    cargo("jcs::tests::control_characters_use_short_escapes")


@case("J6", "Floats are refused rather than approximated [rust]")
def _():
    cargo("jcs::tests::floats_are_refused_rather_than_approximated")


# ------------------------------------------------ round parsing and vendors

@case("S5", "A round answer yields both reads and questions [rust]")
def _():
    cargo("llm::round_tests::extracts_ids_and_questions")


@case("MD1", "Each round carries one dimension of the method, in order [rust]")
def _():
    cargo("llm::round_tests::each_round_carries_one_dimension_of_the_method_in_order")


@case("MD2", "An answer is split into its sections, and graded by what it rests on [rust]")
def _():
    cargo("llm::round_tests::an_answer_is_split_into_its_sections_and_graded")


@case("MD3", "The answer must explain what it does not affect [rust]")
def _():
    cargo("llm::round_tests::the_answer_must_explain_what_it_does_not_affect")


@case("MD5", "Every prompt names the language, and says it last [rust]")
def _():
    cargo("llm::round_tests::every_prompt_names_the_language_and_says_it_last")


@case("MD4", "A short id is not dragged in by a longer one [rust]")
def _():
    cargo("llm::round_tests::a_short_id_is_not_dragged_in_by_a_longer_one")


@case("S6", "An id the model invented never becomes a read [rust]")
def _():
    cargo("llm::round_tests::ignores_ids_outside_the_catalogue")


@case("S7", "An id mentioned inside a question is not a read [rust]")
def _():
    cargo("llm::round_tests::a_question_mentioning_an_id_is_not_a_read")


@case("S8", "The model can report completion [rust]")
def _():
    cargo("llm::round_tests::recognises_completion")


@case("V1", "A named vendor contradicted by the reading is flagged [rust]")
def _():
    cargo("vendors::tests::spots_a_vendor_the_user_did_not_name")


@case("V2", "No false alarm when the reading agrees [rust]")
def _():
    cargo("vendors::tests::stays_quiet_when_the_reading_agrees")


@case("V3", "No claim of mismatch when nothing was read [rust]")
def _():
    cargo("vendors::tests::stays_quiet_when_nothing_was_read")


# ----------------------------------------------- read catalogue and identity

@case("N1", "The catalogue only offers what this machine can run [rust]")
def _():
    cargo("reads::tests::catalogue_only_offers_what_is_runnable_here")


@case("N2", "A missing or unlisted tool is refused with a reason [rust]")
def _():
    cargo("reads::tests::a_missing_tool_is_refused_with_a_reason")


@case("N3", "Consent cannot unlock a denied path [rust]")
def _():
    cargo("reads::tests::denied_paths_stay_denied")


@case("N4", "The free baseline carries os and architecture [rust]")
def _():
    cargo("reads::tests::baseline_is_free_and_complete")


@case("ID1", "A pseudonym differs per vendor [rust]")
def _():
    cargo("identity::tests::differs_per_vendor")


@case("ID2", "A pseudonym is stable within a vendor and epoch [rust]")
def _():
    cargo("identity::tests::stable_within_vendor_and_epoch")


@case("ID3", "The epoch is a year-month [rust]")
def _():
    cargo("identity::tests::epoch_is_a_year_month")


# ------------------------------------------------------ vendor-served language

@case("LG1", "The vendor serves the requested language when it has it")
def _():
    sk, got = serve(CATALOG["torch.precision.consumer-gpu"], "de")
    assert got == "de" and sk.title == content("de")["skills"][sk.id]["title"], sk.title


@case("LG2", "Otherwise it falls back to English, which every vendor owes")
def _():
    sk, got = serve(CATALOG["torch.precision.consumer-gpu"], "fr")
    assert got == "en" and "Training precision" in sk.title, (got, sk.title)


@case("LG3", "A skill without English is refused loudly, not served in the wrong language")
def _():
    """Against a skill built here rather than one picked out of the catalogue.

    This used to name `warranty.rma.precheck`, which was German-only — so the
    case tested a gap in the demo vendor's content, and it stopped testing
    anything the moment somebody filled the gap in. The property is about
    `serve`: a vendor that cannot produce English has not met the protocol,
    whatever the catalogue happens to hold today.
    """
    from podshl.vendor import catalog as _catalog
    only_german = _catalog.CATALOG["warranty.rma.precheck"].model_copy(
        update={"id": "vendor.only-german", "lang": "de"})
    assert "en" not in _catalog.TRANSLATIONS.get(only_german.id, {}),         "this skill has an English variant, so the case would prove nothing"
    try:
        serve(only_german, "en")
    except MissingEnglish:
        pass
    else:
        raise AssertionError("a German-only skill was served as English")

    # And the catalogue the demo vendor actually ships answers in English for
    # every skill in it, which is what a real vendor owes.
    for sid, skill in _catalog.CATALOG.items():
        served, lang = serve(skill, "en")
        assert lang == "en", f"{sid} cannot be served in English"


@case("LG4", "The client is told which language it actually got")
def _():
    r = httpx.post(f"{ACME}/a2a", json={
        "jsonrpc": "2.0", "id": "t", "method": "SendMessage",
        "params": {"message": {"role": "user", "parts": [{"kind": "data", "data": {
            "kind": "triage", "problem": "training is slow, bf16?", "lang": "fr"}}]}}
    }, timeout=10).json()["result"]
    assert r.get("lang_served") == "en" and r.get("lang_requested") == "fr", r


@case("LG5", "Translation is only reached when the vendor lacks the language [rust]")
def _():
    cargo("ui_contract::tests::translation_is_only_reached_when_the_vendor_lacks_the_language")

@case("E1", "The version tree is walked and its manifests read [rust]")
def _():
    cargo("reads::tests::enumerate_read_walks_and_reads")


@case("E2", "A traversal cannot escape its root or enter denied names [rust]")
def _():
    cargo("reads::tests::enumerate_read_refuses_traversal_and_denied_names")


@case("GR1", "Below the floor nothing is reported about that vendor")
def _():
    sv("gr1_below_the_floor_nothing_is_reported")


@case("GR1a", "Repeated submissions from one client are one reporter")
def _():
    sv("gr1a_repeated_submissions_are_one_reporter")


@case("GR1b", "A group on the page counts people the way a cluster does")
def _():
    sv("gr1b_a_group_on_the_page_counts_people_the_way_a_cluster_does")


@case("GR2", "At the floor the report states its policy and its own limits")
def _():
    sv("gr2_at_the_floor_the_report_states_its_policy_and_its_limits")


@case("GR3", "A weak model succeeding is read as a UX defect, not a knowledge gap")
def _():
    sv("gr3_a_weak_model_succeeding_is_a_ux_defect")


# ------------------------------------------- the third outcome: "I need X"

@case("ND1", "A missing decisive fact is acquired, not refused")
def _():
    r = generate(CATALOG["accounting.booking.guidance"],
                 {"question": "how do I book an incoming invoice", "app.version": "14.2"})
    assert r.need and r.need[0].id == "chart.confirmed", r
    assert not r.abstained, "it refused instead of asking"
    assert r.need_reason, "asked without saying why"


@case("ND2", "'Don't know' falls back to the parent and never loops")
def _():
    r = generate(CATALOG["accounting.booking.guidance"],
                 {"question": "x", "chart.confirmed": _ADVICE["text"]["dont_know"]})
    assert not r.need, "it asked again for something the user cannot answer"
    assert r.abstained and r.escalate, "the fallback has no destination"


@case("ND3", "A declined probe is recorded, so the endpoint stops asking")
def _():
    """The key is built from what the endpoint asked for, as the client builds it.

    This used to send `serial.declined` while the probe is `serial.printed`,
    and the endpoint read the same wrong key - so the case and the code agreed
    with each other and both disagreed with the wire. It was green for as long
    as it existed, and the endpoint asked for the serial forever on any card
    whose firmware has none. `SPEC.md` says `<probe id>.declined`, so the
    probe's own id is where the key comes from and it cannot drift again.
    """
    r = generate(CATALOG["warranty.rma.precheck"],
                 {"gpu.name": "RTX", "symptom": _SYMPTOM})
    assert r.need and r.need[0].id == "serial.printed", "did not ask for the serial"
    asked = r.need[0].id
    r2 = generate(CATALOG["warranty.rma.precheck"],
                  {"gpu.name": "RTX", "symptom": _SYMPTOM,
                   f"{asked}.declined": True})
    assert not r2.need, "asked again after being told the user cannot answer"
    assert r2.abstained and r2.escalate, "no destination after the decline"
    # And nothing else is honoured in its place: a key the client never sends
    # must not be what makes an endpoint stop.
    r3 = generate(CATALOG["warranty.rma.precheck"],
                  {"gpu.name": "RTX", "symptom": _SYMPTOM, "serial.declined": True})
    assert r3.need, "the endpoint stops on a key the wire convention never carries"


@case("ND4", "The client loops on `need` instead of answering once [rust]")
def _():
    cargo("ui_contract::tests::the_vendor_path_loops_on_need")


@case("ND7", "A question left empty is recorded as declined [rust]")
def _():
    cargo("ui_contract::tests::a_question_left_empty_is_recorded_as_declined")


@case("ND6", "A round that cannot make progress ends the loop [rust]")
def _():
    cargo("ui_contract::tests::a_round_that_cannot_make_progress_ends_the_loop")


@case("ND5", "The need round offers a way to stop as well as to skip [rust]")
def _():
    cargo("ui_contract::tests::the_need_round_offers_a_way_to_stop_as_well_as_to_skip")

@case("H1", "The vendor declares the target, what it needs and how it answers")
def _():
    r = generate(CATALOG["warranty.rma.precheck"],
                 {"gpu.name": "RTX", "serial.printed": "0324718061234",
                  "symptom": _SYMPTOM, "gpu.driver_version": "610.57.04"})
    e = r.escalate
    assert e.target, "no routing target — the client would have to invent one"
    assert any(p.id == "contact.email" for p in e.require), \
        "a contact address is not declared, so the client would have to hardcode it"
    assert "email" in e.reply_via, e.reply_via


@case("H2", "A channel the client cannot honour is filtered out [rust]")
def _():
    cargo("handover::tests::filters_channels_the_client_cannot_honour")


@case("H3", "An invented reply channel is never offered [rust]")
def _():
    cargo("handover::tests::an_invented_channel_is_never_usable")


@case("H4", "'No reply' is a promise the vendor may make [rust]")
def _():
    cargo("handover::tests::none_is_a_valid_channel")


@case("H5", "The hand-off returns a reference and states the return path")
def _():
    r = httpx.post(f"{ACME}/a2a", json={
        "jsonrpc": "2.0", "id": "h", "method": "SendMessage",
        "params": {"message": {"role": "user", "parts": [{"kind": "data", "data": {
            "kind": "escalate", "queue": "rma-desk", "target": "itsm://acme/rma",
            "reply_via": "ticket_url", "payload": {"a": 1}}}]}}
    }, timeout=10).json()["result"]
    assert r["reference"], r
    assert r["reply_via"] == "ticket_url" and r["ticket_url"], r
    assert r["target"] == "itsm://acme/rma", "the routing target was not passed through"


@case("H6", "A vendor promising no reply says so rather than leaving the user waiting")
def _():
    r = httpx.post(f"{ACME}/a2a", json={
        "jsonrpc": "2.0", "id": "h", "method": "SendMessage",
        "params": {"message": {"role": "user", "parts": [{"kind": "data", "data": {
            "kind": "escalate", "queue": "general", "reply_via": "none",
            "payload": {}}}]}}
    }, timeout=10).json()["result"]
    assert r["reply_note"] == said("en", "reply_none"), r["reply_note"]


@case("H7", "The client refuses a reply channel it cannot honour [rust]")
def _():
    cargo("ui_contract::tests::the_escalate_command_refuses_a_channel_it_cannot_honour")


@case("H8", "Required fields are validated before a case is opened [rust]")
def _():
    cargo("ui_contract::tests::required_fields_are_validated_before_a_case_is_opened")

@case("W1", "Every registered command is reachable from the interface [rust]")
def _():
    cargo("ui_contract::tests::every_registered_command_is_reachable_from_the_window")


@case("W2", "The applicability gate is actually applied [rust]")
def _():
    cargo("ui_contract::tests::the_applicability_gate_is_actually_applied")

@case("I1", "Every language defines the same keys [rust]")
def _():
    cargo("i18n::tests::every_language_defines_the_same_keys")


@case("I2", "Every language names itself [rust]")
def _():
    cargo("i18n::tests::every_language_names_itself")


@case("I3", "Placeholders survive translation [rust]")
def _():
    cargo("i18n::tests::every_translation_keeps_the_placeholders_english_declares")


@case("I7", "The index names every language and the window loads two of them [rust]")
def _():
    cargo("i18n::tests::the_index_names_every_language_and_only_those")
    cargo("i18n::tests::the_window_takes_its_languages_from_the_index")


# ------------------------------------------------------------------- platform

@case("L6", "A closed pipe ends the program quietly, not with a core dump [rust]")
def _():
    cargo("closing_the_pipe_early_does_not_kill_it", target="cli")


@case("L7", "An unknown subcommand is refused by name [rust]")
def _():
    cargo("an_unknown_subcommand_is_refused_by_name", target="cli")


@case("L8", "The client says which version it is [rust]")
def _():
    cargo("it_says_which_version_it_is", target="cli")


@case("L9", "The Windows installer needs no administrator and removes the real data [rust]")
def _():
    cargo("ui_contract::tests::the_windows_installer_needs_no_administrator_and_removes_the_real_data")


@case("L10", "A release build reads no key or directory beside where it was started [rust]")
def _():
    cargo("ui_contract::tests::a_release_build_reads_no_key_or_directory_beside_where_it_was_started")


@case("D11", "A small-order public key is refused as a key [rust]")
def _():
    cargo("jws::tests::a_small_order_public_key_is_refused")


@case("L5", "What needs privilege is done by a separate helper, never in process [rust]")
def _():
    """The rule this case always stood for, now that there is something to hold.

    Until 2026-09-17 nothing in the vocabulary needed privilege and the case was
    that check. Three examples do now (`restart_service`, `set_service_start`,
    `set_machine_env`), so the helper exists as its own program,
    `podshl-elevate`, and the client's source is held to never writing a
    service or the machine's registry itself. The vocabulary marks each action
    that needs privilege, and only those are sent to the helper.
    """
    import json
    from pathlib import Path

    vocab = json.loads(Path("spec/vocabulary/actions.json").read_text(encoding="utf-8"))
    elevated = sorted(a["id"] for a in vocab["actions"] if a.get("elevated"))
    assert elevated == ["restart_service", "set_machine_env", "set_service_start"], (
        f"the actions that need privilege are now {elevated}. Each has to be performed by "
        f"podshl-elevate and checked by client-rs/src/elevated.rs.")
    assert all("elevated" in a for a in vocab["actions"]), "an action does not say whether it needs privilege"
    assert Path("client-rs/src/bin/podshl-elevate.rs").is_file(), "the helper is gone"
    cargo("elevate::tests::nothing_privileged_is_written_in_process")


@case("AN2", "A path in the person's own profile is shown without their account name [rust]")
def _():
    cargo("reads::tests::a_path_in_the_profile_is_shown_without_the_account_name")


@case("EV1", "An administrator action is refused before the prompt unless it is one of three [rust]")
def _():
    cargo("elevated::tests::only_the_three_actions_with_their_parameters_pass")
    cargo("elevated::tests::what_windows_needs_to_run_is_not_touched")
    cargo("elevate::tests::a_refused_request_never_reaches_the_prompt")


@case("EV2", "The helper checks again, and without privilege changes nothing [rust]")
def _():
    cargo("elevated::tests::the_command_line_says_exactly_the_request_and_nothing_else_passes")
    cargo("a_refused_or_unprivileged_request_changes_nothing", target="elevate_helper")


@case("EV3", "A change counts when the machine shows it, and its undo is the reading before [rust]")
def _():
    cargo("elevate::tests::a_change_is_what_the_second_reading_shows_and_its_undo_is_the_first")

@case("L1", "doctor reports what actually resolves on this machine [rust]")
def _():
    cargo("doctor::tests::doctor_reports_what_actually_resolves_on_this_machine")



# ------------------------------------------------------- the server (SV cases)

def server_up():
    """Whether the server's database is reachable.

    The SV cases need Postgres and the rest of this suite does not, so their
    absence is stated rather than either skipped silently or failed loudly. A
    suite that cannot start without a database would stop answering for the
    counterparty, which has nothing to do with the server.
    """
    try:
        from podshl.server import db
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT 1")
        return True
    except Exception:
        return False


# ------------------------------------------------- the flow, through the client


#: The built client, asked the way the window asks it.
#:
#: Every defect on 2026-09-14 was found by a person clicking, and none of them
#: could have been found otherwise: `ui_contract` reads the window's source,
#: and text cannot fall into the wrong branch. These walk the same commands the
#: window calls, in the same order, and assert what came back.
#: `PODSHL_CLIENT` names one explicitly; otherwise wherever the build put it.
#: The container moves the target directory to a volume, so a fixed path here
#: would check a file the build never wrote.
CLIENT = Path(os.environ.get("PODSHL_CLIENT")
              or (Path(os.environ.get("CARGO_TARGET_DIR", "client-rs/target"))
                  / "release" / "podshl-client"))


def client(command: str, args: dict | None = None):
    """One of the window's commands, against the built client.

    Fails with an instruction rather than skipping: a client built without the
    operator and the log key compiled in is not a broken client, it is a
    *plausible* one — it starts, draws its window, and quietly cannot verify the
    published directory. That is exactly what shipped this morning.
    """
    import subprocess
    assert CLIENT.exists(), (
        f"no client at {CLIENT} — build one with `scripts/build/build_client.sh <operator>`. "
        f"These cases walk the real binary; there is nothing to assert without it.")
    r = subprocess.run([str(CLIENT), "invoke", command, json.dumps(args or {})],
                       capture_output=True, text=True, timeout=120)
    assert r.returncode == 0, f"{command} failed: {r.stderr.strip()[:300]}"
    return json.loads(r.stdout)


@case("CL1", "The client is built for an operator, not for loopback")
def _():
    """A client built without `PODSHL_BUILD_SERVER_URL` and `PODSHL_BUILD_LOG_KEY`
    points at loopback and holds no key. It still starts and still draws its
    window — and every published project then falls through to the model,
    because an index it cannot verify is an index it will not use.

    `option_env!` is read at compile time and cargo does not rebuild when only an
    environment variable changed, so this is one `cargo build` away at any
    moment. `scripts/build/build_client.sh` is the guard; this is the case."""
    e = client("endpoints")
    assert e["operator"].startswith("https://"), (
        f"the client talks to {e['operator']!r} — built without an operator, so it "
        f"would ask loopback on somebody's desktop")
    # The window fetches the directory when it opens; nothing else does. A case
    # that only asked `index_status` was asserting that somebody had opened a
    # window first, which is not a property of the client.
    client("refresh_index", {"base": e["operator"]})
    st = client("index_status")
    assert st["have"], (
        "the client holds no verified directory. Either it has no pinned log key "
        "compiled in, or the operator's index did not verify — and in both cases "
        "every published project silently becomes a model question")


@case("CL2", "A published project is found, and found as owner/repo")
def _():
    """What the window does first. `engram` is a word 1872 repositories use, so
    the row has to say *which* — and the forge's own name must not find every
    repository on it at once."""
    hits = client("search_vendors", {"query": "engram"})
    assert hits, "nothing found for a project that is published"
    top = hits[0]
    assert top["vendor"] == "dx111ge/engram", (
        f"shown as {top['vendor']!r} — a repository must be owner/name, never the forge")
    assert top["base"] == "https://github.com/dx111ge/engram/", top
    assert len(top.get("answers") or []) == 3, (
        f"{len(top.get('answers') or [])} problem classes — without them the window "
        f"skips the published path entirely and asks a model instead")
    # The forge's own name must not reach a *published* row. It still gets an
    # answer — the guess, which every unknown name gets — and asserting emptiness
    # here was this case being wrong rather than the client.
    forge = client("search_vendors", {"query": "github"})
    assert not any(h.get("base", "").startswith("https://github.com/") for h in forge), \
        f"the forge's own name reached the repositories on it: {forge}"


@case("CL3", "The published card is fetched for a repository")
def _():
    """The half that was missing when the operator learned about repositories:
    `/mirror/{host}` began refusing a bare forge host — correctly, since
    `github.com` is shared — and the client kept sending the host. Every
    repository anchor fell through to the model, which then asked what the
    project was, having never been told."""
    card = client("published_card", {"base": "https://sdota.de",
                                     "host": "https://github.com/dx111ge/engram/"})
    assert card.get("collect"), (
        "no readings came back — the card was not fetched, and the published path "
        "cannot run without it")
    ids = {c.get("id") for c in card["collect"]}
    assert "os.arch" in ids, ids
    # And the address that must not work, because it is everybody's.
    import subprocess
    r = subprocess.run([str(CLIENT), "invoke", "published_card",
                        json.dumps({"base": "https://sdota.de", "host": "github.com"})],
                       capture_output=True, text=True, timeout=60)
    assert r.returncode != 0, "a bare forge host answered with somebody's card"


@case("CL4", "The whole published path answers, and answers from the project")
def _():
    """Search, card, diagnosis — the walk a person makes, with no model anywhere
    in it. This is the open-source path and the common one."""
    out = client("ask_published", {
        "base": "https://sdota.de",
        "subject": "https://github.com/dx111ge/engram/",
        "problemClass": "engram.search.stale-after-model-change",
        "facts": {"os.arch": "x86_64", "os.name": "linux",
                  "engram.embedding_changed": "yes, and I did not run engram reindex"},
        "stated": ["engram.embedding_changed"]})
    assert out.get("outcome") == "finding", out
    assert out["solution"]["solution_id"] == "search-stale-after-model-change", out["solution"]
    assert "reindex" in json.dumps(out["solution"]["text_by_lang"]), \
        "the answer does not name the fix the project published"
    assert out.get("confidence") == "rests_on_supplied", (
        "an answer that turned on something a person typed must say so")

    # **The case the whole OS reading exists for**, and it was wrong twice over
    # on 2026-09-16: the probe read nothing because it asked `os_fact` for a
    # fact it does not answer, and once that was fixed the answer was still
    # unreachable on an x86_64 desktop, because it says nothing about the
    # processor and the tree had only branched on the one value another rule
    # named (`SV120`). Somebody on Linux holding the Windows archive was told
    # nothing was wrong, both times.
    wrong_os = client("ask_published", {
        "base": "https://sdota.de",
        "subject": "https://github.com/dx111ge/engram/",
        "problemClass": "engram.start.wrong-build",
        "facts": {"os.arch": "x86_64", "os.name": "linux",
                  "engram.download": "engram-windows-x86_64.zip"},
        "stated": ["engram.download"]})
    assert wrong_os.get("outcome") == "finding", wrong_os
    assert wrong_os["solution"]["solution_id"] == "wrong-archive-for-this-system", (
        f"the Windows archive on a Linux desktop was not named: {wrong_os['solution']}")

    # **The commonest one, and the matrix did not have it.** An ordinary Intel
    # Linux desktop holding the ARM build: engram's own `why` calls it the most
    # common reason, only the opposite direction was published, and this case
    # checked Windows-on-Linux and never this. Walked on the Omarchy desktop on
    # 2026-09-16, it answered "find out which archive you have" to somebody who
    # had just said which. Fixed in engram (`wrong-build-on-an-intel-machine`).
    wrong_arch = client("ask_published", {
        "base": "https://sdota.de",
        "subject": "https://github.com/dx111ge/engram/",
        "problemClass": "engram.start.wrong-build",
        "facts": {"os.arch": "x86_64", "os.name": "linux",
                  "engram.download": "engram-linux-aarch64.zip"},
        "stated": ["engram.download"]})
    assert wrong_arch.get("outcome") == "finding", wrong_arch
    assert wrong_arch["solution"]["solution_id"] == "wrong-build-on-an-intel-machine", (
        f"the ARM build on an Intel desktop was not named: {wrong_arch['solution']}")

    # And the same class where the rule genuinely does not apply: the archive
    # matches the machine, so nothing specific fires and the class's own
    # fallback answers instead of a rule about somebody else's machine. A
    # published answer that fires anyway is worse than none.
    other = client("ask_published", {
        "base": "https://sdota.de",
        "subject": "https://github.com/dx111ge/engram/",
        "problemClass": "engram.start.wrong-build",
        "facts": {"os.arch": "x86_64", "os.name": "linux",
                  "engram.download": "engram-linux-x86_64.zip"},
        "stated": ["engram.download"]})
    assert other.get("outcome") == "finding", other
    assert other["solution"]["solution_id"] == "find-out-which-archive", (
        f"an answer about the wrong archive was given to somebody with the right one: "
        f"{other['solution']}")

    # Somebody else's repository on the same forge reaches none of it.
    stranger = client("ask_published", {
        "base": "https://sdota.de",
        "subject": "https://github.com/someone/engram/",
        "problemClass": "engram.search.stale-after-model-change",
        "facts": {"engram.embedding_changed": "yes, and I did not run engram reindex"},
        "stated": []})
    assert stranger.get("outcome") == "no_statement", stranger


def sv(name):
    """Delegate to one server case. Named for the SV row it satisfies.

    The suite fetches from the counterparty on 127.0.0.1, and the crawler refuses
    loopback unless told otherwise — because it follows URLs an attacker chose.
    Switched on here, for the suite only, and never by default.
    """
    import os
    os.environ.setdefault("PODSHL_ALLOW_LOOPBACK", "1")
    from podshl.server import testcases as sv_cases
    assert server_up(), (
        "the server database is not reachable — start it with `mise run db` and "
        "`mise run migrate`. These cases are not skipped when it is absent, "
        "because a case that passes without running proves nothing.")
    sv_cases.ALL[name]()


@case("SV1", "OSS anchor attested; entry appended to the log")
def _():
    sv("sv1_attesting_appends_to_the_log")


@case("SV2", "Challenge file gone once — no state change")
def _():
    sv("sv2_one_failed_check_changes_nothing")


@case("SV2a", "Gone 14 days — stale; 90 days — unknown, never revoked")
def _():
    sv("sv2a_2b_absence_grades_but_never_revokes")


@case("SV4", "There is no display-name field to abuse")
def _():
    sv("sv4_there_is_no_display_name_field")


@case("SV5", "An action outside the vocabulary is refused at ingest")
def _():
    sv("sv5_a_solution_proposing_an_unknown_action_is_refused")


@case("SV7", "A card without English cannot be stored")
def _():
    sv("sv7_a_manifest_without_english_is_refused")


@case("SV13", "Counted once per pseudonym per epoch, never twice")
def _():
    sv("sv13_counted_once_per_pseudonym_per_epoch")


@case("SV14", "Below k distinct pseudonyms, nothing is surfaced")
def _():
    sv("sv14_below_k_nothing_is_surfaced")


@case("SV16", "The same signature is one cluster, found by one indexed lookup")
def _():
    sv("sv16_the_edge_path_is_one_indexed_lookup")


@case("SV19", "The dashboard for an unclaimed domain refuses everyone")
def _():
    sv("sv19_the_dashboard_refuses_without_a_claim")


@case("SV20", "A verified claim opens it")
def _():
    sv("sv20_a_verified_claim_opens_it")


@case("SV21", "No route produces another vendor's figures")
def _():
    sv("sv21_sv24_no_route_produces_another_vendors_figures")


@case("SV22", "Free text needs its own consent, naming a destination")
def _():
    sv("sv22_free_text_needs_its_own_consent")


@case("EN1", "A virtualenv's Python is read without executing anything [rust]")
def _():
    cargo("reads::tests::a_virtualenvs_python_is_read_without_executing_anything")


@case("EN2", "Whether this is a container is answerable [rust]")
def _():
    cargo("reads::tests::whether_this_is_a_container_is_answerable")


@case("T11", "Free text travels only on its own consent [rust]")
def _():
    cargo("report::tests::free_text_travels_only_on_its_own_consent")


@case("T10", "A reading with a question attached arrives either way [rust]")
def _():
    cargo("report::tests::a_reading_with_a_question_attached_arrives_either_way")


@case("T9", "A stated value never travels as a reading [rust]")
def _():
    cargo("report::tests::a_stated_value_never_travels_as_a_reading")


@case("EN3", "A disagreeing interpreter becomes a question, not an answer [rust]")
def _():
    cargo("reads::tests::a_disagreeing_interpreter_becomes_a_question_not_an_answer")


@case("EN4", "A new question ends the last incident's grants first [rust]")
def _():
    cargo("ui_contract::tests::a_new_question_ends_the_last_incident_first")


@case("EN5", "The development root is only read in a development build [rust]")
def _():
    cargo("reads::tests::the_development_root_is_only_read_in_a_development_build")


# --------------------------------------------------- a program's own version

@case("PV1", "A program's version is read by asking it; only the number is kept [rust]")
def _():
    cargo("reads::tests::a_programs_version_is_read_by_asking_it_and_only_the_number_is_kept")


@case("PV2", "Not on the search path: asked where, never searched; the grant is bounded [rust]")
def _():
    cargo("reads::tests::a_program_not_on_the_path_is_asked_for_and_the_answer_is_bounded")
    cargo("ui_contract::tests::a_program_not_on_the_path_is_asked_about_not_searched_for")


@case("PV3", "Shells, launchers, paths, own arguments and system programs are refused [rust]")
def _():
    cargo("reads::tests::a_program_that_is_not_a_program_s_own_version_is_refused")


@case("PV4", "The version is found in what programs actually print [rust]")
def _():
    cargo("reads::tests::a_version_is_found_in_what_programs_actually_print")


@case("PV5", "A program that does not answer is stopped [rust]")
def _():
    cargo("reads::tests::a_program_that_does_not_answer_is_stopped")


@case("PV7", "A list-shaped tool reads what its entry promises and nothing more [rust]")
def _():
    cargo("reads::tests::a_list_shaped_tool_reads_what_its_entry_promises_and_nothing_more")


@case("PV6", "A Docker image is one image however Docker spells it [rust]")
def _():
    cargo("reads::tests::an_image_is_the_same_image_however_docker_spells_it")
    # Against a real Docker where there is one; it says so where there is not.
    cargo("excerpt::tests::a_running_container_is_found_by_its_image_and_its_output_loads")


# ------------------------------------------------------------ log excerpts

@case("LX1", "A named log file is read from its end; the deny list holds [rust]")
def _():
    cargo("excerpt::tests::a_named_file_is_read_from_its_end_and_the_deny_list_holds")


@case("LX2", "Identifiers are replaced before anything is offered, and counted [rust]")
def _():
    cargo("redact::tests::identifiers_are_replaced_before_anything_is_offered")


@case("LX3", "Versions, positions and paths in code survive anonymisation [rust]")
def _():
    cargo("redact::tests::versions_and_positions_survive")


@case("LX4", "A container source is an image name and nothing else [rust]")
def _():
    cargo("excerpt::tests::a_container_source_is_an_image_name_and_nothing_else")


@case("LX5", "An excerpt is bounded, says so, and the bounds are the spec's [rust]")
def _():
    cargo("redact::tests::an_excerpt_is_bounded_and_says_so")
    cargo("excerpt::tests::the_excerpt_bounds_match_the_spec")
    cargo("report::tests::free_text_is_bounded")


@case("LX7", "This machine's own account and host are found, not handed in [rust]")
def _():
    cargo("redact::tests::this_machines_own_names_are_found_and_removed")


@case("LX6", "Free text is anonymised before it is offered, attached only after [rust]")
def _():
    cargo("ui_contract::tests::free_text_is_anonymised_before_it_is_offered")


# ----------------------------------------------------- the published path

@case("PB1", "The published path reads the project's probes before it asks [rust]")
def _():
    cargo("ui_contract::tests::the_published_path_reads_before_it_asks")


@case("PB2", "A needed fact that can be read is read, not typed [rust]")
def _():
    cargo("ui_contract::tests::a_needed_fact_that_can_be_read_is_read")


@case("PB3", "A published report reaches the operator in its own shape [rust]")
def _():
    cargo("report::tests::a_published_report_reaches_the_operator_in_its_own_shape")


@case("PB4", "Whether it worked is asked; the report goes to the operator after consent [rust]")
def _():
    cargo("ui_contract::tests::a_published_report_asks_first_and_goes_to_the_operator")


@case("PB5", "Questions declared for a person are asked, with their choices [rust]")
def _():
    cargo("ui_contract::tests::declared_questions_are_asked_with_their_choices")


@case("PB6", "A published report is accepted by the running operator")
def _():
    """PB3 checks the shape on the client's side; this posts that shape to the
    running server, so the two agree with each other rather than with a fixture.
    The client's `for_operator` output is reproduced field for field."""
    body = {"subject": f"pb6-{time.time_ns()}.example", "pseudonym": "pb6-p", "epoch": "2026-09",
            "observed": {"os.name": "linux", "engram.version": "1.2.2"},
            "stated": {"engram.symptom": "the binary will not start at all", "error.text": None},
            "decided_on": ["os.name"], "dropped": ["error.text"], "failed_actions": [],
            "outcome": "unresolved", "model_class": "none",
            "description": "error.text: zsh: bad CPU type in executable: engram",
            "description_consent": {"granted": True, "destination": "the maintainers, through the operator",
                                    "granted_at": "2026-09"}}
    r = httpx.post("http://127.0.0.1:8725/report", json=body, timeout=10)
    assert r.status_code == 200 and r.json().get("accepted"), r.text


@case("SV73", "The root answers a machine and a browser differently")
def _():
    sv("sv73_the_root_answers_a_machine_and_a_browser_differently")


@case("SV74", "A page is served from disk and rendered from nothing")
def _():
    sv("sv74_a_page_is_served_from_disk_and_rendered_from_nothing")


@case("SV75", "A page's script is permitted by its own header")
def _():
    sv("sv75_a_pages_script_is_permitted_by_its_own_header")


@case("SV76", "The imprint refuses to invent an identity")
def _():
    sv("sv76_the_imprint_refuses_to_invent_an_identity")


@case("SV77", "Proving control again supersedes every earlier token")
def _():
    sv("sv77_proving_control_again_supersedes_every_earlier_token")


@case("SV78", "A revoked token is refused exactly like no token")
def _():
    sv("sv78_a_revoked_token_is_refused_exactly_like_no_token")


@case("SV79", "A confirmed anchor's challenge is never rotated")
def _():
    sv("sv79_a_confirmed_anchors_challenge_is_never_rotated")


@case("SV80", "A maintainer can leave on their own")
def _():
    sv("sv80_a_maintainer_can_leave_on_their_own")


@case("SV72", "An answer says what it turned on")
def _():
    sv("sv72_an_answer_says_what_it_turned_on")


@case("SV71", "The dashboard says which facts were typed, not read")
def _():
    sv("sv71_the_dashboard_says_which_facts_were_typed_not_read")


@case("SV70", "Ingest signs the head it just grew")
def _():
    sv("sv70_ingest_signs_the_head_it_just_grew")


@case("SV64", "The index is searchable by whatever broke")
def _():
    sv("sv64_the_index_is_searchable_by_what_broke")


@case("SV65", "The index carries no self-asserted name")
def _():
    sv("sv65_the_index_carries_no_self_asserted_name")


@case("SV66", "The index is signed against a tree head")
def _():
    sv("sv66_the_index_is_signed_against_a_tree_head")


@case("SV67", "No route answers a name")
def _():
    sv("sv67_no_route_answers_a_name")


@case("SV68", "A held anchor is not indexed")
def _():
    sv("sv68_a_held_anchor_is_not_indexed")


@case("SV25", "The public figures are two integers")
def _():
    sv("sv25_public_figures_are_two_integers")


@case("SV36", "A takedown degrades to unknown; it does not delete")
def _():
    sv("sv36_a_takedown_degrades_the_anchor_and_keeps_the_source")


@case("SV38", "Every takedown carries a public reason code")
def _():
    sv("sv38_every_takedown_is_logged_with_a_public_reason")


@case("SV81", "A takedown is not undone by a good probe")
def _():
    sv("sv_a_takedown_is_not_undone_by_a_good_probe")


@case("SV82", "No CDN-loaded API documentation is served, on either listener")
def _():
    sv("sv_no_interactive_api_documentation_is_served")


@case("SV83", "Both worked examples are served as the bytes spec/ publishes")
def _():
    sv("sv_both_worked_examples_are_served_as_their_own_bytes")


@case("SV112", "A repository URL collapses to one identity, and a deep link to none")
def _():
    sv("sv_a_repository_url_collapses_to_one_identity")


@case("SV113", "A repository is anchored by repository, not by forge")
def _():
    sv("sv_a_repository_is_anchored_by_repo_and_not_by_forge")


@case("SV114", "A redirect is followed only while the host is the same")
def _():
    sv("sv_a_redirect_is_followed_only_while_the_host_is_the_same")


@case("SV115", "A repository goes from claim to served card")
def _():
    sv("sv_a_repository_goes_from_claim_to_served_card")


@case("SV116", "A confusable owner is held, and the forge is not the claimant")
def _():
    sv("sv_a_confusable_owner_is_held_and_the_forge_is_not")


@case("SV117", "A repository may publish the endpoint a person can visit")
def _():
    sv("sv_a_repository_may_publish_the_endpoint_a_person_can_visit")


@case("SV84", "A read instruction cannot walk out of a granted root")
def _():
    sv("sv_a_read_cannot_walk_out_of_a_granted_root")


@case("SV85", "Proving control does not enrol a mirror; saying where the files are does")
def _():
    sv("sv_control_alone_does_not_enrol_a_mirror")


@case("SV86", "A public file is not a credential")
def _():
    sv("sv_a_public_file_is_not_a_credential")


@case("SV87", "A name cannot answer differently between the check and the connection")
def _():
    sv("sv_a_name_cannot_answer_twice")


@case("SV88", "An encoded path cannot leave the anchor")
def _():
    sv("sv_an_encoded_path_cannot_leave_the_anchor")


@case("SV89", "A deleted solution stops being served")
def _():
    sv("sv_a_deleted_solution_stops_being_served")


@case("SV90", "One bad source does not take the crawl batch down")
def _():
    sv("sv_one_bad_source_does_not_take_the_batch_down")


@case("SV91", "A diagnosis exists without anybody authoring a tree")
def _():
    sv("sv_a_diagnosis_exists_without_anybody_authoring_a_tree")


@case("SV92", "A maintainer can read their own dashboard")
def _():
    sv("sv_a_maintainer_can_read_their_own_dashboard")


@case("SV93", "A version is read and a log offered — never run or searched on a publisher's say-so")
def _():
    sv("sv_a_version_is_read_and_a_log_is_offered_never_run_or_searched")


@case("SV94", "Words a person agreed to send reach the maintainer, bounded")
def _():
    sv("sv_words_a_person_agreed_to_send_reach_the_maintainer")


@case("SV95", "A fallback for a declined question is not the answer to a value that contradicts it")
def _():
    sv("sv_a_fallback_is_not_a_match")


@case("SV40", "The log is append-only and its root is verifiable")
def _():
    sv("sv_log_is_append_only_and_verifiable")


@case("SV41", "Inclusion and consistency proofs verify")
def _():
    sv("sv_log_inclusion_and_consistency_verify")


@case("SV42", "A rewritten entry changes the root")
def _():
    sv("sv_a_rewritten_entry_is_detected")


@case("SV43", "The signed tree head verifies, and only under its own key")
def _():
    sv("sv_the_head_is_signed_and_verifiable")


@case("SV44", "A probe result refuses to be a boolean")
def _():
    sv("sv_a_probe_result_refuses_to_be_a_boolean")


@case("SV45", "Absence and inability to ask are different answers")
def _():
    sv("sv_absence_and_inability_to_ask_are_different")


@case("SV46", "Our own outage never ages an anchor")
def _():
    sv("sv_our_own_outage_never_ages_an_anchor")


@case("SV47", "The anchor probe refuses a private address")
def _():
    sv("sv_ingest_refuses_to_fetch_private_addresses")


@case("SV47a", "The manifest fetch refuses one too")
def _():
    sv("sv_ingest_refuses_a_private_address")


@case("SV48", "The epoch salt is destroyed on roll")
def _():
    sv("sv_the_epoch_salt_is_destroyed_on_roll")


@case("SV49", "seen_keys do not join across clusters or epochs")
def _():
    sv("sv_seen_keys_do_not_join_across_clusters_or_epochs")


@case("SV50", "The class is derived from the signature, never supplied")
def _():
    sv("sv_the_class_is_derived_from_the_signature")


@case("SV51", "An enterprise identity cannot be self-asserted")
def _():
    sv("sv_enterprise_identity_cannot_be_self_asserted")


@case("SV7a", "English is enforced by the database as well as the gate")
def _():
    sv("sv7_english_is_enforced_by_the_database")


@case("SV52", "The published example passes its own gate")
def _():
    sv("sv_the_published_example_passes_its_own_gate")


@case("SV3", "An endpoint outside its anchor is refused at ingest")
def _():
    sv("sv3_an_endpoint_outside_its_anchor_is_refused")


@case("SV6", "A read outside the vocabulary is refused at ingest")
def _():
    sv("sv6_a_read_outside_the_vocabulary_is_refused_at_ingest")


@case("SV35", "Naming another vendor's product is accepted")
def _():
    sv("sv35_naming_another_vendors_product_is_accepted")


@case("SV53", "A solution path cannot leave the anchor")
def _():
    sv("sv_a_solution_path_cannot_leave_the_anchor")


@case("SV54", "A fetch result refuses to be a boolean")
def _():
    sv("sv_a_fetch_result_refuses_to_be_a_boolean")


@case("SV17", "The mirror answers when the source does not")
def _():
    sv("sv17_the_mirror_answers_when_the_source_does_not")


@case("SV18", "The served commit is verifiable against the source")
def _():
    sv("sv18_the_served_commit_is_verifiable")


@case("SV55", "A project is ingested from a real host and attested")
def _():
    sv("sv_ingest_stores_a_project_and_attests_it")


@case("SV56", "A refused source does not remember its ETag")
def _():
    sv("sv_a_refused_source_does_not_remember_its_etag")


@case("SV57", "Loopback fetching is off unless switched on")
def _():
    sv("sv_loopback_fetching_is_off_by_default")


@case("SV26", "A missing decisive fact is asked for, not guessed")
def _():
    sv("sv26_a_missing_decisive_fact_is_asked_for_not_guessed")


@case("SV27", "The client answers a need through the same round loop")
def _():
    sv("sv27_the_client_answers_a_need_through_the_same_round_loop")


@case("SV28", "'Don't know' falls back and never dead-ends")
def _():
    sv("sv28_dont_know_falls_back_and_never_dead_ends")


@case("SV28a", "A question with nothing to fall back to is refused")
def _():
    sv("sv28a_a_question_with_nothing_to_fall_back_to_is_refused")


@case("SV29", "A reading switch re-partitions history; a question does not")
def _():
    sv("sv29_sv30_sv32_an_answer_that_helped_some_is_shown_where_it_forks")


@case("SV30", "A switch on a new question applies forward only, and says so")
def _():
    sv("sv29_sv30_sv32_an_answer_that_helped_some_is_shown_where_it_forks")


@case("SV31", "Runtime matching is exact along the walked path")
def _():
    sv("sv31_runtime_matching_is_exact")


@case("SV32", "A solution with mixed outcomes is surfaced")
def _():
    sv("sv29_sv30_sv32_an_answer_that_helped_some_is_shown_where_it_forks")


@case("SV58", "An ambiguous tree is refused when it is authored")
def _():
    sv("sv_an_ambiguous_tree_is_refused_when_it_is_authored")


@case("SV2c", "A key change at a live anchor is shown, never blocked")
def _():
    sv("sv2c_a_key_change_at_a_live_anchor_is_shown_never_blocked")


@case("SV2d", "Deprecation is declared, and the signals stay separate")
def _():
    sv("sv2d_2e_2f_deprecation_is_declared_not_deduced")


@case("SV10", "An anchor we never attested is unknown, not blocked")
def _():
    sv("sv10_an_anchor_we_never_attested_is_unknown_not_blocked")


@case("SV12", "A query touches no store")
def _():
    sv("sv12_a_query_touches_no_store")


@case("SV15", "A report after an attempt carries the outcome")
def _():
    sv("sv15_a_report_after_an_attempt_carries_the_outcome")


@case("SV23", "The operator's outreach view ranks unclaimed domains")
def _():
    sv("sv23_the_outreach_view_ranks_unclaimed_domains")


@case("SV37", "Nothing to remove where we host nothing")
def _():
    sv("sv37_nothing_to_remove_where_we_host_nothing")


@case("SV39", "A takedown aimed at a problem class is refused")
def _():
    sv("sv39_a_notice_aimed_at_a_problem_class_is_refused")


@case("SV59", "A takedown reason comes from a closed vocabulary")
def _():
    sv("sv_a_takedown_reason_comes_from_a_closed_vocabulary")


@case("SV60", "The integration guide matches the code")
def _():
    sv("sv_the_integration_guide_is_true")


@case("SV2b", "Gone 90 days becomes unknown, never revoked")
def _():
    sv("sv2a_2b_absence_grades_but_never_revokes")


@case("SV2e", "The forge reporting a repository archived is an owner's act")
def _():
    sv("sv2d_2e_2f_deprecation_is_declared_not_deduced")


@case("SV2f", "Commit age is stated as an observation, never a verdict")
def _():
    sv("sv2d_2e_2f_deprecation_is_declared_not_deduced")


@case("SV24", "One vendor's figures requested by another party")
def _():
    sv("sv21_sv24_no_route_produces_another_vendors_figures")


@case("SV11", "A revoked anchor is refused")
def _():
    sv("sv11_a_revoked_anchor_is_refused")


@case("SV9", "No fallback when an enterprise endpoint is down")
def _():
    sv("sv9_there_is_no_fallback_for_an_enterprise_endpoint")


@case("SV33", "A confusable anchor is held, and still reachable")
def _():
    sv("sv33_a_confusable_anchor_is_held_and_still_reachable")


@case("SV33a", "A hold withholds the attestation and nothing else")
def _():
    sv("sv33a_a_hold_prevents_attestation_but_not_serving")


@case("SV34", "An anchor carrying a mark it does not own is held")
def _():
    sv("sv34_an_anchor_carrying_a_mark_it_does_not_own_is_held")


@case("SV35a", "The name check never reads a problem class")
def _():
    sv("sv35a_a_problem_class_is_never_read_by_the_confusable_check")


@case("SV61", "The confusables table is the real UTS 39 one")
def _():
    sv("sv_the_confusable_table_is_the_real_one")


@case("SV8", "An enterprise anchor is attested from DNS and the register")
def _():
    sv("sv8_an_enterprise_anchor_is_attested_from_dns_and_the_register")


@case("SV8a", "Each half of an enterprise attestation can fail on its own")
def _():
    sv("sv8a_each_half_of_an_enterprise_attestation_can_fail_on_its_own")


@case("SV62", "An old register record says so rather than refusing")
def _():
    sv("sv_a_stale_register_mirror_says_so_rather_than_refusing")


@case("SV63", "The register client reads the real API fields, and asks only for an LEI")
def _():
    sv("sv_the_register_loader_reads_the_real_columns")


# ----------------------------------------------------------------------- main

#: Started by a different command than the counterparty, so it is named
#: separately in the message below. Seven cases fetch from it over HTTP — the
#: pages, the content negotiation on `/`, the served example and vocabularies —
#: and without this check their absence arrives as a connection-refused
#: traceback that reads like a defect instead of like "start it".
OPERATOR = "http://127.0.0.1:8725"


def services_up():
    for name, url, how in (
        ("vendor", ACME, "mise run services"),
        ("plain", PLAIN, "mise run services"),
        ("index", INDEX, "mise run services"),
        ("oss project", OSS, "mise run services"),
        ("operator server", OPERATOR, "mise run server"),
    ):
        try:
            httpx.get(url + "/", timeout=2)
        except Exception:
            return name, how
    return None


#: The client cases are public and the server cases are not, so they live in two
#: files — the split PUBLISHING.md asks for, made now rather than under time
#: pressure on publication day. Both are parsed: a row in either that claims
#: coverage it does not have is the thing this check exists to catch.
CASE_FILES = ("docs/TESTCASES.md", "docs/TESTCASES-SERVER.md")


def documented_auto():
    """Case ids the documents mark `auto`. Parsed rather than trusted, because
    a list that drifts from its runner is exactly what this file exists to
    prevent."""
    rows = set()
    lines = []
    for name in CASE_FILES:
        path = Path(name)
        assert path.exists(), f"{name} is missing — it is half the coverage claim"
        lines.extend(path.read_text().splitlines())
    for line in lines:
        if not line.startswith("|"):
            continue
        cols = [c.strip() for c in line.strip("|").split("|")]
        if len(cols) >= 4 and cols[-1] == "auto":
            cid = cols[0].strip("*").strip()
            # One or two letters then digits — "D1" and "ID1" are both case ids.
            import re as _re
            if _re.fullmatch(r"[A-Z]{1,2}\d+[a-z]?", cid):
                rows.add(cid)
    return rows


def documented_twice():
    """Ids that appear on more than one row.

    The drift check compares sets, so a case documented twice is invisible to
    it — and six of them had accumulated, saying slightly different things in
    two places. A reader cannot tell which row is the live one, and the count
    at the top of the file is wrong either way.
    """
    import re as _re
    seen = {}
    for name in CASE_FILES:
        for n, line in enumerate(Path(name).read_text().splitlines(), 1):
            if not line.startswith("|"):
                continue
            cols = [c.strip() for c in line.strip("|").split("|")]
            if len(cols) >= 4 and cols[-1] in ("auto", "manual", "open"):
                cid = cols[0].strip("*").strip()
                if _re.fullmatch(r"[A-Z]{1,2}\d+[a-z]?", cid):
                    seen.setdefault(cid, []).append(f"{name}:{n}")
    return {cid: at for cid, at in seen.items() if len(at) > 1}


# ---------------------------------------------------------------- last
#
# These two read what the cases above built. The index is signed over the
# log, and the log is empty until something has been ingested — so on a
# fresh database they have to come after the cases that put something in it.
# Registration order is execution order, so this position is the dependency.

@case("SV69", "The server's own index verifies in the client [rust]")
def _():
    cargo("index::tests::the_servers_own_index_verifies_here")


@case("W4", "Every page's script parses")
def _():
    """The same failure as `W3`, on the operator's side.

    These pages also carry a Content-Security-Policy naming a hash per script
    block, so a page can break in two silent ways: a syntax error, and a hash
    that stopped matching. Both render the layout with nothing filled in.
    """
    for f in page_files():
        scripts_parse(f, "page-syntax-check")
    for f in shared_scripts():
        r = subprocess.run(["node", "--check", str(f)], capture_output=True, text=True)
        assert r.returncode == 0, f"{f.name} does not parse:\n{r.stderr}"


@case("W5", "Every page is an explicitly routed path")
def _():
    """A directory must never become the route table.

    `SV21` and `SV67` prove no path produces another vendor's figures by
    enumerating `app.routes`. A `StaticFiles` mount collapses its whole subtree
    into one route, so dropping a file into a directory would add a public URL
    with no decorator, no review and no case — and the enumeration would stop
    being able to see it.
    """
    from starlette.routing import Mount

    from podshl.server.app import app
    paths = {r.path for r in app.routes if hasattr(r, "path")}
    missing = [p for p in PAGES if p not in paths]
    assert not missing, f"not routed: {missing}"
    assert not any(isinstance(r, Mount) for r in app.routes), \
        "a mount hides its contents from the assertions that enumerate routes"
    # CORS here would make the claim token a cross-origin credential.
    assert not app.user_middleware, "middleware appeared on the public app"
    for r in app.routes:
        if getattr(r, "path", None) in PAGES:
            assert not getattr(r, "param_convertors", {}), \
                f"a page is parameterised: {r.path} — that is a per-domain lookup in a URL"


@case("W6", "A page writes text, never markup")
def _():
    """Log entries and index rows carry values a stranger chose — anyone may
    start a claim on any host — so a page that assigned them to `innerHTML`
    would be a stored-XSS sink on the operator's own origin."""
    banned = ("innerHTML", "outerHTML", "insertAdjacentHTML",
              "document.write", "eval(", "new Function")
    for f in page_files() + shared_scripts():
        src = f.read_text(encoding="utf-8")
        for b in banned:
            assert b not in src, f"{f.name} uses {b}"


@case("W7", "A page fetches only what it is allowed to")
def _():
    """The set of URLs a page may reach is an allow-list, so a page cannot
    quietly become a new disclosure route."""
    import re

    # `/` is on every page that carries the navigation, because every one of
    # them renders the version in its footer from the server's own answer. It
    # is the page this endpoint already serves and it discloses nothing a
    # visitor did not just fetch by arriving.
    version = {"/"}
    allowed = {
        "home.html": {"/", "/stats", "/operator"},
        "publish.html": {"/example/agent.yaml", "/example/desktop/agent.yaml"} | version,
        "security.html": {"/", "/operator"},
        "projects.html": {"/index"} | version,
        "log.html": {"/log/sth", "/log/entries"} | version,
        "notice.html": {"/notice", "/operator"} | version,
        "imprint.html": {"/operator"} | version,
        "privacy.html": {"/operator"} | version,
        "imprint-unconfigured.html": set(),
        "register.html": {"/claim/"} | version,
        "dashboard.html": {"/dashboard/", "/claim/"} | version,
        "build.html": {"/vocabulary/", "/validate"} | version,
    }
    for f in page_files():
        src = f.read_text(encoding="utf-8")
        targets = re.findall(r'fetch\(\s*"([^"]*)"', src) + re.findall(r'fetch\(\s*"([^"]+)"\s*\+', src)
        for t in targets:
            base = t.split("?")[0]
            ok = any(base == a or base.startswith(a) for a in allowed[f.name])
            assert ok, f"{f.name} fetches {t!r}, which is not on its list"
        # No public page may reach the operator's own listener.
        assert "8726" not in src and "/outreach" not in src, f"{f.name} names the operator view"


@case("W8", "No public page can show a per-project figure")
def _():
    """The two lines that must not be crossed, asserted rather than promised:
    one project's numbers are never shown to anyone else, and there is no
    ranking. The public pages carry no count, and the data behind them carries
    none either — `/index` has no count field at all."""
    public = ["home.html", "publish.html", "build.html", "security.html", "projects.html",
              "log.html", "notice.html", "imprint.html", "privacy.html",
              "imprint-unconfigured.html"]
    # Field names, not words. The first version of this failed the landing page
    # for carrying the sentence "No public ranking, ever" — flagging a page for
    # stating the rule it obeys. What must never appear is a per-project figure
    # arriving as data.
    forbidden = ("reports_total", "peak_epoch_reporters", "stated_facts",
                 "model_classes", "/outreach", "/dashboard/")
    for name in public:
        src = (PAGE_DIR / name).read_text(encoding="utf-8")
        for f in forbidden:
            assert f not in src, f"{name} reads {f}, which is a per-project figure"
    # And the participant list is ordered by name, never by anything derived
    # from volume — an ordering is a ranking wearing different clothes.
    projects = (PAGE_DIR / "projects.html").read_text(encoding="utf-8")
    assert "localeCompare" in projects, "the participant list is not sorted by host"


@case("W9", "No page links an internal document")
def _():
    """`PUBLISHING.md` keeps these in the working repository only. A page citing
    one links a document the public repository does not have. `SERVER.md` and
    `TESTCASES-SERVER.md` were on this list until the server was published.

    The list comes from `sync_public.sh`, like `P8`'s: this case kept a third
    copy of it, and a list kept in three places is a list that is wrong in two
    of them the first time it grows."""
    internal = withheld_documents()
    for f in page_files():
        src = f.read_text(encoding="utf-8")
        for name in withheld_tokens(internal):
            assert name not in src, f"{f.name} names {name}"
    skip = {".git", "target", "out", "var", "node_modules", ".venv", "data", "internal"}
    published = {p.name for p in Path(".").rglob("*.md")
                 if not any(part in skip for part in p.parts)}
    missing = unresolved_documents(page_files(), published)
    assert not missing, "a page names a document the public tree does not have: " +         ", ".join(f"{f.name} -> {n}" for f, n in missing)


@case("W22", "The operator says which code it is running")
def _():
    """An operator that asks every project to publish the commit it serves has
    to publish its own.

    The mirror states the commit of everything it mirrors, precisely so that a
    third party can check the mirror against the source. The operator itself
    said nothing, and the only way to find out what `sdota.de` was running was
    to ask the maintainer — which is the shape of claim this whole project
    exists to replace.

    It is baked into the image at build time by `deploy/compose/update.sh`, from
    `git describe --tags --always`, so the string travels with the code it
    describes and cannot disagree with it. Absent stays absent: a build nobody
    told says nothing rather than claiming a version, and the page then renders
    no line at all.
    """
    import podshl.server.config as cfg

    # The chain, end to end, rather than each link on its own.
    dockerfile = (Path("deploy/compose") / "Dockerfile").read_text(encoding="utf-8")
    assert "ARG PODSHL_VERSION" in dockerfile and "ENV PODSHL_VERSION" in dockerfile,         "the image takes no version argument"
    compose = (Path("deploy/compose") / "compose.yaml").read_text(encoding="utf-8")
    assert "PODSHL_VERSION" in compose, "compose does not pass the version to the build"
    update = (Path("deploy/compose") / "update.sh").read_text(encoding="utf-8")
    assert "git describe" in update and "PODSHL_VERSION=" in update,         "the deployment does not compute or pass a version"

    # Said when it is known, and absent when it is not — asked of the route, so
    # this cannot pass on a config constant the page never reads.
    from starlette.testclient import TestClient

    from podshl.server.app import app

    before = cfg.VERSION
    try:
        cfg.VERSION = "v9.9.9-1-gdeadbee"
        with TestClient(app) as c:
            said = c.get("/", headers={"Accept": "application/json"}).json()
        assert said.get("version") == "v9.9.9-1-gdeadbee", said
        cfg.VERSION = None
        with TestClient(app) as c:
            silent = c.get("/", headers={"Accept": "application/json"}).json()
        assert "version" not in silent, (
            f"a build that was not told its version claimed one: {silent.get('version')!r}")
    finally:
        cfg.VERSION = before

    # And every page with the navigation renders it, from that answer rather
    # than from a copy of its own.
    #
    # Except the one that fetches nothing at all. `imprint-unconfigured.html` is
    # what a deployment with no imprint serves in place of every page it owes
    # one on, with an empty allow-list in `W7` — a 503 that reaches back into
    # the server is a 503 that can be wrong twice.
    nav = [f for f in page_files()
           if 'class="top"' in f.read_text(encoding="utf-8")
           and f.name != "imprint-unconfigured.html"]
    assert len(nav) >= 10, f"only {len(nav)} pages to check — is the markup shared?"
    for f in nav:
        src = f.read_text(encoding="utf-8")
        assert 'id="ver"' in src, f"{f.name} has no place to put the version"
        assert "o.version" in src, f"{f.name} never asks the server for it"


@case("W21", "A maintainer can reach their own project from any page")
def _():
    """The dashboard existed and no page linked it.

    Every page's navigation read *Publish · Register* and stopped there, and the
    footers link the imprint, the privacy notice and the log. So a maintainer who
    had published files and proved control had nowhere to click: `/dashboard` was
    reachable only by knowing the URL. Found on 2026-09-16 by the maintainer of
    this project looking for their own reports and not finding a link.

    It goes after *Register* because that is the order the work happens in —
    publish the files, prove control, then read what comes back.
    """
    nav = [f for f in page_files() if 'class="top"' in f.read_text(encoding="utf-8")]
    assert len(nav) >= 10, f"only {len(nav)} pages carry the navigation — is the markup shared?"
    for f in nav:
        assert 'href="/dashboard"' in f.read_text(encoding="utf-8"), (
            f"{f.name} has the navigation and no way to reach the dashboard from it")


@case("P8", "Nothing published names an internal document or this machine")
def _():
    """`W9` checks the served *pages*. It does not check the tree those pages
    ship inside, and that is where this went wrong twice in one day: four
    published files — a migration, a route's docstring, `SERVER.md` and
    `SECURITY.md` — ended a paragraph with "see HANDOVER.md", which the public
    repository does not contain, and the maintainer's own home path sat in
    `mise.toml`, `scripts/ci/check_baseline.py` and a comment in the anonymiser.
    The anonymiser exists to take exactly that out of other people's logs.

    `PUBLISHING.md`'s checklist says to scan "every time, not once". A checklist
    item nobody can run is a wish, so this is the item.

    Scoped to what is published: the working repository's own notes may name
    each other freely.

    **The list is read from the script that performs the split**, not kept here
    beside it. It was kept here, and the two drifted the first time the list
    grew: `CLAIM-TOKENS.md` went onto `sync_public.sh`'s list and this case went
    on treating it as published, so it failed for naming an internal document
    while being one itself. Two copies of "what is withheld" is the failure this
    whole case exists to catch, one level up.

    Parsed rather than imported, because the script is shell. A parse that comes
    back short is treated as a broken parse rather than a short list — the same
    trap as the `git ls-files` one below, where an empty answer made every loop
    run zero times and the case pass while proving nothing."""
    internal = withheld_documents()
    #: Named, with the reason, rather than exempting a directory. An applied
    #: migration is hash-pinned and cannot be edited — the runner refused this
    #: one when the reference was tidied out of a comment, which is the guard
    #: working. So the reference stays and reads as a dangling pointer to
    #: anybody outside, and the fix is forward-only: later migrations do not
    #: name an internal document, and this list must not grow.
    allowed = {
        "src/podshl/server/sql/0018_an_anchor_may_be_a_repository.sql":
            "applied and hash-pinned before the reference was noticed",
    }
    #: Walked rather than asked of git. The first version of this case called
    #: `git ls-files`, which inside the container answers `fatal: detected
    #: dubious ownership` and exits 128 — so the list was empty, every loop ran
    #: zero times, and the case passed while proving nothing. It was shown that
    #: way: a violation added to SERVER.md on purpose did not turn it red.
    skip_dirs = {".git", "target", "out", "var", "__pycache__", "node_modules",
                 ".venv", "data", "release"}
    suffixes = {".md", ".py", ".rs", ".sql", ".toml", ".sh", ".yaml", ".yml",
                ".json", ".html", ".mjs", ".ps1"}
    published: list[Path] = []
    for path in Path(".").rglob("*"):
        if any(part in skip_dirs for part in path.parts):
            continue
        if not path.is_file() or path.suffix not in suffixes:
            continue
        rel = path.as_posix()   # `str()` is `scripts\x` on Windows and matched nothing
        if rel in internal or rel == "run_testcases.py":
            continue
        published.append(path)

    # A count, not a hope. The exact number moves with the tree; an order of
    # magnitude does not, and zero is the answer this case used to get.
    assert len(published) > 100, (
        f"only {len(published)} files to check — this case cannot enumerate the "
        f"tree and would pass over an empty list, which is worse than failing")

    home = "/home/" + "dx"          # split so this line is not its own violation
    offenders: list[str] = []
    for f in published:
        try:
            src = f.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for name in sorted(withheld_tokens(internal)):
            if name in src and f.as_posix() not in allowed:
                offenders.append(f"{f} names {name}")
        if home in src:
            offenders.append(f"{f} carries the maintainer's home path")
    # The same question without the list, so it means something in the public
    # checkout too, where the list does not exist.
    names = {p.name for p in published}
    for f, name in unresolved_documents(published, names):
        if f.as_posix() not in allowed:
            offenders.append(f"{f} names {name}, which is not in the published tree")
    assert not offenders, (
        "published files reach for something the public tree does not have, or "
        "name this machine:\n  " + "\n  ".join(offenders[:10]))


@case("W17", "A refused token is said as one, where it cannot be missed")
def _():
    """The dashboard's refusal was one grey line under two paragraphs, reading
    "this dashboard is private to whoever controls the domain" — a policy, not
    "the token you typed was refused" — and a maintainer who had pasted the token
    without its first characters took it for the page not working. And "Remember
    on this device" stored the token before it was tried, so a mistyped one came
    back on every visit.

    Checked on the page's source; walked in a browser for a wrong token, an
    empty form, a remembered token that stopped working, an unreachable server
    and the right token. What it must still not do is say *which* thing was
    wrong: a missing, revoked and expired token get one answer (`SV78`), and the
    page cannot tell them apart either."""
    src = (PAGE_DIR / "dashboard.html").read_text(encoding="utf-8")
    alert = src.find('id="signin-error" role="alert"')
    assert alert != -1, "the sign-in refusal is not an announced panel"
    assert alert < src.find('<p class="why">The host goes in'), \
        "the refusal is below the explanation again, where it is easy not to see"
    load = src[src.find("function load("):src.find('$("open").addEventListener')]
    assert "o.reason || o.code" not in load, "the server's policy sentence is shown as the refusal again"
    refused = src[src.find("function refused("):src.find("function load(")]
    assert "token" in refused and "host" in refused and 'aria-invalid' in src, \
        "the refusal does not name the host and token, or does not mark the fields"
    for leak in ("revoked\"", "expired\"", "not_claimed"):
        assert leak not in refused, "the page distinguishes kinds of refusal the server deliberately does not"
    assert load.find("if (!ok)") < load.find("localStorage.setItem"), \
        "a token is remembered before it was accepted"
    opener = src[src.find('$("open").addEventListener'):src.find('$("forget").addEventListener')]
    assert "localStorage.setItem" not in opener, "the Open button stores the token before it is tried"


@case("W18", "Every list is paged, filtered and sorted the one way")
def _():
    """The dashboard had a "Show 20 more" button — eleven presses at 263
    configurations, with nothing to say where the end was — and every other
    list had nothing: the public log asked for its first thousand entries once
    and showed them as the log, which at 2,501 left out the newest 1,501.

    One component now (`list.js`): numbered pages, a filter over what the
    browser already holds, and the orders each list allows. What this holds:

    * every page with a list loads it, and no page pages by hand any more;
    * `/projects` offers host order and nothing else — any other order of a list
      of projects is a ranking;
    * `/log` reads the log page by page up to the signed size, rather than one
      fixed page from the start;
    * the file is served as JavaScript with `nosniff`, and every JSON answer
      carries `nosniff` too — a page that loads a script file permits `'self'`,
      and no API answer may then be loadable in the script's place.
    """
    import httpx

    from podshl.server import pages

    for name in ("dashboard.html", "projects.html", "log.html"):
        src = (PAGE_DIR / name).read_text(encoding="utf-8")
        assert pages.LIST_TAG in src and "PodshlList(" in src, f"{name} does not use the shared list"
        assert src.index(pages.LIST_TAG) < src.index("<script>"), f"{name} uses the list before loading it"
        assert "more \" + noun" not in src and "function paged(" not in src, f"{name} pages by hand again"

    projects = (PAGE_DIR / "projects.html").read_text(encoding="utf-8")
    sorts = projects[projects.index("sorts:"):projects.index("empty:")]
    assert sorts.count("compare:") == 2 and sorts.count("byHost") == 2, \
        "/projects can be ordered by something other than its host, and that is a ranking"

    log = (PAGE_DIR / "log.html").read_text(encoding="utf-8")
    assert "start=0&limit=1000" not in log, "/log reads one fixed page again"
    assert "end -= PAGE" in log and "tree_size" in log, "/log does not read up to the signed size"

    served = httpx.get("http://127.0.0.1:8725/list.js", timeout=10)
    assert served.status_code == 200, served.status_code
    assert served.headers["content-type"].startswith("text/javascript"), served.headers["content-type"]
    assert served.headers.get("x-content-type-options") == "nosniff", served.headers
    for path in ("/log/sth", "/index", "/dashboard/nobody.example"):
        r = httpx.get("http://127.0.0.1:8725" + path, timeout=10, headers={"Accept": "application/json"})
        assert r.headers.get("x-content-type-options") == "nosniff", \
            f"{path} ({r.status_code}) can be loaded as a script by a page that permits 'self'"


@case("W19", "What the builder writes, the mirror takes")
def _():
    """The builder on `/publish/build` writes `agent.yaml` and the solution files
    in the browser. Its writer is a block of its own with no page in it, so this
    runs exactly that code in node, on the drafts a maintainer would make —
    readings from the vocabulary, a question with choices, conditions with
    operators, a version written `1.10` that YAML would read as a number, a
    glossary, an escalation — and sends what it wrote to `/validate`, which runs
    the ingest checks (`SV108`). A builder whose own output the mirror refuses
    would be teaching the format wrong to exactly the people who asked for help.
    """
    import json
    import re

    import httpx

    src = (PAGE_DIR / "build.html").read_text(encoding="utf-8")
    block = next(b for b in re.findall(r"<script>(.*?)</script>", src, re.S) if "const Emit" in b)
    assert "document" not in block and "fetch(" not in block, "the builder's writer reaches for the page"

    drafts = [
        {"anchor": "https://example.org/proj/", "commit": "1.10", "status": "active", "langs": "en, de",
         # Both shapes at once: a class with the sentence a person picks it by,
         # and one without, which is every class published before `describes`.
         "classes": [{"id": "proj.install.wrong-archive",
                      "describes": "It will not start — no window, no error"},
                     "proj.search.empty"],
         "probes": [
             {"kind": "machine", "id": "os.name", "describes": "Operating system", "why": "One build each",
              "op": "os_fact", "params": {"name": "os"}},
             {"kind": "machine", "id": "gpu.name", "why": "GPU path",
              "op": "run_tool", "params": {"tool": "nvidia-smi",
                                           "args": ["--query-gpu=name", "--format=csv,noheader"]}},
             {"kind": "human", "id": "proj.symptom", "describes": "What goes wrong", "why": "Decides it",
              "prompt": "Which is closest?", "choices": "it will not start\nsearch returns nothing\n",
              "optional": True},
         ],
         "solutions": [
             {"id": "wrong-archive", "problem_class": "proj.install.wrong-archive",
              "when": [{"fact": "os.name", "op": "in", "value": "linux, macos"},
                       {"fact": "proj.symptom", "op": "=", "value": "it will not start"}],
              "severity": "high", "action": "report_only", "params": {},
              "body": "Take the other archive.\n\n    tar xf proj-aarch64.tar.gz\n"},
             {"id": "reindex", "problem_class": "proj.search.empty",
              "when": [{"fact": "proj.symptom", "op": "=", "value": "search returns nothing"}],
              "severity": "medium", "action": "report_only", "params": {},
              "body": "Rebuild the index: `proj reindex`."},
         ],
         "glossary": "brain, wheel",
         "escalate": {"on": True, "reason": "Nothing explains it", "queue": "github-issues",
                      "target": "https://github.com/x/proj/issues", "reply_via": ["ticket_url", "none"]}},
        # **A project that has stopped, which is the one shape the builder could
        # not express.** `successor` is a key ingest accepts and the form had no
        # field for, so a maintainer following "Saying a project is finished" in
        # INTEGRATING.md had to hand-edit the file the builder wrote. Nothing was
        # lost when they loaded it back — unknown keys are kept verbatim — but
        # "keeps it if you typed it elsewhere" is not the same as "can write it".
        {"anchor": "https://example.org/stopped/", "commit": "v9.9.9", "status": "deprecated",
         "successor": "https://example.org/the-one-that-took-over", "langs": "en",
         "classes": ["stopped.install.wrong-archive"],
         "probes": [{"kind": "machine", "id": "os.name", "op": "os_fact", "params": {"name": "os"}}],
         "solutions": [{"id": "wrong-archive", "problem_class": "stopped.install.wrong-archive",
                        "when": [{"fact": "os.name", "op": "=", "value": "linux"}],
                        "severity": "low", "action": "report_only",
                        "body": "This project has stopped. Use the successor."}]},
        # The example the page itself offers.
        {"anchor": "https://example.org/your-project/", "commit": "v1.0.0", "langs": "en",
         "classes": ["your-project.install.wrong-archive"],
         "probes": [{"kind": "machine", "id": "os.name", "op": "os_fact", "params": {"name": "os"}},
                    {"kind": "machine", "id": "os.arch", "op": "os_fact", "params": {"name": "arch"}}],
         "solutions": [{"id": "wrong-archive", "problem_class": "your-project.install.wrong-archive",
                        "when": [{"fact": "os.name", "op": "=", "value": "linux"},
                                 {"fact": "os.arch", "op": "=", "value": "aarch64"}],
                        "severity": "high", "action": "report_only",
                        "body": "Take the aarch64 one instead."}]},
    ]
    runner = VAR / "build-emit-check.js"
    runner.parent.mkdir(parents=True, exist_ok=True)
    runner.write_text(block + "\nconst drafts = " + json.dumps(drafts) +
                      ";\nconsole.log(JSON.stringify(drafts.map(d => Emit.files(d))));\n", encoding="utf-8")
    out = subprocess.run(["node", str(runner)], capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
    written = json.loads(out.stdout)

    assert 'commit: "1.10"' in written[0]["agent.yaml"], "a version that looks like a number went out unquoted"
    # The sentence a person picks a class by survives the writer, and a class
    # without one stays the bare string it has always been — a builder that
    # promoted every class to a mapping would rewrite files nobody asked it to.
    first = written[0]["agent.yaml"]
    assert 'id: "proj.install.wrong-archive"' in first, (
        f"the builder dropped the class it was given:\n{first}")
    assert 'describes: "It will not start — no window, no error"' in first, (
        f"the builder dropped the sentence a class is picked by:\n{first}")
    assert '- "proj.search.empty"' in first, (
        f"a class with no sentence was promoted to a mapping:\n{first}")
    stopped = written[1]["agent.yaml"]
    assert 'status: deprecated' in stopped and \
           'successor: "https://example.org/the-one-that-took-over"' in stopped, (
        f"the builder cannot say where a stopped project continues:\n{stopped}")
    # And never on a project that has not stopped: a successor to something
    # still running is a claim nobody can act on.
    assert "successor:" not in first, (
        f"an active project was given a successor:\n{first}")
    import hashlib
    for draft, files in zip(drafts, written):
        solutions = {k: v for k, v in files.items() if k != "agent.yaml"}
        assert len(solutions) == len(draft["solutions"]), files.keys()
        r = httpx.post("http://127.0.0.1:8725/validate", timeout=30, json={
            "agent_yaml": files["agent.yaml"], "solutions": solutions, "anchor": draft["anchor"]})
        v = r.json()
        assert r.status_code == 200 and v["accepted"], (
            f"the mirror refuses what the builder wrote: {v}\n{files['agent.yaml']}")
        assert not v["warnings"], f"the builder's own draft carries warnings: {v['warnings']}"
        # Every file it lists, with the digest of exactly the bytes it wrote
        # (`SV133`), so the mirror can check the project with one request.
        assert v["parsed"]["manifest"].get("solution_sha256") == {
            rel: hashlib.sha256(text.encode("utf-8")).hexdigest()
            for rel, text in solutions.items()}, (
            f"the builder's digests are not the files':\n{files['agent.yaml']}")

    # And a file edited after the builder wrote it is refused, by the same
    # sentence ingest gives.
    files = written[0]
    rel = next(k for k in files if k != "agent.yaml")
    edited = {k: v for k, v in files.items() if k != "agent.yaml"}
    edited[rel] = edited[rel] + "One more line.\n"
    v = httpx.post("http://127.0.0.1:8725/validate", timeout=30, json={
        "agent_yaml": files["agent.yaml"], "solutions": edited,
        "anchor": drafts[0]["anchor"]}).json()
    assert not v["accepted"] and "SHA-256" in v["refused"].get(rel, ""), v


@case("W20", "A project's existing files survive the builder unchanged in meaning")
def _():
    """A maintainer who already publishes loads their files into the builder to
    change one thing. What must not happen is the builder quietly changing the
    rest: a condition `>=3.13` tidied into `>= 3.13`, a choice with a comma in it
    split in two, the question a reading carries dropped because the form has no
    field for it, a second language lost.

    So real files — engram's, and both worked examples in `spec/` — are posted to
    `/validate`, which returns what ingest's parser made of them; the builder's
    own `Emit.fromParsed` turns that into the form and `Emit.files` writes it
    back; and the written files are parsed again. Both parses are the same
    parser's, and they must be equal, key for key. What the form cannot edit is
    kept verbatim rather than discarded, and this is what holds it to that.
    """
    import json
    import re

    import httpx

    src = (PAGE_DIR / "build.html").read_text(encoding="utf-8")
    block = next(b for b in re.findall(r"<script>(.*?)</script>", src, re.S) if "const Emit" in b)

    corpora = [Path("examples/engram/.podshl"), Path("spec/example/.podshl"),
               Path("spec/example-desktop/.podshl")]
    for root in corpora:
        agent = (root / "agent.yaml").read_text(encoding="utf-8")
        solutions = {f"solutions/{p.name}": p.read_text(encoding="utf-8")
                     for p in sorted((root / "solutions").glob("*.md"))}
        endpoint = re.search(r"^endpoint:\s*(\S+)", agent, re.M).group(1).strip("\"'")

        def validate(agent_yaml, sols):
            r = httpx.post("http://127.0.0.1:8725/validate", timeout=30,
                           json={"agent_yaml": agent_yaml, "solutions": sols, "anchor": endpoint})
            assert r.status_code == 200, r.text
            return r.json()

        before = validate(agent, solutions)
        assert before["accepted"], f"{root}: the original files are not accepted: {before['refused']}"
        # And the files people copy carry nothing a maintainer would not want —
        # the first import of the Python example found a solution answering
        # a class its manifest never declared, which no client could reach.
        assert not before["warnings"], f"{root}: {before['warnings']}"

        runner = VAR / "build-roundtrip.js"
        runner.write_text(block + "\nconst p = " + json.dumps(before["parsed"]) +
                          ";\nconsole.log(JSON.stringify(Emit.files(Emit.fromParsed(p.manifest, p.solutions))));\n",
                          encoding="utf-8")
        out = subprocess.run(["node", str(runner)], capture_output=True, text=True)
        assert out.returncode == 0, out.stderr
        written = json.loads(out.stdout)

        after = validate(written["agent.yaml"], {k: v for k, v in written.items() if k != "agent.yaml"})
        assert after["accepted"], f"{root}: the builder wrote files the mirror refuses: {after['refused']}"
        # The digests are new and derived: the builder writes them from the
        # files it writes, so they are held to those files rather than to the
        # originals, which carry none.
        digests = after["parsed"]["manifest"].pop("solution_sha256", None)
        assert digests == {k: __import__("hashlib").sha256(v.encode("utf-8")).hexdigest()
                           for k, v in written.items() if k != "agent.yaml"}, (root, digests)
        before["parsed"]["manifest"].pop("solution_sha256", None)
        assert after["parsed"]["manifest"] == before["parsed"]["manifest"], (
            f"{root}: agent.yaml means something else after the builder:\n"
            f"before {before['parsed']['manifest']}\nafter  {after['parsed']['manifest']}")
        assert after["parsed"]["solutions"] == before["parsed"]["solutions"], (
            f"{root}: a solution means something else after the builder:\n"
            + "\n".join(f"{k}:\n  before {before['parsed']['solutions'].get(k)}\n  after  {v}"
                        for k, v in after["parsed"]["solutions"].items()
                        if v != before["parsed"]["solutions"].get(k)))
        assert after["trees"] == before["trees"] and after["no_tree"] == before["no_tree"], root


# **Eight rows in `TESTCASES.md` said `auto` and named nothing here.** The work
# was done and the tests exist — in the client, in Rust — but no case bound a
# documented promise to one of them, so the suite reported them missing on every
# run and exited non-zero for it. A suite that ends red for bookkeeping teaches
# people to read past red, which is the expensive part. Each row is bound to the
# test that actually keeps it; rename the test and the case fails, which is the
# whole point of `cargo(...)`.


@case("PR1", "Where this machine says it got the software [rust]")
def _():
    cargo("provenance::tests::only_a_real_disagreement_is_worth_saying")
    cargo("provenance::tests::what_is_not_part_of_who_published_something_is_folded_away")
    cargo("provenance::tests::a_package_name_that_would_need_escaping_is_refused")


@case("PR2", "Every text the window names exists in every language [rust]")
def _():
    # Two halves: the binary's own sentences, and the window's.
    cargo("msg::tests::every_message_the_binary_says_has_a_sentence")
    cargo("ui_contract::tests::every_language_has_every_sentence")


@case("IS1", "A diagnosis has an exit that is not a report to the operator [rust]")
def _():
    cargo("issue::tests::the_footer_can_be_switched_off")
    cargo("issue::tests::measured_and_supplied_are_two_sections")


@case("IS2", "Nothing reaches the issue that a report would have held back [rust]")
def _():
    cargo("issue::tests::nothing_leaves_that_the_consent_panel_would_have_taken_out")
    cargo("issue::tests::a_value_cannot_break_out_of_its_row")
    cargo("issue::tests::a_model_answer_is_marked_before_it_is_quoted")


@case("IS3", "A version the machine never reported is named before it is copied [rust]")
def _():
    cargo("issue::tests::a_version_the_machine_never_reported_is_named")
    cargo("issue::tests::it_says_nothing_when_there_is_nothing_to_compare_against")
    cargo("issue::tests::a_bare_integer_is_not_a_version")
    cargo("issue::tests::the_mark_is_in_the_text_before_the_answer_it_qualifies")


@case("OM1", "The desktop's own agent is the model, with its tools denied [rust]")
def _():
    cargo("omarchy::tests::the_only_measured_agent_is_called_with_its_tools_denied")
    cargo("omarchy::tests::an_agent_name_is_a_program_name_or_it_is_refused")


@case("OM2", "Two switches that read like a fence and are not [rust]")
def _():
    # The measurement is in the source; what is kept here is its conclusion —
    # only `--disallowed-tools` refused in both places, and it is what is used.
    cargo("omarchy::tests::the_only_measured_agent_is_called_with_its_tools_denied")


@case("OM3", "The agent is only looked for on an Omarchy desktop [rust]")
def _():
    cargo("omarchy::tests::the_agent_is_only_looked_for_on_an_omarchy_desktop")


@case("OM4", "A fresh install on Omarchy uses the desktop's agent without being asked to [rust]")
def _():
    cargo("llm::tests::the_desktop_agent_is_the_model_until_somebody_chooses")


@case("OM5", "Asking the agent leaves no transcript behind [rust]")
def _():
    cargo("omarchy::tests::the_only_measured_agent_is_called_with_its_tools_denied")
    cargo("omarchy::tests::the_leftover_session_directory_is_found_and_only_removed_when_empty")


@case("RS1", "Every Rust test passes, named by a case or not [rust]")
def _():
    cargo_all()



def check_drift():
    documented = documented_auto()
    implemented = {c for c, _, _ in CASES}
    missing = sorted(documented - implemented)
    extra = sorted(implemented - documented)
    return missing, extra


def main():
    missing = services_up()
    if missing:
        name, how = missing
        print(f"Service '{name}' is not answering — start it with `{how}`.")
        return 2

    width = max(len(c) for c, _, _ in CASES)
    passed = failed = 0
    failures = []
    for cid, desc, fn in CASES:
        try:
            fn()
            print(f"  \033[32m✓\033[0m {cid:<{width}}  {desc}")
            passed += 1
        except Exception as e:
            print(f"  \033[31m✗\033[0m {cid:<{width}}  {desc}")
            failures.append((cid, e, traceback.format_exc()))
            failed += 1

    print(f"\n{passed} passed, {failed} failed, {len(CASES)} run")
    if failures:
        print("\nFailures in detail:")
        for cid, e, tb in failures:
            # **The message, not the last line of the traceback.** A `cargo`
            # delegation ends with "error: test failed, to rerun pass `--bin
            # podshl-client`", which names neither the test nor what it
            # asserted — so the first CI run of this suite reported four
            # failures and said nothing whatever about any of them. What a case
            # puts in its own assertion is the part somebody can act on.
            body = str(e).strip() or tb.strip()
            lines = body.splitlines()
            shown = lines if len(lines) <= 40 else lines[:6] + ["  …"] + lines[-32:]
            print(f"\n── {cid} ──\n" + "\n".join(shown))
    undone, extra = check_drift()
    twice = documented_twice()
    if twice:
        print("\n\033[31mThese cases are documented more than once — which line holds?\033[0m")
        for cid, at in sorted(twice.items()):
            print(f"  {cid}: {', '.join(at)}")
    if undone:
        print(f"\n\033[31mTESTCASES.md marks these cases `auto`, but they do not exist "
              f"here:\033[0m {', '.join(undone)}")
    if extra:
        print(f"\n\033[33mImplemented here, not listed as `auto` in TESTCASES.md:\033[0m "
              f"{', '.join(extra)}")

    print("\nNot covered: the lines marked `manual` and `open` in TESTCASES.md,"
          "\nabove all the Rust client end to end through its window.")
    return 1 if (failed or undone or twice) else 0


if __name__ == "__main__":
    sys.exit(main())
