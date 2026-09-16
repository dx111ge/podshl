//! User-initiated problem reports, generalised before they leave.
//!
//! Mirrors the reference implementation: the direction of the request is the
//! design. The user presses "report this", so the question is not "may we
//! collect" but "do you want this fixed" — and there is deliberately no
//! background path doing the same thing quietly, because that would devalue the
//! button immediately.
//!
//! Anonymity is engineered, not asserted. A diagnostic fingerprint is close to
//! unique per machine, so values are coarsened per field, free text never
//! travels, and no timestamp or identifier accompanies the report.

use serde_json::{json, Map, Value};

/// The most free text one report carries, in characters. The operator refuses
/// more too; this is so the user hears it before pressing send, not after.
pub const MAX_DESCRIPTION: usize = 16 * 1024;

/// Every outcome a report may carry, and the whole of it.
///
/// Held against the operator's own `OUTCOMES`, which is held against the
/// database's CHECK: a word this client invents is a report the operator
/// refuses after the person pressed send, which is the worst moment to find
/// out.
pub const OUTCOMES: &[&str] = &["resolved", "unresolved", "escalated", "abstained", "uncovered"];

/// The report as the operator takes it, for a project that published files
/// rather than running an agent.
///
/// Built from the same `build` output as the vendor path — the same policy,
/// the same two maps, the same `dropped` — and re-shaped only in what the
/// recipient keys on: a `subject` host instead of a skill, and a pseudonym per
/// subject. Checked on the way out, so this client is never the one that sends
/// free text without its consent or a report the operator would refuse.
pub fn for_operator(report: &Value, subject: &str, pseudonym: &str, epoch: &str)
    -> Result<Value, String> {
    if subject.trim().is_empty() {
        return Err(m!("report_no_recipient"));
    }
    let outcome = report.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
    // `uncovered` is the fifth, and it is the one the published path could not
    // say. The other four all describe what happened *after* an answer: it
    // worked, it did not, it was handed on, the person stopped. There was no
    // word for the run that never got an answer at all — the person told us
    // none of the published problems fitted, or their rules matched nothing —
    // and asking leaves no trace, because `/diagnose` runs in `db.read()`.
    //
    // So the one case a maintainer most needs to hear about was the one case
    // that could not reach them. A gap in the published answers is invisible
    // from inside the project: nobody files an issue saying "your support page
    // did not have my problem on it", they close the window.
    if !OUTCOMES.contains(&outcome) {
        return Err(m!("report_bad_outcome", o = format!("{outcome:?}")));
    }
    let map = |k: &str| -> Result<Value, String> {
        match report.get(k) {
            Some(v) if v.is_object() => Ok(v.clone()),
            None => Ok(json!({})),
            _ => Err(m!("report_not_object", k = k)),
        }
    };
    let mut body = json!({
        "subject": subject,
        "pseudonym": pseudonym,
        "epoch": epoch,
        "observed": map("observed")?,
        "stated": map("stated")?,
        "decided_on": report.get("decided_on").cloned().unwrap_or(json!([])),
        "dropped": report.get("dropped").cloned().unwrap_or(json!([])),
        "failed_actions": report.get("failed_actions").cloned().unwrap_or(json!([])),
        "outcome": outcome,
        // Nothing was generated on this path: the answer is the publisher's own
        // text, selected by their own rules.
        "model_class": "none",
    });
    if let Some(text) = report.get("description").and_then(|v| v.as_str()) {
        let consent = report.get("description_consent").cloned().unwrap_or(Value::Null);
        let granted = consent.get("granted") == Some(&Value::Bool(true));
        let named = consent.get("destination").and_then(|d| d.as_str()).map_or(false, |d| !d.trim().is_empty());
        if !(granted && named) {
            return Err(m!("free_text_without_consent"));
        }
        if text.chars().count() > MAX_DESCRIPTION {
            return Err(m!("free_text_too_long", n = MAX_DESCRIPTION));
        }
        body["description"] = json!(text);
        body["description_consent"] = consent;
    }
    Ok(body)
}

fn digits(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn major(v: &str, parts: usize) -> String {
    let d = digits(v);
    if d.is_empty() {
        return "?".into();
    }
    let head = d.iter().take(parts).cloned().collect::<Vec<_>>().join(".");
    if d.len() > parts {
        format!("{head}.x")
    } else {
        head
    }
}

/// Chosen to lose information rather than keep it: a human answer is free text
/// until proven otherwise, and anything unrecognised is dropped.
///
/// **A program's own version travels exactly.** `program_version` and
/// `container_image_version` return nothing but a version token, and it is the
/// publisher's own product: 1.2.1 and 1.2.2 differ by exactly the fix a
/// maintainer shipped, and "1.2.x" cannot tell them whether it helped. The
/// `version` rule below was written for readings of *other* software — a
/// driver build, an interpreter — where the last component says more about
/// the machine than about the problem, and it still applies there.
fn policy(id: &str, kind: &str, has_choices: bool, op: Option<&str>) -> &'static str {
    if kind == "human" {
        return if has_choices { "exact" } else { "never" };
    }
    if id.contains("serial") {
        return "never";
    }
    if matches!(op, Some("program_version") | Some("container_image_version")) {
        return "exact";
    }
    // A location. `python.venv.base` is the directory the interpreter lives
    // in, `VIRTUAL_ENV` and `HF_HOME` are directories by definition, and the
    // rest of any such path is the account it sits under.
    if id == "python.venv.base" || id.ends_with("VIRTUAL_ENV") || id.ends_with("HF_HOME") {
        return "path";
    }
    if id.contains("version") {
        return "major_minor";
    }
    if id.ends_with("_mib") {
        return "bucket";
    }
    "exact"
}

/// Does this value look like an absolute path on some platform? `/usr/bin`,
/// `C:\Users\x`, `\\server\share`. A reading that is one is a location
/// whatever its id says, and travels as its last component.
fn looks_like_path(s: &str) -> bool {
    let b = s.as_bytes();
    s.starts_with('/')
        || s.starts_with("\\\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// The last component of a path, whichever separator wrote it. A trailing
/// separator is not a component, and a bare root has none.
fn basename(s: &str) -> Option<String> {
    s.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|c| !c.is_empty())
        .map(str::to_string)
}

fn generalise(v: &Value, how: &str) -> Option<Value> {
    match how {
        "never" => None,
        // A path is a path however the id was named. This is the one policy
        // that looks at the value: a reading nobody thought of as a location
        // — a tool that prints where it was installed — is still one.
        "exact" if v.as_str().map_or(false, looks_like_path) => generalise(v, "path"),
        "exact" => Some(v.clone()),
        "path" => v.as_str().and_then(basename).map(Value::String),
        "major_minor" => v.as_str().map(|s| Value::String(major(s, 2))),
        "major" => v.as_str().map(|s| Value::String(major(s, 1))),
        // A reading arrives as the tool printed it — `nvidia-smi` says
        // `8192 MiB` — and this used to accept only a JSON number, so every VRAM
        // reading on every path was dropped as "held back" and never travelled
        // at all. The leading integer is the value; the unit is in the id.
        "bucket" => v.as_i64().or_else(|| {
            v.as_str().and_then(|s| digits(s).first().and_then(|d| d.parse().ok()))
        }).map(|n| {
            let band = [2048, 4096, 8192, 12288, 16384, 24576]
                .iter()
                .find(|e| n <= **e)
                .map(|e| format!("<={e}"))
                .unwrap_or_else(|| ">24576".into());
            Value::String(band)
        }),
        _ => None,
    }
}

/// Returns (report, held_back) — the second is shown to the user so what did
/// *not* travel is as visible as what did.
/// A value somebody typed is not a value something measured, and the two must
/// not be able to occupy the same slot.
///
/// `vendors::mismatch` already states the rule for the case one level up —
/// *their claim is a claim; the reading is not* — and this is the same rule
/// applied to a fact. A person answering "3.11" where `python.version` was read
/// is not correcting the reading; they are answering a different question, and
/// letting the answer overwrite the reading would make an assertion
/// indistinguishable from a measurement in everything downstream of it: the
/// solution match, the report, and the corpus the report joins.
///
/// So the report has two maps. `observed` carries what was **measured** and
/// nothing else. `stated` carries what a **person supplied** — typed, chosen,
/// or answering a probe the skill itself declared `human`. A key appears in
/// exactly one of them.
///
/// Kept apart rather than flagged in place, because the failure mode to design
/// against is a consumer that does not look. Flag it in place and anyone who
/// ignores the flag reads a claim as a measurement; split it and anyone who
/// ignores `stated` simply sees fewer facts, all of them true.
///
/// A `null` in `stated` means supplied but withheld. Free text a person typed
/// never travels — `T4`, and that does not bend, because a typed answer can
/// contain anything. But *that* somebody supplied it is a property of the
/// report rather than anything about them, and it is exactly what the
/// recipient needs.
///
/// This is what a maintainer needs and cannot otherwise get. A solution that
/// matched on a measurement and failed is a defect in their rule and worth
/// their time. One that matched on a value somebody typed and failed may be
/// nothing of the sort — and without the split those two arrive identical.
/// Attach free text the user asked us to send, with the consent that permits it.
///
/// Kept out of `build` on purpose. Everything `build` produces is derived from
/// readings and policy; this is the one part that exists only because a person
/// read the exact words and said yes. Folding it in would let a caller produce
/// a report carrying free text without ever having shown it to anybody.
///
/// `granted_at` is a year-month, the same granularity as the pseudonym. A
/// precise time would relink the report to the run that produced it, which is
/// the one thing this format refuses — and the receiving database requires
/// *some* record of when consent was given, not a forensic one.
pub fn with_consented_text(
    report: &mut Value,
    text: &str,
    destination: &str,
    epoch: &str,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(m!("nothing_to_send"));
    }
    // A bound here as well as in the window and on the server. An excerpt is
    // the lines around a failure; a whole log is a record of somebody's day,
    // and nobody reviews forty thousand lines before pressing "send".
    if text.chars().count() > MAX_DESCRIPTION {
        return Err(m!("free_text_over", n = text.chars().count(), max = MAX_DESCRIPTION));

    }
    if destination.trim().is_empty() {
        return Err(m!("consent_without_recipient"));
    }
    // The excerpt bound and the anonymiser, here as well as in the window.
    // The window showed the anonymised text and then handed *its* text to
    // this command — which is the same text only as long as the window is the
    // one this binary shipped with. What leaves is what this function makes
    // of it, and the person saw the same function's work on the screen.
    let (text, _) = crate::redact::bound(text);
    let (text, _) = crate::redact::anonymise(&text);
    let o = report.as_object_mut().ok_or_else(|| m!("no_report"))?;
    o.insert("description".into(), json!(text));
    o.insert(
        "description_consent".into(),
        json!({"granted": true, "destination": destination, "granted_at": epoch}),
    );
    Ok(())
}

pub fn build(
    skill: &Value,
    facts: &Value,
    stated: &[String],
    decided_on: &[String],
    resolved_by: &str,
    outcome: &str,
) -> (Value, Vec<String>) {
    let probes = skill.get("probes").and_then(|p| p.as_array()).cloned().unwrap_or_default();
    let mut observed = Map::new();
    let mut asserted = Map::new();
    let mut held = Vec::new();

    if let Some(map) = facts.as_object() {
        for (k, v) in map {
            let p = probes.iter().find(|p| p.get("id").and_then(|i| i.as_str()) == Some(k));
            let declared_kind = p
                .and_then(|p| p.get("kind"))
                .and_then(|x| x.as_str())
                .unwrap_or("machine");
            // A person supplied it if they typed or chose it, or if the skill
            // declared the probe human — those are the same thing arriving by
            // two routes, and the recipient has no reason to care which.
            let from_a_person = stated.iter().any(|s| s == k) || declared_kind == "human";
            let how = match p {
                Some(p) => policy(
                    k,
                    if from_a_person { "human" } else { "machine" },
                    p.get("choices").map(|c| !c.is_null()).unwrap_or(false),
                    p.get("read").and_then(|r| r.get("op")).and_then(|o| o.as_str()),
                ),
                // No declared probe: free text, and it does not travel.
                None => "never",
            };
            // Coarsened by policy, then anonymised: a reading is whatever a
            // program printed, and what a program prints can name the account
            // it was built or installed under, the host, an address.
            match generalise(v, how).map(|g| crate::redact::anonymise_value(&g)) {
                Some(g) if from_a_person => {
                    asserted.insert(k.clone(), g);
                }
                Some(g) => {
                    observed.insert(k.clone(), g);
                }
                // Free text a person typed does not travel — `T4`, and it is
                // not negotiable: a typed answer can contain anything. But
                // *that* a person supplied it is not their data, it is a
                // property of the report, and the recipient needs it to know
                // whether an outcome says anything about their rule. So the
                // key is present and the value is null: supplied, withheld.
                None if from_a_person => {
                    asserted.insert(k.clone(), Value::Null);
                    held.push(k.clone());
                }
                None => held.push(k.clone()),
            }
        }
    }
    held.sort();

    (
        json!({
            "skill_id": skill.get("id"),
            "skill_version": skill.get("version"),
            "resolved_by": resolved_by,
            "outcome": outcome,
            "observed": observed,
            // Kept apart rather than flagged in place. A recipient that knows
            // nothing about provenance reads `observed` and gets only
            // measurements — wrong-but-safe rather than confidently wrong —
            // and one that wants the whole truth reads both.
            "stated": asserted,
            // Which facts the answer turned on, where the producer said so.
            // Empty is honest: it means nobody told us, not that nothing did.
            "decided_on": decided_on,
            "dropped": held.clone(),
            "failed_actions": [],
            // No incident id and no timestamp: an individual report must not be
            // linkable back to the run that produced it.
        }),
        held,
    )
}

/// `observed` for a report with no skill behind it, under the same policy as
/// `build`.
///
/// The no-vendor path sent whatever the window handed it, verbatim: every
/// reading the local model had asked for, at full precision, with no policy
/// between the machine and the operator — the one report path with no
/// coarsening at all. The policy is keyed off the read op, and without a
/// skill the op comes from the catalogue: each fact id is looked up there,
/// and one the catalogue does not know has no policy and does not travel.
/// The two facts the client knows without reading (`os`, `arch`) and the one
/// it derives (`gpu.bf16_native`) are the client's own and travel exactly.
///
/// Returns the map and what was held back, like `build`.
pub fn observed_by_catalogue(facts: &Value) -> (Map<String, Value>, Vec<String>) {
    let catalogue = crate::reads::catalogue_all();
    let op_of = |id: &str| -> Option<String> {
        catalogue.as_array()?.iter()
            .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(id))
            .and_then(|c| c.pointer("/read/op"))
            .and_then(|o| o.as_str())
            .map(str::to_string)
    };
    let mut observed = Map::new();
    let mut held = Vec::new();
    for (k, v) in facts.as_object().into_iter().flatten() {
        let own = crate::reads::baseline().get(k).is_some() || k == "gpu.bf16_native";
        let how = match op_of(k) {
            Some(op) => policy(k, "machine", false, Some(&op)),
            None if own => "exact",
            None => "never",
        };
        match generalise(v, how).map(|g| crate::redact::anonymise_value(&g)) {
            Some(g) => {
                observed.insert(k.clone(), g);
            }
            None => held.push(k.clone()),
        }
    }
    held.sort();
    (observed, held)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The skill whose probes decide the policy. A fact with no matching probe
    /// is dropped rather than passed through: an unrecognised value is exactly
    /// the one nobody has decided the coarsening for.
    fn rma_skill() -> Value {
        json!({
            "id": "warranty.rma.precheck", "version": "1.4.2",
            "probes": [
                {"id": "gpu.name", "kind": "machine"},
                {"id": "gpu.driver_version", "kind": "machine"},
                {"id": "gpu.vram_total_mib", "kind": "machine"},
                {"id": "serial.printed", "kind": "human"},
                {"id": "symptom", "kind": "human"},
                {"id": "chart.confirmed", "kind": "human", "choices": ["SKR03", "SKR04"]}
            ]
        })
    }

    /// T4: a serial number and free text never travel, and the omission is
    /// declared. A serial identifies a person's hardware; free text is where
    /// everything the policy did not anticipate ends up.
    #[test]
    fn a_serial_and_free_text_never_travel() {
        let facts = json!({
            "gpu.name": "RTX 2070 SUPER",
            "serial.printed": "0324718061234",
            "symptom": "Artefakte im Bild, seit gestern"
        });
        let (report, held) = build(&rma_skill(), &facts, &[], &[], "general_agent", "unresolved");
        let blob = serde_json::to_string(&report).unwrap();

        for secret in ["0324718061234", "Artefakte"] {
            assert!(!blob.contains(secret), "{secret:?} leaked into the report");
        }
        for id in ["serial.printed", "symptom"] {
            assert!(held.iter().any(|h| h == id), "{id} was not declared as held back");
        }
    }

    /// A human probe offering a fixed set of answers is *not* free text, so it
    /// travels. The distinction is the point: choosing from a list cannot smuggle
    /// anything the policy has not seen.
    ///
    /// It travels in `stated`, never in `observed`. A person picked it, and a
    /// bounded answer is still an answer — the list makes it safe to carry, not
    /// true.
    #[test]
    fn a_human_answer_from_a_fixed_list_does_travel() {
        let facts = json!({"chart.confirmed": "SKR04"});
        let (report, held) = build(&rma_skill(), &facts, &[], &[], "vendor_skill", "resolved");
        assert_eq!(report["stated"]["chart.confirmed"], "SKR04");
        assert!(report["observed"].get("chart.confirmed").is_none(),
                "an answer a person chose was reported as something the machine measured");
        assert!(!held.iter().any(|h| h == "chart.confirmed"));
    }

    /// T5: version numbers of *other* software are coarsened — an exact driver
    /// build says more about the machine than about the problem. A program's
    /// own version, read by `program_version`, is not: it is the publisher's
    /// product, and the last component is the fix they shipped.
    #[test]
    fn version_numbers_are_coarsened() {
        let facts = json!({"gpu.driver_version": "610.57.04"});
        let (report, _) = build(&rma_skill(), &facts, &[], &[], "vendor_skill", "resolved");
        assert_eq!(report["observed"]["gpu.driver_version"], "610.57.x");
        assert_eq!(major("610.57.04", 1), "610.x");

        let skill = json!({"id": "s", "version": "1", "probes": [
            {"id": "engram.version", "kind": "machine",
             "read": {"op": "program_version", "program": "engram"}},
            {"id": "ollama.container_version", "kind": "machine",
             "read": {"op": "container_image_version", "image": "ollama/ollama"}},
            {"id": "python.version", "kind": "machine",
             "read": {"op": "run_tool", "tool": "python3", "args": ["--version"]}}]});
        let facts = json!({"engram.version": "1.2.2", "ollama.container_version": "0.3.14",
                           "python.version": "Python 3.12.14"});
        let (report, _) = build(&skill, &facts, &[], &[], "vendor_skill", "resolved");
        assert_eq!(report["observed"]["engram.version"], "1.2.2", "the publisher's own version was coarsened");
        assert_eq!(report["observed"]["ollama.container_version"], "0.3.14");
        assert_eq!(report["observed"]["python.version"], "3.12.x", "an interpreter reading travelled exactly");
    }

    /// Memory sizes travel as a band, never as a number.
    #[test]
    fn sizes_travel_as_a_band() {
        let facts = json!({"gpu.vram_total_mib": 8192});
        let (report, _) = build(&rma_skill(), &facts, &[], &[], "vendor_skill", "resolved");
        assert_eq!(report["observed"]["gpu.vram_total_mib"], "<=8192");
        // And as the tool actually prints it. Only a JSON number used to be
        // accepted, so this — the real reading — was always dropped.
        let facts = json!({"gpu.vram_total_mib": "12282 MiB"});
        let (report, held) = build(&rma_skill(), &facts, &[], &[], "vendor_skill", "resolved");
        assert_eq!(report["observed"]["gpu.vram_total_mib"], "<=12288",
                   "the reading nvidia-smi prints did not travel: held back {held:?}");
    }

    /// T6: no timestamp and no identifier. A report that can be linked back to
    /// the run that produced it is not anonymous, whatever else is stripped.
    #[test]
    fn a_report_carries_no_timestamp_and_no_identifier() {
        let facts = json!({"gpu.name": "RTX 2070 SUPER"});
        let (report, _) = build(&rma_skill(), &facts, &[], &[], "general_agent", "resolved");
        let o = report.as_object().unwrap();
        for forbidden in ["incident_id", "opened_at", "closed_at", "at", "timestamp", "id"] {
            assert!(!o.contains_key(forbidden), "the report carries {forbidden}");
        }
        assert_eq!(
            o.keys().cloned().collect::<Vec<_>>(),
            vec!["decided_on", "dropped", "failed_actions", "observed", "outcome",
                 "resolved_by", "skill_id", "skill_version", "stated"],
            "the report's shape changed — every field here is one a vendor sees"
        );
    }

    /// What this client builds must match the schema this project publishes. A
    /// spec a vendor implements against, whose own reference client does not
    /// satisfy it, is worse than no spec.
    #[test]
    fn the_report_this_client_builds_matches_the_published_schema() {
        let facts = json!({"gpu.name": "RTX", "gpu.driver_version": "610.57.04",
                           "serial.printed": "0324718061234"});
        let (built, _) = build(&rma_skill(), &facts, &[], &[], "vendor_skill", "resolved");
        let parsed: crate::wire::Report = serde_json::from_value(built.clone())
            .unwrap_or_else(|e| panic!("this client's own report does not match the schema: {e}\n{built}"));
        assert_eq!(parsed.resolved_by, "vendor_skill");
        assert!(parsed.pseudonym.is_none(), "build() must not invent an identity");
        assert!(parsed.dropped.iter().any(|d| d == "serial.printed"));
    }

    /// A fact the skill never asked for is dropped, not forwarded. The policy is
    /// keyed off the probe list, so an unknown id has no policy at all.
    #[test]
    fn a_fact_the_skill_did_not_ask_for_is_dropped() {
        let facts = json!({"gpu.name": "RTX", "browser.history": "everything"});
        let (report, held) = build(&rma_skill(), &facts, &[], &[], "general_agent", "resolved");
        assert!(report["observed"].get("browser.history").is_none(), "an unasked fact travelled");
        assert!(held.iter().any(|h| h == "browser.history"));
    }

    /// R3. A value somebody typed must not travel wearing the name of one
    /// something measured. The skill declared `gpu.name` a machine probe; a
    /// person answering it is not correcting the reading, and a report that
    /// carried their answer under that name would put an assertion into the
    /// corpus as a measurement — where nothing downstream could ever tell.
    #[test]
    fn a_stated_value_never_travels_as_a_reading() {
        let skill = json!({"id":"s","version":"1","probes":[
            {"id":"gpu.name","kind":"machine"},
            {"id":"which.one","kind":"human","choices":["a","b"]}
        ]});
        let facts = json!({"gpu.name":"RTX 4090","which.one":"a"});

        let (measured, _) = build(&skill, &facts, &[], &[], "vendor_skill", "resolved");
        assert_eq!(measured["observed"]["gpu.name"], "RTX 4090", "a real reading was dropped");
        // A probe the skill declared `human` is a person's answer however it
        // arrived, so it is never a measurement.
        assert!(measured["observed"].get("which.one").is_none(),
                "a declared human answer was reported as a measurement");
        assert_eq!(measured["stated"]["which.one"], "a", "a bounded answer was lost");

        // The same reading, this time typed by a person.
        let (claimed, held) = build(&skill, &facts, &["gpu.name".into()], &[], "vendor_skill", "resolved");
        assert!(claimed["observed"].get("gpu.name").is_none(),
                "a value somebody typed was reported as a machine reading");
        // Typed free text does not travel — T4 — but the recipient is still
        // told that a person supplied it, because that is what decides whether
        // the outcome says anything about their rule.
        assert!(claimed["stated"].as_object().unwrap().contains_key("gpu.name"),
                "the recipient cannot tell this fact came from a person");
        assert_eq!(claimed["stated"]["gpu.name"], Value::Null,
                   "a typed free-text value travelled");
        assert!(held.contains(&"gpu.name".to_string()), "withheld without saying so");

        // The two maps never overlap: a key is measured or supplied, not both.
        for k in claimed["stated"].as_object().unwrap().keys() {
            assert!(claimed["observed"].get(k).is_none(), "{k} is in both maps");
        }
    }

    /// A reading with a question attached — the publisher writes one fact and
    /// it can arrive either way. What matters is that the report says which,
    /// and that the authored `choices` make the answered form travel at all:
    /// bounded answers may, free text never does.
    #[test]
    fn a_reading_with_a_question_attached_arrives_either_way() {
        let skill = json!({"id":"s","version":"1","probes":[
            {"id":"pip.version","kind":"machine",
             "read":{"op":"run_tool","tool":"pip","args":["--version"]},
             "prompt":"Which pip?","choices":["23.x or newer","pip is not installed"]}
        ]});

        // Read on this machine: a measurement.
        let (read, _) = build(&skill, &json!({"pip.version":"23.2"}), &[], &[],
                              "vendor_skill", "resolved");
        assert_eq!(read["observed"]["pip.version"], "23.2");
        assert!(read["stated"].as_object().unwrap().is_empty());

        // Not readable, so the publisher's question was asked instead. The same
        // fact, and the recipient can tell.
        let (asked, _) = build(&skill, &json!({"pip.version":"23.x or newer"}),
                               &["pip.version".into()], &[], "vendor_skill", "resolved");
        assert!(asked["observed"].as_object().unwrap().is_empty(),
                "an answer was reported as a measurement");
        assert_eq!(asked["stated"]["pip.version"], "23.x or newer",
                   "a bounded answer to an authored question did not travel — without it the publisher never learns this fact at all");
    }

    /// PB3: a report about a published project goes to the operator in the
    /// shape the operator takes — subject and pseudonym, the same two maps —
    /// and the reading that the user helped locate is still a reading.
    ///
    /// It used to go through `send_report`, A2A to an agent the project never
    /// ran, built from a skill the published path never had: every fact was
    /// dropped and the send failed. Nothing about that was visible until the
    /// window was walked to the end.
    #[test]
    fn a_published_report_reaches_the_operator_in_its_own_shape() {
        let card = json!({"id": "engram.install.no-build-for-this-platform", "version": "v1.2.0",
            "probes": [
                {"id": "os.name", "kind": "machine", "read": {"op": "os_fact", "name": "os"}},
                {"id": "engram.version", "kind": "machine",
                 "read": {"op": "program_version", "program": "engram"}},
                {"id": "engram.symptom", "kind": "human", "choices": ["the binary will not start at all"]},
                {"id": "error.text", "kind": "human"}
            ]});
        let facts = json!({"os.name": "macos", "engram.version": "1.2.2",
                           "engram.symptom": "the binary will not start at all",
                           "error.text": "zsh: bad CPU type in executable: engram"});
        let (built, _) = build(&card, &facts, &["error.text".into()], &["os.name".into()],
                               "vendor_skill", "resolved");
        let body = for_operator(&built, "engram.localhost", "p-123", "2026-09").unwrap();

        assert_eq!(body["subject"], "engram.localhost");
        assert_eq!(body["pseudonym"], "p-123");
        assert_eq!(body["model_class"], "none", "nothing was generated on this path");
        assert_eq!(body["observed"]["os.name"], "macos");
        // Read by running the program the user pointed at: a measurement — and
        // exact, because it is the publisher's own version. "1.2.x" could not
        // tell a maintainer whether their 1.2.2 fix reached anybody.
        assert_eq!(body["observed"]["engram.version"], "1.2.2");
        assert_eq!(body["stated"]["engram.symptom"], "the binary will not start at all");
        assert_eq!(body["stated"]["error.text"], Value::Null, "free text travelled unasked");
        assert!(body.get("description").is_none());

        // With its own consent it travels, and not without.
        let mut with = built.clone();
        with_consented_text(&mut with, "zsh: bad CPU type", "engram.localhost via operator", "2026-09").unwrap();
        let body = for_operator(&with, "engram.localhost", "p-123", "2026-09").unwrap();
        assert_eq!(body["description"], "zsh: bad CPU type");
        assert_eq!(body["description_consent"]["granted"], true);

        let mut forged = built.clone();
        forged["description"] = json!("typed but never agreed to");
        assert!(for_operator(&forged, "engram.localhost", "p", "2026-09").is_err(),
                "free text without its consent was sent");

        let mut bad = built.clone();
        bad["outcome"] = json!("maybe");
        assert!(for_operator(&bad, "engram.localhost", "p", "2026-09").is_err());

        // **The run that never got an answer travels too.** Somebody said none
        // of the published problems fitted; that is a sentence about the
        // published answers, and until this word existed there was no way for
        // it to reach the person who wrote them.
        let mut none_fitted = built;
        none_fitted["outcome"] = json!("uncovered");
        let body = for_operator(&none_fitted, "engram.localhost", "p", "2026-09").unwrap();
        assert_eq!(body["outcome"], "uncovered");
    }

    /// T12: a location travels as its last component. The rest of a path is
    /// the account it lives under — `C:\Users\jdoe\proj\.venv` says who
    /// somebody is far more than which virtualenv they use — and a reading
    /// nobody named as a location is still one if it looks like one.
    #[test]
    fn a_path_travels_as_its_last_component_only() {
        let skill = json!({"id":"s","version":"1","probes":[
            {"id":"python.venv.base","kind":"machine",
             "read":{"op":"read_ini_key","path":".venv/pyvenv.cfg","key":"home"}},
            {"id":"env.HF_HOME","kind":"machine","read":{"op":"env_var","name":"HF_HOME"}},
            {"id":"app.install_dir","kind":"machine","read":{"op":"read_registry","path":"HKCU:\\X","name":"InstallPath"}},
            {"id":"gpu.name","kind":"machine"}
        ]});
        let facts = json!({"python.venv.base": "C:\\Users\\jdoe\\proj\\.venv\\Scripts",
                           "env.HF_HOME": "/home/jdoe/.cache/huggingface",
                           "app.install_dir": "C:\\Program Files\\Engram\\",
                           "gpu.name": "RTX 4090"});
        let (report, held) = build(&skill, &facts, &[], &[], "vendor_skill", "resolved");
        assert_eq!(report["observed"]["python.venv.base"], "Scripts", "{report}");
        assert_eq!(report["observed"]["env.HF_HOME"], "huggingface", "{report}");
        // Named as nothing in particular, and still a path.
        assert_eq!(report["observed"]["app.install_dir"], "Engram", "{report}");
        assert_eq!(report["observed"]["gpu.name"], "RTX 4090", "a plain value was cut");
        assert!(held.is_empty(), "{held:?}");
        let blob = report.to_string();
        assert!(!blob.contains("jdoe") && !blob.contains("Users"), "the account survived: {blob}");
        assert_eq!(basename("/"), None);
        assert_eq!(basename("/usr/bin/"), Some("bin".into()));
    }

    /// T13: the account, the host and a machine identifier never travel,
    /// whatever a reading printed them inside. The policy coarsens by id; the
    /// anonymiser is the floor under the value, and it is applied to every
    /// string before it leaves.
    #[test]
    fn the_account_the_host_and_a_machine_id_never_travel_inside_a_reading() {
        let skill = json!({"id":"s","version":"1","probes":[
            {"id":"app.owner","kind":"machine"},
            {"id":"app.machine_id","kind":"machine"},
            {"id":"app.node","kind":"machine"},
            {"id":"which.one","kind":"human","choices":["a"]}
        ]});
        let user = ["USERNAME", "USER", "LOGNAME"].iter()
            .find_map(|k| std::env::var(k).ok()).filter(|u| u.chars().count() >= 3);
        let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok()
            .filter(|h| h.chars().count() >= 3);
        let facts = json!({
            "app.owner": format!("built by {}", user.clone().unwrap_or_else(|| "nobody".into())),
            "app.machine_id": "3f2b1c9e-8a7d-4e6f-9b0a-1c2d3e4f5a6b",
            "app.node": format!("on {}", host.clone().unwrap_or_else(|| "nowhere".into())),
            "which.one": "a"
        });
        let (report, _) = build(&skill, &facts, &[], &[], "vendor_skill", "resolved");
        let blob = report.to_string();
        assert!(!blob.contains("3f2b1c9e"), "a machine GUID travelled: {blob}");
        assert_eq!(report["observed"]["app.machine_id"], "<uuid>");
        match user {
            Some(u) => {
                assert!(!blob.contains(&u), "the account name travelled: {blob}");
                assert_eq!(report["observed"]["app.owner"], "built by <user>");
            }
            None => eprintln!("no account name of three characters or more here — that half not attempted"),
        }
        match host {
            Some(h) => assert!(!blob.to_lowercase().contains(&h.to_lowercase()), "the host name travelled: {blob}"),
            None => eprintln!("no host name here — that half not attempted"),
        }
        assert_eq!(report["stated"]["which.one"], "a");
    }

    /// T14: the no-vendor report is coarsened like every other. It sent what
    /// the window handed it, verbatim — the one report path with no policy at
    /// all. Without a skill the policy comes from the catalogue: a fact the
    /// catalogue does not know does not travel.
    #[test]
    fn a_report_without_a_vendor_is_coarsened_by_the_catalogue() {
        let facts = json!({
            "os": "windows", "arch": "x86_64",
            "gpu.name": "NVIDIA GeForce RTX 2070 SUPER",
            "gpu.driver_version": "610.57.04",
            "gpu.vram_total_mib": "8192 MiB",
            "gpu.bf16_native": false,
            "python.venv.base": "/home/jdoe/proj/.venv",
            "browser.history": "everything",
            "gpu.serial": "0324718061234"
        });
        let (observed, held) = observed_by_catalogue(&facts);
        assert_eq!(observed["os"], "windows");
        assert_eq!(observed["gpu.name"], "NVIDIA GeForce RTX 2070 SUPER");
        assert_eq!(observed["gpu.driver_version"], "610.57.x", "a driver build travelled exactly");
        assert_eq!(observed["gpu.vram_total_mib"], "<=8192", "memory travelled as a number");
        assert_eq!(observed["gpu.bf16_native"], false);
        assert_eq!(observed["python.venv.base"], ".venv", "a path travelled whole");
        assert!(observed.get("browser.history").is_none(), "a fact the catalogue does not know travelled");
        assert!(observed.get("gpu.serial").is_none(), "a serial travelled");
        assert_eq!(held, vec!["browser.history", "gpu.serial"]);
    }

    /// LX8: the words a person agreed to send are bounded and anonymised by
    /// the binary, not only by the window that showed them.
    #[test]
    fn consented_text_is_bounded_and_anonymised_by_the_binary() {
        let mut r = json!({});
        let long: String = (0..300).map(|i| format!("line {i} from sven@example.org\n")).collect();
        with_consented_text(&mut r, &long, "acme.example", "2026-09").unwrap();
        let d = r["description"].as_str().unwrap();
        assert!(d.lines().count() <= crate::redact::MAX_LINES, "the whole log was attached");
        assert!(d.ends_with("line 299 from <email>"), "the end of the log is where the failure is: {d:?}");
        assert!(!d.contains("sven@example.org"), "an address travelled: {d}");
    }

    /// An excerpt is bounded before the user is asked, not by a refusal after.
    #[test]
    fn free_text_is_bounded() {
        let mut r = json!({});
        let long = "x".repeat(MAX_DESCRIPTION + 1);
        assert!(with_consented_text(&mut r, &long, "d", "2026-09").is_err(),
                "a whole log was attached as a description");
        assert!(r.get("description").is_none());
    }

    /// A publisher may ask a free-text question, and the answer is the single
    /// most useful thing they can receive — an error message copied by hand is
    /// exactly what no policy can produce for them. So it is not withheld
    /// forever; it is withheld *by default* and offered under its own consent.
    ///
    /// The report is complete without it. Consent adds it, names who receives
    /// it, and records when at the granularity of the pseudonym — a precise
    /// time would relink the report to the run that produced it.
    #[test]
    fn free_text_travels_only_on_its_own_consent() {
        let skill = json!({"id":"s","version":"1","probes":[
            {"id":"error.text","kind":"human","prompt":"Paste the error"}
        ]});
        let facts = json!({"error.text":"ERROR: could not build wheels for lxml"});

        let (mut r, held) = build(&skill, &facts, &["error.text".into()], &[],
                                  "vendor_skill", "unresolved");
        // Nothing yet. The words are not in the report, and the recipient can
        // still see that the question was asked and went unanswered to them.
        assert!(r.get("description").is_none(), "free text travelled unasked");
        assert_eq!(r["stated"]["error.text"], Value::Null);
        assert!(held.contains(&"error.text".to_string()));

        // Consent with nobody named is not consent.
        assert!(with_consented_text(&mut r, "ERROR: …", "  ", "2026-09").is_err());
        assert!(r.get("description").is_none(), "text attached despite a refused consent");

        with_consented_text(&mut r, facts["error.text"].as_str().unwrap(),
                            "acme.example", "2026-09").unwrap();
        assert_eq!(r["description"], "ERROR: could not build wheels for lxml");
        let c = &r["description_consent"];
        assert_eq!(c["granted"], true);
        assert_eq!(c["destination"], "acme.example");
        // Year-month, not an instant: the receiving database wants a record of
        // consent, not a way back to the run.
        assert_eq!(c["granted_at"], "2026-09");
        assert_eq!(c["granted_at"].as_str().unwrap().len(), 7);
    }
}
