//! The user's own model — configured by them, never shipped by us.
//!
//! Where a vendor participates, its pinned model generates and it owns what the
//! model produces. Where no vendor exists there is nobody to own it, so it falls
//! to the model the user chose.
//!
//! **A cloud model means the question does leave the device** — to the provider
//! the user picked, not to us. That is a real difference from the vendor path,
//! where only a signature travels, and the settings screen says so plainly
//! rather than letting "your own model" imply "stays local".
//!
//! Providers are not one shape. Anthropic's Messages API is its own protocol —
//! different endpoint, `x-api-key` and `anthropic-version` headers, a different
//! request and response body — and treating it as OpenAI-compatible produces
//! code that silently fails. Each provider gets its own path.
//!
//! Keys live in the OS credential store, never in the config file. A product
//! whose entire argument is trustworthiness cannot leave an API key in
//! plaintext under a dotfile.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

const KEYRING_SERVICE: &str = "de.podshl.client";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// "anthropic" | "openai_compatible" | "ollama" | ""
    pub provider: String,
    /// Model identifier as the provider names it.
    pub model: String,
    /// Base URL. Empty means the provider's default.
    pub endpoint: String,
    /// "local_small" | "local_large" | "cloud" — reported with a problem
    /// report, and a measurement rather than metadata: a small local model
    /// solving something from public knowledge says the information was very
    /// available and the product surface was very bad.
    pub model_class: String,
    /// Whether a key is expected. The key itself is never stored here.
    pub uses_key: bool,
}

impl Config {
    pub fn configured(&self) -> bool {
        !self.provider.is_empty() && !self.model.is_empty()
    }
    pub fn is_cloud(&self) -> bool {
        // The desktop's own agent is whatever it is configured with, and for the
        // one agent measured so far that is a cloud service. Asked of the agent
        // table rather than assumed here, so adding a local agent later does not
        // leave this sentence quietly wrong.
        if self.provider == "omarchy_agent" {
            return crate::omarchy::headless(&self.model, "").map(|h| h.cloud).unwrap_or(true);
        }
        self.provider == "anthropic" || self.provider == "openai_compatible"
    }
    pub fn ux_severity(&self) -> &'static str {
        match self.model_class.as_str() {
            "local_small" => "high",
            "local_large" => "medium",
            _ => "low",
        }
    }
    pub fn base(&self) -> String {
        if !self.endpoint.is_empty() {
            return self.endpoint.trim_end_matches('/').to_string();
        }
        match self.provider.as_str() {
            "anthropic" => "https://api.anthropic.com".into(),
            "ollama" => "http://localhost:11434".into(),
            _ => "https://api.openai.com".into(),
        }
    }
    /// What to call this model on screen.
    ///
    /// **The model name alone is not always enough to know what you chose.**
    /// For the desktop's own agent it collapsed to `claude`, which says nothing
    /// about whose agent it is or that it runs somewhere else — and that name
    /// went into the consent sentence, where the difference is the whole point.
    /// Everywhere else the provider is evident from the model: `qwen3:4b` is
    /// the local one somebody installed, `claude-opus-5` is the API.
    pub fn label(&self) -> String {
        if self.model.is_empty() {
            return self.provider.clone();
        }
        if self.provider == "omarchy_agent" {
            return format!("{} (Omarchy)", self.model);
        }
        self.model.clone()
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("podshl").join("llm.json"))
}

/// The model this client uses.
///
/// **A saved choice wins, whatever it is**, including a saved choice of none.
/// Without one, the desktop's own agent is the model, where `omarchy::offer()`
/// says it works here: Omarchy names it, it is installed, and how to call it
/// with its tools denied was measured. That is the design the Omarchy work was
/// agreed on — no model setup in PODSHL on a desktop that already has one — and
/// the first fresh install on that desktop still said "No own model", because
/// the agent was only ever offered in the settings nobody had opened.
///
/// Nothing is written: the default is recomputed on every start, so choosing
/// something else, or Omarchy's default changing, is never shadowed by a file
/// this function made up. Where the prompt goes is not hidden by this either;
/// every read panel says so from `is_cloud()`.
pub fn load() -> Config {
    let saved = path().and_then(|p| std::fs::read_to_string(p).ok());
    resolve(saved.as_deref(), crate::omarchy::offer())
}

fn resolve(saved: Option<&str>, desktop_agent: Option<(String, bool)>) -> Config {
    if let Some(s) = saved {
        return serde_json::from_str(s).unwrap_or_default();
    }
    match desktop_agent {
        Some((agent, cloud)) => Config {
            provider: "omarchy_agent".into(),
            model: agent,
            endpoint: String::new(),
            // What the settings panel picks for this row: `local ? small : cloud`.
            model_class: if cloud { "cloud" } else { "local_small" }.into(),
            uses_key: false,
        },
        None => Config::default(),
    }
}

pub fn save(c: &Config) -> Result<(), String> {
    let p = path().ok_or_else(|| m!("no_config_dir"))?;
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&p, serde_json::to_string_pretty(c).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ secrets

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, provider).map_err(|e| e.to_string())
}

pub fn set_key(provider: &str, key: &str) -> Result<(), String> {
    let e = entry(provider)?;
    if key.is_empty() {
        let _ = e.delete_credential();
        return Ok(());
    }
    e.set_password(key).map_err(|e| e.to_string())
}

pub fn has_key(provider: &str) -> bool {
    entry(provider).and_then(|e| e.get_password().map_err(|e| e.to_string())).is_ok()
}

fn get_key(provider: &str) -> Option<String> {
    entry(provider).ok()?.get_password().ok()
}

// ------------------------------------------------------------------ calling

/// The diagnostic method, for the path where nobody publishes one.
///
/// On the published path a maintainer has already decided which fact separates
/// which problem, and the client walks their tree. Here there is no tree and no
/// maintainer — so the model is guessing at what to ask, and a small model
/// guesses badly: it asked which monitor was attached before it asked whether
/// anything had changed.
///
/// This is **method, not content**. It says nothing about NVIDIA or pip; it is
/// the shape of a diagnosis, the way `spec/SPEC.md` fixes the shape of a remedy
/// without writing one. That is the line it must not cross — a client that
/// carried domain knowledge would be competing with the publishers it exists to
/// carry, and it would be wrong more often than they are. It runs only where the
/// alternative is not a vendor's answer but no method at all.
///
/// The method is Kepner-Tregoe problem analysis, reduced to what a person at a
/// desk can answer: describe the deviation, bound it by what is *not* affected,
/// find what changed at that boundary, and then hold a cause to explaining both
/// sides of it. The last step is the one models skip and the one that separates
/// a diagnosis from a plausible sentence.
///
/// The client drives the order and the model fills in the domain, because that
/// is the division each is good at: a model can phrase "what similar thing still
/// works?" for an arbitrary problem far better than any fixed string, and it
/// cannot be relied on to remember to ask.
const DIMENSIONS: &[(&str, &str)] = &[
    ("boundary",
     "WHAT IT IS AND IS NOT. Ask what else the user would expect to be affected \
      and is not - another file, another program, another account, another \
      machine, the same thing yesterday. A fault with no boundary has not been \
      described yet, and the boundary is where the cause lives."),
    ("when",
     "WHEN. Ask since when, and whether it is every time or only under some \
      condition - after a while, under load, after waking, only on the first \
      try. 'Always' and 'sometimes' have different causes and the difference \
      is free to obtain."),
    ("extent",
     "HOW MUCH. Ask whether all of it is affected or only part, and whether it \
      is steady, getting worse, or comes and goes. A trend dates the cause; a \
      partial effect locates it."),
];

/// One `ASK:` line, as the separate questions it actually contains.
///
/// **The prompt says `<one question for the user>` and the model does not obey
/// it.** Measured on the first Omarchy desktop: two ASK lines came back holding
/// five questions between them, one of them *"Which Engram download did you
/// install (the exact archive file name, or the version number and where you
/// got it), and how do you start it: by double-clicking, from a menu, or by
/// typing a command in a terminal? If you use a terminal, what exactly does it
/// print?"* — a paragraph, in a panel with one input box.
///
/// So the client splits rather than asks nicely. `LG8` learnt this about
/// glossary terms and it is the same lesson: *"Telling the model to keep the
/// term was tried first and worked by luck of phrasing."* An instruction that
/// needs the model to cooperate is not a bound.
///
/// **What this can and cannot do**, stated rather than implied. It splits at
/// question marks, so three sentences become three questions. It cannot split
/// *"which download did you install and how do you start it?"*, which is two
/// questions inside one sentence and has no mechanical seam — that half stays
/// the prompt's job, and whether the prompt manages it is a thing to measure
/// and not to assume.
fn split_questions(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in line.chars() {
        current.push(c);
        if c == '?' {
            let q = current.trim().to_string();
            if !q.is_empty() {
                out.push(q);
            }
            current.clear();
        }
    }
    // Whatever is left has no question mark on it. A trailing fragment is not a
    // question; a whole line without one is what a model writes when it says
    // "Tell me about X", and that is still worth asking.
    let rest = current.trim();
    if !rest.is_empty() && out.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// The sections an answer is held to. English keywords so they can be parsed;
/// the content is in the user's language, exactly as `READ:` and `ASK:` work.
const CAUSE: &str = "CAUSE:";
const WHY_NOT: &str = "WHY NOT ELSEWHERE:";
const RESTS_ON: &str = "RESTS ON:";
const NEXT: &str = "NEXT:";
const IF_WRONG: &str = "IF WRONG:";
const ABSTAIN: &str = "ABSTAIN:";

/// The language, named rather than coded, for a prompt.
///
/// "the language with the code 'de'" is an abstraction, and a small model
/// resolves it about as often as it ignores it — which the walk showed: the
/// window's labels were German and `gemma3:4b`'s answer was English, in a
/// prompt whose every other word was English. Naming the language plainly and
/// repeating it *after* the English section keywords costs nothing and is what
/// a 4B model actually follows; recency is most of what it has.
///
/// An unknown code falls back to the code itself, which is still better than
/// dropping the instruction: a model that cannot place `pt-BR` will at least
/// see that something other than English was asked for.
fn language_name(code: &str) -> &str {
    match code.split(['-', '_']).next().unwrap_or(code) {
        "de" => "German",
        "en" => "English",
        "fr" => "French",
        "es" => "Spanish",
        other => other,
    }
}

/// What the project itself published about the thing being diagnosed.
///
/// **Without this the model was told nothing about the project at all.** It got
/// a problem sentence and a map of facts named `engram.embedding_changed`, and
/// was asked to reason about software whose name it had never been given, with
/// field names whose meaning the maintainer had written down and we withheld.
/// It guessed, and the guesses were bad, which is what a person on the first
/// Omarchy desktop reported in those words.
///
/// **Its published words, not a pointer to them.** Naming only the repository
/// would be worse than nothing: the agent runs with every tool denied, so it
/// cannot look anything up, and a model asked about `github.com/owner/name`
/// answers from whatever it half-remembers — confidently, and about an obscure
/// project, entirely invented. So what goes in is text the project actually
/// published and the client already holds, and the instruction says plainly
/// that this is all there is.
fn project_context(ctx: &Value) -> String {
    let get = |k: &str| ctx.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let (name, anchor) = (get("name"), get("anchor"));
    if name.is_empty() && anchor.is_empty() {
        return String::new();
    }
    let mut out = format!("\nThis is about {name}");
    if !anchor.is_empty() {
        out.push_str(&format!(" ({anchor})"));
    }
    out.push_str(
        ". Everything below is what that project published about itself, and it \
         is the only thing you know about it: you have not read its code, its \
         documentation or its issues, and you must not draw on anything you \
         think you remember about it. If these words do not support a cause, \
         say so.\n",
    );

    let list = |key: &str, title: &str, out: &mut String| {
        if let Some(a) = ctx.get(key).and_then(|v| v.as_array()) {
            if a.is_empty() {
                return;
            }
            out.push_str(&format!("\n{title}\n"));
            for item in a {
                if let Some(s) = item.as_str() {
                    out.push_str(&format!("  - {s}\n"));
                }
            }
        }
    };

    // The classes are the strongest thing here, because the person has already
    // said something about them: either none fitted, or one did and the rules
    // behind it produced nothing. Both are evidence about where the answer is
    // not.
    let shown = if ctx.get("rejected").and_then(|v| v.as_bool()).unwrap_or(false) {
        "The problems it says it can answer. The person has been shown these and          says none of them is what they are seeing, so the cause is most likely          outside this list:"
    } else {
        "The problems it says it can answer, and which the person has already been shown:"
    };
    list("classes", shown, &mut out);
    list("means", "What the values above mean, in the maintainer's words:", &mut out);
    list("keep", "Terms that are this project's own. Use them exactly as written:", &mut out);
    out
}

fn prompt(problem: &str, facts: &Value, lang: &str, context: &Value) -> String {
    format!(
        "A user has this problem: {problem}\n{project}\n\
         What is known about their device — values read from it, and answers they \
         gave:\n{known}\n\n\
         Answer in {tongue}, briefly, in EXACTLY these sections, each on its own \
         line and each keyword in English:\n\
         {CAUSE} the cause, in one or two sentences.\n\
         {WHY_NOT} why it does not affect the comparable thing the user said still \
         works. If nothing comparable was established, write that.\n\
         {RESTS_ON} which of the facts above this turns on, by their names.\n\
         {NEXT} what to do, the most decisive thing first.\n\
         {IF_WRONG} the single observation that would show this cause is not it.\n\n\
         A cause that cannot explain why the comparable case is unaffected is not the \
         cause yet. If what is known does not support one, write {ABSTAIN} and what \
         is still missing, instead of {CAUSE} — not knowing is a valid answer, and \
         guessing costs the user a day.\n\n\
         Write everything except the section keywords in {tongue}. The keywords stay \
         in English so they can be found; nothing else does, whatever language this \
         instruction or the value names happen to be in.",
        tongue = language_name(lang),
        project = project_context(context),
        known = serde_json::to_string_pretty(facts).unwrap_or_default()
    )
}

/// What a model's answer said, in the sections it was asked for.
///
/// Parsed rather than trusted: a small model adds prose, drops a section, or
/// translates a keyword it was told to leave in English. Nothing here fails on
/// that — an answer with no sections at all is carried whole in `raw` and the
/// window shows it as it arrived, which is also how the reader can see that the
/// model did not follow the method.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Answer {
    pub cause: String,
    pub why_not: String,
    pub rests_on: String,
    pub next: String,
    pub if_wrong: String,
    pub abstained: String,
    /// The fact ids named in `RESTS ON`, matched against what is actually
    /// known rather than taken from the text — the same rule `parse_round`
    /// applies to read ids, and for the same reason.
    pub rests_on_ids: Vec<String>,
    /// `measured`, `rests_on_supplied`, or `unstated` when the model named
    /// nothing. The published path grades a finding exactly this way, and an
    /// answer nobody can weigh is the thing both paths exist to avoid.
    pub confidence: String,
    /// True when at least one section came back. False means the model wrote
    /// prose and the window shows the prose.
    pub structured: bool,
    pub raw: String,
}

/// Split an answer into its sections, and grade what it rests on.
///
/// `typed` is the ids the person supplied rather than the machine produced. A
/// cause resting on one of those may be right and is not evidence about
/// anything: the person could have been mistaken, and the answer says so
/// rather than looking identical to one a reading decided.
pub fn parse_answer(raw: &str, known: &Value, typed: &[String]) -> Answer {
    let mut a = Answer { raw: raw.to_string(), ..Default::default() };
    let keys = [CAUSE, WHY_NOT, RESTS_ON, NEXT, IF_WRONG, ABSTAIN];
    let mut current: Option<&str> = None;
    for line in raw.lines() {
        let t = line.trim();
        let upper = t.to_uppercase();
        // Longest first: `WHY NOT ELSEWHERE:` would otherwise never be reached
        // if some shorter key were its prefix.
        let hit = keys.iter().find(|k| upper.starts_with(**k));
        if let Some(k) = hit {
            current = Some(k);
            a.structured = true;
            let rest = t[k.len()..].trim();
            push(&mut a, k, rest);
        } else if let Some(k) = current {
            if !t.is_empty() {
                push(&mut a, k, t);
            }
        }
    }
    if !a.structured {
        a.cause = raw.trim().to_string();
    }
    let ids: Vec<String> = known.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    a.rests_on_ids = ids.into_iter().filter(|id| names_id(&a.rests_on, id)).collect();
    a.confidence = if a.rests_on_ids.is_empty() {
        "unstated".into()
    } else if a.rests_on_ids.iter().any(|id| typed.iter().any(|t| t == id)) {
        "rests_on_supplied".into()
    } else {
        "measured".into()
    };
    a
}

fn push(a: &mut Answer, key: &str, text: &str) {
    let field = match key {
        CAUSE => &mut a.cause,
        WHY_NOT => &mut a.why_not,
        RESTS_ON => &mut a.rests_on,
        NEXT => &mut a.next,
        IF_WRONG => &mut a.if_wrong,
        _ => &mut a.abstained,
    };
    if text.is_empty() {
        return;
    }
    if !field.is_empty() {
        field.push('\n');
    }
    field.push_str(text);
}

/// Ask the model which of the client's readable facts it needs for *this*
/// question. The alternative — the client picking — is guesswork dressed as
/// diagnosis, and it produced a compute-capability reading for a flickering
/// screen.
/// One diagnostic round: what to read, and what to ask a human.
///
/// A technician does both. Overclocking, which driver was installed before,
/// when it started, which monitor — none of that appears in any command's
/// output, and a diagnosis that only reads is missing half the evidence.
pub struct Round {
    pub read_ids: Vec<String>,
    pub questions: Vec<String>,
    pub done: bool,
}

/// Parse a model's round answer. Split out because it is pure, and because it
/// is exactly where a prompt change quietly stops working.
/// Does this text name exactly this id?
///
/// `contains` is not enough and the walk proved it: the model wrote
/// `os.version` and `os` matched too, so the answer claimed to rest on a fact
/// nobody had named. The same flaw sat in the read path, where the
/// consequence is worse — a reading the model never asked for appearing on a
/// consent panel, which is precisely the sort of thing this product may not do.
///
/// Ids are `[a-z0-9_.]`, so a match counts only where neither neighbour could
/// be part of an id. Written here once and used by both parsers, because two
/// copies of a rule like this is how they come to disagree.
fn names_id(text: &str, id: &str) -> bool {
    let part = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(at) = text[from..].find(id) {
        let start = from + at;
        let end = start + id.len();
        let before_ok = start == 0 || !part(bytes[start - 1] as char);
        let after_ok = end == bytes.len() || !part(bytes[end] as char);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

pub fn parse_round(raw: &str, catalogue: &Value) -> Round {
    let read = raw.lines()
        .find(|l| l.trim().to_uppercase().starts_with("READ:"))
        .map(|l| l.split(':').nth(1).unwrap_or("").to_string())
        .unwrap_or_else(|| raw.to_string());
    let ids: Vec<String> = catalogue.as_array().into_iter().flatten()
        .filter_map(|c| c.get("id").and_then(|v| v.as_str()))
        .filter(|id| names_id(&read, id))
        .map(String::from)
        .collect();
    let questions: Vec<String> = raw.lines()
        .filter_map(|l| {
            let t = l.trim();
            if t.to_uppercase().starts_with("ASK:") { t.splitn(2, ':').nth(1) } else { None }
        })
        .flat_map(|q| split_questions(q))
        .filter(|q| !q.is_empty())
        .take(3)
        .collect();
    let done = ids.is_empty()
        && (read.to_lowercase().contains("done") || read.to_lowercase().contains("none"));
    Round { read_ids: ids, questions, done }
}

#[cfg(test)]
mod round_tests {
    use super::*;
    use serde_json::json;

    fn cat() -> Value {
        json!([{"id":"gpu.name"},{"id":"gpu.driver_version"},{"id":"gpu.temperature"}])
    }

    #[test]
    fn extracts_ids_and_questions() {
        let r = parse_round("READ: gpu.name, gpu.driver_version\nASK: Is the card overclocked?\nASK: Since when?", &cat());
        assert_eq!(r.read_ids, ["gpu.name", "gpu.driver_version"]);
        assert_eq!(r.questions.len(), 2);
        assert!(!r.done);
    }

    /// An id the model invented must not become a read.
    #[test]
    fn ignores_ids_outside_the_catalogue() {
        let r = parse_round("READ: gpu.name, /etc/shadow, disk.dump", &cat());
        assert_eq!(r.read_ids, ["gpu.name"]);
    }

    /// An id mentioned inside a question is not a request to read it.
    #[test]
    fn a_question_mentioning_an_id_is_not_a_read() {
        let r = parse_round("READ: none\nASK: Do you know your gpu.temperature?", &cat());
        assert!(r.read_ids.is_empty(), "id leaked in from a question");
        assert_eq!(r.questions.len(), 1);
    }

    /// The window knows the platform before the first round, always. The
    /// first round has to be told it is the first all the same — it was not,
    /// MD5: the language is named, and named again after the English keywords.
    ///
    /// Found by walking the window in German: the labels were German and
    /// `gemma3:4b`'s answer was English. "the language with the code 'de'" is
    /// an abstraction a 4B model resolves about as often as it ignores, and it
    /// sat before a wall of English keywords — recency is most of what a small
    /// model has, so the last thing it read was English.
    ///
    /// Naming the language plainly and repeating the instruction *after* the
    /// keywords costs nothing. It is not a guarantee and is not claimed as one:
    /// a model that will not follow an instruction cannot be made to. What is
    /// checked here is that the client asks properly, in every prompt that
    /// produces text a person reads.
    #[test]
    fn every_prompt_names_the_language_and_says_it_last() {
        let facts = json!({"gpu.name": "RTX 5070"});
        let prompts = [
            ("answer", prompt("flicker", &facts, "de", &Value::Null)),
            ("round", round_prompt("flicker", &cat(), &facts, "de", 1, &Value::Null)),
        ];
        for (which, p) in prompts {
            assert!(p.contains("German"),
                    "the {which} prompt asks for a language code rather than a language");
            assert!(!p.contains("code 'de'"), "the {which} prompt still passes a bare code");
            // After the English keywords, not only before them.
            let last_keyword = ["ASK:", CAUSE, IF_WRONG].iter()
                .filter_map(|k| p.rfind(k)).max().expect("no keyword in the prompt");
            let last_language = p.rfind("German").unwrap();
            assert!(last_language > last_keyword,
                    "the {which} prompt names the language only before the English \
                     keywords, which is the half a small model forgets");
        }
        // The code travels when the language is not one of the four, rather
        // than the instruction being dropped.
        assert!(prompt("x", &facts, "pt-BR", &Value::Null).contains("pt"));
        assert_eq!(language_name("de-AT"), "German");
    }

    /// MD4: a short id is not dragged in by a longer one that contains it.
    ///
    /// Found by walking the window, not by reading the code. `gemma3:4b` wrote
    /// `RESTS ON: gpu.driver_version, os.version` and the basis came back
    /// naming `os` as well, because the match was `contains` and `os` is a
    /// substring of `os.version`. The answer claimed to rest on a fact nobody
    /// had named.
    ///
    /// The read path had the same flaw and a worse consequence: a model asking
    /// for `os.version` would have had `os` read too — a reading nobody
    /// requested, appearing on a consent panel. Both parsers share one rule
    /// now, because two copies of it is how they come to disagree.
    #[test]
    fn a_short_id_is_not_dragged_in_by_a_longer_one() {
        assert!(names_id("os.version, gpu.name", "os.version"));
        assert!(!names_id("os.version", "os"), "`os` matched inside `os.version`");
        assert!(!names_id("gpu.name_long", "gpu.name"));
        assert!(!names_id("prefix_os", "os"));
        assert!(names_id("os", "os"), "an id alone does not name itself");
        assert!(names_id("READ: os, gpu.name", "os"), "a listed id is not recognised");
        assert!(names_id("the value os.version told us", "os.version"));

        // Through the read parser, where the consequence is a reading nobody
        // asked for reaching a consent panel.
        let catalogue = json!([{"id": "os"}, {"id": "os.version"}, {"id": "gpu.name"}]);
        let r = parse_round("READ: os.version\nASK: since when?", &catalogue);
        assert_eq!(r.read_ids, vec!["os.version"],
                   "a read nobody asked for was added: {:?}", r.read_ids);

        // And through the answer parser, where it inflates what a cause rests on.
        let known = json!({"os": "windows", "os.version": "25H2", "gpu.name": "RTX 5070"});
        let a = parse_answer("CAUSE: the driver.\nRESTS ON: os.version", &known, &[]);
        assert_eq!(a.rests_on_ids, vec!["os.version"],
                   "the answer claims to rest on a fact nobody named: {:?}", a.rests_on_ids);
    }

    /// MD1: the client walks the method; the model fills in the domain.
    ///
    /// Each round carries exactly one dimension, in order, and which one is the
    /// *client's* decision. Telling a model all three at once produces three
    /// shallow questions in one breath; telling it one produces one good
    /// question. And a model asked to remember where it is in a method does
    /// not — which is the whole reason the order lives here rather than in the
    /// prompt's prose.
    #[test]
    fn each_round_carries_one_dimension_of_the_method_in_order() {
        let known = json!({"os": "windows"});
        let seen: Vec<String> = (1..=5)
            .map(|r| round_prompt("flicker", &cat(), &known, "en", r, &Value::Null))
            .collect();

        for (i, (key, _)) in DIMENSIONS.iter().enumerate() {
            let p = &seen[i];
            assert!(p.contains("one of your questions must cover"),
                    "round {} carries no dimension of the method", i + 1);
            // Exactly one: two in a prompt is the thing this exists to avoid.
            let carried: Vec<&str> = DIMENSIONS.iter()
                .filter(|(_, text)| p.contains(text.split(". ").next().unwrap_or(text)))
                .map(|(k, _)| *k)
                .collect();
            assert_eq!(carried, vec![*key],
                       "round {} carries {:?} rather than only {key}", i + 1, carried);
        }
        // Past the ladder it stops repeating the last one forever rather than
        // running off the end: there is no fourth dimension to invent.
        assert_eq!(
            seen[DIMENSIONS.len()], seen[DIMENSIONS.len() + 1],
            "the rounds past the method are not all the same"
        );
        // And what the window already asked is not asked again by the model.
        assert!(seen[0].contains("already been asked"), "{}", seen[0]);
    }

    /// MD2: an answer is parsed into the sections it was asked for, and what it
    /// rests on is graded the way the published path grades a finding.
    ///
    /// A cause resting on a value the person typed may be perfectly right and is
    /// not evidence about anything — they could have been mistaken — and before
    /// the split those two arrived identical. `SV71` makes the same distinction
    /// on the published path; this is the free-running path's half of it.
    #[test]
    fn an_answer_is_split_into_its_sections_and_graded() {
        let known = json!({"gpu.driver_version": "610.88", "change.last": "a driver update",
                           "os.version": "Windows 25H2"});
        let typed = vec!["change.last".to_string()];

        let raw = "CAUSE: The driver update changed the colour pipeline.\n\
                   It only shows on this panel.\n\
                   WHY NOT ELSEWHERE: The second monitor runs at 60 Hz.\n\
                   RESTS ON: gpu.driver_version, change.last\n\
                   NEXT: Roll back the driver.\n\
                   IF WRONG: Rolling back changes nothing.";
        let a = parse_answer(raw, &known, &typed);
        assert!(a.structured);
        assert!(a.cause.contains("colour pipeline") && a.cause.contains("only shows"),
                "a continuation line was dropped: {:?}", a.cause);
        assert!(a.why_not.contains("60 Hz"));
        assert_eq!(a.next, "Roll back the driver.");
        assert_eq!(a.if_wrong, "Rolling back changes nothing.");
        assert!(a.abstained.is_empty());
        // Matched against what is known, not taken from the text: the same rule
        // `parse_round` applies to read ids, and for the same reason.
        assert_eq!(a.rests_on_ids, vec!["change.last", "gpu.driver_version"]);
        assert_eq!(a.confidence, "rests_on_supplied",
                   "a cause resting on a typed value was graded as measured");

        // The same answer resting only on readings.
        let m = parse_answer(&raw.replace(", change.last", ""), &known, &typed);
        assert_eq!(m.rests_on_ids, vec!["gpu.driver_version"]);
        assert_eq!(m.confidence, "measured");

        // Naming nothing is not the same as naming a measurement.
        let u = parse_answer("CAUSE: Something is wrong.", &known, &typed);
        assert_eq!(u.confidence, "unstated");
        assert!(u.structured && u.rests_on_ids.is_empty());

        // Abstaining is a first-class answer, and it replaces the cause.
        let ab = parse_answer("ABSTAIN: Nothing here says which monitor is attached.",
                              &known, &typed);
        assert!(ab.abstained.contains("which monitor") && ab.cause.is_empty());

        // And a model that ignored the format is carried whole rather than
        // shredded: the window shows the prose, and a reader can see that the
        // method was not followed.
        let prose = parse_answer("Try reinstalling the driver.", &known, &typed);
        assert!(!prose.structured);
        assert_eq!(prose.cause, "Try reinstalling the driver.");
        assert_eq!(prose.raw, "Try reinstalling the driver.");
    }

    /// MD3: the answer is asked for the section that distinguishes a diagnosis
    /// from a plausible sentence — why the fault is *not* somewhere it could
    /// have been — and abstaining is offered in the same breath.
    ///
    /// This is the step models skip. A cause that cannot explain why the
    /// comparable case is unaffected has not been tested against anything, and
    /// it reads exactly like one that has.
    #[test]
    fn the_answer_must_explain_what_it_does_not_affect() {
        let p = prompt("flicker", &json!({"gpu.name": "RTX 5070"}), "de", &Value::Null);
        for key in [CAUSE, WHY_NOT, RESTS_ON, NEXT, IF_WRONG, ABSTAIN] {
            assert!(p.contains(key), "the answer is not asked for {key}");
        }
        assert!(p.contains("is not the cause yet"),
                "nothing tells the model an unexplained boundary is not an answer");
        assert!(p.contains("German"), "the answer is not asked for in the user's language");
        // The keywords stay English so they can be parsed; the content does not.
        assert!(p.contains("keyword in English"), "{p}");
    }

    /// and a small model was invited to say "done" before it had read a thing.
    #[test]
    fn the_first_round_is_told_so_even_when_the_platform_is_known() {
        let known = json!({"os": "windows", "arch": "x86_64"});
        let first = round_prompt("flicker", &cat(), &known, "en", 1, &Value::Null);
        assert!(first.contains("first round"), "{first}");
        assert!(!first.contains("READ: done\n\n"), "the first round is invited to stop: {first}");
        assert!(first.contains("\"windows\""), "what is known is not said");
        let later = round_prompt("flicker", &cat(), &known, "en", 2, &Value::Null);
        assert!(!later.contains("first round") && later.contains("write: READ: done"), "{later}");
    }

    #[test]
    fn recognises_completion() {
        assert!(parse_round("READ: done", &cat()).done);
        assert!(parse_round("READ: none", &cat()).done);
    }
}

/// The prompt for one round, apart from the call so it can be read by a test.
#[cfg(test)]
mod question_tests {
    use super::*;

    /// The real thing, from the first Omarchy desktop this ran on.
    ///
    /// Not a made-up example: this is what `claude` returned through the
    /// Omarchy default agent on 2026-09-16, in one `ASK:` line, into a panel
    /// with one input box under it.
    const AS_MEASURED: &str = "What does work, and where else does it fail? Has Engram ever         started on this machine before, and if so, when did it last work? Does it also fail         when you start it from a different user account?";

    #[test]
    fn one_ask_line_holding_three_questions_becomes_three() {
        let qs = split_questions(AS_MEASURED);
        assert_eq!(qs.len(), 3, "still one wall of text: {qs:?}");
        assert!(qs[0].ends_with('?') && qs[1].ends_with('?') && qs[2].ends_with('?'),
                "a piece without its question mark: {qs:?}");
        assert!(qs[1].contains("Has Engram ever"), "{qs:?}");
        // Nothing is lost on the way: every word the model wrote is still there.
        let rejoined: String = qs.join(" ");
        assert_eq!(rejoined.split_whitespace().collect::<Vec<_>>(),
                   AS_MEASURED.split_whitespace().collect::<Vec<_>>(),
                   "splitting dropped or changed words");
    }

    #[test]
    fn one_question_stays_one() {
        assert_eq!(split_questions("Since when does it happen?"),
                   vec!["Since when does it happen?"]);
        // A line with no question mark is still something to ask.
        assert_eq!(split_questions("Tell me what the terminal prints"),
                   vec!["Tell me what the terminal prints"]);
        assert!(split_questions("   ").is_empty());
    }

    /// **What this deliberately cannot do**, so nobody reads the case above as
    /// a claim that the problem is solved. Two questions inside one sentence
    /// have no seam to cut at, and the measured line had one of those too.
    /// Splitting is the half that can be enforced; the rest is the prompt's,
    /// and the prompt is the half that has to be measured rather than trusted.
    #[test]
    fn a_compound_question_in_one_sentence_is_not_split() {
        let one = "Which download did you install and how do you start it?";
        assert_eq!(split_questions(one).len(), 1,
                   "this test exists to record a limit, and the limit moved — which is good \
                    news, but the comment above is now wrong");
    }
}

#[cfg(test)]
mod project_context_tests {
    use super::*;

    fn ctx() -> Value {
        json!({
            "name": "dx111ge/engram",
            "anchor": "https://github.com/dx111ge/engram/",
            "classes": ["engram.llm.model-not-pulled — Debate and Chat fail, but storing knowledge and search still work"],
            "means": ["engram.embedding_changed: Whether the embedding model was changed — nodes are embedded when stored"],
            "keep": ["Engram", "brain", "Debate", "Chat"]
        })
    }

    /// The model is told which project this is, in the project's own words.
    ///
    /// It used to be told nothing at all: a problem sentence and a map of facts
    /// named `engram.*`, about software whose name nobody had given it. The
    /// maintainer had written down what those fields mean and the client kept
    /// it to itself.
    /// **And that the person already rejected them**, where they did.
    ///
    /// That is the one piece of evidence only the window has, and it points
    /// away from everything in the list. A model told only "here are three
    /// problems" will reach for one of the three; told that all three were put
    /// to the person and refused, it has to look elsewhere — which is the whole
    /// reason this path was taken.
    #[test]
    fn a_rejected_list_is_named_as_rejected() {
        let mut c = ctx();
        c["rejected"] = json!(true);
        let p = prompt("x", &json!({}), "en", &c);
        assert!(p.contains("says none of them is what they are seeing"),
                "the model is not told the list was refused:
{p}");
        assert!(p.contains("outside this list"), "{p}");

        let q = prompt("x", &json!({}), "en", &ctx());
        assert!(!q.contains("says none of them"),
                "a list nobody rejected was described as rejected:
{q}");
    }

    #[test]
    fn the_model_is_told_whose_project_this_is_and_what_the_fields_mean() {
        let p = prompt("chat never answers", &json!({"engram.embedding_changed": "yes"}), "en", &ctx());
        assert!(p.contains("dx111ge/engram"), "the model is not told which project this is:\n{p}");
        assert!(p.contains("Whether the embedding model was changed"),
                "the maintainer wrote what the field means and it did not reach the model:\n{p}");
        assert!(p.contains("Debate and Chat fail"),
                "the classes the person was already shown are not in the prompt:\n{p}");
        assert!(p.contains("brain"), "the project's own terms did not reach the model:\n{p}");
    }

    /// **And told that this is all it knows.**
    ///
    /// Naming a repository to a model that cannot fetch it invites the one
    /// failure worse than ignorance: it answers from what it half-remembers,
    /// confidently, and for an obscure project that is invention. The agent runs
    /// with every tool denied, so it *cannot* check — which makes saying so part
    /// of the prompt rather than a nicety.
    #[test]
    fn the_model_is_told_it_has_not_read_the_project() {
        let p = prompt("x", &json!({}), "en", &ctx());
        assert!(p.contains("only thing you know about it"),
                "nothing stops the model drawing on what it thinks it remembers:\n{p}");
        assert!(p.contains("must not draw on anything you"),
                "the instruction against recall is gone:\n{p}");
    }

    /// **Every prompt that reaches a person, not just the last one.**
    ///
    /// The context was given to `prompt` first, and `prompt` produces the final
    /// answer — the thing somebody sees after the questions are over. What they
    /// actually meet is `round_prompt`, which chooses what to read and what to
    /// ask, and `follow_up`. Those had none, so the questions came from a model
    /// that had never been told whose project this was. "It butts in with
    /// nonsense" was about the rounds, and the fix had been aimed past them.
    ///
    /// This is the case that would have caught it, so a fourth prompt added
    /// later cannot quietly be the one without.
    #[test]
    fn all_three_prompts_carry_the_project() {
        let c = ctx();
        let one = prompt("x", &json!({}), "en", &c);
        let two = round_prompt("x", &json!([]), &json!({}), "en", 1, &c);
        let three = {
            // `follow_up` is async and talks to a model, so its prompt is built
            // the same way here rather than called: what is under test is that
            // the text carries the project, not that the network works.
            project_context(&c)
        };

        for (name, text) in [("prompt", &one), ("round_prompt", &two), ("follow_up context", &three)] {
            assert!(text.contains("dx111ge/engram"),
                    "{name} does not name the project:\n{text}");
            assert!(text.contains("only thing you know about it"),
                    "{name} does not stop the model drawing on what it remembers:\n{text}");
        }
    }

    /// No project, no claim about one. The model path also runs where nothing
    /// was published, and inventing a context there would be the same defect
    /// pointing the other way.
    #[test]
    fn without_a_project_the_prompt_says_nothing_about_one() {
        let p = prompt("x", &json!({}), "en", &Value::Null);
        assert!(!p.contains("This is about"), "a project appeared out of nowhere:\n{p}");
        assert!(!p.contains("only thing you know"), "an instruction about a project that is not there:\n{p}");
        // The rest of the prompt is unchanged by its absence.
        assert!(p.contains("A user has this problem"), "{p}");
    }
}

/// **The rounds get the same context as the answer, and they get it first.**
///
/// Only `prompt` was given it, which is the *last* thing a person sees. What
/// they actually met was this: a model choosing which readings to take and
/// asking follow-up questions about a project nobody had named to it. "Claude
/// butts in with nonsense" described the rounds, and the fix had been aimed at
/// the answer nobody had reached yet.
fn round_prompt(problem: &str, catalogue: &Value, known: &Value, lang: &str,
                round: usize, context: &Value) -> String {
    let list = catalogue.as_array().map(|a| a.iter().map(|c| format!(
        "- {}: {}",
        c.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        c.get("describes").and_then(|v| v.as_str()).unwrap_or("")
    )).collect::<Vec<_>>().join("\n")).unwrap_or_default();
    let project = project_context(context);

    // Diagnosis proceeds in rounds. Asking for everything at once forces the
    // model to guess at hardware it has not established yet — it should first
    // find out *what is installed*, then ask what only matters given that.
    // Two sections, both first class. The previous prompt said "answer ONLY
    // with the ids, no other text" and then invited questions — the model
    // obeyed the stronger instruction and never asked anything, so the human
    // was never involved. That was a prompt bug, not a model failure.
    // The first round is the first round, whatever is already known. This asked
    // whether *nothing* was known — and the window always knows the platform
    // before it asks, so no model was ever told to establish what is installed
    // first; a small one was told "when you know enough, say done" instead,
    // and said it before reading anything.
    let stage = if round <= 1 {
        "This is the first round. First establish WHAT IS INSTALLED AT ALL — maker \
         and model. Only once you know that, ask further, specifically."
    } else {
        "When you know enough, write: READ: done"
    };
    // One dimension of the method per round, in order, and the client decides
    // which — not the model. Telling it all three at once produces three
    // shallow questions in one breath; telling it one produces one good one.
    // The window has already asked what changed, before any round, because
    // that question is always worth asking and often ends the diagnosis.
    let (_, method) = DIMENSIONS[(round.max(1) - 1).min(DIMENSIONS.len() - 1)];
    // `known_block`, not `context`: the parameter of that name is the project's
    // published words, and two different things sharing one name in one
    // function is how the wrong one gets used.
    let known_block = if known.as_object().map_or(true, |o| o.is_empty()) {
        String::new()
    } else {
        format!("You already know this about the device:\n{}\n\n",
                serde_json::to_string_pretty(known).unwrap_or_default())
    };
    format!(
        "A user's problem: {problem}\n{project}\n\
         {known_block}\
         The agent can read these values from the device:\n{list}\n\n\
         {stage}\n\n\
         Answer in EXACTLY this format, with no introduction:\n\
         READ: <ids separated by commas, or 'none', or 'done'>\n\
         ASK: <one question, one sentence>\n\
         ASK: <another, optional>\n\n\
         An ASK line is ONE question and ends at its question mark. Do not join two \
         questions with 'and', and do not add a follow-up after the question mark: the \
         person gets one box under each line, and a paragraph cannot be answered in it. \
         If you need two things, write two ASK lines.\n\n\
         The ASK lines are for everything that is in no tool's output and only the user \
         knows — overclocking, which driver ran before, since when it happens, which \
         monitor, what was changed last. Ask at least one question while you are missing \
         something no tool can answer.\n\n\
         This round, one of your questions must cover: {method}\n\
         If that does not apply to this problem, say so in one ASK line beginning \
         'N/A:' and ask something more useful instead. The user has already been asked \
         what changed shortly before it started; do not ask that again.\n\n\
         `READ:` and `ASK:` stay in English so they can be found. Everything after \
         them — every question you ask — is written in {tongue}.",
        tongue = language_name(lang)
    )
}

/// `round` is 1-based and the client's, not the model's: it decides both what
/// stage the diagnosis is at and which dimension of the method this round must
/// cover. A model asked to remember where it is in a method does not.
pub async fn choose_reads(cfg: &Config, problem: &str, catalogue: &Value, known: &Value,
                         context: &Value,
                          lang: &str, round: usize) -> Result<Round, String> {
    let p = round_prompt(problem, catalogue, known, lang, round, context);
    let raw = match cfg.provider.as_str() {
        // The desktop's agent answers the prompt itself, with every tool
        // denied and from an empty directory — see `omarchy.rs` for what was
        // measured and which two switches look like a fence and are not.
        "omarchy_agent" => crate::omarchy::ask(&cfg.model, &p).await?,
        "anthropic" => anthropic(cfg, &p).await?,
        _ => openai_compatible(cfg, &p).await?,
    };
    // Match against the catalogue rather than trusting the model's formatting:
    // a small model will add prose, and an id it invented must not become a read.
    // One parser, and it is the tested one. Duplicating it inline is how the
    // two quietly disagree after a prompt change.
    Ok(parse_round(&raw, catalogue))
}

/// Continue a diagnosis with what has been gathered since. Answering once and
/// stopping is not a diagnosis — "the values are not enough" has to be able
/// to lead somewhere.
pub async fn follow_up(cfg: &Config, problem: &str, facts: &Value, previous: &str,
                       added: &str, lang: &str, context: &Value) -> Result<String, String> {
    // The same sections as the first answer, and for the same reason. A
    // follow-up that dropped back to a paragraph would undo the method one
    // question in - which is exactly when a person is most likely to act on
    // what they read, because they have just given the thing that was missing.
    let project = project_context(context);
    let p = format!(
        "Problem: {problem}\n{project}\n\
         Your answer so far was:\n{previous}\n\n\
         What is known about their device:\n{known}\n\n\
         What the user added:\n{added}\n\n\
         Answer in {tongue}, briefly, in EXACTLY the same sections, each keyword in \
         English:\n\
         {CAUSE} {WHY_NOT} {RESTS_ON} {NEXT} {IF_WRONG}\n\n\
         What the user added may contradict what you said. If it does, say so and \
         change the cause rather than defending it. If you are still missing \
         something, write {ABSTAIN} and exactly what.\n\n\
         Write everything except the section keywords in {tongue}.",
        tongue = language_name(lang),
        known = serde_json::to_string_pretty(facts).unwrap_or_default()
    );
    match cfg.provider.as_str() {
        "omarchy_agent" => crate::omarchy::ask(&cfg.model, &p).await,
        "anthropic" => anthropic(cfg, &p).await,
        _ => openai_compatible(cfg, &p).await,
    }
}

// ------------------------------------------------------------- the glossary
//
// A published project may name its own terms (`glossary.keep`, `SV107`), and
// they reach the reader as written. How is the part that was measured, against
// `gemma3:4b` and `qwen2.5:7b` on Ollama with engram's own sentences:
//
// * The prompt alone: *brain* became *cerveau* in every French run and
//   *Gehirn* in some German ones.
// * Telling the model to keep the term: six of six in one measurement — and
//   *Gehirn* every time through this client's prompt, whose only differences
//   were the order of two keys and the indentation of the JSON. Phrasing a
//   small model obeys by luck is not a mechanism.
// * Hiding the term: every occurrence replaced by a placeholder before the text
//   is sent, and put back after. Placeholders survived 24 of 24 on both models,
//   in German, French and Spanish, and not one run wrote an organ. The model
//   cannot translate a word it is never shown.
//
// What the model does to the sentence around a placeholder is still its own —
// an article, a case ending — and a placeholder it drops is a term it lost,
// which the count below finds and the window says.

/// `[[0]]`, or `@@0@@` where a text already uses double brackets.
const PLACEHOLDERS: [(&str, &str); 2] = [("[[", "]]"), ("@@", "@@")];

/// Whether `term` starts at `at` in `chars`: case-insensitive, and standing on
/// its own where its own edge is a letter or digit — "brain" in "my.brain"
/// counts, "brain" in "brainstorm" does not, and ".brain" needs no boundary
/// before its dot. Case-insensitive because German writes a noun capitalised.
fn term_at(chars: &[char], at: usize, term: &[char]) -> bool {
    if term.is_empty() || at + term.len() > chars.len() {
        return false;
    }
    let same = chars[at..at + term.len()].iter().zip(term)
        .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()));
    let open = |c: Option<&char>| c.map_or(true, |c| !c.is_alphanumeric());
    same
        && (!term[0].is_alphanumeric() || at == 0 || open(chars.get(at - 1)))
        && (!term[term.len() - 1].is_alphanumeric() || open(chars.get(at + term.len())))
}

/// Every place a kept term stands in `text`, longest term first at each
/// position, as (start, length) in chars.
fn spans(text: &[char], keep: &[Vec<char>]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        match keep.iter().find(|t| term_at(text, i, t)) {
            Some(t) => { out.push((i, t.len())); i += t.len(); }
            None => i += 1,
        }
    }
    out
}

fn by_length(keep: &[String]) -> Vec<Vec<char>> {
    let mut terms: Vec<Vec<char>> = keep.iter().map(|t| t.chars().collect()).collect();
    terms.sort_by(|a, b| b.len().cmp(&a.len()));
    terms
}

/// How often any of `keep` stands in `text`.
fn occurrences(text: &str, term: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    spans(&chars, &by_length(&[term.to_string()])).len()
}

fn strings(v: &Value) -> Vec<(String, String)> {
    v.as_object().map(|m| m.iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect()).unwrap_or_default()
}

/// The code in a Markdown text, as byte ranges: fenced blocks, indented blocks
/// and inline spans.
///
/// **Code is not prose, and a small model does not always know that.** Asked to
/// keep "commands and code unchanged", `gemma3:4b` rewrote engram's indented
/// `engram reindex my.brain` as `reindexer mon.brain` in every French and
/// Spanish run of six, while keeping the inline spans. A person who copies a
/// translated command runs something that does not exist. So code is hidden
/// the same way a project's own terms are — the model is never shown it.
///
/// Deliberately plain: a fence is a line starting with ``` or ~~~ (up to three
/// spaces in) until the same marker; an indented block is lines of four spaces
/// or a tab after a blank line; an inline span is a run of backticks to the
/// next run of the same length, and may cross a line break but not a blank
/// line. Markdown has more corners than that, and a corner missed here only
/// means that piece is translated as it was before.
fn code_spans(text: &str) -> Vec<(usize, usize)> {
    let mut lines: Vec<(usize, usize)> = Vec::new(); // (start, end without newline)
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' {
            lines.push((start, i));
            start = i + 1;
        }
    }
    lines.push((start, text.len()));

    let fence = |line: &str| {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() > 3 { return None; }
        ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m))
    };
    let indented = |line: &str| (line.starts_with("    ") || line.starts_with('\t')) && !line.trim().is_empty();

    let mut blocks = Vec::new();
    let mut i = 0;
    let mut prev_blank = true;
    while i < lines.len() {
        let (s, e) = lines[i];
        let line = &text[s..e];
        if let Some(marker) = fence(line) {
            let mut j = i + 1;
            while j < lines.len() && !text[lines[j].0..lines[j].1].trim_start().starts_with(marker) {
                j += 1;
            }
            let last = j.min(lines.len() - 1);
            blocks.push((s, lines[last].1));
            i = last + 1;
            prev_blank = false;
            continue;
        }
        if prev_blank && indented(line) {
            let mut j = i;
            while j + 1 < lines.len() && indented(&text[lines[j + 1].0..lines[j + 1].1]) {
                j += 1;
            }
            blocks.push((s, lines[j].1));
            i = j + 1;
            prev_blank = false;
            continue;
        }
        prev_blank = line.trim().is_empty();
        i += 1;
    }

    // Inline spans, in the prose between blocks.
    let mut out = Vec::new();
    let mut from = 0;
    for &(bs, be) in blocks.iter().chain(std::iter::once(&(text.len(), text.len()))) {
        let prose = &text[from..bs];
        let bytes = prose.as_bytes();
        let mut k = 0;
        while k < bytes.len() {
            if bytes[k] != b'`' { k += 1; continue; }
            let run = bytes[k..].iter().take_while(|&&b| b == b'`').count();
            let mut m = k + run;
            let mut found = None;
            while m < bytes.len() {
                // Bytes, not a slice of the string: `m` steps one byte at a time
                // and may sit inside a multi-byte character.
                if bytes[m..].starts_with(b"\n\n") { break; }
                if bytes[m] == b'`' {
                    let r = bytes[m..].iter().take_while(|&&b| b == b'`').count();
                    if r == run { found = Some(m + r); break; }
                    m += r;
                } else {
                    m += 1;
                }
            }
            match found {
                Some(end) => { out.push((from + k, from + end)); k = end; }
                None => k += run,
            }
        }
        if bs < text.len() { out.push((bs, be)); }
        from = be;
    }
    out.sort();
    out
}

/// Texts with every kept term and every piece of code hidden, and what each
/// placeholder stands for — the original characters, so a capital a sentence
/// gave the term comes back, and a command comes back byte for byte.
pub struct Shielded {
    pub texts: Value,
    originals: Vec<String>,
    /// Which placeholders stand for code, so one the model dropped can be named.
    code: Vec<usize>,
    form: (&'static str, &'static str),
}

pub fn shield(texts: &Value, keep: &[String]) -> Shielded {
    let all = strings(texts);
    let form = PLACEHOLDERS.iter().copied()
        .find(|(open, _)| !all.iter().any(|(_, s)| s.contains(open)))
        .unwrap_or(PLACEHOLDERS[1]);
    let terms = by_length(keep);
    let mut originals: Vec<String> = Vec::new();
    let mut code = Vec::new();
    let mut out = texts.clone();
    if let Some(map) = out.as_object_mut() {
        for (k, s) in all {
            let mut shielded = String::new();
            let mut hide = |shielded: &mut String, original: &str, is_code: bool| {
                if is_code { code.push(originals.len()); }
                shielded.push_str(&format!("{}{}{}", form.0, originals.len(), form.1));
                originals.push(original.to_string());
            };
            // Prose between pieces of code has its terms hidden; code is hidden whole.
            let mut at = 0;
            for (cs, ce) in code_spans(&s).into_iter().chain(std::iter::once((s.len(), s.len()))) {
                let chars: Vec<char> = s[at..cs].chars().collect();
                let mut c = 0;
                for (start, len) in spans(&chars, &terms) {
                    shielded.extend(&chars[c..start]);
                    let original: String = chars[start..start + len].iter().collect();
                    hide(&mut shielded, &original, false);
                    c = start + len;
                }
                shielded.extend(&chars[c..]);
                if cs < s.len() {
                    hide(&mut shielded, &s[cs..ce], true);
                }
                at = ce;
            }
            map.insert(k, Value::String(shielded));
        }
    }
    Shielded { texts: out, originals, code, form }
}

/// The code the model's answer no longer carries a placeholder for — named, so
/// the window can say which command the translation lost.
pub fn lost_code(translated: &Value, s: &Shielded) -> Vec<String> {
    let all: String = strings(translated).into_iter().map(|(_, v)| v).collect::<Vec<_>>().join("\n");
    s.code.iter()
        .filter(|&&i| !all.contains(&format!("{}{}{}", s.form.0, i, s.form.1)))
        .map(|&i| {
            let one_line = s.originals[i].trim().trim_matches('`').lines().next().unwrap_or("").trim().to_string();
            let short: String = one_line.chars().take(48).collect();
            if short.len() < one_line.len() { format!("{short}…") } else { short }
        })
        .collect()
}

/// The translation with every placeholder the model kept put back. One it
/// dropped stays dropped, and `lost_terms` finds it.
pub fn unshield(translated: &Value, s: &Shielded) -> Value {
    let mut out = translated.clone();
    if let Some(map) = out.as_object_mut() {
        for (_, v) in map.iter_mut() {
            if let Some(text) = v.as_str() {
                let mut restored = text.to_string();
                // Highest first, so `[[1]]` is never read inside `[[12]]`.
                for (i, original) in s.originals.iter().enumerate().rev() {
                    restored = restored.replace(&format!("{}{}{}", s.form.0, i, s.form.1), original);
                }
                *v = Value::String(restored);
            }
        }
    }
    out
}

/// A term that is in a source text and not, as often, in its translation —
/// per text, so a model that kept it in one place and lost it in another is
/// still caught. What makes the glossary a check rather than a hope.
pub fn lost_terms(source: &Value, translated: &Value, keep: &[String]) -> Vec<String> {
    let out = translated.as_object();
    keep.iter()
        .filter(|term| strings(source).iter().any(|(k, s)| {
            let theirs = out.and_then(|o| o.get(k)).and_then(|v| v.as_str()).unwrap_or("");
            occurrences(theirs, term) < occurrences(s, term)
        }))
        .cloned()
        .collect()
}

/// The translation prompt, over texts already shielded. It names no term — the
/// model never sees one — and asks only that placeholders be kept, and only
/// when there are any, so a project without a glossary gets the prompt it had.
pub fn translate_prompt(texts: &Value, to: &str, placeholder: Option<&str>) -> String {
    let keep = placeholder
        .map(|p| format!(" Keep every placeholder like {p} exactly as it is."))
        .unwrap_or_default();
    format!(
        "Translate the values of this JSON object into the language with the code '{to}'. \
         Every value is text a person reads — short labels as much as sentences — so \
         translate each one. Keep the keys exactly as they are, and inside the text keep \
         numbers, versions, file names, commands, code and product names unchanged.{keep} \
         Answer with the JSON object only.\n\n{}",
        serde_json::to_string_pretty(texts).unwrap_or_default()
    )
}

/// Translate vendor text into the user's language, locally.
///
/// Only reached when the vendor does not serve that language. The result is
/// marked as machine-translated and the original stays one click away, because
/// a consent decision must never rest on a translation nobody can check.
///
/// `keep` is the project's glossary. What comes back is the translated object
/// and the terms it lost — the second is empty when there was no glossary.
pub async fn translate(cfg: &Config, texts: &Value, to: &str, keep: &[String])
    -> Result<Value, String> {
    if !cfg.configured() {
        return Err(m!("no_model"));
    }
    let shielded = shield(texts, keep);
    let example = format!("{}0{}", shielded.form.0, shielded.form.1);
    let p = translate_prompt(&shielded.texts, to,
                             (!shielded.originals.is_empty()).then_some(example.as_str()));
    let raw = match cfg.provider.as_str() {
        // The desktop's agent answers the prompt itself, with every tool
        // denied and from an empty directory — see `omarchy.rs` for what was
        // measured and which two switches look like a fence and are not.
        "omarchy_agent" => crate::omarchy::ask(&cfg.model, &p).await?,
        "anthropic" => anthropic(cfg, &p).await?,
        _ => openai_compatible(cfg, &p).await?,
    };
    let start = raw.find('{').ok_or_else(|| m!("translation_no_json"))?;
    let end = raw.rfind('}').ok_or_else(|| m!("translation_no_json"))? + 1;
    let out: Value = serde_json::from_str(&raw[start..end])
        .map_err(|e| m!("translation_unreadable", e = e))?;
    let dropped = lost_code(&out, &shielded);
    let out = unshield(&out, &shielded);
    let mut lost = lost_terms(texts, &out, keep);
    lost.extend(dropped);
    Ok(json!({ "texts": out, "lost": lost }))
}

#[cfg(test)]
mod glossary_tests {
    use super::*;

    fn keep() -> Vec<String> {
        vec!["brain".into(), ".brain".into(), "debate".into()]
    }

    /// LG8: the model is never shown a kept term. Every occurrence is a
    /// placeholder on the way out and the original characters on the way back —
    /// a capital included, `.brain` before `brain`, and "brainstorm" untouched —
    /// and a project without a glossary sends exactly the text and prompt it did.
    #[test]
    fn the_projects_terms_are_hidden_from_the_model_and_put_back() {
        let texts = json!({"w0": "Brain files: a brain in my.brain, not a brainstorm.", "p0": "Which version?"});
        let s = shield(&texts, &keep());
        assert_eq!(s.texts["w0"], "[[0]] files: a [[1]] in my[[2]], not a brainstorm.");
        let p = translate_prompt(&s.texts, "fr", Some("[[0]]"));
        assert_eq!(occurrences(&p, "brain"), 0, "a kept term reached the model: {p}");
        assert!(p.contains("Keep every placeholder like [[0]] exactly as it is."), "{p}");

        let from_model = json!({"w0": "Fichiers [[0]] : un [[1]] dans my[[2]], pas un brainstorming.", "p0": "Quelle version ?"});
        let back = unshield(&from_model, &s);
        assert_eq!(back["w0"], "Fichiers Brain : un brain dans my.brain, pas un brainstorming.");
        assert!(lost_terms(&texts, &back, &keep()).is_empty(), "{back}");

        let plain = shield(&texts, &[]);
        assert_eq!(plain.texts, texts, "a project without a glossary had its text changed");
        let bare = translate_prompt(&plain.texts, "fr", None);
        assert!(!bare.contains("placeholder"), "a project without a glossary got a different prompt");

        // A text that already uses double brackets gets the other form, so a
        // placeholder is never confused with the project's own markup.
        let wiki = shield(&json!({"text": "See [[Setup]] before the brain."}), &keep());
        assert_eq!(wiki.texts["text"], "See [[Setup]] before the @@0@@.");
    }

    /// LG9: code in an answer — a fenced block, an indented command, an inline
    /// span that wraps a line — is hidden whole and comes back byte for byte,
    /// while the prose around it is still translated and its terms still kept.
    /// A command the model dropped is named, not lost silently.
    #[test]
    fn the_code_in_an_answer_is_hidden_from_the_model_and_put_back() {
        let text = "Semantic search on a brain — ünïcode — returns nothing.\n\n\
                    \x20   engram reindex my.brain\n\
                    \x20   engram serve my.brain\n\n\
                    Back up first: `cp my.brain\n  my.brain.bak`. Or:\n\n\
                    ```sh\nengram search --bm25 \"x\"\n```\n\
                    Done.";
        let texts = json!({"text": text, "w0": "Run `engram --version` to see which brain you have"});
        let s = shield(&texts, &keep());
        let shown = s.texts["text"].as_str().unwrap();
        for code in ["engram reindex", "engram serve", "cp my.brain", "--bm25"] {
            assert!(!shown.contains(code), "code reached the model: {code} in {shown}");
        }
        assert!(shown.contains("Semantic search"), "prose was hidden as if it were code: {shown}");
        assert_eq!(occurrences(shown, "brain"), 0, "a kept term in prose reached the model: {shown}");
        assert!(!s.texts["w0"].as_str().unwrap().contains("--version"), "{}", s.texts["w0"]);

        // A model that kept every placeholder gets every byte of code back.
        let back = unshield(&s.texts, &s);
        assert_eq!(back, texts, "shielding and unshielding changed the text");
        assert!(lost_code(&s.texts, &s).is_empty());

        // One that dropped the indented block is caught, and the command named.
        let placeholder = |original: &str| {
            let i = s.originals.iter().position(|o| o.contains(original)).unwrap();
            format!("{}{}{}", s.form.0, i, s.form.1)
        };
        let dropped = json!({
            "text": shown.replace(&placeholder("engram reindex"), "reindexer mon.brain"),
            "w0": s.texts["w0"]});
        assert_eq!(lost_code(&dropped, &s), vec!["engram reindex my.brain".to_string()]);

        // Without code or a glossary nothing changes, as before.
        let plain = json!({"p0": "Which version?"});
        assert_eq!(shield(&plain, &[]).texts, plain);
    }

    /// LG8: a term the translation dropped is found, per text and by count —
    /// "Brain" capitalised is kept, *cerveau* is not, and one of two
    /// occurrences gone is still a loss.
    #[test]
    fn a_term_the_translation_lost_is_found() {
        let src = json!({"text": "Build the brain elsewhere and copy my.brain across.", "p0": "Which version?"});
        let kept = json!({"text": "Erstelle das Brain woanders und kopiere my.brain hinüber.", "p0": "Welche Version?"});
        assert!(lost_terms(&src, &kept, &keep()).is_empty(), "a kept, capitalised term was called lost");
        let organ = json!({"text": "Construisez le cerveau ailleurs et copiez my.brain.", "p0": "Quelle version ?"});
        assert_eq!(lost_terms(&src, &organ, &keep()), vec!["brain".to_string()]);
        // A placeholder the model dropped is a term lost, not a silent gap.
        let s = shield(&src, &keep());
        let dropped = unshield(&json!({"text": "Construisez-le ailleurs et copiez my[[1]].", "p0": "?"}), &s);
        assert_eq!(lost_terms(&src, &dropped, &keep()), vec!["brain".to_string()]);
        assert!(lost_terms(&src, &json!({"p0": "x"}), &keep()).contains(&"brain".to_string()),
                "a text the model left out entirely did not count as losing its terms");
        assert_eq!(occurrences("brainstorm a brain", "brain"), 1, "a term inside a longer word counted");
    }

    /// The glossary against a real model. Not run by the suite, because it
    /// needs one: `PODSHL_LIVE_MODEL=gemma3:4b cargo test live_ -- --ignored`
    /// with Ollama on its default port. It runs the client's own prompt and
    /// its own check, which is the part a mock cannot say anything about.
    #[tokio::test]
    #[ignore]
    async fn live_a_small_model_keeps_the_projects_terms() {
        let Ok(model) = std::env::var("PODSHL_LIVE_MODEL") else { return };
        let cfg = Config { provider: "ollama".into(), model, endpoint: String::new(),
                           model_class: "local_small".into(), uses_key: false };
        let texts = json!({
            "w0": "The .brain format and the embedding defaults changed between releases, \
                   and a brain written by one is not always read the same way by the next",
            "text": "Semantic search returning nothing on a brain that used to answer is almost \
                     always an embedding model change. Build the brain on a machine with a GPU \
                     and copy the file.\n\n    engram reindex my.brain\n\nBack up first: \
                     `cp my.brain my.brain.bak`."});
        for to in ["de", "fr", "es"] {
            let bare = translate(&cfg, &texts, to, &[]).await.unwrap();
            let lost_bare = lost_terms(&texts, &bare["texts"], &["brain".into()]);
            let kept = translate(&cfg, &texts, to, &["brain".into()]).await.unwrap();
            eprintln!("{to}: without a glossary lost {lost_bare:?}; with one lost {} — {}",
                      kept["lost"], kept["texts"]["text"]);
            assert_eq!(kept["lost"], json!([]), "{to}: the model dropped a kept term: {kept}");
        }
    }
}

pub async fn solve(cfg: &Config, problem: &str, facts: &Value, lang: &str,
                   typed: &[String], context: &Value) -> Result<Answer, String> {
    if !cfg.configured() {
        return Err(m!("no_model"));
    }
    let p = prompt(problem, facts, lang, context);
    let raw = match cfg.provider.as_str() {
        // The desktop's agent answers the prompt itself, with every tool
        // denied and from an empty directory — see `omarchy.rs` for what was
        // measured and which two switches look like a fence and are not.
        "omarchy_agent" => crate::omarchy::ask(&cfg.model, &p).await?,
        "anthropic" => anthropic(cfg, &p).await?,
        _ => openai_compatible(cfg, &p).await?,
    };
    Ok(parse_answer(&raw, facts, typed))
}

/// Anthropic Messages API. `thinking` and `output_config` are deliberately
/// omitted: the supported values differ per model, and the user picks the
/// model. Omitting them is valid on every current model, where a wrong
/// explicit value would be a 400 the user cannot diagnose.
async fn anthropic(cfg: &Config, prompt: &str) -> Result<String, String> {
    let key = get_key("anthropic").ok_or_else(|| m!("no_api_key"))?;
    let body = json!({
        "model": cfg.model,
        "max_tokens": 2048,
        "messages": [{ "role": "user", "content": prompt }]
    });
    // The shared client, with the one override the model calls need: a
    // completion legitimately takes minutes, and thirty seconds is the bound
    // for an answer about a machine rather than for a model writing prose.
    let resp = crate::http::client()
        .post(format!("{}/v1/messages", cfg.base()))
        .header("x-api-key", key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .timeout(std::time::Duration::from_secs(180))
        .send().await.map_err(|e| m!("unreachable", e = e))?;
    let v: Value = crate::http::json_capped(resp, crate::http::MAX_BODY).await?;

    if let Some(err) = v.get("error") {
        return Err(m!("provider_says",
            e = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error")));
    }
    // A refusal is a valid response, not a transport failure — say so plainly.
    if v.get("stop_reason").and_then(|s| s.as_str()) == Some("refusal") {
        return Err(m!("model_refused"));
    }
    v.get("content").and_then(|c| c.as_array())
        .and_then(|blocks| blocks.iter()
            .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .and_then(|b| b.get("text")).and_then(|t| t.as_str()))
        .map(|s| s.trim().to_string())
        .ok_or_else(|| m!("unexpected_answer", v = v))
}

/// OpenAI-compatible chat completions — OpenAI, Mistral, Groq, vLLM, LM Studio
/// and Ollama all serve this shape.
async fn openai_compatible(cfg: &Config, prompt: &str) -> Result<String, String> {
    let mut req = crate::http::client()
        .post(format!("{}{}/chat/completions", cfg.base(), api_prefix(&cfg.provider)))
        .timeout(std::time::Duration::from_secs(180));
    if cfg.uses_key {
        let key = get_key(&cfg.provider).ok_or_else(|| m!("no_api_key"))?;
        req = req.bearer_auth(key);
    }
    let resp = req
        .json(&json!({
            "model": cfg.model,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": false,
            "temperature": 0.2
        }))
        .send().await.map_err(|e| m!("unreachable", e = e))?;
    let v: Value = crate::http::json_capped(resp, crate::http::MAX_BODY).await?;

    if let Some(err) = v.get("error") {
        return Err(m!("provider_says",
            e = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error")));
    }
    v.pointer("/choices/0/message/content").and_then(|c| c.as_str())
        .map(|s| s.trim().to_string())
        .ok_or_else(|| m!("unexpected_answer", v = v))
}

// ---------------------------------------------------------------- providers

/// Preset providers. A table rather than branches, because the space is open:
/// aggregators like OpenRouter front many models behind one key, and new
/// providers appear faster than a client can ship. `Custom` covers the rest.
///
/// `protocol` is the load-bearing column. Anthropic speaks its own Messages
/// API; everything else here serves the OpenAI chat-completions shape.
pub const PROVIDERS: &[(&str, &str, &str, &str, bool, &str)] = &[
    // id, display, base, protocol, needs_key, model hint
    ("anthropic", "Anthropic", "https://api.anthropic.com", "anthropic", true,
     "claude-opus-5"),
    ("openai", "OpenAI", "https://api.openai.com", "openai", true, ""),
    ("openrouter", "OpenRouter", "https://openrouter.ai/api", "openai", true,
     "anthropic/claude-opus-5"),
    ("mistral", "Mistral", "https://api.mistral.ai", "openai", true, ""),
    ("groq", "Groq", "https://api.groq.com/openai", "openai", true, ""),
    // The free layer, which is the one that matters for open source: a vendor
    // that publishes needs no model at all, and a user who falls back to one
    // should not have to pay to be able to. `openai_flat` is not a different
    // protocol — it is the same bodies without the `/v1` segment, which these
    // two put in the base or omit entirely.
    ("github", "GitHub Models", "https://models.github.ai/inference",
     "openai_flat", true, ""),
    ("google", "Google AI Studio",
     "https://generativelanguage.googleapis.com/v1beta/openai", "openai_flat", true, ""),
    ("cerebras", "Cerebras", "https://api.cerebras.ai", "openai", true, ""),
    ("together", "Together AI", "https://api.together.xyz", "openai", true, ""),
    ("deepseek", "DeepSeek", "https://api.deepseek.com", "openai", true, ""),
    ("ollama", "Ollama", "http://localhost:11434", "openai", false, ""),
    ("lmstudio", "LM Studio", "http://localhost:1234", "openai", false, ""),
    ("llamacpp", "llama.cpp", "http://localhost:8080", "openai", false, ""),
    // Not an endpoint and not a key: the name of the agent this desktop already
    // has. Offered only where Omarchy names one, it is installed, and somebody
    // has measured how to call it without its tools.
    ("omarchy_agent", "Omarchy default agent", "", "agent", false, ""),
    ("custom", "", "", "openai", true, ""),
];

/// Where the provider puts its OpenAI-compatible paths. Most serve them under
/// `/v1`; GitHub Models serves them at the inference root, and Google's
/// compatibility layer carries its version in the base already. Getting this
/// wrong produces a 404 that reads like a bad key, so it is a table rather
/// than something each call site guesses.
fn api_prefix(provider: &str) -> &'static str {
    match preset(provider).map(|p| p.3) {
        Some("openai_flat") => "",
        _ => "/v1",
    }
}

/// GitHub Models serves its catalogue from a different subtree than the one it
/// answers completions on, so the generic `{base}/models` does not reach it.
fn models_url(cfg: &Config) -> String {
    if cfg.provider == "github" && cfg.endpoint.is_empty() {
        return "https://models.github.ai/catalog/models".into();
    }
    format!("{}{}/models", cfg.base(), api_prefix(&cfg.provider))
}

pub fn preset(id: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str, bool, &'static str)> {
    PROVIDERS.iter().find(|p| p.0 == id)
}

/// What the picker shows: the provider's own name, and what matters about it
/// to the person choosing — free, local, or many models behind one key.
fn display_name(id: &str, name: &str, base: &str, needs_key: bool) -> String {
    match id {
        "custom" => m!("provider_custom"),
        "openrouter" => m!("provider_many", name = name),
        "mistral" | "groq" | "github" | "google" | "cerebras" => m!("provider_free", name = name),
        _ if !needs_key && base.contains("localhost") => m!("provider_local", name = name),
        _ => name.to_string(),
    }
}

pub fn providers_json() -> Value {
    json!(PROVIDERS.iter().map(|(id, name, base, proto, key, hint)| json!({
        "id": id, "name": display_name(id, name, base, *key), "base": base, "protocol": proto,
        "needs_key": key, "hint": hint,
        // Local providers are the only ones where the question truly stays on
        // the device; that distinction has to reach the user.
        "local": !*key && base.contains("localhost")
    })).collect::<Vec<_>>())
}

/// Ask the provider which models it serves. Most OpenAI-compatible endpoints
/// and Anthropic both expose this, which beats making the user paste an
/// identifier they have to look up elsewhere.
pub async fn list_models(cfg: &Config) -> Result<Vec<String>, String> {
    let url = models_url(cfg);
    let mut req = crate::http::client().get(&url).timeout(std::time::Duration::from_secs(30));
    if cfg.provider == "anthropic" {
        let key = get_key("anthropic").ok_or_else(|| m!("no_api_key"))?;
        req = req.header("x-api-key", key).header("anthropic-version", ANTHROPIC_VERSION);
    } else if cfg.uses_key {
        if let Some(k) = get_key(&cfg.provider) {
            req = req.bearer_auth(k);
        }
    }
    let resp = req.send().await.map_err(|e| m!("unreachable", e = e))?;
    // A catalogue of model names, not a catalogue of projects: the ordinary cap.
    let v: Value = crate::http::json_capped(resp, crate::http::MAX_BODY).await?;
    if let Some(err) = v.get("error") {
        return Err(m!("provider_says",
            e = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error")));
    }
    // `{"data": [...]}` is the common shape; GitHub's catalogue answers with a
    // bare array. Both are the same list, and refusing one of them would look
    // to the user like the provider is unreachable.
    let mut out: Vec<String> = v.get("data").and_then(|d| d.as_array()).or_else(|| v.as_array())
        .map(|a| a.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
        .unwrap_or_default();
    out.sort();
    if out.is_empty() { Err(m!("no_models")) } else { Ok(out) }
}

/// Cheap reachability check — does the endpoint answer and is the credential
/// accepted? Costs no tokens, so it can run at startup.
///
/// Reachable is not the same as working, and the UI must not conflate them:
/// a stored configuration that nothing answers is exactly how a demo becomes a
/// lie. This is the weaker of the two checks and says so.
pub async fn probe(cfg: &Config) -> Result<usize, String> {
    if !cfg.configured() {
        return Err(m!("not_configured"));
    }
    // An agent has no model catalogue to list — it *is* the one thing on offer.
    // Asking whether it is installed is the same question `list_models` answers
    // for an endpoint: is there something here to talk to.
    if cfg.provider == "omarchy_agent" {
        let h = crate::omarchy::headless(&cfg.model, "")
            .ok_or_else(|| format!("no measured way to call {:?} without its tools", cfg.model))?;
        return if crate::omarchy::installed(h.program) {
            Ok(1)
        } else {
            Err(format!("{} is not installed", h.program))
        };
    }
    list_models(cfg).await.map(|m| m.len())
}

/// The real check: one short completion through the exact path a fallback
/// would take. Costs tokens on a paid provider, so it is never automatic —
/// the user asks for it.
pub async fn test(cfg: &Config) -> Result<(String, u128), String> {
    let started = std::time::Instant::now();
    let answer = solve(cfg, "Answer with the single word: ready.", &json!({}), "en", &[], &Value::Null).await?;
    Ok((answer.raw.chars().take(120).collect(), started.elapsed().as_millis()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: &str) -> Config {
        Config { provider: provider.into(), model: "m".into(), endpoint: String::new(),
                 model_class: "cloud".into(), uses_key: true }
    }

    /// Every preset has to compose the URL its provider actually serves. A
    /// wrong path here answers 404, which the settings screen shows as "not
    /// reachable" — indistinguishable from a bad key, and the user would go
    /// looking for the wrong thing. Checked without a credential, because the
    /// composition is the part that can be wrong for free.
    #[test]
    fn every_preset_composes_the_path_its_provider_serves() {
        for (provider, chat, models) in [
            ("openai", "https://api.openai.com/v1/chat/completions",
                       "https://api.openai.com/v1/models"),
            ("groq", "https://api.groq.com/openai/v1/chat/completions",
                     "https://api.groq.com/openai/v1/models"),
            ("cerebras", "https://api.cerebras.ai/v1/chat/completions",
                         "https://api.cerebras.ai/v1/models"),
            // Version already in the base, so no second `/v1`.
            ("google", "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
                       "https://generativelanguage.googleapis.com/v1beta/openai/models"),
            // Completions at the inference root; the catalogue is elsewhere.
            ("github", "https://models.github.ai/inference/chat/completions",
                       "https://models.github.ai/catalog/models"),
        ] {
            let c = cfg(provider);
            let base = PROVIDERS.iter().find(|p| p.0 == provider).unwrap().2;
            assert_eq!(format!("{}{}/chat/completions", base, api_prefix(provider)), chat,
                       "{provider}: completions path");
            let mut c2 = c.clone();
            c2.endpoint = String::new();
            // `base()` falls back to OpenAI for unknown ids, so drive it from
            // the table the settings screen actually offers.
            c2.endpoint = base.to_string();
            let got = if provider == "github" { models_url(&cfg(provider)) } else { models_url(&c2) };
            assert_eq!(got, models, "{provider}: model listing path");
        }
    }

    #[test]
    fn the_desktop_agent_is_the_model_until_somebody_chooses() {
        let agent = || Some(("claude".to_string(), true));

        let fresh = resolve(None, agent());
        assert_eq!(fresh.provider, "omarchy_agent");
        assert_eq!(fresh.model, "claude");
        assert!(fresh.configured(), "a fresh Omarchy install still says No own model");
        assert!(fresh.is_cloud(), "the one measured agent sends the prompt away and must say so");
        assert_eq!(fresh.model_class, "cloud");
        assert!(!fresh.uses_key);

        let chosen = r#"{"provider":"ollama","model":"qwen3:4b","endpoint":"",
                         "model_class":"local_small","uses_key":false}"#;
        assert_eq!(resolve(Some(chosen), agent()).provider, "ollama",
                   "the desktop default overrode a saved choice");

        let none = r#"{"provider":"","model":"","endpoint":"","model_class":"","uses_key":false}"#;
        assert!(!resolve(Some(none), agent()).configured(),
                "a saved choice of no model was replaced by the desktop agent");

        assert!(!resolve(None, None).configured(),
                "a model appeared where the desktop names none");
    }

    /// The free layer is the answer to "an open-source project will not run a
    /// model", so it has to be present and it has to be reachable without a
    /// paid account. If a preset is dropped the docs stop being true.
    #[test]
    fn the_free_tier_providers_are_offered() {
        for id in ["github", "google", "cerebras", "groq", "mistral", "openrouter"] {
            assert!(preset(id).is_some(), "{id} is documented as a free option but not offered");
        }
    }
}
