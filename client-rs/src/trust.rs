//! Out-of-band key resolution.
//!
//! Verifying an Agent Card against the key contained in that same card is
//! circular: it proves the document is internally consistent and says nothing
//! about who published it. The key has to arrive by a path its publisher does
//! not control, which is what the Agent Name Service provides — published as a
//! DNS record under a domain the organisation demonstrably controls, and bound
//! to a Legal Entity Identifier.

use serde_json::Value;
use std::path::PathBuf;

pub enum Resolver {
    /// Local stand-in for an ANS lookup. Not a trust mechanism, and says so.
    Pinned(PathBuf),
    /// Production path: a DNS TXT record under `_agent.<domain>`.
    Dns,
}

impl Resolver {
    pub fn source(&self) -> String {
        match self {
            Resolver::Pinned(p) => m!("key_source_pinned", p = p.display()),
            Resolver::Dns => m!("key_source_dns"),
        }
    }

    pub fn jwk_for(&self, domain: &str) -> Option<Value> {
        match self {
            Resolver::Pinned(path) => {
                let raw = std::fs::read_to_string(path).ok()?;
                let map: Value = serde_json::from_str(&raw).ok()?;
                map.get(domain).cloned()
            }
            Resolver::Dns => dns_jwk(domain),
        }
    }
}

/// Resolve `_agent.<domain>` TXT and read `a2a-jwk=<json>`.
///
/// This is the path that makes the trust real: the key arrives under a domain
/// the organisation demonstrably controls, rather than from the document it is
/// supposed to authenticate. Returning `None` on every failure is deliberate —
/// a resolver that cannot answer must refuse, never improvise.
fn dns_jwk(domain: &str) -> Option<Value> {
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    use hickory_resolver::Resolver as DnsResolver;

    let name = format!("_agent.{domain}");
    let r = DnsResolver::new(ResolverConfig::default(), ResolverOpts::default()).ok()?;
    let txt = r.txt_lookup(&name).ok()?;
    for record in txt.iter() {
        // A TXT record arrives in 255-byte chunks; a JWK spans several.
        let joined: String = record
            .txt_data()
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect();
        if let Some(rest) = joined.strip_prefix("a2a-jwk=") {
            if let Ok(v) = serde_json::from_str::<Value>(rest) {
                return Some(v);
            }
        }
    }
    None
}
