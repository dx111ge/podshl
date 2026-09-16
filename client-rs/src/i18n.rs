//! The language files, and the one property that matters about them.
//!
//! A support client that speaks one language is not a horizontal layer, and
//! this product's own argument is that the localisation matrix should collapse
//! onto the client rather than onto the vendor. So adding a language is one
//! file, one line in `languages.json`, and one pull request — and the keys are
//! the contract.
//!
//! **That promise was not keepable until 2026-09-16.** This repository is
//! developed in one place and published to another, and the publication copies
//! over the public checkout — so the first contributed language would have been
//! deleted by the next release, as one `D` line in a diff somebody was asked to
//! read. The step that publishes now refuses instead of overwriting, and
//! `CONTRIBUTING.md` describes the way in. (That script is not named here: it
//! stays in the working repository, and a published file pointing at something
//! nobody outside can read is a dangling reference — `P8`, which caught this
//! paragraph.)
//!
//! **A missing key falls back to English silently.** In a consent dialogue that
//! is the failure that matters: it looks like a design choice rather than a
//! gap, and somebody agrees to something they did not read. Hence the check.
//!
//! These were `.js` files assigning into a global, which meant reading them the
//! way the client reads them required a JavaScript engine. The check therefore
//! shelled out to `node`, and an earlier version of it matched keys with a
//! regular expression that saw only the first key on each line — a quarter of
//! the table was invisible to it, and a checker with blind spots reports clean
//! exactly where it cannot look. As JSON, the suite parses the same bytes the
//! window parses, with no second toolchain in the way.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the language files live, relative to the crate.
pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ui").join("i18n")
}

/// The index beside them: every code, and the name that language calls itself.
///
/// **It exists because the window stopped loading all of them.** A table is
/// 42 KB and the window used to fetch every one at startup to use a single one
/// — 169 KB for four, and this product's own argument is that the localisation
/// matrix collapses onto the client, so four is the small case, not the large
/// one. It now fetches English (the fallback every sentence can fall back to)
/// and the chosen language, and nothing else. What it still needs from the rest
/// is one string each, to draw the picker — which is this file.
///
/// A directory cannot be listed over `fetch`, and in a bundled application
/// `ui/` is inside the binary rather than on disk, so the binary cannot list it
/// either. Hence a file, and hence the test below holding it to the directory:
/// an index that disagrees with what is there offers a language that will not
/// load, or hides one that would.
pub const INDEX: &str = "languages.json";

/// Every language file, by code, parsed. Fails loudly rather than returning an
/// empty map: no languages at all must never look like no problem.
pub fn tables() -> BTreeMap<String, Value> {
    let d = dir();
    let mut out = BTreeMap::new();
    let entries = std::fs::read_dir(&d).unwrap_or_else(|e| panic!("cannot read {}: {e}", d.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // The index is not a language, and reading it as one would invent a
        // table called `languages` with no `_name` in it.
        if path.file_name().and_then(|f| f.to_str()) == Some(INDEX) {
            continue;
        }
        let code = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let table: Value = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
        out.insert(code, table);
    }
    assert!(!out.is_empty(), "no language files in {}", d.display());
    out
}

/// The index, parsed.
pub fn index() -> BTreeMap<String, String> {
    let p = dir().join(INDEX);
    let raw = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    /// W13: a sentence the window asks for without values must not contain any.
    ///
    /// `t("key")` fills nothing in, so a sentence behind such a call that still
    /// carries `{n}` reaches a person with the word `undefined` in it - which is
    /// how it was found, in the report panel of the real window.
    ///
    /// The cause is worth keeping. The count in that sentence is one the server
    /// **withholds on purpose**: below the floor it does not return `reporters`,
    /// because "you are the third" is a count about other people's machines, and
    /// counting up to the floor one report at a time is how the floor gets read
    /// from outside. The sentence could never have been filled in, so the text
    /// was the half that was wrong.
    ///
    /// Every language, because a translator who adds a placeholder English does
    /// not have makes the same hole.
    /// `t("key")` with nothing after the key.
    const RE_BARE: &str = r#"t\(\s*["']([a-z0-9_]+)["']\s*\)"#;
    /// `{n}`, `{host}` - anything the window would have had to fill in.
    const RE_HOLE: &str = r"\{[a-z_]+\}";

    #[test]
    fn a_sentence_asked_for_without_values_carries_none() {
        let ui = crate::ui_contract::ui_source();
        // `t("key")` with nothing after the key: the call sites that supply no
        // values at all.
        let bare = regex::Regex::new(RE_BARE).unwrap();
        let keys: Vec<String> = bare.captures_iter(&ui).map(|c| c[1].to_string()).collect();
        assert!(keys.len() > 50,
                "only {} valueless sentences found - the pattern no longer matches", keys.len());

        let placeholder = regex::Regex::new(RE_HOLE).unwrap();
        let mut holes = vec![];
        for (lang, table) in crate::i18n::tables() {
            for key in &keys {
                if let Some(s) = table.get(key).and_then(|v| v.as_str()) {
                    if let Some(m) = placeholder.find(s) {
                        holes.push(format!("{lang}.{key} wants {}", m.as_str()));
                    }
                }
            }
        }
        assert!(holes.is_empty(),
                "sentences shown with nothing to put in them: {holes:?}");
    }

    use super::*;

    fn keys(table: &Value) -> Vec<String> {
        let mut k: Vec<String> = table.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    /// I1: every language defines the same keys. English is the reference
    /// because it is the one every vendor owes and the one the client falls
    /// back to.
    #[test]
    fn every_language_defines_the_same_keys() {
        let tables = tables();
        let english = tables.get("en").expect("there is no English table to fall back to");
        let expected = keys(english);
        assert!(expected.len() > 50, "the English table is suspiciously small");

        for (code, table) in &tables {
            let got = keys(table);
            let missing: Vec<&String> = expected.iter().filter(|k| !got.contains(k)).collect();
            let extra: Vec<&String> = got.iter().filter(|k| !expected.contains(k)).collect();
            assert!(
                missing.is_empty(),
                "{code} is missing {missing:?} — those would silently fall back to English, \
which in a consent dialogue reads as a design choice rather than a gap"
            );
            assert!(extra.is_empty(), "{code} defines {extra:?}, which English does not");
        }
    }

    /// I2: every language names itself, in itself. A picker that lists
    /// "German" to somebody who reads only German has missed the point.
    #[test]
    fn every_language_names_itself() {
        for (code, table) in tables() {
            let name = table.get("_name").and_then(|v| v.as_str()).unwrap_or_default();
            assert!(!name.is_empty(), "{code} does not name itself");
        }
    }

    /// I7: the index names exactly the languages that exist, by the name each
    /// one gives itself.
    ///
    /// The window draws its picker from the index and fetches a table only when
    /// it is chosen, so the two failures this prevents are both silent from the
    /// window's side: a code in the index with no file behind it offers a
    /// language that fails to load when somebody picks it, and a file with no
    /// entry is a language nobody can reach. Neither shows up in `I1`, which
    /// only compares the tables that were found.
    #[test]
    fn the_index_names_every_language_and_only_those() {
        let tables = tables();
        let index = index();

        let have: Vec<&String> = tables.keys().collect();
        let listed: Vec<&String> = index.keys().collect();
        assert_eq!(
            have, listed,
            "languages.json lists {listed:?} and the directory holds {have:?} — a code with no \
             file behind it is a language that fails to load when somebody picks it, and a file \
             with no entry is one nobody can reach"
        );

        for (code, table) in &tables {
            let own = table.get("_name").and_then(|v| v.as_str()).unwrap_or_default();
            assert_eq!(
                index.get(code).map(String::as_str),
                Some(own),
                "{code} calls itself {own:?} and the index calls it \
                 {:?} — the picker would show a name the language does not use",
                index.get(code)
            );
        }
    }

    /// Placeholders are part of the contract too: a translation that drops
    /// `{n}` renders a sentence with a hole where the number should be, and
    /// nothing else would catch it.
    #[test]
    fn every_translation_keeps_the_placeholders_english_declares() {
        let tables = tables();
        let english = tables["en"].as_object().unwrap();
        let placeholders = |s: &str| -> Vec<String> {
            let mut out: Vec<String> = s
                .match_indices('{')
                .filter_map(|(i, _)| s[i..].find('}').map(|j| s[i..i + j + 1].to_string()))
                .collect();
            out.sort();
            out.dedup();
            out
        };

        for (code, table) in &tables {
            if code == "en" {
                continue;
            }
            for (key, value) in table.as_object().unwrap() {
                let (Some(theirs), Some(ours)) = (value.as_str(), english.get(key).and_then(|v| v.as_str()))
                else {
                    continue;
                };
                assert_eq!(
                    placeholders(theirs),
                    placeholders(ours),
                    "{code}.{key} does not carry the same placeholders as English"
                );
            }
        }
    }

    /// The window finds its languages in the index, and carries no list of its
    /// own to drift from the directory.
    ///
    /// **This case used to say the opposite.** The window declared
    /// `const LANGUAGES = ["de", "en", "fr", "es"]` and fetched every one of
    /// them at startup, and this test held that array to the directory. Both
    /// halves were wrong for a product whose own argument is that the
    /// localisation matrix collapses onto the client: the array was a second
    /// place to maintain, and fetching all of them cost 42 KB per language to
    /// use one — 169 KB at four, and four is the small case.
    ///
    /// So the list moved into `languages.json`, where `I7` holds it to the
    /// files, and the window loads English and the chosen language only. What
    /// this checks is that it has not grown a private list again, which is the
    /// one way the index could stop being the truth.
    #[test]
    fn the_window_takes_its_languages_from_the_index() {
        let ui = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/index.html"))
            .expect("cannot read index.html");
        assert!(
            ui.contains("i18n/languages.json"),
            "the window no longer reads the index, so nothing says which languages exist"
        );
        assert!(
            !ui.contains("const LANGUAGES = ["),
            "the window declares its own list of languages again — that is a second place to \
             maintain, and `I7` cannot see it"
        );
        // English is not optional: every sentence falls back to it, and the
        // binary's messages are recognised by their English template.
        assert!(
            ui.contains("loadTable(\"en\")"),
            "the window does not load English unconditionally, so a missing key in the chosen \
             language has nothing to fall back to"
        );
        assert!(
            dir().join("en.json").exists(),
            "there is no English table to fall back to"
        );
    }
}
