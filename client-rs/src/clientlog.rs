//! What this client did, written down.
//!
//! There was no log. Every failure was therefore diagnosed by guessing: a
//! person described what they saw, and somebody reasoned backwards about which
//! of four possible causes it was — a client built without its operator, a
//! directory that would not verify, a route the client addressed wrongly, or an
//! operator that was answering every TLS handshake with an error. All four
//! happened on 2026-09-14, and each took hours that one line of output would
//! have ended.
//!
//! **One line per thing that happened, and the ones that matter are the ones
//! nobody can see from the window**: which operator this client was built for,
//! whether the published directory verified, and what each call to the operator
//! answered. Those three would have answered every question asked today.
//!
//! It is a file rather than stderr because the window is started from a desktop
//! entry, a bar icon or a menu row, and none of them keep stderr anywhere a
//! person can find it. `PODSHL_LOG=0` turns it off; `PODSHL_LOG=<path>` puts it
//! somewhere else.
//!
//! **Nothing here may carry what a report would not.** It runs on the machine
//! it describes and is read by its owner, but a log is the thing people paste
//! into an issue — so it goes through the same anonymiser, and a value longer
//! than a line is cut rather than wrapped.

use std::io::Write;
use std::path::PathBuf;

fn path() -> Option<PathBuf> {
    match std::env::var("PODSHL_LOG").as_deref() {
        Ok("0") | Ok("off") | Ok("") => None,
        Ok(p) => Some(PathBuf::from(p)),
        Err(_) => dirs::state_dir()
            .or_else(dirs::cache_dir)
            .map(|d| d.join("podshl").join("client.log")),
    }
}

/// One line, with a timestamp, anonymised.
pub fn line(what: &str) {
    let Some(p) = path() else { return };
    let Some(dir) = p.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let (clean, _) = crate::redact::anonymise(what);
    let clean: String = clean.chars().take(600).collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{now} {clean}");
    }
    trim(&p);
}

/// How much of it to keep. Enough that a walk through the window and the calls
/// it made are still both in there together, which is the thing this file is
/// for; not so much that a client running for a year holds a history nobody
/// asked it to keep.
const MAX_BYTES: u64 = 256 * 1024;
const KEEP_BYTES: usize = 128 * 1024;

/// Keep the end, drop the beginning, in place.
///
/// **An append-only file on somebody's machine is a promise to grow forever**,
/// and this one is written on every operator call. It is a diagnostic, not a
/// record: what matters is the last session, and the session before it when
/// somebody asks "it worked yesterday".
///
/// Rewritten rather than rotated, so there is never a `client.log.1` holding
/// older lines than the one the user was told about — a second file is a second
/// place to forget. Cut at a line boundary, because half a line at the top of a
/// log is the kind of thing somebody spends ten minutes reading as a clue.
///
/// Failure here is silence on purpose: a client that cannot tidy its own log
/// must still answer the question it was asked.
fn trim(p: &std::path::Path) {
    let Ok(meta) = std::fs::metadata(p) else { return };
    if meta.len() <= MAX_BYTES {
        return;
    }
    let Ok(body) = std::fs::read_to_string(p) else { return };
    let cut = body.len().saturating_sub(KEEP_BYTES);
    let tail = match body[cut..].find('\n') {
        Some(i) => &body[cut + i + 1..],
        None => return,
    };
    let _ = std::fs::write(p, tail);
}

/// The three facts that decide whether anything can work, written once at start.
///
/// A client built without its operator and without the pinned log key starts,
/// draws its window, and quietly cannot use the published directory — so every
/// project becomes a model question and the screen says the project publishes
/// nothing. This line is the difference between seeing that and arguing about
/// it.
pub fn start(operator: &str, index_entries: usize, index_verified: bool, model: &str) {
    line(&format!(
        "start operator={operator} directory={} entries={index_entries} model={model}",
        if index_verified { "verified" } else { "NOT VERIFIED — nothing will be found by name" }
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The log stops growing, and stops at a line boundary.
    ///
    /// An append-only file on somebody's machine is a promise to grow forever,
    /// and this one is written on every operator call. It was unbounded until
    /// 2026-09-16 — noticed by a person watching it fill up rather than by
    /// anything here.
    #[test]
    fn the_log_is_capped_and_cut_between_lines() {
        let dir = std::env::temp_dir().join(format!("podshl-logtest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("client.log");

        // Every line says which one it is, so the ones that survive can be named.
        let mut body = String::new();
        let mut n = 0u32;
        while body.len() < (MAX_BYTES as usize) + 4096 {
            body.push_str(&format!("{n} line number {n} with some words after it to give it length\n"));
            n += 1;
        }
        let written = n;
        std::fs::write(&p, &body).expect("cannot write the fixture");
        assert!(std::fs::metadata(&p).unwrap().len() > MAX_BYTES);

        trim(&p);

        let after = std::fs::read_to_string(&p).expect("the log is gone");
        assert!(after.len() <= KEEP_BYTES,
                "kept {} bytes, which is more than the {KEEP_BYTES} it promises", after.len());
        assert!(!after.is_empty(), "trimming emptied the log");

        // **The end is what is kept.** A log trimmed from the wrong end answers
        // "what happened just now" with last month.
        assert!(after.contains(&format!("{} line number", written - 1)),
                "the newest line did not survive");
        assert!(!after.contains("\n0 line number 0 "), "the oldest line survived");

        // And the first surviving line is a whole one: half a line at the top of
        // a log is read as a clue for ten minutes.
        let first = after.lines().next().expect("no lines left");
        assert!(first.split(' ').next().and_then(|w| w.parse::<u32>().ok()).is_some(),
                "the log now starts mid-line: {first:?}");

        // Under the cap it is left alone, bytes for bytes.
        std::fs::write(&p, "one\ntwo\n").unwrap();
        trim(&p);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\ntwo\n",
                   "a small log was rewritten for no reason");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
