//! The desktop's own agent, used as this client's model.
//!
//! Omarchy keeps the name of a default coding agent in
//! `~/.config/omarchy/defaults/agent` — just the name, not the model, endpoint
//! or key, because each agent keeps those itself. That is the whole appeal: on
//! a machine where somebody already set one up, PODSHL needs no model
//! configuration at all, no second API key, and no second place to get wrong.
//!
//! **Everything here was measured on 2026-09-14, and two plausible switches
//! turned out to be traps.** A file with a random marker was put somewhere and
//! Claude Code was asked to read it and print the line:
//!
//! | switch | outside the working directory | inside it |
//! |---|---|---|
//! | `--allowed-tools ""` | **read it** | — |
//! | `--permission-mode manual --permission-prompts none` | refused | **read it** |
//! | `--disallowed-tools "Read,Bash,Glob,Grep"` | refused | refused |
//!
//! An empty allow-list means *no restriction* rather than *nothing allowed*.
//! Default-deny turns out to be a **directory boundary** rather than a tool
//! fence, which is the one that would have shipped: it refuses convincingly
//! until the file happens to be under the working directory. Only the explicit
//! deny-list refuses in both places.
//!
//! Two more things the same runs showed, neither of them looked for:
//!
//! * **The agent goes round.** Unprompted, it handed the job to a
//!   general-purpose sub-agent and then an Explore sub-agent. The denial
//!   propagated — but the behaviour to design against is an agent that treats a
//!   fence as an obstacle.
//! * **The fence stops tool calls, not context.** With no file tool at all it
//!   still said *"Git status does show an untracked probe-marker.txt"*, because
//!   the harness puts the working directory's git status into the prompt. So it
//!   is started in an **empty directory of its own**, the same way `reads.rs`
//!   runs anything else.
//!
//! **Only Claude Code is here**, for the reason `forge.rs` gives for having
//! only GitHub: a switch nobody has run is a guess with a flag in it. Codex was
//! tried and does not fit — `--sandbox read-only` bounds what shell commands
//! may *write*, and reading files is not affected — so it is absent rather than
//! offered and hoped for.

use std::path::{Path, PathBuf};

/// Is this an Omarchy desktop at all?
///
/// Asked before anything else here, because everything here is about *this*
/// desktop's conventions. Without it the only guard is that
/// `~/.config/omarchy/defaults/agent` happens not to exist, which is a fact
/// about one file rather than a statement about the system — and on Windows or
/// macOS `dirs::config_dir()` would cheerfully look for that path under
/// `%APPDATA%` or `~/Library/Application Support`.
///
/// **The installation directory is asked first, and on purpose.**
/// `OMARCHY_PATH` is set in a login shell and is **not** in the systemd user
/// environment — measured — so a client started from a desktop entry or from
/// the bar does not inherit it, which is exactly how this feature would have
/// worked when tested from a terminal and vanished for everybody else. A
/// directory on disk survives an empty environment.
///
/// `DESKTOP_SESSION` is accepted as well because it *is* in the session
/// environment, so a session-only install without the shared directory still
/// answers yes.
#[cfg(target_os = "linux")]
pub fn is_omarchy() -> bool {
    if Path::new("/usr/share/omarchy").is_dir() {
        return true;
    }
    if dirs::data_dir()
        .map(|d| d.join("omarchy").is_dir())
        .unwrap_or(false)
    {
        return true;
    }
    if std::env::var_os("OMARCHY_PATH")
        .map(|p| Path::new(&p).is_dir())
        .unwrap_or(false)
    {
        return true;
    }
    [
        "DESKTOP_SESSION",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
    ]
    .iter()
    .filter_map(|k| std::env::var(k).ok())
    .any(|v| v.to_ascii_lowercase().contains("omarchy"))
}

/// Everywhere else this is not a question that has an answer.
#[cfg(not(target_os = "linux"))]
pub fn is_omarchy() -> bool {
    false
}

/// Where Omarchy records the agent a person chose.
fn default_agent_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("omarchy/defaults/agent"))
}

/// The agent this desktop is set up with, if it names one.
pub fn default_agent() -> Option<String> {
    if !is_omarchy() {
        return None;
    }
    let raw = std::fs::read_to_string(default_agent_path()?).ok()?;
    let name = raw.trim().to_string();
    // An agent name is a program name. Anything else is not one, and refusing
    // is better than passing it to a process spawn to find out.
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(name)
}

/// How an agent is asked a question with nothing else switched on.
///
/// The deny-list is written out rather than derived, and it is the fragile part
/// by construction: it names tools, and a tool added in a later version is a
/// hole that opens without a sound. `SV`-style coverage cannot fix that — only
/// re-running the measurement against a new version can, which is why the
/// version this was measured against is recorded beside it.
pub struct Headless {
    pub program: &'static str,
    pub args: Vec<String>,
    /// Whether the answer leaves this machine.
    pub cloud: bool,
    /// What it was measured against, so a later version is a question rather
    /// than an assumption.
    pub measured: &'static str,
}

pub fn headless(agent: &str, prompt: &str) -> Option<Headless> {
    match agent {
        "claude" => Some(Headless {
            program: "claude",
            args: vec![
                "-p".into(),
                prompt.into(),
                // Without it every call left its whole transcript, prompt
                // included and so every consented reading, under
                // ~/.claude/projects/: 29 of them on the first desktop, 76 KB
                // each. Measured with Claude Code 2.1.273: with it, only an
                // empty directory is left, which `forget_session` removes.
                "--no-session-persistence".into(),
                "--disallowed-tools".into(),
                // Every tool that reaches the machine or the network. Named in
                // full rather than by category: a category is a promise the CLI
                // never made.
                "Read,Write,Edit,NotebookEdit,Bash,BashOutput,KillShell,Glob,Grep,\
                 WebFetch,WebSearch,Task,Agent"
                    .into(),
            ],
            cloud: true,
            measured: "Claude Code, 2026-09-14",
        }),
        _ => None,
    }
}

/// Is this agent on the path?
pub fn installed(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|d| d.join(program).is_file())
}

/// What the settings panel may offer here, or `None`.
///
/// `None` covers every uninteresting case together — no Omarchy, no agent
/// chosen, an agent nobody has measured, an agent not installed — because the
/// answer to all of them is the same: do not offer it. An option that appears
/// and then fails is worse than one that never appears.
pub fn offer() -> Option<(String, bool)> {
    let agent = default_agent()?;
    let h = headless(&agent, "")?;
    if !installed(h.program) {
        return None;
    }
    Some((agent, h.cloud))
}

/// A directory with nothing in it, for the agent to be started from.
///
/// Not cosmetic: with every file tool denied, the agent still reported this
/// repository's git status, because the harness puts the working directory into
/// the prompt. Started from an empty directory it has nothing to report.
pub fn scratch() -> Option<PathBuf> {
    for _ in 0..3 {
        let nonce: u64 = rand::random();
        let dir =
            std::env::temp_dir().join(format!("podshl-agent-{}-{nonce:016x}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return Some(dir);
        }
    }
    None
}

/// Ask the desktop's agent, with nothing switched on.
pub async fn ask(agent: &str, prompt: &str) -> Result<String, String> {
    let h = headless(agent, prompt)
        .ok_or_else(|| format!("no measured way to call {agent:?} without its tools"))?;
    if !installed(h.program) {
        return Err(format!("{} is not installed", h.program));
    }
    let dir = scratch().ok_or_else(|| "no scratch directory".to_string())?;
    let out = tokio::process::Command::new(h.program)
        .args(&h.args)
        .current_dir(&dir)
        // Nothing inherited that says where this ran or who ran it.
        .env_remove("OMARCHY_PATH")
        .output()
        .await;
    let _ = std::fs::remove_dir_all(&dir);
    forget_session(&dir);
    let out = out.map_err(|e| format!("{}: {e}", h.program))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // A failure is where a newer version shows first, so it says which
        // version the switches were measured against.
        return Err(format!(
            "{} exited {} (switches measured against {}): {}",
            h.program,
            out.status,
            h.measured,
            err.chars().take(200).collect::<String>()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Where Claude Code files a session started in `cwd`: every character that is
/// not a letter or a digit becomes `-`, so `/tmp/podshl-agent-1-ab` is
/// `~/.claude/projects/-tmp-podshl-agent-1-ab`.
fn session_dir(home: &Path, cwd: &Path) -> PathBuf {
    let slug: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    home.join(".claude").join("projects").join(slug)
}

/// Remove what `--no-session-persistence` still leaves: a directory holding an
/// empty `memory/`. `remove_dir` refuses anything that is not empty, so a
/// directory the agent did write into stays exactly as it is.
fn forget_session(cwd: &Path) {
    if let Some(home) = dirs::home_dir() {
        forget_session_in(&home, cwd);
    }
}

fn forget_session_in(home: &Path, cwd: &Path) {
    let dir = session_dir(home, cwd);
    let _ = std::fs::remove_dir(dir.join("memory"));
    let _ = std::fs::remove_dir(&dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leftover_session_directory_is_found_and_only_removed_when_empty() {
        let home = std::env::temp_dir().join(format!("podshl-home-{:016x}", rand::random::<u64>()));
        let cwd = Path::new("/tmp/podshl-agent-42-00ab");
        let dir = session_dir(&home, cwd);
        assert!(
            dir.ends_with(".claude/projects/-tmp-podshl-agent-42-00ab"),
            "{dir:?}"
        );

        // What the flag leaves: nothing but an empty memory/. Gone afterwards.
        std::fs::create_dir_all(dir.join("memory")).unwrap();
        forget_session_in(&home, cwd);
        assert!(!dir.exists(), "the empty shell was left behind");

        // Something the agent did write is left alone.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("session.jsonl"), "{}").unwrap();
        forget_session_in(&home, cwd);
        assert!(
            dir.join("session.jsonl").exists(),
            "a directory with content was removed"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    /// The real call, on a desktop that has the agent, and what it leaves.
    /// Ignored because it spends a completion on somebody's account:
    /// `cargo test -- --ignored asking_the_agent_leaves_no_session_behind`.
    #[tokio::test]
    #[ignore]
    async fn asking_the_agent_leaves_no_session_behind() {
        let agent = default_agent().expect("not an Omarchy desktop with a default agent");
        let answer = ask(&agent, "Answer with the single word: ready.")
            .await
            .unwrap();
        assert!(answer.to_lowercase().contains("ready"), "{answer}");

        let prefix = format!("-tmp-podshl-agent-{}-", std::process::id());
        let projects = dirs::home_dir().unwrap().join(".claude/projects");
        let left: Vec<_> = std::fs::read_dir(&projects)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&prefix))
            .collect();
        assert!(left.is_empty(), "left behind under {projects:?}: {left:?}");
    }

    #[test]
    fn an_agent_name_is_a_program_name_or_it_is_refused() {
        // The file is read and then handed to a process spawn. Anything that
        // needs quoting to be safe is refused instead of quoted.
        for bad in ["", "  ", "a b", "../../bin/sh", "claude; rm -rf /", "a/b"] {
            let ok = !bad.trim().is_empty()
                && bad.trim().len() <= 64
                && bad
                    .trim()
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            assert!(!ok, "{bad:?} would have been accepted as an agent name");
        }
        for good in ["claude", "opencode", "cursor-agent", "my_agent"] {
            assert!(
                good.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{good:?} was refused"
            );
        }
    }

    /// The fence, written down where it can be read rather than left in a
    /// commit message.
    /// The gate, and why it is the installation directory rather than the
    /// environment variable that reads most naturally.
    #[test]
    fn the_agent_is_only_looked_for_on_an_omarchy_desktop() {
        // Measured: OMARCHY_PATH is set in a login shell and absent from the
        // systemd user environment, so a client started from a desktop entry or
        // from the bar — which is how anybody but a developer starts it — does
        // not inherit it. A feature gated on it alone would have worked in every
        // terminal test and for nobody else.
        assert!(
            cfg!(target_os = "linux") || !is_omarchy(),
            "this is a Linux desktop's convention and must not be looked for elsewhere"
        );

        // And the gate is asked before the file is read, so a stray
        // `omarchy/defaults/agent` under somebody's Windows AppData is not an
        // Omarchy desktop.
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/omarchy.rs"),
        )
        .unwrap();
        let body = src
            .split("pub fn default_agent()")
            .nth(1)
            .expect("default_agent is gone");
        let gate = body
            .find("is_omarchy()")
            .expect("default_agent does not ask whether this is Omarchy");
        let read = body
            .find("read_to_string")
            .expect("default_agent stopped reading the file");
        assert!(
            gate < read,
            "the file is read before the desktop is identified"
        );
    }

    #[test]
    fn the_only_measured_agent_is_called_with_its_tools_denied() {
        let h = headless("claude", "hello").expect("claude is the one measured agent");
        assert_eq!(h.program, "claude");
        assert!(
            h.args.iter().any(|a| a == "-p"),
            "not asked non-interactively"
        );
        let deny = h
            .args
            .iter()
            .position(|a| a == "--disallowed-tools")
            .map(|i| h.args[i + 1].clone())
            .expect("no deny-list, which is the only switch measured to fence");
        for tool in ["Read", "Bash", "Glob", "Grep", "WebFetch", "Task"] {
            assert!(deny.contains(tool), "{tool} is not denied: {deny}");
        }
        assert!(
            !h.args.iter().any(|a| a == "--allowed-tools"),
            "an empty allow-list reads as a fence and is not one — measured"
        );
        assert!(
            !h.args.iter().any(|a| a == "--permission-mode"),
            "default-deny is a directory boundary, not a tool fence — measured"
        );
        assert!(
            h.cloud,
            "Claude Code sends the prompt off this machine and must say so"
        );
        assert!(
            h.args.iter().any(|a| a == "--no-session-persistence"),
            "every call would leave its transcript, readings included, under ~/.claude"
        );

        assert!(
            headless("opencode", "x").is_none(),
            "an agent nobody measured is offered"
        );
        assert!(
            headless("codex", "x").is_none(),
            "codex's read-only sandbox bounds writes, not reads — measured, and absent"
        );
    }
}
