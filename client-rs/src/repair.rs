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
//!
//! **Changes this client did not make are recorded the same way.** An agent, a
//! skill or a person can register a fix made by other means — a file edited, a
//! package built locally in place of the official one, a plugin or setting
//! copied so that it overrides the original — through `podshl-client repairs
//! add` (`repairs_cli.rs`). The tool names the thing; this code reads every
//! value a later decision depends on: the file's digest, the installed and the
//! available version, the original's digest.
//!
//! **Looking again runs without the window**, after an update: Omarchy's
//! `post-update.d`, a systemd user timer, a launchd agent or a Windows scheduled
//! task start `podshl-client repairs review`. It needs no model and no network,
//! except for an upstream issue the person asked to watch (`upstream.rs`).

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
        let Some(v) = v.filter(|v| !v.is_null()) else {
            return Ok(Upstream::default());
        };
        let obj = v
            .as_object()
            .ok_or_else(|| m!("repair_upstream_bad", k = "upstream"))?;
        let mut out = Upstream::default();
        for (k, val) in obj {
            let s = val
                .as_str()
                .ok_or_else(|| m!("repair_upstream_bad", k = k))?;
            let ok = match k.as_str() {
                "package" => crate::provenance::name_ok(s),
                "issue" => {
                    s.len() <= 300
                        && s.starts_with("https://")
                        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
                }
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
    !s.is_empty()
        && s.len() <= 100
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".:-+_~".contains(c))
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
    /// `action` for a change this client made; `file`, `package` or `overlay`
    /// for one registered from outside (`add_external`).
    #[serde(default = "kind_action")]
    pub kind: String,
    /// The changed file's SHA-256 once the change was made, so a later edit or
    /// an update that rewrites it is noticed whatever the file's format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_sha256: Option<String>,
    /// What an override stands in front of, as it was when the copy was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<Original>,
    /// The person asked for `upstream.issue` to be looked up. Off unless they
    /// did: each lookup tells GitHub which issue this machine follows.
    #[serde(default)]
    pub watch_issue: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_state: Option<IssueState>,
    /// When the record was forgotten, if it was. What it said is gone; that it
    /// existed and was removed is not — see `forget`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forgotten_at: Option<u64>,
    /// Declared by a package rather than recorded by a person or an agent —
    /// see `declared.rs`. What was declared, by which file, and what became of
    /// the declaration since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<Declared>,
}

/// Where a declared record came from, and what its declaration says now.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Declared {
    /// The declaration file, resolved.
    pub file: String,
    /// The entry's `id` in that file: the record's identity across updates.
    pub entry: String,
    /// The package that owns the declaration file, as the package manager
    /// says — never as the file says. `None` for a declaration no package
    /// owns, such as one an installer put into the home directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The maintainer withdrew the entry: their reason, or empty when the
    /// entry simply is not declared any more while the package still is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired: Option<String>,
    /// The declaration file is gone: the package that declared this was
    /// removed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
}

fn kind_action() -> String {
    "action".into()
}

/// The component a local copy overrides, read when the copy was recorded.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Original {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// What the upstream issue or pull request said the last time it was asked.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct IssueState {
    pub checked_at: u64,
    /// `open`, `closed` (an issue) or `merged` (a pull request); `unknown` when
    /// the lookup failed, with the reason in `error`.
    pub state: String,
    #[serde(default)]
    pub is_pull: bool,
    /// The first release that contains the merged change, found by code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IssueState {
    /// Nothing further can change what it says: released, or a closed issue.
    fn settled(&self) -> bool {
        self.released_in.is_some() || (self.state == "closed" && !self.is_pull)
    }
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
    /// The file a registered fix changed is no longer what it was afterwards.
    FileChanged,
    /// A package built or pinned locally is older than the one the package
    /// manager offers: the official component moved on without this machine.
    Frozen {
        installed: String,
        available: String,
    },
    /// The component an override stands in front of has changed underneath it.
    OriginalChanged,
    /// The upstream issue is closed.
    IssueClosed,
    /// The upstream pull request is merged and in no release yet.
    PrMerged,
    /// The merged change is in this release, found by code.
    ReleasedIn { tag: String },
    /// The package's maintainer withdrew what the package declared — with
    /// their reason, or none when the entry was simply dropped.
    Retired { reason: String },
    /// The package that declared this is no longer installed.
    DeclarerGone,
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
            Flag::FileChanged => "file_changed",
            Flag::Frozen { .. } => "frozen",
            Flag::OriginalChanged => "original_changed",
            Flag::IssueClosed => "issue_closed",
            Flag::PrMerged => "pr_merged",
            Flag::ReleasedIn { .. } => "released_in",
            Flag::Retired { .. } => "retired",
            Flag::DeclarerGone => "declarer_gone",
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
pub(crate) fn read(root: &Path) -> Result<Vec<Record>, String> {
    let text = match std::fs::read_to_string(path(root)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(m!("repair_not_written", e = e)),
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("records").cloned())
        .and_then(|r| serde_json::from_value(r).ok())
        .ok_or_else(|| {
            m!(
                "repair_not_written",
                e = format!("{} is not readable", path(root).display())
            )
        })
}

pub fn load(root: &Path) -> Vec<Record> {
    read(root).unwrap_or_default()
}

/// Written whole to a file beside it and moved into place, so a crash leaves
/// the old list or the new one and never half of either.
pub(crate) fn save(root: &Path, records: &[Record]) -> Result<(), String> {
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
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The installed version of `package`, from the local package database only.
///
/// `pacman -Q`, `dpkg-query -W`, `brew list --versions`, and on Windows the
/// uninstall entries in the registry: one package, no network, no write — the
/// same bounds as the provenance question.
pub fn installed_version(package: &str) -> Option<Installed> {
    if !crate::provenance::name_ok(package) {
        return None;
    }
    if let Some(exe) = tool("pacman") {
        if let Some((true, out)) = crate::reads::run_bounded(&exe, &["-Q", package]) {
            let mut words = out.split_whitespace();
            if words.next() == Some(package) {
                if let Some(v) = words.next().filter(|v| version_ok(v)) {
                    return Some(Installed {
                        version: v.into(),
                        source: "pacman".into(),
                    });
                }
            }
        }
    }
    if let Some(exe) = tool("dpkg-query") {
        if let Some((true, out)) =
            crate::reads::run_bounded(&exe, &["-W", "-f=${Version}\n", package])
        {
            if let Some(v) = out.lines().next().map(str::trim).filter(|v| version_ok(v)) {
                return Some(Installed {
                    version: v.into(),
                    source: "dpkg".into(),
                });
            }
        }
    }
    if let Some(exe) = tool("brew") {
        if let Some((true, out)) = crate::reads::run_bounded(&exe, &["list", "--versions", package])
        {
            // `name 1.2.3 1.2.2` — the newest is last.
            let mut words = out.lines().next().unwrap_or("").split_whitespace();
            if words.next() == Some(package) {
                if let Some(v) = words.last().filter(|v| version_ok(v)) {
                    return Some(Installed {
                        version: v.into(),
                        source: "brew".into(),
                    });
                }
            }
        }
    }
    #[cfg(windows)]
    if let Some(v) = windows_uninstall_version(package) {
        return Some(Installed {
            version: v,
            source: "windows".into(),
        });
    }
    None
}

/// The version the package manager would install now, from its local
/// database — `pacman -Si`, `apt-cache policy`, `brew info`. No network: the
/// database is as fresh as the last update, which is when this is asked.
/// Windows has no such database for arbitrary programs, so it says nothing.
pub fn available_version(package: &str) -> Option<Installed> {
    if !crate::provenance::name_ok(package) {
        return None;
    }
    if let Some(exe) = tool("pacman") {
        if let Some((true, out)) = crate::reads::run_bounded(&exe, &["-Si", package]) {
            if let Some(v) = field(&out, "Version").filter(|v| version_ok(v)) {
                return Some(Installed {
                    version: v,
                    source: "pacman".into(),
                });
            }
        }
    }
    if let Some(exe) = tool("apt-cache") {
        if let Some((true, out)) = crate::reads::run_bounded(&exe, &["policy", package]) {
            if let Some(v) = field(&out, "Candidate").filter(|v| version_ok(v)) {
                return Some(Installed {
                    version: v,
                    source: "apt".into(),
                });
            }
        }
    }
    if let Some(exe) = tool("brew") {
        if let Some((true, out)) = crate::reads::run_bounded(&exe, &["info", "--json=v2", package])
        {
            let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
            let stable = v["formulae"][0]["versions"]["stable"]
                .as_str()
                .or_else(|| v["casks"][0]["version"].as_str());
            if let Some(stable) = stable.filter(|s| version_ok(s)) {
                return Some(Installed {
                    version: stable.into(),
                    source: "brew".into(),
                });
            }
        }
    }
    None
}

/// `key : value`, first match, as pacman and apt print them.
fn field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key)
            .then(|| v.trim().to_string())
            .filter(|v| !v.is_empty() && v != "(none)")
    })
}

/// What the registry's uninstall entries say `package` is, for the whole
/// machine and for this user. A program is matched by its entry's key or by
/// its display name, case ignored, with spaces standing as `-` — a package
/// name has none.
#[cfg(windows)]
fn windows_uninstall_version(package: &str) -> Option<String> {
    use crate::elevated::wide;
    use windows_sys::Win32::System::Registry::*;
    const ROOTS: [(HKEY, &str); 3] = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];
    let want = package.to_ascii_lowercase();
    let read_value = |key: HKEY, name: &str| -> Option<String> {
        let mut buf = vec![0u16; 512];
        let mut size = (buf.len() * 2) as u32;
        let mut kind = 0u32;
        let n = wide(name);
        let ok = unsafe {
            RegQueryValueExW(
                key,
                n.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr() as *mut u8,
                &mut size,
            )
        } == 0
            && kind == REG_SZ;
        ok.then(|| {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..end])
        })
    };
    for (root, path) in ROOTS {
        let mut key = std::ptr::null_mut();
        if unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, KEY_READ, &mut key) } != 0 {
            continue;
        }
        let mut found = None;
        for i in 0.. {
            let mut name = vec![0u16; 256];
            let mut len = name.len() as u32;
            let r = unsafe {
                RegEnumKeyExW(
                    key,
                    i,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if r != 0 {
                break;
            }
            let sub_name = String::from_utf16_lossy(&name[..len as usize]);
            let mut sub = std::ptr::null_mut();
            if unsafe { RegOpenKeyExW(key, wide(&sub_name).as_ptr(), 0, KEY_READ, &mut sub) } != 0 {
                continue;
            }
            let display = read_value(sub, "DisplayName").unwrap_or_default();
            let matches = sub_name.to_ascii_lowercase() == want
                || display.to_ascii_lowercase() == want
                || display.to_ascii_lowercase().replace(' ', "-") == want;
            let version = if matches {
                read_value(sub, "DisplayVersion")
            } else {
                None
            };
            unsafe { RegCloseKey(sub) };
            if let Some(v) = version.filter(|v| version_ok(v)) {
                found = Some(v);
                break;
            }
        }
        unsafe { RegCloseKey(key) };
        if found.is_some() {
            return found;
        }
    }
    None
}

// ------------------------------------------------------------------ digests

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A file's digest, or a folder's: every file under it, in path order, each
/// named and hashed. Bounded, because an override may point at a large tree
/// and this runs after every update; past the bound it says nothing rather
/// than something partial.
pub fn digest_path(path: &Path) -> Option<String> {
    const MAX_FILES: usize = 5000;
    const MAX_BYTES: u64 = 256 * 1024 * 1024;
    if path.is_file() {
        return std::fs::read(path).ok().map(|b| sha256_hex(&b));
    }
    if !path.is_dir() {
        return None;
    }
    let mut files = vec![];
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let p = entry.path();
            let ft = entry.file_type().ok()?;
            if ft.is_symlink() {
                continue;
            } else if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                files.push(p);
                if files.len() > MAX_FILES {
                    return None;
                }
            }
        }
    }
    files.sort();
    let mut total = 0u64;
    let mut listing = String::new();
    for f in files {
        total += f.metadata().ok()?.len();
        if total > MAX_BYTES {
            return None;
        }
        let rel = f
            .strip_prefix(path)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        listing.push_str(&format!(
            "{rel}\t{}\n",
            sha256_hex(&std::fs::read(&f).ok()?)
        ));
    }
    Some(sha256_hex(listing.as_bytes()))
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
        let same: fn(&u8) -> bool = if numeric {
            u8::is_ascii_digit
        } else {
            u8::is_ascii_alphabetic
        };
        while i < a.len() && same(&a[i]) {
            i += 1;
        }
        while j < b.len() && same(&b[j]) {
            j += 1;
        }
        if j == bj {
            // Different kinds of run: a number is newer than letters.
            return if numeric {
                Ordering::Greater
            } else {
                Ordering::Less
            };
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
        ["-git", "-svn", "-hg", "-bzr", "-darcs", "-fossil"]
            .iter()
            .any(|s| p.ends_with(s))
    });
    let vcs_version = Regex::new(r"(?i)(\.r\d+\.g[0-9a-f]{7,}|[+~.]git|[+~]svn|[+~]hg)")
        .map(|re| re.is_match(version))
        .unwrap_or(false);
    vcs_package || vcs_version
}

/// A release tag as a version: `v1.2.3` and `release-1.2.3` are `1.2.3`.
/// Anything that is still not a version stays unordered.
pub fn tag_version(tag: &str) -> Option<String> {
    let t = tag.trim();
    let t = t
        .strip_prefix("release-")
        .or_else(|| t.strip_prefix("release/"))
        .unwrap_or(t);
    let t = t
        .strip_prefix(['v', 'V'])
        .filter(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(t);
    version_ok(t).then(|| t.to_string())
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
    root.canonicalize()
        .ok()
        .map(|r| r.join(file))
        .and_then(|p| p.canonicalize().ok())
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
    execute_with(id, params, root, state_dir, ctx, installed, &|| {
        crate::actions::execute(id, params, root)
    })
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
        backup: target
            .as_deref()
            .map(|t| backup_of(t).display().to_string()),
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
        kind: kind_action(),
        file_sha256: None,
        original: None,
        watch_issue: false,
        issue_state: None,
        forgotten_at: None,
        declared: None,
    };
    let rid = record.id.clone();
    records.push(record);
    save(state_dir, &records)?;

    let result = apply();
    if let Some(r) = records.iter_mut().find(|r| r.id == rid) {
        match &result {
            Ok(v) => {
                r.state = if ctx.undoes.is_some() {
                    "undo"
                } else {
                    "applied"
                }
                .into();
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
    if let Some(r) = records
        .iter_mut()
        .find(|r| r.id == id && r.state == "applied")
    {
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
    let Some(target) = resolve(root, file) else {
        return Ok(());
    };
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

/// Where the answers to "what is on this machine" and "what does upstream say"
/// come from. The real ones read the package manager and GitHub; the suite
/// passes its own.
pub struct Lookups<'a> {
    pub installed: &'a dyn Fn(&str) -> Option<Installed>,
    pub available: &'a dyn Fn(&str) -> Option<Installed>,
    /// Asked only for a record whose issue the person chose to watch, at most
    /// once a day, and never again once the answer is settled. `None` asks
    /// nobody and leaves what was last found as it is.
    pub issue: Option<&'a dyn Fn(&str) -> IssueState>,
}

impl Lookups<'static> {
    /// The machine's own answers, and no network at all.
    pub fn offline() -> Lookups<'static> {
        Lookups {
            installed: &installed_version,
            available: &available_version,
            issue: None,
        }
    }
}

/// How long an issue's state is trusted before it is asked again.
pub const ISSUE_RECHECK_SECS: u64 = 24 * 3600;

fn wants_issue_lookup(r: &Record, now_s: u64) -> bool {
    r.watch_issue
        && r.upstream
            .issue
            .as_deref()
            .is_some_and(|u| crate::upstream::parse(u).is_some())
        && match &r.issue_state {
            None => true,
            Some(st) => !st.settled() && now_s.saturating_sub(st.checked_at) >= ISSUE_RECHECK_SECS,
        }
}

/// The version a fix is in: what the publisher or the person said, or else the
/// release code found the merged change in.
fn effective_fixed_in(r: &Record) -> Option<String> {
    r.upstream.fixed_in.clone().or_else(|| {
        r.issue_state
            .as_ref()?
            .released_in
            .as_deref()
            .and_then(tag_version)
    })
}

/// What is true of one record now, before anything a person already saw is
/// taken away. Also the version the package manager reports, or empty.
fn flags_for(r: &Record, look: &Lookups) -> (Vec<Flag>, String) {
    let mut flags = vec![];
    // A declaration withdrawn, or whose package is gone, is that and nothing
    // else: its package being unknown or at another version is the same news
    // said twice, and the second time less clearly.
    if let Some(d) = &r.declared {
        let now = r
            .upstream
            .package
            .as_deref()
            .and_then(|p| (look.installed)(p))
            .map(|i| i.version)
            .unwrap_or_default();
        if d.gone {
            return (vec![Flag::DeclarerGone], now);
        }
        if let Some(reason) = &d.retired {
            return (
                vec![Flag::Retired {
                    reason: reason.clone(),
                }],
                now,
            );
        }
    }
    if let Some(t) = r.target.as_deref().map(Path::new) {
        if !t.exists() {
            flags.push(Flag::TargetGone);
        } else if let Some(want) = r.file_sha256.as_deref() {
            if digest_path(t).as_deref() != Some(want) {
                flags.push(Flag::FileChanged);
            }
        } else if still_holds(r, t) == Some(false) {
            flags.push(Flag::Overwritten);
        }
    }
    if r.reversible && r.backup.as_deref().is_some_and(|b| !Path::new(b).exists()) {
        flags.push(Flag::BackupGone);
    }
    if r.target.is_none()
        && crate::elevated::is_elevated(&r.action)
        && crate::elevate::still_holds(&r.action, &r.params) == Some(false)
    {
        flags.push(Flag::NoLongerSet);
    }

    let package = r.upstream.package.as_deref();
    let now = package.and_then(|p| (look.installed)(p));
    let then = r.installed.as_ref().map(|i| i.version.clone());
    match (&then, &now) {
        (_, None) if package.is_some() => flags.push(Flag::CannotCompare {
            why: m!("repair_not_installed", p = package.unwrap_or("")),
        }),
        (Some(from), Some(to)) if from != &to.version => {
            flags.push(Flag::Updated {
                from: from.clone(),
                to: to.version.clone(),
            });
        }
        _ => {}
    }

    // A package held back on purpose: the official one moving on is the whole
    // reason to look (`Frozen`). Asked of the package manager's own database,
    // which the update just refreshed. "Moving on" is a change from what was
    // offered when the fix was recorded, not only a higher number: a local
    // build is often versioned 9999 exactly so that nothing ever outranks it.
    if r.kind == "package" {
        if let (Some(p), Some(now)) = (package, &now) {
            if let Some(avail) = (look.available)(p) {
                let recorded = r.original.as_ref().and_then(|o| o.version.as_deref());
                let moved = recorded.is_some_and(|was| was != avail.version);
                let ahead =
                    vercmp(Some(p), &avail.version, &now.version) == Some(Ordering::Greater);
                if moved || ahead {
                    flags.push(Flag::Frozen {
                        installed: now.version.clone(),
                        available: avail.version,
                    });
                }
            }
        }
    }

    if let Some(orig) = r.original.as_ref().filter(|_| r.kind != "package") {
        let path_moved = orig
            .path
            .as_deref()
            .zip(orig.sha256.as_deref())
            .is_some_and(|(p, want)| digest_path(Path::new(p)).as_deref() != Some(want));
        let package_moved = orig
            .package
            .as_deref()
            .zip(orig.version.as_deref())
            .is_some_and(|(p, was)| (look.installed)(p).map(|i| i.version).as_deref() != Some(was));
        if path_moved || package_moved {
            flags.push(Flag::OriginalChanged);
        }
    }

    if let Some(st) = &r.issue_state {
        if let Some(tag) = &st.released_in {
            flags.push(Flag::ReleasedIn { tag: tag.clone() });
        } else if st.state == "merged" {
            flags.push(Flag::PrMerged);
        } else if st.state == "closed" {
            flags.push(Flag::IssueClosed);
        }
    }

    if let (Some(fixed), Some(now)) = (effective_fixed_in(r), &now) {
        match vercmp(package, &now.version, &fixed) {
            Some(Ordering::Less) => {}
            Some(_) => flags.push(Flag::UpstreamSaysFixed {
                fixed_in: fixed,
                installed: now.version.clone(),
            }),
            None => flags.push(Flag::CannotCompare {
                why: m!("repair_vcs", v = &now.version),
            }),
        }
    }
    (flags, now.map(|n| n.version).unwrap_or_default())
}

/// Every applied change that needs a person to look at it again, and why.
///
/// Reads the package manager and the files, and removes nothing. It writes one
/// thing: what a watched upstream issue said, so it is asked at most once a
/// day. What a person already saw at the version installed now is left out;
/// anything new, and everything after an update, is not.
pub fn review(state_dir: &Path, look: &Lookups) -> Vec<Review> {
    let mut records = load(state_dir);
    let now_s = now();
    let mut asked = false;
    for r in records.iter_mut().filter(|r| r.state == "applied") {
        let Some(ask) = look.issue else { break };
        if wants_issue_lookup(r, now_s) {
            let url = r.upstream.issue.clone().unwrap_or_default();
            let mut st = ask(&url);
            st.checked_at = now_s;
            r.issue_state = Some(st);
            asked = true;
        }
    }
    if asked {
        let _ = save(state_dir, &records);
    }
    let mut out = vec![];
    for r in records.into_iter().filter(|r| r.state == "applied") {
        let (mut flags, version) = flags_for(&r, look);
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
pub fn looked_at(state_dir: &Path, id: &str, look: &Lookups) -> Result<(), String> {
    let mut records = read(state_dir)?;
    let r = records
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| m!("repair_unknown"))?;
    let (flags, version) = flags_for(r, look);
    r.looked_at = Some(version);
    r.seen = flags.iter().map(|f| f.kind().to_string()).collect();
    save(state_dir, &records)
}

/// Watch the record's upstream issue, or stop. Watching is what makes the
/// review ask GitHub about it, so it is the person's switch and nobody else's.
pub fn set_watch(state_dir: &Path, id: &str, on: bool) -> Result<(), String> {
    let mut records = read(state_dir)?;
    let r = records
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| m!("repair_unknown"))?;
    if on
        && r.upstream
            .issue
            .as_deref()
            .is_none_or(|u| crate::upstream::parse(u).is_none())
    {
        return Err(m!("repair_no_issue_to_watch"));
    }
    r.watch_issue = on;
    if !on {
        r.issue_state = None;
    }
    save(state_dir, &records)
}

// ------------------------------------------------------------------ changes made elsewhere

/// A change something other than this client made, as the tool that made it
/// describes it. Names and paths only; every value a decision reads is taken
/// from the machine when the record is written.
#[derive(Debug, Clone, Default)]
pub struct External {
    /// `file`, `package` or `overlay`.
    pub kind: String,
    /// Who made it: an agent, a skill, a person.
    pub by: String,
    /// The changed file, or the local copy that overrides the original.
    pub path: Option<PathBuf>,
    pub upstream: Upstream,
    /// For an overlay: what it stands in front of.
    pub original_path: Option<PathBuf>,
    pub original_package: Option<String>,
    pub watch_issue: bool,
    pub note: Option<String>,
}

fn by_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && !s.chars().any(|c| c.is_control())
}

/// Check what the tool said and turn it into a record, reading the machine.
pub(crate) fn external_record(ext: External, id: String, look: &Lookups) -> Result<Record, String> {
    if !by_ok(&ext.by) {
        return Err(m!("repair_external_bad", k = "by"));
    }
    if ext.note.as_deref().is_some_and(|n| n.len() > 2000) {
        return Err(m!("repair_external_bad", k = "note"));
    }
    let abs = |p: &Path| -> Result<PathBuf, String> {
        p.canonicalize()
            .map_err(|_| m!("repair_external_path", p = crate::reads::display_path(p)))
    };
    let mut rec = Record {
        id,
        at: now(),
        action: format!("external:{}", ext.kind),
        params: BTreeMap::new(),
        target: None,
        backup: None,
        reversible: false,
        subject: ext.by.clone(),
        upstream: ext.upstream,
        installed: None,
        state: "applied".into(),
        error: None,
        looked_at: None,
        seen: vec![],
        undo: None,
        note: ext.note,
        kind: ext.kind.clone(),
        file_sha256: None,
        original: None,
        watch_issue: false,
        issue_state: None,
        forgotten_at: None,
        declared: None,
    };
    if ext.watch_issue {
        if rec
            .upstream
            .issue
            .as_deref()
            .is_none_or(|u| crate::upstream::parse(u).is_none())
        {
            return Err(m!("repair_no_issue_to_watch"));
        }
        rec.watch_issue = true;
    }
    match ext.kind.as_str() {
        "file" => {
            let p = abs(ext
                .path
                .as_deref()
                .ok_or_else(|| m!("repair_external_bad", k = "path"))?)?;
            if !p.is_file() {
                return Err(m!(
                    "repair_external_path",
                    p = crate::reads::display_path(&p)
                ));
            }
            rec.file_sha256 = digest_path(&p);
            rec.target = Some(p.display().to_string());
        }
        "package" => {
            let Some(pkg) = rec.upstream.package.clone() else {
                return Err(m!("repair_external_bad", k = "package"));
            };
            // What the package manager offered instead, when the local build
            // was recorded — the point from which "the official one moved on"
            // is measured.
            rec.original = Some(Original {
                version: (look.available)(&pkg).map(|i| i.version),
                package: Some(pkg),
                ..Original::default()
            });
        }
        "overlay" => {
            let p = abs(ext
                .path
                .as_deref()
                .ok_or_else(|| m!("repair_external_bad", k = "path"))?)?;
            rec.file_sha256 = digest_path(&p);
            rec.target = Some(p.display().to_string());
            let mut orig = Original::default();
            if let Some(op) = ext.original_path.as_deref() {
                let op = abs(op)?;
                orig.sha256 = digest_path(&op);
                orig.path = Some(op.display().to_string());
            }
            if let Some(pkg) = ext.original_package {
                if !crate::provenance::name_ok(&pkg) {
                    return Err(m!("repair_external_bad", k = "original-package"));
                }
                orig.version = (look.installed)(&pkg).map(|i| i.version);
                orig.package = Some(pkg);
            }
            if orig.path.is_none() && orig.package.is_none() {
                return Err(m!("repair_external_bad", k = "original"));
            }
            rec.original = Some(orig);
        }
        _ => return Err(m!("repair_external_bad", k = "kind")),
    }
    rec.installed = rec
        .upstream
        .package
        .as_deref()
        .and_then(|p| (look.installed)(p));
    Ok(rec)
}

pub(crate) fn next_id(records: &[Record]) -> String {
    format!("{}-{}", now(), records.len() + 1)
}

/// Record a change that has already been made. Nothing to undo it with is kept,
/// so the record says it cannot be undone from here.
pub fn add_external(state_dir: &Path, ext: External, look: &Lookups) -> Result<Record, String> {
    let mut records = read(state_dir)?;
    let rec = external_record(ext, next_id(&records), look)?;
    records.push(rec.clone());
    save(state_dir, &records)?;
    Ok(rec)
}

/// Before a file is changed: a copy of it is kept under this client's state,
/// and the record waits for `finish_external` to say the change is made.
pub fn begin_external(state_dir: &Path, ext: External, look: &Lookups) -> Result<Record, String> {
    if ext.kind != "file" {
        return Err(m!("repair_external_bad", k = "kind"));
    }
    let mut records = read(state_dir)?;
    let mut rec = external_record(ext, next_id(&records), look)?;
    let target = PathBuf::from(rec.target.clone().unwrap_or_default());
    let dir = state_dir.join("backups").join(&rec.id);
    std::fs::create_dir_all(&dir).map_err(|e| m!("repair_not_written", e = e))?;
    let copy = dir.join(target.file_name().unwrap_or_default());
    std::fs::copy(&target, &copy).map_err(|e| m!("repair_not_written", e = e))?;
    rec.backup = Some(copy.display().to_string());
    rec.reversible = true;
    rec.state = "pending".into();
    // The digest is taken when the change is finished, not now.
    rec.file_sha256 = None;
    records.push(rec.clone());
    save(state_dir, &records)?;
    Ok(rec)
}

/// A change already made, with a copy of the file from before it that was
/// kept somewhere else — by an agent hook that could not know, before a shell
/// command ran, which of the files it names the command would change. The
/// record is finished at once: `before` becomes its copy, the digest is the
/// file's now.
pub fn record_external_with_copy(
    state_dir: &Path,
    ext: External,
    before: &Path,
    look: &Lookups,
) -> Result<Record, String> {
    if ext.kind != "file" {
        return Err(m!("repair_external_bad", k = "kind"));
    }
    let mut records = read(state_dir)?;
    let mut rec = external_record(ext, next_id(&records), look)?;
    let target = PathBuf::from(rec.target.clone().unwrap_or_default());
    let dir = state_dir.join("backups").join(&rec.id);
    std::fs::create_dir_all(&dir).map_err(|e| m!("repair_not_written", e = e))?;
    let copy = dir.join(target.file_name().unwrap_or_default());
    std::fs::copy(before, &copy).map_err(|e| m!("repair_not_written", e = e))?;
    rec.backup = Some(copy.display().to_string());
    rec.reversible = true;
    rec.file_sha256 = digest_path(&target);
    records.push(rec.clone());
    save(state_dir, &records)?;
    Ok(rec)
}

/// The change announced by `begin_external` is made: its digest is taken now.
pub fn finish_external(state_dir: &Path, id: &str) -> Result<Record, String> {
    let mut records = read(state_dir)?;
    let r = records
        .iter_mut()
        .find(|r| r.id == id && r.state == "pending")
        .ok_or_else(|| m!("repair_unknown"))?;
    let target = PathBuf::from(r.target.clone().unwrap_or_default());
    r.file_sha256 = digest_path(&target);
    r.state = "applied".into();
    let out = r.clone();
    save(state_dir, &records)?;
    Ok(out)
}

/// A finished record's file was changed again by whoever made the change —
/// an agent editing the same file a second time in one session. The digest
/// is taken again; the copy stays the one from before the first change,
/// because going back means going back to before all of it.
pub fn refresh_digest(state_dir: &Path, id: &str) -> Result<Record, String> {
    let mut records = read(state_dir)?;
    let r = records
        .iter_mut()
        .find(|r| r.id == id && r.state == "applied")
        .ok_or_else(|| m!("repair_unknown"))?;
    let target = PathBuf::from(r.target.clone().unwrap_or_default());
    r.file_sha256 = digest_path(&target);
    let out = r.clone();
    save(state_dir, &records)?;
    Ok(out)
}

/// Remove what a record said, and keep that it existed.
///
/// **The ledger is a record of what other tools did to this machine**, and the
/// tools it records run as the person. If any of them could erase an entry,
/// the entry would be worth nothing the moment it mattered — so removal is not
/// an ordinary command; see `repairs_cli`, which will not do it without
/// administrator rights and a person answering.
///
/// What it says goes: the path, the digest, who made it, the note, the
/// upstream issue, and the copy kept to go back to. What stays is a stub — the
/// id, when the change was recorded, its kind, and when it was forgotten. A
/// ledger with a hole in it and a ledger that never had an entry look the same
/// from outside, and they are not the same thing.
pub fn forget(state_dir: &Path, id: &str) -> Result<Record, String> {
    let mut records = read(state_dir)?;
    let r = records
        .iter_mut()
        .find(|r| r.id == id && r.state != "forgotten")
        .ok_or_else(|| m!("repair_unknown"))?;
    let was = r.clone();
    // The copy goes with the record. Leaving it would keep on disk exactly the
    // file contents the record was removed to be rid of, under a name derived
    // from the id that is still in the stub.
    if let Some(b) = r.backup.as_deref() {
        let _ = std::fs::remove_file(b);
        if let Some(dir) = Path::new(b).parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    *r = Record {
        id: was.id.clone(),
        at: was.at,
        action: String::new(),
        params: BTreeMap::new(),
        target: None,
        backup: None,
        reversible: false,
        subject: String::new(),
        upstream: Upstream::default(),
        installed: None,
        state: "forgotten".into(),
        error: None,
        looked_at: None,
        seen: vec![],
        undo: None,
        note: None,
        kind: was.kind.clone(),
        file_sha256: None,
        original: None,
        watch_issue: false,
        issue_state: None,
        forgotten_at: Some(now()),
        declared: None,
    };
    save(state_dir, &records)?;
    Ok(was)
}

/// Put back the copy `begin_external` kept, on the person's word. The record
/// stays, as `undone`.
pub fn restore_external(state_dir: &Path, id: &str) -> Result<Record, String> {
    let mut records = read(state_dir)?;
    let r = records
        .iter_mut()
        .find(|r| r.id == id && r.kind == "file" && (r.state == "applied" || r.state == "pending"))
        .ok_or_else(|| m!("repair_unknown"))?;
    let (Some(target), Some(backup)) = (r.target.clone(), r.backup.clone()) else {
        return Err(m!("no_backup"));
    };
    std::fs::copy(&backup, &target).map_err(|_| m!("no_backup"))?;
    r.state = "undone".into();
    let out = r.clone();
    save(state_dir, &records)?;
    Ok(out)
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
        move |_| {
            Some(Installed {
                version: v.into(),
                source: "pacman".into(),
            })
        }
    }

    fn nothing(_: &str) -> Option<Installed> {
        None
    }

    /// Only what is installed, no package database and no network.
    fn lk<'a>(installed: &'a dyn Fn(&str) -> Option<Installed>) -> Lookups<'a> {
        Lookups {
            installed,
            available: &nothing,
            issue: None,
        }
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
            Some(Installed {
                version: "1:2.4.0-3".into(),
                source: "pacman".into(),
            })
        };
        let params = json!({"file": "app.toml", "key": "modeset", "value": "1"});
        let on_disk = RefCell::new(vec![]);
        let out = execute_with(
            "set_config_key",
            &params,
            &root,
            &root,
            ctx("app", Some("2.5.0")),
            &reader,
            &|| {
                *on_disk.borrow_mut() = load(&root);
                crate::actions::execute("set_config_key", &params, &root)
            },
        )
        .unwrap();
        assert_eq!(
            seen_before.borrow().as_deref(),
            Some("modeset = 0\n"),
            "the version was read after the change, not before it"
        );
        let pending = on_disk.borrow();
        assert!(
            pending.len() == 1 && pending[0].state == "pending",
            "the change started without its record on disk: {pending:?}"
        );

        let records = load(&root);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(out["repair_record"], r.id.as_str());
        assert_eq!(r.state, "applied");
        assert_eq!(r.action, "set_config_key");
        assert_eq!(r.installed.as_ref().unwrap().version, "1:2.4.0-3");
        let target = file.canonicalize().unwrap();
        assert_eq!(
            r.target.as_deref(),
            Some(target.display().to_string().as_str()),
            "the path is not the resolved one"
        );
        assert!(
            r.backup.as_deref().is_some_and(|b| Path::new(b).exists()),
            "{r:?}"
        );
        assert!(r.reversible);
        assert_eq!(
            r.upstream.issue.as_deref(),
            Some("https://github.com/example/app/issues/12")
        );
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
        let err = execute(
            "set_config_key",
            &json!({"file": "app.toml", "key": "modeset", "value": "1"}),
            &root,
            &blocked,
            ctx("app", None),
            &at("1.0-1"),
        )
        .unwrap_err();
        assert!(crate::msg::is("repair_not_written", &err), "{err}");
        assert_eq!(
            std::fs::read_to_string(root.join("app.toml")).unwrap(),
            "modeset = 0\n",
            "the change was made without a record"
        );
        assert!(
            !root.join("app.toml.bak").exists(),
            "the action ran as far as its backup"
        );

        // A list that cannot be read is not an empty one: nothing is changed,
        // and the file is left as it was for a person to look at.
        let state = root.join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join(FILE), "{ not json").unwrap();
        let err = execute(
            "set_config_key",
            &json!({"file": "app.toml", "key": "modeset", "value": "1"}),
            &root,
            &state,
            ctx("app", None),
            &at("1.0-1"),
        )
        .unwrap_err();
        assert!(crate::msg::is("repair_not_written", &err), "{err}");
        assert_eq!(
            std::fs::read_to_string(state.join(FILE)).unwrap(),
            "{ not json",
            "an unreadable list was written over"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("app.toml")).unwrap(),
            "modeset = 0\n"
        );
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
        execute(
            "set_config_key",
            &json!({"file": "app.toml", "key": "modeset", "value": "1"}),
            &root,
            &root,
            ctx("app", Some("2.5.0")),
            &at("2.4.0-3"),
        )
        .unwrap();
        let changed = std::fs::read_to_string(&file).unwrap();
        let backup = std::fs::read_to_string(root.join("app.toml.bak")).unwrap();

        assert!(
            review(&root, &lk(&at("2.4.0-3"))).is_empty(),
            "nothing changed, yet a flag"
        );

        let flags = |v: &str| {
            review(&root, &lk(&at(v)))
                .pop()
                .map(|r| r.flags)
                .unwrap_or_default()
        };
        assert_eq!(
            flags("2.4.0-4"),
            vec![Flag::Updated {
                from: "2.4.0-3".into(),
                to: "2.4.0-4".into()
            }],
            "a new pkgrel is an update and nothing more"
        );
        let fixed = flags("2.5.0-1");
        assert!(
            fixed.contains(&Flag::UpstreamSaysFixed {
                fixed_in: "2.5.0".into(),
                installed: "2.5.0-1".into()
            }),
            "{fixed:?}"
        );
        assert!(
            matches!(
                flags("2.6.r12.g1a2b3c4-1")[..],
                [Flag::Updated { .. }, Flag::CannotCompare { .. }]
            ),
            "a build from head was compared as if it were a release"
        );

        // Nothing was touched by any of that.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), changed);
        assert_eq!(
            std::fs::read_to_string(root.join("app.toml.bak")).unwrap(),
            backup
        );

        // Looked at, at 2.5.0-1: quiet at that version, raised again at the next.
        let id = load(&root)[0].id.clone();
        looked_at(&root, &id, &lk(&at("2.5.0-1"))).unwrap();
        assert!(flags("2.5.0-1").is_empty(), "{:?}", flags("2.5.0-1"));
        assert!(
            !flags("2.5.1-1").is_empty(),
            "an update after the look raised nothing"
        );

        // An update that rewrote the file: new, so shown although the
        // version is the one the person looked at.
        std::fs::write(&file, "modeset = 0\n").unwrap();
        assert_eq!(flags("2.5.0-1"), vec![Flag::Overwritten]);

        // Kept again, and that is not asked about at this version any more —
        // not at every start for as long as the file stays as it is.
        looked_at(&root, &id, &lk(&at("2.5.0-1"))).unwrap();
        assert!(flags("2.5.0-1").is_empty(), "{:?}", flags("2.5.0-1"));
        std::fs::write(&file, "modeset = 1\n").unwrap();

        // And an undo takes the record off the list, without deleting it.
        execute(
            "restore_backup",
            &json!({"file": "app.toml"}),
            &root,
            &root,
            ctx("app", None),
            &at("2.5.0-1"),
        )
        .unwrap();
        assert!(review(&root, &lk(&at("9.9-1"))).is_empty());
        assert_eq!(load(&root)[0].state, "undone");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn ext(kind: &str, path: Option<&Path>) -> External {
        External {
            kind: kind.into(),
            by: "an agent".into(),
            path: path.map(Path::to_path_buf),
            ..External::default()
        }
    }

    /// RR6: a file another tool changes is recorded around the change — a copy
    /// before, a digest after — and a later rewrite is noticed whatever the
    /// file's format. The copy goes back only when a person asks.
    #[test]
    fn a_file_changed_elsewhere_is_recorded_watched_and_can_go_back() {
        let root = dir("rr6");
        let state = root.join("state");
        let file = root.join("hyprland.conf");
        std::fs::write(&file, "monitor=,preferred,auto,1\n").unwrap();
        let look = lk(&nothing);

        let rec = begin_external(&state, ext("file", Some(&file)), &look).unwrap();
        assert_eq!(rec.state, "pending");
        assert!(rec.reversible);
        assert_eq!(
            std::fs::read_to_string(rec.backup.as_deref().unwrap()).unwrap(),
            "monitor=,preferred,auto,1\n",
            "the copy is not what was there before"
        );
        assert!(
            review(&state, &look).is_empty(),
            "a change not yet made was reviewed"
        );

        std::fs::write(&file, "monitor=,preferred,auto,1.25\n").unwrap();
        let done = finish_external(&state, &rec.id).unwrap();
        assert_eq!(done.state, "applied");
        assert_eq!(
            done.file_sha256.as_deref(),
            Some(sha256_hex(b"monitor=,preferred,auto,1.25\n").as_str())
        );
        assert!(
            review(&state, &look).is_empty(),
            "the change as made was flagged"
        );

        // An update writes the file again.
        std::fs::write(&file, "monitor=,preferred,auto,auto\n").unwrap();
        let found = review(&state, &look);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].flags, vec![Flag::FileChanged]);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "monitor=,preferred,auto,auto\n",
            "looking again touched the file"
        );

        restore_external(&state, &rec.id).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "monitor=,preferred,auto,1\n"
        );
        assert_eq!(load(&state)[0].state, "undone");
        assert!(review(&state, &look).is_empty());

        // After the fact: nothing to go back to, and it says so.
        let late = add_external(&state, ext("file", Some(&file)), &look).unwrap();
        assert!(!late.reversible && late.backup.is_none());
        assert!(restore_external(&state, &late.id).is_err());

        for (bad, why) in [
            (
                External {
                    by: String::new(),
                    ..ext("file", Some(&file))
                },
                "by",
            ),
            (ext("script", Some(&file)), "kind"),
            (ext("file", None), "path"),
            (
                ext("file", Some(&root.join("missing.conf"))),
                "missing path",
            ),
            (ext("package", None), "package"),
            (ext("overlay", Some(&file)), "original"),
            (
                External {
                    note: Some("x".repeat(3000)),
                    ..ext("file", Some(&file))
                },
                "note",
            ),
        ] {
            assert!(
                add_external(&state, bad, &look).is_err(),
                "accepted without {why}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RR7: a package built or pinned locally is flagged when the official one
    /// moves on — also when the local build is versioned 9999 so that nothing
    /// ever outranks it, which is the common way of holding a package back.
    #[test]
    fn a_package_held_back_is_flagged_when_the_official_one_moves_on() {
        let root = dir("rr7");
        let offered = std::cell::RefCell::new("0.45.0-1".to_string());
        let available = |_: &str| {
            Some(Installed {
                version: offered.borrow().clone(),
                source: "pacman".into(),
            })
        };
        let installed = at("9999-1");
        let look = Lookups {
            installed: &installed,
            available: &available,
            issue: None,
        };

        let mut e = ext("package", None);
        e.upstream.package = Some("hyprland".into());
        let rec = add_external(&root, e, &look).unwrap();
        assert_eq!(rec.installed.as_ref().unwrap().version, "9999-1");
        assert_eq!(
            rec.original.as_ref().unwrap().version.as_deref(),
            Some("0.45.0-1")
        );
        assert!(review(&root, &look).is_empty(), "nothing moved, yet a flag");

        *offered.borrow_mut() = "0.45.2-1".into();
        let found = review(&root, &look);
        assert_eq!(
            found[0].flags,
            vec![Flag::Frozen {
                installed: "9999-1".into(),
                available: "0.45.2-1".into()
            }]
        );

        // Without a recorded offer, a newer official version is enough.
        let installed = at("0.44.0-1");
        let look = Lookups {
            installed: &installed,
            available: &available,
            issue: None,
        };
        let mut e = ext("package", None);
        e.upstream.package = Some("waybar".into());
        let mut rec = external_record(e, "x".into(), &look).unwrap();
        rec.original = None;
        let (flags, _) = flags_for(&rec, &look);
        assert!(
            flags.contains(&Flag::Frozen {
                installed: "0.44.0-1".into(),
                available: "0.45.2-1".into()
            }),
            "{flags:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RR8: a copy that overrides a component is flagged when the component
    /// changes underneath it — a folder's files, or its package's version.
    #[test]
    fn an_override_is_flagged_when_what_it_overrides_changes() {
        let root = dir("rr8");
        let official = root.join("official/plugin");
        let copy = root.join("mine/plugin");
        for d in [&official, &copy] {
            std::fs::create_dir_all(d.join("lib")).unwrap();
            std::fs::write(d.join("init.lua"), "return 1\n").unwrap();
            std::fs::write(d.join("lib/util.lua"), "return 2\n").unwrap();
        }
        let version = std::cell::RefCell::new("1.0-1".to_string());
        let installed = |_: &str| {
            Some(Installed {
                version: version.borrow().clone(),
                source: "pacman".into(),
            })
        };
        let look = lk(&installed);
        let mut e = ext("overlay", Some(&copy));
        e.original_path = Some(official.clone());
        e.original_package = Some("some-plugin".into());
        let rec = add_external(&root.join("state"), e, &look).unwrap();
        let orig = rec.original.clone().unwrap();
        assert!(
            orig.sha256.is_some() && orig.version.as_deref() == Some("1.0-1"),
            "{orig:?}"
        );
        let state = root.join("state");
        assert!(review(&state, &look).is_empty());

        std::fs::write(official.join("lib/util.lua"), "return 3\n").unwrap();
        assert_eq!(review(&state, &look)[0].flags, vec![Flag::OriginalChanged]);
        std::fs::write(official.join("lib/util.lua"), "return 2\n").unwrap();
        assert!(
            review(&state, &look).is_empty(),
            "the same files, and still flagged"
        );

        *version.borrow_mut() = "1.1-1".into();
        assert_eq!(review(&state, &look)[0].flags, vec![Flag::OriginalChanged]);
        // And the copy itself, edited again.
        std::fs::write(copy.join("init.lua"), "return 9\n").unwrap();
        assert!(review(&state, &look)[0].flags.contains(&Flag::FileChanged));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RR9: an upstream issue is asked about only when the person chose to
    /// watch it, at most once a day, never again once settled, and never
    /// offline; a release found by code becomes the version compared.
    #[test]
    fn a_watched_issue_is_asked_seldom_and_its_release_is_compared() {
        let root = dir("rr9");
        let file = root.join("app.conf");
        std::fs::write(&file, "x = 1\n").unwrap();
        let calls = std::cell::Cell::new(0);
        let answer = std::cell::RefCell::new(IssueState {
            state: "merged".into(),
            is_pull: true,
            ..IssueState::default()
        });
        let ask = |_: &str| {
            calls.set(calls.get() + 1);
            answer.borrow().clone()
        };
        let installed = at("2.1.0-1");
        let look = Lookups {
            installed: &installed,
            available: &nothing,
            issue: Some(&ask),
        };

        let mut e = ext("file", Some(&file));
        e.upstream.package = Some("app".into());
        e.upstream.issue = Some("https://gitlab.com/o/r/-/issues/5".into());
        assert!(
            add_external(
                &root,
                External {
                    watch_issue: true,
                    ..e.clone()
                },
                &look
            )
            .is_err(),
            "a link nobody can look up was watched"
        );
        e.upstream.issue = Some("https://github.com/o/r/pull/5".into());
        let rec = add_external(&root, e, &look).unwrap();

        review(&root, &look);
        assert_eq!(
            calls.get(),
            0,
            "an issue nobody chose to watch was asked about"
        );

        set_watch(&root, &rec.id, true).unwrap();
        let found = review(&root, &look);
        assert_eq!(calls.get(), 1);
        assert_eq!(found[0].flags, vec![Flag::PrMerged]);
        review(&root, &look);
        assert_eq!(calls.get(), 1, "asked twice within a day");

        // A day later the release is out, and it is what the version is compared with.
        let mut records = load(&root);
        records[0].issue_state.as_mut().unwrap().checked_at -= ISSUE_RECHECK_SECS;
        save(&root, &records).unwrap();
        answer.borrow_mut().released_in = Some("v2.1.0".into());
        let found = review(&root, &look);
        assert_eq!(calls.get(), 2);
        assert!(
            found[0].flags.contains(&Flag::ReleasedIn {
                tag: "v2.1.0".into()
            }),
            "{:?}",
            found[0].flags
        );
        assert!(
            found[0].flags.contains(&Flag::UpstreamSaysFixed {
                fixed_in: "2.1.0".into(),
                installed: "2.1.0-1".into()
            }),
            "{:?}",
            found[0].flags
        );

        // Settled: never asked again. Offline: never asked, nothing written.
        let mut records = load(&root);
        records[0].issue_state.as_mut().unwrap().checked_at = 0;
        save(&root, &records).unwrap();
        review(&root, &look);
        assert_eq!(calls.get(), 2, "a settled answer was asked again");
        let before = std::fs::read_to_string(root.join(FILE)).unwrap();
        review(&root, &lk(&installed));
        assert_eq!(std::fs::read_to_string(root.join(FILE)).unwrap(), before);

        set_watch(&root, &rec.id, false).unwrap();
        assert!(
            load(&root)[0].issue_state.is_none(),
            "stopping kept what GitHub said"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A list written before these fields existed is still read.
    #[test]
    fn an_older_list_is_read() {
        let root = dir("rr-old");
        std::fs::write(
            root.join(FILE),
            r#"{"records":[{"id":"1-1","at":1,"action":"set_config_key",
            "params":{"file":"a.toml","key":"k","value":"v"},"reversible":true,"subject":"x",
            "state":"applied"}]}"#,
        )
        .unwrap();
        let recs = load(&root);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].kind, "action");
        assert!(!recs[0].watch_issue);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_tag_is_read_as_the_version_it_names() {
        assert_eq!(tag_version("v2.1.0").as_deref(), Some("2.1.0"));
        assert_eq!(tag_version("release-0.45.0").as_deref(), Some("0.45.0"));
        assert_eq!(
            tag_version("vendor").as_deref(),
            Some("vendor"),
            "a word starting with v lost its v"
        );
        assert_eq!(tag_version("v 1"), None);
    }

    /// The Windows side of "which version is installed": the uninstall entries.
    #[cfg(windows)]
    #[test]
    fn a_windows_program_s_version_is_read_from_its_uninstall_entry() {
        // Something every Windows machine that runs this suite has.
        let found = [
            "Git",
            "PODSHL",
            "Microsoft-Edge",
            "Microsoft-Edge-WebView2-Runtime",
        ]
        .iter()
        .find_map(|p| installed_version(p));
        assert!(
            found
                .as_ref()
                .is_some_and(|i| i.source == "windows" && version_ok(&i.version)),
            "no uninstall entry answered: {found:?}"
        );
        assert_eq!(installed_version("PodshlNoSuchProgram"), None);
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
        assert_eq!(
            vercmp(Some("hyprland-git"), "0.45.0-1", "0.45.0"),
            None,
            "a -git package's version was trusted as a release"
        );
        assert_eq!(vercmp(None, "1.0; rm -rf", "1.0"), None);
    }

    /// RR5: what a publisher says about the software is checked as text before
    /// anything uses it.
    #[test]
    fn a_publisher_s_upstream_is_checked_as_text() {
        let ok = Upstream::from_value(Some(&json!({
            "package": "hyprland", "issue": "https://github.com/hyprwm/Hyprland/issues/1",
            "fixed_in": "0.45.0"})))
        .unwrap();
        assert_eq!(ok.package.as_deref(), Some("hyprland"));
        for bad in [
            json!({"package": "a b"}),
            json!({"issue": "http://x"}),
            json!({"fixed_in": "1.0; x"}),
            json!({"version": "1"}),
            json!({"package": 3}),
            json!("hyprland"),
        ] {
            assert!(Upstream::from_value(Some(&bad)).is_err(), "accepted {bad}");
        }
        assert_eq!(Upstream::from_value(None).unwrap(), Upstream::default());
    }
}
