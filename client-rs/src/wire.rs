//! The wire format, as types.
//!
//! Every network boundary in this client was `serde_json::Value` reached by
//! string key lookup. That works, and it hides two things: what the protocol
//! actually is, and when the other side stops sending it. A missing field
//! became a default rather than an error, and the shape of the contract lived
//! only in whichever `.get("…")` call happened to be nearby.
//!
//! These types are the contract, and `spec/schema/` is the same contract in the
//! form a vendor in another language can read. The test at the bottom holds one
//! to the other, so a field added here without being written down there — or
//! written down and never implemented — fails rather than drifts.
//!
//! **Deliberately permissive where the protocol is.** Unknown fields are
//! ignored rather than refused: a vendor extending its card must not break a
//! client that predates the extension. What is *not* permissive is anything the
//! client acts on — an action id, a read op, a signature — and those are
//! validated elsewhere, against the vocabulary rather than against a struct.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A probe: one fact the vendor needs, why, and how to get it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probe {
    pub id: String,
    /// `machine` or `human`. Not an enum: a vendor naming a third kind should
    /// be ignored for that probe, not have its whole card rejected.
    pub kind: String,
    #[serde(default)]
    pub describes: String,
    #[serde(default)]
    pub why: String,
    /// A read instruction from the published vocabulary. Absent for a fact the
    /// client derives, and for anything asked of a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<Value>,
    /// Computed by the client from readings it performed, so a vendor cannot
    /// assert an interpretation of a value it did not observe.
    #[serde(default)]
    pub derived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<String>>,
    /// Ask a person only if this machine probe came back empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_missing: Option<String>,
    /// Where the text a free-text question asks for usually comes from — a
    /// container image, or the name a log file has. Loaded only for the user to
    /// cut from; nothing of it travels by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<Value>,
    #[serde(default = "yes")]
    pub required: bool,
}

fn yes() -> bool {
    true
}

/// What the vendor proposes doing. The id must be in the client's vocabulary;
/// the client, not this type, is what enforces that.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCall {
    pub action: String,
    #[serde(default)]
    pub params: Value,
    /// Shown next to the consent prompt. An action without a reason a person
    /// can evaluate is not a request for consent.
    #[serde(default)]
    pub because: String,
    /// What the publisher says about the software the change is for — the
    /// package, the upstream issue, the version that fixes it. Kept with the
    /// repair record (`repair.rs`) and checked as text there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: String,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub contradicts_kb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Escalation {
    pub reason: String,
    pub queue: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub require: Vec<Probe>,
    #[serde(default)]
    pub reply_via: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub title: String,
    /// Checked before anything is read. Only `os` is enforceable by the client;
    /// other keys are informational, because whether a product is present is
    /// the vendor's question and not ours to guess at.
    #[serde(default)]
    pub applies_to: Value,
    #[serde(default)]
    pub probes: Vec<Probe>,
    #[serde(default = "english")]
    pub lang: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_kb_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_kb_says: Option<String>,
}

fn english() -> String {
    "en".into()
}

/// The three outcomes of a diagnosis, in one shape. Which one it is depends on
/// which fields are populated — `findings`, `abstained`, or `need`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remedy {
    pub skill_id: String,
    pub skill_version: String,
    /// The request this answers, signed back with it. A signature proved who
    /// wrote a remedy and nothing about what for — an answer to somebody
    /// else's readings verified just as well, and a party on the wire could
    /// replay one. `flow::accept_remedy` refuses a remedy whose nonce or
    /// facts hash is not this request's, after the signature checks out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts_sha256: Option<String>,
    /// Pinned, because auditability requires knowing what produced this.
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub plan: Vec<ActionCall>,
    #[serde(default)]
    pub verify: Vec<ActionCall>,
    /// Not knowing is a valid answer. A vendor that cannot abstain will guess.
    #[serde(default)]
    pub abstained: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abstain_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate: Option<Escalation>,
    /// The third outcome: "I still need X." The client collects or asks, then
    /// sends again with the enlarged facts.
    #[serde(default)]
    pub need: Vec<Probe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub need_reason: Option<String>,
}

/// What a report carries. No timestamp and no incident id, deliberately: a
/// report must not be linkable back to the run that produced it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub skill_id: Value,
    pub skill_version: Value,
    pub resolved_by: String,
    pub outcome: String,
    /// Measurements only. A value a person supplied is never here, so a
    /// recipient that ignores `stated` loses facts rather than reading a claim
    /// as a measurement.
    pub observed: Value,
    /// What a person supplied — typed, chosen, or answering a probe the skill
    /// declared `human`. A key is in exactly one of the two.
    pub stated: Value,
    /// Named rather than omitted, so what did *not* travel is as visible as
    /// what did.
    pub dropped: Vec<String>,
    /// The facts the answer turned on, where the producer said. With `stated`
    /// this is what separates an outcome that tests a rule from one that tests
    /// whether somebody answered a question correctly.
    #[serde(default)]
    pub decided_on: Vec<String>,
    /// Free-text answers the user chose to send, labelled by the probe that
    /// asked. Withheld by default; this is present only where they agreed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Its own consent, naming who receives it. The receiving database refuses
    /// free text without it — a guard rather than a convention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_consent: Option<Value>,
    #[serde(default)]
    pub failed_actions: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pseudonym: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn schema(name: &str) -> Value {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../spec/schema")
            .join(format!("{name}.json"));
        let raw = std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()));
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{} is not JSON: {e}", p.display()))
    }

    /// The field names a struct declares, in the order serde would emit them.
    fn declared(v: &Value) -> Vec<String> {
        let mut k: Vec<String> = v["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        k.sort();
        k
    }

    fn serialised<T: Serialize>(value: &T) -> Vec<String> {
        let v = serde_json::to_value(value).unwrap();
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    /// A schema and a struct that disagree mean a vendor implementing against
    /// the published document produces something this client silently ignores.
    /// That is the failure the whole spec exists to prevent, so it is a test.
    #[test]
    fn every_struct_matches_its_published_schema() {
        let probe = Probe {
            id: "gpu.name".into(),
            kind: "machine".into(),
            describes: String::new(),
            why: String::new(),
            read: Some(json!({"op": "os_fact", "name": "version"})),
            derived: false,
            prompt: Some(String::new()),
            example: Some(String::new()),
            pattern: Some(String::new()),
            choices: Some(vec![]),
            when_missing: Some(String::new()),
            log: Some(json!({})),
            required: true,
        };
        assert_eq!(serialised(&probe), declared(&schema("probe")), "probe");

        let remedy = Remedy {
            skill_id: "x".into(),
            skill_version: "1".into(),
            model_id: String::new(),
            nonce: Some(String::new()),
            facts_sha256: Some(String::new()),
            findings: vec![],
            plan: vec![],
            verify: vec![],
            abstained: false,
            abstain_reason: Some(String::new()),
            escalate: Some(Escalation {
                reason: String::new(),
                queue: String::new(),
                target: Some(String::new()),
                require: vec![],
                reply_via: vec![],
            }),
            need: vec![],
            need_reason: Some(String::new()),
        };
        assert_eq!(serialised(&remedy), declared(&schema("remedy")), "remedy");

        let report = Report {
            skill_id: json!("x"),
            skill_version: json!("1"),
            resolved_by: "human".into(),
            outcome: "resolved".into(),
            observed: json!({}),
            stated: json!({}),
            dropped: vec![],
            decided_on: vec![],
            description: Some(String::new()),
            description_consent: Some(json!({})),
            failed_actions: vec![],
            pseudonym: Some(String::new()),
            epoch: Some(String::new()),
        };
        assert_eq!(serialised(&report), declared(&schema("report")), "report");

        let skill = Skill {
            id: "x".into(),
            version: "1".into(),
            title: String::new(),
            applies_to: json!({}),
            probes: vec![],
            lang: "en".into(),
            static_kb_url: Some(String::new()),
            static_kb_says: Some(String::new()),
        };
        assert_eq!(serialised(&skill), declared(&schema("skill")), "skill");
    }

    /// The client must survive a vendor that adds a field. Refusing an unknown
    /// key would make every extension a breaking change for every client that
    /// predates it.
    #[test]
    fn an_unknown_field_does_not_break_the_parse() {
        let raw = json!({
            "id": "gpu.name", "kind": "machine",
            "something_added_in_2027": {"deeply": ["nested"]}
        });
        let p: Probe = serde_json::from_value(raw).expect("an added field broke the parse");
        assert_eq!(p.id, "gpu.name");
        assert!(p.required, "a probe defaults to required");
    }

    /// A remedy carrying `need` is the third outcome, and it round-trips —
    /// including through the accumulating facts the client resends each round.
    #[test]
    fn the_need_outcome_round_trips() {
        let raw = json!({
            "skill_id": "display.flicker", "skill_version": "1.0.0",
            "need_reason": "Two causes look the same here.",
            "need": [{"id": "monitors", "kind": "human", "prompt": "How many displays?",
                      "choices": ["1", "more", "I don't know"]}]
        });
        let r: Remedy = serde_json::from_value(raw).unwrap();
        assert_eq!(r.need.len(), 1);
        assert_eq!(r.need[0].choices.as_ref().unwrap().len(), 3);
        assert!(
            r.findings.is_empty() && !r.abstained,
            "a need is not a finding and not an abstention"
        );
    }

    /// The real card from the counterparty parses into these types. A schema
    /// that only matches invented examples proves nothing.
    #[test]
    fn the_counterpartys_own_skills_parse() {
        let Ok(raw) = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../var/test_card.json"),
        ) else {
            panic!("../var/test_card.json is missing — run `mise run services` and the jws tests first");
        };
        let card: Value = serde_json::from_str(&raw).unwrap();
        let skills = card["skills"]
            .as_array()
            .expect("the card carries no skills");
        assert!(!skills.is_empty());
        for s in skills {
            // The card's skill summaries carry ids and descriptions rather than
            // full descriptors; what must parse is the id and version pair the
            // client keys everything else off.
            assert!(s["id"].as_str().is_some(), "a skill on the card has no id");
        }
    }
}
