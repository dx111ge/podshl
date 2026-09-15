//! Properties of the window that the compiler cannot see.
//!
//! The client has no automated test through its own window — driving a GUI with
//! synthetic keystrokes is the weakest verification available and it collides
//! with whoever is actually using the machine. So the orchestration lives in
//! `ui/index.html`, and a handful of its properties are load-bearing: the order
//! of consent, the fact that a refusal returns before anything is sent, the
//! fact that every capability the binary registers is actually reachable.
//!
//! Checking those by reading the source is weaker than exercising them, and the
//! weakness is stated rather than hidden. It is still worth doing: this is the
//! check that caught four commands which were registered and called by nothing,
//! and a capability nobody calls is a claim nobody honours.
//!
//! These lived in the Python suite, where they read Rust and JavaScript source
//! from a third language. Whatever else that was, it was not the client
//! answering for itself.

use std::path::PathBuf;

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn ui_source() -> String {
    let p = crate_dir().join("ui/index.html");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

pub fn main_source() -> String {
    let p = crate_dir().join("src/main.rs");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// Every command name in `generate_handler![...]`.
pub fn registered_commands(main_rs: &str) -> Vec<String> {
    let start = main_rs
        .find("generate_handler![")
        .expect("main.rs no longer registers any commands");
    let block = &main_rs[start..];
    let end = block.find(']').expect("unterminated generate_handler!");
    block[..end]
        .replace('\n', " ")
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W1: a command nobody calls is a claim nobody honours. This has already
    /// caught four, including the applicability gate.
    #[test]
    fn every_registered_command_is_reachable_from_the_window() {
        let ui = ui_source();
        let dead: Vec<String> = registered_commands(&main_source())
            .into_iter()
            .filter(|c| !ui.contains(&format!("\"{c}\"")))
            .collect();
        assert!(dead.is_empty(), "registered but never called: {}", dead.join(", "));
    }

    /// W2: the applicability gate exists and something calls it. A gate that is
    /// implemented and never consulted is a comment.
    #[test]
    fn the_applicability_gate_is_actually_applied() {
        assert!(ui_source().contains("\"applicability\""), "the check exists but nothing calls it");
    }

    /// T1: declining the report returns before anything is sent. The refusal
    /// branch has to come first in the source as well as at runtime — if
    /// `send_report` were reachable ahead of the question, the consent would be
    /// decorative.
    #[test]
    fn declining_the_report_returns_before_anything_is_sent() {
        let ui = ui_source();
        let offer = ui
            .split("async function offerReport")
            .nth(1)
            .expect("the report offer is gone");
        let refusal = offer.find("if(!await ask(el))").expect("the report is sent without asking");
        let send = offer.find("\"send_report\"").expect("nothing sends the report");
        assert!(
            refusal < send,
            "the report is sent before the user is asked to allow it"
        );
        assert!(
            offer[refusal..send].contains("return"),
            "declining does not return — the send is reached either way"
        );
    }

    /// The standing check comes before the offer, not after it. Asking a user to
    /// spend effort on a vendor that has never once responded is how the channel
    /// gets discredited for everybody else.
    #[test]
    fn a_vendors_standing_is_checked_before_the_button_is_offered() {
        let ui = ui_source();
        let offer = ui.split("async function offerReport").nth(1).unwrap();
        let standing = offer.find("\"vendor_standing\"").expect("standing is never consulted");
        let preview = offer.find("\"preview_report\"").expect("nothing previews the report");
        assert!(standing < preview, "the report is built before the vendor is judged worth it");
    }

    /// Contributing to the index is a separate decision and must be asked
    /// separately — and the preview must not be the act.
    #[test]
    fn contributing_to_the_index_is_asked_separately() {
        let ui = ui_source();
        let f = ui.split("async function offerContribution").nth(1).expect("no contribution flow");
        assert!(f.contains("dryRun:true"), "the preview is not a dry run");
        let ask = f.find("await ask(el)").expect("contributing is never asked about");
        let real = f.find("dryRun:false").expect("nothing actually contributes");
        assert!(ask < real, "the contribution is sent before the user agrees to it");
    }

    /// ND4: the vendor path loops on `need` instead of answering once, and a
    /// skipped question is recorded so the endpoint stops asking.
    #[test]
    fn the_vendor_path_loops_on_need() {
        let ui = ui_source();
        assert!(
            ui.contains("for(let round=1; rem.need && rem.need.length; round++)"),
            "the vendor path is single-shot again"
        );
        assert!(ui.contains("\".declined\""), "a skipped question is not recorded as declined");
    }

    /// ND7: a question left empty is recorded as declined, on every panel that
    /// asks one.
    ///
    /// "Don't know must never dead-end" is a wire convention in `SPEC.md`:
    /// `<probe id>.declined`. The need loop had always written it. The panel
    /// that *arms* a question — where a machine probe read nothing and the
    /// publisher's `when_missing` question takes over — recorded an answer and
    /// recorded nothing at all for a skip. So the endpoint was never told, and
    /// asked again.
    ///
    /// That is the root of the 37-round loop `ND6` now stops: a card whose
    /// firmware carries no serial, a person who cannot read the sticker, and a
    /// vendor correctly waiting for a fact the client had decided not to
    /// mention. Two other bugs sat on top of it, and neither was the cause.
    #[test]
    fn a_question_left_empty_is_recorded_as_declined() {
        let ui = ui_source();
        // Every panel that collects answers into `FACTS` also records the
        // absence of one. Found by class, so a fourth panel is covered when it
        // is written rather than when it breaks.
        for (class, what) in [(".ai", "the armed question"),
                              (".nq", "the need round"),
                              (".rq", "the hand-off")] {
            let mut saw = false;
            for (at, _) in ui.match_indices(&format!("querySelectorAll(\"{class}\")")) {
                let rest = &ui[at..];
                let body = &rest[..rest.find("});").map(|e| e + 3).unwrap_or(rest.len())];
                if !body.contains("FACTS[") && !body.contains("extra[") {
                    continue;               // a pattern check or a listener, not a collector
                }
                saw = true;
                // The *assignment*, not the word. The first version of this
                // looked for ".declined" and was satisfied by the comment
                // explaining why the assignment mattered — a source-text check
                // that a sentence can pass is not a check, and this one had to
                // be caught by reverting the fix and watching it stay green.
                let records = body.contains(".declined\"]=true")
                    || body.contains(".declined`]=true")
                    || body.contains("bad=true");
                assert!(records,
                        "{what} keeps an answer and forgets a skip, so the endpoint is \
                         never told it was asked something nobody can answer: {body}");
            }
            assert!(saw, "nothing collects answers from {class} any more");
        }
    }

    /// ND6: a round that cannot make progress ends the loop.
    ///
    /// `SPEC.md` is normative — an endpoint receiving `<id>.declined` must not
    /// ask again for that fact — and in the same breath it says an endpoint
    /// returning `need` indefinitely "will loop until the user stops it". That
    /// leaves the person to notice, and the way they notice is by clicking
    /// through the same question a dozen times.
    ///
    /// This walked 37 rounds in the real window before anybody saw it, and only
    /// because a card with no firmware serial finally took the branch. A round
    /// whose every probe is already answered or already declined cannot make
    /// progress whatever the endpoint intended, and that is decidable in the
    /// client without judging the vendor's reasoning.
    #[test]
    fn a_round_that_cannot_make_progress_ends_the_loop() {
        let ui = ui_source();
        let loop_body = ui
            .split("for(let round=1; rem.need && rem.need.length; round++){")
            .nth(1)
            .expect("the vendor path no longer loops on need");

        let guard = loop_body
            .find("const fresh=rem.need.filter")
            .expect("nothing checks whether a round asks for anything new");
        // Before the panel is built, not after: the point is that the person is
        // never shown the question a fourth time.
        let panel = loop_body.find("const el=add(").expect("the need panel is gone");
        assert!(guard < panel,
                "the loop guard runs after the question has already been put to the person");
        assert!(loop_body[guard..panel].contains("break;"),
                "a round that cannot make progress does not end the loop");
        // Both halves of "already": answered, and answered with "I don't know".
        let check = &loop_body[guard..panel];
        assert!(check.contains("p.id in FACTS") && check.contains(".declined\") in FACTS"),
                "a round is judged new by only one of the two ways it can be old: {check}");
    }

    /// ND5: there is a way out of the loop as well as a way past one question.
    /// Only the user ends a diagnosis.
    #[test]
    fn the_need_round_offers_a_way_to_stop_as_well_as_to_skip() {
        let ui = ui_source();
        assert!(ui.contains("needskip"), "no way to skip one question");
        assert!(ui.contains("need_stop"), "no way to stop the loop");
    }

    /// LG5: local translation is reached only where the vendor did not serve the
    /// language — and the original is rendered alongside, so a consent decision
    /// never rests on a translation nobody can check.
    #[test]
    fn translation_is_only_reached_when_the_vendor_lacks_the_language() {
        let ui = ui_source();
        assert!(
            ui.contains("if((triaged.lang_served||\"en\") !== LANG)"),
            "the translation gate is gone"
        );
        assert!(ui.contains(".bi-o{display:none}"), "the original variant is not rendered alongside");
    }

    /// H7: the client refuses a reply channel it cannot honour, in the command
    /// rather than in the window — a check the UI could skip is not a check.
    #[test]
    fn the_escalate_command_refuses_a_channel_it_cannot_honour() {
        assert!(
            main_source().contains("handover::channel_known(&reply_via)"),
            "the escalate command does not check the reply channel"
        );
    }

    /// H8: required fields are validated locally, before a case is opened. Not
    /// after an RMA has been raised against an address that is not one.
    #[test]
    fn required_fields_are_validated_before_a_case_is_opened() {
        assert!(ui_source().contains("if(bad) return;"), "invalid input would reach the vendor");
    }

    /// P4: a question with choices constrains the answer to the list — and does
    /// not arrive with one of them already in it.
    ///
    /// The first half was always true: every such question is a `<select>` built
    /// from exactly `p.choices`, so there is no way to return anything else. The
    /// second half was not. Three of the four asked with the first choice
    /// selected, so a person clicking through sent "screen flickering" without
    /// having chosen it — a fact about their machine, in their name, that they
    /// never stated. One site already had the empty option and the others had
    /// grown without it, which is how a rule kept in four places goes.
    ///
    /// It is the same rule as the refusing button holding focus: a panel may not
    /// answer for the person who is looking at it.
    #[test]
    fn a_question_with_choices_offers_no_answer_of_its_own() {
        let ui = ui_source();
        let mut checked = 0;
        for (at, _) in ui.match_indices("<select class=\"") {
            let rest = &ui[at..];
            let block = &rest[..rest.find("</select>").map(|e| e + 9).unwrap_or(rest.len())];
            // Only the panels that ask a person a published question. The
            // language picker and the model settings are the window's own
            // controls, where a current value is the right thing to show.
            if !block.contains("data-id=") {
                continue;
            }
            checked += 1;
            assert!(block.contains("p.choices.map"),
                    "a question's options are not exactly the choices published: {block}");
            assert!(block.contains("<option value=\"\">"),
                    "a question arrives with an answer already selected, so clicking \
                     through states a fact the person never chose: {block}");
        }
        assert!(checked >= 4, "only {checked} question selects found - the shape has changed");

        // And an unanswered *required* field does not open a case. It could not
        // happen while the first choice was pre-selected, so nothing checked it.
        let esc = ui.split("if((plan.require||[]).length){").nth(1)
            .expect("the hand-off no longer asks for what the vendor requires");
        assert!(esc.contains("if(!v){"),
                "a required field left unanswered still opens a case in somebody's name");
    }

    /// C4a: the consent panel shows the values *as they will be sent*.
    ///
    /// It showed `FACTS` — what the machine read and what the person typed,
    /// raw — while the request anonymised on the way out. So a Windows account
    /// name sat on screen under a sentence promising "only this goes — nothing
    /// about you as a person". Both halves were true and the pair was a lie:
    /// the account name was not going, and nothing on the panel said so.
    ///
    /// Which is the worse direction than a leak, for a product whose whole
    /// claim is that you can see what leaves: a person who reads their own user
    /// name there has every reason to conclude the promise is false and stop.
    /// It was reported exactly that way.
    #[test]
    fn the_consent_panel_shows_the_values_as_they_will_be_sent() {
        let ui = ui_source();
        // Both send panels build their snapshot from the anonymised facts, and
        // the anonymising is a command rather than a repetition of the rules in
        // JavaScript — one implementation, the one `flow::diagnose` uses.
        let snapshots = ui.matches("const sent=await asSent(FACTS);").count();
        assert!(snapshots >= 2,
                "only {snapshots} send panels render what is actually sent");
        assert_eq!(ui.matches("const SHOWN={...FACTS};").count(), 0,
                   "a send panel still snapshots the raw facts");
        assert!(ui.contains("\"facts_as_sent\""),
                "the panel anonymises in the window rather than through the client");
        // And it says what was taken out, as the free-text panel does: a value
        // silently different from what was typed is its own kind of lie.
        assert!(ui.matches("replacedLine(sent.replaced)").count() >= 2,
                "a panel shows anonymised values without saying anything was replaced");
    }

    /// C4b: a person can change their own words before they go.
    ///
    /// Asked for after seeing a real account name in a real panel: *can I make
    /// that `xxx` instead?* Only what they typed is editable — a reading is
    /// what the machine said and editing it would make the report a fiction —
    /// and emptying a box withdraws that answer as `<id>.declined` rather than
    /// sending an empty string.
    #[test]
    fn a_person_can_change_their_own_words_before_they_go() {
        let ui = ui_source();
        assert!(ui.contains("const editableIn = obj => Object.keys(obj).filter(k =>"),
                "nothing decides which values a person may change");
        assert!(ui.contains("TYPED_IDS().has(k)"),
                "a machine reading is editable, which would make the report a fiction");
        let writeback = ui.matches("send.querySelectorAll(\"textarea.sv\")").count()
            + ui.matches("tx.querySelectorAll(\"textarea.sv\")").count();
        assert!(writeback >= 4,
                "the edits are offered and not read back on both panels ({writeback} sites)");
        // Emptied means withdrawn, which is a wire fact and not an empty string.
        assert!(ui.contains("SHOWN[k+\".declined\"]=true;"),
                "emptying an answer sends an empty string rather than withdrawing it");
    }

    /// C4: what is shown before sending is what is sent.
    ///
    /// Marked `manual` for a long time, and the reason it could not be checked
    /// was that it was only ever true by coincidence: the consent panel was
    /// built by reading `FACTS`, and the request that followed read `FACTS`
    /// again — two reads of a mutable object with a person's decision in
    /// between. Nothing moved it, so nothing was wrong; but "nothing currently
    /// moves it" is not a property, it is a fact about today's code, and it is
    /// the kind that stops being true in a patch that looks unrelated.
    ///
    /// `SHOWN` is the object as the panel displayed it, and it is what travels.
    /// A fact arriving after the panel is drawn cannot be sent without being
    /// shown, because there is nothing left that could send it. That turns C4
    /// from something a person had to watch into something the source states.
    #[test]
    fn what_is_shown_before_sending_is_what_is_sent() {
        let ui = ui_source();

        // Every consent panel that lists values snapshots them. Since `C4a`
        // the snapshot is the *anonymised* facts: what is shown and what is
        // sent are one object, and it is the one that leaves.
        let snapshots = ui.matches("const SHOWN=sent.facts;").count();
        assert!(snapshots >= 2,
                "only {snapshots} send panels snapshot what they show - the vendor path \
                 and the published path each have one");

        // And the calls that carry facts off this device carry the snapshot.
        for command in ["diagnose", "ask_published"] {
            let call = format!("\"{command}\"");
            let mut found = 0;
            for at in ui.match_indices(&call).map(|(i, _)| i) {
                let rest = &ui[at..];
                let args = &rest[..rest.find("})").unwrap_or(rest.len())];
                if !args.contains("facts:") {
                    continue;
                }
                found += 1;
                assert!(args.contains("facts:SHOWN"),
                        "{command} is sent facts that were never put in front of anybody: {args}");
            }
            assert!(found > 0, "{command} is never called with facts - has it been renamed?");
        }

        // The snapshot grows only where a panel showed the additions and the
        // person answered them, so `SHOWN` means one thing everywhere: what has
        // been put in front of them and agreed to.
        assert!(ui.contains("Object.assign(SHOWN, FACTS);"),
                "the need rounds do not fold their answers into what may be sent");
    }

    /// W16: a command that grades an answer by what it rests on is given what
    /// the person typed.
    ///
    /// `typed` is optional on the Rust side, because a caller that has nothing
    /// to say about provenance should not be forced to lie — and optional is
    /// exactly how the follow-up came to omit it. An answer resting on a value
    /// the person supplied was then graded `measured`, which is the one
    /// mis-grading the whole distinction exists to prevent, in the place it
    /// matters most: the follow-up is where they have just typed something.
    ///
    /// Derived from `main.rs` rather than listed here, so a third command that
    /// takes `typed` is covered the day it is written.
    #[test]
    fn every_command_that_grades_an_answer_is_told_what_was_typed() {
        let main = main_source();
        let ui = ui_source();
        let mut wants: Vec<String> = vec![];
        for block in main.split("#[tauri::command]").skip(1) {
            let head = block.split('{').next().unwrap_or("");
            if !head.contains("typed: Option<Vec<String>>") {
                continue;
            }
            let name = head
                .split("fn ")
                .nth(1)
                .and_then(|r| r.split('(').next())
                .unwrap_or("")
                .trim()
                .to_string();
            assert!(!name.is_empty(), "a command taking `typed` has no name");
            wants.push(name);
        }
        assert!(wants.len() >= 2, "only {} commands take `typed` - has the grading gone?", wants.len());

        for name in wants {
            let call = format!("\"{name}\"");
            let at = ui.find(&call).unwrap_or_else(|| panic!("{name} is registered and never called"));
            // The invoke's own argument object, not the rest of the file: the
            // next `})` closes it.
            let rest = &ui[at..];
            let args = &rest[..rest.find("})").unwrap_or(rest.len())];
            assert!(args.contains("typed:"),
                    "{name} is called without `typed`, so an answer resting on what the \
                     person said would be graded as a measurement: {args}");
        }
    }

    /// W15: the conversation is the only thing that scrolls, and it can.
    ///
    /// `.app` is a full-height flex column: a bar, the conversation, and the
    /// footer the next question is typed into. A flex item's automatic minimum
    /// size is its *content*, so the middle one never shrank below the panels
    /// inside it — the column grew past the window, `overflow-y` had nothing to
    /// scroll because the box was already as tall as its content, and the
    /// footer was pushed off the bottom of a window that does not scroll.
    ///
    /// It looks fine until the conversation is taller than the window, which is
    /// every window after a few panels — so a screenshot of the first screen
    /// does not show it and a file check has to.
    #[test]
    fn the_conversation_scrolls_and_the_footer_stays() {
        let ui = ui_source();
        let rule = |sel: &str| -> String {
            let at = ui.find(&format!("
  {sel}{{")).unwrap_or_else(|| panic!("no {sel} rule"));
            let rest = &ui[at + 3 + sel.len()..];
            rest[..rest.find('}').expect("unterminated rule")].to_string()
        };
        let main = rule("main");
        assert!(main.contains("overflow-y:auto"), "the conversation no longer scrolls: {main}");
        assert!(main.contains("min-height:0"),
                "a scrolling flex item without min-height:0 never shrinks below its content,                  so it does not scroll and the footer leaves the window: {main}");
        for sel in ["footer", ".idbar"] {
            assert!(rule(sel).contains("flex:none"),
                    "{sel} may be squeezed out when the conversation is long");
        }
    }

    /// W14: a window whose palette follows the system theme has to tell the
    /// engine so, or the parts the engine draws will not follow it.
    ///
    /// The scrollbar, the caret, select popups and the overscroll edge are the
    /// engine's, and it assumes light unless `color-scheme` says otherwise. So
    /// the dark theme shipped with a white scrollbar down its right-hand side —
    /// invisible to every check that reads the file, and the first thing a
    /// person looking at the window says about it.
    #[test]
    fn the_native_parts_of_the_window_follow_its_theme() {
        let ui = crate::ui_contract::ui_source();
        assert!(ui.contains("@media (prefers-color-scheme: dark)"),
                "the window no longer has a dark palette, so this case is about nothing");
        let root = ui.split(":root{").nth(1).expect("no :root block");
        let root = &root[..root.find('}').expect("unterminated :root block")];
        assert!(root.contains("color-scheme:"),
                "the palette follows the system theme and the engine is not told, so the \
                 scrollbar and everything else it draws will not follow it: {root}");
    }

    /// The refusing button holds focus, so a stray Return can never grant. This
    /// is in the CSS and the markup rather than in any command, which is exactly
    /// why it needs its own check.
    #[test]
    fn the_refusing_button_is_the_one_that_takes_focus() {
        let ui = ui_source();
        assert!(
            ui.contains(".deny\")?.focus()") || ui.contains("deny\").focus()"),
            "the refusing button does not take focus — a stray Return could grant consent"
        );
    }

    /// The language files the window loads are the ones that exist. Checked here
    /// too because a startup failure is not something the window can report.
    #[test]
    fn the_window_asks_for_language_files_that_exist() {
        let ui = ui_source();
        assert!(ui.contains("i18n/${lang}.json"), "the window no longer fetches language files");
        assert!(
            !ui.contains("i18n/de.js"),
            "a script tag for a language file survived the move to JSON"
        );
    }

    /// The version is written in two files and shipped as one number. A binary
    /// whose window reports a different version from its own manifest is a
    /// support problem in a support product: the first thing anyone asks is
    /// which version you are running, and two answers is worse than none.
    #[test]
    fn the_version_is_the_same_everywhere() {
        let cargo = std::fs::read_to_string(crate_dir().join("Cargo.toml")).unwrap();
        let manifest = std::fs::read_to_string(crate_dir().join("tauri.conf.json")).unwrap();

        let crate_version = cargo
            .lines()
            .find(|l| l.starts_with("version = "))
            .and_then(|l| l.split('"').nth(1))
            .expect("Cargo.toml declares no version");
        let bundle: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        let bundle_version = bundle["version"].as_str().expect("tauri.conf.json declares no version");

        assert_eq!(
            crate_version, bundle_version,
            "Cargo.toml says {crate_version} and tauri.conf.json says {bundle_version} — \
the binary and its manifest would ship disagreeing about what they are"
        );
        // Releases are tagged `v<version>`, so the version has to be a version.
        let parts: Vec<&str> = crate_version.split('.').collect();
        assert_eq!(parts.len(), 3, "not a three-part version: {crate_version}");
        for part in parts {
            assert!(part.parse::<u32>().is_ok(), "not numeric: {crate_version}");
        }
    }

    /// Where this client talks to is configuration, and a released binary that
    /// can only ever address the machine it runs on is not shippable.
    ///
    /// Both endpoints were `const` literals pinned to loopback, which is right
    /// for a development checkout and wrong for everything after it. They are
    /// now filled from `endpoints`, which reads the environment in Rust — and
    /// the ordering matters as much as the source: the fetch has to happen
    /// before anything in the flow uses either value, or a release quietly
    /// talks to loopback for the first few seconds of its life.
    #[test]
    fn the_endpoints_are_configuration_and_are_read_before_they_are_used() {
        let ui = ui_source();
        for dead in ["const SERVER_URL=", "const INDEX_URL="] {
            assert!(
                !ui.contains(dead),
                "{dead} is a constant again — a packaged build could only ever talk to the machine it is running on"
            );
        }
        // Position in the file is not execution order — every handler that uses
        // these is *defined* above the boot block and *called* after it. What is
        // checkable, and what actually matters, is that the read is the first
        // thing the boot block does: nothing else in it may run first, because
        // the very next statements load languages and start the index clock.
        let boot = ui
            .find(r#"window.addEventListener("load""#)
            .map(|i| &ui[i..])
            .expect("the window has no boot block");
        let fetched = boot
            .find(r#"invoke("endpoints")"#)
            .expect("the window never asks where its server is");
        for (later, what) in [
            ("loadLanguages()", "the language files"),
            ("syncIndex", "the catalogue clock"),
        ] {
            let first = boot
                .find(later)
                .unwrap_or_else(|| panic!("{later} is gone from boot — check this test"));
            assert!(
                fetched < first,
                "{what} is reached before the endpoints are read — the first call of the session would go to the loopback fallback"
            );
        }
        // The names are the server's own convention, not the client's older
        // `VS_*` one, because they name the server being addressed.
        let main = main_source();
        assert!(
            main.contains("PODSHL_SERVER_URL") && main.contains("PODSHL_INDEX_URL"),
            "the endpoints command no longer reads the environment"
        );
    }

    /// The body of one `async function` in the window, up to the next one.
    fn function_body(ui: &str, name: &str) -> String {
        let start = ui
            .find(&format!("async function {name}("))
            .unwrap_or_else(|| panic!("{name} is gone from the window"));
        let rest = &ui[start + 10..];
        let end = rest.find("\nasync function ").map(|i| i + 10).unwrap_or(rest.len());
        ui[start..start + end].to_string()
    }

    /// EN4: a new question is a new incident, and the grants of the last one go
    /// before anything else happens — the project directory and any program the
    /// user pointed at. Nothing ever cleared the project root.
    #[test]
    fn a_new_question_ends_the_last_incident_first() {
        let body = function_body(&ui_source(), "runInner");
        let ended = body.find("invoke(\"end_incident\")").expect("a new question does not end the last incident");
        for later in ["\"search_vendors\"", "chooseVendor(", "\"discover\""] {
            if let Some(at) = body.find(later) {
                assert!(ended < at, "{later} runs before the last incident's grants are withdrawn");
            }
        }
        assert!(body.contains("STATED.clear()"), "what a person said last time survives into this one");
    }

    /// PB3: **every way out of the published path says which way it was**, to
    /// the log and to the caller.
    ///
    /// It used to return a bare `false` for six different things — the person
    /// declined, the card could not be fetched, the operator did not answer,
    /// the readings were refused, nothing matched — and `noVendorPath` read all
    /// of them as permission to start a model. On 2026-09-15 a stale DNS record
    /// made `/mirror` unreachable and the window told the person the project
    /// published nothing, then chose a model for them. So the outcome is named
    /// now, and "unreachable" is handled on its own.
    ///
    /// Reading the source is weaker than exercising it. What it can still do is
    /// refuse a silent or unnamed exit.
    #[test]
    fn every_exit_from_the_published_path_says_which_exit_it_was() {
        let ui = ui_source();
        let body = function_body(&ui, "publishedPath");
        let known = ["answered", "declined", "unreachable", "reads", "nofinding", "none"];
        let lines: Vec<&str> = body.lines().collect();
        let mut exits = 0;
        for (i, line) in lines.iter().enumerate() {
            let Some(at) = line.find("return \"") else { continue };
            exits += 1;
            let outcome = line[at + 8..].split('"').next().unwrap_or("");
            assert!(known.contains(&outcome),
                    "publishedPath returns an outcome nothing handles: {outcome:?}");
            let named = line.contains("logUi(")
                || lines.get(i.wrapping_sub(1)).is_some_and(|p| p.contains("logUi("));
            assert!(named, "publishedPath leaves silently at: {}", line.trim());
        }
        assert!(exits >= 6, "publishedPath has only {exits} named exits; it had six");
        assert!(!body.contains("return false") && !body.contains("return true"),
                "publishedPath is back to a boolean, which cannot tell a refusal from a failure");

        // One event, one panel. The failure used to be announced where it
        // happened and then again, differently, by the panel offering the
        // retry — the person read "the catalogue is not answering" and then a
        // second panel with a gentler tone about the same thing.
        assert!(!body.contains("pub_off_h"),
                "publishedPath announces an unreachable operator itself, so it is said twice");
        assert!(body.contains("UNREACHABLE_WHY ="),
                "the reason is not kept, so the one panel that remains cannot say what happened");

        // The caller must keep the three apart: finished, unreachable, and the
        // rest. An operator that did not answer is not a project that publishes
        // nothing, and a timeout is not a person asking for a model.
        let no_vendor = function_body(&ui, "noVendorPath");
        let unreachable = no_vendor.find("\"unreachable\"")
            .expect("noVendorPath no longer treats an unreachable catalogue as its own case");
        let no_agent = no_vendor.rfind("if(published) saidNoAgent();")
            .expect("noVendorPath no longer says when a project publishes no agent");
        assert!(unreachable < no_agent,
                "the unreachable case is decided after the no-agent message, so it still fires on a network failure");
        assert!(no_vendor.contains("DESPITE published answers"),
                "a project with published answers can reach the model unrecorded");

        // The window can only say any of this if the binary still offers it.
        assert!(registered_commands(&main_source()).iter().any(|c| c == "log_line"),
                "log_line is no longer registered, so none of the lines above are written");
    }

    /// PB4: **the question is asked in the person's words.**
    ///
    /// A problem class is an identifier because a rule matches on it and a
    /// solution answers it. For a long time three identifiers in a dropdown
    /// were the whole of what the published path asked — a question about your
    /// own computer that nobody outside the project could answer. The
    /// maintainer's sentence leads now; the identifier stays under it, because
    /// it is what travels and what a support conversation quotes.
    #[test]
    fn the_class_picker_asks_in_the_persons_words() {
        let ui = ui_source();
        let body = function_body(&ui, "publishedPath");
        assert!(!body.contains("<select id=\"pcls\""),
                "the classes are a dropdown of identifiers again");
        assert!(body.contains("pick.answer_labels"),
                "the window no longer reads the maintainer's sentence for a class");
        assert!(body.contains("labels[c] ? bi(labels[c], labels_t[c]) : esc(c)"),
                "a class with no sentence no longer falls back to its identifier, so                  every manifest published before this would show nothing");
        // What is recorded and sent stays the identifier, never the sentence.
        assert!(body.contains("input.pc:checked"),
                "the chosen class is not read from what the person actually chose");

        // LG7 reaches the first publisher sentence a person meets, too. It is
        // translated by the reader's own model with the original beside it, and
        // the question is asked before the card is fetched, so the translation
        // happens before the picker is drawn rather than after.
        let translated = body.find("translateKeeping(src, [])")
            .expect("the class sentences are never translated, so a German reader meets English");
        let drawn = body.find("role=\"radiogroup\"").expect("the class picker is gone");
        assert!(translated < drawn,
                "the sentences are translated after the picker is drawn, so it is drawn in English");
        assert!(body.contains("bi(labels[c], labels_t[c])"),
                "a translated sentence no longer keeps the publisher's own words beside it");
        // And the binary still puts them on the hit the window reads.
        let vendors = std::fs::read_to_string(crate_dir().join("src/vendors.rs"))
            .expect("cannot read src/vendors.rs");
        assert!(vendors.contains("\"answer_labels\""),
                "the search hit no longer carries the sentences, so the window has only ids");
    }

    /// **A build with no operator compiled in says so, and cannot stop the
    /// window opening by saying it.**
    ///
    /// `cargo test` and a bare `cargo build` rebuild this binary without
    /// `PODSHL_BUILD_*`, and what comes out is not broken — it is plausible:
    /// it starts, it draws this window, it points at loopback with no pinned
    /// key, and every published project falls through to the model. That cost
    /// an afternoon on 2026-09-14 and caught two more builds on 2026-09-15.
    ///
    /// The first version of the marker called `t()` before `loadLanguages()`
    /// had filled `I18N`, which throws — and a throw in `boot` is the "PODSHL
    /// could not start" screen. A notice that a build is untrustworthy must
    /// never be the reason the window will not open.
    #[test]
    fn a_development_build_says_so_and_cannot_stop_the_window_opening() {
        let ui = ui_source();
        assert!(main_source().contains("\"built\": option_env!(\"PODSHL_BUILD_SERVER_URL\").is_some()"),
                "the binary no longer reports whether it was built for an operator");
        let lang = ui.find("applyLang();").expect("the window never applies a language");
        let marker = ui.find("ep.built === false").expect("a development build is no longer marked");
        assert!(lang < marker,
                "the marker reads a sentence before the language files are loaded, which                  throws in boot and shows \"PODSHL could not start\" instead of the window");
        // Wrapped: the few lines before it open a `try`.
        let before = &ui[marker.saturating_sub(120)..marker];
        assert!(before.contains("try{"),
                "the marker is not wrapped, so a missing sentence would stop the window");
    }

    /// Every sentence the window asks for exists in every language it offers.
    /// `t()` falls back to English and then to the key itself, so a missing one
    /// is not a crash — it is a raw `pub_unreach_h` on somebody's screen.
    #[test]
    fn every_language_has_every_sentence() {
        let dir = crate_dir().join("ui/i18n");
        let read = |l: &str| -> serde_json::Map<String, serde_json::Value> {
            let p = dir.join(format!("{l}.json"));
            serde_json::from_str(&std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display())))
                .unwrap_or_else(|e| panic!("{} is not JSON: {e}", p.display()))
        };
        let en = read("en");
        for lang in ["de", "fr", "es"] {
            let other = read(lang);
            let missing: Vec<&String> = en.keys().filter(|k| !other.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{lang}.json is missing {missing:?}");
        }
    }

    /// PB1: the published path reads the project's own probes, under the same
    /// consent as every other reading, before it asks the operator anything —
    /// and it asks a person only for what was not read.
    #[test]
    fn the_published_path_reads_before_it_asks() {
        let body = function_body(&ui_source(), "publishedPath");
        let card = body.find("\"published_card\"").expect("the project's own probes are never fetched");
        let collect = body.find("collectFacts(").expect("the published path never reads anything");
        let asked = body.find("\"ask_published\"").expect("the published path never asks the operator");
        assert!(card < collect && collect < asked,
                "the operator is asked before the project's probes were read");
        assert!(!body.contains("\"baseline_facts\""),
                "the published path went back to asking with nothing but the platform");
    }

    /// PB2: a `need` that carries a reading is read, not typed. It went
    /// straight to a question, so a person typed their operating system into a
    /// box that `os_fact` answers without asking anybody.
    #[test]
    fn a_needed_fact_that_can_be_read_is_read() {
        let body = function_body(&ui_source(), "askOneProbe");
        let read = body.find("own.read").expect("askOneProbe never looks at the probe's reading");
        let reads = body.find("collectFacts(").expect("a readable need is never read");
        let asks = body.find("add(`").expect("askOneProbe no longer asks at all");
        assert!(read < asks && reads < asks, "a readable need is asked before it is read");
        assert!(body.contains(".declined"), "a declined read is not recorded, so the operator asks again");
    }

    /// PB4 and PB5: whether the answer worked is asked, not assumed — and the
    /// report goes to the operator only after the person agreed to send it.
    /// It was sent as `resolved` the moment the text appeared, through a path
    /// that spoke to a vendor agent the project never ran.
    #[test]
    fn a_published_report_asks_first_and_goes_to_the_operator() {
        let ui = ui_source();
        let path = function_body(&ui, "publishedPath");
        assert!(!path.contains("offerReport("), "the published path reports through the vendor path again");
        assert!(!path.contains("\"resolved\")"), "the outcome is assumed rather than asked");
        assert!(path.contains("offerPublishedReport(pick, outcome)"), "the asked outcome is not what is reported");

        let offer = function_body(&ui, "offerPublishedReport");
        let refusal = offer.find("if(!await ask(el))").expect("the report is sent without asking");
        let send = offer.find("\"send_published_report\"").expect("nothing sends the published report");
        assert!(refusal < send, "the report is sent before the person is asked");
        assert!(offer[refusal..send].contains("return"), "declining does not return");
        assert!(!offer.contains("\"send_report\""), "a published report goes to a vendor agent again");
    }

    /// PB6: the questions a project declared for a person are asked, with the
    /// publisher's own choices. The panel was built from the read plan, which
    /// carries machine probes only — so a human probe was never asked and a
    /// `choices` list was never offered.
    #[test]
    fn declared_questions_are_asked_with_their_choices() {
        let body = function_body(&ui_source(), "collectFacts");
        assert!(body.contains("p.kind!==\"machine\""), "questions declared for a person are not asked");
        assert!(body.contains("byId[p.id]"), "an empty reading is asked without the publisher's own question");
        let ask = function_body(&ui_source(), "askQuestions");
        assert!(ask.contains("p.choices.map"), "a publisher's choices are not offered");
        // And a reading that simply found nothing is not handed to a person as
        // a bare text box — only one the publisher wrote a question for, or one
        // the person withheld themselves.
        assert!(body.contains("p.prompt || (withheld(p.id) && !p.refused)"),
                "every empty reading is put to the person as free text again");
    }

    /// P3: an answer that does not fit the publisher's `pattern` is caught in
    /// the window and asked again, before a report is built around it. The
    /// hand-off checked this; the question panel never did.
    #[test]
    fn an_answer_that_misses_its_pattern_is_asked_again() {
        let ask = function_body(&ui_source(), "askQuestions");
        let check = ask.find("matchesPattern(p.pattern, v)").expect("the question panel does not check patterns");
        let stop = ask.find("if(bad){").expect("a failed pattern does not stop the panel");
        // The answer is kept in a local first now, because the same pass also
        // records a skip as declined — so this looks for the store rather than
        // for one spelling of it.
        let store = ask.rfind("FACTS[i.dataset.id]=").expect("answers are never stored");
        assert!(check < stop && stop < store, "an answer is stored before its pattern is checked");
        assert!(ask[stop..store].contains("return;"), "a failed pattern does not keep the panel open");
    }

    /// LX6: free text is anonymised before it is shown for consent — not after
    /// it is agreed to — and attached only after the person said yes.
    #[test]
    fn free_text_is_anonymised_before_it_is_offered() {
        let body = function_body(&ui_source(), "offerFreeText");
        let anon = body.find("\"anonymise_text\"").expect("free text is offered as typed");
        let asked = body.find("await ask(el)").expect("free text is sent without asking");
        let attach = body.find("\"attach_consented_text\"").expect("nothing attaches the text");
        assert!(anon < asked, "the text is anonymised after the person agreed to it");
        assert!(asked < attach, "the text is attached before the person agreed");
    }

    /// PV2, the window's half: a program not on the search path is asked
    /// about, and the refusing button holds focus there too.
    #[test]
    fn a_program_not_on_the_path_is_asked_about_not_searched_for() {
        let body = function_body(&ui_source(), "locatePrograms");
        assert!(body.contains("\"grant_program_path\""), "the location the person gives is not validated by the binary");
        assert!(body.contains(".deny\").focus()"), "a stray Return could run a program");
    }

    /// W11: no inline style attributes. Tauri puts a nonce into `style-src`,
    /// and a browser that sees a nonce ignores 'unsafe-inline' — so every
    /// `style="…"` in this window was blocked, and every answer box, list and
    /// question past the first screen rendered at the browser's default width.
    /// Found by looking at a screenshot of the running window; no check that
    /// reads this file as text could have seen it, this one included — it can
    /// only stop the pattern coming back.
    #[test]
    fn the_window_carries_no_inline_style_attributes() {
        let ui = ui_source();
        // `style="…"` — with the ellipsis — is how a comment names the
        // attribute; anything else is the attribute.
        let n = ui.matches("style=\"").count() - ui.matches("style=\"…\"").count()
            + ui.matches("style='").count();
        assert_eq!(n, 0, "an inline style attribute is back — the window's CSP blocks it");
        assert!(ui.contains(".field{"), "the class that replaced the inline field styles is gone");
    }

    /// LG7: a published project owes English and nothing more, so a person
    /// reading the window in their own language met every question and every
    /// answer in English. Where their own model is set up, it translates —
    /// before the questions are asked, marked, with the original one click
    /// away — and never the `choices`, because the answer that is recorded has
    /// to be the publisher's own word.
    #[test]
    fn a_published_projects_words_are_translated_by_the_readers_own_model() {
        let ui = ui_source();
        let path = ui.split("async function publishedPath").nth(1).expect("the published path is gone");
        let translate = path.find("translatePublisher(SKILL.probes").expect("the project's questions are never translated");
        let collect = path.find("collectFacts(SKILL.probes").expect("the project's readings are never taken");
        assert!(translate < collect, "the questions are asked before they are translated");
        assert!(path.contains("translatePublisherText(text"), "the answer is never translated");
        assert!(path.contains("pub_translated"), "a translated answer is not marked as one");
        let f = ui.split("async function translatePublisher(probes").nth(1).unwrap();
        let f = &f[..f.find("\n}\n").unwrap()];
        assert!(!f.contains("choices"), "a publisher's choices are translated, so the recorded answer would not be theirs");
        assert!(ui.contains("return cfg.configured ? cfg : null;"), "translation is attempted without a model");
    }

    /// LG8, the window's half: the project's glossary reaches both
    /// translations — its questions and its answer — and a term the model
    /// changed anyway is said beside the translation in every language, not
    /// left for the reader to take *cerveau* for what the project meant.
    #[test]
    fn a_projects_own_terms_are_kept_and_a_lost_one_is_said() {
        let ui = ui_source();
        let path = function_body(&ui, "publishedPath");
        assert!(path.contains("card.glossary"), "the project's glossary is never read");
        assert!(path.contains("translatePublisher(SKILL.probes, KEEP)"), "the questions are translated without the glossary");
        assert!(path.contains("translatePublisherText(text, KEEP)"), "the answer is translated without the glossary");
        assert!(path.contains("termsLostLine(textT.lost)"), "a term lost from the answer is not said");
        let questions = function_body(&ui, "translatePublisher");
        assert!(questions.contains("translateKeeping(src, keep)") && questions.contains("termsLostLine(lost)"),
                "a term lost from the questions is not said");
        assert!(ui.contains("invoke(\"llm_translate\",{texts, to:LANG, keep:terms})"),
                "the glossary never reaches the binary");
        for lang in ["en", "de", "fr", "es"] {
            let table: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(crate_dir().join(format!("ui/i18n/{lang}.json"))).unwrap()).unwrap();
            let s = table["pub_terms_lost"].as_str().unwrap_or_else(|| panic!("{lang} cannot say a term was lost"));
            assert!(s.contains("{list}"), "{lang} does not name the term it lost");
        }
        let main = std::fs::read_to_string(crate_dir().join("src/main.rs")).unwrap();
        let card = main.split("async fn published_card(").nth(1).unwrap();
        assert!(card.contains("\"glossary\""), "the client drops the glossary before the window sees it");
    }

    /// I5: what the binary says reaches the person in the window's language.
    /// Its errors arrive as strings, and they were shown as they came — in
    /// German, on an English screen. Every place that shows one now says it
    /// through `tr`, which recognises the binary's sentence and says it again.
    #[test]
    fn the_binarys_messages_are_shown_through_tr() {
        let ui = ui_source();
        assert!(ui.contains("function tr(s)"), "the window no longer translates the binary's messages");
        // A local of the same name hid the function for a whole flow, and the
        // first message on the vendor path threw instead of being said.
        for local in ["const tr=", "let tr=", "const tr =", "let tr ="] {
            assert!(!ui.contains(local), "a local `tr` hides the function that says the binary's messages");
        }
        assert!(ui.contains("I18N.en).filter(([k]) => k.startsWith(\"m_\"))"),
                "the window no longer recognises messages by the templates the binary uses");
        for raw in ["${esc(e)}", "String(e)", ",{e})", "esc(e && e.message || e)", "esc(st.note)",
                    "esc(pl.reason)", "esc(dry)", "esc(h.how)", "esc(p.name)"] {
            assert!(!ui.contains(raw), "{raw} shows the binary's words untranslated");
        }
    }

    /// I4: every reading's consent sentence exists in every language, as do the
    /// risk levels and the kinds of refusal — and the window uses them. The
    /// sentence came from the binary in German, on an English screen; it is the
    /// one text the user actually decides on.
    #[test]
    fn the_consent_text_exists_in_every_language() {
        let ui = ui_source();
        assert!(ui.contains("${esc(whatText(p))}"), "the consent list shows the binary's own sentence again");
        assert!(ui.contains("t(\"refused_\"+p.refused_kind)"), "refusals are shown in the binary's language again");
        let spec: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(crate_dir().join("../spec/vocabulary/reads.json")).unwrap()).unwrap();
        let mut keys: Vec<String> = spec["ops"].as_array().unwrap().iter()
            .map(|o| format!("m_what_{}", o["op"].as_str().unwrap())).collect();
        keys.push("m_what_program_version_absent".into());
        for k in ["risk_low", "risk_medium", "refused_absent", "refused_denied", "refused_system",
                  "refused_outside", "refused_invalid", "pseudo_explain", "q_pattern", "env_why"] {
            keys.push(k.into());
        }
        // Every kind of refusal the binary can name for a location or a log,
        // and every answer to the interpreter question — named by the binary,
        // said by the window.
        for k in ["wrong_name", "system", "denied", "empty", "not_in_folder", "not_executable", "invalid"] {
            keys.push(format!("loc_err_{k}"));
        }
        for k in ["no_docker", "no_container", "denied", "not_file", "invalid", "unreadable"] {
            keys.push(format!("log_err_{k}"));
        }
        let conflict = crate::reads::interpreter_conflict(&serde_json::json!(
            {"python.version": "3.12.1", "python.venv.version": "3.11.9"})).unwrap();
        for c in conflict["choices"].as_array().unwrap() {
            keys.push(format!("env_c_{}", c.as_str().unwrap()));
        }
        assert!(ui.contains("t(\"env_c_\"+c)"), "the interpreter question shows the binary's ids again");
        assert!(ui.contains("t(\"loc_err_\"+g.code"), "a refused location is shown in the binary's language");
        assert!(ui.contains("t(\"log_err_\"+r.code"), "a refused log is shown in the binary's language");
        for lang in ["en", "de", "fr", "es"] {
            let table: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(crate_dir().join(format!("ui/i18n/{lang}.json"))).unwrap()).unwrap();
            for k in &keys {
                assert!(table.get(k).and_then(|v| v.as_str()).map_or(false, |s| !s.is_empty()),
                        "{lang}.json has no {k}");
            }
        }
    }


    /// Every text the window asks for by name exists in every language it
    /// offers.
    ///
    /// `i18n.rs` says it plainly: **a missing key falls back to English
    /// silently**, and in a consent dialogue that is the wrong failure — the
    /// person is told in a language they may not read what is about to be read
    /// from their machine. Two cases already checked a handful of keys they
    /// happened to care about, by hand. Everything added since was covered by
    /// nobody, which is how a key typed once and spelled differently in four
    /// files would ship.
    ///
    /// Literal keys only. `t("env_c_"+c)` builds its name at runtime and is
    /// checked where it is built; what this catches is the ordinary case, which
    /// is also the common one.
    #[test]
    fn every_text_the_window_names_exists_in_every_language() {
        let ui = ui_source();
        let mut keys: Vec<String> = Vec::new();
        let bytes = ui.as_bytes();
        let mut at = 0usize;
        while let Some(i) = ui[at..].find("t(\"") {
            let start = at + i;
            at = start + 3;
            // `t(` is the tail of `split(`, `format(`, `insert(` and a dozen
            // others. The first version of this case matched all of them and
            // reported that `en.json` was missing the text `div`, which is how
            // it became clear it was reading identifiers rather than keys.
            let before = if start == 0 { b' ' } else { bytes[start - 1] };
            if before.is_ascii_alphanumeric() || before == b'_' || before == b'$' {
                continue;
            }
            let Some(end) = ui[at..].find('"') else { break };
            let key = &ui[at..at + end];
            let after = ui[at + end + 1..].chars().next().unwrap_or(' ');
            // `t("x"+y)` names nothing on its own.
            if after != '+' && !key.is_empty() && !key.contains(' ') {
                keys.push(key.to_string());
            }
            at += end + 1;
        }
        keys.sort();
        keys.dedup();
        assert!(keys.len() > 50, "only {} keys found — this case is reading the wrong file", keys.len());

        for lang in ["en", "de", "fr", "es"] {
            let table: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(crate_dir().join(format!("ui/i18n/{lang}.json"))).unwrap()).unwrap();
            let missing: Vec<&String> = keys.iter()
                .filter(|k| table.get(k.as_str()).and_then(|v| v.as_str())
                                 .map_or(true, |s| s.trim().is_empty()))
                .collect();
            assert!(missing.is_empty(),
                    "{lang}.json is missing {} of the window's texts: {:?}",
                    missing.len(), &missing[..missing.len().min(8)]);
        }
    }


    /// IS1: the diagnosis has an exit that is not a report to the operator.
    ///
    /// It ended with one offer — a pseudonymous report — and that is the wrong
    /// shape for the case the open-source branch rests on: the published answers
    /// did not cover somebody's problem, they now hold more about it than they
    /// could have assembled in an hour, and there was nowhere to put it. "A good
    /// bug report in two minutes" was the promise and it did not exist.
    ///
    /// Three properties, and the second is the one that could go quietly wrong.
    #[test]
    fn the_published_path_offers_a_report_the_person_can_take_away() {
        let ui = ui_source();
        let path = function_body(&ui, "publishedPath");
        assert!(path.contains("offerIssueReport("),
                "the published path still ends at the operator or nowhere");

        let panel = function_body(&ui, "offerIssueReport");

        // Shown before it is copied, and what was taken out is said — the same
        // sentence the consent panel uses, about a larger audience, because an
        // issue tracker is more public than a report and keeps it for ever.
        assert!(panel.contains("issue_took") && panel.contains("r.replaced"),
                "the panel does not say what the anonymiser removed");
        assert!(panel.contains("<textarea") && panel.contains("box.value = withFooter"),
                "the text is not shown before it is copied");

        // **What is copied is what is in the box.** Copying `r.markdown` would
        // hand over the generated text after the person edited it — something
        // they did not read, from a panel whose whole argument is that they did.
        let copy = panel.split("issuecopy\").onclick").nth(1).unwrap_or("");
        assert!(copy.contains("box.value"),
                "the copy button copies the generated text rather than what is shown");
        assert!(!copy.contains("r.markdown"),
                "the copy button reaches past the box to the original");

        // The footer is the publisher's line about us, and it is theirs to drop.
        assert!(panel.contains("issuefoot") && panel.contains("issue_footer"),
                "the footer cannot be switched off");
    }

    /// AT1: a published answer says what the operator attests about whoever
    /// published it — control of the location, when it was last confirmed, the
    /// log entry, and the project's own word about itself. The window knew only
    /// a vendor's key, verified or untrusted, and could not say "stale".
    #[test]
    fn a_published_answer_says_what_is_attested_about_its_publisher() {
        let ui = ui_source();
        let path = function_body(&ui, "publishedPath");
        assert!(path.contains("attestationLine(card, pick.domain)"), "the answer no longer says who stands behind it");
        let line = ui.split("function attestationLine(").nth(1).expect("attestationLine is gone");
        for state in ["\"live\"", "\"stale\"", "att_unknown", "deprecated", "forge_archived", "log_seq"] {
            assert!(line.contains(state), "the attestation line does not handle {state}");
        }
        let main = main_source();
        let card = main.split("async fn published_card(").nth(1).unwrap();
        assert!(card.contains("\"anchor\"") && card.contains("\"project\""),
                "the client drops what the mirror attests before the window sees it");
        // AT2's window half: the check the sentence promises is made, and its
        // result — either way — is shown.
        let path = function_body(&ui, "publishedPath");
        assert!(path.contains("checkLogEntry(answered, card)"), "the log entry is announced and never checked");
        let check = function_body(&ui, "checkLogEntry");
        assert!(check.contains("\"verify_log_entry\"") && check.contains("content_sha256"),
                "the check does not bind the entry to the files being served");
        assert!(check.contains("att_unproved"), "a failed proof is not shown");
    }

    /// V4's window half: switching to the vendor the readings name ends this
    /// run. It used to set the input and carry on — sending the readings to the
    /// vendor the person had just said was the wrong one.
    #[test]
    fn switching_vendor_ends_the_run() {
        let ui = ui_source();
        let calls = ui.matches("await checkMismatch(").count();
        let returning = ui.matches("if(await checkMismatch(").count();
        assert!(calls > 0, "nothing checks the named vendor against the readings");
        assert_eq!(calls, returning, "a switch of vendor does not end the run everywhere it is offered");
    }

    /// W12: the window's one escaper covers attribute values. It is used inside
    /// `data-id="…"` with ids a vendor's skill chooses, and it escaped `&<>`
    /// only — so a `"` ended the attribute. The CSP blocks inline handlers, and
    /// that is meant to be the second line, not the only one.
    #[test]
    fn the_escaper_covers_attribute_values() {
        let ui = ui_source();
        let line = ui.lines().find(|l| l.starts_with("const esc = ")).expect("the escaper is gone");
        for c in ["&amp;", "&lt;", "&gt;", "&quot;", "&#39;"] {
            assert!(line.contains(c), "the escaper does not produce {c}: {line}");
        }
    }

    /// Nothing in the window may reach a vendor endpoint directly. Every call
    /// goes through a command, because that is where the bounds are.
    #[test]
    fn the_window_never_talks_to_a_vendor_itself() {
        let ui = ui_source();
        for (offender, why) in [
            ("fetch(BASE", "a direct fetch to the vendor base"),
            ("fetch(`${BASE}", "a direct fetch to the vendor base"),
            ("XMLHttpRequest", "a raw request object"),
        ] {
            assert!(!ui.contains(offender), "the window bypasses the command layer: {why}");
        }
    }

    /// L9: the Windows installer asks for nothing above the person's own
    /// account, and its "delete application data" box removes the directory
    /// the client actually keeps its data in — which is not the one named
    /// after the bundle identifier that Tauri's uninstaller removes by itself.
    #[test]
    fn the_windows_installer_needs_no_administrator_and_removes_the_real_data() {
        let conf: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(crate_dir().join("tauri.conf.json")).expect("tauri.conf.json"),
        )
        .expect("tauri.conf.json is not JSON");
        let nsis = &conf["bundle"]["windows"]["nsis"];
        assert_eq!(nsis["installMode"], "currentUser",
                   "an installer for the whole machine needs an administrator — this client promises bounded effect");
        let hooks = nsis["installerHooks"].as_str().expect("no installer hooks named");
        let hooks = std::fs::read_to_string(crate_dir().join(hooks)).expect("the named hooks file is missing");
        assert!(hooks.contains("NSIS_HOOK_POSTUNINSTALL") && hooks.contains("$DeleteAppDataCheckboxState"),
                "uninstalling no longer honours the delete-data box");
        let removes = hooks.lines().filter(|l| !l.trim_start().starts_with(';'))
            .any(|l| l.contains("RMDir /r \"$APPDATA\\podshl\""));
        assert!(removes, "the uninstaller does not remove %APPDATA%\\podshl");
        // The directory the hook removes is the one the client writes.
        for (file, needle) in [("src/main.rs", ".join(\"podshl\")"), ("src/llm.rs", ".join(\"podshl\")"),
                               ("src/identity.rs", ".join(\"podshl\")")] {
            let src = std::fs::read_to_string(crate_dir().join(file)).unwrap();
            assert!(src.contains("dirs::config_dir()") && src.contains(needle),
                    "{file} no longer keeps its state under the config directory's podshl");
        }
    }

    /// L10: a release build trusts nothing found relative to the directory it
    /// was started from. `../var/log_key.json` is a development convenience; in
    /// an installed program it is a key anybody could plant beside a shortcut.
    /// Every such default outside the tests has to sit behind `debug_assertions`.
    #[test]
    fn a_release_build_reads_no_key_or_directory_beside_where_it_was_started() {
        for file in ["src/index.rs", "src/vendors.rs", "src/main.rs", "src/logproof.rs"] {
            let src = std::fs::read_to_string(crate_dir().join(file)).unwrap();
            let body = src.split("#[cfg(test)]").next().unwrap();
            let lines: Vec<&str> = body.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.trim_start().starts_with("//") || !line.contains("\"../var/") {
                    continue;
                }
                let guarded = lines[i.saturating_sub(3)..=i].iter().any(|l| l.contains("debug_assertions"));
                assert!(guarded, "{file}:{} reads the checkout's var/ in a release build: {}", i + 1, line.trim());
            }
        }
        let main = main_source();
        assert!(!main.contains("unwrap_or_else(|_| PathBuf::from(\".\"))"),
                "the client's state directory defaults to wherever it was started");
    }
}
