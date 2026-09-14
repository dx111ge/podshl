//! The discovery index: fetched whole, searched here.
//!
//! **The user names whatever is not working** — a program (`pip`), a device or
//! its maker (`nvidia`), a product (`datev`) — and this answers *do we have
//! anything for that* without asking anyone. Both kinds matter: the classes a
//! project declares name other people's software and hardware alike. The whole
//! catalogue is downloaded, verified once, cached, and searched on this
//! machine.
//!
//! That is the point rather than an optimisation. A `GET /search?q=` would let
//! the operator answer "who looked for what", and what a user types is the
//! problem they have. Fetching the catalogue means the operator learns that
//! somebody has it and never what they wanted from it.
//!
//! **The signature is checked against a pinned key, not one the index arrives
//! with.** An index that carries the key it is verified with proves nothing, in
//! exactly the way a self-asserted agent card proves nothing — so the log's
//! public key is pinned out of band, the same way `ans_stub.json` pins a
//! vendor's. An index that does not verify is not a degraded index; it is not
//! an index, and search falls back to what it would have done anyway.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::jws;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Entry {
    pub host: String,
    #[serde(default)]
    pub anchor_url: String,
    #[serde(default)]
    pub problem_classes: Vec<String>,
    #[serde(default)]
    pub search_tokens: Vec<String>,
    #[serde(default)]
    pub langs: Vec<String>,
    /// The publisher's own word — `active` or `deprecated` — and the only one
    /// that is authoritative. A dormant project is not a wrong one.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub successor_url: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub log_seq: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Index {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub tree_size: u64,
    #[serde(default)]
    pub generated_ms: u64,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

/// How long a cached catalogue is treated as current.
///
/// The catalogue moves — projects are published, deprecated, held — so "fetched
/// once at startup" is wrong for an application somebody leaves open for days.
/// Six hours is a compromise between that and being noisy: a conditional
/// request costs a 304, and the entries themselves change on the order of an
/// ingest cycle, not seconds.
pub const MAX_AGE_MS: u64 = 6 * 60 * 60 * 1000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Old enough to be worth re-fetching. A clock that has gone backwards makes
/// `now` smaller than `generated_ms`; that is not freshness, so it counts as
/// stale rather than as eternally current.
pub fn is_stale(idx: &Index, now: u64) -> bool {
    now.saturating_sub(idx.generated_ms) >= MAX_AGE_MS || now < idx.generated_ms
}

fn cache_path() -> PathBuf {
    if let Ok(root) = std::env::var("VS_ROOT") {
        return PathBuf::from(root).join("index_cache.json");
    }
    dirs::config_dir()
        .map(|p| p.join("podshl").join("index_cache.json"))
        .unwrap_or_else(|| PathBuf::from("index_cache.json"))
}

/// The log's public key, pinned out of band. Absent means we cannot check a
/// signature, and an unchecked index is not used.
///
/// An installed client has no checkout beside it, so a release is built with
/// the key of the operator it is built for (`PODSHL_BUILD_LOG_KEY`, the JWK's
/// text — `scripts/build_windows_installer.ps1`). The environment still wins,
/// so a development run is unchanged. The `../var` fallback exists only in a
/// debug build: in a release it would be a key read relative to whatever
/// directory the program was started from, and a key anybody can plant beside
/// a shortcut is not pinned.
pub(crate) fn pinned_key() -> Option<Value> {
    let named = std::env::var("VS_LOG_KEY").map(PathBuf::from).ok().or_else(|| {
        std::env::var("VS_ROOT").map(|r| PathBuf::from(r).join("log_key.json")).ok()
    });
    if let Some(p) = named {
        return serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok();
    }
    if let Some(built) = option_env!("PODSHL_BUILD_LOG_KEY") {
        return serde_json::from_str(built).ok();
    }
    #[cfg(debug_assertions)]
    {
        serde_json::from_str(&std::fs::read_to_string("../var/log_key.json").ok()?).ok()
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

/// Verify the signed document and return the index inside it.
///
/// The signature covers the index body exactly as the server canonicalised it,
/// so this is the same detached-JWS-over-JCS check the client already performs
/// on an agent card. One verifier, two uses.
pub fn verify(doc: &Value) -> Result<Index, String> {
    let jwk = pinned_key().ok_or_else(|| m!("index_no_pinned_key"))?;
    let body = doc.get("index").ok_or_else(|| m!("index_missing"))?;
    let sig = doc.get("signature").ok_or_else(|| m!("index_unsigned"))?;
    jws::verify_detached(&jwk, body, sig)?;

    let idx: Index = serde_json::from_value(body.clone()).map_err(|e| e.to_string())?;
    // A version we cannot read is not a version we should guess at.
    if idx.version != 1 {
        return Err(m!("index_version_unknown", v = idx.version));
    }
    // The server publishes only what its signed head can prove. Checking it
    // here as well costs nothing and makes the claim ours rather than theirs.
    for e in &idx.entries {
        match e.log_seq {
            Some(seq) if seq < idx.tree_size => {}
            _ => {
                return Err(m!("index_entry_unproven", host = e.host))
            }
        }
    }
    Ok(idx)
}

/// Fetch, verify, and cache. Only a verified index is ever written.
pub async fn refresh(base: &str) -> Result<Index, String> {
    let url = format!("{}/index", base.trim_end_matches('/'));
    let resp = crate::http::client()
        .get(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| m!("index_unreachable", e = e))?;
    // The catalogue is the one document fetched whole by design, and it grows
    // with the number of published projects, so it gets the larger cap rather
    // than the one an answer about a single machine is held to.
    let doc: Value = crate::http::json_capped(resp, crate::http::MAX_INDEX_BODY)
        .await
        .map_err(|e| m!("index_unreadable", e = e))?;

    let idx = verify(&doc)?;
    let p = cache_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&p, serde_json::to_string(&doc).unwrap_or_default());
    Ok(idx)
}

/// Refresh only if the cache is missing or old.
///
/// **Never call this from the search path.** A fetch immediately before a query
/// correlates the two: the operator cannot see *what* was searched, but a
/// request arriving the moment a user types would say that one happened. The
/// refresh is on a clock of its own precisely so that its timing carries no
/// information about the person using it.
pub async fn ensure_fresh(base: &str) -> Result<(bool, Index), String> {
    if let Some(idx) = cached() {
        if !is_stale(&idx, now_ms()) {
            return Ok((false, idx));
        }
    }
    refresh(base).await.map(|i| (true, i))
}

/// The cached index, if one was ever verified and stored.
///
/// Re-verified on load rather than trusted because we wrote it: a cache file is
/// on disk, and on disk is where other things can reach it.
pub fn cached() -> Option<Index> {
    let raw = std::fs::read_to_string(cache_path()).ok()?;
    let doc: Value = serde_json::from_str(&raw).ok()?;
    verify(&doc).ok()
}

/// Everything that answers for what the user typed.
///
/// Matching is on tokens the publisher earned — segments of the problem classes
/// it declared, and of the anchor it proved control of. There is no name field
/// to match against, because there is no name field.
pub fn search(idx: &Index, query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Entry> = idx
        .entries
        .iter()
        .filter(|e| {
            e.search_tokens.iter().any(|t| t == &q)
                || e.problem_classes.iter().any(|c| c.to_lowercase().contains(&q))
                || e.host.to_lowercase().contains(&q)
        })
        .cloned()
        .collect();
    // An exact token beats a substring, and a live project beats a deprecated
    // one. Beyond that the order is the index's, which is the host order.
    hits.sort_by_key(|e| {
        (
            e.status == "deprecated",
            !e.search_tokens.iter().any(|t| t == &q),
        )
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> Index {
        Index {
            version: 1,
            tree_size: 10,
            generated_ms: 0,
            entries: vec![
                Entry {
                    host: "example.org".into(),
                    problem_classes: vec!["pip.install.wheel-missing".into()],
                    search_tokens: vec!["pip".into(), "install".into(), "wheel".into()],
                    status: "active".into(),
                    log_seq: Some(1),
                    ..Default::default()
                },
                // A project that repairs driver problems. Nominative use of a
                // hardware maker's name, which is the same mechanism as `pip`.
                Entry {
                    host: "drivers.example".into(),
                    problem_classes: vec!["nvidia.driver.version-mismatch".into()],
                    search_tokens: vec!["nvidia".into(), "driver".into()],
                    status: "active".into(),
                    log_seq: Some(3),
                    ..Default::default()
                },
                Entry {
                    host: "old.example".into(),
                    problem_classes: vec!["pip.install.version-conflict".into()],
                    search_tokens: vec!["pip".into(), "install".into()],
                    status: "deprecated".into(),
                    log_seq: Some(2),
                    ..Default::default()
                },
            ],
        }
    }

    /// The whole point of the field: a user types the thing that is broken,
    /// whether that is a program or a device's maker.
    #[test]
    fn what_broke_is_what_is_typed() {
        let hits = search(&idx(), "pip");
        assert_eq!(hits.len(), 2, "typing the software found nothing");
        assert_eq!(hits[0].status, "active", "a deprecated project was offered first");
    }

    /// A hardware maker is as valid a thing to type as a program. The field is
    /// "what is not working", and both kinds reach it the same way.
    #[test]
    fn a_hardware_makers_name_is_as_good_as_a_programs() {
        let hits = search(&idx(), "nvidia");
        assert_eq!(hits.len(), 1, "typing a hardware maker found nothing");
        assert_eq!(hits[0].host, "drivers.example");
    }

    #[test]
    fn an_unknown_name_is_not_a_match() {
        assert!(search(&idx(), "kodak").is_empty(), "an unrelated name matched");
        assert!(search(&idx(), "").is_empty(), "an empty query matched");
    }

    /// A catalogue that moves needs a clock, not a one-off at startup.
    #[test]
    fn a_cached_index_goes_stale_and_a_backwards_clock_is_not_freshness() {
        let mut i = idx();
        i.generated_ms = 1_000_000;
        assert!(!is_stale(&i, 1_000_000 + MAX_AGE_MS - 1), "fresh index called stale");
        assert!(is_stale(&i, 1_000_000 + MAX_AGE_MS), "an index past its age was called current");
        assert!(is_stale(&i, 500_000), "a clock that went backwards was read as freshness");
    }

    /// The interop case: the index this client verifies is the one the server
    /// actually signs. Fetched from the running operator rather than from a
    /// fixture, because a fixture proves the two agree with a file, not with
    /// each other — the same reason the card test fetches from `:8721`.
    #[test]
    fn the_servers_own_index_verifies_here() {
        let doc: Value = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let base = crate::http::operator_base();
                reqwest::get(format!("{base}/index")).await?.json::<Value>().await
            })
            .expect(
                "the operator is not answering — start it with `mise run server`, or set PODSHL_SERVER_URL to one that is. This test must never pass without a real signed index.",
            );
        let idx = verify(&doc).expect("the server's index did not verify against the pinned key");
        assert!(
            idx.tree_size > 0,
            "{} signs a correct index over an empty log: nothing has been \
             registered there, so there is no entry for this case to verify. \
             Not a client defect — claim an anchor on that operator, or point \
             PODSHL_SERVER_URL at one that has entries.",
            crate::http::operator_base()
        );
    }

    /// An index whose entries the signed head cannot prove is refused whole.
    /// Serving nine provable rows and one unprovable one is worse than none,
    /// because the user cannot tell which they got.
    #[test]
    fn an_entry_beyond_the_signed_head_refuses_the_index() {
        let mut i = idx();
        i.entries[0].log_seq = Some(99);
        let doc = serde_json::json!({"index": i, "signature": {}});
        assert!(verify(&doc).is_err(), "an unprovable entry was accepted");
    }
}
