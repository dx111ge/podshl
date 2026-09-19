//! What this client can actually do on this machine.
//!
//! A capability list that hides its own gaps overstates what the user is
//! agreeing to, so both halves are reported: what is readable here, and what is
//! not and why. The second half is the one that matters — an agent that quietly
//! offers a reading it cannot perform produces a dead end after the user has
//! already consented to it.
//!
//! This lived in Python, alongside a duplicate of the client. It belongs to the
//! binary that ships, because the answer is a property of that binary on this
//! machine and of nothing else.

use serde_json::{json, Value};

use crate::{actions, handover, probes, reads};

pub fn report() -> Value {
    let available = reads::catalogue();
    let unavailable = reads::catalogue_unavailable();

    json!({
        "platform": {
            "os": probes::os_id(),
            "arch": std::env::consts::ARCH,
            // Windows and macOS are unverified: no machine here runs them, and
            // saying so is more useful than a branch that claims to work.
            "verified_here": probes::os_id() == "linux",
        },
        "readable": available,
        "unavailable": unavailable,
        "actions": actions::VOCABULARY.iter().map(|a| json!({
            "id": a.id, "mutating": a.mutating, "reversible": a.reversible
        })).collect::<Vec<_>>(),
        "reply_channels": handover::channels_json(),
        "limits": {
            "max_reads": reads::MAX_READS,
            "max_entries": reads::MAX_ENTRIES,
            "max_depth": reads::MAX_DEPTH,
        },
    })
}

/// Human-readable, because this is run by a person wondering why a reading did
/// not happen.
pub fn print_report() {
    let r = report();
    let n_ok = r["readable"].as_array().map(|a| a.len()).unwrap_or(0);
    let n_no = r["unavailable"].as_array().map(|a| a.len()).unwrap_or(0);

    println!("PODSHL — what this client can do on this machine\n");
    println!(
        "  Platform      {} / {}{}",
        r["platform"]["os"].as_str().unwrap_or("?"),
        r["platform"]["arch"].as_str().unwrap_or("?"),
        if r["platform"]["verified_here"] == true {
            ""
        } else {
            "   (unverified branch)"
        }
    );
    println!("  Readable      {n_ok} values");
    for e in r["readable"].as_array().unwrap() {
        println!("                · {}", e["id"].as_str().unwrap_or("?"));
    }
    if n_no > 0 {
        println!("  Not readable  {n_no} — and why:");
        for e in r["unavailable"].as_array().unwrap() {
            println!(
                "                · {}  {}",
                e["id"].as_str().unwrap_or("?"),
                e["why"].as_str().unwrap_or("")
            );
        }
    }
    println!("  Actions       {}", r["actions"].as_array().unwrap().len());
    for a in r["actions"].as_array().unwrap() {
        println!(
            "                · {}{}",
            a["id"].as_str().unwrap_or("?"),
            if a["mutating"] == true {
                "  (changes things)"
            } else {
                ""
            }
        );
    }
    println!(
        "\n  Limits        at most {} readings per diagnosis",
        r["limits"]["max_reads"]
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L1: the report names what actually resolves here, and it names the gaps
    /// too. A doctor that only lists successes is a doctor nobody consults.
    #[test]
    fn doctor_reports_what_actually_resolves_on_this_machine() {
        // Held, because the two halves are computed separately and a project
        // granted by another case between them would move a reading from one
        // half to the other mid-report.
        let _g = reads::grants_held();
        let r = report();
        assert!(["linux", "windows", "macos"].contains(&r["platform"]["os"].as_str().unwrap()));

        let readable = r["readable"].as_array().unwrap();
        let unavailable = r["unavailable"].as_array().unwrap();
        assert!(
            !readable.is_empty() || !unavailable.is_empty(),
            "the catalogue is empty in both directions — nothing was actually checked"
        );

        // Every entry appears in exactly one half. A reading that is in neither
        // has been dropped silently; one in both is a contradiction.
        let total = readable.len() + unavailable.len();
        assert_eq!(
            total,
            reads::catalogue_all_len(),
            "the two halves do not account for the whole catalogue"
        );

        for e in unavailable {
            assert!(
                !e["why"].as_str().unwrap_or("").is_empty(),
                "{} is unavailable with no reason given",
                e["id"]
            );
        }
        assert!(!r["actions"].as_array().unwrap().is_empty());
    }
}
