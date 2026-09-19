//! Recording what a coding agent changes, as it changes it.
//!
//! **The record depends on somebody writing into it, and the person it is
//! for does not.** Whoever patches a config at eleven at night to get to bed
//! does not type `repairs begin` first, and an agent does not either unless
//! something makes it. So the agent's own hook system does: before a file is
//! written, a copy is kept (`begin`); after it is written, the record is
//! finished (`done`). The agent never knows, and nothing depends on it.
//!
//! **It only ever writes down.** It never stops the agent, never changes what
//! the agent writes, and never fails in a way the agent sees: every path out
//! of here is exit 0, and what went wrong goes to stderr. A record missed is
//! a gap; an agent held up by a record is a tool somebody removes within the
//! hour.
//!
//! **Not everything is worth a record.** A file inside a git working tree
//! already has a history, and one better than this; recording every edit an
//! agent makes to source code would bury the one change to `~/.config` that
//! matters under hundreds that do not. The same goes for temporary files, the
//! agent's own directory, and this record itself.
//!
//! **One record per file per session.** An agent edits a config five times
//! getting it right; five records would say the same thing five times. The
//! first edit keeps the copy from before the session, and later ones only
//! bring the record's digest up to date — so undo goes back to before the
//! agent touched the file, which is what somebody undoing it means.
//!
//! **What it cannot see.** Only the agent's own file tools pass through its
//! hooks. A change made through a shell command — `sed -i`, `tee`, a script —
//! is not a file write the agent reports, and is not recorded here.
//!
//! Only Claude Code for now. Each agent has its own hook format and its own
//! input, and a format written from documentation rather than measured is the
//! kind of hole that opens without a sound — the same rule `omarchy.rs` keeps
//! for calling an agent headless.

use crate::repair::{self, External, Lookups, Upstream};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The agents whose hook format has been measured.
pub const MEASURED: &[&str] = &["claude"];

/// The tools whose writes are recorded.
pub const MATCHER: &str = "Write|Edit|MultiEdit|NotebookEdit";

/// Why a file was not recorded. Not logged: a skipped file is the ordinary
/// case, and a line per skip would fill the log with every edit to source.
#[derive(Debug, PartialEq)]
pub enum Skip {
    NoPath,
    InGit,
    Temporary,
    AgentsOwn,
    TheRecordItself,
    NotAFile,
}

/// Where the record lives, where the agent keeps its own files, and what
/// counts as temporary. A value rather than constants so the flow can be
/// tested: a test's files live in the temp dir, which the live rules skip.
pub struct Places {
    pub state_dir: PathBuf,
    pub agent_dir: PathBuf,
    pub temp: Vec<PathBuf>,
}

impl Places {
    /// This machine's.
    pub fn live(state_dir: &Path, home: &Path) -> Places {
        Places {
            state_dir: state_dir.to_path_buf(),
            agent_dir: claude_dir(home),
            temp: vec![
                std::env::temp_dir(),
                PathBuf::from("/tmp"),
                PathBuf::from("/var/tmp"),
            ],
        }
    }
}

/// Where Claude Code keeps its own files. `CLAUDE_CONFIG_DIR` moves it.
pub fn claude_dir(home: &Path) -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
}

/// Whether a change to `path` is one the record should hold.
pub fn worth_recording(path: &Path, at: &Places) -> Result<(), Skip> {
    if path.is_dir() {
        return Err(Skip::NotAFile);
    }
    if path.starts_with(&at.state_dir) {
        return Err(Skip::TheRecordItself);
    }
    if path.starts_with(&at.agent_dir) {
        return Err(Skip::AgentsOwn);
    }
    if at.temp.iter().any(|t| path.starts_with(t)) {
        return Err(Skip::Temporary);
    }
    // Every directory above the file, up to the filesystem root: `.git` as a
    // directory is a repository, as a file a worktree or a submodule.
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.join(".git").exists() {
            return Err(Skip::InGit);
        }
        dir = d.parent();
    }
    Ok(())
}

/// The file a hook call is about, from Claude Code's input.
fn target_of(input: &Value) -> Option<PathBuf> {
    let t = input.get("tool_input")?;
    let p = t
        .get("file_path")
        .or_else(|| t.get("notebook_path"))
        .and_then(|v| v.as_str())?;
    let p = PathBuf::from(p);
    if p.is_absolute() {
        return Some(p);
    }
    let cwd = input.get("cwd").and_then(|v| v.as_str())?;
    Some(Path::new(cwd).join(p))
}

/// Which record belongs to which file in which session.
fn sessions_path(state_dir: &Path) -> PathBuf {
    state_dir.join("agent-sessions.json")
}

fn load_sessions(state_dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(sessions_path(state_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_sessions(state_dir: &Path, s: &BTreeMap<String, String>) -> Result<(), String> {
    std::fs::create_dir_all(state_dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(sessions_path(state_dir), text).map_err(|e| e.to_string())
}

fn key(session: &str, path: &Path) -> String {
    format!("{session}\n{}", path.display())
}

/// What one hook call did, for the log and for tests.
#[derive(Debug, PartialEq)]
pub enum Done {
    Skipped(Skip),
    /// A copy was kept before the agent wrote.
    Began(String),
    /// The agent wrote, and the record is finished.
    Finished(String),
    /// A later edit in the same session: the record's digest is current again.
    Refreshed(String),
    /// A file the agent created: recorded without a copy, there was nothing.
    Added(String),
    /// Before a write to a file this session already recorded: nothing to do.
    AlreadyRecorded(String),
}

/// One call of the hook: `event` is `pre` or `post`, `input` what the agent
/// passed on stdin.
pub fn handle(event: &str, agent: &str, input: &str, at: &Places) -> Result<Done, String> {
    if event != "pre" && event != "post" {
        return Err(format!("unknown hook event {event:?}: pre or post"));
    }
    let input: Value = serde_json::from_str(input).map_err(|e| format!("not JSON: {e}"))?;
    let Some(path) = target_of(&input) else {
        return Ok(Done::Skipped(Skip::NoPath));
    };
    if let Err(skip) = worth_recording(&path, at) {
        return Ok(Done::Skipped(skip));
    }
    let state_dir = at.state_dir.as_path();
    let session = input
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("no-session")
        .to_string();
    let k = key(&session, &path);
    let mut sessions = load_sessions(state_dir);
    let ext = || External {
        kind: "file".into(),
        by: agent.to_string(),
        path: Some(path.clone()),
        upstream: Upstream::default(),
        original_path: None,
        original_package: None,
        watch_issue: false,
        note: Some(format!("recorded by the {agent} hook, session {session}")),
    };
    let look = Lookups::offline();
    if event == "pre" {
        if let Some(id) = sessions.get(&k) {
            return Ok(Done::AlreadyRecorded(id.clone()));
        }
        if !path.is_file() {
            // Nothing to keep a copy of; `post` records the new file.
            return Ok(Done::Skipped(Skip::NotAFile));
        }
        let rec = repair::begin_external(state_dir, ext(), &look)?;
        sessions.insert(k, rec.id.clone());
        save_sessions(state_dir, &sessions)?;
        return Ok(Done::Began(rec.id));
    }
    if !path.is_file() {
        return Ok(Done::Skipped(Skip::NotAFile));
    }
    match sessions.get(&k).cloned() {
        Some(id) => {
            let pending = repair::load(state_dir)
                .iter()
                .any(|r| r.id == id && r.state == "pending");
            if pending {
                repair::finish_external(state_dir, &id)?;
                Ok(Done::Finished(id))
            } else {
                repair::refresh_digest(state_dir, &id)?;
                Ok(Done::Refreshed(id))
            }
        }
        None => {
            let rec = repair::add_external(state_dir, ext(), &look)?;
            sessions.insert(k, rec.id.clone());
            save_sessions(state_dir, &sessions)?;
            Ok(Done::Added(rec.id))
        }
    }
}

/// The command a hook runs, spelled for a shell on every system: the program
/// in double quotes with forward slashes, which `bash` and `cmd` both read.
pub fn hook_command(exe: &Path, client: bool, event: &str, agent: &str) -> Result<String, String> {
    let s = exe.to_string_lossy().replace('\\', "/");
    if s.chars()
        .any(|c| c == '"' || c == '\'' || c == '`' || c == '$' || c.is_control())
    {
        return Err(format!("{s} cannot be written into a hook safely"));
    }
    let sub = if client {
        "repairs agent-hook"
    } else {
        "agent-hook"
    };
    Ok(format!("\"{s}\" {sub} {event} --agent {agent}"))
}

/// Is this hook entry one of ours? By the command, not by position: the
/// person's own hooks stay where they are and as they are.
fn ours(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains("agent-hook") && c.contains("podshl"))
            })
        })
}

/// Claude Code's settings with this hook in them, or taken out again.
///
/// Everything else in the file is kept. Entries of ours from an earlier
/// install are replaced rather than added to, so installing twice leaves one.
pub fn claude_settings(
    current: &Value,
    pre: &str,
    post: &str,
    install: bool,
) -> Result<Value, String> {
    let mut out = if current.is_null() {
        json!({})
    } else {
        current.clone()
    };
    let obj = out
        .as_object_mut()
        .ok_or("the agent's settings are not a JSON object")?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or("the agent's `hooks` setting is not an object")?;
    for (event, command) in [("PreToolUse", pre), ("PostToolUse", post)] {
        let list = hooks.entry(event).or_insert_with(|| json!([]));
        let list = list
            .as_array_mut()
            .ok_or(format!("the agent's `hooks.{event}` is not a list"))?;
        list.retain(|e| !ours(e));
        if install {
            list.push(json!({
                "matcher": MATCHER,
                "hooks": [{ "type": "command", "command": command }]
            }));
        }
    }
    // Leave no empty shells behind on removal.
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        obj.remove("hooks");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("podshl-agent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn places(base: &Path) -> Places {
        Places {
            state_dir: base.join("state"),
            agent_dir: base.join("home/.claude"),
            temp: vec![],
        }
    }

    /// What Claude Code passes a hook for an edit.
    fn call(path: &Path, session: &str) -> String {
        json!({
            "session_id": session,
            "cwd": "/",
            "hook_event_name": "PreToolUse",
            "tool_name": "Edit",
            "tool_input": { "file_path": path.display().to_string() }
        })
        .to_string()
    }

    /// RR21: an agent's edit to a file outside any repository is recorded,
    /// with a copy from before it, once per session; an edit inside a
    /// repository, a temporary file, the agent's own directory and the record
    /// itself are not.
    #[test]
    fn an_agents_edit_outside_a_repository_is_recorded_once_per_session() {
        let base = tmp("rr21");
        let at = places(&base);
        let conf = base.join("home/.config/app/app.conf");
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::write(&conf, "mode=old\n").unwrap();

        // Before the agent writes: a copy. After: finished.
        let Ok(Done::Began(id)) = handle("pre", "claude", &call(&conf, "s1"), &at) else {
            panic!("the first edit was not begun");
        };
        std::fs::write(&conf, "mode=new\n").unwrap();
        assert_eq!(
            handle("post", "claude", &call(&conf, "s1"), &at),
            Ok(Done::Finished(id.clone()))
        );

        // The same file again in the same session: the same record, its
        // digest brought up to date, its copy still the one from before.
        assert_eq!(
            handle("pre", "claude", &call(&conf, "s1"), &at),
            Ok(Done::AlreadyRecorded(id.clone()))
        );
        std::fs::write(&conf, "mode=newer\n").unwrap();
        assert_eq!(
            handle("post", "claude", &call(&conf, "s1"), &at),
            Ok(Done::Refreshed(id.clone()))
        );
        let all = repair::load(&at.state_dir);
        assert_eq!(all.len(), 1, "one file, one session, one record: {all:?}");
        let rec = &all[0];
        assert_eq!(rec.state, "applied");
        assert_eq!(rec.subject, "claude");
        let copy = std::fs::read_to_string(rec.backup.as_ref().expect("no copy")).unwrap();
        assert_eq!(
            copy, "mode=old\n",
            "the copy is not from before the session"
        );
        // Up to date, so nothing to look at yet.
        let found = repair::review(&at.state_dir, &Lookups::offline());
        assert!(
            found.is_empty(),
            "a record the agent just finished is flagged: {found:?}"
        );
        // Then something else rewrites it: now it is.
        std::fs::write(&conf, "mode=reset\n").unwrap();
        assert_eq!(repair::review(&at.state_dir, &Lookups::offline()).len(), 1);

        // Another session is another record.
        assert!(matches!(
            handle("pre", "claude", &call(&conf, "s2"), &at),
            Ok(Done::Began(other)) if other != id
        ));

        // A file the agent creates: recorded without a copy.
        let fresh = base.join("home/.config/app/extra.conf");
        assert_eq!(
            handle("pre", "claude", &call(&fresh, "s1"), &at),
            Ok(Done::Skipped(Skip::NotAFile))
        );
        std::fs::write(&fresh, "x=1\n").unwrap();
        assert!(matches!(
            handle("post", "claude", &call(&fresh, "s1"), &at),
            Ok(Done::Added(_))
        ));

        // And what is not recorded at all.
        let repo = base.join("home/src/project");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let code = repo.join("src/main.rs");
        std::fs::create_dir_all(code.parent().unwrap()).unwrap();
        std::fs::write(&code, "fn main() {}\n").unwrap();
        assert_eq!(
            handle("pre", "claude", &call(&code, "s1"), &at),
            Ok(Done::Skipped(Skip::InGit))
        );
        assert_eq!(
            worth_recording(&at.agent_dir.join("settings.json"), &at),
            Err(Skip::AgentsOwn)
        );
        assert_eq!(
            worth_recording(&at.state_dir.join("repairs.json"), &at),
            Err(Skip::TheRecordItself)
        );
        let live = Places::live(&at.state_dir, &base.join("home"));
        assert_eq!(worth_recording(&conf, &live), Err(Skip::Temporary));
    }

    /// Input that is not what Claude Code sends is an error the command line
    /// turns into stderr and exit 0 — never a panic, never a stopped agent.
    #[test]
    fn a_hook_call_it_cannot_read_is_an_error_not_a_stop() {
        let base = tmp("rr21b");
        let at = places(&base);
        assert!(handle("pre", "claude", "not json", &at).is_err());
        assert_eq!(
            handle("pre", "claude", "{}", &at),
            Ok(Done::Skipped(Skip::NoPath))
        );
        assert!(handle("sideways", "claude", &call(&base.join("f"), "s"), &at).is_err());
    }

    /// RR22: installing the hook keeps everything else in the agent's
    /// settings, installing twice leaves one entry, and removing it takes out
    /// only ours.
    #[test]
    fn the_agents_settings_keep_everything_that_is_not_ours() {
        let theirs = json!({
            "theme": "dark",
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "their-guard" }] }
                ]
            }
        });
        let exe = Path::new("/home/x/.local/bin/podshl-repairs");
        let pre = hook_command(exe, false, "pre", "claude").unwrap();
        let post = hook_command(exe, false, "post", "claude").unwrap();
        let once = claude_settings(&theirs, &pre, &post, true).unwrap();
        let twice = claude_settings(&once, &pre, &post, true).unwrap();
        assert_eq!(once, twice, "installing twice changed the settings");
        assert_eq!(twice["theme"], "dark");
        let pre_list = twice["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_list.len(), 2, "{pre_list:?}");
        assert_eq!(pre_list[0]["hooks"][0]["command"], "their-guard");
        assert_eq!(pre_list[1]["matcher"], MATCHER);
        assert_eq!(
            twice["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "\"/home/x/.local/bin/podshl-repairs\" agent-hook post --agent claude"
        );

        let removed = claude_settings(&twice, &pre, &post, false).unwrap();
        assert_eq!(
            removed, theirs,
            "removing left something, or took something of theirs"
        );

        // Nothing there before: nothing left after.
        let empty = claude_settings(&Value::Null, &pre, &post, true).unwrap();
        assert_eq!(
            claude_settings(&empty, &pre, &post, false).unwrap(),
            json!({})
        );

        // Windows: forward slashes, double quotes, the client's spelling.
        assert_eq!(
            hook_command(
                Path::new(r"C:\Users\x\.local\bin\podshl-client.exe"),
                true,
                "pre",
                "claude"
            )
            .unwrap(),
            "\"C:/Users/x/.local/bin/podshl-client.exe\" repairs agent-hook pre --agent claude"
        );
        assert!(hook_command(
            Path::new("/home/x/it's/podshl-repairs"),
            false,
            "pre",
            "claude"
        )
        .is_err());
    }
}
