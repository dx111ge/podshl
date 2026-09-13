//! Taking the person out of a piece of text before they are asked to send it.
//!
//! Free text is the one thing no generalisation policy can coarsen — a version
//! can be cut to `major.minor`, a log line cannot — and it is also the most
//! useful thing a maintainer can receive. The design already answers the
//! consent half: the user sees the exact words, can edit them, is told who
//! receives them, and the refusing button holds focus. This is the other half.
//! Nobody reliably spots their own user name in forty lines of a log, or the
//! address of their home router, or a session token three screens to the
//! right. So the obvious identifiers are replaced *before* the text is shown,
//! and the user is told what was replaced and how often.
//!
//! **This is a floor, not a guarantee.** It removes what has a recognisable
//! shape; it cannot know that a project name or a customer name in a log line
//! is identifying. That is why the result is still shown, still editable, and
//! still sent only on its own consent — this makes the default safer, it does
//! not replace the person reading it.
//!
//! Timestamps are among the things replaced, and not for tidiness: a report
//! carries no time on purpose, because a precise time relinks it to the run
//! that produced it. A log excerpt full of them would undo that in one paste.

use regex::{Captures, Regex};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// What leaves, at most. An excerpt is the lines around a failure, not a log.
pub const MAX_CHARS: usize = 8000;
pub const MAX_LINES: usize = 120;

struct Rule {
    kind: &'static str,
    re: Regex,
    /// `$n` references allowed; a rule that keeps part of the match (the key
    /// in `password=…`) says so here.
    with: &'static str,
}

fn rules() -> Vec<Rule> {
    let r = |kind, pat: &str, with| Rule { kind, re: Regex::new(pat).expect("redaction pattern"), with };
    vec![
        // Credentials first: a token inside a URL or after `password=` must go
        // whole, before any later rule nibbles a piece of it off as an address
        // or a number and leaves the rest standing.
        //
        // A PEM block before anything else. Its lines are base64, which the
        // token rule would take one line at a time and leave the armour
        // standing around the holes — and a key pasted into a log is a key.
        r("secret", r"(?s)-----BEGIN [A-Z ]+-----.*?-----END [A-Z ]+-----", "<key-block>"),
        r("jwt", r"\beyJ[A-Za-z0-9_-]{5,}\.eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}", "<jwt>"),
        r("credentials", r"(?i)\b([a-z][a-z0-9+.-]{1,15}://)[^\s/:@]+:[^\s/@]+@", "${1}<credentials>@"),
        // `Bearer <token>` before the `authorization:` rule. The other way
        // round, `Authorization: Bearer abc…` matched `authorization: Bearer`
        // as key and value — the word *Bearer* was redacted and the token
        // after it stood untouched.
        r("secret", r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}", "${1} <redacted>"),
        r("secret",
          r#"(?i)\b(password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key|client[_-]?secret|private[_-]?key|session[_-]?id|cookie|authorization)(["']?\s*[:=]\s*["']?)([^\s"',;&]+)"#,
          "${1}${2}<redacted>"),
        // Keys that announce themselves by prefix are recognised at twenty
        // characters, where a run of ordinary base64 needs forty: the prefix
        // is the identification, and a truncated `sk-…` in a log is still
        // most of a key.
        r("secret", r"\b(?:sk-|sk_live_|AKIA|ghp_|gho_|xox[abp]-|AIza)[A-Za-z0-9+/_-]{16,}", "<token>"),
        r("email", r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b", "<email>"),
        // A home directory names its owner, on every platform.
        r("user", r"(?i)\b([A-Z]:\\(?:Users|Documents and Settings)\\)[^\\/\s:*?<>|]+", "${1}<user>"),
        r("user", r"(/home/|/Users/)[^/\s:]+", "${1}<user>"),
        r("uuid", r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", "<uuid>"),
        r("mac", r"(?i)\b(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b", "<mac>"),
        // Times before addresses: `14:23:05` is also three groups of hex.
        r("time",
          r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(?::\d{2}(?:[.,]\d{1,9})?)?(?:Z|[+-]\d{2}:?\d{2})?\b",
          "<time>"),
        r("time", r"\b(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s{1,2}\d{1,2}\s\d{2}:\d{2}:\d{2}\b", "<time>"),
        r("time", r"\b\d{2}/\d{2}/\d{4}[ :]\d{2}:\d{2}(?::\d{2})?\b", "<time>"),
        r("time", r"\b\d{1,2}:\d{2}:\d{2}(?:[.,]\d{1,9})?\b", "<time>"),
        r("time", r"\b\d{4}-\d{2}-\d{2}\b", "<date>"),
        // Go's `log` and Gin write the date with slashes: `[GIN] 2026/09/11 - …`.
        r("time", r"\b\d{4}/\d{2}/\d{2}\b", "<date>"),
        // Tokens that are too long to be words. Hex first, then the base64url
        // alphabet — without `/`, so a long path is not mistaken for a key.
        r("token", r"(?i)\b[0-9a-f]{32,}\b", "<hex>"),
        r("token", r"\b[A-Za-z0-9+_-]{40,}={0,2}", "<token>"),
    ]
}

/// Base64 runs that contain a solidus.
///
/// The token rule above leaves `/` out of its alphabet so that a long path is
/// not mistaken for a key — and a key whose base64 happens to contain a `/`
/// was left standing for the same reason. A run is taken as a token here when
/// it does not have the shape of a path: it is a whole word, it does not begin
/// with `/`, `.` or `~`, it carries no `://`, no `\` and no `//`, and it is
/// long enough that no word in a log is that long by accident.
fn slashed_tokens(text: &str, counts: &mut BTreeMap<&'static str, usize>) -> String {
    let re = Regex::new(r#"(?:^|[\s"'=:,;(\[{])([A-Za-z0-9+_-][A-Za-z0-9+/_-]{39,}={0,2})(?:$|[\s"',;)\]}])"#)
        .expect("slashed token");
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for c in re.captures_iter(text) {
        let m = c.get(1).unwrap();
        let run = m.as_str();
        if !run.contains('/') || run.contains("//") || run.contains("://") {
            continue;
        }
        out.push_str(&text[last..m.start()]);
        out.push_str("<token>");
        last = m.end();
        *counts.entry("token").or_default() += 1;
    }
    out.push_str(&text[last..]);
    out
}

/// IPv4 addresses, except the ones that say nothing about anybody.
///
/// Loopback and the unspecified address are kept because they are diagnosis:
/// "listening on 127.0.0.1:11434" and "listening on 0.0.0.0:11434" are two
/// different problems. Every other address is replaced, private ranges included
/// — a home network layout is not nothing.
fn ipv4(text: &str, counts: &mut BTreeMap<&'static str, usize>) -> String {
    let re = Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").expect("ipv4");
    re.replace_all(text, |c: &Captures| {
        let octets: Vec<u32> = (1..=4).filter_map(|i| c[i].parse().ok()).collect();
        let whole = &c[0];
        if octets.len() != 4 || octets.iter().any(|o| *o > 255) {
            return whole.to_string();
        }
        if octets[0] == 127 || octets == [0, 0, 0, 0] {
            return whole.to_string();
        }
        *counts.entry("ip").or_default() += 1;
        "<ip>".to_string()
    })
    .to_string()
}

/// IPv6, conservatively. Colon-separated hex is also a time, a MAC, a
/// `file:line:column` and a Rust path — `std::io` contains `d::` — so a
/// candidate counts only if it stands on its own (no letter or digit touching
/// either end), carries a digit, and is either compressed with two groups or
/// has at least five. `::1` is loopback and stays.
fn ipv6(text: &str, counts: &mut BTreeMap<&'static str, usize>) -> String {
    let re = Regex::new(r"(?i)(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}").expect("ipv6");
    let touching = |c: Option<char>| c.map(|c| c.is_ascii_alphanumeric() || c == '_').unwrap_or(false);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in re.find_iter(text) {
        let whole = m.as_str();
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        let groups = whole.split(':').filter(|g| !g.is_empty()).count();
        let looks = (whole.contains("::") && groups >= 2) || groups >= 5;
        let is_address = looks
            && !touching(before)
            && !touching(after)
            && whole.chars().any(|ch| ch.is_ascii_digit())
            && whole != "::1";
        if is_address {
            out.push_str(&text[last..m.start()]);
            out.push_str("<ip>");
            last = m.end();
            *counts.entry("ip").or_default() += 1;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// This machine's own names — the account and the host — wherever they occur,
/// not only inside a path. Short names are left alone: replacing every "dx" or
/// "pc" in a log would destroy it and protect nobody.
fn own_names() -> Vec<(&'static str, String, &'static str)> {
    let mut out = Vec::new();
    for k in ["USERNAME", "USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(k) {
            if v.chars().count() >= 3 {
                out.push(("user", v, "<user>"));
            }
        }
    }
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string());
    if let Some(h) = host.filter(|h| h.chars().count() >= 3) {
        out.push(("host", h, "<host>"));
    }
    out
}

/// Replace what has a recognisable shape, and say what was replaced. The
/// machine's own account and host name are removed wherever they occur, not
/// only where they sit inside a path.
pub fn anonymise(text: &str) -> (String, BTreeMap<&'static str, usize>) {
    anonymise_with(text, &own_names())
}

fn anonymise_with(text: &str, names: &[(&'static str, String, &'static str)])
    -> (String, BTreeMap<&'static str, usize>) {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut out = text.replace("\r\n", "\n");

    for rule in rules() {
        let n = rule.re.find_iter(&out).count();
        if n > 0 {
            out = rule.re.replace_all(&out, rule.with).to_string();
            *counts.entry(rule.kind).or_default() += n;
        }
    }
    out = slashed_tokens(&out, &mut counts);
    out = ipv4(&out, &mut counts);
    out = ipv6(&out, &mut counts);

    // Literal names last, so a user name inside a path already replaced is not
    // counted twice, and case-insensitively, because Windows is.
    for (kind, name, with) in names {
        let re = match Regex::new(&format!(r"(?i)\b{}\b", regex::escape(name))) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let n = re.find_iter(&out).count();
        if n > 0 {
            out = re.replace_all(&out, *with).to_string();
            *counts.entry(kind).or_default() += n;
        }
    }
    (out, counts)
}

/// Every string inside a JSON value, anonymised in place; nothing else is
/// touched. This is the floor under every fact that leaves — a reading is
/// whatever a program printed, and a program that prints its build path
/// prints the account it was built under.
pub fn anonymise_value(v: &Value) -> Value {
    anonymise_value_counting(v, &mut BTreeMap::new())
}

/// The same walk, saying what it replaced.
///
/// The consent panel needs this and not only the result: a person shown a
/// value has to be told that something was taken out of it, or the panel is
/// quietly different from what they typed and they have no way to know.
pub fn anonymise_value_counting(v: &Value, counts: &mut BTreeMap<&'static str, usize>) -> Value {
    match v {
        Value::String(s) => {
            let (out, seen) = anonymise(s);
            for (kind, n) in seen {
                *counts.entry(kind).or_default() += n;
            }
            Value::String(out)
        }
        Value::Array(a) => Value::Array(
            a.iter().map(|x| anonymise_value_counting(x, counts)).collect(),
        ),
        Value::Object(o) => Value::Object(
            o.iter().map(|(k, x)| (k.clone(), anonymise_value_counting(x, counts))).collect(),
        ),
        other => other.clone(),
    }
}

/// Cut to what an excerpt may be: the last lines, then the last characters.
/// The end of a log is where the failure is.
pub fn bound(text: &str) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    let mut cut = lines.len() > MAX_LINES;
    let kept = if cut { &lines[lines.len() - MAX_LINES..] } else { &lines[..] };
    let mut s = kept.join("\n");
    if s.chars().count() > MAX_CHARS {
        cut = true;
        let skip = s.chars().count() - MAX_CHARS;
        s = s.chars().skip(skip).collect();
    }
    (s, cut)
}

/// What the window shows before the send consent: the text as it would
/// travel, and a count per kind of what was taken out.
pub fn preview(text: &str) -> Value {
    let (bounded, cut) = bound(text);
    let (clean, counts) = anonymise(&bounded);
    let mut replaced = Map::new();
    for (k, n) in counts {
        replaced.insert(k.to_string(), json!(n));
    }
    json!({ "text": clean, "replaced": replaced, "cut": cut,
            "max_lines": MAX_LINES, "max_chars": MAX_CHARS })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(s: &str) -> String {
        anonymise_with(s, &[("user", "jdoe".into(), "<user>"),
                            ("host", "WORKSTATION-7".into(), "<host>")]).0
    }

    /// LX7: the entry the window actually calls removes *this* machine's own
    /// account and host, found from the environment rather than handed in.
    ///
    /// Every other case here calls `anonymise_with` and supplies the names, so
    /// they prove the replacing and never the finding. `own_names()` — the half
    /// that reads `USERNAME`, `USER`, `LOGNAME`, `COMPUTERNAME`, `HOSTNAME` and
    /// `/etc/hostname` — had no test at all, and it is the half that decides
    /// whether a real person's account leaves their machine.
    ///
    /// Nothing is printed on failure but the count: a message naming the
    /// account would put it in the log of whoever ran the suite, which is the
    /// thing this exists to prevent.
    #[test]
    fn this_machines_own_names_are_found_and_removed() {
        let names = own_names();
        let user = names.iter().find(|(k, _, _)| *k == "user").map(|(_, v, _)| v.clone());
        let Some(user) = user else {
            // Said rather than skipped. A suite that quietly tests nothing
            // where the environment is thin is worse than one that is red.
            assert!(std::env::var("USERNAME").is_err() && std::env::var("USER").is_err()
                        && std::env::var("LOGNAME").is_err(),
                    "the environment names an account and `own_names` did not find it");
            return;
        };

        // Through `anonymise`, not `anonymise_with`: the public entry is what
        // `anonymise_text` calls, and the wiring is the thing under test.
        let text = format!("loaded C:\\Users\\{user}\\proj\\app.toml for {user}");
        let (out, counts) = anonymise(&text);
        assert!(!out.to_lowercase().contains(&user.to_lowercase()),
                "this machine's account name survived anonymising ({} occurrences replaced)",
                counts.get("user").copied().unwrap_or(0));
        assert_eq!(counts.get("user").copied().unwrap_or(0), 2,
                   "not every occurrence of the account name was counted");
        assert!(out.contains("<user>"), "the account name was removed without saying so");
        // The rest of the line is the diagnosis and must survive.
        assert!(out.contains("app.toml") && out.contains("proj"),
                "anonymising took the path apart: {out}");

        // And the host, where the machine has one worth hiding.
        if let Some((_, host, _)) = names.iter().find(|(k, _, _)| *k == "host") {
            let (out, _) = anonymise(&format!("Sep 11 14:23:06 {host} ollama[812]: ready"));
            assert!(!out.to_lowercase().contains(&host.to_lowercase()),
                    "this machine's host name survived anonymising");
        }
    }

    /// LX2: what has the shape of an identifier is gone before the user is
    /// asked, and each kind is counted so they are told what happened.
    #[test]
    fn identifiers_are_replaced_before_anything_is_offered() {
        let log = "\
2026-09-11T14:23:05.123Z INFO engram: loaded C:\\Users\\jdoe\\.engram\\data\\default.brain
Sep 11 14:23:06 WORKSTATION-7 ollama[812]: listening on 192.168.0.26:11434
14:23:07 connect http://admin:hunter2@10.0.0.5:3030/api failed
token=ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCD user=sven@example.org
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U
session 3f2b1c9e-8a7d-4e6f-9b0a-1c2d3e4f5a6b from fe80::1ff:fe23:4567:890a on aa:bb:cc:dd:ee:ff
/home/jdoe/projects/x.brain  still on 127.0.0.1:11434 and 0.0.0.0:3030
[GIN] 2026/09/11 - 09:14:22 | 404 | 412.3µs | 192.168.0.40 | POST \"/api/chat\"";
        let (out, counts) = anonymise_with(log, &[("user", "jdoe".into(), "<user>"),
                                                  ("host", "WORKSTATION-7".into(), "<host>")]);
        for gone in ["jdoe", "WORKSTATION-7", "192.168.0.26", "hunter2", "10.0.0.5",
                     "ghp_abcdefghij", "sven@example.org", "eyJhbGci", "3f2b1c9e",
                     "fe80::1ff", "aa:bb:cc", "14:23:05", "2026-09-11", "2026/09/11",
                     "09:14:22", "192.168.0.40"] {
            assert!(!out.contains(gone), "{gone:?} survived:\n{out}");
        }
        // Diagnosis survives: loopback, the unspecified address, ports, and
        // the words around them.
        for kept in ["127.0.0.1:11434", "0.0.0.0:3030", ":11434", "listening on",
                     "default.brain", "connect", "failed", "INFO engram"] {
            assert!(out.contains(kept), "{kept:?} was removed and it is diagnosis:\n{out}");
        }
        for kind in ["user", "host", "ip", "email", "secret", "credentials", "uuid", "mac", "time"] {
            assert!(counts.get(kind).copied().unwrap_or(0) > 0, "{kind} was not counted: {counts:?}");
        }
    }

    /// A version number is not an address, and a file position is not IPv6.
    /// Replacing those would take the diagnosis out along with the person.
    #[test]
    fn versions_and_positions_survive() {
        let s = "engram v1.2.2 on Python 3.12.14, driver 610.57.04, at src/main.rs:657:5 \
                 (build 10.0.26200) in std::io::Error::new, Cafe::Bad";
        assert_eq!(clean(s), s);
    }

    /// Short account names are left alone. Replacing every "pc" in a log would
    /// destroy it and protect nobody.
    #[test]
    fn a_short_name_is_not_hunted_through_the_text() {
        let (out, _) = anonymise_with("the pc restarted", &[("user", "pc".into(), "<user>")]);
        // `own_names` never offers a name this short; given one anyway, the
        // word boundary still keeps it from eating other words.
        assert!(out.contains("restarted"));
    }

    /// LX7: the shapes a key takes that the first rules missed. A bearer
    /// token after `Authorization:` was left standing because the header
    /// rule ran first and took the word *Bearer* as the value; a PEM block
    /// lost only its lines; base64 with a `/` in it was taken for a path; and
    /// a key that announces itself by prefix was too short for the token rule.
    #[test]
    fn keys_in_every_shape_are_taken_out() {
        let s = "Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ";
        let out = clean(s);
        assert!(!out.contains("abcdefghij"), "the bearer token survived: {out}");
        assert!(out.contains("Authorization"), "the header name is diagnosis: {out}");

        let pem = "loaded key\n-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7\nVJTUt9Us8cKj\n-----END PRIVATE KEY-----\ndone";
        let out = clean(pem);
        assert!(!out.contains("MIIEvQ") && !out.contains("BEGIN PRIVATE"), "{out}");
        assert!(out.starts_with("loaded key\n") && out.ends_with("\ndone"), "{out}");

        let slashed = "key=abcdefghijklmnop/qrstuvwxyz0123456789/ABCDEFGHIJKLMNOP== next";
        let out = clean(slashed);
        assert!(!out.contains("qrstuvwxyz0123456789"), "base64 with a solidus survived: {out}");
        // A path of the same length is a path, and stays.
        let path = "/home/<user>/projects/a-long-directory-name-for-a-project/src/lib/module.rs";
        assert_eq!(clean(path), path);
        let url = "https://example.org/a/very/long/path/that/goes/on/and/on/and/on/for/a/while/x";
        assert_eq!(clean(url), url);

        for short in ["sk-abcdefghijklmnopqrstuv", "AKIAIOSFODNN7EXAMPLEXY", "ghp_0123456789abcdefghij",
                      "xoxb-1234567890-abcdefghijkl", "AIzaSyA1234567890abcdefgh"] {
            let out = clean(&format!("using {short} now"));
            assert_eq!(out, "using <token> now", "a prefixed key survived: {out}");
        }
    }

    /// Every string in a value is anonymised and nothing else changes shape.
    #[test]
    fn a_value_is_anonymised_string_by_string() {
        let v = json!({"gpu.name": "RTX 2070", "path": "/home/jdoe/x", "n": 3,
                       "list": ["sven@example.org", true]});
        let out = anonymise_value(&v);
        assert_eq!(out["gpu.name"], "RTX 2070");
        assert_eq!(out["path"], "/home/<user>/x");
        assert_eq!(out["n"], 3);
        assert_eq!(out["list"][0], "<email>");
        assert_eq!(out["list"][1], true);
    }

    /// An excerpt is the end of a log, bounded in lines and in characters,
    /// and the preview says when it was cut.
    #[test]
    fn an_excerpt_is_bounded_and_says_so() {
        let long: String = (0..500).map(|i| format!("line {i}\n")).collect();
        let (s, cut) = bound(&long);
        assert!(cut);
        assert_eq!(s.lines().count(), MAX_LINES);
        assert!(s.ends_with("line 499"), "the end of the log is where the failure is");
        let p = preview(&long);
        assert_eq!(p["cut"], true);
    }
}
