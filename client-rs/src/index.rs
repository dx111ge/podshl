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
    /// Which kind of location was verified — `url` for a domain, `repo` for a
    /// repository on a forge. Not one claim: a domain holds without a third
    /// party, because DNS and TLS say who served the bytes, while a forge
    /// decides who may write in a repository. Shown, never collapsed into one
    /// sentence about control being confirmed.
    #[serde(default)]
    pub anchor_kind: String,
    #[serde(default)]
    pub problem_classes: Vec<String>,
    /// The sentence a person picks a class by, where the maintainer wrote one.
    ///
    /// A class is an identifier — `engram.llm.model-not-pulled` — because a
    /// rule matches on it and a solution answers it. It is not a question
    /// anybody can answer about their own computer, and for a long time three
    /// of them in a dropdown was the whole of what the window asked. Absent for
    /// every manifest published before this existed, which is why the window
    /// falls back to the identifier rather than showing nothing.
    #[serde(default)]
    pub class_labels: std::collections::BTreeMap<String, String>,
    /// The project's own words, kept as written by whatever model translates
    /// its text (`LG8`).
    ///
    /// Here rather than only on the card, because the card is fetched under a
    /// consent that comes *after* the question `class_labels` asks, and the
    /// labels are translated to put that question in the reader's language. At
    /// the one moment the terms are needed, nothing else has them. They are the
    /// publisher's own published words and this index is public, so carrying
    /// them discloses nothing; ingest bounds them, so nothing here has to.
    ///
    /// Absent from every index built before this existed, which is the whole
    /// reason for `serde(default)` — an older operator serves entries without
    /// it and a client reads them exactly as it did yesterday.
    #[serde(default)]
    pub glossary_keep: Vec<String>,
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
pub(crate) fn pinned_key() -> Result<(Value, String), String> {
    let named = std::env::var("VS_LOG_KEY")
        .map(|p| (PathBuf::from(p), "VS_LOG_KEY"))
        .ok()
        .or_else(|| {
            std::env::var("VS_ROOT")
                .map(|r| (PathBuf::from(r).join("log_key.json"), "VS_ROOT/log_key.json"))
                .ok()
        });
    if let Some((p, what)) = named {
        // Somebody named this file. If it cannot be read, or is not a key, that
        // is their answer being wrong — not an invitation to quietly use a
        // different one, which is how a client ends up verifying against a key
        // nobody chose.
        let raw = std::fs::read_to_string(&p)
            .map_err(|e| format!("{what} names {}, which cannot be read: {e}", p.display()))?;
        let k = serde_json::from_str(&raw)
            .map_err(|e| format!("{what} names {}, which is not a JWK: {e}", p.display()))?;
        return Ok(announce(k, what.to_string()));
    }
    if let Some(built) = option_env!("PODSHL_BUILD_LOG_KEY") {
        let k = serde_json::from_str(built)
            .map_err(|e| format!("the key compiled into this build is not a JWK: {e}"))?;
        return Ok(announce(k, "compiled into this build".to_string()));
    }
    #[cfg(debug_assertions)]
    {
        const DEV: &str = "../var/log_key.json";
        let raw = std::fs::read_to_string(DEV).map_err(|e| format!(
            "no key was named and none is compiled in, and the development key at {DEV} cannot be read: {e}"))?;
        let k = serde_json::from_str(&raw).map_err(|e| format!("{DEV} is not a JWK: {e}"))?;
        // Named in full, because this is the one source a person did not choose
        // and the one that quietly made a good index look forged.
        return Ok(announce(k, format!("{DEV} beside this checkout — a development key, not any operator's")));
    }
    #[cfg(not(debug_assertions))]
    Err("no log key: none was named and none is compiled into this build".to_string())
}

/// Say once, in the log, which key everything will be checked against.
///
/// The one that was used is the fact every failure here turns on, and until now
/// it was the one fact nobody had. `cargo test` against a live operator
/// reported "signature does not verify" about a perfectly good index, for a day
/// of 2026-09-15, because a debug build had silently reached for
/// `../var/log_key.json` — a stub `make_trust_stub.py` regenerates — and no
/// message anywhere named it.
fn announce(k: Value, what: String) -> (Value, String) {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| crate::clientlog::line(&format!("pinned log key from {what}")));
    (k, what)
}

/// Verify the signed document and return the index inside it.
///
/// The signature covers the index body exactly as the server canonicalised it,
/// so this is the same detached-JWS-over-JCS check the client already performs
/// on an agent card. One verifier, two uses.
pub fn verify(doc: &Value) -> Result<Index, String> {
    let (jwk, from) = pinned_key().map_err(|e| format!("{} — {e}", m!("index_no_pinned_key")))?;
    let body = doc.get("index").ok_or_else(|| m!("index_missing"))?;
    let sig = doc.get("signature").ok_or_else(|| m!("index_unsigned"))?;
    // Naming the key in the failure is the whole point: "signature does not
    // verify" is true of a wrong key and of a wrong index alike, and the two
    // have entirely different remedies.
    jws::verify_detached(&jwk, body, sig).map_err(|e| format!("{e} (key from {from})"))?;

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
impl Entry {
    /// Is this a repository on a forge rather than a domain?
    pub fn is_repo(&self) -> bool {
        self.anchor_kind == "repo"
    }

    /// What a person is shown, and what they compare against their own memory.
    ///
    /// A domain is its host. A repository is `owner/name` and **never** its
    /// host: `github.com` is shared by everything on it, so showing the host
    /// would present every repository under one name and tell the person
    /// nothing about which project this is. `owner/name` is the form they
    /// already know, because it is where they got the software.
    pub fn display(&self) -> String {
        if !self.is_repo() {
            return self.host.clone();
        }
        let tail = self.anchor_url.split("://").nth(1).unwrap_or("");
        let parts: Vec<&str> = tail.split('/').filter(|p| !p.is_empty()).collect();
        match parts.len() {
            0 | 1 => self.host.clone(),
            _ => parts[1..].join("/"),
        }
    }

    /// The words this entry can be found by, for a query typed by a person.
    ///
    /// For a repository the forge's own host is left out. It is shared by every
    /// repository there, so `github` would match all of them at once, and it is
    /// the forge's name rather than the project's. The server leaves it out of
    /// `search_tokens` for the same reason; this is the other half, because the
    /// client also matched on `host` directly.
    fn haystack(&self) -> Vec<String> {
        let mut out: Vec<String> = self.search_tokens.iter().map(|t| t.to_lowercase()).collect();
        out.extend(self.problem_classes.iter().map(|c| c.to_lowercase()));
        if self.is_repo() {
            out.push(self.display().to_lowercase());
        } else {
            out.push(self.host.to_lowercase());
        }
        out
    }
}

pub fn search(idx: &Index, query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Entry> = idx
        .entries
        .iter()
        .filter(|e| e.haystack().iter().any(|h| h == &q || h.contains(&q)))
        .cloned()
        .collect();
    // **Ordered by the query, never by the projects.** An exact token beats a
    // substring and a live project beats a deprecated one, because both are
    // statements about this search rather than about whose project is better.
    // Beyond that the order is the index's, which is host order.
    //
    // Nothing else goes in here, and the reason is `W8`: *"the participant list
    // is ordered by name — an ordering is a ranking wearing different clothes"*.
    // Stars and downloads are bought by the hour and need a forge API; this
    // operator's own reporter counts cannot be in a public index at all, since
    // no public page may carry a per-project figure; and weighting by whether
    // some third party corroborates a project is this operator ranking
    // projects, which is the thing that rule refuses. Sorting by popularity was
    // measured against the only concrete case anybody looked at and put the
    // *wrong* project first.
    //
    // So the list does not decide. It shows `owner/name`, what kind of location
    // was verified and what each project claims to answer for, and the person
    // recognises their own — which they can, because it is where they got it.
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

    /// An entry from an operator that has not been updated still reads.
    ///
    /// `glossary_keep` was added so the first question's labels could be
    /// translated with the project's own terms kept, and every index signed
    /// before that lacks it. A client that refused those, or that read a
    /// missing list as anything but "no terms", would stop working against
    /// every operator in the world on the day this shipped.
    #[test]
    fn an_entry_without_a_glossary_is_an_entry_with_no_terms() {
        let old: Entry = serde_json::from_value(serde_json::json!({
            "host": "github.com",
            "anchor_url": "https://github.com/dx111ge/engram/",
            "problem_classes": ["engram.llm.model-not-pulled"],
            "class_labels": {"engram.llm.model-not-pulled": "Search returns nothing"}
        }))
        .expect("an index from before the glossary did not parse");
        assert!(old.glossary_keep.is_empty(), "a missing glossary is no terms, not a failure");

        let new: Entry = serde_json::from_value(serde_json::json!({
            "host": "github.com",
            "problem_classes": [],
            "glossary_keep": ["brain", ".brain"]
        }))
        .expect("an index with a glossary did not parse");
        assert_eq!(new.glossary_keep, vec!["brain".to_string(), ".brain".to_string()]);
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

    /// A repository is shown as `owner/name`, found by it, and never found by
    /// the forge.
    ///
    /// `github.com` is shared by every repository on it. Showing the host would
    /// put the same name on every row; matching on it would make one word find
    /// all of them at once. Both were true — the client matched `host` directly
    /// even after the server stopped emitting the forge as a token.
    #[test]
    fn a_repository_is_its_owner_and_name_not_its_forge() {
        let mut i = idx();
        i.entries = vec![
            Entry {
                host: "github.com".into(),
                anchor_url: "https://github.com/dx111ge/engram/".into(),
                anchor_kind: "repo".into(),
                search_tokens: vec!["dx111ge".into(), "engram".into()],
                problem_classes: vec!["engram.index.corrupt".into()],
                status: "active".into(),
                ..Default::default()
            },
            Entry {
                host: "github.com".into(),
                anchor_url: "https://github.com/someone/other/".into(),
                anchor_kind: "repo".into(),
                search_tokens: vec!["someone".into(), "other".into()],
                status: "active".into(),
                ..Default::default()
            },
        ];

        assert_eq!(i.entries[0].display(), "dx111ge/engram",
                   "a repository was shown under the forge's name");

        let hits = search(&i, "engram");
        assert_eq!(hits.len(), 1, "searching a project name found {} rows", hits.len());
        assert_eq!(hits[0].display(), "dx111ge/engram");

        assert!(search(&i, "github").is_empty(),
                "the forge's own name found every repository on it at once");
        assert!(search(&i, "github.com").is_empty(),
                "the forge's host found every repository on it at once");

        // And a domain is still found by its host, which is its name.
        let mut d = idx();
        d.entries = vec![Entry {
            host: "curl.se".into(),
            anchor_url: "https://curl.se/".into(),
            anchor_kind: "url".into(),
            status: "active".into(),
            ..Default::default()
        }];
        assert_eq!(d.entries[0].display(), "curl.se");
        assert_eq!(search(&d, "curl").len(), 1, "a domain stopped being findable");
    }

}
