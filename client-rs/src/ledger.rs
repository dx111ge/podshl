//! Client-side record of whether vendors actually act on reports.
//!
//! The weak point of the report channel is that **efficacy is entirely the
//! vendor's to deliver**, while the user's trust relationship is with the agent
//! that asked. A vendor who ignores reports therefore damages the channel for
//! every other vendor — a commons problem, not a bilateral one.
//!
//! The fix is measurement pointed the other way: the client knows what fraction
//! of its reports ever reached a state, so responsiveness becomes an observable
//! property rather than a promise — shown *before* the user spends effort, and
//! where a vendor has earned it the button is not offered at all, with the
//! reason stated.
//!
//! Local experience is the fallback. The network median is what makes the
//! figure meaningful, and contributing to it is its own transmission decision
//! with its own consent: **contributions go one vendor at a time.** The set of
//! vendors a client has dealt with is a profile of the software it runs, and a
//! batch would disclose it in one request.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A state that tells the user something changed, as against one that merely
/// acknowledges receipt. Only the first counts as the vendor acting — otherwise
/// an auto-reply would score as responsiveness.
pub const ACTED: &[&str] = &["fixed_in", "known", "wont_fix"];

/// Below this many reports there is no basis for a judgement, and saying so is
/// better than quoting a percentage computed from three observations.
pub const MIN_BASIS: u32 = 4;

/// A client with fewer observations than this has no opinion worth contributing.
pub const MIN_LOCAL_BASIS: u32 = 4;

/// Below this share, the button is withheld: the report would be wasted effort,
/// and asking for it anyway spends the one thing the channel cannot replace.
pub const OFFER_FLOOR: i64 = 15;

const WEIGHT_BANDS: &[(u32, &str)] = &[(4, "4-9"), (10, "10-24"), (25, "25-99"), (100, "100+")];

pub fn weight_band(n: u32) -> &'static str {
    let mut band = WEIGHT_BANDS[0].1;
    for (edge, label) in WEIGHT_BANDS {
        if n >= *edge {
            band = label;
        }
    }
    band
}

/// Coarse to 10 %. A precise rate derived from few observations is identifying.
pub fn round_rate(rate: f64) -> i64 {
    (rate * 10.0).round() as i64 * 10
}

#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    pub vendor: String,
    pub reports: u32,
    pub acted: u32,
}

impl Standing {
    pub fn rate(&self) -> Option<f64> {
        if self.reports < MIN_BASIS {
            None
        } else {
            Some(self.acted as f64 / self.reports as f64)
        }
    }

    /// `(offer the button?, what to tell the user)`
    ///
    /// The network figure outranks local experience where it exists — it rests
    /// on more observations — but it never silently replaces a local reading
    /// that contradicts it. Both are shown, because a user whose own experience
    /// is being overruled is entitled to see that happening.
    pub fn advice(&self, network: Option<&Value>) -> (bool, String) {
        if let Some(n) = network {
            if n.get("published").and_then(|v| v.as_bool()) == Some(true) {
                let contributors = n.get("contributors").and_then(|v| v.as_i64()).unwrap_or(0);
                let pct = n.get("rate_pct").and_then(|v| v.as_i64()).unwrap_or(0);
                let spread = n.get("spread_pct").and_then(|v| v.as_array()).cloned();
                let (lo, hi) = match spread.as_deref() {
                    Some([a, b]) => (a.as_i64().unwrap_or(pct), b.as_i64().unwrap_or(pct)),
                    _ => (pct, pct),
                };
                // Sentences, each one a message of its own, so the window can
                // say each in the person's language.
                let mut line = m!(
                    "standing_network",
                    v = self.vendor,
                    pct = pct,
                    n = contributors,
                    lo = lo,
                    hi = hi
                );
                if let Some(mine) = self.rate() {
                    if (mine * 100.0 - pct as f64).abs() >= 30.0 {
                        line.push(' ');
                        line.push_str(&m!("standing_differs", pct = (mine * 100.0).round() as i64));
                    }
                }
                if pct >= OFFER_FLOOR {
                    return (true, line);
                }
                line.push(' ');
                line.push_str(&m!("standing_not_offered"));
                return (false, line);
            }
        }
        let Some(rate) = self.rate() else {
            return (true, m!("standing_too_few", v = self.vendor));
        };
        let pct = (rate * 100.0).round() as i64;
        if pct < OFFER_FLOOR {
            return (
                false,
                format!(
                    "{} {}",
                    m!("standing_rate_low", v = self.vendor, pct = pct),
                    m!("standing_not_offered")
                ),
            );
        }
        (true, m!("standing_rate", v = self.vendor, pct = pct))
    }
}

pub struct Ledger {
    path: PathBuf,
    rows: BTreeMap<String, (u32, u32)>,
}

impl Ledger {
    pub fn open(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut rows = BTreeMap::new();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&raw) {
                for (vendor, row) in o {
                    let g = |k: &str| row.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    rows.insert(vendor, (g("reports"), g("acted")));
                }
            }
        }
        Ledger { path, rows }
    }

    pub fn record(&mut self, vendor: &str, state: &str) -> Result<(), String> {
        let row = self.rows.entry(vendor.to_string()).or_insert((0, 0));
        row.0 += 1;
        if ACTED.contains(&state) {
            row.1 += 1;
        }
        let out: serde_json::Map<String, Value> = self
            .rows
            .iter()
            .map(|(v, (r, a))| (v.clone(), json!({"reports": r, "acted": a})))
            .collect();
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            &self.path,
            serde_json::to_string_pretty(&Value::Object(out)).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn standing(&self, vendor: &str) -> Standing {
        let (reports, acted) = self.rows.get(vendor).copied().unwrap_or((0, 0));
        Standing {
            vendor: vendor.to_string(),
            reports,
            acted,
        }
    }

    /// One entry per vendor. The caller submits them **separately**; batching
    /// them would disclose the set of vendors this client deals with, which is
    /// a sharper identifier than any single value in it.
    pub fn pending_contributions(&self) -> Vec<Value> {
        self.rows
            .iter()
            .filter(|(_, (reports, _))| *reports >= MIN_LOCAL_BASIS)
            .map(|(vendor, (reports, acted))| {
                json!({
                    "vendor": vendor,
                    "rate_pct": round_rate(*acted as f64 / *reports as f64),
                    "weight": weight_band(*reports),
                })
            })
            .collect()
    }
}

/// The published figure for one vendor, or `None` if the index cannot be asked.
/// A missing index is not an accusation: the caller falls back to local
/// experience and says which one it is using.
pub async fn network(vendor: &str, index_url: &str) -> Option<Value> {
    let url = format!("{}/index/{vendor}", index_url.trim_end_matches('/'));
    let resp = crate::http::client().get(&url).send().await.ok()?;
    crate::http::json_capped(resp, crate::http::MAX_BODY)
        .await
        .ok()
}

pub async fn contribute(c: &Value, index_url: &str) -> Option<Value> {
    let url = format!("{}/contribute", index_url.trim_end_matches('/'));
    let resp = crate::http::client().post(&url).json(c).send().await.ok()?;
    crate::http::json_capped(resp, crate::http::MAX_BODY)
        .await
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture path that is *empty*, which it was not.
    ///
    /// Keyed on the process id alone, and `Ledger::open` reads whatever is
    /// already there — so on a machine that recycles pids, a run inherited an
    /// earlier run's records and `reports == 4` became `reports == 8`. It
    /// passed alone and failed inside the suite, which is the worst shape a
    /// flake can take: the failure looks like the code and reproduces nowhere.
    /// Found with two hundred `/tmp/vs-ledger-*` directories still on disk.
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vs-ledger-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join(name);
        let _ = std::fs::remove_file(&p);
        p
    }

    /// G4: a vendor that never acts loses the button, and is told why. This is
    /// the whole two-way-measurement argument — without it the report channel
    /// asks users for effort a vendor has demonstrably never repaid.
    #[test]
    fn a_vendor_that_never_acts_loses_the_button() {
        let mut led = Ledger::open(tmp("silent.json"));
        for _ in 0..8 {
            led.record("Schweiger AG", "received").unwrap();
        }
        let (offer, note) = led.standing("Schweiger AG").advice(None);
        assert!(!offer, "the button was still offered: {note}");
        assert!(
            note.contains("0 %"),
            "the reason does not state the figure: {note}"
        );
    }

    /// Too little evidence is stated as too little evidence, not as a rate.
    #[test]
    fn below_the_basis_it_says_so_rather_than_quoting_a_percentage() {
        let mut led = Ledger::open(tmp("thin.json"));
        for _ in 0..3 {
            led.record("Kaum AG", "received").unwrap();
        }
        let s = led.standing("Kaum AG");
        assert_eq!(
            s.rate(),
            None,
            "a rate was computed from three observations"
        );
        let (offer, note) = s.advice(None);
        assert!(offer, "a vendor with no history was pre-judged");
        assert!(crate::msg::is("standing_too_few", &note), "{note}");
    }

    /// Acknowledgement is not action. An auto-reply must not score.
    #[test]
    fn only_a_state_that_changed_something_counts_as_acting() {
        let mut led = Ledger::open(tmp("mixed.json"));
        for state in ["received", "new", "received", "fixed_in"] {
            led.record("Teils AG", state).unwrap();
        }
        let s = led.standing("Teils AG");
        assert_eq!(s.reports, 4);
        assert_eq!(s.acted, 1, "an acknowledgement was counted as action");
    }

    /// The network median outranks local experience, but a contradiction is
    /// shown rather than quietly resolved.
    #[test]
    fn a_contradicting_local_reading_is_shown_beside_the_network_figure() {
        let mut led = Ledger::open(tmp("diverge.json"));
        for _ in 0..8 {
            led.record("Streit AG", "fixed_in").unwrap();
        }
        let published = json!({"published": true, "contributors": 9,
                               "rate_pct": 20, "spread_pct": [0, 40]});
        let (offer, note) = led.standing("Streit AG").advice(Some(&published));
        assert!(offer, "20 % is above the floor");
        assert!(
            note.contains(&m!(
                "standing_network",
                v = "Streit AG",
                pct = 20,
                n = 9,
                lo = 0,
                hi = 40
            )),
            "{note}"
        );
        assert!(
            note.contains(&m!("standing_differs", pct = 100)),
            "the local contradiction was hidden: {note}"
        );
    }

    /// G5: one vendor per contribution, and only the three declared fields.
    #[test]
    fn a_contribution_carries_one_vendor_and_nothing_else() {
        let mut led = Ledger::open(tmp("contrib.json"));
        for _ in 0..10 {
            led.record("ACME", "fixed_in").unwrap();
        }
        for _ in 0..2 {
            led.record("Zu Wenig AG", "received").unwrap();
        }
        let pending = led.pending_contributions();
        assert_eq!(
            pending.len(),
            1,
            "a vendor below the local basis contributed"
        );
        let c = pending[0].as_object().unwrap();
        let keys: Vec<&String> = c.keys().collect();
        assert_eq!(
            keys,
            vec!["rate_pct", "vendor", "weight"],
            "unexpected fields: {keys:?}"
        );
        assert_eq!(c["vendor"], "ACME");
        assert_eq!(c["rate_pct"], 100);
    }

    /// A rate is coarsened to 10 % before it leaves: a precise figure from few
    /// observations is identifying.
    #[test]
    fn a_contributed_rate_is_coarsened() {
        assert_eq!(round_rate(0.0), 0);
        assert_eq!(round_rate(0.44), 40);
        assert_eq!(round_rate(0.46), 50);
        assert_eq!(round_rate(1.0), 100);
    }
}
