//! What the client knows about itself.
//!
//! Deliberately almost nothing: operating system and architecture. Everything
//! else a vendor needs comes from that vendor's own read instructions, because
//! only it knows what is worth knowing about its product.
//!
//! A probe returning `None` is not a failure — it is what arms a human probe,
//! so the user is only ever asked for what the machine could not supply. The
//! serial is the standing example: consumer cards report nothing, so the
//! printed sticker is the only source, and a warranty check needs it.

use serde_json::Value;

pub fn os_id() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// Decide before probing whether a skill applies to this machine at all.
/// "We could not check" and "this advice is not for your machine" are different
/// answers, and conflating them is how a Mac user gets told to update a driver
/// that cannot exist.
pub fn applicability(required: &Value) -> (bool, String) {
    // Only facts about ourselves. Whether a *product* is present is the
    // vendor's question to answer: its read instructions either return a value
    // or they do not, and it abstains on what it cannot see. Probing for
    // hardware here would be the client guessing at a domain it does not own.
    if let Some(want) = required.get("os").and_then(|v| v.as_str()) {
        if want != os_id() {
            return (false, m!("guide_other_os", want = want, os = os_id()));
        }
    }
    (true, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// S4: advice for another operating system is refused before anything is
    /// read, and the refusal says which system it was written for.
    #[test]
    fn a_skill_for_another_os_is_refused_before_probing() {
        let (ok, why) = applicability(&json!({"os": "plan9"}));
        assert!(!ok, "a skill for plan9 was accepted on {}", os_id());
        assert!(why.contains("plan9"), "the refusal does not name the system: {why}");
    }

    /// The skill for *this* system is not refused, or the gate would be a wall.
    #[test]
    fn a_skill_for_this_os_is_accepted() {
        let (ok, why) = applicability(&json!({"os": os_id()}));
        assert!(ok, "the skill for this very system was refused: {why}");
    }

    /// S3, and this is a deliberate divergence rather than a gap. The other
    /// implementation refused a skill whose `applies_to` asked for CUDA by
    /// probing the machine for a GPU. This client does not: whether a *product*
    /// is present is the vendor's question, answered by its own read
    /// instructions returning a value or not. Probing hardware here would be
    /// the client guessing at a domain it does not own — and it would be a read
    /// performed before anyone consented to one.
    ///
    /// So a key that is not `os` is informational, and must not become a
    /// silent refusal.
    #[test]
    fn a_non_os_requirement_is_not_a_refusal() {
        for required in [
            json!({"cuda": "true"}),
            json!({"product": "toolkit", "component": "gpu"}),
            json!({"os": os_id(), "product": "accounting-suite"}),
        ] {
            let (ok, why) = applicability(&required);
            assert!(ok, "{required} was refused without the client being able to check it: {why}");
        }
    }

    /// An empty requirement applies everywhere. A vendor that declares nothing
    /// has not thereby excluded anyone.
    #[test]
    fn no_requirement_applies_everywhere() {
        assert!(applicability(&json!({})).0);
    }
}
