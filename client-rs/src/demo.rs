//! The whole argument, executed rather than described.
//!
//!     cargo run --release -- demo
//!
//! Five acts. The first is the important one: it shows what happens with a
//! vendor that has no A2A presence — which is every vendor today, and therefore
//! the reason the dark matter exists at all.
//!
//! This ran against a second implementation of the client, in Python. It now
//! drives the binary that ships, which is the only version of the argument that
//! proves anything: every consent, every bound and every refusal below is the
//! one a user would actually meet.

use serde_json::{json, Value};

use crate::{actions, flow, ledger, reads, report, trust};

const ACME: &str = "http://127.0.0.1:8721";
const PLAIN: &str = "http://127.0.0.1:8722";
const INDEX: &str = "http://127.0.0.1:8723";

const B: &str = "\x1b[1m";
const D: &str = "\x1b[2m";
const R: &str = "\x1b[0m";

fn act(n: u8, title: &str) {
    println!("\n{B}{}\n  ACT {n}  ·  {title}\n{}{R}", "─".repeat(76), "─".repeat(76));
}

fn beat(text: &str) {
    println!("\n{B}▸ {text}{R}");
}

fn dim(text: &str) {
    println!("{D}   {text}{R}");
}

fn pinned() -> trust::Resolver {
    trust::Resolver::Pinned(std::path::PathBuf::from("../var/ans_stub.json"))
}

pub async fn run() -> Result<(), String> {
    if reqwest::get(ACME).await.is_err() {
        return Err(
            "The counterparty is not answering on :8721 — start it with `mise run services`."
                .into(),
        );
    }

    // ───────────────────────────────────────────────────────────── act 1
    act(1, "Discovery — and the vendor who is not there at all");

    beat("The user has a problem, and their agent looks for the vendor.");
    dim(&format!("GET {PLAIN}/.well-known/agent-card.json"));
    let (dark, _) = flow::discover(PLAIN, &pinned()).await?;
    println!("   {}", dark.reason);
    dim("no_agent = true — this is NOT a trust problem, it is absence.");
    dim("Confusing the two would be the mistake: \"nobody there\" and \"do not trust\"");
    dim("lead to opposite behaviour.");

    beat("And this is exactly where the dark matter comes from.");
    println!("   The user solves it alone or not at all. The vendor never learns");
    println!("   that it happened — not that their documentation leads to the wrong");
    println!("   configuration, not how many people met the same thing.");

    beat("The same flow against a vendor that publishes an Agent Card.");
    let (found, jwk) = flow::discover(ACME, &pinned()).await?;
    let jwk = jwk.ok_or("no key despite a verified card")?;
    println!("   {} · LEI {}", found.org, found.lei);
    dim(&format!("key from: {}", found.key_source));
    dim("The key did NOT come from the card. Checking a card against its own");
    dim("key proves only that it agrees with itself.");

    // ───────────────────────────────────────────────────────────── act 2
    act(2, "The vendor path — skill, consent, bounded action");

    beat("The vendor names the matching skill and what it needs read.");
    let tr = flow::triage(ACME, "training is slow, bf16?", "en").await?;
    let skill = tr.get("skill").cloned().ok_or("no skill")?;
    let skill_id = skill["id"].as_str().unwrap_or_default().to_string();
    println!(
        "   {} v{}",
        skill["title"].as_str().unwrap_or("?"),
        skill["version"].as_str().unwrap_or("?")
    );

    let probes: Vec<Value> = skill["probes"].as_array().cloned().unwrap_or_default();
    beat("Before reading: what would be read, what for, and at what risk.");
    let plan = flow::plan_reads(&probes);
    for p in plan["plan"].as_array().unwrap() {
        let refused = p["refused"] == json!(true);
        println!(
            "   {} {}  {}",
            if refused { "✗" } else { "·" },
            p["describes"].as_str().unwrap_or("?"),
            p.get("what").and_then(|v| v.as_str()).unwrap_or("(derived)")
        );
        dim(&format!("  because: {}", p["why"].as_str().unwrap_or("")));
    }
    dim("Refused instructions are refused here already — not after the user");
    dim("has agreed. Otherwise people learn to click through.");

    beat("Only then is anything read, and only what was allowed item by item.");
    let allow: Vec<String> = probes
        .iter()
        .filter(|p| p["kind"] == json!("machine"))
        .filter_map(|p| p["id"].as_str().map(String::from))
        .collect();
    let collected = flow::perform_reads(&probes, &allow);
    for (k, v) in collected["facts"].as_object().unwrap() {
        println!("   {k} = {v}");
    }
    if let Some(missing) = collected["missing"].as_array() {
        if !missing.is_empty() {
            dim(&format!("not readable: {missing:?} — that becomes a question, not a dead end"));
        }
    }

    beat("The finding comes from the vendor, signed, and is checked.");
    let out = flow::diagnose(ACME, &skill_id, &collected["facts"], &jwk, "en").await?;
    let remedy = &out["remedy"];
    for f in remedy["findings"].as_array().unwrap() {
        println!("   [{}] {}", f["severity"].as_str().unwrap_or("?"), f["summary"].as_str().unwrap_or(""));
        for e in f["evidence"].as_array().into_iter().flatten() {
            dim(&format!("  · {}", e.as_str().unwrap_or("")));
        }
    }
    dim("signature_valid = true — against the same key as the card.");

    beat("The plan is made of actions this client knows. First the dry run.");
    for call in remedy["plan"].as_array().into_iter().flatten() {
        let id = call["action"].as_str().unwrap_or_default();
        let params = &call["params"];
        match actions::dry_run(id, params) {
            Ok(preview) => {
                println!("   {id}: {preview}");
                dim(&format!("  because: {}", call["because"].as_str().unwrap_or("")));
            }
            Err(e) => println!("   {id}: refused — {e}"),
        }
    }
    dim("The vendor chooses from this vocabulary. It cannot bring a capability of its own.");

    // ───────────────────────────────────────────────────────────── act 3
    act(3, "The report button — the user asks, not the vendor");

    beat("First: has this vendor ever repaid the effort?");
    let standing_file = format!("../var/demo_standing_{}.json", std::process::id());
    let mut led = ledger::Ledger::open(&standing_file);
    // Four is the floor below which no rate is quoted at all; the demo has to be
    // above it or act 4 shows an empty list and proves nothing.
    for state in ["fixed_in", "known", "received", "fixed_in"] {
        led.record("ACME Components GmbH", state)?;
    }
    let standing = led.standing("ACME Components GmbH");
    let (offer, note) = standing.advice(None);
    println!("   {note}");
    dim(&format!("offer the button: {offer}"));

    beat("And the user sees beforehand what would be reported — and what would NOT.");
    let (built, held) = report::build(&skill, &collected["facts"], &[], &[], "vendor_skill", "resolved");
    for (k, v) in built["observed"].as_object().unwrap() {
        println!("   {k} = {v}");
    }
    if !held.is_empty() {
        dim(&format!("held back: {}", held.join(", ")));
    }
    dim("No timestamp, no identifier. A report must not lead back to the run");
    dim("it came from.");

    // ───────────────────────────────────────────────────────────── act 4
    act(4, "Aggregation — the index no single vendor can compute");

    beat("A vendor that never acts loses the button — with the reason stated.");
    let mut silent = ledger::Ledger::open(format!("../var/demo_silent_{}.json", std::process::id()));
    for _ in 0..8 {
        silent.record("Schweiger AG", "received")?;
    }
    let (offer, note) = silent.standing("Schweiger AG").advice(None);
    println!("   {note}");
    dim(&format!("offer the button: {offer}"));
    dim("Self-enforcing, with no police: the measurement runs both ways.");

    beat("The network median outranks one's own experience — but shows both.");
    match ledger::network("ACME Components GmbH", INDEX).await {
        Some(n) if n["published"] == json!(true) => println!("   {n}"),
        Some(_) => dim("Too few independent contributions to publish yet."),
        None => dim("Index not reachable — local experience applies, and says so."),
    }
    for c in led.pending_contributions() {
        println!("   {c}");
    }
    dim("One vendor per request. The list of vendors would be a picture");
    dim("of the installed software — it never leaves in one piece.");

    // ───────────────────────────────────────────────────────────── act 5
    act(5, "Negative controls");

    beat("A card without an out-of-band key is not accepted.");
    let missing = trust::Resolver::Pinned(std::path::PathBuf::from("../var/does-not-exist.json"));
    match flow::discover(ACME, &missing).await {
        Err(e) => println!("   ✓ {e}"),
        Ok(_) => println!("   ✗ a self-asserted card was accepted"),
    }

    beat("A finding under a foreign key is not accepted.");
    let other = json!({"kty": "OKP", "crv": "Ed25519",
                       "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"});
    match flow::diagnose(ACME, &skill_id, &json!({}), &other, "en").await {
        Err(e) => println!("   ✓ {e}"),
        Ok(_) => println!("   ✗ a finding verified under a foreign key"),
    }

    beat("An action outside the vocabulary is refused.");
    match actions::dry_run("run_powershell", &json!({})) {
        Err(e) => println!("   ✓ {e}"),
        Ok(_) => println!("   ✗ an unknown action was accepted"),
    }

    beat("A barred path stays barred — even with consent.");
    let denied = json!({"op": "read_file_key", "path": ".ssh/id_ed25519", "key": "x"});
    match reads::precheck(&denied) {
        Err(e) => println!("   ✓ {e}"),
        Ok(_) => println!("   ✗ a barred path was allowed"),
    }

    beat("A vendor who is not there is not a trust problem.");
    println!("   ✓ {}", dark.reason);

    println!("\n{B}{}{R}\n", "─".repeat(76));
    Ok(())
}
