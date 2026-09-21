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
//! **Shell commands too.** Agents write config files with a shell command as
//! often as with their file tools — `cat >> file`, `sed -i`, `tee` — and
//! Omarchy's own instructions for agents do it that way. Before the agent
//! runs a command, every file the command names is copied aside; after it,
//! each one that changed becomes a record with that copy, and each one the
//! command created becomes a record without. Nothing the command did not
//! name: a file reached through a variable, a glob or a script it calls is a
//! gap, and one this says rather than guesses at.
//!
//! **Measured for Claude Code, and honest about the rest.** Each agent has its
//! own hook format and its own settings file, and a format written from
//! documentation rather than measured is the kind of hole that opens without a
//! sound — the same rule `omarchy.rs` keeps for calling an agent headless. An
//! agent refused by name was the whole of it until somebody with another agent
//! was asked to test this and got a refusal instead, and nothing came back from
//! them. So there are two ways on, and both say which one was taken: measuring
//! keeps what the agent really sends and records nothing until it is known
//! (below), and a format in `agent-formats.json` may be read off a page rather
//! than walked — taken with `--guessed`, never counted as measured, saying so
//! in every record it makes, and keeping a sample of every call it could not
//! read.

use crate::repair::{self, External, Lookups, Upstream};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The agents whose hook format has been measured.
pub const MEASURED: &[&str] = &["claude"];

/// The tools whose writes are recorded: the file tools, and the shell.
pub const MATCHER: &str = "Write|Edit|MultiEdit|NotebookEdit|Bash";

/// A file larger than this is not copied aside before a shell command: a
/// config file is not, and a binary a command merely names should not cost
/// a copy on every call.
const SHELL_COPY_MAX: u64 = 4 * 1024 * 1024;

/// At most this many files named in one shell command are looked at.
const SHELL_PATHS_MAX: usize = 32;

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
    /// A copy somebody keeps by hand (`file.bak.123`, `file.orig`, `file~`):
    /// the change is the file next to it.
    AHandCopy,
}

/// Where the record lives, where the agent keeps its own files, and what
/// counts as temporary. A value rather than constants so the flow can be
/// tested: a test's files live in the temp dir, which the live rules skip.
pub struct Places {
    pub home: PathBuf,
    pub state_dir: PathBuf,
    pub agent_dir: PathBuf,
    pub temp: Vec<PathBuf>,
}

impl Places {
    /// This machine's.
    pub fn live(state_dir: &Path, home: &Path) -> Places {
        Places {
            home: home.to_path_buf(),
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
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.ends_with('~')
        || name.contains(".bak")
        || name.ends_with(".orig")
        || name.ends_with(".swp")
    {
        return Err(Skip::AHandCopy);
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

// ------------------------------------------------------------------ what an agent sends

/// Where an agent's hook input keeps the things a record needs: dotted keys,
/// tried in order, the first one that is there wins.
///
/// Claude Code's is below and built in, because it was walked. Another agent's
/// can be put in `agent-formats.json` next to the record — **read from that
/// agent's documentation is allowed here, and says so**: `measured` is false,
/// the install says it out loud, every record it makes carries it, and every
/// call it cannot read is kept as a sample. A guess that tells you where it is
/// wrong is a different thing from a guess that fails quietly.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Format {
    pub agent: String,
    /// True only for a format walked on a machine, never for one read off a
    /// page.
    #[serde(default)]
    pub measured: bool,
    /// Where an unmeasured one was read from, for whoever measures it later.
    #[serde(default)]
    pub source: Option<String>,
    /// The key holding the name of the tool the agent is about to use.
    pub tool: Vec<String>,
    /// Tool names that mean a shell command rather than a file write.
    pub shell: Vec<String>,
    /// The file a write is about.
    pub path: Vec<String>,
    /// The command a shell call will run.
    pub command: Vec<String>,
    /// The directory it runs in, for the relative paths in it.
    pub cwd: Vec<String>,
    /// What tells one of the agent's sessions from another.
    pub session: Vec<String>,
    /// What tells one tool call from another, when the agent gives it.
    #[serde(default)]
    pub call_id: Vec<String>,
}

/// Claude Code's, measured.
pub fn claude_format() -> Format {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect();
    Format {
        agent: "claude".into(),
        measured: true,
        source: None,
        tool: v(&["tool_name"]),
        shell: v(&["Bash"]),
        path: v(&["tool_input.file_path", "tool_input.notebook_path"]),
        command: v(&["tool_input.command"]),
        cwd: v(&["cwd"]),
        session: v(&["session_id"]),
        call_id: v(&["tool_use_id"]),
    }
}

/// Where formats for agents nobody here has walked are kept.
pub fn formats_path(state_dir: &Path) -> PathBuf {
    state_dir.join("agent-formats.json")
}

/// The formats in that file. A file that is not there, not JSON, or holds
/// something else is no formats: the hook is called with an agent waiting.
pub fn load_formats(state_dir: &Path) -> Vec<Format> {
    let Ok(text) = std::fs::read_to_string(formats_path(state_dir)) else {
        return vec![];
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return vec![];
    };
    let list = match v.get("formats") {
        Some(l) => l.clone(),
        None => v,
    };
    serde_json::from_value::<Vec<Format>>(list)
        .unwrap_or_default()
        .into_iter()
        .filter(|f| plain_agent_name(&f.agent).is_ok() && !f.tool.is_empty())
        .map(|mut f| {
            // A file cannot promote itself to measured: measuring happens on
            // a machine, and `MEASURED` is the only place that says so.
            f.measured = MEASURED.contains(&f.agent.as_str());
            f
        })
        .collect()
}

/// How this agent's calls are to be read, if there is a way at all.
pub fn format_for(state_dir: &Path, agent: &str) -> Option<Format> {
    if agent == "claude" {
        return Some(claude_format());
    }
    load_formats(state_dir)
        .into_iter()
        .find(|f| f.agent == agent)
}

/// A value under a dotted key: `tool_input.file_path`.
fn at_key<'a>(input: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut here = input;
    for part in dotted.split('.') {
        here = here.get(part)?;
    }
    Some(here)
}

/// The first of these keys that holds a string.
fn first_str<'a>(input: &'a Value, keys: &[String]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| at_key(input, k).and_then(|v| v.as_str()))
}

/// The file a hook call is about.
fn target_of(input: &Value, fmt: &Format) -> Option<PathBuf> {
    let p = PathBuf::from(first_str(input, &fmt.path)?);
    if p.is_absolute() {
        return Some(p);
    }
    let cwd = first_str(input, &fmt.cwd)?;
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
    /// Before a shell command: this many files it names are watched.
    ShellWatching(usize),
    /// After a shell command: the records it made or brought up to date.
    ShellRecorded(Vec<String>),
    /// An agent whose format is not known: what it sent was kept, and nothing
    /// was recorded.
    Measured(PathBuf),
    /// Enough samples of it are kept; this one was not.
    EnoughSamples(usize),
    /// An agent whose format is not known, and nobody is measuring it.
    NotMeasured,
}

/// One call of the hook: `event` is `pre` or `post`, `input` what the agent
/// passed on stdin.
pub fn handle(event: &str, agent: &str, input: &str, at: &Places) -> Result<Done, String> {
    if event != "pre" && event != "post" {
        return Err(format!("unknown hook event {event:?}: pre or post"));
    }
    // An agent with no format is not read at all: one agent's fields looked
    // for in another's call is the wrong record, quietly made.
    let Some(fmt) = format_for(&at.state_dir, agent) else {
        return unread(event, agent, input, at);
    };
    let raw = input;
    let input: Value = serde_json::from_str(raw).map_err(|e| format!("not JSON: {e}"))?;
    let tool = first_str(&input, &fmt.tool).unwrap_or("");
    if fmt.shell.iter().any(|s| s == tool) {
        return shell(event, agent, &fmt, &input, at);
    }
    let Some(path) = target_of(&input, &fmt) else {
        // A format read off a page keeps what it could not read, so the place
        // it is wrong can be seen rather than guessed at a second time.
        if !fmt.measured {
            return unread(event, agent, raw, at);
        }
        return Ok(Done::Skipped(Skip::NoPath));
    };
    if let Err(skip) = worth_recording(&path, at) {
        return Ok(Done::Skipped(skip));
    }
    let state_dir = at.state_dir.as_path();
    let session = first_str(&input, &fmt.session)
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
        note: Some(format!(
            "recorded by the {agent} hook, session {session}{}",
            howsure(&fmt)
        )),
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

/// The files a shell command names, as far as they can be read without
/// running it: absolute paths, `~/` and `$HOME/`, and relative ones against
/// the command's directory — the one it starts in, then the one each `cd` or
/// `pushd` in it moves to (`cd ~/.config/hypr && cp looknfeel.lua …` is how
/// agents write it). A word with any other variable, a glob or a command
/// substitution in it names nothing that can be known in advance.
pub fn named_paths(command: &str, cwd: Option<&Path>, home: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    let mut here: Option<PathBuf> = cwd.filter(|c| c.is_absolute()).map(Path::to_path_buf);
    let mut after_cd = false;
    let split = |c: char| c.is_whitespace() || ";|&<>()'\"`=,".contains(c);
    for word in command.split(split) {
        let w = word.trim();
        if w.is_empty() {
            continue;
        }
        let cd = std::mem::replace(&mut after_cd, w == "cd" || w == "pushd");
        if w.starts_with('-') || after_cd {
            continue;
        }
        let in_home = w
            .strip_prefix("$HOME/")
            .or_else(|| w.strip_prefix("${HOME}/"))
            .or_else(|| w.strip_prefix("~/"))
            .or_else(|| (w == "~" || w == "$HOME").then_some(""));
        if in_home
            .unwrap_or(w)
            .contains(['$', '*', '?', '[', '{', '\\'])
        {
            continue;
        }
        let w = in_home
            .map(|rest| home.join(rest).display().to_string())
            .unwrap_or_else(|| w.to_string());
        let p = PathBuf::from(&w);
        let p = if p.is_absolute() {
            p
        } else {
            match &here {
                Some(c) => c.join(&p),
                None => continue,
            }
        };
        if cd {
            // Where the rest of the command runs, if it is a directory.
            if p.is_dir() {
                here = Some(p);
            }
            continue;
        }
        // A bare word is a path only when there is a file by that name;
        // otherwise every argument would be one.
        if !w.contains('/') && !p.is_file() {
            continue;
        }
        // A file that is there, or one the command could create: its
        // directory is there.
        if !(p.is_file() || (!p.exists() && p.parent().is_some_and(|d| d.is_dir()))) {
            continue;
        }
        if !out.contains(&p) {
            out.push(p);
        }
        if out.len() >= SHELL_PATHS_MAX {
            break;
        }
    }
    out
}

/// What a shell command's `pre` kept, for its `post`.
#[derive(serde::Serialize, serde::Deserialize)]
struct Staged {
    path: PathBuf,
    /// The copy from before, when the file was there.
    copy: Option<PathBuf>,
    /// The record this session already has for the file.
    known: Option<String>,
}

fn staging_root(state_dir: &Path) -> PathBuf {
    state_dir.join("agent-shell")
}

/// One command's staging place: by the id the agent gives the tool call, or
/// by the session and the command when it gives none.
fn staging_dir(
    state_dir: &Path,
    fmt: &Format,
    input: &Value,
    session: &str,
    command: &str,
) -> PathBuf {
    let id = match first_str(input, &fmt.call_id) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("{session}\n{command}"),
    };
    let h = repair::sha256_hex(id.as_bytes());
    staging_root(state_dir).join(&h[..24])
}

/// A shell command, before and after it runs.
fn shell(
    event: &str,
    agent: &str,
    fmt: &Format,
    input: &Value,
    at: &Places,
) -> Result<Done, String> {
    let command = first_str(input, &fmt.command).unwrap_or("");
    let session = first_str(input, &fmt.session)
        .unwrap_or("no-session")
        .to_string();
    let state_dir = at.state_dir.as_path();
    let dir = staging_dir(state_dir, fmt, input, &session, command);
    let manifest = dir.join("staged.json");
    if event == "pre" {
        forget_stale_staging(state_dir);
        let cwd = first_str(input, &fmt.cwd).map(Path::new);
        let sessions = load_sessions(state_dir);
        let mut staged = vec![];
        for (n, path) in named_paths(command, cwd, &at.home).into_iter().enumerate() {
            if worth_recording(&path, at).is_err() {
                continue;
            }
            if let Some(id) = sessions.get(&key(&session, &path)) {
                staged.push(Staged {
                    path,
                    copy: None,
                    known: Some(id.clone()),
                });
                continue;
            }
            let copy = if path.is_file() {
                if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= SHELL_COPY_MAX) {
                    continue;
                }
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let c = dir.join(n.to_string());
                if std::fs::copy(&path, &c).is_err() {
                    // Not readable as the person: not theirs to record.
                    continue;
                }
                Some(c)
            } else {
                None
            };
            staged.push(Staged {
                path,
                copy,
                known: None,
            });
        }
        if staged.is_empty() {
            return Ok(Done::ShellWatching(0));
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string(&staged).map_err(|e| e.to_string())?;
        std::fs::write(&manifest, text).map_err(|e| e.to_string())?;
        return Ok(Done::ShellWatching(staged.len()));
    }
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return Ok(Done::ShellRecorded(vec![]));
    };
    let staged: Vec<Staged> = serde_json::from_str(&text).unwrap_or_default();
    let mut sessions = load_sessions(state_dir);
    let look = Lookups::offline();
    let mut ids = vec![];
    let mut result = Ok(());
    for s in staged {
        if !s.path.is_file() {
            continue;
        }
        if let Some(id) = s.known {
            if repair::refresh_digest(state_dir, &id).is_ok() {
                ids.push(id);
            }
            continue;
        }
        let ext = External {
            kind: "file".into(),
            by: agent.to_string(),
            path: Some(s.path.clone()),
            upstream: Upstream::default(),
            original_path: None,
            original_package: None,
            watch_issue: false,
            note: Some(format!(
                "recorded by the {agent} hook from a shell command, session {session}{}",
                howsure(fmt)
            )),
        };
        let made = match &s.copy {
            Some(copy) if repair::digest_path(copy) == repair::digest_path(&s.path) => continue,
            Some(copy) => repair::record_external_with_copy(state_dir, ext, copy, &look),
            None => repair::add_external(state_dir, ext, &look),
        };
        match made {
            Ok(rec) => {
                sessions.insert(key(&session, &s.path), rec.id.clone());
                ids.push(rec.id);
            }
            Err(e) => result = Err(e),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    if !ids.is_empty() {
        save_sessions(state_dir, &sessions)?;
    }
    result?;
    Ok(Done::ShellRecorded(ids))
}

/// Copies from commands whose `post` never came — the person said no to the
/// command, or the agent was stopped — are not kept past an hour: they are
/// copies of the person's files, and nothing will ever read them.
fn forget_stale_staging(state_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(staging_root(state_dir)) else {
        return;
    };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() > 3600);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

// ------------------------------------------------------------------ measuring an agent

// **An agent whose hook format nobody has walked is not guessed at in the
// dark.** The record knows Claude Code's input because it was measured on a
// machine; another agent's is either written down in `agent-formats.json` —
// from that agent's documentation, which is allowed and is marked as such —
// or it is nothing at all, and a call that is nothing at all records nothing.
//
// What such a call can do instead is be kept. While measuring is switched on,
// every call from an unknown agent, and every call a documented-but-unwalked
// format could not read, is written to `agent-samples/` exactly as it
// arrived. Somebody who knows where their agent's hook configuration lives —
// this program does not, and that is half of what is being measured — points
// it here, works as usual, and afterwards has the agent's real format on disk
// instead of a second guess about it.
//
// **The samples stay here.** They hold the paths and shell commands the agent
// used, which is somebody's machine written down. Nothing sends them, and the
// command that switches measuring on says so before the first one exists.

/// A sample longer than this is cut: a hook's input is a few hundred bytes,
/// and a file pasted into a command should not land here whole.
const SAMPLE_MAX: usize = 64 * 1024;

/// Measuring stops growing here. Twenty calls show a format; two hundred is
/// already generous, and a switch left on should not fill a disk.
const SAMPLES_MAX: usize = 200;

/// The agent being measured, and since when.
pub struct Measuring {
    pub agent: String,
    pub since: u64,
}

fn measure_marker(state_dir: &Path) -> PathBuf {
    state_dir.join("agent-measure.json")
}

/// Where the samples land. Every command that touches them names it: a
/// directory somebody is asked to read before sending has to be nameable.
pub fn samples_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("agent-samples")
}

/// Which agent is being measured, if any.
pub fn measuring(state_dir: &Path) -> Option<Measuring> {
    let text = std::fs::read_to_string(measure_marker(state_dir)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    Some(Measuring {
        agent: v.get("agent")?.as_str()?.to_string(),
        since: v.get("since").and_then(|n| n.as_u64()).unwrap_or(0),
    })
}

/// A name that can go into a file name and a hook command line, or none: the
/// rule an agent called headless is held to, kept here too.
pub fn plain_agent_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    let ok = !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| n.to_string())
        .ok_or_else(|| format!("{name:?} is not an agent's name: letters, digits, - and _"))
}

/// What a record says about the format that made it. Nothing, when it was
/// measured; when it was not, the record carries that as long as it exists.
fn howsure(fmt: &Format) -> String {
    if fmt.measured {
        return String::new();
    }
    let from = match &fmt.source {
        Some(src) => format!(", from {src}"),
        None => String::new(),
    };
    format!(" (this agent's hook format was read rather than measured{from})")
}

/// Begin measuring `agent`. Replaces an earlier one: two at once would leave
/// samples nobody can tell apart.
pub fn start_measuring(state_dir: &Path, agent: &str) -> Result<String, String> {
    let agent = plain_agent_name(agent)?;
    std::fs::create_dir_all(state_dir).map_err(|e| e.to_string())?;
    let body = serde_json::to_string_pretty(&json!({ "agent": agent, "since": now() }))
        .map_err(|e| e.to_string())?;
    std::fs::write(measure_marker(state_dir), body).map_err(|e| e.to_string())?;
    Ok(agent)
}

/// Stop. The samples already taken stay where they are: stopping is not
/// throwing away what was measured.
pub fn stop_measuring(state_dir: &Path) -> Result<bool, String> {
    let marker = measure_marker(state_dir);
    if !marker.is_file() {
        return Ok(false);
    }
    std::fs::remove_file(&marker)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// The samples taken so far, oldest first: their names carry the time.
pub fn samples(state_dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(samples_dir(state_dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    out.sort();
    out
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A call nothing here can read: kept if it is being measured, dropped if not.
fn unread(event: &str, agent: &str, input: &str, at: &Places) -> Result<Done, String> {
    match measuring(&at.state_dir) {
        Some(m) if m.agent == agent => keep_sample(&at.state_dir, agent, event, input),
        _ => Ok(Done::NotMeasured),
    }
}

/// Keep one call, as the agent sent it.
fn keep_sample(state_dir: &Path, agent: &str, event: &str, input: &str) -> Result<Done, String> {
    let taken = samples(state_dir).len();
    if taken >= SAMPLES_MAX {
        return Ok(Done::EnoughSamples(taken));
    }
    let agent = plain_agent_name(agent)?;
    let dir = samples_dir(state_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("{agent}-{at_ms:013}-{event}.json"));
    let body = if input.len() > SAMPLE_MAX {
        let cut = floor_char(input, SAMPLE_MAX);
        format!(
            "{}\n\n[cut here: the call was {} bytes]",
            &input[..cut],
            input.len()
        )
    } else {
        input.to_string()
    };
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(Done::Measured(path))
}

/// The largest cut at or below `n` that does not land inside a character.
fn floor_char(s: &str, n: usize) -> usize {
    let mut i = n.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
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
            home: base.join("home"),
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

    /// What an agent nobody here has walked might pass: other names, other
    /// nesting. Nothing in it is Claude Code's.
    fn other_call(path: &Path, session: &str) -> String {
        json!({
            "sessionID": session,
            "directory": "/",
            "tool": { "name": "edit" },
            "args": { "filePath": path.display().to_string() }
        })
        .to_string()
    }

    fn write_format(at: &Places, body: Value) {
        std::fs::create_dir_all(&at.state_dir).unwrap();
        std::fs::write(
            formats_path(&at.state_dir),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
    }

    /// RR29: an agent whose hook format nobody has walked records nothing at
    /// all — not a guess at Claude Code's fields in somebody else's call —
    /// and while it is being measured, every call is kept instead, up to the
    /// cap, with nothing recorded and nothing sent.
    #[test]
    fn an_unknown_agent_records_nothing_and_can_be_measured_instead() {
        let base = tmp("rr29");
        let at = places(&base);
        let conf = base.join("home/.config/app/app.conf");
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::write(&conf, "mode=old\n").unwrap();

        // Not measured, not written down: nothing happens, and nothing is
        // kept. Claude Code's own shape from another agent changes nothing.
        assert_eq!(
            handle("pre", "opencode", &other_call(&conf, "s1"), &at),
            Ok(Done::NotMeasured)
        );
        assert_eq!(
            handle("pre", "opencode", &call(&conf, "s1"), &at),
            Ok(Done::NotMeasured)
        );
        assert!(repair::load(&at.state_dir).is_empty());
        assert!(samples(&at.state_dir).is_empty());

        // Measuring: the call is kept exactly as it arrived, and still
        // nothing is recorded.
        start_measuring(&at.state_dir, "opencode").unwrap();
        let sent = other_call(&conf, "s1");
        let Ok(Done::Measured(kept)) = handle("pre", "opencode", &sent, &at) else {
            panic!("the call was not kept");
        };
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), sent);
        assert!(repair::load(&at.state_dir).is_empty(), "measuring records");

        // Another agent's calls are not this measurement's.
        assert_eq!(
            handle("pre", "cursor-agent", &other_call(&conf, "s1"), &at),
            Ok(Done::NotMeasured)
        );
        assert_eq!(samples(&at.state_dir).len(), 1);

        // Stopping keeps what was measured; a name that is not one is refused
        // before it reaches a file name.
        assert!(stop_measuring(&at.state_dir).unwrap());
        assert!(!stop_measuring(&at.state_dir).unwrap());
        assert_eq!(samples(&at.state_dir).len(), 1);
        assert!(start_measuring(&at.state_dir, "../sh").is_err());
        assert_eq!(
            handle("pre", "opencode", &sent, &at),
            Ok(Done::NotMeasured),
            "stopped means stopped"
        );
    }

    /// RR30: a format read from an agent's documentation rather than walked
    /// is taken, and says so in every record it makes; a call it cannot read
    /// is kept while measuring rather than passed over, so the place the
    /// reading is wrong can be seen. A file cannot call itself measured.
    #[test]
    fn a_format_that_was_read_rather_than_measured_says_so() {
        let base = tmp("rr30");
        let at = places(&base);
        let conf = base.join("home/.config/app/app.conf");
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::write(&conf, "mode=old\n").unwrap();
        write_format(
            &at,
            json!({ "formats": [{
                "agent": "opencode",
                "measured": true,
                "source": "opencode's plugin page",
                "tool": ["tool.name"],
                "shell": ["bash"],
                "path": ["args.filePath"],
                "command": ["args.command"],
                "cwd": ["directory"],
                "session": ["sessionID"],
                "call_id": ["callID"]
            }]}),
        );

        let fmt = format_for(&at.state_dir, "opencode").expect("the format was not read");
        assert!(!fmt.measured, "a file said of itself that it was measured");

        let Ok(Done::Began(id)) = handle("pre", "opencode", &other_call(&conf, "s1"), &at) else {
            panic!("the edit was not begun");
        };
        std::fs::write(&conf, "mode=new\n").unwrap();
        assert_eq!(
            handle("post", "opencode", &other_call(&conf, "s1"), &at),
            Ok(Done::Finished(id.clone()))
        );
        let rec = repair::load(&at.state_dir)
            .into_iter()
            .find(|r| r.id == id)
            .expect("no record");
        let why = rec.note.clone().unwrap_or_default();
        assert!(
            why.contains("read rather than measured") && why.contains("plugin page"),
            "the record does not say the format was not measured: {why}"
        );

        // A call this reading cannot make sense of: kept while measuring, so
        // the next version of the format comes from the machine.
        start_measuring(&at.state_dir, "opencode").unwrap();
        let strange = json!({ "event": "write", "file": "/etc/hosts" }).to_string();
        assert!(matches!(
            handle("pre", "opencode", &strange, &at),
            Ok(Done::Measured(_))
        ));
        assert_eq!(samples(&at.state_dir).len(), 1);
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

    /// What Claude Code passes a hook for a shell command.
    fn bash(command: &str, cwd: &Path, id: &str) -> String {
        json!({
            "session_id": "s1",
            "cwd": cwd.display().to_string(),
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_use_id": id,
            "tool_input": { "command": command }
        })
        .to_string()
    }

    /// RR28: a file an agent changes with a shell command is recorded with
    /// a copy from before the command; one it only reads is not; one it
    /// creates is recorded without a copy; a copy kept by hand next to the
    /// file is not a change of its own; a later command on the same file in
    /// the same session brings the same record up to date.
    #[test]
    fn an_agents_shell_command_is_recorded_like_its_file_writes() {
        let base = tmp("rr28");
        let at = places(&base);
        let hypr = base.join("home/.config/hypr");
        std::fs::create_dir_all(&hypr).unwrap();
        let conf = hypr.join("looknfeel.lua");
        std::fs::write(&conf, "-- defaults\n").unwrap();

        // Omarchy's way: a copy by hand, then an append.
        let cmd = "cp ~/.config/hypr/looknfeel.lua ~/.config/hypr/looknfeel.lua.bak.$(date +%s)\n\
                   cat >> ~/.config/hypr/looknfeel.lua <<'EOF'\ngaps_in = 3\nEOF\n\
                   cat ~/.config/hypr/hyprland.lua; hyprctl reload";
        assert_eq!(
            handle("pre", "claude", &bash(cmd, &base, "t1"), &at),
            Ok(Done::ShellWatching(2)),
            "the file it changes and the one it only reads"
        );
        std::fs::copy(&conf, hypr.join("looknfeel.lua.bak.1")).unwrap();
        std::fs::write(&conf, "-- defaults\ngaps_in = 3\n").unwrap();
        let Ok(Done::ShellRecorded(ids)) = handle("post", "claude", &bash(cmd, &base, "t1"), &at)
        else {
            panic!("the command was not recorded");
        };
        assert_eq!(ids.len(), 1, "{ids:?}");
        let all = repair::load(&at.state_dir);
        assert_eq!(all.len(), 1, "{all:?}");
        let rec = &all[0];
        assert_eq!(rec.state, "applied");
        assert!(rec.reversible);
        assert_eq!(
            std::fs::read_to_string(rec.backup.as_ref().unwrap()).unwrap(),
            "-- defaults\n",
            "the copy is not from before the command"
        );
        assert!(repair::review(&at.state_dir, &Lookups::offline()).is_empty());

        // Only reading it: nothing.
        let read = "cat ~/.config/hypr/looknfeel.lua";
        handle("pre", "claude", &bash(read, &base, "t0"), &at).unwrap();
        assert_eq!(
            handle("post", "claude", &bash(read, &base, "t0"), &at),
            Ok(Done::ShellRecorded(ids.clone())),
            "an unchanged file already recorded is only brought up to date"
        );
        let other = hypr.join("bindings.lua");
        std::fs::write(&other, "bind\n").unwrap();
        let look = "grep bind $HOME/.config/hypr/bindings.lua";
        handle("pre", "claude", &bash(look, &base, "t2"), &at).unwrap();
        assert_eq!(
            handle("post", "claude", &bash(look, &base, "t2"), &at),
            Ok(Done::ShellRecorded(vec![]))
        );

        // The same file again in the session: the same record, current.
        let again = "sed -i s/3/2/ ~/.config/hypr/looknfeel.lua";
        handle("pre", "claude", &bash(again, &base, "t3"), &at).unwrap();
        std::fs::write(&conf, "-- defaults\ngaps_in = 2\n").unwrap();
        assert_eq!(
            handle("post", "claude", &bash(again, &base, "t3"), &at),
            Ok(Done::ShellRecorded(ids.clone()))
        );
        assert_eq!(repair::load(&at.state_dir).len(), 1);
        assert!(repair::review(&at.state_dir, &Lookups::offline()).is_empty());

        // A file the command creates, named relative to where it runs.
        let new = "echo x > extra.conf";
        handle("pre", "claude", &bash(new, &hypr, "t4"), &at).unwrap();
        // A bare word is only a file when there is one: `extra.conf` is not
        // there yet, so it is not watched — the gap the module says.
        std::fs::write(hypr.join("extra.conf"), "x\n").unwrap();
        assert_eq!(
            handle("post", "claude", &bash(new, &hypr, "t4"), &at),
            Ok(Done::ShellRecorded(vec![]))
        );
        let made = "echo x > ./made.conf";
        handle("pre", "claude", &bash(made, &hypr, "t5"), &at).unwrap();
        std::fs::write(hypr.join("made.conf"), "x\n").unwrap();
        assert!(matches!(
            handle("post", "claude", &bash(made, &hypr, "t5"), &at),
            Ok(Done::ShellRecorded(v)) if v.len() == 1
        ));
        assert_eq!(repair::load(&at.state_dir).len(), 2);

        // Nothing left staged.
        let left = std::fs::read_dir(at.state_dir.join("agent-shell"))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(left, 0, "copies left behind");

        let home = base.join("home");
        // Claude's own spelling on Omarchy (take 7): into the directory
        // first, then bare names.
        let took = "cd ~/.config/hypr && cp looknfeel.lua looknfeel.lua.bak.$(date +%s) \
                    && cat >> looknfeel.lua <<'EOF'\ngaps_in = 3\nEOF\nhyprctl reload";
        assert_eq!(
            named_paths(took, Some(Path::new("/")), &home),
            vec![conf.clone()]
        );

        // What cannot be known in advance names nothing.
        assert!(named_paths("cat $XDG_CONFIG_HOME/x ~/.config/*/y", None, &home).is_empty());
        assert_eq!(
            named_paths("tee ~/.config/hypr/looknfeel.lua >/dev/null", None, &home),
            vec![conf.clone()],
            "a device is not a file"
        );
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
