//! The report a person takes with them.
//!
//! A diagnosis ends and the only exit is a pseudonymous report to the operator.
//! That is the wrong shape for the case the whole open-source branch rests on:
//! somebody's machine is broken, the published answers did not cover it, and the
//! person now has more about their own problem written down than they could
//! assemble in an hour -- and no way to hand it to the project. "A good bug
//! report in two minutes" was the promise, and it did not exist.
//!
//! **This is more public than a report, not less.** A report goes to one
//! operator under a pseudonym; this goes into an issue tracker anybody can read,
//! for ever, with the person's own name on it. So every string here goes through
//! the same anonymiser the consent panel uses, including the words the person
//! typed themselves, and the panel says what it took out -- the same sentence,
//! about a larger audience.
//!
//! **Measured and supplied stay apart.** The report keeps `observed` and
//! `stated` in separate maps because a solution that failed on a measurement is
//! a defect in a rule and worth a maintainer's afternoon, while one that failed
//! on a typed value may be nothing of the sort. A maintainer reading an issue
//! deserves the same distinction, and it is free here.
//!
//! **English.** `check_english` makes it the one obligation on a publisher, so
//! it is the language every project can certainly read. The headings are
//! English and the person's own words stay exactly as they wrote them, which is
//! what a bug report looks like anyway. The panel says so rather than leaving
//! somebody to paste a document they cannot read.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

fn cell(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    // A newline or a pipe would end the row it is in and silently take the rest
    // of the value out of the table.
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn table(rows: &[(String, String)], left: &str, right: &str, out: &mut String) {
    out.push_str(&format!("| {left} | {right} |\n|---|---|\n"));
    for (k, v) in rows {
        out.push_str(&format!("| `{k}` | {v} |\n"));
    }
    out.push('\n');
}

/// Fact name and its cell, in the order the table shows them.
type Rows = Vec<(String, String)>;

/// Split the facts into what the machine read and what the person answered.
fn split(facts: &Map<String, Value>, stated: &[String]) -> (Rows, Rows) {
    let (mut read, mut said) = (Vec::new(), Vec::new());
    for (k, v) in facts {
        let row = (k.clone(), cell(v));
        if stated.iter().any(|s| s == k) {
            said.push(row)
        } else {
            read.push(row)
        }
    }
    (read, said)
}

/// Version numbers an answer states that this machine did not report.
///
/// The failure this exists for was measured, on a released client against a
/// local model: the answer said *"revert to driver 610.86"* while the machine
/// had reported `610.57`. Nothing had read or been told `610.86`; the model
/// produced it, and it read exactly like the rest of the answer. The existing
/// `rests_on` grading could not catch it — that says which facts a *match*
/// turned on, and an invented number is in the prose rather than in the match.
///
/// **Only where there is something to compare against.** If nothing
/// version-shaped was read or supplied, this says nothing: a published answer
/// legitimately names versions nobody measured — *"the actual fix is driver 555
/// or newer"* is correct and 555 is not a reading. Firing there would be noise
/// on top of a correct answer, and noise is how a warning stops being read.
///
/// **And it is not a verdict.** What it can say is that a number appears in the
/// answer and was not among the readings, which is true; whether that makes the
/// answer wrong is not ours to decide. The wording says so.
///
/// What it does not catch is the other half of the same walk: *"other
/// applications work correctly"*, a claim about facts nobody stated. Detecting
/// that is reading prose for assertions, which is a different problem and is
/// not attempted here rather than attempted badly.
pub fn unstated_versions(answer: &str, facts: &Value, stated: &[String]) -> Vec<String> {
    let empty = Map::new();
    let map = facts.as_object().unwrap_or(&empty);

    // The project's own naming convention decides what a version is: the
    // generalisation policy coarsens on exactly this suffix, so a publisher who
    // names a reading `..._version` has already said it is one.
    let known: Vec<String> = map
        .iter()
        .filter(|(k, _)| k.ends_with("_version") || k.ends_with(".version"))
        .filter_map(|(_, v)| v.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect();
    if known.is_empty() {
        return Vec::new();
    }
    let _ = stated; // Read or typed, both count as reported by this machine.

    let mut out: Vec<String> = Vec::new();
    for tok in version_tokens(answer) {
        // A number the machine did report, or a prefix of one — `610` against
        // `610.57` is the same version spoken coarsely, not a different claim.
        if known.iter().any(|k| {
            k == &tok || k.starts_with(&format!("{tok}.")) || tok.starts_with(&format!("{k}."))
        }) {
            continue;
        }
        if !out.contains(&tok) {
            out.push(tok);
        }
    }
    out
}

/// Runs of digits and dots that read as a version, with a leading `v` dropped.
///
/// Deliberately narrow. A bare integer is not a version — `3030` is a port,
/// `47` is a count of tools — and treating one as a version would fire on
/// almost every answer. Two dotted components is the floor.
fn version_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
            i += 1;
        }
        let tok: String = bytes[start..i].iter().collect();
        let tok = tok.trim_end_matches('.').to_string();
        if tok.matches('.').count() >= 1 && tok.split('.').all(|p| !p.is_empty()) {
            out.push(tok);
        }
    }
    out
}

/// The line a copied issue carries about how it was made.
///
/// A constant because two places need the same words: the markdown this builds,
/// and the panel's checkbox that takes them out again. The panel is handed this
/// string rather than a pattern that hopes to match it.
pub const FOOTER: &str = "---\n*Assembled by PODSHL on my own machine. The readings above were \
                          taken with my permission and anonymised before I saw them.*\n";

/// The issue text, and what the anonymiser took out of it.
///
/// Everything that could carry a path, an account or an address is anonymised
/// **before** it is assembled, not after: assembling first and anonymising the
/// whole document would work too, and would mean one regex mistake silently
/// publishes everything rather than one field.
#[allow(clippy::too_many_arguments)]
pub fn build(
    subject: &str,
    problem: &str,
    facts: &Value,
    stated: &[String],
    outcome: &str,
    answer: &str,
    answer_from_model: bool,
    tried: &[String],
    footer: bool,
) -> Value {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let clean = |s: &str, counts: &mut BTreeMap<&'static str, usize>| -> String {
        let (out, c) = crate::redact::anonymise(s);
        for (k, n) in c {
            *counts.entry(k).or_default() += n;
        }
        out
    };

    let safe_facts = crate::redact::anonymise_value_counting(facts, &mut counts);
    let empty = Map::new();
    let map = safe_facts.as_object().unwrap_or(&empty);
    let (read, said) = split(map, stated);

    let mut md = String::new();
    md.push_str(&format!("## {}\n\n", clean(subject, &mut counts)));

    let p = clean(problem, &mut counts);
    if !p.trim().is_empty() {
        md.push_str(&format!("{}\n\n", p.trim()));
    }

    if !read.is_empty() {
        md.push_str("### What this machine reports\n\n");
        md.push_str("Read from the machine, with permission, and anonymised.\n\n");
        table(&read, "reading", "value", &mut md);
    }
    if !said.is_empty() {
        md.push_str("### What I answered\n\n");
        md.push_str(
            "Supplied by me rather than measured — worth weighing differently if \
             something here is wrong.\n\n",
        );
        table(&said, "question", "answer", &mut md);
    }
    if !tried.is_empty() {
        md.push_str("### What was tried\n\n");
        for t in tried {
            md.push_str(&format!("- {}\n", clean(t, &mut counts)));
        }
        md.push('\n');
    }

    let a = clean(answer, &mut counts);
    let invented = unstated_versions(&a, &safe_facts, stated);
    if !a.trim().is_empty() {
        md.push_str("### The answer I was given\n\n");
        if !invented.is_empty() {
            // Above the answer, for the same reason the model mark is: a
            // maintainer who reads the number first and the caveat afterwards
            // has already started looking.
            md.push_str(&format!(
                "> **Not from this machine:** {}. {} in the answer below, and \
                 this machine did not report {}. That does not make the answer \
                 wrong — it may be about a version to move *to* — but nothing \
                 here measured it.\n\n",
                invented
                    .iter()
                    .map(|v| format!("`{v}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                if invented.len() == 1 {
                    "It appears"
                } else {
                    "They appear"
                },
                if invented.len() == 1 { "it" } else { "them" },
            ));
        }
        if answer_from_model {
            // The one thing that must not be quiet. A model's answer reads
            // exactly like a published one, and a maintainer who acts on an
            // invented version number has been cost an afternoon by us.
            md.push_str(
                "> **Unchecked.** This came from a language model, not from anything \
                 this project published. It may be wrong in ways that read as \
                 confident, and it has not been compared against the values above.\n\n",
            );
        }
        md.push_str(a.trim());
        md.push_str("\n\n");
    } else if outcome == "no_statement" {
        md.push_str("### The answer I was given\n\nNothing published covered this.\n\n");
    }

    // Stated once, here, and handed back whole. The panel has a checkbox for
    // this, and it used to strip the footer with a regex of its own -- the same
    // sentence written twice, in two languages, one of which would have gone on
    // claiming to remove text the other had stopped producing.
    if footer {
        md.push_str(FOOTER);
    }

    json!({
        "markdown": md,
        // What the checkbox takes out and puts back, so the page never has to
        // recognise it.
        "footer": FOOTER,
        // Returned as well as written into the text, so the panel can say it
        // before anybody scrolls — "marked before anything is copied".
        "unstated_versions": invented,
        "replaced": counts.iter()
            .map(|(k, n)| ((*k).to_string(), json!(n)))
            .collect::<serde_json::Map<String, Value>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Value {
        json!({
            "gpu.driver_version": "610.57",
            "os.arch": "aarch64",
            "engram.download": "engram-linux-x86_64.zip",
        })
    }

    /// The property that matters most: this is more public than a report, so
    /// nothing may reach it that a report would have held back.
    #[test]
    fn nothing_leaves_that_the_consent_panel_would_have_taken_out() {
        let v = build(
            "github.com/example/thing",
            "it broke after I moved it to C:\\Users\\jdoe\\proj and the log says \
             192.168.0.26 refused the token ghp_abcdefghijklmnopqrstuvwxyz0123",
            &json!({"path": "/home/jdoe/.config/thing.toml"}),
            &[],
            "finding",
            "Check the address in C:\\Users\\jdoe\\thing.ini",
            false,
            &["removed /home/jdoe/.cache/thing".into()],
            true,
        );
        let md = v["markdown"].as_str().unwrap();
        for leak in ["jdoe", "192.168.0.26", "ghp_abcdefghij"] {
            assert!(
                !md.contains(leak),
                "{leak} survived into text meant for a public issue tracker:\n{md}"
            );
        }
        assert!(
            !v["replaced"].as_object().unwrap().is_empty(),
            "things were replaced and the panel is not told, so it cannot say so"
        );
    }

    /// A maintainer must be able to tell a measurement from something typed,
    /// because only one of them is evidence about their rule.
    #[test]
    fn measured_and_supplied_are_two_sections() {
        let v = build(
            "s",
            "",
            &facts(),
            &["engram.download".into()],
            "finding",
            "",
            false,
            &[],
            false,
        );
        let md = v["markdown"].as_str().unwrap();
        let read = md
            .find("What this machine reports")
            .expect("no readings section");
        let said = md.find("What I answered").expect("no answers section");
        let dl = md
            .find("engram.download")
            .expect("the answered fact is missing");
        let arch = md.find("os.arch").expect("the read fact is missing");
        assert!(
            read < arch && arch < said,
            "a measured fact is under the answers"
        );
        assert!(said < dl, "a supplied fact is under the readings");
    }

    /// A model's answer reads exactly like a published one. In an issue it must
    /// not.
    #[test]
    fn a_model_answer_is_marked_before_it_is_quoted() {
        let v = build(
            "s",
            "",
            &facts(),
            &[],
            "finding",
            "Revert to driver 610.86.",
            true,
            &[],
            false,
        );
        let md = v["markdown"].as_str().unwrap();
        let mark = md
            .find("Unchecked")
            .expect("a model's answer is not marked as one");
        let text = md.find("Revert to driver").expect("the answer is missing");
        assert!(mark < text, "the mark comes after the answer it qualifies");

        let published = build(
            "s",
            "",
            &facts(),
            &[],
            "finding",
            "Revert to driver 610.86.",
            false,
            &[],
            false,
        );
        assert!(
            !published["markdown"]
                .as_str()
                .unwrap()
                .contains("Unchecked"),
            "a project's own answer was marked as a guess"
        );
    }

    /// A pipe or a newline in a value ends the row it is in and takes the rest
    /// of the value out of the document without saying so.
    #[test]
    fn a_value_cannot_break_out_of_its_row() {
        let v = build(
            "s",
            "",
            &json!({"k": "a | b\nc"}),
            &[],
            "finding",
            "",
            false,
            &[],
            false,
        );
        let md = v["markdown"].as_str().unwrap();
        let row = md
            .lines()
            .find(|l| l.contains("`k`"))
            .expect("the row is gone");
        assert!(row.contains("\\|"), "an unescaped pipe: {row}");
        assert!(
            row.contains('c'),
            "the value was truncated at the newline: {row}"
        );
    }

    /// The measured failure, from a released client against a local model: the
    /// answer said "revert to driver 610.86" while the machine had reported
    /// 610.57. Nothing had read or been told 610.86.
    #[test]
    fn a_version_the_machine_never_reported_is_named() {
        let facts = json!({"gpu.driver_version": "610.57", "os.name": "windows"});
        let got = unstated_versions("Revert to driver 610.86 and reboot.", &facts, &[]);
        assert_eq!(
            got,
            vec!["610.86".to_string()],
            "the invented version was not caught"
        );

        // The one that was reported is not named, spoken coarsely or in full.
        assert!(unstated_versions("You are on 610.57, which is fine.", &facts, &[]).is_empty());
        assert!(
            unstated_versions("The 610 series is affected.", &facts, &[]).is_empty(),
            "a coarser form of the same version was treated as a different claim"
        );
    }

    /// Where nothing version-shaped was read, there is nothing to compare
    /// against and this must stay quiet. A published answer naming "driver 555
    /// or newer" is correct, and firing on it is noise on top of a right answer
    /// — which is how a warning stops being read.
    #[test]
    fn it_says_nothing_when_there_is_nothing_to_compare_against() {
        let facts = json!({"os.name": "linux", "session.type": "wayland"});
        assert!(
            unstated_versions("The actual fix is driver 555 or newer.", &facts, &[]).is_empty(),
            "fired with no reading to compare against"
        );
    }

    /// A bare integer is a port, a count, a year. Treating one as a version
    /// would fire on almost every answer ever written.
    #[test]
    fn a_bare_integer_is_not_a_version() {
        let facts = json!({"gpu.driver_version": "610.57"});
        for prose in [
            "Open http://localhost:3030 and log in.",
            "There are 47 tools across 8 clusters.",
            "Released in 2026.",
        ] {
            assert!(
                unstated_versions(prose, &facts, &[]).is_empty(),
                "a number in {prose:?} was read as a version"
            );
        }
    }

    /// And it has to reach the document, above the answer rather than below it.
    #[test]
    fn the_mark_is_in_the_text_before_the_answer_it_qualifies() {
        let v = build(
            "s",
            "",
            &json!({"gpu.driver_version": "610.57"}),
            &[],
            "finding",
            "Revert to driver 610.86.",
            true,
            &[],
            false,
        );
        let md = v["markdown"].as_str().unwrap();
        let mark = md
            .find("Not from this machine")
            .expect("the invented version is not marked");
        let text = md.find("Revert to driver").expect("the answer is missing");
        assert!(mark < text, "the mark comes after the number it qualifies");
        assert!(
            md.contains("610.86"),
            "the number itself is not named in the mark"
        );
        assert_eq!(
            v["unstated_versions"],
            json!(["610.86"]),
            "the panel is not told, so it cannot say it before anybody scrolls"
        );
    }

    #[test]
    fn the_footer_can_be_switched_off() {
        let on = build("s", "", &facts(), &[], "finding", "", false, &[], true);
        let off = build("s", "", &facts(), &[], "finding", "", false, &[], false);
        assert!(on["markdown"].as_str().unwrap().contains("PODSHL"));
        assert!(
            !off["markdown"].as_str().unwrap().contains("PODSHL"),
            "the footer stayed after it was switched off"
        );
    }

    /// The panel's checkbox does not call this twice; it takes the footer off
    /// the end of the text it already has and puts it back. That is only the
    /// same document if the footer is exactly a suffix -- so this is the
    /// property the page is allowed to rely on, checked here rather than
    /// assumed there.
    #[test]
    fn the_footer_is_exactly_a_suffix() {
        let on = build("s", "", &facts(), &[], "finding", "", false, &[], true);
        let off = build("s", "", &facts(), &[], "finding", "", false, &[], false);
        let with = on["markdown"].as_str().unwrap();
        let without = off["markdown"].as_str().unwrap();
        assert_eq!(
            with,
            format!("{without}{FOOTER}"),
            "switching the footer off is not the same as removing it from the end"
        );
        assert_eq!(
            on["footer"].as_str().unwrap(),
            FOOTER,
            "the panel is handed the words, so it never has to recognise them"
        );
    }
}
