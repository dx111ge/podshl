//! What the binary says to a person, and the one place it is written.
//!
//! The window chooses the language; the binary does not know which, and most of
//! what it says to a person crosses the Tauri bridge as an error string. Those
//! strings were German literals in the source, so somebody who had picked
//! English read the consent sentence in English and the reason it failed in
//! German — and the dry run, the one sentence the whole consent design rests
//! on, in German too.
//!
//! Every such sentence is now an `m_*` entry in `ui/i18n/en.json`, which the
//! binary reads at compile time; the source holds codes, and every language —
//! German included — lives in the language files and nowhere else. The window
//! has the same files: it recognises a message by its English template and
//! says it again in the chosen language, with the values carried across. A
//! sentence the window does not recognise is shown as the binary said it,
//! in English, rather than not at all.
//!
//! One file for both sides is the point. Two copies of a sentence drift, and
//! the drift would be invisible: the window would stop recognising a message
//! and quietly show it untranslated again.

use serde_json::Value;
use std::sync::OnceLock;

fn table() -> &'static Value {
    static T: OnceLock<Value> = OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str(include_str!("../ui/i18n/en.json")).expect("ui/i18n/en.json is not JSON")
    })
}

/// Whether `s` is the message `code`, whatever values it was filled with.
///
/// For the code that sorts a refusal into a kind the window can name. It used
/// to look for German words inside the sentence, which tied the sorting to the
/// wording: rephrase a message and a refusal silently became `invalid`.
pub fn is(code: &str, s: &str) -> bool {
    let Some(template) = table().get(format!("m_{code}")).and_then(|v| v.as_str()) else {
        return false;
    };
    let mut pattern = String::from("^");
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        pattern.push_str(&regex::escape(&rest[..open]));
        match rest[open..].find('}') {
            Some(close) => {
                pattern.push_str("(?s:.*?)");
                rest = &rest[open + close + 1..];
            }
            None => {
                pattern.push_str(&regex::escape(&rest[open..]));
                rest = "";
            }
        }
    }
    pattern.push_str(&regex::escape(rest));
    pattern.push('$');
    regex::Regex::new(&pattern).map(|re| re.is_match(s)).unwrap_or(false)
}

/// The sentence for `code`, with its placeholders filled in one pass — a value
/// that happens to contain `{p}` is not filled a second time.
pub fn fill(code: &str, args: &[(&str, String)]) -> String {
    let key = format!("m_{code}");
    let Some(template) = table().get(&key).and_then(|v| v.as_str()) else {
        // The suite proves every code in the source has an entry; a release
        // build that somehow lacks one says which, rather than nothing.
        return key;
    };
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    #[cfg(test)]
    let mut used: Vec<&str> = Vec::new();
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if after[..close].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                let name = &after[..close];
                match args.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        #[cfg(test)]
                        panic!("{key} needs {{{name}}}, and the call does not give it");
                        #[cfg(not(test))]
                        out.push_str(&rest[open..open + close + 2]);
                    }
                }
                #[cfg(test)]
                used.push(name);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    #[cfg(test)]
    for (k, _) in args {
        assert!(used.contains(k), "{key} has no {{{k}}}, and the call gives it");
    }
    out
}

/// `m!("not_a_dir", p = path.display())` — the sentence `m_not_a_dir`, filled.
/// Values are `Display`; pass `format!("{x:?}")` for a quoted one.
macro_rules! m {
    ($code:literal $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::msg::fill($code, &[$((stringify!($k), ($v).to_string())),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Every `m!("…")` in the source, by code.
    fn codes_in_source() -> BTreeSet<String> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let re = regex::Regex::new(r#"m!\(\s*"([a-z0-9_]+)""#).unwrap();
        let mut out = BTreeSet::new();
        for entry in std::fs::read_dir(&src).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") || path.ends_with("msg.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for c in re.captures_iter(&text) {
                out.insert(c[1].to_string());
            }
        }
        out
    }

    /// M1: every message the binary can say has its sentence — and so, through
    /// I1, a sentence in every language the window offers. A code without one
    /// reaches the person as `m_code`.
    #[test]
    fn every_message_the_binary_says_has_a_sentence() {
        let codes = codes_in_source();
        assert!(codes.len() > 40, "suspiciously few messages found: {codes:?}");
        let missing: Vec<&String> = codes
            .iter()
            .filter(|c| table().get(format!("m_{c}")).and_then(|v| v.as_str()).is_none())
            .collect();
        assert!(missing.is_empty(), "no m_* sentence in en.json for {missing:?}");
    }

    /// M2: and no sentence is kept for a message nothing says. A dead entry is
    /// harmless until somebody translates it carefully, which is not.
    #[test]
    fn no_sentence_is_kept_for_a_message_nothing_says() {
        let codes = codes_in_source();
        let dead: Vec<String> = table()
            .as_object()
            .unwrap()
            .keys()
            .filter_map(|k| k.strip_prefix("m_"))
            .filter(|k| !codes.contains(*k))
            .map(str::to_string)
            .collect();
        assert!(dead.is_empty(), "en.json carries m_* sentences nothing says: {dead:?}");
    }

    /// `is` recognises a message whatever it was filled with, and nothing else.
    #[test]
    fn a_message_is_recognised_by_its_code_not_its_values() {
        let s = m!("not_a_dir", p = "C:\\some where\\x");
        assert!(is("not_a_dir", &s), "{s}");
        assert!(!is("no_backup", &s), "{s}");
        assert!(!is("not_a_dir", &format!("{s}, and more")), "{s}");
    }

    /// M3: the window recognises a message by the text between its
    /// placeholders. Two placeholders side by side leave nothing to tell where
    /// one value ends; a sentence that is only a placeholder matches anything.
    #[test]
    fn every_sentence_can_be_recognised_again() {
        for (k, v) in table().as_object().unwrap() {
            let Some(s) = v.as_str() else { continue };
            if !k.starts_with("m_") {
                continue;
            }
            assert!(!s.contains("}{"), "{k}: two placeholders touch, so the window cannot split them");
            let literal: String = regex::Regex::new(r"\{\w+\}").unwrap().replace_all(s, "").into();
            assert!(literal.trim().len() >= 4, "{k} is almost only placeholders: {s:?}");
        }
    }

    /// Filling is one pass, so a value is never filled again.
    #[test]
    fn a_value_that_looks_like_a_placeholder_stays_a_value() {
        let s = m!("not_a_dir", p = "{p}");
        assert!(s.contains("{p}") && !s.contains("m_not_a_dir"), "{s}");
    }
}
