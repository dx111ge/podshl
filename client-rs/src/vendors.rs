//! Finding out *whom* to ask.
//!
//! **The user names the vendor, and the vendor says how to look.** There is no
//! device detection here, on purpose. Enumerating the machine to guess a vendor
//! could only ever see part of it — a PCI bus shows a graphics chip and a
//! network chip, never a networked printer, an installed application or an
//! attached peripheral — so it splits hardware from software for no reason and
//! implies a completeness it cannot have.
//!
//! It is also unnecessary. Once a vendor is named, its skill carries the read
//! instructions: it knows what is worth knowing about its own product, in the
//! version the customer has. The client never needs to guess in advance.
//!
//! And it was a rule violation: enumerating the bus is itself a read, performed
//! before anyone consented to anything.

use serde_json::Value;

/// Vendors the client can resolve by name without touching the machine. A real
/// deployment resolves these through the Agent Name Service; this list is the
/// offline fallback and covers the common case of a user typing a brand.
const KNOWN: &[(&str, &str)] = &[
    ("nvidia", "nvidia.com"), ("amd", "amd.com"), ("intel", "intel.com"),
    ("microsoft", "microsoft.com"), ("apple", "apple.com"), ("dell", "dell.com"),
    ("lenovo", "lenovo.com"), ("hp", "hp.com"), ("datev", "datev.de"),
    ("bosch", "bosch.com"), ("siemens", "siemens.com"), ("logitech", "logitech.com"),
    ("samsung", "samsung.com"), ("seagate", "seagate.com"), ("brother", "brother.de"),
];

/// Where a vendor's agent would live if it had one.
pub fn agent_base(domain: &str) -> String {
    format!("https://support.{domain}")
}

/// Local stand-in for an ANS directory lookup.
/// The checkout's file is a debug-build default only; a release reads a
/// directory it was pointed at, never one beside wherever it was started.
fn directory_lookup(q: &str) -> Option<Value> {
    let path = match std::env::var("VS_DIRECTORY") {
        Ok(p) => p,
        Err(_) if cfg!(debug_assertions) => "../var/vendor_directory.json".into(),
        Err(_) => return None,
    };
    let raw = std::fs::read_to_string(path).ok()?;
    let map: Value = serde_json::from_str(&raw).ok()?;
    let obj = map.as_object()?;
    obj.iter()
        .find(|(name, _)| q.contains(name.as_str()) || name.contains(q))
        .map(|(_, v)| v.clone())
}

/// Resolve what the user typed. Never guesses silently: every result says how
/// it was arrived at, so an unknown brand is visibly a guess at a domain rather
/// than a lookup.
pub fn search(query: &str) -> Value {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return serde_json::json!([]);
    }

    // An explicit address is used verbatim, and it comes first: a support
    // endpoint does not always live at support.<domain>, and an operator who
    // typed a URL means it. `SERVER.md` puts a directory last for this reason —
    // it is a hint, and a hint must not outrank what the user actually said.
    if q.starts_with("http://") || q.starts_with("https://") {
        let base = query.trim().trim_end_matches('/').to_string();
        let host = base.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").to_string();
        return serde_json::json!([{
            "vendor": host.clone(), "domain": host, "base": base,
            "how": m!("how_full_address")
        }]);
    }

    // The published catalogue, searched on this machine. This is where a
    // published project is found, and it is found by **whatever is not
    // working**: a program like `pip`, or a device and its maker like
    // `nvidia`. Both, because the classes a project declares name other
    // people's software and hardware alike. Tokens come from those classes and
    // from the anchor it proved — never from a name it asserted.
    if let Some(idx) = crate::index::cached() {
        let hits = crate::index::search(&idx, &q);
        if !hits.is_empty() {
            return serde_json::json!(hits
                .iter()
                .map(|e| serde_json::json!({
                    "vendor": e.host.clone(),
                    "domain": e.host.clone(),
                    "base": e.anchor_url.clone(),
                    "answers": e.problem_classes.clone(),
                    "status": e.status.clone(),
                    "how": m!("how_published")
                }))
                .collect::<Vec<_>>());
        }
    }

    // A local directory file. Last, and only a hint.
    if let Some(hit) = directory_lookup(&q) {
        return serde_json::json!([hit]);
    }

    let mut out: Vec<Value> = KNOWN
        .iter()
        .filter(|(name, _)| name.contains(&q) || q.contains(*name))
        .map(|(name, domain)| serde_json::json!({
            "vendor": name, "domain": domain, "base": agent_base(domain),
            "how": m!("how_known_vendor")
        }))
        .collect();

    if q.contains('.') && !q.contains(' ') {
        let d = q.trim_start_matches("https://").trim_start_matches("http://")
                 .split('/').next().unwrap_or(&q).to_string();
        if !out.iter().any(|v| v["domain"] == d.as_str()) {
            out.push(serde_json::json!({
                "vendor": d.clone(), "domain": d.clone(), "base": agent_base(&d),
                "how": m!("how_address")
            }));
        }
    } else if out.is_empty() {
        out.push(serde_json::json!({
            "vendor": q.clone(), "domain": format!("{q}.com"),
            "base": agent_base(&format!("{q}.com")),
            "how": m!("how_guessed")
        }));
    }
    serde_json::json!(out)
}


/// Does what was actually read contradict the vendor the user named?
///
/// A person at a computer says "Intel" and an AMD card is installed. Their
/// claim is a claim; the reading is not. Catching that is cheap — read values
/// carry manufacturer names — and letting it pass means the whole diagnosis
/// runs against the wrong vendor, in both paths: a vendor skill would be asked
/// about hardware that is not theirs, and a model without a vendor would reason
/// from a false premise the user handed it.
///
/// This never overrides the user. It says what it saw and offers the switch.
pub fn mismatch(chosen: &str, facts: &Value) -> Option<(String, String)> {
    let blob = facts.as_object()?.values()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if blob.is_empty() {
        return None;
    }
    let chosen_l = chosen.to_lowercase();
    // A contradiction needs two claims of the same kind. "Intel" against an AMD
    // card is one; "ACME", a typed address or a software project against an
    // NVIDIA card is not — the user named whom they are asking, not the chip,
    // and a card vendor's product has somebody else's GPU in it. Flagging that
    // told the walk through the window "that does not add up" about the
    // counterparty's own demo.
    if !KNOWN.iter().any(|(name, _)| chosen_l.contains(name)) {
        return None;
    }
    for (name, domain) in KNOWN {
        if chosen_l.contains(name) {
            continue;                       // the vendor the user picked
        }
        // Word-ish match so "amd" does not fire inside "amdgpu-adjacent" prose
        // that happens to contain it as a substring of something else.
        let found = blob.split(|c: char| !c.is_alphanumeric()).any(|w| w == *name);
        if found {
            return Some((name.to_string(), domain.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spots_a_vendor_the_user_did_not_name() {
        let facts = json!({"gpu.name": "AMD Radeon RX 7900 XTX"});
        let (found, _) = mismatch("intel", &facts).expect("mismatch not detected");
        assert_eq!(found, "amd");
    }

    #[test]
    fn stays_quiet_when_the_reading_agrees() {
        let facts = json!({"gpu.name": "NVIDIA GeForce RTX 2070 SUPER"});
        assert!(mismatch("nvidia", &facts).is_none(), "false alarm on a match");
    }

    #[test]
    fn stays_quiet_when_nothing_was_read() {
        assert!(mismatch("intel", &json!({})).is_none());
    }

    /// V4: naming a vendor that is not a chip brand — a card maker, a typed
    /// address, a software project — is not contradicted by the chip inside.
    /// The walk through the window was told "that does not add up" about the
    /// counterparty's own demo, and the switch it offered went nowhere.
    #[test]
    fn a_vendor_that_is_not_a_brand_is_not_contradicted_by_the_chip() {
        let facts = json!({"gpu.name": "NVIDIA GeForce RTX 2070 SUPER"});
        for chosen in ["127.0.0.1", "ACME Components GmbH", "engram.localhost", "pip"] {
            assert!(mismatch(chosen, &facts).is_none(), "{chosen} was contradicted by an NVIDIA reading");
        }
    }
}
