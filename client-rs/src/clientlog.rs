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
