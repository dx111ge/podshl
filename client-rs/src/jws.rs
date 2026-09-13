//! RFC 7515 detached JWS with EdDSA — the verification half only.
//!
//! The client never signs anything, so there is deliberately no signing code
//! here: a client that cannot produce signatures cannot be tricked into
//! producing one.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::Value;

use crate::jcs;

#[derive(Debug)]
pub struct Verified {
    pub protected: Value,
}

pub fn b64u(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

fn b64u_decode(s: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD.decode(s).map_err(|e| format!("base64url: {e}"))
}

pub fn key_from_jwk(jwk: &Value) -> Result<VerifyingKey, String> {
    if jwk.get("kty").and_then(|v| v.as_str()) != Some("OKP")
        || jwk.get("crv").and_then(|v| v.as_str()) != Some("Ed25519")
    {
        return Err("unsupported JWK: expected OKP/Ed25519".into());
    }
    let x = jwk.get("x").and_then(|v| v.as_str()).ok_or("JWK has no x")?;
    let raw = b64u_decode(x)?;
    let bytes: [u8; 32] = raw.as_slice().try_into().map_err(|_| "JWK x is not 32 bytes")?;
    let key = VerifyingKey::from_bytes(&bytes).map_err(|e| format!("bad public key: {e}"))?;
    // A small-order point is a valid encoding and a worthless key: signatures
    // under it can be produced without any secret, for many messages at once.
    // `verify_strict` below would refuse every signature it checks, but that
    // reads as "signature does not verify" — as if the card had been tampered
    // with. Refused here, where it is, the refusal names what is actually
    // wrong: the key offered as the vendor's is not a key.
    if key.is_weak() {
        return Err("the offered public key has small order — it is not a key anybody holds a secret for".into());
    }
    Ok(key)
}

/// Verify a detached signature over `payload`, canonicalised per RFC 8785.
pub fn verify_detached(jwk: &Value, payload: &Value, sig: &Value) -> Result<Verified, String> {
    let key = key_from_jwk(jwk)?;
    let protected_b64 = sig.get("protected").and_then(|v| v.as_str()).ok_or("no protected header")?;
    let signature_b64 = sig.get("signature").and_then(|v| v.as_str()).ok_or("no signature")?;

    let protected: Value =
        serde_json::from_slice(&b64u_decode(protected_b64)?).map_err(|e| format!("protected: {e}"))?;
    if protected.get("alg").and_then(|v| v.as_str()) != Some("EdDSA") {
        return Err("unsupported alg — only EdDSA is accepted".into());
    }

    let signing_input = format!("{}.{}", protected_b64, b64u(&jcs::canonicalize(payload)?));
    let raw = b64u_decode(signature_b64)?;
    let sig_bytes: [u8; 64] = raw.as_slice().try_into().map_err(|_| "signature is not 64 bytes")?;

    // verify_strict rejects small-order public keys; the permissive variant is
    // not appropriate for a trust boundary.
    key.verify_strict(signing_input.as_bytes(), &Signature::from_bytes(&sig_bytes))
        .map_err(|_| "signature does not verify".to_string())?;
    Ok(Verified { protected })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// The interop test that matters: a card signed by the Python vendor side
    /// must verify here. A canonicalisation that disagrees by one byte reports
    /// "signature invalid" on a perfectly good card, which is the worst
    /// possible failure mode — it looks like an attack.
    #[test]
    fn verifies_a_card_signed_by_the_python_vendor() {
        let (jwk, body, sig) = fixture();
        let v = verify_detached(&jwk, &body, &sig).expect("card must verify");
        assert_eq!(v.protected["org"], "ACME Components GmbH");
    }

    /// A single changed byte must break the signature. Its own case, because a
    /// rejection folded into the acceptance test is a rejection nobody checks.
    #[test]
    fn rejects_the_same_card_after_one_byte_changes() {
        let (jwk, body, sig) = fixture();
        let mut tampered = body;
        tampered["name"] = Value::String("EVIL Corp".into());
        assert!(verify_detached(&jwk, &tampered, &sig).is_err(), "tampered card accepted");
    }

    /// D6: a card carrying no signature at all is refused as untrusted. It is
    /// not the same as no card — that is "nobody there" — and the difference is
    /// the whole of `unknown != blocked`.
    #[test]
    fn a_card_without_a_signature_cannot_be_verified() {
        let (jwk, body, _) = fixture();
        for absent in [json!({}), json!({"protected": "", "signature": ""})] {
            assert!(
                verify_detached(&jwk, &body, &absent).is_err(),
                "a card with no usable signature was accepted"
            );
        }
    }

    /// D9: a different vendor's key does not verify this vendor's card. The
    /// signature has to bind to *whose* it is, not merely to being well formed.
    #[test]
    fn another_vendors_key_does_not_verify() {
        let (_, body, sig) = fixture();
        // A syntactically perfect Ed25519 JWK that simply is not theirs.
        let other = json!({"kty": "OKP", "crv": "Ed25519",
                           "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"});
        assert!(
            verify_detached(&other, &body, &sig).is_err(),
            "a card verified against a key that did not sign it"
        );
    }

    /// D10: a protected header declaring another algorithm is refused rather
    /// than negotiated. Algorithm agility at a trust boundary is how you get
    /// talked down to something weaker.
    #[test]
    fn a_header_declaring_another_algorithm_is_refused() {
        let (jwk, body, sig) = fixture();
        for alg in ["HS256", "none", "RS256", "ES256"] {
            let header = b64u(
                serde_json::to_string(&json!({"alg": alg, "kid": "x"}))
                    .unwrap()
                    .as_bytes(),
            );
            let forged = json!({"protected": header, "signature": sig["signature"].clone()});
            let err = verify_detached(&jwk, &body, &forged).unwrap_err();
            assert!(
                err.contains("EdDSA") || err.contains("alg"),
                "alg {alg} was refused for the wrong reason: {err}"
            );
        }
    }

    /// D11: a small-order public key offered as a vendor's key is refused, and
    /// refused as a bad key rather than as a bad signature.
    ///
    /// The encodings are the canonical small-order points — the identity, the
    /// point of order two, and the two of order four — which decode perfectly
    /// well and which no secret key produces. A key a signature can be forged
    /// under is not a trust anchor, whatever it verifies.
    #[test]
    fn a_small_order_public_key_is_refused() {
        let (_, body, sig) = fixture();
        let mut identity = [0u8; 32];
        identity[0] = 1;
        let mut order_two = [0u8; 32];
        order_two[0] = 0xec;
        for b in order_two.iter_mut().take(31).skip(1) {
            *b = 0xff;
        }
        order_two[31] = 0x7f;
        let order_four = [0u8; 32];
        for (name, raw) in [("identity", identity), ("order 2", order_two), ("order 4", order_four)] {
            let jwk = json!({"kty": "OKP", "crv": "Ed25519", "x": b64u(&raw)});
            let err = verify_detached(&jwk, &body, &sig).unwrap_err();
            assert!(err.contains("small order"), "{name}: refused for the wrong reason: {err}");
        }
        // And a real key is not caught by it, or this is an outage.
        let (jwk, body, sig) = fixture();
        verify_detached(&jwk, &body, &sig).expect("the vendor's real key was refused as weak");
    }

    /// R2: the same rules apply to a remedy. A finding is acted on, so it is
    /// verified exactly as strictly as the card that introduced the vendor.
    #[test]
    fn a_remedy_is_held_to_the_same_rules_as_the_card() {
        let (jwk, body, sig) = fixture();
        let mut remedy = body;
        remedy["findings"] = json!([{"id": "x", "severity": "high", "summary": "s"}]);
        assert!(
            verify_detached(&jwk, &remedy, &sig).is_err(),
            "a modified payload verified against the original signature"
        );
    }

    /// The fixture is a card actually signed by the Python vendor. It used to
    /// be read from a file that nothing generates, and the test returned early
    /// when it was missing — so on a fresh checkout the interop test that
    /// matters most reported green having verified nothing. It now fetches the
    /// card from the running counterparty and says so when it cannot.
    fn fixture() -> (Value, Value, Value) {
        let path = std::path::Path::new("../var/test_card.json");
        let card: Value = match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).expect("../var/test_card.json is not JSON"),
            Err(_) => {
                let fetched = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async {
                        reqwest::get("http://127.0.0.1:8721/.well-known/agent-card.json")
                            .await?
                            .json::<Value>()
                            .await
                    })
                    .expect(
                        "no ../var/test_card.json and the vendor is not answering on :8721 — \
start the counterparty with `mise run services`. This test must never pass \
without a real signed card to check.",
                    );
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(path, serde_json::to_string_pretty(&fetched).unwrap());
                fetched
            }
        };
        let trust: Value = serde_json::from_str(
            &std::fs::read_to_string("../var/ans_stub.json")
                .expect("../var/ans_stub.json is missing — it is the out-of-band key source"),
        )
        .expect("../var/ans_stub.json is not JSON");
        let jwk = trust
            .get("127.0.0.1")
            .expect("no key for 127.0.0.1 in the trust stub")
            .clone();
        let sig = card["signatures"][0].clone();
        assert!(!sig.is_null(), "the fixture card carries no signature");
        let mut body = card;
        body.as_object_mut().unwrap().remove("signatures");
        (jwk, body, sig)
    }
}
