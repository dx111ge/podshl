// The Basic Agent — the customer side, and the only permanent installation.
//
// Order is enforced rather than intended:
//   discover → verify identity out of band → triage → collect →
//   consent to transmit → diagnose → verify signature → per-action dry-run →
//   consent to execute → verify.
//
// The window never elevates. Where an action needs elevation the platform layer
// says so and a separate helper is required — an app able to elevate itself
// in-process cannot honestly claim bounded effect.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

// First, so `m!` is in scope in every module below it.
#[macro_use]
mod msg;
// The desktop's own agent as this client's model — measured, not assumed.
mod omarchy;
mod clientlog;
mod a2a;
mod actions;
mod demo;
mod doctor;
mod excerpt;
mod flow;
mod http;
mod jcs;
mod ledger;
mod handover;
// The window fetches the language files itself; this module exists so the
// suite can check them without a JavaScript engine.
#[cfg(test)]
mod i18n;
mod index;
// The report a person takes with them — more public than the one they send
// here, so it is anonymised harder rather than less.
mod issue;
mod identity;
mod jws;
mod llm;
mod logproof;
mod probes;
// The client's own question, asked on the person's behalf and requestable by
// no publisher — see the module for why that separation is the point.
mod provenance;
mod reads;
mod redact;
mod report;
mod trust;
// Source-level checks only; nothing here ships in the binary.
#[cfg(test)]
mod ui_contract;
mod vendors;
mod wire;


use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState {
    /// The out-of-band key for the session, once verified. Never taken from the card.
    jwk: Mutex<Option<Value>>,
    resolver: trust::Resolver,
    root: PathBuf,
}

use flow::Discovery;

#[tauri::command]
async fn discover(base: String, state: State<'_, AppState>) -> Result<Discovery, String> {
    let resolver = match &state.resolver {
        trust::Resolver::Dns => trust::Resolver::Dns,
        trust::Resolver::Pinned(p) => trust::Resolver::Pinned(p.clone()),
    };
    let (found, jwk) = flow::discover(&base, &resolver).await?;
    if let Some(k) = jwk {
        *state.jwk.lock().unwrap() = Some(k);
    }
    Ok(found)
}

/// The primary route: the user names the vendor. No reading of the machine.
/// Compare the vendor the user named against what the readings actually say.
#[tauri::command]
fn vendor_mismatch(chosen: String, facts: Value) -> Value {
    match vendors::mismatch(&chosen, &facts) {
        Some((name, domain)) => json!({ "conflict": true, "found": name, "domain": domain }),
        None => json!({ "conflict": false }),
    }
}

/// The window's own line in the log.
///
/// Every gate on the published path is decided in JavaScript — whether a hit
/// carries answers, whether the person took the offer, whether the card was
/// fetched — and until now JavaScript had no way to write here. So a log could
/// record `answers=3`, which it did on 2026-09-14, and say nothing whatever
/// about what the window then did with them. A day went into reasoning
/// backwards about which branch had been taken, and it was never established.
/// One line per gate ends that.
///
/// The text is the window's, so it is anonymised and cut like every other line
/// — `clientlog::line` does both — and `ui` marks which side wrote it.
#[tauri::command]
fn log_line(what: String) {
    clientlog::line(&format!("ui {what}"));
}

#[tauri::command]
fn search_vendors(query: String) -> Value {
    let out = vendors::search(&query);
    let first = out.as_array().and_then(|a| a.first());
    clientlog::line(&format!(
        "search {query:?} -> {} hit(s) first={} answers={} how={}",
        out.as_array().map(|a| a.len()).unwrap_or(0),
        first.and_then(|h| h.get("vendor")).and_then(|v| v.as_str()).unwrap_or("-"),
        first.and_then(|h| h.get("answers")).and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        first.and_then(|h| h.get("how")).and_then(|v| v.as_str()).unwrap_or("-")));
    out
}

/// Fetch the published catalogue and keep it. One request, the whole index, so
/// nothing about it says which project the user cares about — the search itself
/// then happens on this machine and is never sent anywhere.
///
/// Failure is not fatal and must not be silent: an out-of-date or absent index
/// means a project may not be found by name, which is a smaller thing than a
/// wrong answer, and the caller says so rather than pretending freshness.
#[tauri::command]
async fn refresh_index(base: String) -> Result<Value, String> {
    let (fetched, idx) = index::ensure_fresh(&base).await?;
    Ok(json!({
        "fetched": fetched,
        "entries": idx.entries.len(),
        "tree_size": idx.tree_size,
        "generated_ms": idx.generated_ms
    }))
}

/// What the client already holds, and how old it is. The age is reported
/// because a stale index is a legitimate state, not an error to hide.
/// Where the host interpreter and the project's disagree, hand the caller a
/// question. Never a decision: a solution keyed on the wrong one is a
/// confidently wrong answer, which is worse than asking.
#[tauri::command]
fn interpreter_conflict(facts: Value) -> Value {
    match reads::interpreter_conflict(&facts) {
        Some(q) => json!({"conflict": true, "question": q}),
        None => json!({"conflict": false}),
    }
}

/// The project the user is asking about, granted for this incident.
///
/// A path rather than a dialog for now: the grant is the point, and the picker
/// is presentation. Passing nothing withdraws it.
#[tauri::command]
fn set_project_root(path: Option<String>) -> Result<Value, String> {
    match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => {
            let pb = std::path::PathBuf::from(p.trim());
            if !pb.is_dir() {
                return Err(m!("not_a_dir", p = pb.display()));
            }
            reads::set_project_root(Some(pb.clone()));
            Ok(json!({"granted": true, "path": pb.to_string_lossy()}))
        }
        None => {
            reads::set_project_root(None);
            Ok(json!({"granted": false}))
        }
    }
}

/// A new question is a new incident, and every grant made for the last one
/// goes: the project directory and any program the user pointed at. Nothing
/// else ever cleared them, so a directory granted for one diagnosis stayed
/// readable for every diagnosis after it.
#[tauri::command]
fn end_incident() -> Value {
    reads::end_incident();
    json!({"cleared": true})
}

/// The user says where a program is, because the search path did not have it.
/// Validated here rather than in the window — a check the window could skip is
/// not a check.
#[tauri::command]
fn grant_program_path(program: String, path: String) -> Value {
    // A refusal is an answer with a kind the window translates, not an error:
    // the person typed a location and is owed a sentence in their language.
    match reads::grant_program_path(&program, &path) {
        Ok(p) => json!({"granted": true, "program": program, "path": reads::display_path(&p)}),
        Err(e) => json!({"granted": false, "code": reads::location_error_kind(&e), "detail": e}),
    }
}

/// Load the tail of a log for the user to cut from. `source` is `container`
/// (then `target` is the image the publisher named) or `file` (then it is the
/// path the user typed). Shown in the window and nowhere else.
#[tauri::command]
fn load_log_excerpt(source: String, target: String) -> Value {
    let loaded = match source.as_str() {
        "container" => excerpt::from_container(&target),
        "file" => excerpt::from_file(&target),
        other => Err(m!("log_source_unknown", source = format!("{other:?}"))),
    };
    match loaded {
        Ok(v) => v,
        Err(e) => json!({"loaded": false, "code": excerpt::error_kind(&e), "detail": e}),
    }
}

/// Free text as it would travel: bounded, with identifiers replaced, and a
/// count of what was replaced so the user is told rather than trusted to spot
/// it.
#[tauri::command]
fn anonymise_text(text: String) -> Value {
    redact::preview(&text)
}

/// Every fact, as it will leave this device, and what was taken out of it.
///
/// The consent panel used to render `FACTS` — what the machine read and what
/// the person typed, raw — while `flow::diagnose` anonymised on the way out.
/// So the panel showed a Windows account name under a sentence promising
/// "only this goes, nothing about you as a person", and the person had no way
/// to see that the account name was not in fact going. Shown is sent now: the
/// panel renders this, and this is what travels.
#[tauri::command]
fn facts_as_sent(facts: Value) -> Value {
    let mut counts = std::collections::BTreeMap::new();
    let out = redact::anonymise_value_counting(&facts, &mut counts);
    json!({ "facts": out, "replaced": counts })
}

/// A published project's own files, from the operator's mirror: what it asks
/// to be read, and how each fact is coarsened before it may travel. Fetched
/// once the user has picked the project, so the request says which project —
/// which the diagnosis that follows says anyway.
#[tauri::command]
async fn published_card(base: String, host: String) -> Result<Value, String> {
    if let Some(why) = a2a::insecure_base(&base) {
        return Err(why);
    }
    // `host` may be an identity rather than a host name, and for a repository it
    // has to be: `github.com` is shared by everything on that forge, so the
    // operator refuses a bare forge host rather than answer with whichever
    // project happened to be first. The window passes what it verified; this
    // turns it into the address the mirror takes.
    //
    // This is the half that was missed when the operator learned about
    // repositories: the route started requiring `?repo=` and the client kept
    // sending the host, so every repository anchor fell through the published
    // path into the model — which then asked what the project was, having never
    // been told.
    let url = if host.starts_with("https://") || host.starts_with("http://") {
        let tail = host.split("://").nth(1).unwrap_or("");
        let mut parts = tail.split('/').filter(|p| !p.is_empty());
        let h = parts.next().unwrap_or("");
        let repo = parts.collect::<Vec<_>>().join("/");
        if repo.is_empty() {
            format!("{}/mirror/{}", base.trim_end_matches('/'), h)
        } else {
            format!("{}/mirror/{}?repo={}", base.trim_end_matches('/'), h, repo)
        }
    } else {
        format!("{}/mirror/{}", base.trim_end_matches('/'), host)
    };
    let resp = http::client()
        .get(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send().await.map_err(|e| m!("unreachable", e = e))?;
    let v: Value = http::json_capped(resp, http::MAX_BODY).await?;
    clientlog::line(&format!("card {} -> attested={:?} collect={}", url,
        v.get("attested"),
        v.get("card").and_then(|c| c.get("collect")).and_then(|c| c.as_array())
            .map(|a| a.len()).unwrap_or(0)));
    if v.get("attested") != Some(&Value::Bool(true)) {
        return Err(m!("not_mirrored", host = host));
    }
    Ok(json!({
        "collect": v["card"]["collect"].clone(),
        "commit": v["serving"]["commit"].clone(),
        "escalate": v["card"]["escalate"].clone(),
        // The project's own terms, which the reader's model keeps as written
        // when it translates (`LG8`).
        "glossary": v["card"]["glossary"].clone(),
        // What the operator attests, and no more: control of the location and
        // when it was last confirmed — `live` or `stale` — and the project's own
        // word about itself. The window had only verified-or-untrusted, which
        // is a vendor's key, and no way to say "confirmed, three weeks ago".
        "anchor": v["anchor"].clone(),
        "project": v["project"].clone(),
        "log_seq": v["serving"]["log_seq"].clone(),
        // What the log entry has to attest for the answer on screen to be the
        // one it vouches for.
        "content_sha256": v["serving"]["content_sha256"].clone(),
    }))
}

/// Check, on this machine, that a log entry is in the operator's signed log
/// and attests exactly the files being served — the check the answer panel
/// tells people anybody can make.
#[tauri::command]
async fn verify_log_entry(base: String, seq: u64, expected: Value) -> Result<Value, String> {
    logproof::prove(&base, seq, &expected).await
}

/// A report about a published project, to the operator that mirrors it.
///
/// This path used to hand its report to `send_report`, which speaks A2A to a
/// *vendor's* agent — at the project's own address, where no agent exists, in
/// a shape the operator does not take — and it built the report from a skill
/// the published path never has, so every fact was dropped on the way. The one
/// branch whose whole offer is "the maintainer finally hears about it" could
/// not deliver a single report.
#[tauri::command]
async fn send_published_report(base: String, subject: String, report: Value) -> Result<Value, String> {
    if let Some(why) = a2a::insecure_base(&base) {
        return Err(why);
    }
    let body = report::for_operator(&report, &subject, &identity::pseudonym(&subject)?,
                                    &identity::epoch())?;
    let resp = http::client()
        .post(format!("{}/report", base.trim_end_matches('/')))
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send().await.map_err(|e| m!("unreachable", e = e))?;
    let v: Value = http::json_capped(resp, http::MAX_BODY).await?;
    Ok(v)
}

#[tauri::command]
fn index_status() -> Value {
    match index::cached() {
        Some(i) => {
            let now = index::now_ms();
            json!({
                "have": true, "entries": i.entries.len(),
                "tree_size": i.tree_size, "generated_ms": i.generated_ms,
                // Age is reported, never hidden. A stale catalogue is a
                // legitimate state — offline is normal — and the honest move is
                // to say how old it is rather than to imply freshness.
                "age_ms": now.saturating_sub(i.generated_ms),
                "stale": index::is_stale(&i, now)
            })
        }
        None => json!({"have": false, "entries": 0, "stale": true}),
    }
}


#[tauri::command]
async fn triage(base: String, problem: String, lang: String) -> Result<Value, String> {
    // The client states its language and the vendor serves the skill in it.
    // This is the protocol's answer to the localisation matrix: a vendor
    // answers the language asked for instead of maintaining N translations
    // that rot independently.
    flow::triage(&base, &problem, &lang).await
}

/// Phase one: say what would be read, and refuse anything out of bounds BEFORE
/// asking. A user must never be offered a choice the client would decline
/// anyway — that trains people to click through.
#[tauri::command]
fn plan_reads(probes: Vec<Value>) -> Value {
    flow::plan_reads(&probes)
}

/// Phase two: only ever after the user allowed it. Refused instructions are not
/// executed even if they somehow reach here.
/// `allow` lists the probe ids the user actually permitted. Consent is
/// per-item rather than all-or-nothing, and anything withheld simply becomes a
/// question for the user — declining must never be a dead end.
#[tauri::command]
fn perform_reads(probes: Vec<Value>, allow: Vec<String>) -> Value {
    flow::perform_reads(&probes, &allow)
}

#[tauri::command]
async fn diagnose(base: String, skill_id: String, facts: Value, lang: Option<String>,
                  state: State<'_, AppState>) -> Result<Value, String> {
    let jwk = state.jwk.lock().unwrap().clone().ok_or_else(|| m!("no_verified_key"))?;
    flow::diagnose(&base, &skill_id, &facts, &jwk, lang.as_deref().unwrap_or("en")).await
}

/// Build the report and show it before anything is sent. The user sees exactly
/// what would travel and what is held back, then decides.
#[tauri::command]
fn preview_report(skill: Value, facts: Value, stated: Vec<String>, decided_on: Vec<String>,
                  resolved_by: String, outcome: String) -> Value {
    let (r, held) = report::build(&skill, &facts, &stated, &decided_on, &resolved_by, &outcome);
    json!({ "report": r, "held_back": held })
}

/// Ask the operator for a published answer, before any model is involved.
///
/// This is the OSS path and it is the common one: a project published files, we
/// mirror them, and the decision is a walk over what they wrote. No generation,
/// and nothing about the user's problem is inferred.
///
/// `stated` travels with the facts so the answer can say what it turned on. The
/// walk is not refused a supplied fact — that would make asking the question
/// pointless — but a finding that rests on one is graded differently, and the
/// user is told, because a confident wrong answer is the thing worth avoiding.
#[tauri::command]
async fn ask_published(base: String, subject: String, problem_class: String,
                       facts: Value, stated: Vec<String>) -> Result<Value, String> {
    if let Some(why) = a2a::insecure_base(&base) {
        return Err(why);
    }
    clientlog::line(&format!(
        "ask {} about {} class={} — {} fact(s), {} of them stated",
        base, subject, problem_class,
        facts.as_object().map(|o| o.len()).unwrap_or(0), stated.len()));
    let resp = http::client()
        .post(format!("{}/diagnose", base.trim_end_matches('/')))
        .json(&json!({"subject": subject, "problem_class": problem_class,
                      "facts": facts, "stated": stated}))
        .timeout(std::time::Duration::from_secs(30))
        .send().await.map_err(|e| m!("unreachable", e = e))?;
    let v: Value = http::json_capped(resp, http::MAX_BODY).await?;
    Ok(v)
}

/// Attach free text the user has read and agreed to send. Separate command
/// because it is a separate consent: the report is complete without it, and a
/// caller must have shown the exact words to somebody to get here.
#[tauri::command]
fn attach_consented_text(report: Value, text: String, destination: String)
    -> Result<Value, String> {
    let mut r = report;
    report::with_consented_text(&mut r, &text, &destination, &identity::epoch())?;
    Ok(r)
}

#[tauri::command]
async fn send_report(base: String, report: Value, domain: String, lang: Option<String>)
    -> Result<Value, String> {
    let n = report.get("observed").and_then(|o| o.as_object()).map(|o| o.len()).unwrap_or(0);
    let s = report.get("stated").and_then(|o| o.as_object()).map(|o| o.len()).unwrap_or(0);
    let text = report.get("consented_text").is_some();
    clientlog::line(&format!(
        "send report to {domain} via {base} — {n} measured, {s} stated{}",
        if text { ", with the free text agreed separately" } else { "" }));
    flow::send_report(&base, report, &domain, lang.as_deref().unwrap_or("en")).await
}

#[tauri::command]
fn identity_info(domain: String) -> Value {
    json!({
        "epoch": identity::epoch(),
        "pseudonym": identity::pseudonym(&domain).unwrap_or_default(),
    })
}

/// Whether this vendor has earned the right to be reported to, and what to tell
/// the user either way. Asked **before** the button is offered: a user who
/// spends the effort on a vendor that has never once responded learns only that
/// the channel is worthless, and stops using it for everyone.
#[tauri::command]
async fn vendor_standing(vendor: String, index_url: Option<String>, state: State<'_, AppState>) -> Result<Value, String> {
    let led = ledger::Ledger::open(state.root.join("vendor_standing.json"));
    let standing = led.standing(&vendor);
    // The network median rests on more observations than one client can have,
    // so it outranks local experience — but only if it can actually be reached.
    // An unreachable index falls back to local experience and says which.
    let net = match &index_url {
        Some(u) if !u.is_empty() => ledger::network(&vendor, u).await,
        _ => None,
    };
    let (offer, note) = standing.advice(net.as_ref());
    Ok(json!({
        "offer": offer, "note": note,
        "reports": standing.reports, "acted": standing.acted,
        "source": if net.is_some() { "network" } else { "local" }
    }))
}

/// Record what a vendor's receipt actually said. Only a state that changed
/// something counts as the vendor acting; an acknowledgement does not.
#[tauri::command]
fn record_report_state(vendor: String, receipt_state: String, state: State<'_, AppState>) -> Result<Value, String> {
    let mut led = ledger::Ledger::open(state.root.join("vendor_standing.json"));
    led.record(&vendor, &receipt_state)?;
    let s = led.standing(&vendor);
    Ok(json!({ "reports": s.reports, "acted": s.acted }))
}

/// Contributing to the index is its own transmission decision, with its own
/// consent — and the entries go **one vendor per request**. The set of vendors
/// this client has dealt with is a profile of the software it runs, so a batch
/// would disclose in one call exactly what the design refuses to hold.
#[tauri::command]
async fn contribute_standing(index_url: String, dry_run: bool, state: State<'_, AppState>) -> Result<Value, String> {
    let pending = ledger::Ledger::open(state.root.join("vendor_standing.json")).pending_contributions();
    // The preview must be exactly what would travel, and must travel nothing.
    // A dry-run that quietly does the thing is worse than no dry-run at all.
    if dry_run {
        return Ok(json!({ "pending": pending, "sent": [] }));
    }
    let mut sent = Vec::new();
    for c in &pending {
        if let Some(r) = ledger::contribute(c, &index_url).await {
            sent.push(json!({ "vendor": c.get("vendor"), "accepted": r.get("accepted") }));
        }
    }
    Ok(json!({ "pending": pending, "sent": sent }))
}

#[tauri::command]
fn identity_reset() -> Result<Value, String> {
    identity::reset()?;
    Ok(json!({ "reset": true }))
}


/// Let the user's model pick from the catalogue for this specific question.
#[tauri::command]
async fn llm_choose_reads(problem: String, known: Value, lang: String, round: Option<u32>,
                          context: Option<Value>)
    -> Result<Value, String> {
    let cfg = llm::load();
    let cat = reads::catalogue();
    // Anything already gathered drops out of the menu, so a round cannot ask
    // for the same value twice.
    let have: Vec<String> = known.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    let remaining = json!(cat.as_array().cloned().unwrap_or_default().into_iter()
        .filter(|c| !have.iter().any(|h| c.get("id").and_then(|v| v.as_str()) == Some(h)))
        .collect::<Vec<_>>());
    // The project's own words, on the rounds as well as on the answer. These
    // are the calls a person actually meets: the model choosing what to read
    // and what to ask. Giving the context only to the final answer left the
    // questions being asked about a project nobody had named.
    let round = llm::choose_reads(&cfg, &problem, &remaining, &known,
                                  &context.unwrap_or(Value::Null), &lang,
                                  round.unwrap_or(1).max(1) as usize).await?;
    let ids = round.read_ids;
    let chosen: Vec<Value> = remaining.as_array().into_iter().flatten()
        .filter(|c| ids.iter().any(|id| c.get("id").and_then(|v| v.as_str()) == Some(id)))
        .cloned()
        .map(|mut c| { c["kind"] = json!("machine"); c })
        .collect();
    Ok(json!({
        "probes": chosen,
        // Things no command can answer — asked of the user, as a technician would.
        "questions": round.questions,
        "done": round.done
    }))
}

#[tauri::command]
async fn llm_follow_up(problem: String, facts: Value, previous: String, added: String,
                       lang: String, typed: Option<Vec<String>>,
                       context: Option<Value>) -> Result<Value, String> {
    let cfg = llm::load();
    let typed = typed.unwrap_or_default();
    let raw = llm::follow_up(&cfg, &problem, &facts, &previous, &added, &lang,
                             &context.unwrap_or(Value::Null)).await?;
    // Held to the same sections as the first answer. A follow-up that dropped
    // back to a paragraph would quietly undo the method one question in, which
    // is exactly when a person is most likely to act on what they read.
    let sections = llm::parse_answer(&raw, &facts, &typed);
    Ok(json!({ "answer": raw, "sections": sections }))
}

/// Solve without a vendor, using the model the user configured.
/// Translate vendor-authored text locally. Never used when the vendor already
/// serves the user's language — then there is nothing to compare against.
///
/// `keep` is a published project's glossary — its own terms, kept as written.
/// The answer is `{texts, lost}`: the translation, and any kept term it dropped.
#[tauri::command]
async fn llm_translate(texts: Value, to: String, keep: Option<Vec<String>>) -> Result<Value, String> {
    let cfg = llm::load();
    llm::translate(&cfg, &texts, &to, &keep.unwrap_or_default()).await
}

#[tauri::command]
async fn llm_solve(problem: String, facts: Value, lang: String, typed: Option<Vec<String>>,
                   context: Option<Value>)
    -> Result<Value, String> {
    let cfg = llm::load();
    // `typed` is what the person answered rather than what the machine read.
    // A cause resting on one of those may still be right and is not evidence
    // about anything — the person could have been mistaken — and the published
    // path grades a finding exactly this way. An answer nobody can weigh is
    // what both paths exist to avoid.
    let typed = typed.unwrap_or_default();
    // What the project published about itself. Absent on a path where there is
    // no project — then the model is told nothing about one, which is honest.
    let context = context.unwrap_or(Value::Null);
    let answer = llm::solve(&cfg, &problem, &facts, &lang, &typed, &context).await?;
    Ok(json!({ "answer": answer.raw, "sections": answer,
               "model_class": cfg.model_class, "ux_severity": cfg.ux_severity() }))
}

/// Report when the vendor has no agent. This is what makes "they will never
/// find out" false — and it is the whole point: value accumulates about a
/// vendor without the vendor participating.
///
/// This used to post to the catch-all, which took a free-text `product` and a
/// `problem_class` the client chose, and counted submissions. The server takes
/// a `subject` host and a pseudonym, derives the class from the signature, and
/// counts distinct pseudonyms — so a class sent from here would be ignored and
/// a report without a pseudonym is refused. The pseudonym is per subject and
/// per epoch, exactly as the vendor path derives one per vendor: within a
/// month the subject can be counted, across subjects nothing links.
/// What the no-vendor report would actually carry, before anybody agrees to it.
///
/// **The panel promised "exactly what would be sent" and showed two fields** —
/// the subject and the model class — while the call beside it handed over every
/// fact the window held. Most of those do not travel (a reading the catalogue
/// does not know has no policy and is dropped), so nothing leaked; but a person
/// deciding whether to send was shown an administrative pair and not the
/// readings, which are the part the decision is about.
///
/// Same coarsening as the send, from the same function, so the two cannot
/// disagree: a preview computed a second way is a preview that drifts.
#[tauri::command]
fn preview_without_vendor(observed: Value) -> Value {
    let (observed, held) = report::observed_by_catalogue(&observed);
    json!({ "observed": observed, "held_back": held })
}

#[tauri::command]
async fn report_without_vendor(base: String, subject: String,
                               model_class: String, ux_severity: String, observed: Value,
                               outcome: Option<String>)
    -> Result<Value, String> {
    // **The run that reached this through a gap says so.** A model guessing is
    // the second exit from "nothing published covered this", and the maintainer
    // needs that fact exactly as much as they need it from the first exit — the
    // person who took it to the tracker instead. Without the word, the same
    // event arrives labelled as a report about a project with no published
    // answers, which is the opposite of what happened.
    //
    // Optional because this command is also the catch-all for a project that
    // publishes nothing at all, where there is no gap to report: nobody wrote
    // an answer, so none is missing. Checked here against the same list the
    // operator holds, so a word this window invents is refused before the send
    // rather than after it.
    let outcome = outcome.filter(|o| !o.is_empty());
    if let Some(o) = &outcome {
        if !report::OUTCOMES.contains(&o.as_str()) {
            return Err(m!("report_bad_outcome", o = format!("{o:?}")));
        }
    }
    // The same coarsening policy the vendor path applies, which this path did
    // not: it posted whatever the window handed it, at full precision, with
    // nothing between the machine and the operator. Without a skill the policy
    // comes from the catalogue, and a reading the catalogue does not know has
    // no policy and does not travel.
    // What was held back is not sent: the operator has no use for a list of
    // readings it is not getting, and naming them would be a fact about this
    // machine arriving by the back door.
    let (observed, held) = report::observed_by_catalogue(&observed);
    // **A send leaves a line.** The log calls itself one line per operator call
    // and had none for any send at all — not this one, not a report to a
    // vendor, not a question to the operator. On a product whose argument is
    // that you can see what leaves your machine, the sends were the one thing
    // it did not write down. Counts and destination, never contents: the
    // contents were just shown to somebody on a panel they agreed to.
    clientlog::line(&format!(
        "send report(no vendor) to {} about {} — {} value(s), {} held back{}",
        base, subject, observed.len(), held.len(),
        outcome.as_deref().map(|o| format!(", outcome {o}")).unwrap_or_default()));
    let mut body = json!({ "subject": subject,
                       "pseudonym": identity::pseudonym(&subject)?,
                       "model_class": model_class, "ux_severity": ux_severity,
                       "observed": observed });
    if let Some(o) = outcome {
        body["outcome"] = json!(o);
    }
    if let Some(why) = a2a::insecure_base(&base) {
        return Err(why);
    }
    let resp = http::client()
        .post(format!("{}/report", base.trim_end_matches('/')))
        .json(&body)
        .send().await.map_err(|e| e.to_string())?;
    let v: Value = http::json_capped(resp, http::MAX_BODY).await?;
    Ok(v)
}

#[tauri::command]
fn llm_providers() -> Value {
    let mut list = llm::providers_json();
    // The desktop's own agent is offered **only where it actually works**: this
    // desktop names one, it is installed, and somebody has measured how to call
    // it with its tools denied. An option that appears and then fails is worse
    // than one that never appears — and here the failure would be a person
    // choosing "no setup needed" and getting nothing.
    //
    // Where it is offered, the agent's name is filled in as the model and the
    // row says whether the prompt leaves this machine, because for the one
    // agent measured so far it does.
    let offer = omarchy::offer();
    if let Some(arr) = list.as_array_mut() {
        arr.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some("omarchy_agent"));
        if let Some((agent, cloud)) = offer {
            arr.insert(0, json!({
                "id": "omarchy_agent",
                // **`name`, because that is the key the window renders.** This
                // row carried only `label`, so the settings drew it as an empty
                // option — the first entry in the list, and the selected one on
                // the desktop this was built for. `tr(undefined)` is the empty
                // string, so nothing showed and nothing complained.
                "name": format!("Omarchy default agent ({agent})"),
                "label": format!("Omarchy default agent ({agent})"),
                "endpoint": "",
                "model": agent,
                "needs_key": false,
                "cloud": cloud,
                // Stated rather than absent. The window reads `local` to pick a
                // model class, and a missing one reads as false by accident — right
                // for this agent today, wrong the moment a local one is measured.
                "local": !cloud,
                "note": "no model setup here — this desktop already has one"
            }));
        }
    }
    list
}

#[tauri::command]
fn llm_get() -> Value {
    let c = llm::load();
    json!({
        "provider": c.provider, "model": c.model, "endpoint": c.endpoint,
        "model_class": c.model_class, "uses_key": c.uses_key,
        "configured": c.configured(), "is_cloud": c.is_cloud(),
        "label": c.label(), "ux_severity": c.ux_severity(),
        // Whether a key exists is reported; the key itself never is.
        "has_key": !c.provider.is_empty() && llm::has_key(&c.provider)
    })
}

#[tauri::command]
fn llm_set(provider: String, model: String, endpoint: String, model_class: String,
           api_key: Option<String>) -> Result<Value, String> {
    let uses_key = llm::preset(&provider).map(|p| p.4).unwrap_or(true);
    let c = llm::Config { provider: provider.clone(), model, endpoint, model_class, uses_key };
    llm::save(&c)?;
    // An empty string clears the stored key; `None` leaves it untouched, so
    // re-saving other settings never silently drops the credential.
    if let Some(k) = api_key {
        llm::set_key(&provider, &k)?;
    }
    Ok(json!({ "saved": true, "configured": c.configured(),
               "has_key": llm::has_key(&provider) }))
}

/// Cheap: does the endpoint answer? Runs at startup, costs nothing.
#[tauri::command]
async fn llm_probe() -> Value {
    let cfg = llm::load();
    if !cfg.configured() {
        return json!({ "state": "unconfigured" });
    }
    match llm::probe(&cfg).await {
        Ok(n) => json!({ "state": "reachable", "models": n, "label": cfg.label() }),
        Err(e) => json!({ "state": "unreachable", "error": e, "label": cfg.label() }),
    }
}

/// Expensive and explicit: one real completion through the fallback path.
#[tauri::command]
async fn llm_test() -> Result<Value, String> {
    let cfg = llm::load();
    let (answer, ms) = llm::test(&cfg).await?;
    Ok(json!({ "ok": true, "answer": answer, "ms": ms, "label": cfg.label() }))
}

#[tauri::command]
async fn llm_models(provider: String, endpoint: String) -> Result<Value, String> {
    let uses_key = llm::preset(&provider).map(|p| p.4).unwrap_or(true);
    let cfg = llm::Config { provider, model: String::new(), endpoint,
                            model_class: String::new(), uses_key };
    Ok(json!({ "models": llm::list_models(&cfg).await? }))
}

/// What this agent can do, and what this particular machine cannot supply.
/// Both halves belong together: a capability list that hides its own gaps
/// overstates what the user is agreeing to.
/// Which return paths this client can honour, so the vendor's offer can be
/// filtered before the user is asked to choose one.
#[tauri::command]
fn reply_channels(offered: Value) -> Value {
    json!({ "all": handover::channels_json(), "usable": handover::usable(&offered) })
}

/// Hand the case to a person. The routing target is the vendor's and passes
/// through untouched; the payload is exactly what the user saw and approved.
#[tauri::command]
async fn escalate(base: String, queue: String, target: Option<String>,
                  reply_via: String, payload: Value, lang: Option<String>) -> Result<Value, String> {
    if !handover::channel_known(&reply_via) {
        return Err(m!("reply_channel_unknown", via = format!("{reply_via:?}"),
                      known = format!("{:?}", handover::REPLY_CHANNELS)));
    }
    a2a::send_message(&base, json!({
        "kind": "escalate", "queue": queue, "target": target,
        "reply_via": reply_via, "payload": payload,
        // The reply note is the vendor's sentence to this person; it comes in
        // their language where the vendor has it.
        "lang": lang.as_deref().unwrap_or("en")
    })).await
}

#[tauri::command]
fn vocabulary() -> Value {
    json!({
        "actions": actions::VOCABULARY,
        "readable": reads::catalogue(),
        "unavailable": reads::catalogue_unavailable()
    })
}

#[tauri::command]
fn dry_run(action: String, params: Value) -> Result<String, String> {
    actions::dry_run(&action, &params)
}

#[tauri::command]
fn execute(action: String, params: Value, state: State<'_, AppState>) -> Result<Value, String> {
    actions::execute(&action, &params, &state.root)
}

/// Known without asking and without reading — shown, not hidden. The operating
/// system family and the architecture are compile-time constants, so this is a
/// fact about the program rather than about the user's machine.
#[tauri::command]
fn baseline_facts() -> Value {
    reads::baseline()
}

/// Where this client talks to, and why it is not a constant in the window.
///
/// Both of these were literals in `ui/index.html`, pinned to loopback because
/// that is where the development counterparty runs. That is fine until there is
/// a host, and then it is a released binary that can only ever talk to the
/// machine it is running on.
///
/// Read from the environment with the loopback defaults kept, so nothing about
/// a development checkout changes and a packaged build can be pointed somewhere
/// real without a rebuild. The names match the server's own `PODSHL_*`
/// convention rather than the client's older `VS_*` one, because these name the
/// *server* being addressed.
///
/// Deliberately not a stored setting the window can write. An operator endpoint
/// that a page could change is one a page could redirect, and this client sends
/// a report there.
#[tauri::command]
fn endpoints() -> Value {
    let from = |name: &str, default: &str| -> String {
        match std::env::var(name) {
            Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
            _ => default.to_string(),
        }
    };
    // A release is built for one operator (`PODSHL_BUILD_*`), because an
    // installed client has nobody to set its environment. Unset, loopback.
    json!({
        "operator": from("PODSHL_SERVER_URL", option_env!("PODSHL_BUILD_SERVER_URL").unwrap_or("http://127.0.0.1:8725")),
        "index": from("PODSHL_INDEX_URL", option_env!("PODSHL_BUILD_INDEX_URL").unwrap_or("http://127.0.0.1:8723")),
        // **Whether anything was compiled into this binary at all.**
        //
        // `cargo test` and a bare `cargo build` rebuild the same path without
        // `PODSHL_BUILD_*` set, and what comes out is not a broken client — it
        // is a plausible one, pointing at loopback with no pinned key, drawing
        // the same window. It cost this project an afternoon on 2026-09-14 and
        // caught the author of this comment twice more on 2026-09-15, once from
        // `cargo test`, which nobody thinks of as a build.
        //
        // So the binary says which it is, and the window says it on screen. A
        // development build is a perfectly good thing to be; being one silently
        // is not.
        "built": option_env!("PODSHL_BUILD_SERVER_URL").is_some(),
        "has_key": option_env!("PODSHL_BUILD_LOG_KEY").is_some(),
    })
}

/// The operating system's language preference, used as the initial UI language.
#[tauri::command]
fn os_locale() -> Value {
    let raw = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    let code = raw.split(['.', '_', '-']).next().unwrap_or("").to_lowercase();
    json!({ "raw": raw, "code": if code.is_empty() { "en".into() } else { code } })
}

#[tauri::command]
fn applicability(required: Value) -> Value {
    let (ok, why) = probes::applicability(&required);
    json!({ "applies": ok, "why": why })
}

/// Everything the binary can do without a window.
///
/// Both of these used to live in Python, next to a second implementation of
/// this client — so `doctor` reported what *that* code could do, and the demo
/// exercised it too. Whatever they were measuring, it was not the program
/// anybody installs. They answer for the shipped binary now.
/// Die quietly when the reader goes away.
///
/// The Rust runtime sets SIGPIPE to SIG_IGN, so a write to a closed pipe returns
/// EPIPE instead of killing the process — and `println!` turns that error into a
/// panic, which `panic = "abort"` turns into SIGABRT and a multi-megabyte core
/// dump. `podshl-client demo | head` did exactly that.
///
/// Every ordinary Unix program exits silently there, and a diagnostic tool that
/// litters the coredump directory when someone pipes it through `less` is not
/// one anybody will trust. Restored only for the command-line path: leaving the
/// GUI's disposition alone, because there a write to a closed socket returning
/// an error is the behaviour the rest of the code expects.
#[cfg(unix)]
fn exit_quietly_on_a_closed_pipe() {
    // SAFETY: setting a signal disposition to the system default, before any
    // thread is spawned and before anything is written.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn exit_quietly_on_a_closed_pipe() {}

/// One of the window's commands, by name, for `invoke`.
///
/// Named explicitly rather than generated: a command reachable from a shell is
/// a surface, and the ones that write to this machine or spend somebody's money
/// are not on it. `execute`, `llm_set` and the identity commands are absent on
/// purpose — a case that needs them is a case that should say so.
async fn invoke_by_name(name: &str, a: &Value) -> Result<Value, String> {
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let v = |k: &str| a.get(k).cloned().unwrap_or(Value::Null);
    let list = |k: &str| a.get(k).and_then(|x| x.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(match name {
        "search_vendors" => search_vendors(s("query")),
        "index_status" => index_status(),
        "refresh_index" => refresh_index(s("base")).await?,
        "llm_get" => llm_get(),
        "llm_providers" => llm_providers(),
        "llm_probe" => llm_probe().await,
        "endpoints" => endpoints(),
        "baseline_facts" => baseline_facts(),
        "vocabulary" => vocabulary(),
        "published_card" => published_card(s("base"), s("host")).await?,
        "ask_published" => ask_published(s("base"), s("subject"), s("problemClass"),
                                         v("facts"), list("stated")).await?,
        "preview_report" => preview_report(v("skill"), v("facts"), list("stated"),
                                           list("decidedOn"), s("resolvedBy"), s("outcome")),
        "facts_as_sent" => facts_as_sent(v("facts")),
        "anonymise_text" => anonymise_text(s("text")),
        "provenance_check" => provenance_check(s("anchorUrl"), s("subject")),
        // It writes, but only to the log it is allowed to write, and a harness
        // driving the window must be able to say what the window would have
        // said or the trace it produces is not the window's.
        "log_line" => { log_line(s("what")); json!({ "logged": true }) }

        // **Read-only, and on the surface for everybody.** None of these four
        // touches the machine, sends anything or spends anything: a pure
        // function over probes, a network read with a verification, an
        // environment variable, and clearing grants that live in this process
        // and die with it. They were missing for no reason anybody wrote down.
        "plan_reads" => plan_reads(a.get("probes").and_then(|p| p.as_array()).cloned().unwrap_or_default()),
        "verify_log_entry" => verify_log_entry(s("base"), a.get("seq").and_then(|x| x.as_u64()).unwrap_or(0), v("expected")).await?,
        "os_locale" => os_locale(),
        "end_incident" => end_incident(),

        // **Only in a build that asked for them.** These run readings on
        // somebody's machine, send their data to an operator, and can spend
        // their money — which from a window happens behind consent screens,
        // and from here would not. A released client is built without
        // `uitest`, so it does not have them at all: a decision made at compile
        // time by whoever builds, not at run time by whatever is running.
        #[cfg(feature = "uitest")]
        "perform_reads" => perform_reads(
            a.get("probes").and_then(|p| p.as_array()).cloned().unwrap_or_default(), list("allow")),
        #[cfg(feature = "uitest")]
        "send_published_report" => send_published_report(s("base"), s("subject"), v("report")).await?,
        #[cfg(feature = "uitest")]
        "llm_translate" => llm_translate(v("texts"), s("to"), Some(list("keep"))).await?,

        // **`discover`, for the same build and with a stated limit.** It is
        // absent from the ordinary surface for a reason that is architecture
        // rather than policy: the window's copy stores the verified
        // out-of-band key in `AppState` and every later step reads it from
        // there, while this surface is one process per call — the key would be
        // gone before anybody could use it.
        //
        // The published path never needs it: a project with no agent card has
        // no key to keep. So this answers, and refuses the one case it cannot
        // honour, rather than returning a half-answer that looks like the
        // window's.
        #[cfg(feature = "uitest")]
        "discover" => {
            let trust_env = std::env::var("VS_TRUST").unwrap_or_else(|_| "dns".into());
            let resolver = if trust_env == "dns" {
                trust::Resolver::Dns
            } else {
                trust::Resolver::Pinned(std::path::PathBuf::from(trust_env))
            };
            let (found, jwk) = flow::discover(&s("base"), &resolver).await?;
            if jwk.is_some() {
                return Err(
                    "this anchor publishes an agent card, and the key verified for it cannot be                      kept: `invoke` is one process per call and the window holds that key in                      application state for every later step. Drive the vendor path through the                      window instead."
                        .to_string());
            }
            serde_json::to_value(found).map_err(|e| e.to_string())?
        }
        "issue_report" => issue_report(
            s("subject"), s("problem"), v("facts"), list("stated"), s("outcome"),
            s("answer"), a.get("answerFromModel").and_then(|b| b.as_bool()).unwrap_or(false),
            list("tried"), a.get("footer").and_then(|b| b.as_bool()).unwrap_or(true)),
        // Two kinds of absence, said apart. "Not in this build" is not "no such
        // command", and telling somebody the second when the first is true
        // costs them the afternoon this distinction exists to save.
        #[cfg(not(feature = "uitest"))]
        other @ ("perform_reads" | "send_published_report" | "llm_translate") => return Err(format!(
            "{other:?} is not in this build. It runs readings on this machine, sends data, or spends money. Build with `--features uitest` if you are driving the window in a test.")),
        other => return Err(format!(
            "no such command on this surface: {other:?}. The window has more; the ones \
             that write to this machine or spend money are deliberately not here.")),
    })
}


fn run_subcommand(name: &str) -> Result<(), String> {
    exit_quietly_on_a_closed_pipe();
    match name {
        "doctor" => {
            doctor::print_report();
            Ok(())
        }
        "demo" => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(demo::run()),
        // The window's own commands, reachable without the window.
        //
        // **Every defect found on 2026-09-14 was found by a person clicking**,
        // and none of them could have been found any other way: `ui_contract`
        // reads the window's source, which is text, and text cannot fall into
        // the wrong branch. `TESTCASES.md` has said so for months — *"the Rust
        // client is untested through its window"* — and it stayed true because
        // the only door into these functions was a window nobody can drive.
        //
        // This is that door. It dispatches to the same functions the window
        // calls, so a case can walk a whole path — search, pick, the published
        // answer, the report — and assert what came back, without a desktop and
        // without anybody clicking. What it cannot check is what a person sees;
        // that stays `manual`, and it is a far smaller thing than a flow that
        // silently takes the wrong turn.
        "invoke" => {
            let name = std::env::args().nth(2)
                .ok_or_else(|| "usage: podshl-client invoke <command> [json]".to_string())?;
            let raw = std::env::args().nth(3).unwrap_or_else(|| "{}".into());
            let args: Value = serde_json::from_str(&raw)
                .map_err(|e| format!("arguments are not JSON: {e}"))?;
            let out = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(invoke_by_name(&name, &args))?;
            println!("{}", serde_json::to_string(&out).map_err(|e| e.to_string())?);
            Ok(())
        }
        "--help" | "-h" | "help" => {
            println!("podshl-client [doctor|demo|invoke <command> [json]|--version]\n");
            println!("  (no argument)  the window");
            println!("  doctor         what this client can do on this machine");
            println!("  demo           the whole argument in five acts, against live services");
            println!("  invoke         one of the window's commands, without the window");
            println!("  --version      which version this is");
            Ok(())
        }
        // The first thing anybody supporting a program asks is which version it
        // is, and this client now asks other programs exactly that. It should
        // be able to answer the question itself.
        "--version" | "-V" | "version" => {
            println!("podshl-client {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(format!("unknown command {other:?} — possible are: doctor, demo, --version")),
    }
}

/// What `WEBKIT_DISABLE_DMABUF_RENDERER` should become, given whether somebody
/// already chose a value. Separated from `main` so the decision can be tested
/// without setting a process-wide variable from inside a threaded test run.
///
/// Nobody chose means we choose, because the failure we are avoiding is silent.
/// Somebody chose means we leave it alone, whatever they chose — `0` included,
/// which is a person asking for the accelerated path back and being given it.
#[cfg(target_os = "linux")]
fn dmabuf_renderer_setting(already_set: bool) -> Option<&'static str> {
    if already_set { None } else { Some("1") }
}

#[cfg(all(test, target_os = "linux"))]
mod dmabuf_tests {
    use super::dmabuf_renderer_setting;

    /// L2: the client turns the DMA-BUF renderer off for itself, and stops if
    /// the person said otherwise. The part that matters — that the window then
    /// opens at all — needs a Wayland session and is walked rather than run
    /// here; this only holds the override from being quietly dropped, which is
    /// what would turn a per-application default into one nobody can undo.
    #[test]
    fn it_chooses_only_when_nobody_else_did() {
        assert_eq!(dmabuf_renderer_setting(false), Some("1"), "left the window to fail");
        assert_eq!(dmabuf_renderer_setting(true), None, "overrode the person's own value");
    }
}

/// Ask this machine where it got `subject` from, and say so only if that
/// disagrees with the anchor the person picked.
///
/// **Nobody can request this and nobody can switch it off.** Every other reading
/// here is one a publisher asked for; this one is the client's own question,
/// asked on the person's behalf, which is the only reason it is worth anything:
/// a project pretending to be another cannot prevent the machine from naming the
/// real one, because the answer is not on their side of the wire.
///
/// It is still a reading, so the window asks before calling this. And it answers
/// `null` far more often than not — nothing installed under that name, nothing
/// recorded about it, or it agrees — because only a disagreement is worth a
/// person's attention, and even that is a disagreement rather than a verdict:
/// forks, vendored copies and distributions that repackage all produce one
/// honestly.
#[tauri::command]
fn provenance_check(anchor_url: String, subject: String) -> Value {
    match provenance::disagrees_with(&anchor_url, &subject) {
        None => Value::Null,
        Some(p) => json!({
            "package": p.package,
            "declared": p.url,
            "source": p.source,
            "picked": anchor_url,
        }),
    }
}


/// The report a person takes with them, as Markdown, and what was taken out of
/// it.
///
/// The only exit from a diagnosis was a pseudonymous report to the operator,
/// which is the wrong shape for the case the open-source branch rests on: the
/// published answers did not cover somebody's problem, and they now hold more
/// about it than they could have assembled in an hour, with nowhere to put it.
///
/// This is **more public** than a report — an issue tracker, for ever, under
/// their own name — so every string goes through the same anonymiser the
/// consent panel uses, the person's own words included, and `replaced` is
/// returned so the panel can say what it took out. The same sentence, about a
/// larger audience.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn issue_report(subject: String, problem: String, facts: Value, stated: Vec<String>,
                outcome: String, answer: String, answer_from_model: bool,
                tried: Vec<String>, footer: bool) -> Value {
    issue::build(&subject, &problem, &facts, &stated, &outcome, &answer,
                 answer_from_model, &tried, footer)
}


fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        if let Err(e) = run_subcommand(&arg) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }

    // The client's own state — the vendor ledger, and the only directory a
    // bounded action may write in. It was ".", which for an installed program is
    // whatever directory a shortcut happened to start it in: the install folder,
    // removed with the program, or somewhere it has no business writing. Beside
    // the rest of its state instead, where `llm.json` and the identity live.
    let root = std::env::var("VS_ROOT").map(PathBuf::from).unwrap_or_else(|_| {
        dirs::config_dir().map(|p| p.join("podshl")).unwrap_or_else(|| PathBuf::from("."))
    });
    let _ = std::fs::create_dir_all(&root);
    // VS_TRUST=dns selects the production path: keys resolved from DNS under a
    // domain the organisation controls. Anything else is treated as a pinned
    // file, which announces itself as a stand-in.
    // Production default is the real path. A pinned file is a development
    // convenience and has to be requested explicitly, so shipping without
    // configuration fails closed rather than trusting a local stub.
    let trust_env = std::env::var("VS_TRUST").unwrap_or_else(|_| "dns".into());
    let resolver = if trust_env == "dns" {
        trust::Resolver::Dns
    } else {
        trust::Resolver::Pinned(PathBuf::from(trust_env))
    };

    // The window does not open on Wayland unless WebKitGTK's DMA-BUF renderer
    // is off. Measured on this project's own target desktop — Omarchy 4.0.2,
    // Hyprland 0.56.2, webkit2gtk 2.52.6, NVIDIA 610.57.04 — where without it
    // the process dies before any window exists, with one line on stderr:
    // "Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display."
    // The desktop entry is Terminal=false, so that line goes nowhere a person
    // will read it: they click the icon and nothing happens at all. A support
    // tool that looks broken on first contact is worse than one that is slow.
    //
    // Set unconditionally for this process, which is what the published answer
    // for this class recommends — "both are per-application on purpose" — and
    // deliberately not decided by driver version. That answer names "< 555" as
    // the boundary and 610 fails here, so the boundary is not known; encoding a
    // guess would fail invisibly for whoever falls outside it. What it costs
    // when it was not needed is the software path in one window of text and
    // forms. Set the variable yourself to anything to override this.
    #[cfg(target_os = "linux")]
    {
        let var = "WEBKIT_DISABLE_DMABUF_RENDERER";
        if let Some(v) = dmabuf_renderer_setting(std::env::var_os(var).is_some()) {
            std::env::set_var(var, v);
        }
    }

    // Written before the window exists, because these three decide whether
    // anything can work and none of them is visible from it.
    {
        let st = index_status();
        let cfg = llm::load();
        clientlog::start(
            &endpoints()["operator"].as_str().unwrap_or("?").to_string(),
            st.get("entries").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
            st.get("have").and_then(|v| v.as_bool()).unwrap_or(false),
            &format!("{}/{}", cfg.provider, cfg.model),
        );
    }

    tauri::Builder::default()
        .setup(move |app| {
            app.manage(AppState {
                jwk: Mutex::new(None),
                resolver: match &resolver {
                    trust::Resolver::Dns => trust::Resolver::Dns,
                    trust::Resolver::Pinned(p) => trust::Resolver::Pinned(p.clone()),
                },
                root: root.clone(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            discover, triage, plan_reads, perform_reads, diagnose, vocabulary, dry_run,
            execute, applicability, reply_channels, escalate, preview_report, ask_published, attach_consented_text, send_report, search_vendors, refresh_index, index_status, interpreter_conflict, set_project_root, vendor_mismatch, identity_info, identity_reset, vendor_standing, record_report_state, contribute_standing, llm_solve, llm_translate, report_without_vendor, preview_without_vendor, llm_providers, llm_models, llm_probe, llm_test, baseline_facts, os_locale, endpoints, llm_choose_reads, llm_follow_up,
            llm_get, llm_set, end_incident, grant_program_path, load_log_excerpt, anonymise_text,
            facts_as_sent,
            published_card, send_published_report, verify_log_entry,
            provenance_check, issue_report, log_line
        ])
        .run(tauri::generate_context!())
        .expect("PODSHL could not start");
}
