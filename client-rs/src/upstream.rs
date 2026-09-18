//! What an upstream issue or pull request says, asked of GitHub by code.
//!
//! **Only when the person chose to watch it** (`repair::set_watch`). Each
//! lookup tells GitHub, by this machine's address, which issue it follows, so
//! it is off unless switched on per record, and it goes straight to GitHub's
//! public API — not through the operator, who would otherwise learn it instead.
//! No account and no token: the public API allows sixty requests an hour, and a
//! record is asked at most once a day.
//!
//! What it finds out, and nothing more:
//!
//! * an issue: open or closed;
//! * a pull request: open, closed, or merged — and if merged, **the first
//!   release that contains the merge commit**, found by asking GitHub whether
//!   the commit is an ancestor of each release after the merge. That release is
//!   what `repair` compares with the installed version.
//!
//! A merged pull request is a reason to look again and never proof that a local
//! fix can go: the release may not have reached this machine, the distribution
//! may have patched it differently, the fix may not have held.

use crate::repair::IssueState;
use serde_json::Value;

const API: &str = "https://api.github.com";

/// At most this many releases are asked whether they contain the commit.
const MAX_CANDIDATES: usize = 8;

fn ident_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && s != "." && s != ".."
}

/// `https://github.com/<owner>/<repo>/issues/<n>` or `…/pull/<n>`: the owner,
/// the repository, the number, and whether the link says it is a pull request.
/// Anything else is not something this looks up.
pub fn parse(url: &str) -> Option<(String, String, u64, bool)> {
    let rest = url.strip_prefix("https://github.com/")?;
    let rest = rest.split(['#', '?']).next()?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    let [owner, repo, kind, number] = parts.as_slice() else { return None };
    let is_pull = match *kind {
        "issues" => false,
        "pull" => true,
        _ => return None,
    };
    let n: u64 = number.parse().ok().filter(|n| *n > 0)?;
    (ident_ok(owner) && ident_ok(repo)).then(|| (owner.to_string(), repo.to_string(), n, is_pull))
}

fn failed(why: impl Into<String>, is_pull: bool) -> IssueState {
    IssueState { state: "unknown".into(), is_pull, error: Some(why.into()), ..IssueState::default() }
}

/// Ask GitHub, through `get` (a path under the API, answered as JSON).
pub fn check(url: &str, get: &dyn Fn(&str) -> Result<Value, String>) -> IssueState {
    let Some((owner, repo, n, pull_hint)) = parse(url) else {
        return failed("not a GitHub issue or pull request link", false);
    };
    let base = format!("/repos/{owner}/{repo}");
    let issue = match get(&format!("{base}/issues/{n}")) {
        Ok(v) => v,
        Err(e) => return failed(e, pull_hint),
    };
    // GitHub answers a pull request's number on the issues route too, and says
    // so; the link's own spelling is only a hint.
    let is_pull = issue.get("pull_request").is_some_and(|p| !p.is_null()) || pull_hint;
    let state = issue["state"].as_str().unwrap_or("unknown").to_string();
    if !is_pull {
        return IssueState { state, is_pull, ..IssueState::default() };
    }
    let pr = match get(&format!("{base}/pulls/{n}")) {
        Ok(v) => v,
        Err(e) => return failed(e, true),
    };
    if pr["merged"].as_bool() != Some(true) {
        return IssueState { state, is_pull, ..IssueState::default() };
    }
    let (Some(sha), Some(merged_at)) = (pr["merge_commit_sha"].as_str(), pr["merged_at"].as_str())
    else {
        return failed("merged, without a merge commit", true);
    };
    if !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return failed("merged, with a malformed merge commit", true);
    }
    let released_in = first_release_containing(&base, sha, merged_at, get);
    IssueState {
        state: "merged".into(),
        is_pull,
        released_in: released_in.as_ref().ok().cloned().flatten(),
        error: released_in.err(),
        ..IssueState::default()
    }
}

/// The earliest published release after the merge whose tag contains `sha`.
fn first_release_containing(
    base: &str,
    sha: &str,
    merged_at: &str,
    get: &dyn Fn(&str) -> Result<Value, String>,
) -> Result<Option<String>, String> {
    let releases = get(&format!("{base}/releases?per_page=50"))?;
    let mut candidates: Vec<(String, String)> = releases
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"].as_bool() != Some(true) && r["prerelease"].as_bool() != Some(true))
        .filter_map(|r| Some((r["published_at"].as_str()?.to_string(), r["tag_name"].as_str()?.to_string())))
        // RFC 3339 in UTC, as GitHub writes it, orders as text.
        .filter(|(at, _)| at.as_str() >= merged_at)
        .filter(|(_, tag)| tag_ok(tag))
        .collect();
    candidates.sort();
    // One tag GitHub cannot compare (a tag on another history, a commit it has
    // since dropped) says nothing about the next one; only when no release is
    // found does a failure become the answer, because "in no release" would
    // then be a guess.
    let mut failure = None;
    for (_, tag) in candidates.into_iter().take(MAX_CANDIDATES) {
        match get(&format!("{base}/compare/{tag}...{sha}")) {
            // `behind`: the commit is behind the tag, so the tag contains it.
            Ok(cmp) if matches!(cmp["status"].as_str(), Some("behind" | "identical")) => {
                return Ok(Some(tag));
            }
            Ok(_) => {}
            Err(e) => failure = Some(e),
        }
    }
    failure.map_or(Ok(None), Err)
}

/// A tag is put into a URL path, so it is held to the characters tags use.
fn tag_ok(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 100
        && tag.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c))
}

/// The real `get`: GitHub's public API, no credentials, bounded like every
/// other request this client makes.
pub fn github_get(path: &str) -> Result<Value, String> {
    let url = format!("{API}{path}");
    let run = async move {
        let resp = crate::http::client()
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", concat!("podshl-client/", env!("CARGO_PKG_VERSION")))
            .send()
            .await
            .map_err(|e| m!("unreachable", e = e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(m!("upstream_http", s = status.as_u16()));
        }
        crate::http::json_capped(resp, crate::http::MAX_BODY).await
    };
    // Called from the command line and from a blocking task of the window,
    // never on a runtime thread, so a runtime of its own is right here.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    #[test]
    fn only_a_github_issue_or_pull_link_is_looked_up() {
        assert_eq!(parse("https://github.com/hyprwm/Hyprland/issues/1234"),
                   Some(("hyprwm".into(), "Hyprland".into(), 1234, false)));
        assert_eq!(parse("https://github.com/basecamp/omarchy/pull/7/files#diff"), None,
                   "a sub-page is not the pull request");
        assert_eq!(parse("https://github.com/basecamp/omarchy/pull/7#issuecomment-1"),
                   Some(("basecamp".into(), "omarchy".into(), 7, true)));
        for bad in ["http://github.com/a/b/issues/1", "https://gitlab.com/a/b/issues/1",
                    "https://github.com/a/b/issues/0", "https://github.com/a/b/issues/x",
                    "https://github.com/../b/issues/1", "https://github.com/a/b%2F/issues/1",
                    "https://github.com/a/b/commit/1"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    /// RR9: a merged pull request's first release is found by asking which
    /// release contains the merge commit, and only releases after the merge,
    /// oldest first, are asked.
    #[test]
    fn a_merged_pull_request_is_followed_to_the_release_that_contains_it() {
        let asked = RefCell::new(vec![]);
        let get = |path: &str| -> Result<Value, String> {
            asked.borrow_mut().push(path.to_string());
            Ok(match path {
                "/repos/o/r/issues/5" => json!({"state": "closed", "pull_request": {}}),
                "/repos/o/r/pulls/5" => json!({"merged": true, "merge_commit_sha": "abc123",
                                               "merged_at": "2026-05-01T10:00:00Z"}),
                "/repos/o/r/releases?per_page=50" => json!([
                    {"tag_name": "v2.1.0", "published_at": "2026-07-01T00:00:00Z"},
                    {"tag_name": "v2.0.1", "published_at": "2026-05-20T00:00:00Z"},
                    {"tag_name": "v2.0.2", "published_at": "2026-06-10T00:00:00Z"},
                    {"tag_name": "v2.0.0", "published_at": "2026-04-01T00:00:00Z"},
                    {"tag_name": "v2.2.0-rc1", "published_at": "2026-06-01T00:00:00Z", "prerelease": true},
                    {"tag_name": "v9", "published_at": "2026-06-02T00:00:00Z", "draft": true},
                ]),
                // v2.0.1 was cut from a branch without the fix; v2.0.2 is a
                // tag GitHub cannot compare, which does not end the search.
                "/repos/o/r/compare/v2.0.1...abc123" => json!({"status": "diverged"}),
                "/repos/o/r/compare/v2.0.2...abc123" => return Err("404".into()),
                "/repos/o/r/compare/v2.1.0...abc123" => json!({"status": "behind"}),
                other => return Err(format!("unexpected {other}")),
            })
        };
        let st = check("https://github.com/o/r/pull/5", &get);
        assert_eq!(st.state, "merged");
        assert!(st.is_pull);
        assert_eq!(st.released_in.as_deref(), Some("v2.1.0"), "{st:?}");
        assert_eq!(st.error, None);
        let asked = asked.borrow();
        assert!(!asked.iter().any(|p| p.contains("v2.0.0") || p.contains("rc1") || p.contains("v9")),
                "a release before the merge, a pre-release or a draft was asked: {asked:?}");
    }

    #[test]
    fn an_issue_a_failure_and_an_unmerged_pull_are_said_as_what_they_are() {
        let issue = |p: &str| -> Result<Value, String> {
            assert_eq!(p, "/repos/o/r/issues/9");
            Ok(json!({"state": "closed"}))
        };
        let st = check("https://github.com/o/r/issues/9", &issue);
        assert_eq!((st.state.as_str(), st.is_pull, st.released_in.as_deref()), ("closed", false, None));

        let down = |_: &str| -> Result<Value, String> { Err("unreachable".into()) };
        let st = check("https://github.com/o/r/issues/9", &down);
        assert_eq!(st.state, "unknown");
        assert_eq!(st.error.as_deref(), Some("unreachable"));

        let open_pr = |p: &str| -> Result<Value, String> {
            Ok(match p {
                "/repos/o/r/issues/3" => json!({"state": "open", "pull_request": {}}),
                _ => json!({"merged": false}),
            })
        };
        let st = check("https://github.com/o/r/issues/3", &open_pr);
        assert_eq!((st.state.as_str(), st.is_pull), ("open", true), "a pull request linked as an issue");
    }
}
