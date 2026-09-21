//! `podshl-client repairs …`: the repair record without the window.
//!
//! Two kinds of caller. **A tool that changed something** — an agent, a skill,
//! a script, a person — registers the change (`add`, or `begin` and `done`
//! around it, which also keeps a copy to go back to). **Something that runs
//! after an update** asks for a review (`review`), and `install-hook` puts that
//! in the place each system runs things after an update or once a day:
//!
//! | system | where |
//! |---|---|
//! | Omarchy | `~/.config/omarchy/hooks/post-update.d/podshl-repairs` |
//! | other Linux | a systemd user timer, daily |
//! | macOS | a launchd agent, daily and at login |
//! | Windows | a scheduled task, daily |
//!
//! `review` reads the machine and prints what wants another look. It changes
//! nothing on the machine, needs no model, and asks the network only about an
//! upstream issue the person chose to watch — `--offline` asks nobody.

use crate::repair::{self, External, Lookups, Review, Upstream};
use serde_json::json;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// `review` found something to look at. Distinct from failure (1), so a hook
/// can tell "nothing to do" from "look" from "broken".
pub const EXIT_LOOK: i32 = 3;

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
pub fn exit_quietly_on_a_closed_pipe() {
    // SAFETY: setting a signal disposition to the system default, before any
    // thread is spawned and before anything is written.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
pub fn exit_quietly_on_a_closed_pipe() {}

/// Whether this is `podshl-repairs` rather than `podshl-client repairs`.
///
/// It decides one thing: what the notification tells a person to run. It
/// said "Open PODSHL, or run: podshl-client repairs review" whoever raised
/// it, and somebody with only `podshl-repairs` has neither a window to open
/// nor that program.
static STANDALONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called once by `podshl-repairs`, before anything else.
pub fn run_as_standalone() {
    STANDALONE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn standalone() -> bool {
    STANDALONE.load(std::sync::atomic::Ordering::Relaxed)
}

/// `repairs …` as a whole program: run it and exit with its code. The same for
/// `podshl-client repairs` and for `podshl-repairs`, so the two cannot drift.
pub fn exit_with(args: Vec<String>) -> ! {
    exit_quietly_on_a_closed_pipe();
    match run(args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

pub fn state_root() -> PathBuf {
    std::env::var("VS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::config_dir()
                .map(|p| p.join("podshl"))
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

const USAGE: &str = "\
podshl-client repairs <command>

  --version, -V
        the version
  review [--json] [--notify] [--offline]
        what wants another look; exit 0 nothing, 3 something, 1 error
  list [--json]
        every record
  show ID [--json]
        everything about one record: why, who declared it, upstream, the copy
  add --kind file|package|overlay --by NAME [--path P] [--package NAME]
      [--original P] [--original-package NAME] [--issue URL] [--fixed-in V]
      [--watch] [--note TEXT]
        record a change that has already been made
  begin --kind file --by NAME --path P [same options]
        before changing a file: keep a copy, print the record id
  done ID
        the change announced by `begin` is made
  keep ID
        looked at, keep it; not raised again until something changes
  watch ID on|off
        ask GitHub about the record's issue (each lookup tells GitHub which
        issue this machine follows)
  restore ID
        put back the copy `begin` kept
  forget ID
        remove what a record said; needs administrator rights and the id typed
        back here. A stub stays, saying a record existed and was removed
  install-hook [--print]
  remove-hook [--print]
        run `review --notify` after updates (Omarchy) or daily
  install-agent-hook [--agent NAME] [--print] [--guessed]
  remove-agent-hook [--agent NAME] [--print]
        record what a coding agent writes outside git repositories, through
        the agent's own hooks; on Omarchy the default agent unless named.
        Measured for: claude. Another agent's format can be written into
        agent-formats.json and taken with --guessed, which says in every
        record it makes that it was read rather than measured
  measure-agent-hook [--agent NAME] [--stop]
        an agent whose format is not known: keep what its hooks send here and
        record nothing, so the format can be walked rather than guessed twice.
        Without either flag: what is being measured, and where it is kept";

/// The usage, under the name of the program that is running: `podshl-repairs
/// help` began with "podshl-client repairs <command>", which is the other
/// program, and the one somebody with only the record does not have.
fn usage() -> String {
    if standalone() {
        USAGE.replacen(
            "podshl-client repairs <command>",
            "podshl-repairs <command>",
            1,
        )
    } else {
        USAGE.to_string()
    }
}

struct Args {
    words: Vec<String>,
}

impl Args {
    fn flag(&mut self, name: &str) -> bool {
        match self.words.iter().position(|w| w == name) {
            Some(i) => {
                self.words.remove(i);
                true
            }
            None => false,
        }
    }
    fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        match self.words.iter().position(|w| w == name) {
            Some(i) if i + 1 < self.words.len() => {
                self.words.remove(i);
                Ok(Some(self.words.remove(i)))
            }
            Some(_) => Err(format!("{name} needs a value")),
            None => Ok(None),
        }
    }
    fn positional(&mut self) -> Option<String> {
        (!self.words.is_empty()).then(|| self.words.remove(0))
    }
    fn done(&self) -> Result<(), String> {
        match self.words.first() {
            Some(w) => Err(format!("unexpected argument {w:?}\n\n{}", usage())),
            None => Ok(()),
        }
    }
}

/// **The one place this feature leaves the machine**, so it is the one place
/// that has to be in the log. Watching is off unless somebody switched it on
/// per record, and each lookup tells GitHub which issue this computer follows
/// — a person told that in a panel should be able to see afterwards that it
/// happened, and when, without having been at the window. The hook runs this
/// unattended after every update, where nobody is watching at all.
fn live_issue(url: &str) -> repair::IssueState {
    let got = crate::upstream::check(url, &crate::upstream::github_get);
    let said = match (&got.error, &got.released_in) {
        (Some(e), _) => format!("could not be asked: {e}"),
        (None, Some(tag)) => format!("{}, released in {tag}", got.state),
        (None, None) => got.state.clone(),
    };
    crate::clientlog::line(&format!("upstream asked about {url}: {said}"));
    got
}

/// The machine's answers, and GitHub's for issues the person chose to watch.
pub fn live_lookups(offline: bool) -> Lookups<'static> {
    let mut l = Lookups::offline();
    if !offline {
        l.issue = Some(&live_issue);
    }
    l
}

/// What installed packages declare, into the record before it is shown —
/// see `declared.rs`. Each thing it did, or could not do, is a line in the
/// log: a maintainer whose declaration is refused finds out why there.
pub fn take_declarations(root: &Path) {
    let look = repair::Lookups::offline();
    match crate::declared::sync(root, &crate::declared::Sources::live(), &look) {
        Ok(said) => {
            for line in said {
                crate::clientlog::line(&format!("repairs declared: {line}"));
            }
        }
        Err(e) => crate::clientlog::line(&format!("repairs declared: not taken: {e}")),
    }
}

/// One line per flag, in the words the window uses.
pub fn describe(r: &Review) -> Vec<String> {
    use repair::Flag::*;
    r.flags
        .iter()
        .map(|f| match f {
            Updated { from, to } => m!("repair_cli_updated", from = from, to = to),
            UpstreamSaysFixed {
                fixed_in,
                installed,
            } => m!("repair_cli_fixed", f = fixed_in, i = installed),
            CannotCompare { why } => why.clone(),
            Overwritten => m!("repair_cli_overwritten"),
            NoLongerSet => m!("repair_cli_no_longer_set"),
            TargetGone => m!("repair_cli_target_gone"),
            BackupGone => m!("repair_cli_backup_gone"),
            FileChanged => m!("repair_cli_file_changed"),
            Frozen {
                installed,
                available,
            } => m!("repair_cli_frozen", i = installed, a = available),
            OriginalChanged => m!("repair_cli_original_changed"),
            IssueClosed => m!("repair_cli_issue_closed"),
            PrMerged => m!("repair_cli_pr_merged"),
            ReleasedIn { tag } => m!("repair_cli_released_in", t = tag),
            Retired { reason } if reason.is_empty() => m!("repair_cli_retired_silent"),
            Retired { reason } => m!("repair_cli_retired", r = reason),
            DeclarerGone => m!("repair_cli_declarer_gone"),
        })
        .collect()
}

/// Whether this process is running as root, or as an administrator.
///
/// **Not a file-permission question.** `repairs.json` is in the person's own
/// configuration directory and they can write it without any of this. The
/// point is who *cannot*: the agents, skills and scripts this record exists to
/// keep track of run as the person too, so a removal an ordinary process can
/// perform is a removal the thing being recorded can perform. Asking for the
/// privilege puts a prompt the person sees between a tool and the evidence.
fn running_privileged() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: `geteuid` takes nothing, returns a value, and cannot fail.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut raised = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut raised as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            CloseHandle(token);
            ok != 0 && raised.TokenIsElevated != 0
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// The two gates on removing a record that do not depend on what is typed.
///
/// A decision of its own so that every combination can be tested: whether it
/// refuses correctly cannot be checked by running the command, because the
/// answer depends on how the machine running the suite happens to be set up —
/// the container is root and a developer's terminal is not.
///
/// There is no flag that skips either of them. One was written while this was
/// being built, to make the success path easy to test, and it was exactly the
/// hole the gates exist to close: a tool running as root could have passed it.
fn may_forget(privileged: bool, interactive: bool) -> Result<(), String> {
    if !privileged {
        return Err(m!("repair_forget_needs_root"));
    }
    if !interactive {
        return Err(m!("repair_forget_needs_a_person"));
    }
    Ok(())
}

/// A flag's own name, for the log: `file_changed`, `released_in`.
///
/// Not the sentence `describe` builds. That one is translated, and a log is
/// read long after the fact and often by somebody who did not set the window's
/// language — so it says the name the JSON says, which does not move.
fn flag_name(f: &repair::Flag) -> String {
    serde_json::to_value(f)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(String::from))
        .unwrap_or_else(|| "flag".into())
}

fn title(r: &repair::Record) -> String {
    let what = r
        .target
        .as_deref()
        .map(|t| crate::reads::display_path(Path::new(t)))
        .or_else(|| r.upstream.package.clone())
        .unwrap_or_else(|| r.action.clone());
    format!("[{}] {} — {} ({})", r.id, r.kind, what, r.subject)
}

/// A date, from seconds since the epoch, as YYYY-MM-DD (UTC) — without
/// pulling in a date library for one line.
fn epoch_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    // Howard Hinnant's days_from_civil, inverted.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Everything about one record a person needs a year later: what, why, who
/// said so, what upstream has to do, and whether there is a way back.
fn show_lines(r: &repair::Record) -> Vec<String> {
    let mut out = vec![title(r), m!("repair_cli_show_state", s = &r.state)];
    if let Some(n) = r.note.as_deref().filter(|n| !n.trim().is_empty()) {
        out.push(m!("repair_cli_show_why", v = n));
    }
    if let Some(d) = &r.declared {
        let who = d
            .package
            .clone()
            .unwrap_or_else(|| crate::reads::display_path(Path::new(&d.file)));
        out.push(m!("repair_cli_show_declared", v = who));
        if d.gone {
            out.push(format!("  {}", m!("repair_cli_declarer_gone")));
        } else if let Some(why) = &d.retired {
            out.push(format!(
                "  {}",
                if why.is_empty() {
                    m!("repair_cli_retired_silent")
                } else {
                    m!("repair_cli_retired", r = why)
                }
            ));
        }
    }
    if let Some(issue) = &r.upstream.issue {
        out.push(m!("repair_cli_show_issue", v = issue));
        out.push(if r.watch_issue {
            m!("repair_cli_show_watched")
        } else {
            m!("repair_cli_show_not_watched", id = &r.id)
        });
    }
    if let Some(f) = &r.upstream.fixed_in {
        out.push(m!("repair_cli_show_fixed_in", v = f));
    }
    if let Some(i) = &r.installed {
        out.push(m!("repair_cli_show_installed", v = &i.version));
    }
    match r.backup.as_deref().filter(|_| r.reversible) {
        Some(b) => out.push(m!(
            "repair_cli_show_copy",
            v = crate::reads::display_path(Path::new(b))
        )),
        None => out.push(m!("repair_cli_show_no_copy")),
    }
    out.push(m!("repair_cli_show_when", v = epoch_date(r.at)));
    out
}

pub fn run(words: Vec<String>) -> Result<i32, String> {
    let mut a = Args { words };
    let cmd = a.positional().unwrap_or_else(|| "help".into());
    // `<command> --help` asks for help, as with Omarchy's own commands
    // (`omarchy plugin add --help`), rather than being refused as an argument
    // the command does not know — and it never runs the command.
    if a.words.iter().any(|w| w == "--help" || w == "-h") {
        println!("{}", usage());
        return Ok(0);
    }
    let root = state_root();
    match cmd.as_str() {
        "review" => {
            let json_out = a.flag("--json");
            let notify_out = a.flag("--notify");
            let offline = a.flag("--offline");
            a.done()?;
            take_declarations(&root);
            let found = repair::review(&root, &live_lookups(offline));
            // What the scheduled run concluded. Without it the only trace of a
            // review that happened while nobody was looking is an exit code
            // that `|| true` in the hook throws away.
            crate::clientlog::line(&format!(
                "repairs review{}: {} recorded, {} want another look{}",
                if offline { " (offline)" } else { "" },
                repair::load(&root).len(),
                found.len(),
                if found.is_empty() {
                    String::new()
                } else {
                    format!(
                        ": {}",
                        found
                            .iter()
                            .map(|r| format!(
                                "{} [{}]",
                                r.record.id,
                                r.flags.iter().map(flag_name).collect::<Vec<_>>().join(", ")
                            ))
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                }
            ));
            if json_out {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&found).map_err(|e| e.to_string())?
                );
            } else if found.is_empty() {
                println!("{}", m!("repair_cli_nothing"));
            } else {
                // Run from a hook, this lands in the middle of somebody else's
                // output: `omarchy-update` runs its hooks between the package
                // upgrade and the AUR and mise steps, and one bare line of ours
                // scrolled away under hundreds of theirs. A blank line and a
                // heading make it findable.
                if notify_out {
                    println!(
                        "
{}",
                        m!("repair_cli_heading")
                    );
                }
                for r in &found {
                    println!("{}", title(&r.record));
                    for line in describe(r) {
                        println!("  - {line}");
                    }
                }
            }
            if notify_out && !found.is_empty() {
                notify(&notice(found.len()));
            }
            Ok(if found.is_empty() { 0 } else { EXIT_LOOK })
        }
        "list" => {
            let json_out = a.flag("--json");
            a.done()?;
            take_declarations(&root);
            let all = repair::load(&root);
            if json_out {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?
                );
            } else {
                for r in &all {
                    // A stub has no target and no subject to build a title
                    // from; it is still a line, because that a record was
                    // removed is part of what the ledger says.
                    if r.state == "forgotten" {
                        println!("[{}] {} — {}", r.id, r.kind, m!("repair_cli_forgotten_row"));
                    } else {
                        println!("{} — {}", title(r), r.state);
                    }
                }
            }
            Ok(0)
        }
        "add" | "begin" => {
            let ext = External {
                kind: a.value("--kind")?.unwrap_or_default(),
                by: a.value("--by")?.unwrap_or_default(),
                path: a.value("--path")?.map(PathBuf::from),
                upstream: {
                    let mut up = serde_json::Map::new();
                    for (flag, key) in [
                        ("--package", "package"),
                        ("--issue", "issue"),
                        ("--fixed-in", "fixed_in"),
                    ] {
                        if let Some(v) = a.value(flag)? {
                            up.insert(key.into(), json!(v));
                        }
                    }
                    Upstream::from_value(Some(&serde_json::Value::Object(up)))?
                },
                original_path: a.value("--original")?.map(PathBuf::from),
                original_package: a.value("--original-package")?,
                watch_issue: a.flag("--watch"),
                note: a.value("--note")?,
            };
            a.done()?;
            let look = Lookups::offline();
            let rec = if cmd == "add" {
                repair::add_external(&root, ext, &look)?
            } else {
                repair::begin_external(&root, ext, &look)?
            };
            crate::clientlog::line(&format!(
                "repairs {cmd} {}: {} {} by {}{}",
                rec.id,
                rec.kind,
                rec.target
                    .as_deref()
                    .or(rec.upstream.package.as_deref())
                    .unwrap_or("?"),
                rec.subject,
                if rec.watch_issue {
                    ", watching the upstream issue"
                } else {
                    ""
                }
            ));
            println!("{}", rec.id);
            Ok(0)
        }
        "show" => {
            let json_out = a.flag("--json");
            let id = a
                .positional()
                .ok_or_else(|| "show needs a record id".to_string())?;
            a.done()?;
            take_declarations(&root);
            let all = repair::load(&root);
            let r = all
                .iter()
                .find(|r| r.id == id)
                .ok_or_else(|| m!("repair_unknown"))?;
            if json_out {
                println!(
                    "{}",
                    serde_json::to_string_pretty(r).map_err(|e| e.to_string())?
                );
            } else {
                for line in show_lines(r) {
                    println!("{line}");
                }
            }
            Ok(0)
        }
        "done" | "keep" | "restore" => {
            let id = a
                .positional()
                .ok_or_else(|| format!("{cmd} needs a record id"))?;
            a.done()?;
            match cmd.as_str() {
                "done" => {
                    repair::finish_external(&root, &id)?;
                    crate::clientlog::line(&format!("repairs done {id}"));
                }
                "keep" => {
                    repair::looked_at(&root, &id, &Lookups::offline())?;
                    crate::clientlog::line(&format!("repairs keep {id}"));
                }
                _ => {
                    // `restore` writes over a file on disk. Saying nothing left
                    // the one command that changes something indistinguishable
                    // from the two that only keep the record straight.
                    let r = repair::restore_external(&root, &id)?;
                    crate::clientlog::line(&format!(
                        "repairs restore {id}: {}",
                        r.target.as_deref().unwrap_or("?")
                    ));
                    if let Some(t) = r.target.as_deref() {
                        println!(
                            "{}",
                            m!(
                                "repair_cli_restored",
                                p = crate::reads::display_path(Path::new(t))
                            )
                        );
                    }
                }
            }
            Ok(0)
        }
        "forget" => {
            let id = a
                .positional()
                .ok_or_else(|| "forget needs a record id".to_string())?;
            a.done()?;

            // **Three gates, and each one is a different thing going wrong.**
            //
            // The privilege, because the tools this ledger records run as the
            // person and must not be able to erase it quietly. The terminal,
            // because a script that found a way to run privileged still cannot
            // answer a question. And the id typed back, because the person has
            // to have read which record they are removing — `forget` takes an
            // id, and ids are easy to paste wrongly.
            may_forget(running_privileged(), std::io::stdin().is_terminal())?;
            let all = repair::load(&root);
            let rec = all
                .iter()
                .find(|r| r.id == id && r.state != "forgotten")
                .ok_or_else(|| m!("repair_unknown"))?;
            println!("{}", title(rec));
            if let Some(t) = rec.target.as_deref() {
                println!("  {}", crate::reads::display_path(Path::new(t)));
            }
            println!("{}", m!("repair_forget_what_goes"));

            print!("{} ", m!("repair_forget_confirm", id = &id));
            let _ = std::io::stdout().flush();
            let mut typed = String::new();
            std::io::stdin()
                .lock()
                .read_line(&mut typed)
                .map_err(|e| e.to_string())?;
            if typed.trim() != id {
                return Err(m!("repair_forget_not_confirmed"));
            }

            let was = repair::forget(&root, &id)?;
            // Logged before it is announced, and logged whatever else happens:
            // the ledger no longer says what this was, so the log is the only
            // place left that says a record was removed at all.
            crate::clientlog::line(&format!(
                "repairs forget {id}: {} {} by {} — removed, a stub remains",
                was.kind,
                was.target
                    .as_deref()
                    .or(was.upstream.package.as_deref())
                    .unwrap_or("?"),
                was.subject
            ));
            println!("{}", m!("repair_forgotten", id = &id));
            Ok(0)
        }
        "watch" => {
            let id = a
                .positional()
                .ok_or_else(|| "watch needs a record id".to_string())?;
            let on = match a.positional().as_deref() {
                Some("on") => true,
                Some("off") => false,
                _ => return Err("watch ID on|off".into()),
            };
            a.done()?;
            repair::set_watch(&root, &id, on)?;
            // Switching it on is consent to ask GitHub, and that belongs in
            // the log beside the asking.
            crate::clientlog::line(&format!(
                "repairs watch {id} {}",
                if on { "on" } else { "off" }
            ));
            Ok(0)
        }
        "install-hook" | "remove-hook" => {
            let print_only = a.flag("--print");
            a.done()?;
            let plan = hook_plan(&installed_exe()?, cmd == "install-hook")?;
            for step in &plan {
                println!("{}", step.describe());
            }
            if !print_only {
                for step in &plan {
                    step.apply()?;
                    crate::clientlog::line(&format!("repairs {cmd}: {}", step.describe()));
                }
            }
            Ok(0)
        }
        "agent-hook" => {
            // Called by the agent, on every file write and shell command, with the agent
            // waiting. Whatever happens here, the answer is 0: a record
            // missed is a gap, an agent stopped by a record is a removed tool.
            let event = a.positional().unwrap_or_default();
            let agent = a
                .value("--agent")
                .ok()
                .flatten()
                .unwrap_or_else(|| "claude".into());
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            let Some(home) = dirs::home_dir() else {
                eprintln!("podshl agent hook: no home directory");
                return Ok(0);
            };
            let at = crate::agent_hook::Places::live(&root, &home);
            match crate::agent_hook::handle(&event, &agent, &input, &at) {
                Ok(crate::agent_hook::Done::Began(id)) => {
                    crate::clientlog::line(&format!("repairs agent-hook {agent}: began {id}"))
                }
                Ok(crate::agent_hook::Done::Added(id)) => {
                    crate::clientlog::line(&format!("repairs agent-hook {agent}: added {id}"))
                }
                Ok(crate::agent_hook::Done::Finished(id)) => {
                    crate::clientlog::line(&format!("repairs agent-hook {agent}: finished {id}"))
                }
                Ok(crate::agent_hook::Done::ShellRecorded(ids)) if !ids.is_empty() => {
                    crate::clientlog::line(&format!(
                        "repairs agent-hook {agent}: shell command, {}",
                        ids.join(" ")
                    ))
                }
                Ok(crate::agent_hook::Done::Measured(p)) => crate::clientlog::line(&format!(
                    "repairs agent-hook {agent}: kept a sample, {}",
                    p.display()
                )),
                Ok(_) => {}
                Err(e) => eprintln!("podshl agent hook: {e}"),
            }
            Ok(0)
        }
        "measure-agent-hook" => {
            let stop = a.flag("--stop");
            let named = a.value("--agent")?;
            a.done()?;
            let samples = crate::agent_hook::samples_dir(&root);
            if stop {
                let was = crate::agent_hook::measuring(&root).map(|m| m.agent);
                crate::agent_hook::stop_measuring(&root)?;
                match was {
                    Some(agent) => {
                        crate::clientlog::line(&format!(
                            "repairs measure-agent-hook: stopped {agent}"
                        ));
                        println!("no longer measuring {agent}");
                    }
                    None => println!("nothing was being measured"),
                }
                println!("{} kept in {}", calls(&root), samples.display());
                println!("take out the hook entries you added to the agent yourself");
                return Ok(0);
            }
            let Some(named) = named else {
                match crate::agent_hook::measuring(&root) {
                    Some(m) => println!(
                        "measuring {} since {}; {} kept in {}",
                        m.agent,
                        epoch_date(m.since),
                        calls(&root),
                        samples.display()
                    ),
                    None => {
                        println!("not measuring any agent");
                        println!();
                        println!("start:  {} measure-agent-hook --agent NAME", me());
                    }
                }
                let known = crate::agent_hook::load_formats(&root);
                println!();
                println!("hook formats known here:");
                println!("  claude (measured, built in)");
                for f in &known {
                    if f.agent != "claude" {
                        println!(
                            "  {} (read, not measured{})",
                            f.agent,
                            f.source
                                .as_deref()
                                .map(|s| format!(", from {s}"))
                                .unwrap_or_default()
                        );
                    }
                }
                println!(
                    "  from {}",
                    crate::agent_hook::formats_path(&root).display()
                );
                return Ok(0);
            };
            let agent = crate::agent_hook::start_measuring(&root, &named)?;
            crate::clientlog::line(&format!("repairs measure-agent-hook: measuring {agent}"));
            let exe = installed_exe()?;
            let pre = crate::agent_hook::hook_command(&exe, !standalone(), "pre", &agent)?;
            let post = crate::agent_hook::hook_command(&exe, !standalone(), "post", &agent)?;
            println!("measuring {agent}. The two commands its hooks should run:");
            println!("  before a file write or a shell command:");
            println!("    {pre}");
            println!("  after it:");
            println!("    {post}");
            println!();
            println!("Put those into {agent}'s own hook configuration. This program does not");
            println!("know where that is — which is half of what is being measured. Then use");
            println!("the agent as you normally would.");
            println!();
            println!("Nothing is recorded while measuring: the format is not known yet, and");
            println!("guessing at it is what this avoids. Each call is kept as it arrived in");
            println!("  {}", samples.display());
            println!("Those files hold the paths and the shell commands your agent used.");
            println!("Nothing sends them anywhere. Read them before you send them to anybody.");
            println!();
            println!("Stop: {} measure-agent-hook --stop", me());
            Ok(0)
        }
        "install-agent-hook" | "remove-agent-hook" => {
            let print_only = a.flag("--print");
            let guessed = a.flag("--guessed");
            let named = a.value("--agent")?;
            a.done()?;
            let install = cmd == "install-agent-hook";
            let agent = match named {
                Some(n) => n,
                None => omarchy_default_agent().ok_or(
                    "name the agent with --agent: only on Omarchy is there a default to take",
                )?,
            };
            let fmt = crate::agent_hook::format_for(&root, &agent);
            match &fmt {
                // Written down but not walked: taken only when the person
                // says so in the command, because every record it makes will
                // carry that it was read rather than measured.
                Some(f) if !f.measured && !guessed => {
                    return Err(unmeasured_advice(&agent, true));
                }
                Some(_) => {}
                None => return Err(unmeasured_advice(&agent, false)),
            }
            // Only Claude Code's settings file is a place this program knows.
            // For any other agent the two commands are printed and put in by
            // hand: writing a file whose shape nobody here has seen is how a
            // tool destroys somebody's configuration.
            if agent != "claude" {
                let exe = installed_exe()?;
                let pre = crate::agent_hook::hook_command(&exe, !standalone(), "pre", &agent)?;
                let post = crate::agent_hook::hook_command(&exe, !standalone(), "post", &agent)?;
                if install {
                    println!("{agent}'s hook format is read, not measured: every record it makes says so.");
                    println!(
                        "Where {agent} keeps its hooks is not known here, so put these in by hand:"
                    );
                    println!("  before a file write or a shell command:");
                    println!("    {pre}");
                    println!("  after it:");
                    println!("    {post}");
                } else {
                    println!("take these two out of {agent}'s hook configuration by hand:");
                    println!("    {pre}");
                    println!("    {post}");
                }
                return Ok(0);
            }
            let home = dirs::home_dir().ok_or("no home directory")?;
            let settings = crate::agent_hook::claude_dir(&home).join("settings.json");
            // A file that is there and cannot be read is left alone: writing
            // it would replace somebody's settings with ours.
            let current: serde_json::Value = match std::fs::read_to_string(&settings) {
                Ok(t) if t.trim().is_empty() => serde_json::Value::Null,
                Ok(t) => serde_json::from_str(&t).map_err(|e| {
                    format!("{} is not JSON, left as it is: {e}", settings.display())
                })?,
                Err(_) => serde_json::Value::Null,
            };
            let exe = installed_exe()?;
            let pre = crate::agent_hook::hook_command(&exe, !standalone(), "pre", &agent)?;
            let post = crate::agent_hook::hook_command(&exe, !standalone(), "post", &agent)?;
            let next = crate::agent_hook::claude_settings(&current, &pre, &post, install)?;
            if next == current || (current.is_null() && next == json!({})) {
                println!("{}: nothing to change", settings.display());
                return Ok(0);
            }
            println!("write  {}", settings.display());
            if install {
                println!("  before a file write or a shell command: {pre}");
                println!("  after it:                                {post}");
            } else {
                println!("  the two entries of this hook are taken out; nothing else changes");
            }
            if print_only {
                return Ok(0);
            }
            // The change to the agent's settings is itself a change this
            // record is for: kept with a copy, so it can be undone.
            let existed = settings.is_file();
            let ext = External {
                kind: "file".into(),
                by: format!("podshl {cmd}"),
                path: Some(settings.clone()),
                upstream: Upstream::default(),
                original_path: None,
                original_package: None,
                watch_issue: false,
                note: Some(format!(
                    "{} PODSHL's hook in {agent}'s settings, which records what the agent writes outside a git repository",
                    if install { "Installs" } else { "Removes" }
                )),
            };
            let look = Lookups::offline();
            let begun = if existed {
                Some(repair::begin_external(&root, ext.clone(), &look)?)
            } else {
                None
            };
            if let Some(dir) = settings.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let text = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())? + "\n";
            std::fs::write(&settings, text).map_err(|e| e.to_string())?;
            let rec = match begun {
                Some(r) => repair::finish_external(&root, &r.id)?,
                None => repair::add_external(&root, ext, &look)?,
            };
            crate::clientlog::line(&format!(
                "repairs {cmd} {agent}: {} (record {})",
                settings.display(),
                rec.id
            ));
            println!("recorded as {}", rec.id);
            Ok(0)
        }
        "--version" | "-V" | "version" => {
            let name = if standalone() {
                "podshl-repairs"
            } else {
                "podshl-client repairs"
            };
            println!("{name} {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            Ok(0)
        }
        other => Err(format!("unknown repairs command {other:?}\n\n{}", usage())),
    }
}

// ------------------------------------------------------------------ after updates

/// One thing `install-hook` does, said before it is done.
#[derive(Debug, PartialEq)]
pub enum Step {
    Write { path: PathBuf, body: String },
    Remove { path: PathBuf },
    Run { program: String, args: Vec<String> },
}

impl Step {
    fn describe(&self) -> String {
        match self {
            Step::Write { path, .. } => format!("write  {}", path.display()),
            Step::Remove { path } => format!("remove {}", path.display()),
            Step::Run { program, args } => format!("run    {program} {}", args.join(" ")),
        }
    }

    fn apply(&self) -> Result<(), String> {
        match self {
            Step::Write { path, body } => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
                }
                Ok(())
            }
            Step::Remove { path } => match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("{}: {e}", path.display()))
                }
                _ => Ok(()),
            },
            Step::Run { program, args } => {
                let mut cmd = std::process::Command::new(program);
                cmd.args(args);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0800_0000);
                }
                let out = cmd.output().map_err(|e| format!("{program}: {e}"))?;
                // Removing what is not there is not a failure.
                if out.status.success()
                    || args
                        .iter()
                        .any(|a| a == "/Delete" || a == "disable" || a == "unload")
                {
                    Ok(())
                } else {
                    Err(format!(
                        "{program} {}: {}",
                        args.join(" "),
                        String::from_utf8_lossy(&out.stderr).trim()
                    ))
                }
            }
        }
    }
}

/// The agent Omarchy is set to use, from Omarchy itself. `None` anywhere
/// else, and on an Omarchy where nobody chose one — there is no default to
/// guess at, which is also what Omarchy's own invitation hook concludes.
/// What to say to somebody whose agent is not Claude Code. Not a dead end:
/// the record itself never needed the hook, and the one thing that would make
/// the hook possible for their agent is a thing they can do.
/// How to type this program's name in a sentence it prints. The record has two
/// programs on the same commands, and a line telling somebody to run
/// `podshl-repairs …` when they have `podshl-client` names a command they do
/// not have — the notice above already kept the two apart.
fn me() -> &'static str {
    if standalone() {
        "podshl-repairs"
    } else {
        "podshl-client repairs"
    }
}

/// "1 call", "4 calls": a count somebody reads, not a field.
fn calls(root: &Path) -> String {
    match crate::agent_hook::samples(root).len() {
        1 => "1 call".to_string(),
        n => format!("{n} calls"),
    }
}

fn unmeasured_advice(agent: &str, written_down: bool) -> String {
    let mut lines = vec![];
    if written_down {
        lines.push(format!(
            "{agent}'s hook format is written down here but was not measured on a machine."
        ));
        lines.push("Take it anyway with --guessed: every record it makes says so.".into());
    } else {
        lines.push(format!(
            "no hook format for {agent:?}; measured: {}.",
            crate::agent_hook::MEASURED.join(", ")
        ));
        lines.push("The record itself does not need the hook: `begin`/`done`, `add`,".into());
        lines.push("`list`, `review` and `restore` work with any agent, or none.".into());
    }
    lines.push(format!(
        "Help it along: {} measure-agent-hook --agent {agent}",
        me()
    ));
    lines.push(format!(
        "keeps what {agent}'s hooks send, records nothing, and sends nothing."
    ));
    lines.join(
        "
",
    )
}

fn omarchy_default_agent() -> Option<String> {
    let exe = which("omarchy-default-agent")?;
    let (ok, out) = crate::reads::run_bounded(&exe, &[])?;
    let name = out.lines().next().unwrap_or("").trim().to_string();
    (ok && !name.is_empty()).then_some(name)
}

/// A path that goes into a shell line or a unit file unquoted-safely, or not
/// at all.
fn plain_path(p: &Path) -> Result<String, String> {
    let s = p.to_string_lossy().to_string();
    if s.chars()
        .any(|c| c == '\'' || c == '"' || c == '%' || c == '\n' || c.is_control())
    {
        return Err(format!("{s} cannot be written into a hook safely"));
    }
    Ok(s)
}

pub const TASK_NAME: &str = r"PODSHL\Repairs review";
pub const UNIT: &str = "podshl-repairs";
pub const LAUNCHD_LABEL: &str = "de.podshl.repairs";

/// What `install-hook` (or `remove-hook`) does on this system.
pub fn hook_plan(exe: &Path, install: bool) -> Result<Vec<Step>, String> {
    let exe_s = plain_path(exe)?;
    let home = dirs::home_dir().ok_or("no home directory")?;
    if cfg!(windows) {
        return Ok(if install {
            vec![Step::Run {
                program: "schtasks".into(),
                args: vec![
                    "/Create".into(),
                    "/F".into(),
                    "/TN".into(),
                    TASK_NAME.into(),
                    "/SC".into(),
                    "DAILY".into(),
                    "/ST".into(),
                    "12:00".into(),
                    "/TR".into(),
                    format!("\"{exe_s}\" repairs review --notify"),
                ],
            }]
        } else {
            vec![Step::Run {
                program: "schtasks".into(),
                args: vec![
                    "/Delete".into(),
                    "/F".into(),
                    "/TN".into(),
                    TASK_NAME.into(),
                ],
            }]
        });
    }
    if cfg!(target_os = "macos") {
        let plist = home
            .join("Library/LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist"));
        let plist_s = plain_path(&plist)?;
        return Ok(if install {
            vec![
                Step::Write {
                    path: plist.clone(),
                    body: launchd_plist(&exe_s),
                },
                Step::Run {
                    program: "launchctl".into(),
                    args: vec!["load".into(), "-w".into(), plist_s],
                },
            ]
        } else {
            vec![
                Step::Run {
                    program: "launchctl".into(),
                    args: vec!["unload".into(), "-w".into(), plist_s],
                },
                Step::Remove { path: plist },
            ]
        });
    }
    // Linux. Omarchy runs every file in its post-update hook folder with bash
    // after each update, which is exactly when to look; elsewhere, once a day.
    let omarchy = home.join(".config/omarchy");
    if omarchy.is_dir() {
        let hook = omarchy.join("hooks/post-update.d").join(UNIT);
        return Ok(if install {
            vec![Step::Write {
                path: hook,
                body: omarchy_hook(&exe_s),
            }]
        } else {
            vec![Step::Remove { path: hook }]
        });
    }
    let units = home.join(".config/systemd/user");
    let service = units.join(format!("{UNIT}.service"));
    let timer = units.join(format!("{UNIT}.timer"));
    Ok(if install {
        vec![
            Step::Write {
                path: service,
                body: systemd_service(&exe_s),
            },
            Step::Write {
                path: timer,
                body: SYSTEMD_TIMER.into(),
            },
            Step::Run {
                program: "systemctl".into(),
                args: vec!["--user".into(), "daemon-reload".into()],
            },
            Step::Run {
                program: "systemctl".into(),
                args: vec![
                    "--user".into(),
                    "enable".into(),
                    "--now".into(),
                    format!("{UNIT}.timer"),
                ],
            },
        ]
    } else {
        vec![
            Step::Run {
                program: "systemctl".into(),
                args: vec![
                    "--user".into(),
                    "disable".into(),
                    "--now".into(),
                    format!("{UNIT}.timer"),
                ],
            },
            Step::Remove { path: timer },
            Step::Remove { path: service },
        ]
    })
}

/// The program a hook should start later. An AppImage runs from a mount that
/// is gone when it exits; the file to start again is the one it names in
/// `APPIMAGE`, and only when that file exists.
fn installed_exe() -> Result<PathBuf, String> {
    if cfg!(target_os = "linux") {
        if let Some(image) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
            if image.is_absolute() && image.is_file() {
                return Ok(image);
            }
        }
    }
    std::env::current_exe().map_err(|e| e.to_string())
}

fn omarchy_hook(exe: &str) -> String {
    // Named for the program that wrote it: the file is what somebody finds in
    // their hooks directory, and it said `podshl-client` when `podshl-repairs`
    // had written it.
    let by = if standalone() {
        "podshl-repairs install-hook"
    } else {
        "podshl-client repairs install-hook"
    };
    format!(
        "#!/bin/bash\n\
# Written by `{by}`, removed by `remove-hook`.\n\
# After every Omarchy update: which recorded local fixes want another look.\n\
# It changes nothing, and asks the network only about issues you chose to watch.\n\
'{exe}' repairs review --notify || true\n"
    )
}

fn systemd_service(exe: &str) -> String {
    format!(
        "[Unit]\nDescription=PODSHL: which recorded local fixes want another look\n\n\
[Service]\nType=oneshot\nExecStart=\"{exe}\" repairs review --notify\n\
SuccessExitStatus=3\n"
    )
}

const SYSTEMD_TIMER: &str = "[Unit]\nDescription=PODSHL: look at recorded local fixes daily\n\n\
[Timer]\nOnCalendar=daily\nPersistent=true\nRandomizedDelaySec=1h\n\n\
[Install]\nWantedBy=timers.target\n";

fn launchd_plist(exe: &str) -> String {
    let esc = exe
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{esc}</string><string>repairs</string><string>review</string><string>--notify</string></array>
  <key>RunAtLoad</key><true/>
  <key>StartCalendarInterval</key><dict><key>Hour</key><integer>12</integer><key>Minute</key><integer>0</integer></dict>
</dict>
</plist>
"#
    )
}

// ------------------------------------------------------------------ telling the person

/// What the notification says about `n` records wanting another look.
///
/// One record is the ordinary case — a single fix a single update disturbed —
/// and the sentence had only a plural form, so the notification read "1
/// recorded local fixes want another look", and in German "1 aufgezeichnete
/// lokale Korrekturen", in Spanish "1 correcciones". Every language the client
/// speaks got it wrong in the one text most people ever see of this feature.
fn notice(n: usize) -> String {
    notice_for(n, standalone())
}

/// The notification's sentence, for the client or for `podshl-repairs`.
fn notice_for(n: usize, alone: bool) -> String {
    match (n == 1, alone) {
        (true, false) => m!("repair_cli_notify_one"),
        (false, false) => m!("repair_cli_notify", n = n),
        (true, true) => m!("repair_cli_notify_run_one", cmd = "podshl-repairs review"),
        (false, true) => m!(
            "repair_cli_notify_run",
            n = n,
            cmd = "podshl-repairs review"
        ),
    }
}

/// On Omarchy, the notification goes through Omarchy's own sender, so that a
/// click opens a terminal with the review — the way Omarchy's own hooks invite
/// somebody to act. `notify-send` has no click that runs anything.
///
/// **Critical, so that it stays.** Omarchy shows a low notice for 5 seconds
/// and a normal one for 8, then files it in its history. The hook runs in the
/// middle of `omarchy-update`, between the package upgrade and the AUR and
/// mise steps, while the person is watching the terminal — all three notices
/// of 2026-09-19 were shown, and filed, and not seen. A critical one stays
/// until it is dismissed; Omarchy uses exactly that for its own invitation
/// after an update. It does not get through do-not-disturb: Omarchy lets only
/// its own senders past that, and silenced notices go to the history.
///
/// The click runs `omarchy-launch-floating-terminal-with-presentation`, which
/// joins its arguments into one `bash -c` line: so the program is passed
/// quoted, and a path that cannot be quoted safely is refused rather than
/// passed. `omarchy-notification-send` itself takes each word as one value.
///
/// **One notice, not a pile.** A critical notice stays, so every update nobody
/// looked after left one more on top of the last — three identical notices on
/// the Omarchy desktop after a morning of dry runs. The id of the last one is
/// kept (`replace`), and the next replaces it: the same notice while it is on
/// screen, and a new one once it was dismissed — both measured on Omarchy.
/// `-p` makes the sender print the id to keep.
fn omarchy_notification(
    text: &str,
    exe: &Path,
    alone: bool,
    replace: Option<u32>,
) -> Option<Vec<String>> {
    let exe = plain_path(exe).ok()?;
    let mut exec = vec![
        "omarchy-launch-floating-terminal-with-presentation".to_string(),
        format!("'{exe}'"),
    ];
    if !alone {
        exec.push("repairs".into());
    }
    exec.push("review".into());
    let mut args: Vec<String> = vec!["-p".into()];
    if let Some(id) = replace {
        args.extend(["-r".to_string(), id.to_string()]);
    }
    args.extend(
        ["-u", "critical", "--app-name", "PODSHL", "PODSHL", text]
            .iter()
            .map(|s| s.to_string()),
    );
    args.push("--exec".into());
    args.extend(exec);
    Some(args)
}

/// What Windows files the notification under. It is `tauri.conf.json`'s
/// `identifier`, which is also the installed shortcut's identity.
///
/// A toast is shown by an *app*, and an unpackaged program has no app identity
/// until it claims one. Raising the toast through `powershell.exe` and letting
/// it use PowerShell's own identity worked — the toast arrived — but it arrived
/// as "Windows PowerShell": that is the name in the notification, the name the
/// Action Center groups it under, and the name in the per-app notification
/// settings, so somebody turning PODSHL's notifications off would be turning
/// PowerShell's off instead. It is also why the first walk on Windows did not
/// find the notification it had just sent.
///
/// Not behind `cfg(windows)`, and neither is the script below it or the case
/// that checks them: the suite runs on Linux, so a Windows-only test is one
/// nothing ever runs, and a case nothing runs reads exactly like a case that
/// passes.
const APP_ID: &str = "de.podshl.client";

/// Where the name lives: the current user's own hive, and nowhere else.
#[cfg_attr(not(windows), allow(dead_code))]
const APP_ID_KEY: &str = r"HKCU\Software\Classes\AppUserModelId\de.podshl.client";

/// Claim the app identity, so the toast is PODSHL's and says so.
///
/// `HKCU\Software\Classes\AppUserModelId\<id>` with a `DisplayName` is what
/// Windows reads for an unpackaged program's notifications.
///
/// Through `reg.exe`, and not through the registry-writing calls themselves:
/// those belong to the elevate helper and to nothing the client is built from
/// (`L5`), and that rule is worth keeping whole even for a key that needs no
/// privilege at all. It is also how the client already reads the registry.
/// `reg.exe` takes an argv, so there is no shell for a value to break out of,
/// and every part of this call is a constant besides.
#[cfg(windows)]
fn register_app_id() {
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let exe = system.join(r"System32\reg.exe");
    let _ = crate::reads::run_bounded(
        &exe,
        &[
            "add",
            APP_ID_KEY,
            "/v",
            "DisplayName",
            "/t",
            "REG_SZ",
            "/d",
            "PODSHL",
            "/f",
        ],
    );
}

/// The PowerShell that raises the toast, under PODSHL's own identity.
#[cfg_attr(not(windows), allow(dead_code))]
fn toast_script(text: &str) -> String {
    format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
         $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
         $t = $x.GetElementsByTagName('text'); $t.Item(0).AppendChild($x.CreateTextNode('PODSHL')) > $null; \
         $t.Item(1).AppendChild($x.CreateTextNode('{text}')) > $null; \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{APP_ID}').Show([Windows.UI.Notifications.ToastNotification]::new($x))")
}

/// A desktop notification, where the system has a way to show one. Best
/// effort: a review that found something has already said so on stdout and in
/// its exit code.
pub fn notify(text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    let text = text.replace(['"', '\'', '`', '$', '\\'], "");
    let run = |program: &Path, args: &[&str]| {
        let _ = crate::reads::run_bounded(program, args);
    };
    if cfg!(target_os = "macos") {
        let script = format!("display notification \"{text}\" with title \"PODSHL\"");
        run(Path::new("/usr/bin/osascript"), &["-e", &script]);
    } else if cfg!(windows) {
        #[cfg(windows)]
        {
            register_app_id();
            let ps = toast_script(&text);
            let system = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            let exe = system.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
            run(&exe, &["-NoProfile", "-NonInteractive", "-Command", &ps]);
        }
    } else if let (Some(sender), Ok(me)) = (which("omarchy-notification-send"), installed_exe()) {
        let kept = state_root().join("omarchy-notice-id");
        let last = std::fs::read_to_string(&kept)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        match omarchy_notification(&text, &me, standalone(), last) {
            Some(args) => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                if let Some((true, out)) = crate::reads::run_bounded(&sender, &args) {
                    if let Ok(id) = out.trim().parse::<u32>() {
                        let _ = std::fs::write(&kept, id.to_string());
                    }
                }
            }
            None => {
                if let Some(exe) = which("notify-send") {
                    run(&exe, &["-a", "PODSHL", "PODSHL", &text]);
                }
            }
        }
    } else if let Some(exe) = which("notify-send") {
        run(&exe, &["-a", "PODSHL", "PODSHL", &text]);
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RR27: `show` says everything about one record a person needs a year
    /// later — why, who declared it and whether that was withdrawn, upstream,
    /// the version with the fix, whether there is a way back, and when.
    #[test]
    fn show_says_why_who_upstream_and_whether_there_is_a_way_back() {
        assert_eq!(epoch_date(0), "1970-01-01");
        assert_eq!(epoch_date(1_709_164_800), "2024-02-29");
        assert_eq!(epoch_date(1_789_822_433), "2026-09-19");

        let mut r: repair::Record = serde_json::from_value(json!({
            "id": "1-1", "at": 1_789_822_433u64, "action": "external:package", "params": {},
            "reversible": false, "subject": "declared by podshl-bin", "state": "applied",
            "kind": "package",
            "note": "Built outside the AUR, because registration is closed.",
            "upstream": { "package": "podshl-bin", "issue": "https://github.com/o/r/issues/1",
                          "fixed_in": "0.2.0" },
            "installed": { "version": "0.1.8-1", "source": "pacman" },
            "declared": { "file": "/usr/share/podshl/repairs.d/podshl-bin.json",
                          "entry": "outside-the-aur", "package": "podshl-bin" }
        }))
        .unwrap();
        let text = show_lines(&r).join("\n");
        for want in [
            "Built outside the AUR, because registration is closed.",
            "podshl-bin",
            "https://github.com/o/r/issues/1",
            "watch 1-1 on",
            "0.2.0",
            "0.1.8-1",
            "2026-09-19",
        ] {
            assert!(text.contains(want), "show does not say {want:?}:\n{text}");
        }
        let lines = show_lines(&r);
        assert!(
            crate::msg::is("repair_cli_show_no_copy", &lines[lines.len() - 2]),
            "a record without a copy does not say there is no way back:
{text}"
        );

        // Withdrawn by its maintainer: said, with the reason.
        r.declared.as_mut().unwrap().retired = Some("Now in the AUR.".into());
        let text = show_lines(&r).join("\n");
        assert!(text.contains("Now in the AUR."), "{text}");
    }

    /// RR26: the man pages say what the program does. Every command and every
    /// flag in the built-in help is in `podshl-repairs(1)`, and every field a
    /// declaration is read for is in `podshl-repairs.d(5)` — so a command added
    /// to one and not the other fails here rather than in somebody's terminal.
    #[test]
    fn the_man_pages_name_every_command_flag_and_field() {
        let read = |name: &str| {
            let p = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../packaging/repairs")
                .join(name);
            std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
                .replace("\\-", "-")
                .replace("\\e", "\\")
        };
        let man1 = read("podshl-repairs.1");
        let man5 = read("podshl-repairs.d.5");

        // Commands: the first word of each entry in the usage, indented by two.
        let mut commands = vec![];
        for line in USAGE.lines() {
            if let Some(rest) = line.strip_prefix("  ") {
                if rest.starts_with(' ') {
                    continue;
                }
                let first = rest.split([' ', ',']).next().unwrap_or("");
                if !first.is_empty() {
                    commands.push(first.to_string());
                }
            }
        }
        assert!(commands.len() > 10, "found too few commands: {commands:?}");
        for c in &commands {
            let marked = [".B ", ".BR ", ".BI "].iter().any(|m| {
                man1.lines().any(|l| {
                    l.starts_with(m) && l[m.len()..].trim_start_matches('"').starts_with(c.as_str())
                })
            });
            assert!(marked, "podshl-repairs(1) has no entry for `{c}`");
        }
        // Flags: every --word in the usage.
        let re = regex::Regex::new(r"--[a-z][a-z-]*").unwrap();
        for f in re.find_iter(USAGE).map(|m| m.as_str()) {
            assert!(man1.contains(f), "podshl-repairs(1) does not mention {f}");
        }
        // The agent-hook command the hooks call is on the page too.
        assert!(
            man1.contains("agent-hook"),
            "the command the hooks call is not documented"
        );

        for field in [
            "id",
            "kind",
            "reason",
            "until",
            "issue",
            "fixed_in",
            "path",
            "package",
            "original",
            "original_package",
            "retired",
        ] {
            assert!(
                man5.contains(&format!(".B {field}\n"))
                    || man5.contains(&format!("\"{field}\""))
                    || man5.contains(&format!(".B {field}")),
                "podshl-repairs.d(5) does not describe the field `{field}`"
            );
        }
    }

    /// RR20: the notification names the program that raised it, and on
    /// Omarchy a click opens the review.
    ///
    /// It said "Open PODSHL, or run: podshl-client repairs review" whoever
    /// raised it, so somebody with only `podshl-repairs` was told to open a
    /// window they do not have and run a program they do not have. And
    /// `notify-send` has no click that runs anything.
    #[test]
    fn the_notification_names_the_program_that_raised_it() {
        for n in [1, 3] {
            let alone = notice_for(n, true);
            assert!(alone.contains("podshl-repairs review"), "{alone}");
            assert!(!alone.contains("podshl-client"), "{alone}");
            assert!(!alone.contains("Open PODSHL"), "{alone}");
            let client = notice_for(n, false);
            assert!(client.contains("podshl-client repairs review"), "{client}");
        }

        let exe = Path::new("/home/someone/.local/bin/podshl-repairs");
        let args = omarchy_notification("text", exe, true, None).expect("a plain path was refused");
        // The id is always asked for, so the next notice can replace this one.
        assert_eq!(args[0], "-p");
        assert!(
            !args.contains(&"-r".to_string()),
            "nothing to replace yet: {args:?}"
        );
        // And given one, it replaces it: one notice, not a pile.
        let next = omarchy_notification("text", exe, true, Some(7)).unwrap();
        let r = next
            .iter()
            .position(|a| a == "-r")
            .expect("the last notice is not replaced");
        assert_eq!(next[r + 1], "7");
        assert!(r < next.iter().position(|a| a == "--exec").unwrap());
        let at = args.iter().position(|a| a == "--exec").expect("no click");
        assert_eq!(
            &args[at + 1..],
            &[
                "omarchy-launch-floating-terminal-with-presentation",
                "'/home/someone/.local/bin/podshl-repairs'",
                "review"
            ],
            "the click does not run this program's review"
        );
        // Omarchy shows low for 5 seconds and normal for 8, in the middle of
        // an update; critical stays until it is dismissed.
        let u = args.iter().position(|a| a == "-u").expect("no urgency");
        assert_eq!(args[u + 1], "critical");
        // The text is one value, before the click, not part of it.
        assert!(args[..at].contains(&"text".to_string()));

        let client =
            omarchy_notification("text", Path::new("/usr/bin/podshl-client"), false, None).unwrap();
        assert!(
            client.ends_with(&["repairs".into(), "review".into()]),
            "{client:?}"
        );

        // The click is joined into one `bash -c` line, so a path that could
        // break out of its quotes is not passed at all.
        assert!(
            omarchy_notification("t", Path::new("/tmp/it's/podshl-repairs"), true, None).is_none()
        );
    }

    /// RR10: what `install-hook` writes is said first, runs the review and
    /// nothing else, and a path that could break out of the line is refused.
    #[test]
    fn the_hook_runs_the_review_and_nothing_else() {
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\PODSHL\podshl-client.exe")
        } else {
            PathBuf::from("/usr/bin/podshl-client")
        };
        let plan = hook_plan(&exe, true).unwrap();
        assert!(!plan.is_empty());
        for step in &plan {
            match step {
                Step::Write { body, .. } => {
                    // Every system writes the same call, and each spells it its
                    // own way: a shell line and a systemd `ExecStart` hold it
                    // contiguously, a launchd plist holds it as argv elements,
                    // and the systemd *timer* holds no command at all.
                    //
                    // This used to accept only the first two spellings, and
                    // passed everywhere it was ever run — because until the
                    // macOS walk existed it was never run on a Mac, where the
                    // plan it checks is the plist. A case that cannot see a
                    // third of what it covers.
                    let argv_plist = ["repairs", "review", "--notify"]
                        .iter()
                        .all(|w| body.contains(&format!("<string>{w}</string>")));
                    assert!(
                        body.contains("repairs review --notify")
                            || argv_plist
                            || body.contains("[Timer]"),
                        "{body}"
                    );
                    assert!(!body.contains("rm ") && !body.contains("sudo"), "{body}");
                }
                Step::Run { program, args } => {
                    assert!(
                        ["schtasks", "systemctl", "launchctl"].contains(&program.as_str()),
                        "{program}"
                    );
                    if program == "schtasks" {
                        assert!(
                            args.iter().any(|a| a.ends_with("repairs review --notify")),
                            "{args:?}"
                        );
                        assert!(
                            !args.iter().any(|a| a.eq_ignore_ascii_case("/RL")),
                            "the task asks for elevation"
                        );
                    }
                }
                Step::Remove { .. } => panic!("install removed something"),
            }
        }
        let remove = hook_plan(&exe, false).unwrap();
        assert!(
            remove.iter().all(|s| !matches!(s, Step::Write { .. })),
            "{remove:?}"
        );

        let bad = PathBuf::from("/tmp/x'; rm -rf ~; '");
        assert!(
            hook_plan(&bad, true).is_err(),
            "a path with a quote was written into a hook"
        );
        // **All four bodies, from whatever machine runs this.** `hook_plan`
        // picks by `cfg!`, so the loop above only ever sees the plan for the
        // system the suite happens to be on — which is why the plist's
        // spelling went unchecked until a Mac ran it. These builders take a
        // path and return a string, so each can be read anywhere.
        let exe = "/usr/bin/podshl-client";
        assert!(omarchy_hook(exe).starts_with("#!/bin/bash\n"));
        assert!(omarchy_hook(exe).contains("repairs review --notify"));
        assert!(
            systemd_service(exe).contains("SuccessExitStatus=3"),
            "a review that found something would read as a failed unit"
        );
        assert!(systemd_service(exe).contains("repairs review --notify"));
        assert!(
            SYSTEMD_TIMER.contains("[Timer]") && SYSTEMD_TIMER.contains("Persistent=true"),
            "a machine that was off when the timer was due would never look"
        );

        let plist = launchd_plist(exe);
        for w in ["repairs", "review", "--notify"] {
            assert!(
                plist.contains(&format!("<string>{w}</string>")),
                "the launchd plist does not call {w}:\n{plist}"
            );
        }
        assert!(plist.contains(LAUNCHD_LABEL), "{plist}");
        assert!(plist.contains(exe), "{plist}");
        // The same two refusals the loop makes of every body.
        for body in [omarchy_hook(exe), systemd_service(exe), plist] {
            assert!(!body.contains("rm ") && !body.contains("sudo"), "{body}");
        }
    }

    /// RR11: the notification counts in whichever language it is read.
    ///
    /// Found by walking a real Omarchy desktop: with one record the sentence
    /// read "1 recorded local fixes want another look". The count is the
    /// common case, so the wrong half of the sentence was the half everybody
    /// saw. Each language file is checked directly, because the binary only
    /// ever reads `en.json` and a German reader would have been the last to
    /// know.
    #[test]
    fn one_record_is_said_in_the_singular_in_every_language() {
        let one = notice(1);
        let many = notice(4);
        assert!(
            !one.starts_with('1'),
            "the singular still leads with a digit: {one}"
        );
        assert_ne!(one, many);
        assert!(many.starts_with('4'), "{many}");

        // The English the window matches on must be two distinct sentences,
        // or the window would show the plural for both.
        assert!(crate::msg::is("repair_cli_notify_one", &one), "{one}");
        assert!(crate::msg::is("repair_cli_notify", &many), "{many}");
        assert!(
            !crate::msg::is("repair_cli_notify", &one),
            "the singular still matches the plural template"
        );

        // Every language says it, and says something other than the plural.
        for lang in ["en", "de", "es", "fr"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("ui/i18n")
                .join(format!("{lang}.json"));
            let text = std::fs::read_to_string(&path).unwrap();
            let table: serde_json::Value = serde_json::from_str(&text).unwrap();
            let single = table["m_repair_cli_notify_one"]
                .as_str()
                .unwrap_or_else(|| panic!("{lang} has no m_repair_cli_notify_one"));
            let plural = table["m_repair_cli_notify"].as_str().unwrap();
            assert!(
                !single.contains("{n}"),
                "{lang}: the singular still takes a count: {single}"
            );
            assert_ne!(single, plural, "{lang}: the singular is the plural");
        }
    }

    /// RR12: the toast is PODSHL's, and says PODSHL.
    ///
    /// It used to be raised under `powershell.exe`'s own app identity, so it
    /// arrived named "Windows PowerShell" — grouped under PowerShell in the
    /// Action Center, and switched off by PowerShell's notification setting.
    #[test]
    fn the_windows_toast_is_shown_under_podshls_own_identity() {
        let script = toast_script("something wants another look");
        assert!(
            script.contains(&format!("CreateToastNotifier('{APP_ID}')")),
            "{script}"
        );
        assert!(
            !script.to_ascii_lowercase().contains("powershell.exe"),
            "the toast still borrows PowerShell's identity: {script}"
        );
        assert!(!script.contains("1AC14E77"), "{script}");
        // The identity is the one the installer gives the program, not a
        // second name invented here.
        let conf: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(conf["identifier"].as_str(), Some(APP_ID));
        // The name is claimed in the user's own hive, and the key says so.
        assert!(APP_ID_KEY.starts_with(r"HKCU\"), "{APP_ID_KEY}");
        assert!(APP_ID_KEY.ends_with(APP_ID), "{APP_ID_KEY}");
    }

    /// RR15: what the client registers outside its own folder, the uninstaller
    /// takes back.
    ///
    /// Two marks live outside `$INSTDIR`, so removing the folder does not
    /// remove them: the daily task `install-hook` creates, and the
    /// notification identity. The task is the serious one — left behind,
    /// Windows goes on running it once a day against an executable that is no
    /// longer there, and nobody uninstalling a program thinks to run
    /// `remove-hook` first. Checked here rather than in the installer, because
    /// the installer is a thing this project ships and never runs.
    #[test]
    fn the_uninstaller_takes_back_what_was_registered_outside_the_folder() {
        let nsh = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("windows/installer-hooks.nsh"),
        )
        .unwrap();
        let (_, uninstall) = nsh
            .split_once("NSIS_HOOK_POSTUNINSTALL")
            .expect("there is no uninstall hook");
        // Named from the constant `hook_plan` creates it with, so the two
        // cannot drift apart: renaming the task and forgetting the installer
        // would leave the old one running daily against a deleted program.
        assert!(
            uninstall.contains(&format!(r#"schtasks /Delete /F /TN "{TASK_NAME}""#)),
            "the uninstaller does not delete {TASK_NAME}:\n{uninstall}"
        );
        // The same name the client claims, spelled the same way.
        let key = APP_ID_KEY.strip_prefix(r"HKCU\").unwrap();
        assert!(
            uninstall.contains(&format!(r#"DeleteRegKey HKCU "{key}""#)),
            "the uninstaller leaves the notification identity behind:\n{uninstall}"
        );
        // ...and an update is not an uninstall: the new version wants both.
        assert!(uninstall.contains("$UpdateMode <> 1"), "{uninstall}");
    }

    /// RR16: a record can be removed, and almost nothing can remove one.
    ///
    /// Both gates, in all four combinations. The command itself cannot answer
    /// this: whether it refuses depends on how the machine running the suite
    /// is set up — the CI container is root, a developer's terminal is not —
    /// so the decision is tested and the wiring is one line.
    #[test]
    fn removing_a_record_needs_the_privilege_and_a_person() {
        assert!(may_forget(true, true).is_ok());

        let no_root = may_forget(false, true).unwrap_err();
        assert!(
            crate::msg::is("repair_forget_needs_root", &no_root),
            "{no_root}"
        );
        // Being at a keyboard is not a substitute for the privilege, and
        // having the privilege is not a substitute for being there.
        let no_person = may_forget(true, false).unwrap_err();
        assert!(
            crate::msg::is("repair_forget_needs_a_person", &no_person),
            "{no_person}"
        );
        // Neither: the privilege is named first, because it is the one the
        // person has to do something about.
        let neither = may_forget(false, false).unwrap_err();
        assert_eq!(neither, no_root);

        // No option anywhere skips either gate. A flag that did was written
        // while this was built and taken out again.
        let src = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/repairs_cli.rs"),
        )
        .unwrap();
        let wiring = src.split("\"forget\" =>").nth(1).expect("no forget branch");
        let wiring = &wiring[..wiring.find("\"watch\" =>").unwrap_or(wiring.len())];
        assert!(
            wiring.contains("may_forget(running_privileged(), std::io::stdin().is_terminal())"),
            "the forget branch does not ask both gates:
{wiring}"
        );
        assert!(
            !wiring.contains("a.flag("),
            "the forget branch takes an option that could skip a gate"
        );
    }

    /// RR16: what a forgotten record leaves behind.
    #[test]
    fn a_forgotten_record_leaves_a_stub_and_takes_its_copy() {
        let dir = std::env::temp_dir().join(format!("podshl-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("app.conf");
        std::fs::write(
            &file, "before
",
        )
        .unwrap();

        std::env::set_var("VS_ROOT", &dir);
        run(vec![
            "begin".into(),
            "--kind".into(),
            "file".into(),
            "--by".into(),
            "an agent".into(),
            "--path".into(),
            file.display().to_string(),
            "--note".into(),
            "a secret path nobody should keep".into(),
        ])
        .unwrap();
        let id = repair::load(&dir).last().unwrap().id.clone();
        std::fs::write(
            &file, "after
",
        )
        .unwrap();
        run(vec!["done".into(), id.clone()]).unwrap();

        let kept = repair::load(&dir).into_iter().find(|r| r.id == id).unwrap();
        let copy = kept.backup.clone().expect("begin kept no copy");
        assert!(Path::new(&copy).exists());

        let was = repair::forget(&dir, &id).unwrap();
        assert_eq!(was.id, id);
        assert_eq!(
            was.note.as_deref(),
            Some("a secret path nobody should keep")
        );

        // The copy is gone: leaving it would keep on disk the very contents
        // the record was removed to be rid of.
        assert!(
            !Path::new(&copy).exists(),
            "the copy the record kept is still there"
        );

        let after = repair::load(&dir);
        let stub = after
            .iter()
            .find(|r| r.id == id)
            .expect("the record vanished entirely");
        assert_eq!(stub.state, "forgotten");
        assert!(stub.forgotten_at.is_some());
        assert_eq!(stub.at, kept.at, "the stub forgot when the change was made");
        assert_eq!(stub.kind, kept.kind);
        // ...and it says nothing else.
        assert!(stub.target.is_none() && stub.backup.is_none() && stub.note.is_none());
        assert!(stub.subject.is_empty() && stub.file_sha256.is_none());
        assert!(!stub.watch_issue && stub.issue_state.is_none() && stub.upstream.issue.is_none());
        // Nothing on disk still holds what it said.
        let ledger = std::fs::read_to_string(dir.join("repairs.json")).unwrap();
        assert!(
            !ledger.contains("a secret path nobody should keep"),
            "{ledger}"
        );
        assert!(!ledger.contains("an agent"), "{ledger}");

        // A review does not raise a stub, and forgetting twice is refused.
        assert!(repair::review(&dir, &repair::Lookups::offline()).is_empty());
        assert!(repair::forget(&dir, &id).is_err());

        std::env::remove_var("VS_ROOT");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_command_line_says_what_it_did_not_understand() {
        assert!(run(vec!["review".into(), "--bogus".into()])
            .unwrap_err()
            .contains("--bogus"));
        assert!(run(vec!["watch".into(), "x".into(), "maybe".into()]).is_err());
        assert!(run(vec!["nonsense".into()])
            .unwrap_err()
            .contains("unknown repairs command"));
        assert_eq!(run(vec!["help".into()]), Ok(0));
    }
}
