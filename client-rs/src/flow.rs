//! The order of the diagnosis, separated from the window that drives it.
//!
//! Discovery, collection and diagnosis were written as Tauri commands taking
//! the app's state, which meant the only way to reach them was to run the
//! application and click. So the suite tested a second implementation in
//! another language instead — which then drifted from this one, silently,
//! because nothing compared them.
//!
//! Everything here takes what it needs as arguments and returns what it found.
//! The commands in `main.rs` are wrappers that unlock the mutex and call in.
//! That is the whole change: the logic did not move house, it just stopped
//! requiring a graphical session to be observed.

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{a2a, identity, jcs, jws, probes, reads, redact, trust, wire};

#[derive(Serialize, Clone, Debug)]
pub struct Discovery {
    pub ok: bool,
    /// Distinguishes "nobody is there" from "do not trust this".
    pub no_agent: bool,
    pub reason: String,
    pub org: String,
    pub lei: String,
    pub kid: String,
    pub key_source: String,
    pub url: String,
    pub skills: Vec<String>,
}

/// The host an out-of-band key is looked up under. The port is stripped: a
/// trust anchor is per host, not per endpoint.
///
/// A literal IPv6 address is bracketed in a URL — `http://[::1]:8721` — and
/// splitting that on the first colon yields `[`, which is not a host and is
/// not loopback either. The brackets are kept here, because they are how the
/// address is written in a URL, and `a2a::is_loopback` strips them to parse
/// it. Everything else is the authority up to the first colon.
pub fn host_of(base: &str) -> String {
    let authority = base
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("");
    match authority.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((inside, _)) => format!("[{inside}]"),
        None => authority.split(':').next().unwrap_or("").to_string(),
    }
}

/// Fetch the card, resolve the key out of band, verify. Returns the verified
/// key alongside the result so the caller can hold it for the rest of the
/// session — a remedy is checked against the same key that proved the card.
pub async fn discover(
    base: &str,
    resolver: &trust::Resolver,
) -> Result<(Discovery, Option<Value>), String> {
    let host = host_of(base);

    let card = match a2a::fetch_card(base).await {
        Ok(c) => c,
        Err(a2a::DiscoveryError::NoAgent(w)) => {
            return Ok((
                Discovery {
                    ok: false,
                    no_agent: true,
                    reason: w,
                    org: host,
                    lei: String::new(),
                    kid: String::new(),
                    key_source: String::new(),
                    url: base.to_string(),
                    skills: vec![],
                },
                None,
            ))
        }
        Err(e) => return Err(e.to_string()),
    };

    // The key comes from outside the card; verifying it against a key it
    // carries itself would prove only internal consistency.
    let jwk = resolver.jwk_for(&host).ok_or_else(|| {
        a2a::untrusted(m!("no_oob_key", host = host, source = resolver.source())).to_string()
    })?;

    let sigs = card.get("signatures").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let sig = sigs
        .first()
        .ok_or_else(|| a2a::untrusted(m!("card_unsigned")).to_string())?;
    let mut body = card.clone();
    if let Some(o) = body.as_object_mut() {
        o.remove("signatures");
    }

    let verified = jws::verify_detached(&jwk, &body, sig).map_err(|e| a2a::untrusted(e).to_string())?;

    let p = &verified.protected;
    let get = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok((
        Discovery {
            ok: true,
            no_agent: false,
            reason: String::new(),
            org: get("org"),
            lei: get("lei"),
            kid: get("kid"),
            key_source: resolver.source(),
            url: base.to_string(),
            skills: card
                .get("skills")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.get("id").and_then(|i| i.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        },
        Some(jwk),
    ))
}

pub async fn triage(base: &str, problem: &str, lang: &str) -> Result<Value, String> {
    let res = a2a::send_message(
        base,
        json!({ "kind": "triage", "problem": problem, "lang": lang, "context": {} }),
    )
    .await?;

    // A skill the client cannot make sense of is the vendor's problem, and it
    // should be named as such here rather than surfacing as an empty probe list
    // after the user has already been asked to allow a read. No skill at all is
    // a different and perfectly good answer, so it passes through untouched.
    if let Some(skill) = res.get("skill").filter(|s| !s.is_null()) {
        serde_json::from_value::<wire::Skill>(skill.clone())
            .map_err(|e| m!("skill_malformed", e = e))?;
    }
    Ok(res)
}

/// Why a reading was refused, as a word the window can translate — sorted by
/// which message the refusal is, never by the words inside it. It used to look
/// for German words, so the sorting and the sentence could not be changed
/// apart: reword a refusal and it silently became `invalid`.
pub fn refusal_kind(reason: &str) -> &'static str {
    use crate::msg::is;
    let any = |codes: &[&str]| codes.iter().any(|c| is(c, reason));
    if any(&["tool_absent"]) {
        "absent"
    } else if any(&["denied", "denied_even_with_consent", "key_denied", "program_never_run",
                    "gpu_field_identifying", "registry_name_identifying", "env_var_not_allowed"]) {
        "denied"
    } else if any(&["system_program"]) {
        "system"
    } else if any(&["path_backstep", "path_outside", "path_relative_no_project", "glob_path_change"]) {
        "outside"
    } else {
        "invalid"
    }
}

/// Phase one: say what would be read, and refuse anything out of bounds BEFORE
/// asking. A user must never be offered a choice the client would decline
/// anyway — that trains people to click through.
pub fn plan_reads(probes_in: &[Value]) -> Value {
    let machine: Vec<&Value> = probes_in
        .iter()
        .filter(|p| p.get("kind").and_then(|v| v.as_str()) == Some("machine"))
        .collect();

    if machine.len() > reads::MAX_READS {
        return json!({
            "ok": false,
            "reason": m!("too_many_reads", n = machine.len(), max = reads::MAX_READS)
        });
    }

    let mut plan = Vec::new();
    for p in machine {
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let why = p.get("why").and_then(|v| v.as_str()).unwrap_or_default();
        let describes = p.get("describes").and_then(|v| v.as_str()).unwrap_or(id);
        let Some(read) = p.get("read").filter(|v| !v.is_null()) else {
            // Derived facts need no reading of their own.
            plan.push(json!({ "id": id, "describes": describes, "why": why, "derived": true }));
            continue;
        };
        match reads::precheck(read).and_then(|_| reads::describe(read)) {
            // `what` is the client's own sentence, a message from the language
            // files the window says again in the user's language — the consent
            // screen is the one text the user actually decides on, and it was
            // German on an English screen. The instruction and the resolved
            // program travel beside it.
            Ok((what, risk)) => plan.push(json!({
                "id": id, "describes": describes, "why": why,
                "what": what, "risk": risk.label(), "risk_id": risk.id(),
                "read": read,
                "resolved": read.get("program").and_then(|p| p.as_str())
                    .and_then(reads::locate_program)
                    .map(|p| reads::display_path(&p)),
                "refused": false
            })),
            Err(e) => plan.push(json!({
                "id": id, "describes": describes, "why": why,
                "what": e, "risk": "refused", "refused": true,
                "refused_kind": refusal_kind(&e)
            })),
        }
    }
    json!({ "ok": true, "plan": plan, "os": probes::os_id() })
}

/// Phase two: only ever after the user allowed it. Refused instructions are not
/// executed even if they somehow reach here. `allow` lists the probe ids the
/// user actually permitted — consent is per item rather than all-or-nothing,
/// and anything withheld simply becomes a question, never a dead end.
pub fn perform_reads(probes_in: &[Value], allow: &[String]) -> Value {
    let mut facts = serde_json::Map::new();
    let mut missing = Vec::new();
    // **The cap is enforced here too, not only in `plan_reads`.** These are two
    // separately invokable commands with no state tying one to the other, so a
    // stale plan — or a direct invoke — ran every reading a manifest declared
    // however many there were. A limit that holds only on the path that asks
    // permission is not a limit on what gets read.
    let machine: Vec<&Value> = probes_in
        .iter()
        .filter(|p| p.get("kind").and_then(|v| v.as_str()) == Some("machine"))
        .take(reads::MAX_READS)
        .collect();
    for p in machine {
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        if !allow.iter().any(|a| *a == id) {
            missing.push(id);
            continue;
        }
        let value = match p.get("read").filter(|v| !v.is_null()) {
            Some(r) if reads::precheck(r).is_ok() => reads::perform(r),
            _ => None,
        };
        match value {
            Some(v) => {
                facts.insert(id, v);
            }
            None => missing.push(id),
        }
    }
    // Derived facts are computed here rather than asked for, so a vendor cannot
    // assert an interpretation of readings it did not perform.
    for p in probes_in.iter() {
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if !facts.contains_key(id) {
            if let Some(v) = reads::derive(id, &facts) {
                facts.insert(id.to_string(), v);
                missing.retain(|m| m != id);
            }
        }
    }
    json!({ "facts": facts, "missing": missing, "os": probes::os_id() })
}

/// A remedy that does not verify is not acted on, ever — and it is checked
/// against the key that proved the card, not against anything the response
/// carries with it.
pub async fn diagnose(
    base: &str,
    skill_id: &str,
    facts: &Value,
    jwk: &Value,
    lang: &str,
) -> Result<Value, String> {
    // The floor under every fact that leaves: a reading is whatever a program
    // printed, and what it printed can name the account or the host.
    let facts = redact::anonymise_value(facts);
    // What this request is, so that the answer can be held to it. A signed
    // remedy proved who wrote it and nothing about *when* or *for what*: a
    // vendor's answer to somebody else's readings last month verified just as
    // well, and a party on the wire could replay one. The nonce is fresh per
    // request; the hash binds the answer to exactly these facts.
    let nonce = fresh_nonce();
    let facts_sha256 = sha256_hex(&jcs::canonicalize(&facts).map_err(|e| m!("facts_not_canonical", e = e))?);
    // The language travels with the diagnosis as it does with triage. Without
    // it the vendor could not know which language a finding should be in, and
    // the walk through the window got a German finding on an English screen.
    let res = a2a::send_message(
        base,
        json!({ "kind": "diagnose", "skill_id": skill_id, "facts": facts, "lang": lang,
                "nonce": nonce, "facts_sha256": facts_sha256 }),
    )
    .await?;
    accept_remedy(&res, jwk, skill_id, &nonce, &facts_sha256)
}

/// Sixteen random bytes, as hex. Enough that nobody guesses one, small
/// enough to read in a log.
fn fresh_nonce() -> String {
    rand::random::<[u8; 16]>().iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The vendor's answer, accepted only if it is *this* request's answer.
///
/// Verified against the key that proved the card, held to the published
/// shape, and then held to the request: the same skill, the same nonce, the
/// same facts. A remedy that fails the last of those is signed by the right
/// vendor and is still not an answer to what was asked.
pub fn accept_remedy(res: &Value, jwk: &Value, skill_id: &str, nonce: &str, facts_sha256: &str)
    -> Result<Value, String> {
    let remedy = res.get("remedy").cloned().ok_or_else(|| m!("no_remedy"))?;
    let sig = res.get("signature").cloned().ok_or_else(|| m!("finding_unsigned"))?;
    jws::verify_detached(jwk, &remedy, &sig)
        .map_err(|e| m!("finding_signature_invalid", e = e))?;

    // Signed is not the same as well formed. A remedy that verifies but does not
    // match the published shape used to reach the window and fail there, as a
    // missing key somewhere in the middle of a consent flow. Checked here, the
    // vendor is named as the source of the problem.
    //
    // The original is what travels onward: parsing is for validation, not for
    // rewriting. Round-tripping would silently drop any field a vendor added
    // after this client was built, which is the one thing the wire format is
    // deliberately permissive about.
    serde_json::from_value::<wire::Remedy>(remedy.clone())
        .map_err(|e| m!("finding_malformed", e = e))?;
    // What a step says about the software it is for is checked here too, so a
    // malformed one names the vendor rather than failing at the consent button.
    for step in remedy.get("plan").and_then(|p| p.as_array()).into_iter().flatten() {
        crate::repair::Upstream::from_value(step.get("upstream"))
            .map_err(|e| m!("finding_malformed", e = e))?;
    }

    let carries = |k: &str, want: &str| remedy.get(k).and_then(|v| v.as_str()) == Some(want);
    if !carries("skill_id", skill_id) || !carries("nonce", nonce) || !carries("facts_sha256", facts_sha256) {
        return Err(m!("finding_not_for_this_request", skill = skill_id));
    }

    Ok(json!({ "remedy": remedy, "signature_valid": true }))
}

/// Send a report, and stamp it with the per-vendor per-epoch pseudonym on the
/// way out. The recipient can count, rate-limit and block; it cannot link this
/// client to what it sent anyone else, or to itself next month.
pub async fn send_report(base: &str, mut report: Value, domain: &str, lang: &str)
    -> Result<Value, String> {
    // Free text only under its own consent, naming who receives it — the
    // rule `for_operator` already enforces on the published path. This path
    // validated the shape and sent whatever `description` it was handed.
    if report.get("description").is_some() {
        let consent = report.get("description_consent").cloned().unwrap_or(Value::Null);
        let granted = consent.get("granted") == Some(&Value::Bool(true));
        let named = consent.get("destination").and_then(|d| d.as_str()).map_or(false, |d| !d.trim().is_empty());
        if !(granted && named) {
            return Err(m!("free_text_without_consent"));
        }
    }
    let p = identity::pseudonym(domain)?;
    if let Some(o) = report.as_object_mut() {
        o.insert("pseudonym".into(), Value::String(p));
        o.insert("epoch".into(), Value::String(identity::epoch()));
    }

    // Checked immediately before it leaves, so what is validated is exactly what
    // travels — after the pseudonym and epoch were added, not before. This
    // client must not be the one that sends a vendor something the
    // specification says is not a report.
    serde_json::from_value::<wire::Report>(report.clone())
        .map_err(|e| m!("report_malformed", e = e))?;

    // The language beside the report, not in it: it is how the receipt is
    // worded, and it says nothing about the machine.
    let res = a2a::send_message(base, json!({ "kind": "report", "report": report, "lang": lang })).await?;
    Ok(res.get("receipt").cloned().unwrap_or(json!({})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ACME: &str = "http://127.0.0.1:8721";
    const PLAIN: &str = "http://127.0.0.1:8722";

    fn pinned() -> trust::Resolver {
        trust::Resolver::Pinned(std::path::PathBuf::from("../var/ans_stub.json"))
    }

    /// The counterparty is part of what the client is tested against, so its
    /// absence is a failure with an instruction rather than a silent skip. A
    /// suite that quietly tests nothing is worse than one that is red.
    async fn require_services() {
        assert!(
            reqwest::get(ACME).await.is_ok(),
            "the counterparty is not answering on :8721 — start it with `mise run services`"
        );
    }

    /// D1: the card verifies against a key that came from outside it, and the
    /// identity it carries is surfaced before anything is read.
    #[tokio::test]
    async fn a_signed_card_verifies_and_exposes_its_identity() {
        require_services().await;
        let (d, jwk) = discover(ACME, &pinned()).await.expect("discovery failed");
        assert!(d.ok && !d.no_agent, "the card did not verify: {}", d.reason);
        assert!(!d.lei.is_empty(), "no LEI surfaced");
        assert!(!d.org.is_empty(), "no organisation surfaced");
        assert!(jwk.is_some(), "no key retained for the rest of the session");
        assert!(
            d.key_source.contains("ans_stub.json"),
            "the key source is not stated to the user: {}",
            d.key_source
        );
        assert!(!d.skills.is_empty(), "a verified vendor offered no skills");
    }

    /// D2-D5: every way of there being no agent is "nobody there", never a
    /// trust failure — and each states its own reason. Today this is the normal
    /// case: almost no vendor publishes an agent, and treating absence as
    /// suspicion would be wrong about nearly everyone.
    #[tokio::test]
    async fn every_kind_of_absence_is_nobody_there() {
        require_services().await;

        // A port nothing is listening on, obtained by binding one and letting
        // it go. `127.0.0.1:9` used to stand here on the assumption that the
        // discard port is always dead; on a Windows machine with Simple TCP/IP
        // Services enabled it is very much alive, accepts everything and answers
        // nothing — so this case sat there for ten minutes and passed, which is
        // how a missing timeout in `a2a` survived for the life of the project.
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("no port");
            let p = l.local_addr().unwrap().port();
            drop(l);
            format!("http://127.0.0.1:{p}")
        };

        for (base, why, expect) in [
            (PLAIN.to_string(), "a 404 at the well-known URI", "Agent Card"),
            (dead, "a host that refuses the connection", ""),
            (format!("{PLAIN}/nonjson"), "a 200 that is not a card", "not an Agent Card"),
            (format!("{PLAIN}/broken"), "a 500", "500"),
        ] {
            let (d, jwk) = discover(&base, &pinned())
                .await
                .unwrap_or_else(|e| panic!("{why} was reported as a trust failure: {e}"));
            assert!(d.no_agent, "{why} was not classified as 'nobody there'");
            assert!(!d.ok);
            assert!(jwk.is_none(), "{why} yielded a key");
            assert!(!d.reason.is_empty(), "{why} gave the user no reason");
            if !expect.is_empty() {
                assert!(
                    d.reason.contains(expect),
                    "{why}: reason does not mention {expect:?} — got {:?}",
                    d.reason
                );
            }
        }
    }

    /// A host that accepts and never answers is also "nobody there" — and it
    /// must *say so*, within a bounded time.
    ///
    /// This is the case that was missing, and its absence cost the whole
    /// project a ten-minute test suite on Windows without anybody noticing why.
    /// `a2a` used `reqwest::get`, which builds a client with no timeout at all,
    /// so a stalling host froze the client for good — at the moment the user's
    /// machine is already broken, with no message and nothing to press.
    ///
    /// The stall is served here rather than assumed: a listener that accepts
    /// the connection, reads nothing and holds it open. Windows' discard service
    /// on port 9 does exactly this, which is how it was found.
    #[tokio::test]
    async fn a_host_that_accepts_and_never_answers_is_still_nobody_there() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("no port");
        let port = listener.local_addr().unwrap().port();
        // Accept and hold: never write, never close. The flag is what ends it —
        // sleeping a fixed span here would make the case take that long whatever
        // the client does, which is the same mistake in a smaller form.
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&done);
        let held = std::thread::spawn(move || {
            let mut open = Vec::new();
            for stream in listener.incoming().take(1) {
                if let Ok(s) = stream {
                    open.push(s);
                }
            }
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            drop(open);
        });

        let began = std::time::Instant::now();
        let (d, jwk) = discover(&format!("http://127.0.0.1:{port}"), &pinned())
            .await
            .expect("a stalling host was reported as a trust failure");
        let took = began.elapsed();
        done.store(true, Ordering::Relaxed);

        assert!(d.no_agent, "a host that never answers was not 'nobody there'");
        assert!(!d.ok);
        assert!(jwk.is_none());
        assert!(!d.reason.is_empty(), "the user was given no reason");
        assert!(
            took < std::time::Duration::from_secs(45),
            "discovery waited {took:?} on a host that answers nothing. Without a \\
             timeout it waits for ever, and a spinner is not a statement of absence."
        );
        let _ = held.join();
    }

    /// D8: with no out-of-band key there is no verification, and a card that
    /// asserts its own key is not a substitute. This is a refusal, and it must
    /// be distinct from "nobody there".
    #[tokio::test]
    async fn without_an_out_of_band_key_a_card_is_refused_not_accepted() {
        require_services().await;
        let missing = trust::Resolver::Pinned(std::path::PathBuf::from("../var/does-not-exist.json"));
        let err = discover(ACME, &missing)
            .await
            .expect_err("a self-asserted card was accepted");
        assert!(
            crate::msg::is("no_oob_key", &err),
            "the refusal does not explain itself: {err}"
        );
    }

    /// R1: a matching problem yields a signed remedy, and the signature is
    /// checked against the key that proved the card rather than anything the
    /// response carried with it.
    #[tokio::test]
    async fn a_matching_problem_yields_a_verified_remedy() {
        require_services().await;
        let (_, jwk) = discover(ACME, &pinned()).await.unwrap();
        let jwk = jwk.unwrap();
        let tr = triage(ACME, "training is slow, bf16?", "de").await.unwrap();
        let skill = tr.get("skill").expect("no skill for a problem the vendor handles");
        let skill_id = skill["id"].as_str().unwrap();

        let facts = json!({"gpu.name": "NVIDIA GeForce RTX 2070 SUPER",
                           "gpu.compute_capability": "7.5", "gpu.bf16_native": false});
        let out = diagnose(ACME, skill_id, &facts, &jwk, "en").await.expect("diagnose failed");
        assert_eq!(out["signature_valid"], true);
        assert!(
            !out["remedy"]["findings"].as_array().unwrap().is_empty(),
            "no findings returned for a problem the vendor claims to handle"
        );
    }

    /// LG6: the finding comes in the language the diagnosis asked for, the
    /// same rule as the skill. `diagnose` carried no language, so the walk
    /// through the window got a German finding on an English screen.
    #[tokio::test]
    async fn the_finding_comes_in_the_language_asked_for() {
        require_services().await;
        let (_, jwk) = discover(ACME, &pinned()).await.unwrap();
        let jwk = jwk.unwrap();
        let tr = triage(ACME, "training is slow, bf16?", "en").await.unwrap();
        let skill_id = tr["skill"]["id"].as_str().unwrap().to_string();
        let facts = json!({"gpu.name": "RTX 2070 SUPER", "gpu.compute_capability": "7.5",
                           "gpu.bf16_native": false});
        // What the vendor wrote for each language, filled the way it fills it:
        // the answer has to be that sentence, not merely some sentence.
        let expected = |lang: &str| -> String {
            let raw = std::fs::read_to_string(format!("../src/podshl/vendor/content/{lang}.json"))
                .expect("the demo vendor's content is missing");
            let v: Value = serde_json::from_str(&raw).unwrap();
            v["skills"][&skill_id]["text"]["emulated"].as_str().unwrap()
                .replace("{name}", "RTX 2070 SUPER").replace("{cc}", "7.5")
        };
        for lang in ["en", "de"] {
            let out = diagnose(ACME, &skill_id, &facts, &jwk, lang).await.unwrap();
            let text = out["remedy"]["findings"][0]["summary"].as_str().unwrap().to_string();
            assert_eq!(text, expected(lang), "asked in {lang}, answered in another language");
        }
    }

    /// A remedy is not trusted because it arrived — it is trusted because it
    /// verifies. Against the wrong key, it does not.
    #[tokio::test]
    async fn a_remedy_is_refused_under_a_key_that_did_not_sign_it() {
        require_services().await;
        let tr = triage(ACME, "training is slow, bf16?", "de").await.unwrap();
        let skill_id = tr["skill"]["id"].as_str().unwrap().to_string();
        let other = json!({"kty": "OKP", "crv": "Ed25519",
                           "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"});
        let err = diagnose(ACME, &skill_id, &json!({}), &other, "en")
            .await
            .expect_err("a remedy verified under a foreign key");
        assert!(crate::msg::is("finding_signature_invalid", &err), "{err}");
    }

    /// R5: a signed remedy is the answer to *this* request or it is refused.
    /// The signature proved who wrote it and nothing about what for: an
    /// answer to somebody else's readings, or last month's to these, verified
    /// just as well. The vendor now signs the nonce and the facts hash back;
    /// a remedy that carries another skill, another nonce, or none, is
    /// refused after it verifies.
    #[tokio::test]
    async fn a_remedy_is_accepted_only_as_the_answer_to_the_request_that_asked() {
        require_services().await;
        let (_, jwk) = discover(ACME, &pinned()).await.unwrap();
        let jwk = jwk.unwrap();
        let tr = triage(ACME, "training is slow, bf16?", "en").await.unwrap();
        let skill_id = tr["skill"]["id"].as_str().unwrap().to_string();
        let facts = json!({"gpu.name": "RTX 2070 SUPER", "gpu.compute_capability": "7.5",
                           "gpu.bf16_native": false});
        let nonce = fresh_nonce();
        assert_eq!(nonce.len(), 32);
        let hash = sha256_hex(&jcs::canonicalize(&facts).unwrap());

        let ask = |nonce: Option<&str>| {
            let mut req = json!({"kind": "diagnose", "skill_id": skill_id, "facts": facts, "lang": "en",
                                 "facts_sha256": hash});
            if let Some(n) = nonce {
                req["nonce"] = json!(n);
            }
            a2a::send_message(ACME, req)
        };
        let res = ask(Some(&nonce)).await.expect("the vendor did not answer");
        let out = accept_remedy(&res, &jwk, &skill_id, &nonce, &hash).expect("the vendor's own answer was refused");
        assert_eq!(out["remedy"]["nonce"], nonce, "the vendor did not sign the nonce back");
        assert_eq!(out["remedy"]["facts_sha256"], hash);

        for (why, skill, n, h) in [
            ("another nonce", skill_id.as_str(), fresh_nonce(), hash.clone()),
            ("another skill", "warranty.rma.precheck", nonce.clone(), hash.clone()),
            ("other facts", skill_id.as_str(), nonce.clone(), sha256_hex(b"{}")),
        ] {
            let e = accept_remedy(&res, &jwk, skill, &n, &h).expect_err(&format!("accepted with {why}"));
            assert!(crate::msg::is("finding_not_for_this_request", &e), "{why}: refused for the wrong reason: {e}");
        }
        // And one that carries no nonce at all — a vendor that predates the
        // binding, or an answer replayed from before it.
        let without = ask(None).await.unwrap();
        let e = accept_remedy(&without, &jwk, &skill_id, &nonce, &hash).expect_err("a remedy without a nonce was accepted");
        assert!(crate::msg::is("finding_not_for_this_request", &e), "{e}");
        // The whole path still works end to end, with the binding in it.
        diagnose(ACME, &skill_id, &facts, &jwk, "en").await.expect("diagnose failed with the binding");
    }

    /// X1: a base off this machine is spoken to over TLS or not at all. A
    /// plain-text connection cannot forge a signed remedy, but it carries the
    /// readings — everything the client generalised and anonymised before
    /// transmission — to anybody on the wire. Loopback has no wire.
    #[tokio::test]
    async fn a_plain_http_base_off_this_machine_is_refused() {
        for (base, fine) in [
            ("http://127.0.0.1:8721", true), ("http://localhost:8721", true), ("http://[::1]:8721", true),
            ("http://127.5.5.5", true), ("https://support.example.org", true),
            ("http://support.example.org", false), ("http://10.0.0.5:8721", false),
            ("http://192.168.0.26", false), ("ftp://example.org", false), ("example.org", false),
        ] {
            assert_eq!(a2a::insecure_base(base).is_none(), fine, "{base}");
        }
        let e = discover("http://support.example.org", &pinned()).await
            .expect_err("a plain-text base off this machine was discovered");
        assert!(crate::msg::is("https_required", &e), "{e}");
        let e = triage("http://10.0.0.5:8721", "x", "en").await
            .expect_err("readings were offered to a plain-text base off this machine");
        assert!(crate::msg::is("https_required", &e), "{e}");
    }

    /// T15: free text leaves on the vendor path only under its own consent,
    /// naming who receives it — the rule the published path enforced and
    /// this one did not.
    #[tokio::test]
    async fn a_report_with_free_text_and_no_consent_is_not_sent() {
        let mut forged = json!({"skill_id": "s", "skill_version": "1", "resolved_by": "vendor_skill",
                                "outcome": "resolved", "observed": {}, "stated": {}, "dropped": []});
        forged["description"] = json!("typed but never agreed to");
        let e = send_report(ACME, forged.clone(), "127.0.0.1", "en").await
            .expect_err("free text without its consent was sent");
        assert!(crate::msg::is("free_text_without_consent", &e), "{e}");
        forged["description_consent"] = json!({"granted": true, "destination": "  "});
        assert!(send_report(ACME, forged, "127.0.0.1", "en").await.is_err(), "consent naming nobody was enough");
    }

    /// A signed remedy that does not match the published shape is refused here,
    /// naming the vendor as the source — rather than reaching the window and
    /// failing as a missing key in the middle of a consent flow.
    #[test]
    fn a_remedy_that_verifies_but_is_malformed_is_still_refused() {
        // `skill_id` is required; a remedy without it is not a remedy.
        let malformed = json!({"skill_version": "1.0.0", "findings": []});
        assert!(
            serde_json::from_value::<wire::Remedy>(malformed).is_err(),
            "a remedy with no skill_id parsed"
        );
        // But an unknown field must not be a refusal: extending is allowed.
        let extended = json!({"skill_id": "x", "skill_version": "1",
                              "vendor_specific_2027": {"a": 1}});
        assert!(
            serde_json::from_value::<wire::Remedy>(extended).is_ok(),
            "a vendor extension broke the parse"
        );
    }

    /// S2: a problem the vendor has no skill for routes to a person rather than
    /// producing a shrug. Abstention with a destination.
    #[tokio::test]
    async fn an_unmatched_problem_routes_to_a_human() {
        require_services().await;
        let tr = triage(ACME, "my printer smells odd", "de").await.unwrap();
        assert!(tr.get("skill").map_or(true, |s| s.is_null()), "a skill was invented for an unrelated problem");
        assert!(
            tr.get("escalate").is_some() || tr.get("reason").is_some(),
            "no skill, and no destination either — that is a dead end: {tr}"
        );
    }

    /// P2: a machine probe that cannot answer arms a human probe rather than
    /// failing. Consumer cards really do report no serial, and a warranty check
    /// needs one, so the printed sticker is the only source.
    #[tokio::test]
    async fn an_unreadable_machine_fact_arms_a_question() {
        require_services().await;
        let tr = triage(ACME, "graphics card defect, warranty?", "de").await.unwrap();
        let probes_in: Vec<Value> =
            tr["skill"]["probes"].as_array().cloned().expect("no probes on the RMA skill");
        let gated: Vec<&Value> = probes_in
            .iter()
            .filter(|p| p.get("when_missing").map_or(false, |v| !v.is_null()))
            .collect();
        assert!(!gated.is_empty(), "nothing is gated on a machine probe coming back empty");
        for g in &gated {
            let on = g["when_missing"].as_str().unwrap();
            assert!(
                probes_in.iter().any(|p| p["id"] == on),
                "a question is gated on {on}, which the skill never tries to read"
            );
        }
    }

    /// The whole report path, end to end against the real vendor: build it,
    /// stamp it, validate it, send it, get a receipt. Nothing exercised this
    /// before — the validation added on the way out could have refused every
    /// report this client produces and no test would have noticed.
    #[tokio::test]
    async fn a_report_is_built_stamped_and_accepted() {
        require_services().await;
        let tr = triage(ACME, "training is slow, bf16?", "de").await.unwrap();
        let skill = tr["skill"].clone();
        let facts = json!({"gpu.name": "NVIDIA GeForce RTX 2070 SUPER",
                           "gpu.compute_capability": "7.5"});
        let (built, _held) = crate::report::build(&skill, &facts, &[], &[], "general_agent", "resolved");

        let receipt = send_report(ACME, built, "127.0.0.1", "en").await.expect("the report was refused");
        assert!(
            receipt.get("state").is_some(),
            "a report was accepted without a receipt — the receipt is the entire reward: {receipt}"
        );
    }

    /// A report that does not match the published schema never leaves, and the
    /// refusal says so rather than letting the vendor reject it.
    #[tokio::test]
    async fn a_malformed_report_is_not_sent() {
        require_services().await;
        let err = send_report(ACME, json!({"observed": {}}), "127.0.0.1", "en")
            .await
            .expect_err("a report with no skill_id was sent");
        assert!(crate::msg::is("report_malformed", &err), "{err}");
    }

    /// C1: withholding consent withholds the read. Consent is per item, so this
    /// is not all-or-nothing — and what is withheld becomes a question rather
    /// than a dead end.
    ///
    /// Two probes, one granted. Both halves used to be `nvidia-smi`, which
    /// made the case say "a GPU is present" as well as what it meant to say:
    /// on a machine without the tool the granted path read nothing and the
    /// case failed for a reason that has nothing to do with consent. The fact
    /// that is granted here is one every machine can answer.
    #[test]
    fn what_is_not_consented_to_is_not_read() {
        let probes_in = vec![
            json!({
                "id": "gpu.name", "kind": "machine",
                "read": {"op": "run_tool", "tool": "nvidia-smi",
                         "args": ["--query-gpu=name", "--format=csv,noheader"]}
            }),
            json!({
                "id": "os.version", "kind": "machine",
                "read": {"op": "os_fact", "name": "version"}
            }),
        ];

        let refused = perform_reads(&probes_in, &[]);
        assert!(
            refused["facts"].as_object().unwrap().is_empty(),
            "a reading happened without consent"
        );
        let missing: Vec<&str> = refused["missing"].as_array().unwrap().iter()
            .filter_map(|m| m.as_str()).collect();
        assert!(missing.contains(&"gpu.name") && missing.contains(&"os.version"),
                "a withheld fact was not declared missing: {missing:?}");

        let granted = perform_reads(&probes_in, &["os.version".to_string()]);
        assert!(
            granted["facts"].get("os.version").is_some(),
            "consent was given and nothing was read — the two paths must differ"
        );
        assert!(
            granted["facts"].get("gpu.name").is_none(),
            "consent to one reading was taken as consent to the other"
        );
    }

    /// C2: planning says what *would* be read and changes nothing. A plan that
    /// acts is not a plan, and the user is deciding on the strength of it.
    #[test]
    fn planning_a_read_does_not_perform_it() {
        let probes_in = vec![json!({
            "id": "os.version", "kind": "machine", "describes": "OS", "why": "w",
            "read": {"op": "os_fact", "name": "version"}
        })];
        let plan = plan_reads(&probes_in);
        assert_eq!(plan["ok"], true);
        assert_eq!(plan["plan"][0]["id"], "os.version");
        assert_eq!(plan["plan"][0]["refused"], false);
        assert!(
            plan["plan"][0].get("value").is_none() && plan.get("facts").is_none(),
            "the plan carries a value — it read the machine before anyone agreed"
        );
    }

    /// An instruction the client would refuse is shown as refused *before* the
    /// user is asked. Offering a choice that would be declined anyway trains
    /// people to click through.
    #[test]
    fn an_out_of_bounds_read_is_refused_in_the_plan_not_after_it() {
        let probes_in = vec![json!({
            "id": "secrets", "kind": "machine", "describes": "ssh keys", "why": "w",
            "read": {"op": "read_file_key", "path": ".ssh/id_ed25519", "key": "x"}
        })];
        let plan = plan_reads(&probes_in);
        assert_eq!(plan["plan"][0]["refused"], true, "a denied path was offered to the user");

        let out = perform_reads(&probes_in, &["secrets".to_string()]);
        assert!(
            out["facts"].as_object().unwrap().is_empty(),
            "a denied read ran because the user said yes — consent cannot unlock it"
        );
    }

    /// Every way the client refuses a reading has a kind the window can put in
    /// the user's language. The sentences are the client's own German, and an
    /// English consent screen showed them verbatim.
    #[test]
    fn every_refusal_has_a_kind_the_window_can_translate() {
        let _g = crate::reads::grants_held();
        for (read, kind) in [
            (json!({"op": "run_tool", "tool": "podshl-nonexistent-tool"}), "invalid"),
            (json!({"op": "read_file_key", "path": ".ssh/config", "key": "k"}), "denied"),
            (json!({"op": "program_version", "program": "bash"}), "denied"),
            (json!({"op": "read_ini_key", "path": "project/../../.npmrc", "key": "k"}), "outside"),
            (json!({"op": "read_ini_key", "path": ".venv/pyvenv.cfg", "key": "version"}), "outside"),
            (json!({"op": "program_version", "program": "engram", "flag": "-c"}), "invalid"),
            (json!({"op": "run_powershell"}), "invalid"),
        ] {
            crate::reads::end_incident();
            let e = reads::precheck(&read).expect_err("expected a refusal");
            assert_eq!(refusal_kind(&e), kind, "{read} → {e}");
        }
        // Absence of a tool is its own kind wherever the tool is absent.
        if let Err(e) = reads::precheck(&json!({"op": "run_tool", "tool": "system_profiler",
                                                "args": ["SPDisplaysDataType"]})) {
            assert_eq!(refusal_kind(&e), "absent", "{e}");
        }
    }

    /// More readings than a diagnosis can justify is refused as a whole. There
    /// is no per-item consent that makes enumerating a machine acceptable.
    #[test]
    fn an_unreasonable_number_of_reads_is_refused_outright() {
        let probes_in: Vec<Value> = (0..reads::MAX_READS + 1)
            .map(|i| json!({"id": format!("x{i}"), "kind": "machine",
                            "read": {"op": "os_fact", "name": "version"}}))
            .collect();
        let plan = plan_reads(&probes_in);
        assert_eq!(plan["ok"], false, "{} reads were accepted", probes_in.len());
        assert!(crate::msg::is("too_many_reads", plan["reason"].as_str().unwrap()));
    }

    /// The trust anchor is per host, not per endpoint — so a port never selects
    /// a different key, and a path never does either.
    #[test]
    fn the_key_is_looked_up_by_host_alone() {
        assert_eq!(host_of("http://127.0.0.1:8721"), "127.0.0.1");
        assert_eq!(host_of("https://support.example.org/a2a"), "support.example.org");
        assert_eq!(host_of("https://example.org:443/x/y"), "example.org");
    }
}
