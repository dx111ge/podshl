//! Where the software on this machine says it came from.
//!
//! Every other reading in this client is one a **publisher asked for**: the
//! catalogue holds them, a manifest names them, and the person consents to each.
//! This one is the client's own question, asked on the person's behalf and on
//! nobody else's, which is why it is here and not in the read vocabulary. A
//! vocabulary entry is something a vendor may request; this must not be, because
//! its whole value is that a hostile publisher cannot switch it off.
//!
//! **What it is for.** A repository anchor's identity is `owner/name`, and a
//! name belongs to nobody -- GitHub holds 1872 repositories with `engram` in the
//! name. So the window shows candidates and the person recognises theirs. This
//! is the one fact that can help them do it and that an impostor cannot forge,
//! because it is not on their side of the wire: the machine's own package
//! database already records where each installed program came from, written by
//! the distribution rather than by anybody publishing here.
//!
//! Measured on this project's target desktop, 2026-09-14:
//!
//!     hyprland           https://github.com/hyprwm/Hyprland
//!     curl               https://curl.se/
//!     nvidia-open-dkms   https://www.nvidia.com/
//!
//! Both shapes fall out of one question, which is the point: a repository and a
//! domain are compared the same way and neither is special-cased.
//!
//! **What it is not.** It is not discovery -- nothing here searches, and a
//! machine that says nothing produces no answer rather than a guess. It is not
//! an accusation: a mismatch means the installed copy names a different home,
//! which happens legitimately for forks, vendored copies and distributions that
//! repackage. It is said, and the person decides.
//!
//! **Bounded like everything else.** One package name, matched against a strict
//! pattern before it is passed to anything; a query of the local database with
//! no network and no writes; output capped; and it is a read, so it is shown and
//! consented before it happens.

use std::path::PathBuf;

/// What a package name may be. Deliberately narrower than any packaging
/// system's own rule: this string is about to be an argument, and one that
/// needs escaping to be safe is refused instead of escaped.
pub(crate) fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.+@".contains(c))
}

/// Where an answer came from, so the window can say it rather than asserting a
/// fact with no author.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The URL the system records as this package's upstream.
    pub url: String,
    /// The package as the system names it.
    pub package: String,
    /// Which database answered — `pacman`, `dpkg`.
    pub source: &'static str,
}

fn tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The value of `key` out of `field: value` lines, first match, trimmed.
fn field(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let (k, v) = line.split_once(':')?;
        if k.trim().eq_ignore_ascii_case(key) {
            let v = v.trim();
            if !v.is_empty() && v != "None" {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// What the system package database says about `package`, or `None`.
///
/// `None` covers every uninteresting case at once — no package manager, no such
/// package, a package with no upstream recorded — and they are uninteresting
/// together because the answer to all of them is to say nothing.
pub fn declared_home(package: &str) -> Option<Provenance> {
    if !name_ok(package) {
        return None;
    }
    // `-Qi` queries the local database only: no network, no write, one package.
    // Not `-Ql` or a bare `-Q`, which would be an inventory of the machine —
    // the same line the read vocabulary draws at `pip list`.
    if let Some(exe) = tool("pacman") {
        if let Some((true, out)) = crate::reads::run_bounded(&exe, &["-Qi", package]) {
            if let Some(url) = field(&out, "URL") {
                return Some(Provenance { url, package: package.into(), source: "pacman" });
            }
        }
    }
    if let Some(exe) = tool("dpkg-query") {
        if let Some((true, out)) =
            crate::reads::run_bounded(&exe, &["-W", "-f=Homepage: ${Homepage}\n", package])
        {
            if let Some(url) = field(&out, "Homepage") {
                return Some(Provenance { url, package: package.into(), source: "dpkg" });
            }
        }
    }
    None
}

/// The comparable form of a URL: scheme, `www.`, case, trailing slash and a
/// `.git` suffix are not part of who published something.
pub fn canonical(url: &str) -> String {
    let mut s = url.trim().to_lowercase();
    for p in ["https://", "http://", "git+https://", "git://", "ssh://git@"] {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.to_string();
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest.to_string();
    }
    s = s.trim_end_matches('/').to_string();
    if let Some(rest) = s.strip_suffix(".git") {
        s = rest.to_string();
    }
    s
}

/// What to tell the person about the anchor they picked, if anything.
///
/// `None` is the common and correct answer: nothing installed under that name,
/// nothing recorded about it, or it agrees. Only a disagreement is worth a
/// person's attention, and even then it is a disagreement rather than a verdict.
pub fn disagrees_with(anchor_url: &str, package: &str) -> Option<Provenance> {
    let found = declared_home(package)?;
    let (a, b) = (canonical(anchor_url), canonical(&found.url));
    // A prefix match, not equality: a distribution may record the project's
    // website where the anchor is its repository, or the other way round, and
    // `github.com/hyprwm/hyprland` under `github.com/hyprwm/hyprland/issues` is
    // the same publisher. What is a disagreement is a different host or a
    // different owner, which is the case this exists for.
    if a.starts_with(&b) || b.starts_with(&a) {
        return None;
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_name_that_would_need_escaping_is_refused() {
        for bad in ["", "-Qi", "a b", "a;b", "a/b", "../etc", "a$(id)", "a'b"] {
            assert!(!name_ok(bad), "{bad:?} was accepted as a package name");
        }
        for good in ["hyprland", "nvidia-open-dkms", "lib32-nvidia-utils", "python3.12"] {
            assert!(name_ok(good), "{good:?} was refused");
        }
    }

    #[test]
    fn what_is_not_part_of_who_published_something_is_folded_away() {
        let same = [
            "https://github.com/hyprwm/Hyprland",
            "https://github.com/hyprwm/Hyprland/",
            "http://www.github.com/HyprWM/hyprland.git",
            "git+https://github.com/hyprwm/hyprland",
        ];
        let first = canonical(same[0]);
        for u in same {
            assert_eq!(canonical(u), first, "{u} did not fold to {first}");
        }
        assert_ne!(canonical("https://github.com/hyprwm/hyprland"),
                   canonical("https://github.com/hyprwmm/hyprland"),
                   "an owner one letter out folded together with the real one");
    }

    /// The thing this exists to catch, and the three it must stay quiet about.
    #[test]
    fn only_a_real_disagreement_is_worth_saying() {
        let anchor = "https://github.com/hyprwm/hyprland/";
        for agrees in [
            "https://github.com/hyprwm/Hyprland",
            "https://github.com/hyprwm/hyprland/issues",
        ] {
            let (a, b) = (canonical(anchor), canonical(agrees));
            assert!(a.starts_with(&b) || b.starts_with(&a), "{agrees} read as a disagreement");
        }
        let (a, b) = (canonical(anchor), canonical("https://github.com/someone/hyprland"));
        assert!(!(a.starts_with(&b) || b.starts_with(&a)),
                "a different owner was not a disagreement — which is the whole case");
    }
}


