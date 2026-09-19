//! What a package declares about itself, taken into the record.
//!
//! **The maintainer knows why, and the person does not.** A package that
//! deliberately leaves the ordinary path — installed from a PKGBUILD because
//! it cannot be in the AUR, a patched copy standing in for a broken upstream
//! release, a file outside itself it has to change — carries a reason nobody
//! on the machine will ever write down. So the package says it, once, in a
//! small file it installs, and the record takes it from there:
//!
//! ```text
//! /usr/share/podshl/repairs.d/<name>.json                   installed by a package
//! ~/.local/share/podshl/repairs.d/<name>.json               installed into the home directory
//! ~/.config/omarchy/plugins/<plugin>/repairs.d/<name>.json  shipped by an Omarchy plugin
//! ```
//!
//! **A plugin is a package by other means**: it arrives, is updated and is
//! removed without the package manager, and it can change the machine as much
//! as a package can. So it declares the same way, from inside its own
//! directory, and is attributed by where Omarchy installed it — the directory
//! is the plugin's name — not by anything the file says.
//!
//! ```json
//! { "records": [ {
//!     "id": "outside-the-aur",
//!     "kind": "package",
//!     "package": "podshl-bin",
//!     "reason": "Installed from a PKGBUILD, not the AUR: …",
//!     "until": { "issue": "https://github.com/…/issues/1" }
//! } ] }
//! ```
//!
//! `kind` is `file`, `package` or `overlay`, with the same fields as
//! `podshl-repairs add` (`path`, `package`, `original`, `original_package`);
//! `until` takes `issue` and `fixed_in`. An entry the maintainer withdraws in a
//! later version says so: `{ "id": "outside-the-aur", "retired": "Now in the
//! AUR; the local repository can go." }`.
//!
//! **What happens to a declaration**, every time the record is reviewed:
//!
//! * new: it becomes a record, attributed to the package that owns the
//!   declaration file — asked of the package manager, never taken from the
//!   file, which anybody could write any name into;
//! * changed by an update: the record follows it, and keeps its identity;
//! * withdrawn by the maintainer, or no longer declared by a package that is
//!   still installed: the record says so, once, with the maintainer's reason;
//! * its package removed: the record says that.
//!
//! **Nothing is ever removed, and nothing is done.** A declaration is
//! information: it cannot undo a change, cannot change a file, and cannot
//! switch on a GitHub lookup — `until.issue` is asked only once the person
//! chooses to watch it. The worst a malicious declaration can do is add a line
//! to a list.

use crate::repair::{self, Declared, External, Lookups, Upstream};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Where Omarchy installs plugins.
fn plugins_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config/omarchy/plugins"))
}

/// The directories declarations are read from on this machine: the two
/// fixed ones, and one inside every Omarchy plugin installed now — read at
/// the time, so a plugin added or removed since the last review counts.
pub fn live_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/podshl/repairs.d")];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/podshl/repairs.d"));
    }
    if let Some(root) = plugins_root() {
        dirs.extend(plugin_dirs(&root));
    }
    dirs
}

/// `<root>/<plugin>/repairs.d` for every plugin under `root`.
fn plugin_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path().join("repairs.d"))
        .filter(|d| d.is_dir())
        .collect();
    out.sort();
    out
}

/// The plugin a declaration file belongs to: the directory under `root` it
/// sits in, which is where Omarchy put the plugin and so its name.
fn plugin_of(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let name = rel.components().next()?.as_os_str().to_str()?;
    crate::provenance::name_ok(name).then(|| format!("omarchy plugin {name}"))
}

/// Who a declaration file belongs to: the Omarchy plugin it sits in, or else
/// the package that owns it, as the package manager says.
pub fn live_owner(path: &Path) -> Option<String> {
    if let Some(p) = plugins_root().and_then(|root| {
        let root = root.canonicalize().unwrap_or(root);
        plugin_of(path, &root)
    }) {
        return Some(p);
    }
    let p = path.to_str()?;
    if let Some(pacman) = crate::reads::find_on_path("pacman") {
        // "/usr/share/… is owned by podshl-bin 0.1.7-1"
        let (ok, out) = crate::reads::run_bounded(&pacman, &["-Qqo", p])?;
        let name = out.lines().next().unwrap_or("").trim();
        return (ok && crate::provenance::name_ok(name)).then(|| name.to_string());
    }
    if let Some(dpkg) = crate::reads::find_on_path("dpkg-query") {
        // "podshl-bin: /usr/share/…"
        let (ok, out) = crate::reads::run_bounded(&dpkg, &["-S", p])?;
        let name = out.split(':').next().unwrap_or("").trim();
        return (ok && crate::provenance::name_ok(name)).then(|| name.to_string());
    }
    None
}

/// Where declarations are, and who owns a file. The live ones read the
/// directories above and the package manager; the suite passes its own.
pub struct Sources<'a> {
    pub dirs: Vec<PathBuf>,
    pub owner: &'a dyn Fn(&Path) -> Option<String>,
}

impl Sources<'static> {
    pub fn live() -> Sources<'static> {
        Sources {
            dirs: live_dirs(),
            owner: &live_owner,
        }
    }
}

/// One entry of a declaration, checked.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    retired: Option<String>,
    ext: Option<External>,
}

fn id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn text_ok(s: &str, max: usize) -> bool {
    s.len() <= max && !s.chars().any(|c| c.is_control() && c != '\n')
}

/// One entry, or why it cannot be taken.
fn entry(v: &Value, by: &str) -> Result<Entry, String> {
    let str_of = |k: &str| v.get(k).and_then(|x| x.as_str());
    let id = str_of("id")
        .filter(|s| id_ok(s))
        .ok_or("an entry has no usable id")?;
    if let Some(r) = v.get("retired") {
        let r = r
            .as_str()
            .filter(|s| text_ok(s, 500))
            .ok_or(format!("{id}: retired is not a short text"))?;
        return Ok(Entry {
            id: id.into(),
            retired: Some(r.into()),
            ext: None,
        });
    }
    let kind = str_of("kind").ok_or(format!("{id}: no kind"))?;
    if !["file", "package", "overlay"].contains(&kind) {
        return Err(format!(
            "{id}: kind {kind:?} is not file, package or overlay"
        ));
    }
    let reason = str_of("reason")
        .filter(|s| !s.trim().is_empty() && text_ok(s, 2000))
        .ok_or(format!(
            "{id}: a declaration says why, and this one does not"
        ))?;
    let mut up = serde_json::Map::new();
    if let Some(p) = str_of("package") {
        up.insert("package".into(), p.into());
    }
    if let Some(until) = v.get("until").and_then(|u| u.as_object()) {
        for (k, val) in until {
            up.insert(k.clone(), val.clone());
        }
    }
    let upstream =
        Upstream::from_value(Some(&Value::Object(up))).map_err(|e| format!("{id}: {e}"))?;
    let ext = External {
        kind: kind.into(),
        by: by.into(),
        path: str_of("path").map(PathBuf::from),
        upstream,
        original_path: str_of("original").map(PathBuf::from),
        original_package: str_of("original_package").map(String::from),
        watch_issue: false,
        note: Some(reason.into()),
    };
    Ok(Entry {
        id: id.into(),
        retired: None,
        ext: Some(ext),
    })
}

/// Every declaration file in `dirs`, resolved, in a stable order.
fn files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = vec![];
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("json") && p.is_file() {
                out.push(p.canonicalize().unwrap_or(p));
            }
        }
    }
    out.sort();
    out
}

/// Bring the record in line with what is declared now.
///
/// Returns one line for each thing it did or could not do — a declaration
/// taken, followed, withdrawn, gone, or refused and why — for the log, so a
/// maintainer's broken file is findable rather than silently ignored.
pub fn sync(state_dir: &Path, src: &Sources, look: &Lookups) -> Result<Vec<String>, String> {
    let mut records = repair::read(state_dir)?;
    let mut said = vec![];
    let mut changed = false;
    let found = files(&src.dirs);

    // What each file declares now; a file that cannot be read is left out
    // entirely rather than treated as declaring nothing, or every record it
    // ever made would read as withdrawn.
    let mut declared: Vec<(PathBuf, Option<String>, Vec<Entry>)> = vec![];
    for f in &found {
        let owner = (src.owner)(f);
        let by = match &owner {
            Some(p) => format!("declared by {p}"),
            None => format!(
                "declared by {}",
                f.file_stem().and_then(|s| s.to_str()).unwrap_or("?")
            ),
        };
        let parsed = std::fs::read_to_string(f)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let Some(list) = parsed
            .as_ref()
            .and_then(|v| v.get("records"))
            .and_then(|r| r.as_array())
        else {
            said.push(format!(
                "declaration {} is not readable, left as it is",
                f.display()
            ));
            continue;
        };
        let mut entries = vec![];
        for v in list {
            match entry(v, &by) {
                Ok(e) => entries.push(e),
                Err(e) => said.push(format!("declaration {}: {e}", f.display())),
            }
        }
        declared.push((f.clone(), owner, entries));
    }
    let readable: Vec<&PathBuf> = declared.iter().map(|(f, _, _)| f).collect();

    // Records whose declaration file is gone, or no longer declares them.
    for r in records.iter_mut() {
        let Some(d) = r.declared.as_mut() else {
            continue;
        };
        let file = PathBuf::from(&d.file);
        if !file.exists() {
            if !d.gone {
                d.gone = true;
                changed = true;
                said.push(format!("{}: its declaration {} is gone", r.id, d.file));
            }
            continue;
        }
        if !readable.contains(&&file) {
            continue;
        }
        let still = declared
            .iter()
            .find(|(f, _, _)| *f == file)
            .is_some_and(|(_, _, es)| es.iter().any(|e| e.id == d.entry));
        if !still && d.retired.is_none() {
            d.retired = Some(String::new());
            changed = true;
            said.push(format!("{}: no longer declared by {}", r.id, d.file));
        }
    }

    for (file, owner, entries) in &declared {
        for e in entries {
            let fs = file.display().to_string();
            let existing = records.iter_mut().find(|r| {
                r.declared
                    .as_ref()
                    .is_some_and(|d| d.file == fs && d.entry == e.id)
            });
            match (existing, &e.retired, &e.ext) {
                (Some(r), Some(reason), _) => {
                    let d = r.declared.as_mut().unwrap();
                    if d.retired.as_deref() != Some(reason.as_str()) {
                        d.retired = Some(reason.clone());
                        changed = true;
                        said.push(format!("{}: withdrawn by its maintainer", r.id));
                    }
                }
                // Declared again, or still: follow what it says now. The
                // installed version follows too — the package that declared
                // this is the package being updated, so its own update is not
                // news about the record.
                (Some(r), None, Some(ext)) => {
                    let d = r.declared.as_mut().unwrap();
                    let mut touched = false;
                    if d.retired.take().is_some() || d.gone {
                        d.gone = false;
                        touched = true;
                    }
                    if d.package != *owner {
                        d.package = owner.clone();
                        touched = true;
                    }
                    if r.note != ext.note || r.upstream != ext.upstream || r.subject != ext.by {
                        r.note = ext.note.clone();
                        r.upstream = ext.upstream.clone();
                        r.subject = ext.by.clone();
                        touched = true;
                    }
                    let now = r
                        .upstream
                        .package
                        .as_deref()
                        .and_then(|p| (look.installed)(p));
                    if r.installed != now {
                        r.installed = now;
                        touched = true;
                    }
                    if touched {
                        changed = true;
                        said.push(format!("{}: follows its declaration", r.id));
                    }
                }
                // A new entry that is already withdrawn is not worth a record.
                (None, Some(_), _) => {}
                (None, None, Some(ext)) => {
                    match repair::external_record(ext.clone(), repair::next_id(&records), look) {
                        Ok(mut rec) => {
                            rec.declared = Some(Declared {
                                file: fs.clone(),
                                entry: e.id.clone(),
                                package: owner.clone(),
                                retired: None,
                                gone: false,
                            });
                            said.push(format!("{}: {} ({})", rec.id, rec.subject, e.id));
                            records.push(rec);
                            changed = true;
                        }
                        Err(err) => said.push(format!("declaration {fs}: {}: {err}", e.id)),
                    }
                }
                _ => {}
            }
        }
    }
    if changed {
        repair::save(state_dir, &records)?;
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::{Flag, Installed};
    use serde_json::json;
    use std::cell::RefCell;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("podshl-declared-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, name: &str, v: Value) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        p
    }

    thread_local! {
        static VERSION: RefCell<Option<String>> = const { RefCell::new(None) };
    }
    fn installed(p: &str) -> Option<Installed> {
        (p == "podshl-bin")
            .then(|| VERSION.with(|v| v.borrow().clone()))
            .flatten()
            .map(|version| Installed {
                version,
                source: "pacman".into(),
            })
    }
    fn nothing(_: &str) -> Option<Installed> {
        None
    }
    fn look() -> Lookups<'static> {
        Lookups {
            installed: &installed,
            available: &nothing,
            issue: None,
        }
    }
    /// The package manager's answer: a file in `pkgs/` belongs to the
    /// package named after it; anything else to nobody.
    fn owner(p: &Path) -> Option<String> {
        p.parent()
            .and_then(|d| d.file_name())
            .filter(|d| *d == "pkgs")
            .and(p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
    }

    fn flags(state: &Path) -> Vec<(String, Vec<Flag>)> {
        repair::review(state, &look())
            .into_iter()
            .map(|r| (r.record.id.clone(), r.flags))
            .collect()
    }

    /// RR24: a package's declaration becomes a record, attributed to the
    /// package the package manager says owns it — not to the name written in
    /// it — and follows the package's updates without a duplicate, and
    /// without an update of the declaring package counting as news.
    #[test]
    fn a_packages_declaration_becomes_a_record_that_follows_its_updates() {
        let base = tmp("rr24");
        let state = base.join("state");
        let pkgs = base.join("pkgs");
        VERSION.with(|v| *v.borrow_mut() = Some("0.1.7-1".into()));
        let decl = |reason: &str| {
            json!({ "records": [{
                "id": "outside-the-aur", "kind": "package", "package": "podshl-bin",
                "reason": reason, "until": { "issue": "https://github.com/o/r/issues/1" }
            }]})
        };
        write(
            &pkgs,
            "podshl-bin.json",
            decl("Installed from a PKGBUILD: the AUR is closed."),
        );
        let src = Sources {
            dirs: vec![pkgs.clone()],
            owner: &owner,
        };

        sync(&state, &src, &look()).unwrap();
        let all = repair::load(&state);
        assert_eq!(all.len(), 1, "{all:?}");
        let r = &all[0];
        assert_eq!(r.subject, "declared by podshl-bin");
        assert_eq!(
            r.declared.as_ref().unwrap().package.as_deref(),
            Some("podshl-bin")
        );
        assert_eq!(
            r.note.as_deref(),
            Some("Installed from a PKGBUILD: the AUR is closed.")
        );
        assert!(!r.watch_issue, "a declaration switched on a GitHub lookup");
        assert!(flags(&state).is_empty(), "a fresh declaration is flagged");

        // Nothing new: nothing changes, and there is still one record.
        assert!(sync(&state, &src, &look()).unwrap().is_empty());
        assert_eq!(repair::load(&state).len(), 1);

        // The package is updated and its declaration with it.
        VERSION.with(|v| *v.borrow_mut() = Some("0.1.8-1".into()));
        write(
            &pkgs,
            "podshl-bin.json",
            decl("Still outside the AUR, now with a reason that changed."),
        );
        sync(&state, &src, &look()).unwrap();
        let all = repair::load(&state);
        assert_eq!(all.len(), 1, "an update made a second record");
        assert_eq!(
            all[0].id, r.id,
            "the record lost its identity across the update"
        );
        assert!(all[0].note.as_deref().unwrap().contains("changed"));
        assert!(
            flags(&state).is_empty(),
            "the declaring package's own update is flagged"
        );

        // Who declared it is the package manager's answer, not the file's:
        // the same file nobody owns is attributed to nobody.
        let loose = base.join("home");
        write(&loose, "podshl-bin.json", decl("Claims to be podshl-bin."));
        let state2 = base.join("state2");
        sync(
            &state2,
            &Sources {
                dirs: vec![loose],
                owner: &owner,
            },
            &look(),
        )
        .unwrap();
        let r2 = &repair::load(&state2)[0];
        assert_eq!(r2.declared.as_ref().unwrap().package, None);
        assert_eq!(
            r2.subject, "declared by podshl-bin",
            "named after the file, and owned by nobody"
        );
    }

    /// RR25: the maintainer can take a declaration back. Withdrawn with a
    /// reason in a later version, dropped from the file, or gone with the
    /// package, the record says which — once — and is never removed.
    #[test]
    fn a_maintainer_withdraws_a_declaration_and_the_record_says_so_once() {
        let base = tmp("rr25");
        let state = base.join("state");
        let pkgs = base.join("pkgs");
        VERSION.with(|v| *v.borrow_mut() = Some("0.1.7-1".into()));
        let conf = base.join("pacman.conf");
        std::fs::write(&conf, "[podshl]\n").unwrap();
        let entry_pkg = json!({ "id": "outside-the-aur", "kind": "package", "package": "podshl-bin",
                                "reason": "Not in the AUR." });
        let entry_file = json!({ "id": "local-repo", "kind": "file", "path": conf.display().to_string(),
                                 "reason": "A local repository for podshl-bin." });
        write(
            &pkgs,
            "podshl-bin.json",
            json!({ "records": [entry_pkg.clone(), entry_file.clone()] }),
        );
        let src = Sources {
            dirs: vec![pkgs.clone()],
            owner: &owner,
        };
        sync(&state, &src, &look()).unwrap();
        assert_eq!(repair::load(&state).len(), 2);

        // A later version withdraws one with a reason, and drops the other.
        write(
            &pkgs,
            "podshl-bin.json",
            json!({ "records": [
                { "id": "outside-the-aur", "retired": "Now in the AUR; the local repository can go." }
            ]}),
        );
        sync(&state, &src, &look()).unwrap();
        let mut got = flags(&state);
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(
            got.iter().all(|(_, f)| f.len() == 1),
            "a withdrawal says more than that: {got:?}"
        );
        assert!(
            got.iter().any(|(_, f)| f.contains(&Flag::Retired {
                reason: "Now in the AUR; the local repository can go.".into()
            })),
            "{got:?}"
        );
        assert!(
            got.iter().any(|(_, f)| f.contains(&Flag::Retired {
                reason: String::new()
            })),
            "{got:?}"
        );

        // Looked at and kept: said once, then quiet.
        for (id, _) in &got {
            repair::looked_at(&state, id, &look()).unwrap();
        }
        assert!(
            flags(&state).is_empty(),
            "a withdrawn declaration is raised again"
        );
        assert_eq!(
            repair::load(&state).len(),
            2,
            "a withdrawn record was removed"
        );

        // The package is removed: its declaration file goes with it.
        std::fs::remove_file(pkgs.join("podshl-bin.json")).unwrap();
        sync(&state, &src, &look()).unwrap();
        let got = flags(&state);
        assert!(
            // Only that: its package being unknown now is the same news again.
            got.iter().all(|(_, f)| f == &vec![Flag::DeclarerGone]),
            "{got:?}"
        );
        assert_eq!(got.len(), 2);

        // A withdrawn entry nobody had a record of does not become one.
        let state2 = base.join("state2");
        write(
            &pkgs,
            "other.json",
            json!({ "records": [{ "id": "x", "retired": "gone" }] }),
        );
        sync(&state2, &src, &look()).unwrap();
        assert!(repair::load(&state2).is_empty());
    }

    /// A plugin declares from inside its own directory, and is attributed by
    /// that directory: a plugin added is found, a plugin removed leaves its
    /// records saying so — the same as a package, without a package manager.
    #[test]
    fn an_omarchy_plugin_declares_like_a_package() {
        let base = tmp("plugins");
        let root = base.join("plugins");
        let file = write(
            &root.join("omarchy-podshl/repairs.d"),
            "podshl.json",
            json!({ "records": [{ "id": "p", "kind": "package", "package": "podshl-bin",
                                  "reason": "The plugin builds the client from a PKGBUILD." }] }),
        );
        assert_eq!(
            plugin_dirs(&root),
            vec![root.join("omarchy-podshl/repairs.d")]
        );
        assert_eq!(
            plugin_of(&file, &root).as_deref(),
            Some("omarchy plugin omarchy-podshl")
        );
        assert_eq!(plugin_of(&base.join("elsewhere.json"), &root), None);

        VERSION.with(|v| *v.borrow_mut() = Some("0.1.7-1".into()));
        // Resolved, as `live_owner` resolves it: the files are.
        let croot = root.canonicalize().unwrap();
        let owner = |p: &Path| plugin_of(p, &croot);
        let state = base.join("state");
        let src = || Sources {
            dirs: plugin_dirs(&root),
            owner: &owner,
        };
        sync(&state, &src(), &look()).unwrap();
        let r = &repair::load(&state)[0];
        assert_eq!(r.subject, "declared by omarchy plugin omarchy-podshl");

        // The plugin is removed: its directory goes, and so does the
        // directory the next review reads.
        std::fs::remove_dir_all(root.join("omarchy-podshl")).unwrap();
        sync(&state, &src(), &look()).unwrap();
        let got = flags(&state);
        assert!(
            got.iter().any(|(_, f)| f.contains(&Flag::DeclarerGone)),
            "{got:?}"
        );
    }

    /// A declaration that cannot be taken is said, not guessed at: no reason,
    /// an unknown kind, a file that is not JSON. The rest is still taken, and
    /// a file that cannot be read does not make its records read as withdrawn.
    #[test]
    fn a_broken_declaration_is_said_and_the_rest_still_taken() {
        let base = tmp("rr24b");
        let state = base.join("state");
        let pkgs = base.join("pkgs");
        write(
            &pkgs,
            "a.json",
            json!({ "records": [
                { "id": "no-reason", "kind": "package", "package": "a" },
                { "id": "odd-kind", "kind": "service", "reason": "x" },
                { "id": "fine", "kind": "package", "package": "a", "reason": "A good one." }
            ]}),
        );
        let src = Sources {
            dirs: vec![pkgs.clone()],
            owner: &owner,
        };
        let said = sync(&state, &src, &look()).unwrap();
        assert!(
            said.iter()
                .any(|s| s.contains("no-reason") && s.contains("says why")),
            "{said:?}"
        );
        assert!(said.iter().any(|s| s.contains("odd-kind")), "{said:?}");
        assert_eq!(repair::load(&state).len(), 1);

        std::fs::write(pkgs.join("a.json"), "{ not json").unwrap();
        let said = sync(&state, &src, &look()).unwrap();
        assert!(said.iter().any(|s| s.contains("not readable")), "{said:?}");
        let withdrawn = flags(&state).into_iter().any(|(_, f)| {
            f.iter()
                .any(|x| matches!(x, Flag::Retired { .. } | Flag::DeclarerGone))
        });
        assert!(!withdrawn, "an unreadable declaration withdrew its records");
    }
}
