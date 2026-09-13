//! Handing the case to a person on the vendor's side.
//!
//! **The vendor defines the hand-off; the client bounds it.** Where the case
//! goes is the vendor's routing decision and stays opaque here — their ITSM,
//! their queue. What is needed to open it arrives as ordinary probes, so a
//! contact address is a field the vendor asks for and the user consents to,
//! rather than a column baked into every client. And how the answer comes back
//! is chosen from the list below: the vendor picks a channel, it cannot invent
//! one, for the same reason it cannot invent an action.
//!
//! A vendor that will not reply must say `none`. Leaving a person waiting for
//! an answer that was never coming is the failure this list exists to prevent.

use serde_json::{json, Value};

/// Channels this client can actually honour. `agent_callback` is deliberately
/// absent: it would need an inbound endpoint on the customer's machine, and
/// declaring support we do not have is worse than declaring none.
pub const REPLY_CHANNELS: &[&str] = &["email", "ticket_url", "none"];

pub fn channel_known(id: &str) -> bool {
    REPLY_CHANNELS.contains(&id)
}

/// What a channel means for the person waiting, in a sentence.
fn describes(id: &str) -> String {
    match id {
        "email" => m!("channel_email"),
        "ticket_url" => m!("channel_ticket_url"),
        _ => m!("channel_none"),
    }
}

pub fn channels_json() -> Value {
    json!(REPLY_CHANNELS.iter().map(|id| json!({"id": id, "describes": describes(id)}))
        .collect::<Vec<_>>())
}

/// Reject a reply channel the client cannot honour, before the user is asked to
/// pick it.
pub fn usable(offered: &Value) -> Vec<String> {
    offered.as_array().into_iter().flatten()
        .filter_map(|v| v.as_str())
        .filter(|c| channel_known(c))
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vendor may only choose a channel this client can honour — the same
    /// rule as the action vocabulary, for the same reason.
    #[test]
    fn filters_channels_the_client_cannot_honour() {
        let offered = json!(["email", "carrier_pigeon", "agent_callback", "ticket_url"]);
        assert_eq!(usable(&offered), vec!["email", "ticket_url"]);
    }

    /// "No reply" is a legitimate promise. Silence is not.
    #[test]
    fn none_is_a_valid_channel() {
        assert!(channel_known("none"));
        assert_eq!(usable(&json!(["none"])), vec!["none"]);
    }

    #[test]
    fn an_invented_channel_is_never_usable() {
        assert!(!channel_known("install_our_helper"));
        assert!(usable(&json!(["install_our_helper"])).is_empty());
    }
}
