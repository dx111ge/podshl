//! A2A client: discovery at the well-known URI, and one JSON-RPC method.
//!
//! `NoAgent` is kept strictly distinct from a verification failure. "There is
//! nobody there" and "do not trust this" lead to opposite behaviour, and today
//! the first is the normal case: almost no vendor publishes an agent.

use serde_json::{json, Value};

pub const WELL_KNOWN: &str = "/.well-known/agent-card.json";

/// **Absence has to be stated, and a hang states nothing.**
///
/// Both calls in this file used `reqwest::get` and `reqwest::Client::new()`,
/// which build a client with *no timeout at all*. A host that accepts the
/// connection and then never answers therefore froze the client for good — at
/// the moment the user's machine is already broken, with no message and nothing
/// to press. That is the exact failure this module's own header rules out: "there
/// is nobody there" is a state the user must be told about, not one they infer
/// from a spinner.
///
/// Found on Windows, where the suite's "unreachable host" — `127.0.0.1:9` —
/// turned out to be very reachable: Simple TCP/IP Services was enabled, and the
/// discard service on port 9 accepts everything and answers nothing. On Linux
/// nothing listens there, the connection is refused in microseconds, and the
/// missing timeout was invisible for the whole life of the project.
///
/// Connect and total are separate on purpose. A host that will not accept is a
/// different fact from one that accepts and stalls, and the second is the one
/// worth being impatient about.
const TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Is this host one of this machine's own names? Loopback in either
/// protocol, and `localhost`, which resolves to it. The development stack is
/// there, and there is no wire between this process and it for anybody to
/// stand on.
pub fn is_loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.eq_ignore_ascii_case("localhost") || bare == "::1" {
        return true;
    }
    match bare.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// Why this base may not be spoken to at all, if it may not.
///
/// A vendor's answer is signed, so a plain-text connection cannot forge a
/// remedy — but it can read what was sent, and what is sent is the readings.
/// Everything this client generalises and anonymises before transmission was
/// being transmitted in the clear to any base a person typed with `http://`.
/// The only bases that need no wire are the ones on this machine.
pub fn insecure_base(base: &str) -> Option<String> {
    let host = crate::flow::host_of(base);
    let scheme = base.split("://").next().unwrap_or("").to_ascii_lowercase();
    if scheme == "https" || (scheme == "http" && is_loopback(&host)) {
        None
    } else {
        Some(m!("https_required", host = host))
    }
}

fn client() -> Result<&'static reqwest::Client, String> {
    Ok(crate::http::client())
}

#[derive(Debug)]
pub enum DiscoveryError {
    /// The vendor publishes no agent at all.
    NoAgent(String),
    /// Someone answered, but the identity does not hold up.
    Untrusted(String),
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryError::NoAgent(w) => write!(f, "{w}"),
            DiscoveryError::Untrusted(w) => write!(f, "{w}"),
        }
    }
}

pub async fn fetch_card(base: &str) -> Result<Value, DiscoveryError> {
    if let Some(why) = insecure_base(base) {
        return Err(DiscoveryError::Untrusted(why));
    }
    let url = format!("{}{}", base.trim_end_matches('/'), WELL_KNOWN);
    let resp = client()
        .map_err(DiscoveryError::NoAgent)?
        .get(&url)
        .timeout(TOTAL_TIMEOUT)
        .send()
        .await
        // A timeout lands here, as `NoAgent`, which is the right grade: a host
        // that will not answer is not a host that failed verification, and the
        // user is told which of the two it was.
        .map_err(|e| {
            DiscoveryError::NoAgent(if e.is_timeout() {
                m!("no_answer_in_time")
            } else {
                m!("unreachable_paren", e = e)
            })
        })?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(DiscoveryError::NoAgent(
            m!("no_card_at_well_known"),
        ));
    }
    if !resp.status().is_success() {
        return Err(DiscoveryError::NoAgent(m!("card_http", n = resp.status().as_u16())));
    }
    crate::http::json_capped(resp, crate::http::MAX_BODY)
        .await
        .map_err(|_| DiscoveryError::NoAgent(m!("not_a_card")))
}

/// Reject a card whose identity does not hold up. Separate from `NoAgent`
/// because the two demand opposite behaviour, and the type is what keeps them
/// apart — a comment claiming the distinction is not the same as enforcing it.
pub fn untrusted(reason: impl Into<String>) -> DiscoveryError {
    DiscoveryError::Untrusted(reason.into())
}

pub async fn send_message(base: &str, data: Value) -> Result<Value, String> {
    if let Some(why) = insecure_base(base) {
        return Err(why);
    }
    let url = format!("{}/a2a", base.trim_end_matches('/'));
    let body = json!({
        "jsonrpc": "2.0",
        "id": "rs-client",
        "method": "SendMessage",
        "params": { "message": { "role": "user", "parts": [ { "kind": "data", "data": data } ] } }
    });
    let resp = client()?
        .post(&url)
        .json(&body)
        .timeout(TOTAL_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                m!("no_answer_in_time")
            } else {
                e.to_string()
            }
        })?;
    let v: Value = crate::http::json_capped(resp, crate::http::MAX_BODY).await?;
    if let Some(err) = v.get("error") {
        return Err(m!("vendor_error", e = err));
    }
    v.get("result").cloned().ok_or_else(|| m!("no_result"))
}
