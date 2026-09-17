//! A record of every change this client makes, and code that watches it.
//!
//! **A fix outlives its reason.** A setting changed today to work around a bug
//! is still set after the release that fixed the bug, an update may overwrite it
//! without anybody noticing, and a year later nobody remembers why it is there.
//! So before any mutating action runs, this writes down what is about to
//! change: which action, which file, where the backup is, whether it can be
//! undone, which package the fix is for and the version the package manager
//! reports, and — where the publisher said — the upstream issue and the version
//! that fixes it. If the record cannot be written, the action does not run.
//!
//! **Code writes every field a decision reads.** The package version comes from
//! the package manager, the path from resolving the file, the comparison from
//! `vercmp` below. A model may add an explanation (`note`), and nothing reads
//! it: version strings, package names and paths are exactly where a model is
//! wrong without anybody noticing.
//!
//! **Looking again never removes anything.** `review` compares what is installed
//! now with what was installed when the change was made, and flags a record for
//! a person to look at: the package was updated, the version upstream named as
//! the fix is installed, the change no longer holds, or the versions cannot be
//! compared. It does not undo the change. A backport, or a new version that
//! still fails, makes a version number no proof; undoing stays the person's
//! decision, through `restore_backup`.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "repairs.json";

/// What a publisher may say about the software a change is for. All optional,
/// and none of it is trusted as a fact about this machine: the package name is
/// only ever used to *ask* the package manager.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Upstream {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The upstream issue or pull request the change works around.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    /// The first version the publisher says carries the fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_in: Option<String>,
}

impl Upstream {
    /// Each field checked as the text it has to be, or refused. A field that
    /// would need escaping to be passed on is refused rather than escaped.
    pub fn from_value(v: Option<&Value>) -> Result<Upstream, String> {
        let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Upstream::default()) };
        let obj = v.as_object().ok_or_else(|| m!("repair_upstream_bad", k = "upstream"))?;
        let mut out = Upstream::default();
        for (k, val) in obj {
            let s = val.as_str().ok_or_else(|| m!("repair_upstream_bad", k = k))?;
            let ok = match k.as_str() {
                "package" => crate::provenance::name_ok(s),
                "issue" => s.len() <= 300 && s.starts_with("https://")
                    && !s.chars().any(|c| c.is_whitespace() || c.is_control()),
                "fixed_in" => version_ok(s),
                _ => false,
            };
            if !ok {
                return Err(m!("repair_upstream_bad", k = k));
            }
            match k.as_str() {
                "package" => out.package = Some(s.into()),
                "issue" => out.issue = Some(s.into()),
                _ => out.fixed_in = Some(s.into()),
            }
        }
        Ok(out)
    }
}

/// What a version string may be before it is compared or shown.
fn version_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || ".:-+_~".contains(c))
}

/// What the package manager said, and which one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Installed {
    pub version: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub id: String,
    /// Seconds since the Unix epoch.
    pub at: u64,
    pub action: String,
    pub params: BTreeMap<String, String>,
    /// The file, resolved — not the relative name the publisher wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    pub reversible: bool,
    /// Who proposed it: the vendor or project the window was talking to.
    pub subject: String,
    #[serde(default)]
    pub upstream: Upstream,
    /// Read from the package manager when the record was written. `None` when
    /// no package was named, or the package manager does not know it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<Installed>,
    /// `pending` until the action returns, then `applied` or `failed`;
    /// `undone` once the backup was put back.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// A person looked at this record while this version was installed (empty
    /// where no package was named), and kept the change. The flags they saw
    /// then are in `seen`, and are not raised again at that version; anything
    /// new, or any later update, is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looked_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<String>,
    /// The action call that reverses a change with no file to restore — a
    /// service's start type, a machine variable — from the reading before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo: Option<Value>,
    /// Free text a model may add. Shown, never read by anything here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Flag {
    /// The package is at another version than when the change was made.
    Updated { from: String, to: String },
    /// The installed version is at or past the one upstream named as the fix,
    /// so the change may no longer be needed. Not a verdict: a backport or a
    /// fix that did not hold looks the same from here.
    UpstreamSaysFixed { fixed_in: String, installed: String },
    /// The versions cannot be ordered — a build from a repository's head, or a
    /// package the package manager no longer knows.
    CannotCompare { why: String },
    /// The change is no longer in the file; something rewrote it.
    Overwritten,
    /// A setting outside any file is no longer what the change set.
    NoLongerSet,
    /// The file the change was made in is gone.
    TargetGone,
    /// The copy an undo would restore is gone.
    BackupGone,
}

impl Flag {
    fn kind(&self) -> &'static str {
        match self {
            Flag::Updated { .. } => "updated",
            Flag::UpstreamSaysFixed { .. } => "upstream_says_fixed",
            Flag::CannotCompare { .. } => "cannot_compare",
            Flag::Overwritten => "overwritten",
            Flag::NoLongerSet => "no_longer_set",
            Flag::TargetGone => "target_gone",
            Flag::BackupGone => "backup_gone",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Review {
    pub record: Record,
    pub flags: Vec<Flag>,
}

// ------------------------------------------------------------------ storage

fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// The records, or why they cannot be read.
///
/// **A list that cannot be read is not an empty list.** Read as empty, the next
/// change would write a new list over it and every earlier record would be
/// gone — so a change is refused instead, and the file is left for a person.
fn read(root: &Path) -> Result<Vec<Record>, String> {
    let text = match std::fs::read_to_string(path(root)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(m!("repair_not_written", e = e)),
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("records").cloned())
        .and_then(|r| serde_json::from_value(r).ok())
        .ok_or_else(|| m!("repair_not_written", e = format!("{} is not readable", path(root).display())))
}

pub fn load(root: &Path) -> Vec<Record> {
    read(root).unwrap_or_default()
}

/// Written whole to a file beside it and moved into place, so a crash leaves
/// the old list or the new one and never half of either.
fn save(root: &Path, records: &[Record]) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| m!("repair_not_written", e = e))?;
    let tmp = root.join(format!("{FILE}.tmp"));
    let body = serde_json::to_string_pretty(&serde_json::json!({ "records": records }))
        .map_err(|e| m!("repair_not_written", e = e))?;
    std::fs::write(&tmp, body).map_err(|e| m!("repair_not_written", e = e))?;
    std::fs::rename(&tmp, path(root)).map_err(|e| m!("repair_not_written", e = e))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ------------------------------------------------------------------ the package manager

fn tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The installed version of `package`, from the local package database only.
///
/// `pacman -Q <name>` and `dpkg-query -W -f=${Version}`: one package, no
/// network, no write — the same bounds as the provenance question.
pub fn installed_version(package: &str) -> Option<Installed> {
    if !crate::provenance::name_ok(package) {
        return None;
    }
    if let Some(exe) = tool("pacman") {
        if let Some((true, out)) = crate::reads::run_bounded(&exe, &["-Q", package]) {
            let mut words = out.split_whitespace();
            if words.next() == Some(package) {
                if let Some(v) = words.next().filter(|v| version_ok(v)) {
                    return Some(Installed { version: v.into(), source: "pacman".into() });
                }
            }
        }
    }
    if let Some(exe) = tool("dpkg-query") {
        if let Some((true, out)) =
            crate::reads::run_bounded(&exe, &["-W", "-f=${Version}\n", package])
        {
            if let Some(v) = out.lines().next().map(str::trim).filter(|v| version_ok(v)) {
                return Some(Installed { version: v.into(), source: "dpkg".into() });
            }
        }
    }
    None
}

// ------------------------------------------------------------------ versions

/// pacman's `rpmvercmp` on one part of a version: runs of digits compare as
/// numbers, runs of letters as text, and a run of letters is older than none —
/// `1.0rc` comes before `1.0`.
fn rpmvercmp(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (si, sj) = (i, j);
        while i < a.len() && !a[i].is_ascii_alphanumeric() {
            i += 1;
        }
        while j < b.len() && !b[j].is_ascii_alphanumeric() {
            j += 1;
        }
        if i >= a.len() || j >= b.len() {
            break;
        }
        if i - si != j - sj {
            return (i - si).cmp(&(j - sj));
        }
        let (ai, bj) = (i, j);
        let numeric = a[i].is_ascii_digit();
        let same: fn(&u8) -> bool = if numeric { u8::is_ascii_digit } else { u8::is_ascii_alphabetic };
        while i < a.len() && same(&a[i]) {
            i += 1;
        }
        while j < b.len() && same(&b[j]) {
            j += 1;
        }
        if j == bj {
            // Different kinds of run: a number is newer than letters.
            return if numeric { Ordering::Greater } else { Ordering::Less };
        }
        let (mut x, mut y) = (&a[ai..i], &b[bj..j]);
        if numeric {
            while x.len() > 1 && x[0] == b'0' {
                x = &x[1..];
            }
            while y.len() > 1 && y[0] == b'0' {
                y = &y[1..];
            }
            if x.len() != y.len() {
                return x.len().cmp(&y.len());
            }
        }
        match x.cmp(y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    let (a_done, b_done) = (i >= a.len(), j >= b.len());
    if a_done && b_done {
        return Ordering::Equal;
    }
    // Whatever is left decides, and letters left over never beat nothing.
    if (a_done && !(j < b.len() && b[j].is_ascii_alphabetic()))
        || (i < a.len() && a[i].is_ascii_alphabetic())
    {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// `epoch:version-release`, as pacman splits it.
fn split_evr(s: &str) -> (&str, &str, Option<&str>) {
    let (epoch, rest) = match s.split_once(':') {
        Some((e, r)) if !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit()) => (e, r),
        _ => ("0", s),
    };
    match rest.rsplit_once('-') {
        Some((v, r)) => (epoch, v, Some(r)),
        None => (epoch, rest, None),
    }
}

/// A build from a repository's head rather than a release: its version names a
/// commit, and no release number can be said to be before or after it.
pub fn is_vcs(package: Option<&str>, version: &str) -> bool {
    let vcs_package = package.is_some_and(|p| {
        ["-git", "-svn", "-hg", "-bzr", "-darcs", "-fossil"].iter().any(|s| p.ends_with(s))
    });
    let vcs_version = Regex::new(r"(?i)(\.r\d+\.g[0-9a-f]{7,}|[+~.]git|[+~]svn|[+~]hg)")
        .map(|re| re.is_match(version))
        .unwrap_or(false);
    vcs_package || vcs_version
}

/// pacman's `vercmp`: epoch first, then the version, then the release where
/// both have one. `None` where the two cannot be ordered honestly.
pub fn vercmp(package: Option<&str>, a: &str, b: &str) -> Option<Ordering> {
    if !version_ok(a) || !version_ok(b) || is_vcs(package, a) || is_vcs(package, b) {
        return None;
    }
    let (ea, va, ra) = split_evr(a);
    let (eb, vb, rb) = split_evr(b);
    Some(
        rpmvercmp(ea, eb)
            .then_with(|| rpmvercmp(va, vb))
            .then_with(|| match (ra, rb) {
                (Some(x), Some(y)) => rpmvercmp(x, y),
                _ => Ordering::Equal,
            }),
    )
}

// ------------------------------------------------------------------ writing

/// The context a change is made in, from the window: who proposed it, what the
/// publisher said about the software, and an optional explanation.
pub struct Context<'a> {
    pub subject: &'a str,
    pub upstream: Upstream,
    pub note: Option<String>,
    /// The record this change reverses. It is marked undone, and this change
    /// is kept as the undo it is rather than reviewed as a fix of its own.
    pub undoes: Option<String>,
}

fn resolve(root: &Path, file: &str) -> Option<PathBuf> {
    root.canonicalize().ok().map(|r| r.join(file)).and_then(|p| p.canonicalize().ok())
}

fn backup_of(target: &Path) -> PathBuf {
    target.with_extension(format!(
        "{}.bak",
        target.extension().and_then(|e| e.to_str()).unwrap_or("")
    ))
}

/// Write the record, then run the action, then write how it went.
///
/// **The record comes first.** A change that could be made without a record is
/// the change nobody can explain a year later, so a record that cannot be
/// written stops the action before it touches anything.
pub fn execute(
    id: &str,
    params: &Value,
    root: &Path,
    state_dir: &Path,
    ctx: Context,
    installed: &dyn Fn(&str) -> Option<Installed>,
) -> Result<Value, String> {
    execute_with(id, params, root, state_dir, ctx, installed,
                 &|| crate::actions::execute(id, params, root))
}

/// `execute`, with the step that changes the machine passed in — so the suite
/// can look at the record from inside that step.
fn execute_with(
    id: &str,
    params: &Value,
    root: &Path,
    state_dir: &Path,
    ctx: Context,
    installed: &dyn Fn(&str) -> Option<Installed>,
    apply: &dyn Fn() -> Result<Value, String>,
) -> Result<Value, String> {
    let spec = crate::actions::spec(id);
    let checked = crate::actions::validate(id, params)?;
    if !spec.is_some_and(|s| s.mutating) {
        return crate::actions::execute(id, params, root);
    }
    if id == "restore_backup" {
        let out = crate::actions::execute(id, params, root)?;
        if let Some(rid) = ctx.undoes.as_deref() {
            let _ = mark_undone_by_id(state_dir, rid);
            return Ok(out);
        }
        // The file is back either way. A record that cannot be updated goes on
        // being reviewed, and says the change no longer holds — which is true.
        let _ = mark_undone(state_dir, root, &checked["file"]);
        return Ok(out);
    }

    let target = checked.get("file").and_then(|f| resolve(root, f));
    let mut records = read(state_dir)?;
    let record = Record {
        id: format!("{}-{}", now(), records.len() + 1),
        at: now(),
        action: id.into(),
        params: checked.clone(),
        backup: target.as_deref().map(|t| backup_of(t).display().to_string()),
        target: target.as_deref().map(|t| t.display().to_string()),
        reversible: spec.is_some_and(|s| s.reversible),
        subject: ctx.subject.into(),
        installed: ctx.upstream.package.as_deref().and_then(installed),
        upstream: ctx.upstream,
        state: "pending".into(),
        error: None,
        looked_at: None,
        seen: vec![],
        undo: None,
        note: ctx.note,
    };
    let rid = record.id.clone();
    records.push(record);
    save(state_dir, &records)?;

    let result = apply();
    if let Some(r) = records.iter_mut().find(|r| r.id == rid) {
        match &result {
            Ok(v) => {
                r.state = if ctx.undoes.is_some() { "undo" } else { "applied" }.into();
                r.undo = v.get("undo").filter(|u| !u.is_null()).cloned();
            }
            Err(e) => {
                r.state = "failed".into();
                r.error = Some(e.clone());
            }
        }
    }
    // The change has happened either way; a record left at `pending` still says
    // what was attempted, which is the part that matters.
    let _ = save(state_dir, &records);
    if let (Ok(_), Some(undone)) = (&result, ctx.undoes.as_deref()) {
        let _ = mark_undone_by_id(state_dir, undone);
    }
    result.map(|mut v| {
        if let Some(o) = v.as_object_mut() {
            o.insert("repair_record".into(), Value::String(rid));
        }
        v
    })
}

fn mark_undone_by_id(state_dir: &Path, id: &str) -> Result<(), String> {
    let mut records = read(state_dir)?;
    if let Some(r) = records.iter_mut().find(|r| r.id == id && r.state == "applied") {
        r.state = "undone".into();
        return save(state_dir, &records);
    }
    Ok(())
}

/// The newest applied change to that file is the one an undo reverses.
///
/// Compared as resolved paths on both sides: on Windows one spelling of a path
/// carries `\\?\` and another does not, and they are the same file.
fn mark_undone(state_dir: &Path, root: &Path, file: &str) -> Result<(), String> {
    let Some(target) = resolve(root, file) else { return Ok(()) };
    let same = |t: &str| Path::new(t).canonicalize().is_ok_and(|p| p == target);
    let mut records = read(state_dir)?;
    if let Some(r) = records
        .iter_mut()
        .rev()
        .find(|r| r.state == "applied" && r.target.as_deref().is_some_and(same))
    {
        r.state = "undone".into();
        return save(state_dir, &records);
    }
    Ok(())
}

// ------------------------------------------------------------------ looking again

/// Whether a change still says what it said when it was made.
fn still_holds(r: &Record, target: &Path) -> Option<bool> {
    match r.action.as_str() {
        "set_config_key" => {
            let text = std::fs::read_to_string(target).ok()?;
            let re = Regex::new(&format!(
                r"(?m)^\s*{}\s*=\s*{}\s*$",
                regex::escape(r.params.get("key")?),
                regex::escape(r.params.get("value")?)
            ))
            .ok()?;
            Some(re.is_match(&text))
        }
        _ => None,
    }
}

/// What is true of one record now, before anything a person already saw is
/// taken away. Also the version the package manager reports, or empty.
fn flags_for(r: &Record, installed: &dyn Fn(&str) -> Option<Installed>) -> (Vec<Flag>, String) {
    let mut flags = vec![];
    if let Some(t) = r.target.as_deref().map(Path::new) {
        if !t.exists() {
            flags.push(Flag::TargetGone);
        } else if still_holds(r, t) == Some(false) {
            flags.push(Flag::Overwritten);
        }
    }
    if r.reversible && r.backup.as_deref().is_some_and(|b| !Path::new(b).exists()) {
        flags.push(Flag::BackupGone);
    }
    if r.target.is_none() && crate::elevated::is_elevated(&r.action)
        && crate::elevate::still_holds(&r.action, &r.params) == Some(false)
    {
        flags.push(Flag::NoLongerSet);
    }

    let package = r.upstream.package.as_deref();
    let now = package.and_then(installed);
    let then = r.installed.as_ref().map(|i| i.version.clone());
    match (&then, &now) {
        (_, None) if package.is_some() => flags.push(Flag::CannotCompare {
            why: m!("repair_not_installed", p = package.unwrap_or("")),
        }),
        (Some(from), Some(to)) if from != &to.version => {
            flags.push(Flag::Updated { from: from.clone(), to: to.version.clone() });
        }
        _ => {}
    }
    if let (Some(fixed), Some(now)) = (r.upstream.fixed_in.as_deref(), &now) {
        match vercmp(package, &now.version, fixed) {
            Some(Ordering::Less) => {}
            Some(_) => flags.push(Flag::UpstreamSaysFixed {
                fixed_in: fixed.into(),
                installed: now.version.clone(),
            }),
            None => flags.push(Flag::CannotCompare { why: m!("repair_vcs", v = &now.version) }),
        }
    }
    (flags, now.map(|n| n.version).unwrap_or_default())
}

/// Every applied change that needs a person to look at it again, and why.
///
/// Reads the package manager and the files; writes nothing and removes
/// nothing. What a person already saw at the version installed now is left
/// out; anything new, and everything after an update, is not.
pub fn review(state_dir: &Path, installed: &dyn Fn(&str) -> Option<Installed>) -> Vec<Review> {
    let mut out = vec![];
    for r in load(state_dir).into_iter().filter(|r| r.state == "applied") {
        let (mut flags, version) = flags_for(&r, installed);
        if r.looked_at.as_deref() == Some(version.as_str()) {
            flags.retain(|f| !r.seen.iter().any(|k| k == f.kind()));
        }
        if !flags.is_empty() {
            out.push(Review { record: r, flags });
        }
    }
    out
}

/// A person looked at a change and keeps it: what they were shown is not
/// raised again at the version installed now. A later update raises it
/// afresh, and so does anything they were not shown.
pub fn looked_at(
    state_dir: &Path,
    id: &str,
    installed: &dyn Fn(&str) -> Option<Installed>,
) -> Result<(), String> {
    let mut records = read(state_dir)?;
    let r = records.iter_mut().find(|r| r.id == id).ok_or_else(|| m!("repair_unknown"))?;
    let (flags, version) = flags_for(r, installed);
    r.looked_at = Some(version);
    r.seen = flags.iter().map(|f| f.kind().to_string()).collect();
    save(state_dir, &records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("podshl-repair-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn ctx(package: &str, fixed_in: Option<&str>) -> Context<'static> {
        Context {
            subject: "example.org",
            upstream: Upstream {
                package: Some(package.into()),
                issue: Some("https://github.com/example/app/issues/12".into()),
                fixed_in: fixed_in.map(Into::into),
            },
            note: Some("a model's words, which nothing reads".into()),
            undoes: None,
        }
    }

    fn at(v: &str) -> impl Fn(&str) -> Option<Installed> + '_ {
        move |_| Some(Installed { version: v.into(), source: "pacman".into() })
    }

    /// RR1: the record is on disk before the file is touched, and says what a
    /// person needs a year later — with every field a decision reads written
    /// by code.
    #[test]
    fn the_record_is_written_before_the_change_and_by_code() {
        let root = dir("rr1");
        let file = root.join("app.toml");
        std::fs::write(&file, "modeset = 0\n").unwrap();

        // The version is read before the change, and the record is on disk
        // when the change starts: the step that changes the file looks.
        let seen_before = RefCell::new(None);
        let reader = |_: &str| {
            *seen_before.borrow_mut() = Some(std::fs::read_to_string(&file).unwrap());
            Some(Installed { version: "1:2.4.0-3".into(), source: "pacman".into() })
        };
        let params = json!({"file": "app.toml", "key": "modeset", "value": "1"});
        let on_disk = RefCell::new(vec![]);
        let out = execute_with("set_config_key", &params, &root, &root,
                               ctx("app", Some("2.5.0")), &reader, &|| {
            *on_disk.borrow_mut() = load(&root);
            crate::actions::execute("set_config_key", &params, &root)
        }).unwrap();
        assert_eq!(seen_before.borrow().as_deref(), Some("modeset = 0\n"),
                   "the version was read after the change, not before it");
        let pending = on_disk.borrow();
        assert!(pending.len() == 1 && pending[0].state == "pending",
                "the change started without its record on disk: {pending:?}");

        let records = load(&root);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(out["repair_record"], r.id.as_str());
        assert_eq!(r.state, "applied");
        assert_eq!(r.action, "set_config_key");
        assert_eq!(r.installed.as_ref().unwrap().version, "1:2.4.0-3");
        let target = file.canonicalize().unwrap();
        assert_eq!(r.target.as_deref(), Some(target.display().to_string().as_str()),
                   "the path is not the resolved one");
        assert!(r.backup.as_deref().is_some_and(|b| Path::new(b).exists()), "{r:?}");
        assert!(r.reversible);
        assert_eq!(r.upstream.issue.as_deref(), Some("https://github.com/example/app/issues/12"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RR2: no record, no change. A ledger that cannot be written stops the
    /// action before it touches the file.
    #[test]
    fn a_record_that_cannot_be_written_stops_the_change() {
        let root = dir("rr2");
        std::fs::write(root.join("app.toml"), "modeset = 0\n").unwrap();
        // A file where the state directory should be: nothing can be written.
        let blocked = root.join("state-is-a-file");
        std::fs::write(&blocked, "").unwrap();
        let err = execute("set_config_key",
                          &json!({"file": "app.toml", "key": "modeset", "value": "1"}),
                          &root, &blocked, ctx("app", None), &at("1.0-1")).unwrap_err();
        assert!(crate::msg::is("repair_not_written", &err), "{err}");
        assert_eq!(std::fs::read_to_string(root.join("app.toml")).unwrap(), "modeset = 0\n",
                   "the change was made without a record");
        assert!(!root.join("app.toml.bak").exists(), "the action ran as far as its backup");

        // A list that cannot be read is not an empty one: nothing is changed,
        // and the file is left as it was for a person to look at.
        let state = root.join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join(FILE), "{ not json").unwrap();
        let err = execute("set_config_key",
                          &json!({"file": "app.toml", "key": "modeset", "value": "1"}),
                          &root, &state, ctx("app", None), &at("1.0-1")).unwrap_err();
        assert!(crate::msg::is("repair_not_written", &err), "{err}");
        assert_eq!(std::fs::read_to_string(state.join(FILE)).unwrap(), "{ not json",
                   "an unreadable list was written over");
        assert_eq!(std::fs::read_to_string(root.join("app.toml")).unwrap(), "modeset = 0\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RR3: an update, the version upstream named, and a change something
    /// rewrote are each flagged; looking again never removes anything; and a
    /// person who looked is not asked again until the next update.
    #[test]
    fn looking_again_flags_and_never_removes() {
        let root = dir("rr3");
        let file = root.join("app.toml");
        std::fs::write(&file, "modeset = 0\n").unwrap();
        execute("set_config_key", &json!({"file": "app.toml", "key": "modeset", "value": "1"}),
                &root, &root, ctx("app", Some("2.5.0")), &at("2.4.0-3")).unwrap();
        let changed = std::fs::read_to_string(&file).unwrap();
        let backup = std::fs::read_to_string(root.join("app.toml.bak")).unwrap();

        assert!(review(&root, &at("2.4.0-3")).is_empty(), "nothing changed, yet a flag");

        let flags = |v: &str| review(&root, &at(v)).pop().map(|r| r.flags).unwrap_or_default();
        assert_eq!(flags("2.4.0-4"), vec![Flag::Updated { from: "2.4.0-3".into(), to: "2.4.0-4".into() }],
                   "a new pkgrel is an update and nothing more");
        let fixed = flags("2.5.0-1");
        assert!(fixed.contains(&Flag::UpstreamSaysFixed { fixed_in: "2.5.0".into(), installed: "2.5.0-1".into() }),
                "{fixed:?}");
        assert!(matches!(flags("2.6.r12.g1a2b3c4-1")[..], [Flag::Updated { .. }, Flag::CannotCompare { .. }]),
                "a build from head was compared as if it were a release");

        // Nothing was touched by any of that.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), changed);
        assert_eq!(std::fs::read_to_string(root.join("app.toml.bak")).unwrap(), backup);

        // Looked at, at 2.5.0-1: quiet at that version, raised again at the next.
        let id = load(&root)[0].id.clone();
        looked_at(&root, &id, &at("2.5.0-1")).unwrap();
        assert!(flags("2.5.0-1").is_empty(), "{:?}", flags("2.5.0-1"));
        assert!(!flags("2.5.1-1").is_empty(), "an update after the look raised nothing");

        // An update that rewrote the file: new, so shown although the
        // version is the one the person looked at.
        std::fs::write(&file, "modeset = 0\n").unwrap();
        assert_eq!(flags("2.5.0-1"), vec![Flag::Overwritten]);

        // Kept again, and that is not asked about at this version any more —
        // not at every start for as long as the file stays as it is.
        looked_at(&root, &id, &at("2.5.0-1")).unwrap();
        assert!(flags("2.5.0-1").is_empty(), "{:?}", flags("2.5.0-1"));
        std::fs::write(&file, "modeset = 1\n").unwrap();

        // And an undo takes the record off the list, without deleting it.
        execute("restore_backup", &json!({"file": "app.toml"}), &root, &root,
                ctx("app", None), &at("2.5.0-1")).unwrap();
        assert!(review(&root, &at("9.9-1")).is_empty());
        assert_eq!(load(&root)[0].state, "undone");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RR4: pacman's ordering, including the cases a string comparison or a
    /// naive split gets wrong.
    ///
    /// Every expectation below is what `vercmp` from `archlinux:base` printed
    /// on 2026-09-17 — not what seemed right. One of them did not: `1.5.a` is
    /// *newer* than `1.5`, while `1.5b` is older.
    #[test]
    fn versions_are_ordered_as_pacman_orders_them() {
        use Ordering::*;
        let cases = [
            ("1.5.0", "1.5.0", Equal),
            ("1.5.1", "1.5.0", Greater),
            ("1.5.1", "1.5", Greater),
            ("1.10", "1.9", Greater),
            ("1.001", "1.1", Equal),
            ("1.0rc", "1.0", Less),
            ("1.0a", "1.0alpha", Less),
            ("1.0alpha", "1.0b", Less),
            ("1.0beta", "1.0rc", Less),
            ("1.5b", "1.5.1", Less),
            ("1.5b", "1.5", Less),
            ("1.5.a", "1.5", Greater),
            ("1.5.b", "1.5.a", Greater),
            ("1.5.", "1.5.a", Greater),
            ("1.5.", "1.5.1", Less),
            ("1.5a", "1.5.a", Less),
            ("1.0.", "1.0", Greater),
            ("1.0~rc1", "1.0", Greater),
            ("1.0_1", "1.0.1", Equal),
            ("1..0", "1.0", Greater),
            ("1:1.0", "2.0", Greater),
            ("1:1.0", "1:1.1", Less),
            ("1.0-2", "1.0-1", Greater),
            ("1.0-1", "1.0", Equal),
            ("1.0-1", "1.0.1", Less),
            ("2.4.0-3", "2.5.0", Less),
            ("2.5.0-1", "2.5.0", Equal),
        ];
        for (a, b, want) in cases {
            assert_eq!(vercmp(None, a, b), Some(want), "{a} vs {b}");
            assert_eq!(vercmp(None, b, a), Some(want.reverse()), "{b} vs {a}");
        }
        for vcs in ["2.6.r12.g1a2b3c4-1", "1.0+git20240101-1", "0.9~git3-2"] {
            assert_eq!(vercmp(None, vcs, "1.0"), None, "{vcs} was ordered");
        }
        assert_eq!(vercmp(Some("hyprland-git"), "0.45.0-1", "0.45.0"), None,
                   "a -git package's version was trusted as a release");
        assert_eq!(vercmp(None, "1.0; rm -rf", "1.0"), None);
    }

    /// RR5: what a publisher says about the software is checked as text before
    /// anything uses it.
    #[test]
    fn a_publisher_s_upstream_is_checked_as_text() {
        let ok = Upstream::from_value(Some(&json!({
            "package": "hyprland", "issue": "https://github.com/hyprwm/Hyprland/issues/1",
            "fixed_in": "0.45.0"}))).unwrap();
        assert_eq!(ok.package.as_deref(), Some("hyprland"));
        for bad in [json!({"package": "a b"}), json!({"issue": "http://x"}),
                    json!({"fixed_in": "1.0; x"}), json!({"version": "1"}),
                    json!({"package": 3}), json!("hyprland")] {
            assert!(Upstream::from_value(Some(&bad)).is_err(), "accepted {bad}");
        }
        assert_eq!(Upstream::from_value(None).unwrap(), Upstream::default());
    }
}
