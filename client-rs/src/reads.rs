//! The read vocabulary — vendor-parameterised, client-bounded.
//!
//! Collectors compiled into the client are the same mistake as skills bundled
//! at install time: they rot with the client's release cycle, and a vendor can
//! only ask for facts we happened to anticipate. So the *instruction* comes
//! from the vendor, per incident.
//!
//! What must not come from the vendor is **code**. If a vendor could ship
//! collection logic it could read anything, and consent-before-transmission
//! does not save that: a user shown forty fields does not evaluate them, and
//! some reads are harmful before anything is sent. So this is the same shape as
//! the action vocabulary, one level down — the client holds a small set of
//! bounded read *capabilities* and the vendor fills their parameters.
//!
//! Three limits stay on this side and are not negotiable by a skill:
//!   * **Root allow-list** — reads are confined to application configuration
//!     areas, never a home directory at large.
//!   * **Deny-list** — credentials, keys, browser profiles and wallets are
//!     refused whatever the vendor asks and whatever the user clicks. Consent
//!     cannot unlock these, because a user cannot evaluate the request.
//!   * **Volume cap** — a skill demanding hundreds of facts is not a diagnostic.
//!
//! Each capability carries a risk class, and the user approves the concrete
//! instance before any of it runs.

use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_READS: usize = 24;
/// An enumeration that walks further than this is an inventory sweep, not a
/// diagnostic, and the user cannot meaningfully review its result.
pub const MAX_ENTRIES: usize = 200;
pub const MAX_DEPTH: usize = 4;

#[derive(Serialize, Clone, Copy, PartialEq)]
pub enum Risk {
    Low,
    Medium,
}

impl Risk {
    pub fn label(&self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
        }
    }

    /// Language-neutral, for the window to translate. `label` is German, and
    /// it was shown as "risk gering" on an English consent screen.
    pub fn id(&self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
        }
    }
}

/// Tools a skill may invoke, with the argument shape each accepts. Anything
/// outside this table is refused — "run this command" is not a capability we
/// hand to a remote party.
const TOOLS: &[(&str, &str)] = &[
    ("nvidia-smi", r"--(query-gpu=[a-z_,.]+|format=csv,noheader)"),
    ("lspci", r"-[nmv]{1,3}"),
    ("system_profiler", r"SP[A-Za-z]+DataType"),
    ("sw_vers", r"-(productVersion|buildVersion)"),
    // A runtime version is the first thing any software project asks for, and
    // for a long time this list could not express one: every entry was hardware.
    // The open-source branch is the supply side the whole design depends on, and
    // a Python project that cannot say *which* Python cannot use it at all.
    //
    // Version flags only. `python3 -c` and `pip install` are the obvious things
    // to want next, and both are arbitrary execution — precisely what this list
    // exists to keep out of a vendor's reach. No `pip list` either: a package
    // inventory is a fingerprint of the machine.
    ("python3", r"(--version|-VV?)"),
    ("pip", r"--version"),
    ("node", r"(--version|-v)"),
    ("uname", r"-[rsmv]{1,4}"),
];

/// What a `read_registry` path and value name may contain. Held to
/// `spec/vocabulary/reads.json` by `vocabulary_matches_the_spec`, like the tool
/// patterns above, so the two implementations cannot drift apart on the one
/// question that used to be arbitrary code execution.
const REGISTRY_PATH: &str = r"(HKLM|HKCU|HKCR|HKU|HKCC):?(\\[A-Za-z0-9._ -]{1,64}){1,12}";
const REGISTRY_NAME: &str = r"[A-Za-z0-9._ -]{1,128}";

/// What a `program_version` instruction may name: a bare program name, never a
/// path. Where the program lives is this machine's business — the search path,
/// or a location the user points at — and a publisher-supplied path would be a
/// way of choosing *which file runs*.
const PROGRAM_NAME: &str = r"[A-Za-z0-9][A-Za-z0-9._+-]{0,63}";

/// The only arguments a program is ever run with. Closed, and chosen by the
/// client: the publisher picks one of these, it cannot write its own. `-v` is
/// deliberately absent — for half the programs that exist it means *verbose*,
/// which is a request to do the real work more loudly. So is the bare word
/// `version`: to a program with subcommands that is a subcommand, and to a
/// program without them it is an argument — a file to open, a target to
/// build, a host to connect to.
const VERSION_FLAGS: &[&str] = &["--version", "-V", "-version"];

/// Fields `nvidia-smi --query-gpu` may not be asked for. The pattern for the
/// tool admits any field name, and some of them identify the card rather than
/// describe it: its UUID, its bus address, and the firmware images that are
/// unique per unit. A publisher naming one is refused at the gate, before the
/// user is asked to approve reading it.
///
/// `serial` is deliberately not here. A warranty precheck is the one diagnosis
/// that genuinely turns on which unit this is; the user approves that reading
/// on the consent panel like any other, and `report.rs` holds the value back
/// out of anything that leaves. Refusing it would protect nobody — it would
/// move the same number off the card and onto a sticker the user reads out.
const GPU_FIELD_DENY: &str = r"(?i)uuid|pci|vbios|gsp";

/// Environment variables a publisher may ask for. An allow list rather than
/// the deny list: the environment is where every credential a person ever
/// exported lives, under names nobody can enumerate in advance, and a list of
/// what is *not* a secret is the only list that can be finished. These name
/// where a model endpoint is, which card is visible, where models are cached,
/// what kind of session the window runs in, and which GLX vendor library the
/// session loaded — nothing else is a fact a diagnosis has needed.
///
/// Kept identical to `env.allow` in `spec/vocabulary/reads.json`, and a test
/// below reads that file and compares: two lists that can drift are a gate
/// that refuses on one side and admits on the other.
const ENV_ALLOW: &[&str] = &[
    "OLLAMA_HOST", "OLLAMA_MODELS", "CUDA_VISIBLE_DEVICES", "HF_HOME", "XDG_SESSION_TYPE",
    "WAYLAND_DISPLAY", "__GLX_VENDOR_LIBRARY_NAME", "DISPLAY", "LANG", "SHELL", "TERM",
    "VIRTUAL_ENV",
];

/// Registry values that name the machine or its owner rather than describe
/// software on it. The deny list catches most of these by word; this catches
/// the rest, and it is checked on the value name alone so that a harmless key
/// path cannot be made to carry one of them past the gate.
const REGISTRY_NAME_DENY: &str = r"(?i)machineguid|productid|registeredowner|computername|hostname|username|serial|uuid";

/// Programs that are never run for their version, whoever names them and
/// whatever the user clicks.
///
/// The flag is fixed, so the risk is not what the publisher asks the program to
/// do — it is a program that does not parse its arguments at all and simply
/// does its job. A shell or a launcher runs something else; a power or session
/// tool changes the state of the machine; a disk tool destroys it. None of them
/// can have a version worth this, and a list is cheaper than a mistake. The
/// system directories are refused as well, below, which catches the long tail
/// of GUI programs that ignore arguments and open a window.
const PROGRAM_DENY: &[&str] = &[
    // shells and command runners
    "sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "nu", "cmd", "command",
    "powershell", "pwsh", "wsl", "env", "xargs", "nohup", "nice", "timeout", "time",
    "watch", "chroot", "setsid", "script", "expect", "busybox",
    // privilege
    "sudo", "su", "doas", "runas", "pkexec", "gsudo",
    // power, session and service state
    "shutdown", "reboot", "halt", "poweroff", "init", "telinit", "logoff", "logout",
    "systemctl", "launchctl", "service", "sc",
    // destructive
    "rm", "rmdir", "del", "erase", "dd", "mkfs", "format", "diskpart", "fdisk",
    "parted", "wipefs", "shred", "kill", "killall", "pkill", "taskkill",
    // launchers and script hosts
    "open", "xdg-open", "start", "explorer", "rundll32", "regsvr32", "mshta",
    "cscript", "wscript", "msiexec", "schtasks", "at", "crontab", "osascript",
    // package runners and build tools: each one runs whatever the directory
    // it is started in says — a `package.json` script, a `Makefile`, a
    // `justfile`, a `Rakefile` — and `--version` is no protection against a
    // tool that reads its configuration before it reads its arguments.
    "npx", "uvx", "bunx", "pipx", "pnpm", "yarn", "npm", "make", "just", "rake", "task",
    "gradle", "mvn",
    // language runtimes: an interpreter is a shell by another name, and the
    // ones a diagnosis needs are on the `run_tool` list with their own
    // closed patterns.
    "node", "python", "python3", "perl", "ruby", "php",
];

/// A Docker image repository as a publisher may name it: lower case, the
/// registry host and path segments Docker itself accepts, no tag and no digest.
/// The tag is what is being read, so a publisher naming one would be asserting
/// the answer.
pub(crate) const IMAGE_NAME: &str = r"[a-z0-9]+(?:[._-][a-z0-9]+)*(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*){0,3}";

/// How long a program gets to say its version. A program that is still busy
/// after this is not answering the question it was asked.
const RUN_LIMIT: Duration = Duration::from_secs(5);
/// How much of its output is looked at. A version is on the first lines.
const OUTPUT_LIMIT: u64 = 64 * 1024;

/// Refused regardless of consent. These are not judgement calls a user can make
/// under time pressure while something is broken.
const DENY: &[&str] = &[
    ".ssh", ".gnupg", "id_rsa", "id_ed25519", ".aws", ".kube",
    "credentials", "keychain", "Login Data", "cookies", "wallet", ".netrc",
    "shadow", ".env", "token", "secret", "password", "passwd", "api_key",
    "apikey", "access_key", "private_key", "privatekey", ".pem",
    "authorization", "bearer",
    // Shell and REPL histories: `.bash_history`, `.zsh_history`,
    // `.python_history`, PowerShell's `ConsoleHost_history.txt`. Every command
    // somebody typed, passwords given on a command line included. A manifest
    // asking for `.bash_history` passed the gate — the engram walkthrough's own
    // refusal harness printed "ACCEPTED (it should not have been)" and nobody
    // had read that line.
    "_history",
    // The words around a credential that the first list did not have: the
    // short forms, the login and session state a browser or a tool keeps,
    // the credential stores by name — and the values that identify a machine
    // or its owner rather than describe it, which no report may carry.
    "pass", "pwd", "passphrase", "auth", "login", "logins", "oauth", "session", "keyring",
    "pgpass", "machineguid", "productid", "uuid", "serial", "hostname", "computername",
    "username",
];

/// The largest file a `read_file_key`, `read_ini_key` or `enumerate_read`
/// will open. A configuration file is kilobytes; a megabyte is a database or
/// a log wearing a `.json` extension, and parsing it whole is the client
/// being made to hold something it was not asked to read.
const FILE_LIMIT: u64 = 1024 * 1024;
/// The longest value one of those reads hands back. A version, a path, a
/// name — none of them is a paragraph, and a paragraph is free text under a
/// key's name.
const VALUE_LIMIT: usize = 256;

/// Is the tool actually on this machine? A read that silently yields nothing
/// because `nvidia-smi` does not exist is indistinguishable from a card that
/// reports nothing, and the user is told neither. Absence must be stated.
///
/// The same lookup that decides what is *run*, so that the two cannot
/// disagree. They did: this accepted anything `PATHEXT` names — a `.cmd`, a
/// `.bat` — while the run went to `Command::new(name)`, which resolves the
/// name again on its own terms. Windows names executables with an extension,
/// and an earlier version that looked only for the bare name reported every
/// tool absent, `nvidia-smi` included, on a machine where it was plainly on
/// PATH.
fn tool_available(tool: &str) -> bool {
    find_on_path(tool).is_some()
}

/// The file names a program may have on this platform. Windows only runs
/// `.exe` and `.com` for this: a `.cmd` or `.bat` is a script handed to
/// `cmd.exe`, and "run this program for its version" must not quietly become
/// "run this script through a shell".
fn program_file_names(program: &str) -> Vec<String> {
    if cfg!(windows) {
        let low = program.to_ascii_lowercase();
        if low.ends_with(".exe") || low.ends_with(".com") {
            vec![program.to_string()]
        } else {
            vec![format!("{program}.exe"), format!("{program}.com")]
        }
    } else {
        vec![program.to_string()]
    }
}

fn is_runnable_file(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else { return false };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Where the program is on this process's search path, if it is there at all.
///
/// **The search path and nothing else.** No walk of the disk, no guess at
/// `Program Files` or `~/.local/bin`: a client that went looking for a
/// program by scanning directories would be reading the machine far beyond
/// what the user was asked about. If it is not on the path, the user is asked
/// where it is — they know, and pointing at it is itself the consent.
pub(crate) fn find_on_path(program: &str) -> Option<PathBuf> {
    if let Some(hit) = PATH_CACHE.lock().ok().and_then(|c| c.get(program).cloned()) {
        return hit;
    }
    let found = std::env::var_os("PATH").and_then(|p| find_in(&p, program));
    if let Ok(mut c) = PATH_CACHE.lock() {
        c.insert(program.to_string(), found.clone());
    }
    found
}

/// What the search path answered, remembered for one incident.
///
/// The catalogue asks whether every tool is present each time it is built,
/// the plan asks again for every reading, and the run asks once more — a
/// dozen walks of the search path per screen, each one a `stat` per directory
/// per candidate name. The answer does not change within a diagnosis, and
/// `end_incident` forgets it, so a program installed between two questions is
/// found by the second.
static PATH_CACHE: std::sync::Mutex<std::collections::BTreeMap<String, Option<PathBuf>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

fn find_in(paths: &std::ffi::OsStr, program: &str) -> Option<PathBuf> {
    let names = program_file_names(program);
    for dir in std::env::split_paths(paths) {
        // Absolute entries only. An empty entry or `.` means "wherever this
        // process was started", and a file sitting there — a download, say —
        // would be run as the program the publisher named.
        if !dir.is_absolute() {
            continue;
        }
        for n in &names {
            let p = dir.join(n);
            if is_runnable_file(&p) {
                return Some(p);
            }
        }
    }
    None
}

/// Where every program on the machine lives, the operating system's own and
/// everybody else's alike. A directory that resolves to one of these has
/// stopped telling the two apart, whatever it is called.
fn is_shared_bin(d: &Path) -> bool {
    ["/usr/bin", "/bin", "/usr/local/bin"].iter().any(|g| {
        let g = PathBuf::from(g);
        d == g.canonicalize().unwrap_or(g).as_path()
    })
}

/// Is this file one of the operating system's own?
///
/// The long tail that `PROGRAM_DENY` cannot name lives here: on Windows every
/// GUI program in the system directory ignores an argument it does not know and
/// opens a window, and on every platform the administrative tools are in the
/// `sbin` directories. None of them is somebody's project.
///
/// **Except that on a merged `/usr` they are not.** `/sbin` and `/usr/sbin` are
/// symbolic links to `/usr/bin` on Arch, Debian 12, Ubuntu 21 and later, Fedora
/// and openSUSE — so canonicalising them, which this did in order to see through
/// exactly such links, turned `/usr/bin` into a system directory and refused
/// **every** program on the machine. Measured on Omarchy: `git`, `docker`,
/// `curl`, `nvidia-smi` and `lspci` all answered "belongs to the operating
/// system and is not run for a version". That is the whole open-source path —
/// a project asking which version of its own package is installed — dead on
/// current Linux, and it went unseen because no walk had run on one.
///
/// So a name that resolves onto the shared binary directory is dropped rather
/// than believed: there it names nothing smaller than "every program", and a
/// rule that matches everything is not a rule. What still holds people back is
/// `PROGRAM_DENY` and `deny_hit`, which work on what a program *is* rather than
/// where a distribution decided to put it — `bash` and `python3` are refused
/// here by name, from `/usr/bin`, with the merge or without it.
fn in_system_directory(p: &Path) -> bool {
    let real = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let mut system: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        system.push(root);
    } else {
        for d in ["/sbin", "/usr/sbin", "/usr/local/sbin", "/System", "/usr/libexec"] {
            system.push(PathBuf::from(d));
        }
    }
    system
        .into_iter()
        .map(|d| d.canonicalize().unwrap_or(d))
        .filter(|d| !is_shared_bin(d))
        .any(|d| real.starts_with(&d))
}

fn program_denied(program: &str) -> Option<String> {
    let low = program.to_ascii_lowercase();
    let stem = low
        .strip_suffix(".exe")
        .or_else(|| low.strip_suffix(".com"))
        .unwrap_or(&low);
    if PROGRAM_DENY.iter().any(|d| *d == stem) {
        return Some(m!("program_never_run", p = program));
    }
    deny_hit(program).map(|d| m!("denied", d = d))
}

/// Programs the user pointed at for this incident, because they were not on the
/// search path. Same lifetime as the project root: granted for one diagnosis,
/// gone with it, never a remembered default.
static PROGRAM_PATHS: std::sync::Mutex<Vec<(String, PathBuf)>> = std::sync::Mutex::new(Vec::new());

fn granted_program(program: &str) -> Option<PathBuf> {
    PROGRAM_PATHS
        .lock()
        .ok()?
        .iter()
        .find(|(p, _)| p.eq_ignore_ascii_case(program))
        .map(|(_, path)| path.clone())
}

/// The user says where a program is, because it was not on the search path.
///
/// A file, or the directory holding it — the directory is looked into for this
/// one name and nothing else. The file has to *be* that program: its name must
/// be the one the publisher asked about, so a question about `engram` cannot be
/// answered by running whatever the user happened to point at. The same deny
/// list and the same system-directory refusal apply as on the search path.
pub fn grant_program_path(program: &str, given: &str) -> Result<PathBuf, String> {
    let re = regex::Regex::new(&format!("^(?:{PROGRAM_NAME})$")).map_err(|e| e.to_string())?;
    if !re.is_match(program) {
        return Err(m!("not_a_program_name", p = format!("{program:?}")));
    }
    if let Some(why) = program_denied(program) {
        return Err(why);
    }
    let given = given.trim().trim_matches('"');
    if given.is_empty() {
        return Err(m!("no_path_given"));
    }
    let p = PathBuf::from(given);
    let file = if p.is_dir() {
        program_file_names(program)
            .into_iter()
            .map(|n| p.join(n))
            .find(|c| is_runnable_file(c))
            .ok_or_else(|| m!("no_program_in_folder", dir = p.display(), p = program))?
    } else if is_runnable_file(&p) {
        p
    } else {
        return Err(m!("not_executable", p = p.display()));
    };

    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !program_file_names(program).iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return Err(m!("wrong_program_name", name = name, p = program));
    }
    if let Some(d) = deny_hit(&file.to_string_lossy()) {
        return Err(m!("denied_even_with_consent", d = d));
    }
    if in_system_directory(&file) {
        return Err(m!("system_program", p = file.display()));
    }
    let real = file.canonicalize().unwrap_or(file);
    if let Ok(mut g) = PROGRAM_PATHS.lock() {
        g.retain(|(p, _)| !p.eq_ignore_ascii_case(program));
        g.push((program.to_string(), real.clone()));
    }
    Ok(real)
}

/// A path as a person would write it. `canonicalize` on Windows returns the
/// verbatim form, `\\?\G:\…`, and that is what the window showed as the place a
/// program was read from — correct, and not a path anybody recognises as theirs.
pub fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.starts_with("UNC\\") => format!(r"\\{}", &rest[4..]),
        Some(rest) => rest.to_string(),
        None => s.to_string(),
    }
}

/// Why a location the user gave was not accepted, as a word the window can put
/// in the user's language. Sorted by which message it is, never by its wording.
pub fn location_error_kind(e: &str) -> &'static str {
    use crate::msg::is;
    if is("wrong_program_name", e) {
        "wrong_name"
    } else if is("system_program", e) {
        "system"
    } else if is("denied", e) || is("denied_even_with_consent", e) || is("program_never_run", e) {
        "denied"
    } else if is("no_path_given", e) {
        "empty"
    } else if is("no_program_in_folder", e) {
        "not_in_folder"
    } else if is("not_executable", e) {
        "not_executable"
    } else {
        "invalid"
    }
}

/// Where a program would be run from, if it would be run at all. The user's own
/// answer outranks the search path: they were asked because the search path
/// did not have it, or had the wrong one.
pub fn locate_program(program: &str) -> Option<PathBuf> {
    granted_program(program).or_else(|| find_on_path(program))
}

/// End of the incident: every grant made for it goes.
///
/// `set_project_root` was documented and tested as per incident while nothing
/// ever cleared it except the user emptying a text field — so a directory
/// granted for one diagnosis stayed readable for every diagnosis after it,
/// which is a wider default root arrived at by accident. A new question is a
/// new incident, and the window calls this when one begins.
pub fn end_incident() {
    set_project_root(None);
    if let Ok(mut g) = PROGRAM_PATHS.lock() {
        g.clear();
    }
    if let Ok(mut c) = PATH_CACHE.lock() {
        c.clear();
    }
}

/// Run one program with fixed arguments, bounded in time and in output, and
/// hand back what it printed. No shell and no input.
///
/// The environment is **inherited**, and that was a decision rather than an
/// oversight. The first version cleared it, which is tidy and wrong: half of
/// the programs a project ships are reached through a version manager's shim —
/// rustup, pyenv, nvm, asdf, mise — and a shim with its environment taken away
/// cannot find the program it stands for. The reading would then be "no
/// version", about a program that is plainly installed. Nothing of the
/// environment leaves: only the version token comes back from here.
pub(crate) fn run_bounded(exe: &Path, args: &[&str]) -> Option<(bool, String)> {
    let (ok, out, err) = run_bounded_split(exe, args)?;
    let mut text = out;
    if !err.is_empty() {
        text.push('\n');
        text.push_str(&err);
    }
    Some((ok, text))
}

/// A directory nothing is in, for the program to be started from.
///
/// The working directory was inherited, which for the window is wherever it
/// was launched from and for the suite is the crate — and a directory is an
/// argument to half the programs on the deny list: a package runner reads
/// its `package.json`, a build tool its `Makefile`, an interpreter its
/// `.pth` files. Started from an empty directory of its own, a program that
/// reads its surroundings reads nothing.
fn scratch_dir() -> Option<PathBuf> {
    for _ in 0..3 {
        let nonce: u64 = rand::random();
        let dir = std::env::temp_dir().join(format!("podshl-run-{}-{nonce:016x}", std::process::id()));
        // `create_dir`, not `create_dir_all`: a directory that already exists
        // is one somebody else made, and its contents are theirs.
        if std::fs::create_dir(&dir).is_ok() {
            return Some(dir);
        }
    }
    None
}

/// `run_bounded` with the two streams kept apart. A reading is what the
/// program printed on its standard output; what it printed on standard error
/// is a complaint, and a version is not found in a complaint.
pub(crate) fn run_bounded_split(exe: &Path, args: &[&str]) -> Option<(bool, String, String)> {
    let scratch = scratch_dir()?;
    let result = run_in(exe, args, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn run_in(exe: &Path, args: &[&str], cwd: &Path) -> Option<(bool, String, String)> {
    use std::sync::mpsc;

    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // English, and the same English every time: a version is found by reading
    // the output, and a translated "Version" is a different string.
    cmd.env("LC_ALL", "C").env("LANG", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up behind the consent screen.
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own session, so that the whole process group can be ended when
        // the limit is reached rather than only the process that was started.
        // A version manager's shim starts the real program as a child, and
        // killing the shim alone left the child running with our pipes.
        //
        // SAFETY: `setsid` is async-signal-safe and touches nothing shared
        // with the parent after the fork.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn().ok()?;

    // Readers on threads, reporting through a channel rather than being
    // joined: a program that started something of its own and exited leaves
    // that something holding the pipe, and a join would wait on it for as
    // long as it lived. Windows has no process group to end, so the deadline
    // is what bounds the wait there. The thread ends by itself when the last
    // holder of the pipe does.
    fn drain(r: Option<Box<dyn Read + Send>>) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let _ = r.take(OUTPUT_LIMIT).read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    }
    let out = drain(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));

    let began = Instant::now();
    let deadline = began + RUN_LIMIT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                end_process_tree(&mut child);
                break None;
            }
        }
    };
    let status = status?;
    let remaining = |until: Instant| until.saturating_duration_since(Instant::now());
    // Exited, but its output is still not complete by the deadline: something
    // it started is still writing, and a reading from a program that did not
    // finish is not a reading.
    let Ok(o) = out.recv_timeout(remaining(deadline)) else { return None };
    let Ok(e) = err.recv_timeout(remaining(deadline)) else { return None };
    Some((
        status.success(),
        String::from_utf8_lossy(&o).to_string(),
        String::from_utf8_lossy(&e).to_string(),
    ))
}

/// Stop the program and whatever it started, as far as the platform allows.
fn end_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The whole session made by `setsid` above, so that a shim's real
        // program goes with it.
        //
        // SAFETY: a signal to a process group this process created; the id is
        // that of a child not yet waited on, so it cannot have been reused.
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The version a program printed, and nothing else it printed.
///
/// Only the token travels: whatever else a program says about itself — a build
/// path, a user name baked in at compile time, a licence banner — stays here.
/// Looked for on the first few lines; a program that did not exit cleanly is
/// believed only on a line that names it, because an error message is full of
/// numbers that are not versions.
pub fn version_token(text: &str, program: &str, clean_exit: bool) -> Option<String> {
    let re = regex::Regex::new(
        r"(?:^|[^0-9A-Za-z.])v?(\d{1,6}\.\d{1,6}(?:\.\d{1,9}){0,2}(?:[-+][0-9A-Za-z][0-9A-Za-z.-]{0,30})?)",
    )
    .ok()?;
    let stem = program
        .to_ascii_lowercase()
        .trim_end_matches(".exe")
        .trim_end_matches(".com")
        .to_string();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()).take(5) {
        if !clean_exit && !line.to_ascii_lowercase().contains(&stem) {
            continue;
        }
        if let Some(c) = re.captures(line) {
            let v = c.get(1)?.as_str().trim_end_matches(['.', '-']);
            return Some(v.chars().take(40).collect());
        }
    }
    None
}

/// Which version of a program is installed, by asking it.
fn program_version(program: &str, flag: &str) -> Option<String> {
    let exe = locate_program(program)?;
    if in_system_directory(&exe) {
        return None;
    }
    let (ok, text) = run_bounded(&exe, &[flag])?;
    version_token(&text, program, ok)
}

/// A reference Docker prints, reduced to the repository the publisher would
/// have named and the tag. `docker.io/library/ollama:0.3` and `ollama:0.3` are
/// the same image, and a digest is not a version.
fn split_image(reference: &str) -> (String, Option<String>) {
    let no_digest = reference.split('@').next().unwrap_or(reference);
    let (repo, tag) = match no_digest.rfind(':') {
        Some(i) if !no_digest[i..].contains('/') => (&no_digest[..i], Some(no_digest[i + 1..].to_string())),
        _ => (no_digest, None),
    };
    let mut repo = repo.to_ascii_lowercase();
    for prefix in ["docker.io/", "index.docker.io/", "registry-1.docker.io/"] {
        if let Some(r) = repo.strip_prefix(prefix) {
            repo = r.to_string();
        }
    }
    if let Some(r) = repo.strip_prefix("library/") {
        repo = r.to_string();
    }
    (repo, tag)
}

fn normalise_image(image: &str) -> String {
    split_image(image).0
}

/// The running containers of one image, as Docker lists them: `(id, reference)`.
///
/// `docker ps` with a format the client wrote, and nothing else — no `exec`,
/// nothing run inside a container, no container named by the publisher. They
/// name an image; which containers run it is Docker's answer.
pub fn containers_of(image: &str) -> Vec<(String, String)> {
    let Some(docker) = find_on_path("docker") else { return vec![] };
    let Some((true, text)) = run_bounded(&docker, &["ps", "--no-trunc", "--format", "{{.ID}}\t{{.Image}}"]) else {
        return vec![];
    };
    let want = normalise_image(image);
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, r)| normalise_image(r.trim()) == want)
        .map(|(id, r)| (id.trim().to_string(), r.trim().to_string()))
        .filter(|(id, _)| id.chars().all(|c| c.is_ascii_hexdigit()))
        .collect()
}

/// Which version of an image is running in a container on this machine.
///
/// The tag, if the tag is a version. `latest` says nothing, so then the image's
/// own `org.opencontainers.image.version` label — metadata Docker holds about
/// the image, read without entering the container.
fn container_image_version(image: &str) -> Option<String> {
    let (id, reference) = containers_of(image).into_iter().next()?;
    let (_, tag) = split_image(&reference);
    if let Some(v) = tag.as_deref().and_then(|t| version_token(t, "", true)) {
        return Some(v);
    }
    let docker = find_on_path("docker")?;
    let label = r#"{{index .Config.Labels "org.opencontainers.image.version"}}"#;
    if let Some((true, text)) = run_bounded(&docker, &["inspect", "--format", label, &id]) {
        if let Some(v) = version_token(&text, "", true) {
            return Some(v);
        }
    }
    // A tag that is not a version is still an answer about which image this is
    // — `latest`, `rocm` — and a bounded one. A reference with no tag at all
    // *is* `latest` to Docker, and saying so still says the thing a publisher
    // cannot otherwise learn: that it runs in a container here at all.
    tag.or_else(|| Some("latest".into()))
        .filter(|t| !t.is_empty() && t.len() <= 64 && t.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
}

/// The path with every symlink in it resolved, as far as it exists.
///
/// `canonicalize` alone is not enough because the file being read may legally
/// not be there — an absent configuration file is a reading that yields
/// nothing, not a refusal. So the deepest ancestor that does exist is resolved
/// and the rest is re-attached, which is enough to see a symlinked directory in
/// the middle of the path.
fn resolved_for_check(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    while let Some(parent) = cur.parent().map(|x| x.to_path_buf()) {
        if let Some(name) = cur.file_name() {
            suffix.push(name.to_os_string());
        }
        if let Ok(c) = parent.canonicalize() {
            let mut out = c;
            for name in suffix.iter().rev() {
                out.push(name);
            }
            return out;
        }
        if parent.as_os_str().is_empty() {
            break;
        }
        cur = parent;
    }
    p.to_path_buf()
}

pub(crate) fn deny_hit(s: &str) -> Option<&'static str> {
    let low = s.to_lowercase();
    DENY.iter().find(|d| low.contains(&d.to_lowercase())).copied()
}

/// The file, whole, if it is a file a settings read should open at all.
/// Larger than `FILE_LIMIT` is not a configuration file, and it is not read
/// in part either — a read that returns the first megabyte of something is a
/// read of something else.
fn read_bounded(p: &Path) -> Option<String> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return None;
    }
    std::fs::read_to_string(p).ok()
}

/// A value a key read may hand back: a string of bounded length, a number,
/// or a boolean. An object or an array under a key is a document, and a
/// string longer than `VALUE_LIMIT` is a paragraph — free text, which no key
/// read was consented to as.
fn scalar(v: &Value) -> Option<Value> {
    match v {
        Value::String(s) if s.chars().count() <= VALUE_LIMIT => Some(v.clone()),
        Value::Number(_) | Value::Bool(_) => Some(v.clone()),
        _ => None,
    }
}

/// `reg.exe`, which is the one program the registry reads run. On the search
/// path in any ordinary session; under the system root where the path was
/// cut down.
#[cfg(target_os = "windows")]
fn reg_exe() -> Option<PathBuf> {
    find_on_path("reg").or_else(|| {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let p = root.join("System32").join("reg.exe");
        is_runnable_file(&p).then_some(p)
    })
}

/// Where file reads are permitted: application configuration areas only.
/// The project the user is asking about, granted for this incident only.
///
/// `python3 --version` reads the interpreter on *this process's* PATH, and a
/// desktop application does not inherit an activated virtualenv — so for a
/// project in `.venv`, pyenv, conda, uv or a container that reading is about an
/// interpreter the project never uses, and a solution matching on it is
/// confidently wrong. Being wrong is worse than being silent.
///
/// The fix is not a wider default root. It is the user pointing at the
/// directory they mean, which is itself a consent act, and which is gone when
/// the diagnosis is.
static PROJECT_ROOT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub fn set_project_root(p: Option<PathBuf>) {
    if let Ok(mut g) = PROJECT_ROOT.lock() {
        *g = p;
    }
}

pub fn project_root() -> Option<PathBuf> {
    PROJECT_ROOT.lock().ok().and_then(|g| g.clone())
}

/// The file a `read_file_key` or `read_ini_key` names, as this machine sees it.
///
/// An absolute path is itself; a relative one is relative to the project the
/// user granted for this incident, and without one it names nothing. It used to
/// be handed to the filesystem as written, which resolves it against the
/// *process's* working directory — so the catalogue's own
/// `.venv/pyvenv.cfg`, the reading `EN1` exists for, looked for a virtualenv in
/// whatever directory the client happened to be started from, found none, and
/// was quietly dropped from the menu as "not readable here". Granting the
/// project changed nothing, because nothing joined the two.
fn file_target(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        project_root().map(|r| r.join(p))
    }
}

fn roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = dirs::config_dir() {
        v.push(p);
    }
    if let Some(p) = dirs::data_dir() {
        v.push(p);
    }
    // Granted per incident, never a default.
    if let Some(p) = project_root() {
        v.push(p);
    }
    if let Some(p) = extra_root() {
        v.push(p);
    }
    v
}

/// A development root, for the suite's own fixtures — and **only in a build
/// that is not a release**.
///
/// It was read unconditionally, so a shipped binary took one environment
/// variable as a new root on the allow-list: anything able to set it for the
/// process — a launcher, a desktop file, a wrapper script — widened what every
/// publisher's read instruction could reach, with no consent screen involved.
/// A release build does not read it at all now, which is the only version of
/// "development only" that the binary can enforce.
fn extra_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        std::env::var("VS_EXTRA_ROOT").ok().map(PathBuf::from)
    } else {
        None
    }
}

/// Which container runtime this process is inside, if any.
///
/// Two different failures hide behind one answer. If the *user's* code runs in
/// a container, host readings are about the wrong machine and a solution keyed
/// on the host's libraries is meaningless. If the *client* runs in one, every
/// reading is about the container and the whole diagnosis is worthless. Either
/// way it has to be said rather than guessed around — and it is read from
/// files, so it costs no execution.
fn container_runtime() -> String {
    if Path::new("/.dockerenv").exists() {
        return "docker".into();
    }
    if Path::new("/run/.containerenv").exists() {
        return "podman".into();
    }
    if let Ok(cg) = std::fs::read_to_string("/proc/1/cgroup") {
        for (marker, name) in [("docker", "docker"), ("containerd", "containerd"),
                               ("kubepods", "kubernetes"), ("lxc", "lxc")] {
            if cg.contains(marker) {
                return name.into();
            }
        }
    }
    "none".into()
}

/// The first `major.minor` in a string, so `Python 3.12.14` and `3.11.9` — the
/// shapes `python3 --version` and `pyvenv.cfg` actually produce — compare.
fn version_pair(s: &str) -> Option<(u32, u32)> {
    // Indexed and sliced on the *same* sequence. It used to walk a `Vec<char>`
    // and then slice the original `&str` with those indices, which are byte
    // offsets — so anything multi-byte before the digits either panicked on a
    // char boundary or quietly cut the number in the wrong place. With
    // `panic = "abort"` in the release profile the first of those takes the
    // whole client down rather than failing one reading, and the input is a
    // version string from a vendor-influenced read or from any `pyvenv.cfg` on
    // disk.
    let chars: Vec<char> = s.chars().collect();
    let number = |from: usize, to: usize| -> Option<u32> {
        chars[from..to].iter().collect::<String>().parse().ok()
    };
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                let major = number(start, i)?;
                i += 1;
                let m0 = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i > m0 {
                    return Some((major, number(m0, i)?));
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Does the interpreter on this machine's PATH disagree with the one the
/// project actually uses?
///
/// **This resolves nothing.** Where the two differ, neither is obviously the
/// one the user means: a solution keyed on the host version would be about an
/// interpreter their project never runs, and one keyed on the venv would be
/// wrong if they are asking about the system install. The design's rule for
/// exactly this situation is already written down — *ask rather than guess*,
/// and *what is withheld becomes a question rather than a dead end* — so this
/// returns the makings of a question and never a decision.
///
/// Silent when they agree, and silent when only one is known: a single reading
/// is not a contradiction.
pub fn interpreter_conflict(facts: &Value) -> Option<Value> {
    let get = |k: &str| facts.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let host = get("python.version")?;
    let project = get("python.venv.version")?;
    let (a, b) = (version_pair(&host)?, version_pair(&project)?);
    if a == b {
        return None;
    }
    Some(json!({
        "fact": "python.version",
        "host": host,
        "project": project,
        // The options a person can actually answer, and no default among them.
        // Ids, not sentences: the answer is recorded, and a value that is
        // "das Projekt (.venv)" for one person and "the project (.venv)" for
        // the next splits one situation into two by the language of the
        // window. The window says them in the user's language.
        "choices": ["project", "system", "unknown"]
    }))
}

/// A human sentence for the consent screen. This is what the user actually
/// decides on, so it names the concrete target rather than the capability.
pub fn describe(read: &Value) -> Result<(String, Risk), String> {
    let op = read.get("op").and_then(|v| v.as_str()).ok_or_else(|| m!("read_no_op"))?;
    let g = |k: &str| read.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(match op {
        "os_fact" => (m!("what_os_fact", name = g("name")), Risk::Low),
        "env_var" => (m!("what_env_var", name = g("name")), Risk::Low),
        // With the arguments. Without them all six `nvidia-smi` readings — the
        // card's name, its driver version, its memory, its temperature —
        // rendered one identical sentence, so the consent screen could not tell
        // the user which of them they were approving. The user decides on this
        // text; it has to name the concrete thing.
        "run_tool" => {
            let args: Vec<String> = read
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let what = if args.is_empty() {
                m!("what_run_tool_bare", tool = g("tool"))
            } else {
                m!("what_run_tool", tool = g("tool"), args = args.join(" "))
            };
            (what, Risk::Medium)
        }
        "read_file_key" => (
            m!("what_read_file_key", key = g("key"), path = g("path")),
            Risk::Medium,
        ),
        // `pyvenv.cfg` states the interpreter version as a plain key. Reading
        // it answers what `python3 --version` would have said for *that*
        // environment, with no execution at all — which is a strictly better
        // trade than putting `python3 -c` on an allow list.
        "read_ini_key" => (
            m!("what_read_ini_key", key = g("key"), path = g("path")),
            Risk::Medium,
        ),
        "read_registry" => (
            m!("what_read_registry", path = g("path"), name = g("name")),
            Risk::Medium,
        ),
        // The resolved file, not the name. The user is agreeing to one program
        // being started, and which file that is on their machine is the thing
        // they can check — a name on its own could be anything on the path.
        "program_version" => {
            let flag = if g("flag").is_empty() { "--version".to_string() } else { g("flag") };
            let what = match locate_program(&g("program")) {
                Some(p) => m!("what_program_version", path = display_path(&p), flag = flag),
                None => m!("what_program_version_absent", program = g("program"), flag = flag),
            };
            (what, Risk::Medium)
        }
        "container_image_version" => (m!("what_container_image_version", image = g("image")), Risk::Low),
        // The version-tree case: a traversal, but a bounded and fully
        // enumerable one — the client can state exactly what will be touched
        // before anything runs, which arbitrary code never permits.
        "enumerate_read" => (
            m!(
                "what_enumerate_read",
                root = g("root"),
                glob = g("glob"),
                keys = read.get("keys").and_then(|k| k.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default(),
                max = MAX_ENTRIES
            ),
            Risk::Medium,
        ),
        other => return Err(m!("read_op_unknown", op = format!("{other:?}"))),
    })
}

/// Reject before the consent screen is even shown, so a user is never asked to
/// approve something that would be refused anyway.
pub fn precheck(read: &Value) -> Result<(), String> {
    let op = read.get("op").and_then(|v| v.as_str()).ok_or_else(|| m!("read_no_op"))?;
    let g = |k: &str| read.get(k).and_then(|v| v.as_str()).unwrap_or("");

    // ASCII only, in every field the deny list screens. `deny_hit` folds case
    // and nothing else, so a Cyrillic `ѕeсret` is a different string to it and
    // went through while `secret` was refused. The server ships a UTS 39 table
    // for exactly that evasion on hostnames; rather than carry one here too,
    // these fields are held to ASCII — a configuration key or an environment
    // variable outside it is rare enough that saying so beats normalising and
    // hoping.
    for field in ["name", "path", "glob", "key", "program", "flag", "image"] {
        let value = g(field);
        if !value.is_ascii() {
            return Err(m!("not_ascii", field = field, v = format!("{value:?}")));
        }
    }
    for k in read.get("keys").and_then(|v| v.as_array()).into_iter().flatten() {
        let value = k.as_str().unwrap_or("");
        if !value.is_ascii() {
            return Err(m!("key_not_ascii", v = format!("{value:?}")));
        }
    }

    match op {
        "os_fact" => {
            if let Some(d) = deny_hit(g("name")) {
                return Err(m!("denied", d = d));
            }
        }
        // An allow list, not the deny list. The deny list is checked on the
        // name too, and it was the only check — so `HOME`, `PATH`, `SSH_AUTH_
        // SOCK` and every variable somebody exported a credential under
        // without a listed word in its name went through. The names a
        // diagnosis has needed are twelve, and they are written down.
        "env_var" => {
            let name = g("name");
            if !ENV_ALLOW.contains(&name) {
                return Err(m!("env_var_not_allowed", name = format!("{name:?}"),
                              allowed = ENV_ALLOW.join(", ")));
            }
        }
        "run_tool" => {
            let tool = g("tool");
            let (_, pat) = TOOLS
                .iter()
                .find(|(t, _)| *t == tool)
                .ok_or_else(|| {
                    let known: Vec<&str> = TOOLS.iter().map(|(t, _)| *t).collect();
                    m!("tool_not_allowed", tool = format!("{tool:?}"), known = format!("{known:?}"))
                })?;
            if !tool_available(tool) {
                return Err(m!("tool_absent", tool = tool));
            }
            let re = regex::Regex::new(&format!("^(?:{pat})$")).map_err(|e| e.to_string())?;
            let identifying = regex::Regex::new(GPU_FIELD_DENY).map_err(|e| e.to_string())?;
            for a in read.get("args").and_then(|v| v.as_array()).into_iter().flatten() {
                let a = a.as_str().unwrap_or("");
                if !re.is_match(a) {
                    return Err(m!("arg_not_allowed", a = format!("{a:?}"), tool = tool));
                }
                // The pattern admits any field name, because the useful ones
                // are many; the identifying ones are few and named.
                if let Some(fields) = a.strip_prefix("--query-gpu=") {
                    if let Some(f) = fields.split(',').find(|f| identifying.is_match(f)) {
                        return Err(m!("gpu_field_identifying", field = f));
                    }
                }
            }
        }
        "read_file_key" | "read_ini_key" | "read_registry" => {
            // `key` belongs in here. Without it the deny list reads the file
            // name and not the field being taken out of it, which stops
            // `credentials.json` and permits the `api_secret` key of anything
            // else.
            // Separated. Concatenated, a path ending "coo" beside a key
            // "kies" reads as "cookies" and refuses a read nobody asked for.
            let target = format!("{} {} {}", g("path"), g("name"), g("key"));
            if let Some(d) = deny_hit(&target) {
                return Err(m!("denied_even_with_consent", d = d));
            }
            if op == "read_registry" {
                // Anchored, and both fields. The read no longer builds a command
                // string, so this is defence in depth rather than the defence —
                // but a path that cannot hold a quote, a semicolon or a newline
                // stays harmless if some later reader reaches for a shell again.
                for (field, pat) in [("path", REGISTRY_PATH), ("name", REGISTRY_NAME)] {
                    let re = regex::Regex::new(&format!("^(?:{pat})$"))
                        .map_err(|e| e.to_string())?;
                    if !re.is_match(g(field)) {
                        return Err(m!("registry_field_not_allowed", field = field,
                                      v = format!("{:?}", g(field))));
                    }
                }
                // `MachineGuid`, `ProductId`, `RegisteredOwner`: values that
                // say which machine this is, not what is installed on it.
                let identifying = regex::Regex::new(REGISTRY_NAME_DENY).map_err(|e| e.to_string())?;
                if identifying.is_match(g("name")) {
                    return Err(m!("registry_name_identifying", name = g("name")));
                }
            }
            if op == "read_file_key" || op == "read_ini_key" {
                let p = Path::new(g("path"));
                // Before the root check, not after. `Path::starts_with` is a
                // component-wise prefix test and it does not normalise, so
                // `<config>/../../.npmrc` starts with `<config>` and walks out
                // of it — the check would pass and the file read would happen
                // outside every granted root. `enumerate_read` has refused
                // `..` since it was written; these two never did.
                if p.components().any(|c| c == Component::ParentDir) {
                    return Err(m!("path_backstep", p = g("path")));
                }
                // A relative path is relative to the project the user granted,
                // and to nothing else — not to wherever this process happens to
                // have been started.
                let Some(p) = file_target(g("path")) else {
                    return Err(m!("path_relative_no_project", p = g("path")));
                };
                // Both sides resolved. A prefix test compares the names it
                // was given, so a symlink inside a granted root reads as being
                // inside it while pointing anywhere at all — and unlike `..`,
                // there is nothing in the written path to notice. `actions.rs`
                // has canonicalised both sides since it was written; this is the
                // same rule one level down.
                let real = resolved_for_check(&p);
                let inside = roots()
                    .iter()
                    .filter_map(|r| r.canonicalize().ok())
                    .any(|r| real.starts_with(&r));
                if !inside {
                    return Err(m!("path_outside", p = g("path")));
                }
            }
        }
        "program_version" => {
            let program = g("program");
            let re = regex::Regex::new(&format!("^(?:{PROGRAM_NAME})$")).map_err(|e| e.to_string())?;
            if !re.is_match(program) {
                return Err(m!("program_not_allowed", p = format!("{program:?}")));
            }
            if let Some(why) = program_denied(program) {
                return Err(why);
            }
            let flag = g("flag");
            if !flag.is_empty() && !VERSION_FLAGS.contains(&flag) {
                return Err(m!("flag_not_allowed", flag = format!("{flag:?}"),
                              allowed = format!("{VERSION_FLAGS:?}")));
            }
            // Absent is not refused: that is the case the user is asked about.
            // Present in a system directory is.
            if let Some(p) = locate_program(program) {
                if in_system_directory(&p) {
                    return Err(m!("system_program", p = p.display()));
                }
            }
        }
        "container_image_version" => {
            let image = g("image");
            let re = regex::Regex::new(&format!("^(?:{IMAGE_NAME})$")).map_err(|e| e.to_string())?;
            if image.len() > 128 || !re.is_match(image) {
                return Err(m!("image_not_allowed", image = format!("{image:?}")));
            }
            if !tool_available("docker") {
                return Err(m!("tool_absent", tool = "docker"));
            }
        }
        "enumerate_read" => {
            let glob = g("glob");
            if glob.contains("..") || glob.starts_with('/') {
                return Err(m!("glob_path_change"));
            }
            if let Some(d) = deny_hit(glob) {
                return Err(m!("denied", d = d));
            }
            if resolve_root(g("root")).is_none() {
                return Err(m!("root_unknown", root = format!("{:?}", g("root"))));
            }
            for k in read.get("keys").and_then(|v| v.as_array()).into_iter().flatten() {
                if let Some(d) = deny_hit(k.as_str().unwrap_or("")) {
                    return Err(m!("key_denied", d = d));
                }
            }
        }
        other => return Err(m!("read_op_unknown", op = format!("{other:?}"))),
    }
    Ok(())
}

/// Named roots only. A vendor never states a filesystem path for enumeration —
/// it names an area, and the client decides where that is on this platform.
fn resolve_root(name: &str) -> Option<PathBuf> {
    match name {
        "config" => dirs::config_dir(),
        "data" => dirs::data_dir(),
        "dev" => extra_root(),
        _ => None,
    }
}

/// Walk `root` to a bounded depth, matching a simple `dir/file` pattern, and
/// read the named keys out of each JSON file found. Enough for a version tree;
/// deliberately not enough to be a general file crawler.
fn enumerate_read(read: &Value) -> Option<Value> {
    let g = |k: &str| read.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let root = resolve_root(g("root"))?;
    let glob = g("glob");

    // **The directory part of the glob is honoured, not discarded.** Only the
    // text after the last `/` was kept, so `telemetry-cache/*.json` became
    // `*.json` and the walk covered the whole root — while `describe()` showed
    // the user the full pattern and implied it was confined to that directory.
    // They approved a scoped search and got a root-wide sweep of every other
    // application's files.
    //
    // Each directory segment is matched with the same simple matcher as the
    // file part, so `*/manifest.json` keeps working: a segment pattern must
    // match the directory at that depth for the walk to go on into it.
    let mut segments: Vec<String> = glob.split('/').map(String::from).collect();
    let file_pat = segments.pop()?;
    let dir_pats: Vec<String> = segments.into_iter().filter(|s| !s.is_empty()).collect();
    let keys: Vec<String> = read.get("keys").and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut out = Vec::new();
    let mut stack = vec![(root.clone(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        // `continue`, not `break`. The stack is last-in-first-out, so an
        // over-deep branch is popped while shallow siblings are still queued —
        // and ending the whole walk there returned a silently incomplete result
        // that looked like "there is nothing here". Only the entry cap ends it,
        // because that one really is the end.
        if depth > MAX_DEPTH {
            continue;
        }
        if out.len() >= MAX_ENTRIES {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        // Sorted. `read_dir` yields whatever order the filesystem feels like,
        // so the same machine could return the same readings in a different
        // order on two runs, and — worse — a case about *which* branch the walk
        // reaches first could not be written at all. The entry cap makes the
        // order load-bearing: which entries are dropped when it is hit should
        // not be a property of the filesystem.
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= MAX_ENTRIES {
                break;
            }
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if deny_hit(&name).is_some() {
                continue;                     // never descend into denied areas
            }
            // `is_dir()` follows symlinks, so a link inside a granted root led
            // the walk anywhere the link pointed — a dotfile manager pointing
            // `~/.config/app/cache` at `~/Documents` was enough, and the deny
            // list only ever saw the link's own harmless name. The root is a
            // bound on what may be read, and a link is not an exception to it.
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            if path.is_dir() {
                // Descend only where the glob's own directory pattern for this
                // depth allows it. A glob with no directory part descends
                // freely, which is what it always did.
                let allowed = match dir_pats.get(depth) {
                    Some(pat) => matches_simple(&name, pat),
                    None => dir_pats.is_empty(),
                };
                if allowed {
                    stack.push((path, depth + 1));
                }
            } else if depth == dir_pats.len() && matches_simple(&name, &file_pat) {
                if let Some(raw) = read_bounded(&path) {
                    if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                        let mut row = serde_json::Map::new();
                        for k in &keys {
                            // A scalar or nothing: a key whose value is an
                            // object is a whole document under one name.
                            if let Some(val) = v.get(k).and_then(scalar) {
                                row.insert(k.clone(), val);
                            }
                        }
                        if !row.is_empty() {
                            out.push(Value::Object(row));
                        }
                    }
                }
            }
        }
    }
    Some(json!(out))
}

/// `*` wildcards only — no regex from a remote party.
fn matches_simple(name: &str, pat: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return name == pat;
    }
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match name[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false;
                }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    parts.last().map(|l| l.is_empty() || name.ends_with(l)).unwrap_or(true)
}

/// Only ever called after `precheck` passed *and* the user consented.
pub fn perform(read: &Value) -> Option<Value> {
    let op = read.get("op")?.as_str()?;
    let g = |k: &str| read.get(k).and_then(|v| v.as_str()).unwrap_or("");
    match op {
        "os_fact" => match g("name") {
            "os" => Some(json!(std::env::consts::OS)),
            "arch" => Some(json!(std::env::consts::ARCH)),
            "version" => os_version().map(Value::String),
            "container" => Some(json!(container_runtime())),
            _ => None,
        },
        // The gate holds here as well: `perform` is its own command, and a
        // value that `precheck` would refuse must not be readable by calling
        // this directly.
        "env_var" if ENV_ALLOW.contains(&g("name")) => std::env::var(g("name")).ok().map(Value::String),
        "env_var" => None,
        "run_tool" => {
            let args: Vec<String> = read
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            // The file the gate resolved, run the way every other program is
            // run here: bounded in time and output, from an empty directory.
            // `Command::new(name)` resolved the name again by its own rules —
            // through `PATHEXT`, so a `.cmd` could answer for `nvidia-smi` —
            // and waited on it for ever.
            let exe = find_on_path(g("tool"))?;
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let (ok, out, _) = run_bounded_split(&exe, &args)?;
            if !ok {
                return None;
            }
            shape_tool_output_for(g("tool"), &args, out.trim()).map(Value::String)
        }
        "read_file_key" => {
            let raw = read_bounded(&file_target(g("path"))?)?;
            let v: Value = serde_json::from_str(&raw).ok()?;
            scalar(v.get(g("key"))?)
        }
        // `key = value`, one per line, `#` and `;` are comments. Deliberately
        // not a general INI parser: no sections, no continuations, no includes.
        // A parser that can follow an include can be pointed somewhere else.
        "read_ini_key" => {
            let raw = read_bounded(&file_target(g("path"))?)?;
            let want = g("key");
            for line in raw.lines().take(500) {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    if k.trim() == want {
                        return scalar(&json!(v.trim()));
                    }
                }
            }
            None
        }
        // No command string. This used to interpolate both parameters into a
        // PowerShell `-Command`, single-quoted, unescaped — so a publisher
        // writing `HKCU:\X'; <anything>; '` closed the quote and the rest ran.
        // Arbitrary code, behind a consent screen that said "read a registry
        // value", in the one file whose header says "run this command" is not a
        // capability we hand to a remote party.
        //
        // `reg.exe` takes an argv, so there is no shell to escape out of and no
        // quoting rule to get right. The gate's patterns are the second line;
        // this is the first, and it holds on its own.
        #[cfg(target_os = "windows")]
        "read_registry" => {
            // `reg.exe` spells a hive `HKCU\...`; the PowerShell provider spells
            // it `HKCU:\...`. Accept what the vocabulary accepts and normalise.
            let key = g("path").replacen(':', "", 1);
            let want = g("name");
            // The gate again, for a direct call.
            let identifying = regex::Regex::new(REGISTRY_NAME_DENY).ok()?;
            if identifying.is_match(want) || deny_hit(want).is_some() {
                return None;
            }
            let (ok, text, _) = run_bounded_split(&reg_exe()?, &["query", &key, "/v", want])?;
            if !ok {
                return None;
            }
            // `    Name    REG_SZ    the value`. The value may contain spaces,
            // so the name and the type are taken as tokens and everything after
            // the type is the value — never a fixed column.
            //
            // Sliced by char count rather than by byte offset. `want` is ASCII
            // by the vocabulary's pattern but the *line* is whatever the
            // registry holds, and byte-indexing a line that opens with a
            // multi-byte character panics — which, with `panic = "abort"` in
            // the release profile, takes the whole client with it.
            let want = want.trim();
            for line in text.lines() {
                let t = line.trim();
                let mut it = t.char_indices().skip_while(|(_, c)| !c.is_whitespace());
                let split = it.next().map(|(i, _)| i).unwrap_or(t.len());
                let (name, rest) = t.split_at(split);
                if !name.eq_ignore_ascii_case(want) {
                    continue;
                }
                let rest = rest.trim_start();
                let Some((_ty, value)) = rest.split_once(char::is_whitespace) else {
                    continue;
                };
                let value = value.trim();
                if !value.is_empty() {
                    return scalar(&Value::String(value.to_string()));
                }
            }
            None
        }
        #[cfg(not(target_os = "windows"))]
        "read_registry" => None,
        "enumerate_read" => enumerate_read(read),
        "program_version" => {
            let flag = if g("flag").is_empty() { "--version" } else { g("flag") };
            program_version(g("program"), flag).map(Value::String)
        }
        "container_image_version" => container_image_version(g("image")).map(Value::String),
        _ => None,
    }
}

/// What a tool's output becomes as a reading.
///
/// It used to be the first line, always — which is right for a tool that
/// prints one value and emptied the two readings whose output is a list:
/// `pci.devices` came back as the host bridge, and `mac.displays` as the word
/// "Graphics/Displays:". Returning everything was never the fix either: a whole
/// PCI inventory is the fingerprint of a machine this vocabulary refuses `pip
/// list` for. So each multi-line tool is shaped to what its catalogue entry
/// promises — the graphics and network chips; the display chipset and
/// resolution — by a fixed filter in the client, never by the publisher.
/// `shape_tool_output`, with the arguments the tool was given.
///
/// A tool's way of saying "I do not have this" is sometimes per field. On this
/// machine `nvidia-smi --query-gpu=serial` prints `0` for an RTX 5070, which is
/// the card saying it carries no serial in firmware — the exact case
/// `warranty.rma.precheck` ships a `serial.printed` question for, and the case
/// `P2` is about. It arrived as the string "0" instead, so the fallback never
/// fired and a warranty precheck went out reading "serial number 0".
///
/// Found by walking the window on a real card. It is knowledge about the tool,
/// which is what this function is for; it cannot be a general "0 means
/// nothing" rule, because a temperature or a fan speed of zero is a reading.
pub(crate) fn shape_tool_output_for(tool: &str, args: &[&str], text: &str) -> Option<String> {
    let shaped = shape_tool_output(tool, text)?;
    if tool == "nvidia-smi" {
        let queried: Vec<&str> = args.iter()
            .filter_map(|a| a.strip_prefix("--query-gpu="))
            .flat_map(|f| f.split(','))
            .collect();
        // Only where the whole answer is one identifier: a row of several
        // fields containing a zero says nothing about the others.
        if queried.len() == 1 && queried[0].trim() == "serial" && shaped.trim() == "0" {
            return None;
        }
    }
    Some(shaped)
}

pub(crate) fn shape_tool_output(tool: &str, text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let picked: Vec<String> = match tool {
        "lspci" => {
            let re = regex::Regex::new(
                r"(?i)(VGA compatible controller|3D controller|Display controller|Ethernet controller|Network controller)",
            )
            .ok()?;
            lines
                .iter()
                .filter(|l| re.is_match(l))
                // The bus address says where the card sits, not what it is.
                .map(|l| l.split_once(' ').map(|(slot, rest)| if slot.contains(':') { rest } else { l }).unwrap_or(l).to_string())
                .take(8)
                .collect()
        }
        "system_profiler" => lines
            .iter()
            .filter(|l| {
                ["Chipset Model:", "Resolution:", "Vendor:", "Metal Support:", "Metal Family:"]
                    .iter()
                    .any(|k| l.starts_with(k))
            })
            .map(|l| l.to_string())
            .take(12)
            .collect(),
        _ => lines.first().map(|l| l.to_string()).into_iter().collect(),
    };
    let joined = picked.join("; ");
    let low = joined.to_lowercase();
    if joined.is_empty() || low == "[n/a]" || low == "n/a" {
        None
    } else {
        Some(joined)
    }
}

/// The operating system's own name and version.
///
/// Implemented only where there is an honest mechanism. Where there is not, it
/// returns `None` and the fact is reported as unreadable — never a plausible
/// constant.
fn os_version() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/etc/os-release").ok()?;
        return text.lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_string());
    }
    #[cfg(target_os = "macos")]
    {
        let exe = find_on_path("sw_vers").or_else(|| {
            let p = PathBuf::from("/usr/bin/sw_vers");
            is_runnable_file(&p).then_some(p)
        })?;
        return run_bounded_split(&exe, &["-productVersion"])
            .filter(|(ok, _, _)| *ok)
            .map(|(_, out, _)| out.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    // Windows had no mechanism and the catalogue offered the reading anyway, so
    // a model picked `os.version`, the user spent a consent click on it, and the
    // id landed silently in `missing` — the dead end the filtering exists to
    // prevent, on the one platform whose branch nobody had walked. Found by
    // `everything_the_catalogue_offers_here_actually_reads` failing on Windows
    // while passing everywhere else.
    //
    // `reg query` rather than a shell: same argv, same reason as `read_registry`.
    // `DisplayVersion` is the marketing version (24H2) and is absent on older
    // builds, where `CurrentBuild` is what there is — so both are tried and the
    // answer says which it is rather than presenting a build number as a
    // version.
    #[cfg(target_os = "windows")]
    {
        let key = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let read = |name: &str| -> Option<String> {
            let (ok, text, _) = run_bounded_split(&reg_exe()?, &["query", key, "/v", name])?;
            if !ok {
                return None;
            }
            for line in text.lines() {
                let t = line.trim();
                let mut it = t.char_indices().skip_while(|(_, c)| !c.is_whitespace());
                let split = it.next().map(|(i, _)| i).unwrap_or(t.len());
                let (found, rest) = t.split_at(split);
                if !found.eq_ignore_ascii_case(name) {
                    continue;
                }
                let (_ty, value) = rest.trim_start().split_once(char::is_whitespace)?;
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
            None
        };
        return read("DisplayVersion")
            .map(|v| format!("Windows {v}"))
            .or_else(|| read("CurrentBuild").map(|b| format!("Windows build {b}")));
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // No honest mechanism wired up yet on this platform.
        None
    }
}

/// What the client knows about itself without reading anything: the operating
/// system family and the architecture are compile-time constants.
///
/// This is the one thing detected automatically, and it does not contradict the
/// rule against ambient reading — it is a fact about the program, not about the
/// user's machine or their products. It is still shown rather than hidden, and
/// it saves the model an entire round: it never has to establish what platform
/// it is reasoning about.
pub fn baseline() -> Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

/// Derived facts the client computes from readings, so a vendor does not have
/// to be trusted to interpret raw values it did not read itself.
pub fn derive(id: &str, facts: &serde_json::Map<String, Value>) -> Option<Value> {
    match id {
        // bf16 exists in silicon from compute capability 8.0. This is a
        // hardware fact; what a *library* claims about it is the vendor's own
        // knowledge and is derived on their side, not fabricated here.
        "gpu.bf16_native" => {
            let cc = facts.get("gpu.compute_capability")?.as_str()?;
            let maj: i64 = cc.split('.').next()?.parse().ok()?;
            Some(json!(maj >= 8))
        }
        _ => None,
    }
}

/// The facts this client is able to read, as a menu.
///
/// Needed only where no vendor is involved: in the vendor path the skill states
/// what to look at, because the vendor knows its own product. Without one, the
/// client must not invent a list — reading "compute capability" for a flickering
/// screen is noise, and hardcoding three fields is the same mistake as
/// hardcoding collectors.
///
/// So the catalogue is offered to the user's model, which picks what it needs
/// for *this* question, and the user consents to that selection. The client
/// still bounds what is readable; it just stops pretending to know what is
/// relevant.
pub fn catalogue() -> Value {
    // Filtered to what this machine can actually do. Offering an NVIDIA reading
    // on an AMD or Apple machine would have the model pick it, the user consent
    // to it, and nothing come back — a dead end dressed as a diagnosis.
    let all = catalogue_all();
    let usable: Vec<Value> = all.as_array().cloned().unwrap_or_default().into_iter()
        .filter(|c| c.get("read").map(|r| precheck(r).is_ok()).unwrap_or(false))
        .collect();
    json!(usable)
}

/// How many readings the catalogue knows about at all, regardless of this
/// machine. Used to check that the available and unavailable halves account for
/// the whole of it — a reading in neither has been dropped silently.
#[cfg(test)]
pub fn catalogue_all_len() -> usize {
    catalogue_all().as_array().map(|a| a.len()).unwrap_or(0)
}

pub(crate) fn catalogue_all() -> Value {
    json!([
      { "id": "gpu.name", "describes": m!("cat_gpu_name"),
        "read": {"op":"run_tool","tool":"nvidia-smi","args":["--query-gpu=name","--format=csv,noheader"]} },
      { "id": "gpu.driver_version", "describes": m!("cat_gpu_driver_version"),
        "read": {"op":"run_tool","tool":"nvidia-smi","args":["--query-gpu=driver_version","--format=csv,noheader"]} },
      { "id": "gpu.vram_total_mib", "describes": m!("cat_gpu_vram_total_mib"),
        "read": {"op":"run_tool","tool":"nvidia-smi","args":["--query-gpu=memory.total","--format=csv,noheader"]} },
      { "id": "gpu.compute_capability", "describes": m!("cat_gpu_compute_capability"),
        "read": {"op":"run_tool","tool":"nvidia-smi","args":["--query-gpu=compute_cap","--format=csv,noheader"]} },
      { "id": "gpu.temperature", "describes": m!("cat_gpu_temperature"),
        "read": {"op":"run_tool","tool":"nvidia-smi","args":["--query-gpu=temperature.gpu","--format=csv,noheader"]} },
      { "id": "gpu.power_draw", "describes": m!("cat_gpu_power_draw"),
        "read": {"op":"run_tool","tool":"nvidia-smi","args":["--query-gpu=power.draw","--format=csv,noheader"]} },
      { "id": "os.version", "describes": m!("cat_os_version"),
        "read": {"op":"os_fact","name":"version"} },
      { "id": "os.container", "describes": m!("cat_os_container"),
        "read": {"op":"os_fact","name":"container"} },
      { "id": "python.venv.version", "describes": m!("cat_python_venv_version"),
        "read": {"op":"read_ini_key","path":".venv/pyvenv.cfg","key":"version"} },
      { "id": "python.venv.base", "describes": m!("cat_python_venv_base"),
        "read": {"op":"read_ini_key","path":".venv/pyvenv.cfg","key":"home"} },
      { "id": "pci.devices", "describes": m!("cat_pci_devices"),
        "read": {"op":"run_tool","tool":"lspci","args":["-nn"]} },
      { "id": "mac.displays", "describes": m!("cat_mac_displays"),
        "read": {"op":"run_tool","tool":"system_profiler","args":["SPDisplaysDataType"]} },
      { "id": "mac.os_version", "describes": m!("cat_mac_os_version"),
        "read": {"op":"run_tool","tool":"sw_vers","args":["-productVersion"]} },
      { "id": "python.version", "describes": m!("cat_python_version"),
        "read": {"op":"run_tool","tool":"python3","args":["--version"]} },
      { "id": "pip.version", "describes": m!("cat_pip_version"),
        "read": {"op":"run_tool","tool":"pip","args":["--version"]} },
      { "id": "node.version", "describes": m!("cat_node_version"),
        "read": {"op":"run_tool","tool":"node","args":["--version"]} },
      { "id": "os.kernel", "describes": m!("cat_os_kernel"),
        "read": {"op":"run_tool","tool":"uname","args":["-r"]} }
    ])
}

/// What the catalogue would offer on a machine with every tool present —
/// used to explain what is *missing* here, rather than hiding it.
pub fn catalogue_unavailable() -> Value {
    let usable: Vec<String> = catalogue().as_array().cloned().unwrap_or_default().into_iter()
        .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    json!(catalogue_all().as_array().cloned().unwrap_or_default().into_iter()
        .filter(|c| !usable.iter().any(|u| c.get("id").and_then(|v| v.as_str()) == Some(u)))
        .map(|c| json!({
            "id": c.get("id"), "describes": c.get("describes"),
            "why": c.get("read").map(|r| precheck(r).err().unwrap_or_default())
        }))
        .collect::<Vec<_>>())
}

/// The project root and the program grants are process-wide, and the suite
/// runs cases in parallel. Every case that grants, withdraws, or reads what a
/// grant makes readable — the catalogue included, since a relative reading is
/// offered only while a project is granted — holds this, or one case's grant
/// becomes another's reading and the failure looks like the bug.
#[cfg(test)]
static TEST_GRANTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn grants_held() -> std::sync::MutexGuard<'static, ()> {
    TEST_GRANTS.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `VS_EXTRA_ROOT` is process-wide and several cases set it. Run them one
    /// at a time, or one case's root becomes another's and the failure looks
    /// like the bug rather than like the harness.
    static EXTRA_ROOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_extra_root(dir: &Path) -> std::sync::MutexGuard<'static, ()> {
        let g = EXTRA_ROOT.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("VS_EXTRA_ROOT", dir);
        g
    }

    /// The catalogue must not offer what this machine cannot do: the model
    /// would pick it, the user would consent, and nothing would come back.
    #[test]
    fn catalogue_only_offers_what_is_runnable_here() {
        let _g = grants_held();
        for entry in catalogue().as_array().unwrap() {
            let read = entry.get("read").unwrap();
            assert!(precheck(read).is_ok(), "unusable entry offered: {entry}");
        }
    }

    /// P1: the catalogue's promise is kept. Everything it offers on this
    /// machine must actually yield a value — an entry that prechecks clean and
    /// then returns nothing is the dead end the filtering exists to prevent,
    /// and the user has consented to a read by the time it is discovered.
    #[test]
    fn everything_the_catalogue_offers_here_actually_reads() {
        let _g = grants_held();
        let offered = catalogue();
        let offered = offered.as_array().unwrap();
        assert!(
            !offered.is_empty(),
            "this machine offers no readings at all — the suite would prove nothing"
        );
        for entry in offered {
            let id = entry["id"].as_str().unwrap();
            assert!(
                perform(&entry["read"]).is_some(),
                "{id} was offered but returned nothing"
            );
        }
    }

    /// Absence is stated, not silently empty.
    #[test]
    fn a_missing_tool_is_refused_with_a_reason() {
        let r = json!({"op":"run_tool","tool":"nvidia-smi","args":["--query-gpu=name"]});
        if !tool_available("nvidia-smi") {
            let e = precheck(&r).unwrap_err();
            assert!(crate::msg::is("tool_absent", &e), "unhelpful: {e}");
        }
        // A tool outside the allow-list is refused whether present or not.
        let bad = json!({"op":"run_tool","tool":"bash","args":["-c"]});
        assert!(precheck(&bad).is_err(), "an unlisted tool was allowed");
    }

    /// Consent cannot unlock the deny-list.
    #[test]
    fn denied_paths_stay_denied() {
        let r = json!({"op":"read_file_key","path":"/home/x/.ssh/config","key":"k"});
        assert!(precheck(&r).is_err(), "an ssh path was permitted");
    }

    /// A symlink is the other way out, and it leaves no trace in the path.
    ///
    /// The `..` ban closed one spelling of the escape. This is the other: a link
    /// inside a granted root, pointing anywhere, reads as being inside it —
    /// `starts_with` compares the names it was given, and the written path
    /// contains nothing to notice. A dotfile manager pointing
    /// `~/.config/app/cache` at `~/Documents` is enough, and the deny list only
    /// ever sees the link's own harmless name.
    ///
    /// Skipped rather than failed where the platform will not make links —
    /// Windows needs Developer Mode or elevation — because a case that silently
    /// passes on a machine that could not attempt the attack is worse than one
    /// that says so.
    #[test]
    fn a_symlink_cannot_lead_a_read_out_of_a_granted_root() {
        let granted = std::env::temp_dir().join(format!("vs-granted-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("vs-outside-{}", std::process::id()));
        std::fs::create_dir_all(&granted).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("private.json"), r#"{"leaked":"yes"}"#).unwrap();

        let link = granted.join("cache");
        let _ = std::fs::remove_file(&link);
        let made = {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside, &link).is_ok()
            }
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_dir(&outside, &link).is_ok()
            }
            #[cfg(not(any(unix, windows)))]
            {
                false
            }
        };
        if !made {
            eprintln!("symlinks cannot be created here — attack not attempted");
            let _ = std::fs::remove_dir_all(&granted);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }

        let guard = with_extra_root(&granted);
        let through = link.join("private.json");
        assert!(
            through.starts_with(&granted),
            "this case is pointless unless the naive prefix test accepts the link"
        );
        assert!(
            std::fs::read_to_string(&through).is_ok(),
            "the link does not actually reach the file, so nothing is being proved"
        );

        let r = json!({"op": "read_file_key", "path": through.to_string_lossy(),
                       "key": "leaked"});
        let verdict = precheck(&r);

        // And an enumeration must not walk through it either.
        let e = json!({"op": "enumerate_read", "root": "dev",
                       "glob": "*.json", "keys": ["leaked"]});
        let walked = enumerate_read(&e).unwrap_or_else(|| json!([])).to_string();
        drop(guard);

        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&granted);
        let _ = std::fs::remove_dir_all(&outside);

        assert!(
            verdict.is_err(),
            "a symlink led a read out of every granted root, and the prefix test              agreed it was inside one"
        );
        assert!(
            !walked.contains("yes"),
            "the walk followed a symlink out of the root: {walked}"
        );
    }

    /// The depth guard ended the whole walk instead of skipping one branch.
    ///
    /// The stack is last-in-first-out, so a deep branch is popped while shallow
    /// siblings are still queued — `break` there discards them and returns a
    /// silently incomplete result that reads as "there is nothing here".
    #[test]
    fn one_deep_branch_does_not_end_the_walk() {
        // `read_dir` is sorted by the walk, and the stack is last-in-first-out,
        // so `zz-deep` is popped before `aa-shallow` — deterministically. That
        // is the whole construction: at the moment the over-deep directory is
        // popped, a perfectly reachable sibling is still queued behind it.
        let base = std::env::temp_dir().join(format!("vs-depth-{}", std::process::id()));
        let deep = base.join("zz-deep").join("x1").join("x2").join("x3").join("x4");
        let shallow = base.join("aa-shallow").join("mod");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(&shallow).unwrap();
        std::fs::write(shallow.join("manifest.json"), r#"{"name":"reachable"}"#).unwrap();

        let guard = with_extra_root(&base);
        let e = json!({"op": "enumerate_read", "root": "dev",
                       "glob": "*/*/manifest.json", "keys": ["name"]});
        let walked = enumerate_read(&e).unwrap_or_else(|| json!([])).to_string();
        drop(guard);
        let _ = std::fs::remove_dir_all(&base);

        assert!(
            walked.contains("reachable"),
            "a branch past the depth limit ended the walk and the sibling queued              behind it was never visited: {walked}"
        );
    }

    /// The glob's directory part bounds the walk, and the consent text is what
    /// promised that it would.
    ///
    /// Only the text after the last `/` was kept, so `telemetry-cache/*.json`
    /// became `*.json` and the walk covered the whole root — every other
    /// application's files included — while `describe()` showed the user the
    /// full pattern.
    #[test]
    fn a_glob_does_not_search_wider_than_it_reads() {
        let base = std::env::temp_dir().join(format!("vs-glob-{}", std::process::id()));
        let wanted = base.join("telemetry-cache");
        let other = base.join("someone-elses-app");
        std::fs::create_dir_all(&wanted).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(wanted.join("a.json"), r#"{"name":"asked-for"}"#).unwrap();
        std::fs::write(other.join("b.json"), r#"{"name":"not-asked-for"}"#).unwrap();
        std::fs::write(base.join("c.json"), r#"{"name":"root-level"}"#).unwrap();

        let guard = with_extra_root(&base);
        let e = json!({"op": "enumerate_read", "root": "dev",
                       "glob": "telemetry-cache/*.json", "keys": ["name"]});
        let walked = enumerate_read(&e).unwrap_or_else(|| json!([])).to_string();
        drop(guard);
        let _ = std::fs::remove_dir_all(&base);

        assert!(walked.contains("asked-for"), "the named directory was not read: {walked}");
        assert!(
            !walked.contains("not-asked-for"),
            "the walk read another application's directory, which the pattern did              not name and the consent text did not offer: {walked}"
        );
        assert!(
            !walked.contains("root-level"),
            "the walk read the root itself, above the directory named: {walked}"
        );
    }

    /// A registry read was arbitrary code execution.
    ///
    /// Both parameters were interpolated into a PowerShell `-Command`, single
    /// quoted and unescaped, so `HKCU:\X'; <anything>; '` closed the quote and
    /// the rest ran — behind a consent screen that said "read a registry
    /// value", in the file whose own header says that "run this command" is not
    /// a capability we hand to a remote party.
    ///
    /// The read no longer builds a command string. This guards the other half:
    /// nothing that could be a command survives the pattern, so the property
    /// holds even if some later reader reaches for a shell again.
    #[test]
    fn a_registry_read_cannot_carry_a_command() {
        for bad in [
            r"HKCU:\X'; calc; '",
            r"HKCU:\A;B",
            "HKCU:\\A\nB",
            r"HKCU:\A&calc",
            r"HKCU:\A|calc",
            r"HKCU:\A$(calc)",
            r"NOTAHIVE\X",
            r"..\..\etc",
        ] {
            let r = json!({"op": "read_registry", "path": bad, "name": "Version"});
            assert!(precheck(&r).is_err(), "a registry path carrying a command was allowed: {bad}");
        }
        for bad in ["a'; calc", "a;b", "a\nb"] {
            let r = json!({"op": "read_registry", "path": r"HKCU:\Software\X", "name": bad});
            assert!(precheck(&r).is_err(), "a registry value name carrying a command was allowed: {bad}");
        }
        // And an ordinary one still passes, or the guard is just an outage.
        let good = json!({"op": "read_registry", "path": r"HKCU:\Software\Engram",
                          "name": "InstallPath"});
        precheck(&good).expect("an ordinary registry read was refused");
    }

    /// The deny list must actually name the things people call credentials.
    /// The plumbing that screens `key` landed before the words did, so
    /// `api_secret` was refused because "secret" happened to be listed while
    /// `password` and `api_key` went straight through.
    #[test]
    fn the_deny_list_names_the_common_credential_words() {
        let root = dirs::config_dir().expect("no config dir on this machine");
        for key in ["password", "api_key", "apikey", "private_key", "authorization"] {
            let r = json!({"op": "read_file_key",
                           "path": root.join("app.json").to_string_lossy(),
                           "key": key});
            assert!(precheck(&r).is_err(), "a key named {key:?} was permitted");
        }
        for name in ["OPENAI_API_KEY", "AWS_ACCESS_KEY_ID", "DB_PASSWORD"] {
            let r = json!({"op": "env_var", "name": name});
            assert!(precheck(&r).is_err(), "the environment variable {name:?} was permitted");
        }
        // Not so broad that ordinary readings stop working.
        precheck(&json!({"op": "env_var", "name": "XDG_SESSION_TYPE"}))
            .expect("an ordinary environment variable was refused");
        precheck(&json!({"op": "env_var", "name": "OLLAMA_HOST"}))
            .expect("an ordinary environment variable was refused");

        // And a name that merely *looks* permitted does not get through. The
        // list folds case and nothing else, so a Cyrillic spelling was simply a
        // different string to it — the same evasion this project already ships a
        // UTS 39 table against, one level down and with no table here.
        let root = dirs::config_dir().expect("no config dir on this machine");
        for key in ["ѕeсret", "pаssword", "аpi_key"] {
            let r = json!({"op": "read_file_key",
                           "path": root.join("app.json").to_string_lossy(),
                           "key": key});
            assert!(precheck(&r).is_err(),
                    "a homoglyph spelling of a denied word was permitted: {key:?}");
        }
        assert!(precheck(&json!({"op": "env_var", "name": "АPI_KEY"})).is_err(),
                "a homoglyph environment variable name was permitted");
    }

    /// A granted root is a prefix test, and a prefix test cannot see a way out
    /// of the directory it is testing. `Path::starts_with` compares components
    /// without normalising, so `<config>/../../.npmrc` *starts with* `<config>`
    /// — the root check passed and the read happened outside every granted
    /// root. The deny list caught `.ssh` and `.aws` by name and nothing else.
    ///
    /// Both halves are here: the escape is refused, and the field name is on
    /// the deny list as well as the file name.
    #[test]
    fn a_path_cannot_walk_out_of_a_granted_root() {
        let root = dirs::config_dir().expect("no config dir on this machine");
        let escape = root.join("..").join("..").join(".npmrc");
        let p = Path::new(&escape);
        assert!(
            p.starts_with(&root),
            "this test is pointless if the prefix check does not accept the escape"
        );
        let r = json!({"op":"read_ini_key","path":escape.to_string_lossy(),"key":"registry"});
        let e = precheck(&r).unwrap_err();
        assert!(crate::msg::is("path_backstep", &e), "refused for the wrong reason: {e}");

        // A directory whose name merely begins with two dots is not a way out.
        // Inside a root this case grants itself rather than the real config
        // directory: that one exists on a desktop, and in a fresh container it
        // existed only because the identity tests used to write the installed
        // client's secret into it — which they must not.
        let granted = std::env::temp_dir().join(format!("vs-dots-{}", std::process::id()));
        std::fs::create_dir_all(&granted).unwrap();
        let _g = grants_held();
        set_project_root(Some(granted.clone()));
        let ok = granted.join("..hidden").join("a.json");
        let r = json!({"op":"read_file_key","path":ok.to_string_lossy(),"key":"theme"});
        let verdict = precheck(&r);
        set_project_root(None);
        let _ = std::fs::remove_dir_all(&granted);
        verdict.expect("`..hidden` is a directory name, not a traversal");

        // The key is a name a publisher chooses, so it is on the deny list too.
        let r = json!({"op":"read_file_key",
                       "path":root.join("app.json").to_string_lossy(),
                       "key":"api_secret"});
        assert!(precheck(&r).is_err(), "a key naming a secret was permitted");
    }

    /// The version-tree case, exercised rather than assumed: a bounded walk
    /// that reads named keys out of the manifests it finds.
    #[test]
    fn enumerate_read_walks_and_reads() {
        let root = std::env::temp_dir().join(format!("vs-enum-{}", std::process::id()));
        let a = root.join("mod-a");
        let b = root.join("mod-b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("manifest.json"), r#"{"name":"alpha","version":"1.2.3"}"#).unwrap();
        std::fs::write(b.join("manifest.json"), r#"{"name":"beta","version":"4.5"}"#).unwrap();
        let _guard = with_extra_root(&root);

        let r = json!({"op":"enumerate_read","root":"dev","glob":"*/manifest.json",
                       "keys":["name","version"]});
        let out = enumerate_read(&r).expect("no result");
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 2, "expected both manifests, got {arr:?}");
        let names: Vec<&str> = arr.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(names.contains(&"alpha") && names.contains(&"beta"), "{names:?}");
    }

    /// The point of part two: the environment a project actually uses is
    /// readable **without running anything**. `pyvenv.cfg` states the version
    /// as a plain key, so this answers what `python3 --version` would have said
    /// for that virtualenv — and it answers it for the venv rather than for
    /// whatever interpreter happens to be on this process's PATH.
    #[test]
    fn a_virtualenvs_python_is_read_without_executing_anything() {
        let _g = grants_held();
        let root = std::env::temp_dir().join(format!("podshl-venv-{}", std::process::id()));
        let venv = root.join(".venv");
        std::fs::create_dir_all(&venv).unwrap();
        std::fs::write(
            venv.join("pyvenv.cfg"),
            "home = /usr/bin
# a comment
include-system-site-packages = false
version = 3.11.9
",
        )
        .unwrap();
        set_project_root(Some(root.clone()));

        let r = json!({"op":"read_ini_key",
                       "path": venv.join("pyvenv.cfg").to_string_lossy(),
                       "key":"version"});
        precheck(&r).expect("a file inside the granted project root was refused");
        assert_eq!(perform(&r), Some(json!("3.11.9")));

        // And the way the product actually asks for it: the catalogue's own
        // entry, written relative, which must mean relative to the project
        // that was granted. It was resolved against the process's working
        // directory instead, so this reading — the one this case exists for —
        // looked for a virtualenv wherever the client had been started, found
        // none, and was dropped from the menu as not readable here.
        let catalogued = catalogue_all().as_array().unwrap().iter()
            .find(|e| e["id"] == "python.venv.version").unwrap()["read"].clone();
        assert!(catalogued["path"].as_str().unwrap().starts_with(".venv"),
                "the catalogue entry is no longer relative — this half proves nothing");
        precheck(&catalogued).expect("the catalogue's own venv reading was refused inside a granted project");
        assert_eq!(perform(&catalogued), Some(json!("3.11.9")),
                   "the relative reading did not read the granted project's virtualenv");
        assert!(catalogue().as_array().unwrap().iter().any(|e| e["id"] == "python.venv.version"),
                "a granted project does not make its virtualenv readable from the menu");

        // The grant is per incident. Withdrawn, the same read is out of bounds
        // again — a project directory is not a new default root — and the
        // relative one names nothing at all.
        set_project_root(None);
        assert!(precheck(&r).is_err(), "the project root outlived the incident");
        let e = precheck(&catalogued).unwrap_err();
        assert!(crate::msg::is("path_relative_no_project", &e), "a relative path without a project was refused for the wrong reason: {e}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// EN3. Where the host interpreter and the project's disagree, neither is
    /// obviously the one meant — so this produces a question and never a
    /// resolution. Nobody is told a version that is not theirs.
    #[test]
    fn a_disagreeing_interpreter_becomes_a_question_not_an_answer() {
        let clash = json!({"python.version": "Python 3.12.14",
                           "python.venv.version": "3.11.9"});
        let q = interpreter_conflict(&clash).expect("a real disagreement was resolved silently");
        assert_eq!(q["host"], "Python 3.12.14");
        assert_eq!(q["project"], "3.11.9");
        assert!(q["choices"].as_array().unwrap().len() >= 2, "a question with nothing to answer");
        // The patch level is not a disagreement: 3.11.9 and 3.11.2 are the same
        // minor, and wheels are published per minor.
        assert!(interpreter_conflict(&json!({"python.version": "Python 3.11.2",
                                             "python.venv.version": "3.11.9"})).is_none());
        // Agreement is silent, and so is knowing only one of them — a single
        // reading cannot contradict anything.
        assert!(interpreter_conflict(&json!({"python.version": "Python 3.11.9",
                                             "python.venv.version": "3.11.9"})).is_none());
        assert!(interpreter_conflict(&json!({"python.version": "Python 3.12.1"})).is_none());
        assert!(interpreter_conflict(&json!({})).is_none());
    }

    /// Containerisation is a fact, read from files, never inferred. Both cases
    /// it answers are ones where a host reading would otherwise be quietly
    /// about the wrong machine.
    #[test]
    fn whether_this_is_a_container_is_answerable() {
        let r = json!({"op":"os_fact","name":"container"});
        precheck(&r).expect("container detection was refused");
        let v = perform(&r).expect("no answer about containerisation");
        let s = v.as_str().unwrap_or("");
        assert!(
            ["none", "docker", "podman", "containerd", "kubernetes", "lxc"].contains(&s),
            "unexpected container answer: {s:?}"
        );
    }

    /// A traversal must not escape its root or walk into denied territory.
    #[test]
    fn enumerate_read_refuses_traversal_and_denied_names() {
        assert!(precheck(&json!({"op":"enumerate_read","root":"config",
                                 "glob":"../../*/manifest.json","keys":[]})).is_err());
        assert!(precheck(&json!({"op":"enumerate_read","root":"config",
                                 "glob":".ssh/*.json","keys":[]})).is_err());
        assert!(precheck(&json!({"op":"enumerate_read","root":"nowhere",
                                 "glob":"*.json","keys":[]})).is_err());
    }

    #[test]
    fn baseline_is_free_and_complete() {
        let b = baseline();
        assert!(b.get("os").is_some() && b.get("arch").is_some());
    }

    /// What a vendor may ask for has to be written down somewhere a vendor can
    /// read, and the written form has to be held to the code — otherwise the
    /// server's "rejected at ingest" gate validates against a list that has
    /// quietly stopped describing this client.
    ///
    /// The catalogue is compared machine-independently on purpose: `catalogue()`
    /// filters to what runs here, and a spec that changed shape depending on
    /// which machine generated it would not be a spec.
    #[test]
    fn vocabulary_matches_the_spec() {
        let raw = std::fs::read_to_string("../spec/vocabulary/reads.json")
            .expect("spec/vocabulary/reads.json is missing");
        let spec: Value = serde_json::from_str(&raw).expect("reads.json is not JSON");

        assert_eq!(spec["limits"]["max_reads"], MAX_READS, "max_reads");
        assert_eq!(spec["limits"]["max_entries"], MAX_ENTRIES, "max_entries");
        assert_eq!(spec["limits"]["max_depth"], MAX_DEPTH, "max_depth");

        let listed: Vec<&str> = spec["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["tool"].as_str().unwrap())
            .collect();
        let known: Vec<&str> = TOOLS.iter().map(|(t, _)| *t).collect();
        assert_eq!(listed, known, "the tool allow list differs from the spec");
        for t in spec["tools"].as_array().unwrap() {
            let (_, pat) = TOOLS
                .iter()
                .find(|(name, _)| *name == t["tool"].as_str().unwrap())
                .unwrap();
            assert_eq!(t["args"].as_str().unwrap(), *pat, "argument pattern for {}", t["tool"]);
        }

        let denied: Vec<&str> = spec["deny"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect();
        assert_eq!(denied, DENY.to_vec(), "the deny list differs from the spec");

        // The fields `nvidia-smi` may not be asked for, the environment
        // variables that may be asked for at all, and the registry values
        // that name the machine: each a bound a publisher reads in the spec.
        let nvsmi = spec["tools"].as_array().unwrap().iter()
            .find(|t| t["tool"] == "nvidia-smi").unwrap();
        assert_eq!(nvsmi["refuse_fields"].as_str().unwrap(), GPU_FIELD_DENY,
                   "the identifying nvidia-smi fields differ from the spec");
        let env_allowed: Vec<&str> = spec["env"]["allow"].as_array().unwrap().iter()
            .map(|v| v.as_str().unwrap()).collect();
        assert_eq!(env_allowed, ENV_ALLOW.to_vec(), "the environment allow list differs from the spec");

        assert_eq!(spec["registry"]["path"].as_str().unwrap(), REGISTRY_PATH,
                   "the registry path pattern differs from the spec");
        assert_eq!(spec["registry"]["name"].as_str().unwrap(), REGISTRY_NAME,
                   "the registry name pattern differs from the spec");
        assert_eq!(spec["registry"]["refuse_names"].as_str().unwrap(), REGISTRY_NAME_DENY,
                   "the identifying registry names differ from the spec");

        // Which programs may be run for their version, with which argument, is
        // the one place this vocabulary starts a program a publisher chose. It
        // is held to the spec field by field, like the registry patterns, so
        // the gate on the server and the gate here refuse the same things.
        let programs = &spec["programs"];
        assert_eq!(programs["name"].as_str().unwrap(), PROGRAM_NAME, "program name pattern");
        let flags: Vec<&str> = programs["flags"].as_array().unwrap().iter()
            .map(|f| f.as_str().unwrap()).collect();
        assert_eq!(flags, VERSION_FLAGS.to_vec(), "the version flags differ from the spec");
        let never: Vec<&str> = programs["deny"].as_array().unwrap().iter()
            .map(|f| f.as_str().unwrap()).collect();
        assert_eq!(never, PROGRAM_DENY.to_vec(), "the program deny list differs from the spec");
        assert_eq!(spec["images"]["name"].as_str().unwrap(), IMAGE_NAME, "image name pattern");

        // Compared on id and read instruction only. Each entry also carries a
        // `describes` label, but that is this client's own presentation string
        // and is translatable; pinning it in the spec would make a translation
        // a protocol change.
        let contract = |v: &Value| -> Vec<Value> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|e| json!({"id": e["id"], "read": e["read"]}))
                .collect()
        };
        assert_eq!(
            contract(&spec["catalogue"]),
            contract(&catalogue_all()),
            "the specified catalogue differs from this client's"
        );

        // Every documented op is one this client knows. `precheck` may still
        // refuse a particular instruction — a tool absent from this machine, a
        // path outside the roots — but it must never answer "unknown".
        for entry in spec["ops"].as_array().unwrap() {
            let op = entry["op"].as_str().unwrap();
            if let Err(e) = precheck(&json!({"op": op})) {
                assert!(!crate::msg::is("read_op_unknown", &e), "spec names an op this client does not implement: {op}");
            }
        }
        let unknown = precheck(&json!({"op": "run_powershell"})).unwrap_err();
        assert!(crate::msg::is("read_op_unknown", &unknown), "an op outside the vocabulary was not refused: {unknown}");
    }

    /// PV1: a program's version is read by asking the program — and only the
    /// version comes back. Everything else it prints stays on this machine.
    ///
    /// `rustc` is the program because it is the one thing guaranteed present
    /// wherever this suite runs: cargo is running it.
    #[test]
    fn a_programs_version_is_read_by_asking_it_and_only_the_number_is_kept() {
        let _g = grants_held();
        end_incident();
        let r = json!({"op": "program_version", "program": "rustc"});
        precheck(&r).expect("an ordinary program was refused");
        let (what, _) = describe(&r).unwrap();
        assert!(what.contains("--version"), "the consent text does not say what runs: {what}");
        let v = perform(&r).expect("rustc is running this suite and could not say its version");
        let s = v.as_str().unwrap();
        assert!(version_pair(s).is_some(), "not a version: {s:?}");
        assert!(!s.contains(' ') && !s.contains("rustc"), "more than the number travelled: {s:?}");
    }

    /// PV2: not on the search path is not refused — it is a question, and the
    /// answer is a location. The grant names one program, the file has to be
    /// that program, and it goes with the incident.
    #[test]
    fn a_program_not_on_the_path_is_asked_for_and_the_answer_is_bounded() {
        let _g = grants_held();
        end_incident();
        let absent = json!({"op": "program_version", "program": "podshl-no-such-program"});
        precheck(&absent).expect("a program that is simply not installed was refused — it should be asked about");
        let (what, _) = describe(&absent).unwrap();
        assert!(crate::msg::is("what_program_version_absent", &what), "the plan does not say the user will be asked: {what}");
        assert!(perform(&absent).is_none(), "a version was invented for a program that is not there");

        // The directory rustc lives in, as a user would point at it.
        let rustc = find_on_path("rustc").expect("rustc is not on PATH");
        let dir = rustc.parent().unwrap().to_string_lossy().to_string();
        let granted = grant_program_path("rustc", &dir).expect("pointing at the directory was refused");
        assert_eq!(granted.file_name(), rustc.canonicalize().unwrap().file_name());
        assert_eq!(locate_program("rustc").as_deref(), Some(granted.as_path()),
                   "the user's answer did not outrank the search path");

        // The file has to be the program that was asked about: pointing at
        // rustc does not answer a question about engram.
        let e = grant_program_path("engram", &rustc.to_string_lossy()).unwrap_err();
        assert!(crate::msg::is("wrong_program_name", &e) && e.contains("engram"), "a different program was accepted: {e}");
        assert_eq!(location_error_kind(&e), "wrong_name");
        let e = grant_program_path("engram", &dir).unwrap_err();
        assert_eq!(location_error_kind(&e), "not_in_folder", "a directory without engram in it answered for it: {e}");
        assert_eq!(location_error_kind(&grant_program_path("engram", "  ").unwrap_err()), "empty");
        assert_eq!(location_error_kind(&grant_program_path("bash", &dir).unwrap_err()), "denied");
        let nowhere = std::env::temp_dir().join("podshl-no-such-file");
        assert_eq!(location_error_kind(&grant_program_path("engram", &nowhere.to_string_lossy()).unwrap_err()),
                   "not_executable");

        // A relative entry on the search path is the working directory, and a
        // file sitting there — a download next to the client, say — is not the
        // program anybody installed. Planted in a directory under this
        // process's working directory, so the relative entry really reaches it.
        let relative = PathBuf::from(format!("vs-cwd-{}", std::process::id()));
        let planted = std::env::current_dir().unwrap().join(&relative);
        std::fs::create_dir_all(&planted).unwrap();
        let name = if cfg!(windows) { "planted.exe" } else { "planted" };
        std::fs::write(planted.join(name), b"#!/bin/sh\necho planted 6.6.6\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(planted.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let rel_path = std::env::join_paths([relative.clone(), PathBuf::from(".")]).unwrap();
        let via_relative = find_in(&rel_path, "planted");
        let abs_path = std::env::join_paths([planted.clone()]).unwrap();
        let via_absolute = find_in(&abs_path, "planted");
        let _ = std::fs::remove_dir_all(&planted);
        assert!(via_relative.is_none(), "a relative PATH entry ({}) was searched", relative.display());
        assert!(via_absolute.is_some(),
                "the same file on an absolute entry was not found — the case proves nothing");

        // And it goes with the incident — and so does the project root, which
        // nothing used to clear at all.
        set_project_root(Some(std::env::temp_dir()));
        end_incident();
        assert!(granted_program("rustc").is_none(), "a program grant outlived the incident");
        assert!(project_root().is_none(), "the project root outlived the incident");
    }

    /// PV3: what a publisher may never have run, whatever the user clicks — a
    /// shell, a launcher, a path instead of a name, an argument of their own,
    /// and anything belonging to the operating system.
    #[test]
    fn a_program_that_is_not_a_program_s_own_version_is_refused() {
        for (program, flag) in [("bash", ""), ("cmd", ""), ("cmd.exe", ""), ("powershell", ""),
                                ("sudo", ""), ("shutdown", ""), ("explorer", ""), ("rm", ""),
                                ("../engram", ""), ("C:\\x\\engram", ""), ("/bin/engram", ""),
                                ("engram", "-c"), ("engram", "--help; rm -rf ~"), ("engram", "-v"),
                                ("ѕecret", ""), ("my-token-tool", ""),
                                // A bare word is an argument to a program without
                                // subcommands — a file, a target, a host.
                                ("engram", "version"),
                                // Package runners, build tools and interpreters
                                // run what the directory says, whatever the flag.
                                ("npx", ""), ("npm", ""), ("make", ""), ("just", ""), ("python3", ""),
                                ("node", ""), ("ruby", ""), ("mvn", "")] {
            let r = json!({"op": "program_version", "program": program, "flag": flag});
            assert!(precheck(&r).is_err(), "{program:?} {flag:?} was permitted");
        }
        for flag in ["--version", "-V", "-version"] {
            precheck(&json!({"op": "program_version", "program": "engram", "flag": flag}))
                .unwrap_or_else(|e| panic!("the closed flag {flag} was refused: {e}"));
        }

        // The system directories, which is where the programs that ignore
        // their arguments and open a window live. On a merged /usr there is no
        // such directory — /usr/sbin *is* /usr/bin — so the claim to make there
        // is the other one, and it is the one that was false.
        if cfg!(windows) {
            let sys = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
                .join("System32").join("where.exe");
            assert!(in_system_directory(&sys), "{} is not recognised as the system's", sys.display());
        } else if merged_usr() {
            assert!(!in_system_directory(Path::new("/usr/bin/podshl-anything")),
                    "a merged /usr made every program on the machine the system's");
        } else {
            let sys = PathBuf::from("/usr/sbin/podshl-anything");
            assert!(in_system_directory(&sys), "{} is not recognised as the system's", sys.display());
        }
        assert!(!in_system_directory(&std::env::temp_dir().join("engram")),
                "an ordinary directory was treated as the system's");
        if cfg!(windows) {
            let e = precheck(&json!({"op": "program_version", "program": "where"})).unwrap_err();
            assert!(crate::msg::is("system_program", &e), "a system program was offered: {e}");
        }
    }

    /// Does this machine have a merged `/usr` — `/usr/sbin` resolved onto the
    /// same directory as `/usr/bin`? True on every current Linux distribution
    /// and false on macOS, and the cases below say different things on each
    /// rather than one thing that is only true on one of them.
    fn merged_usr() -> bool {
        let sbin = std::fs::canonicalize("/usr/sbin");
        let bin = std::fs::canonicalize("/usr/bin");
        matches!((sbin, bin), (Ok(a), Ok(b)) if a == b)
    }

    /// An ordinary program installed the ordinary way can be asked its version.
    ///
    /// `in_system_directory` canonicalises the names it holds, to see through
    /// the links a distribution puts there — and on a merged `/usr` that turned
    /// `/sbin` and `/usr/sbin` into `/usr/bin`, which is every program on the
    /// machine. The published path is a project asking which version of its own
    /// package is installed, and on current Linux it answered that nothing
    /// could be asked. Named here rather than left to the tests that use
    /// `rustc`, because those go red for a dozen reasons and this one is worth
    /// recognising on sight.
    #[test]
    fn the_shared_binary_directory_is_not_the_operating_system() {
        if cfg!(windows) {
            return;
        }
        for d in ["/usr/bin", "/bin", "/sbin", "/usr/sbin"] {
            let Ok(real) = std::fs::canonicalize(d) else { continue };
            if real == Path::new("/usr/bin") {
                assert!(!in_system_directory(&real.join("podshl-not-a-real-program")),
                        "{d} resolves onto {} and was treated as the system's, \
                         which refuses every program here", real.display());
            }
        }
        // And the refusal that has to survive it: what a program *is*, rather
        // than where it sits, still holds.
        for named in ["bash", "sh"] {
            assert!(program_denied(named).is_some(),
                    "{named} stopped being refused by name");
        }
    }

    /// The token, and only the token, out of what real programs print — and
    /// nothing out of an error message that does not name the program.
    #[test]
    fn a_version_is_found_in_what_programs_actually_print() {
        for (out, program, want) in [
            ("engram v1.2.2 -- AI Memory Engine\n\nUsage:\n  engram create [path]", "engram", "1.2.2"),
            ("Python 3.12.14", "python3", "3.12.14"),
            ("v20.1.0", "node", "20.1.0"),
            ("pip 23.2 from /usr/lib/python3/dist-packages/pip (python 3.12)", "pip", "23.2"),
            ("git version 2.43.0.windows.1", "git", "2.43.0"),
            ("Docker version 24.0.7, build afdd53b", "docker", "24.0.7"),
            ("ollama version is 0.3.14-rc1", "ollama", "0.3.14-rc1"),
        ] {
            assert_eq!(version_token(out, program, true).as_deref(), Some(want), "{out:?}");
        }
        // A failed run is believed only on a line that names the program.
        assert_eq!(version_token("error: unknown option; see manual section 3.4", "engram", false), None);
        assert_eq!(version_token("engram v1.2.2 -- AI Memory Engine", "engram", false).as_deref(), Some("1.2.2"));
        assert_eq!(version_token("nothing numeric here", "x", true), None);
    }

    /// A program that does not answer is stopped, not waited on. The consent
    /// screen promised a version, not an open-ended run.
    #[test]
    fn a_program_that_does_not_answer_is_stopped() {
        let (exe, args): (PathBuf, Vec<&str>) = if cfg!(windows) {
            (PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
                .join("System32").join("ping.exe"), vec!["-n", "30", "127.0.0.1"])
        } else {
            (find_on_path("sleep").expect("no sleep"), vec!["30"])
        };
        let began = Instant::now();
        let out = run_bounded(&exe, &args);
        assert!(out.is_none(), "a run that outlived the limit returned a result");
        assert!(began.elapsed() < RUN_LIMIT + Duration::from_secs(5),
                "the limit was not enforced: {:?}", began.elapsed());
    }

    /// PV7: a list-shaped tool is shaped to what its reading promises — not the
    /// first line, which emptied both, and not everything, which is the
    /// machine's fingerprint.
    #[test]
    fn a_list_shaped_tool_reads_what_its_entry_promises_and_nothing_more() {
        let lspci = "\
00:00.0 Host bridge [0600]: Advanced Micro Devices, Inc. [AMD] Starship/Matisse Root Complex [1022:1480]
00:01.0 PCI bridge [0604]: Advanced Micro Devices, Inc. [AMD] Starship/Matisse PCIe Dummy Host Bridge [1022:1482]
05:00.0 Ethernet controller [0200]: Intel Corporation I211 Gigabit Network Connection [8086:1539] (rev 03)
06:00.0 Network controller [0280]: Intel Corporation Wi-Fi 6 AX200 [8086:2723] (rev 1a)
0a:00.0 VGA compatible controller [0300]: NVIDIA Corporation TU104 [GeForce RTX 2070 SUPER] [10de:1e84] (rev a1)
0a:00.1 Audio device [0403]: NVIDIA Corporation TU104 HD Audio Controller [10de:10f8] (rev a1)
0c:00.3 USB controller [0c03]: Advanced Micro Devices, Inc. [AMD] Matisse USB 3.0 Host Controller [1022:149c]";
        let got = shape_tool_output("lspci", lspci).unwrap();
        assert!(got.contains("GeForce RTX 2070 SUPER") && got.contains("I211") && got.contains("AX200"), "{got}");
        for absent in ["Host bridge", "USB controller", "Audio device", "0a:00.0"] {
            assert!(!got.contains(absent), "{absent} travelled in pci.devices: {got}");
        }

        let mac = "\
Graphics/Displays:

    Apple M2:

      Chipset Model: Apple M2
      Type: GPU
      Bus: Built-In
      Total Number of Cores: 10
      Vendor: Apple (0x106b)
      Metal Support: Metal 3
      Displays:
        Color LCD:
          Display Type: Built-In Liquid Retina Display
          Resolution: 2560 x 1664 Retina
          Main Display: Yes";
        let got = shape_tool_output("system_profiler", mac).unwrap();
        assert!(got.contains("Chipset Model: Apple M2") && got.contains("Resolution: 2560 x 1664"), "{got}");
        assert!(!got.starts_with("Graphics/Displays:"), "the heading came back as the reading: {got}");

        // A one-value tool is still its first line, and "[N/A]" is still nothing.
        assert_eq!(shape_tool_output("nvidia-smi", "8192 MiB\n").as_deref(), Some("8192 MiB"));
        assert_eq!(shape_tool_output("nvidia-smi", "[N/A]"), None);
    }

    /// PV9: a card that reports `0` for its serial has no serial.
    ///
    /// `nvidia-smi --query-gpu=serial` prints `0` on a consumer RTX card — the
    /// card saying it carries none in firmware. It arrived as the string "0",
    /// so `warranty.rma.precheck`'s `serial.printed` question never fired and a
    /// precheck went to the vendor reading "serial number 0": a return
    /// authorised against a number that identifies nothing.
    ///
    /// Found by walking the window on a real RTX 5070, which is the only way it
    /// could have been found — every fixture had a serial or an empty string.
    /// `[N/A]` was handled; `0` is the same statement in a different dialect.
    #[test]
    fn a_card_reporting_zero_for_its_serial_has_no_serial() {
        fn nvsmi(field: &str, out: &str) -> Option<String> {
            let arg = format!("--query-gpu={field}");
            shape_tool_output_for("nvidia-smi", &[&arg, "--format=csv,noheader"], out)
        }

        assert_eq!(nvsmi("serial", "0"), None);
        assert_eq!(nvsmi("serial", "[N/A]"), None);
        assert_eq!(nvsmi("serial", "0324718061234").as_deref(), Some("0324718061234"));

        // A zero that is a reading stays a reading. This is why the rule is per
        // field rather than "0 means nothing".
        for field in ["temperature.gpu", "power.draw", "fan.speed", "memory.used"] {
            assert_eq!(nvsmi(field, "0").as_deref(), Some("0"),
                       "{field} of zero was thrown away");
        }
        // And a row of several fields is not judged by one of them.
        assert_eq!(nvsmi("name,serial", "RTX 5070, 0").as_deref(), Some("RTX 5070, 0"));
        // Another tool printing 0 is another tool's business.
        assert_eq!(shape_tool_output_for("lscpu", &[], "0").as_deref(), Some("0"));
    }

    /// Docker names an image several ways; the publisher names it once.
    #[test]
    fn an_image_is_the_same_image_however_docker_spells_it() {
        assert_eq!(split_image("ollama/ollama:0.3.14"), ("ollama/ollama".into(), Some("0.3.14".into())));
        assert_eq!(split_image("docker.io/library/postgres:18"), ("postgres".into(), Some("18".into())));
        assert_eq!(split_image("ghcr.io/dx111ge/engram"), ("ghcr.io/dx111ge/engram".into(), None));
        assert_eq!(split_image("localhost:5000/x/y:1.2@sha256:abc").1, Some("1.2".into()));
        assert_eq!(normalise_image("docker.io/ollama/ollama:latest"), "ollama/ollama");
        for bad in ["Ollama", "ollama:latest", "a;b", "x/../y", ""] {
            let r = json!({"op": "container_image_version", "image": bad});
            assert!(precheck(&r).is_err(), "{bad:?} was permitted as an image name");
        }
    }

    /// PV8: `nvidia-smi` is asked about the card, never for the card's
    /// identity. The argument pattern admits any field name, so a publisher
    /// could name `uuid` or the bus address and the consent screen would have
    /// offered it as one more reading about the GPU.
    ///
    /// `serial` is the deliberate exception, and it is checked here as one: a
    /// warranty precheck is the one diagnosis that genuinely turns on which
    /// unit this is, the user approves it on the consent panel like any other
    /// reading, and `report.rs` holds the value back out of anything that
    /// leaves. Refusing it would protect nobody — it would move the same
    /// number off the card and onto a sticker the user reads out.
    #[test]
    fn a_gpu_reading_that_identifies_the_card_is_refused_at_the_gate() {
        let nvsmi = |fields: &str| json!({"op": "run_tool", "tool": "nvidia-smi",
                                          "args": [format!("--query-gpu={fields}"), "--format=csv,noheader"]});
        for bad in ["uuid", "gpu_uuid", "pci.bus_id", "vbios_version", "gsp.version",
                    "name,uuid", "memory.total,uuid"] {
            let e = precheck(&nvsmi(bad)).expect_err(&format!("{bad} was offered"));
            assert!(crate::msg::is("gpu_field_identifying", &e) || crate::msg::is("tool_absent", &e),
                    "refused for the wrong reason: {e}");
            if crate::msg::is("tool_absent", &e) {
                // Without the tool the gate stops earlier; the field check is
                // then exercised on the pattern alone.
                let identifying = regex::Regex::new(GPU_FIELD_DENY).unwrap();
                assert!(bad.split(',').any(|f| identifying.is_match(f)), "{bad} is not recognised as identifying");
            }
        }
        // And the readings the catalogue offers are not caught by it —
        // `serial` among them, which the RMA precheck asks for and gets.
        let identifying = regex::Regex::new(GPU_FIELD_DENY).unwrap();
        for ok in ["name", "driver_version", "memory.total", "compute_cap", "temperature.gpu",
                   "power.draw", "serial"] {
            assert!(!identifying.is_match(ok), "{ok} is refused, and the catalogue offers it");
        }
        precheck(&nvsmi("name,serial")).map(|_| ()).or_else(|e| {
            // Absent on this machine is the other permitted answer; refused as
            // identifying is not.
            assert!(crate::msg::is("tool_absent", &e), "the RMA precheck cannot read the serial: {e}");
            Ok::<(), String>(())
        }).unwrap();
    }

    /// EV1: the environment is read by allow list. The deny list screened the
    /// name, and it was the only screen — `HOME`, `PATH` and every variable
    /// somebody exported a credential under without a listed word in its
    /// name went through the gate.
    #[test]
    fn an_environment_variable_is_read_only_from_the_allow_list() {
        for bad in ["HOME", "PATH", "SSH_AUTH_SOCK", "GITHUB_TOKEN", "MY_SECRET_THING", "USERPROFILE", "PWD"] {
            let r = json!({"op": "env_var", "name": bad});
            let e = precheck(&r).expect_err(&format!("{bad} was offered"));
            assert!(crate::msg::is("env_var_not_allowed", &e), "{bad}: {e}");
            assert!(perform(&r).is_none(), "{bad} was read by a direct call, past the gate");
        }
        for ok in ENV_ALLOW {
            precheck(&json!({"op": "env_var", "name": ok}))
                .unwrap_or_else(|e| panic!("{ok} is on the allow list and was refused: {e}"));
        }
        // The allow list is what makes `XDG_SESSION_TYPE` readable although
        // `session` is a denied word: a name on the list was judged by name.
        assert!(deny_hit("XDG_SESSION_TYPE").is_some(), "the case proves nothing unless the word is denied");
    }

    /// RG1: a registry value that names the machine or its owner is refused
    /// on the value name, whatever key it sits under.
    #[test]
    fn a_registry_value_that_names_the_machine_is_refused() {
        for (path, name) in [
            (r"HKLM:\SOFTWARE\Microsoft\Cryptography", "MachineGuid"),
            (r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "ProductId"),
            (r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "RegisteredOwner"),
            (r"HKLM:\SYSTEM\CurrentControlSet\Control\ComputerName\ComputerName", "ComputerName"),
            (r"HKCU:\Software\X", "SerialNumber"),
        ] {
            let r = json!({"op": "read_registry", "path": path, "name": name});
            assert!(precheck(&r).is_err(), "{name} under {path} was offered");
            assert!(perform(&r).is_none(), "{name} was read by a direct call, past the gate");
        }
        precheck(&json!({"op": "read_registry", "path": r"HKCU:\Software\Engram", "name": "InstallPath"}))
            .expect("an ordinary registry value was refused");
    }

    /// FK1: a key read returns a scalar of bounded length or nothing, and does
    /// not open a file that is not a settings file. A key whose value is an
    /// object is a whole document under one name; a megabyte with a `.json`
    /// extension is a database.
    #[test]
    fn a_key_read_returns_a_bounded_scalar_or_nothing() {
        let _g = grants_held();
        let root = std::env::temp_dir().join(format!("podshl-scalar-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let long = "x".repeat(VALUE_LIMIT + 1);
        std::fs::write(root.join("app.json"),
                       format!(r#"{{"version":"1.2.3","n":7,"nested":{{"a":1}},"list":[1],"long":"{long}"}}"#)).unwrap();
        std::fs::write(root.join("big.json"), format!(r#"{{"version":"1","pad":"{}"}}"#, "p".repeat(FILE_LIMIT as usize))).unwrap();
        std::fs::write(root.join("settings.cfg"), format!("home = /usr/bin\nlong = {long}\n")).unwrap();
        set_project_root(Some(root.clone()));

        let read = |file: &str, key: &str| perform(&json!({"op": if file.ends_with(".json") { "read_file_key" } else { "read_ini_key" },
                                                           "path": root.join(file).to_string_lossy(), "key": key}));
        assert_eq!(read("app.json", "version"), Some(json!("1.2.3")));
        assert_eq!(read("app.json", "n"), Some(json!(7)));
        assert_eq!(read("app.json", "nested"), None, "an object travelled under a key");
        assert_eq!(read("app.json", "list"), None, "an array travelled under a key");
        assert_eq!(read("app.json", "long"), None, "a paragraph travelled under a key");
        assert_eq!(read("big.json", "version"), None, "a file past the size limit was opened");
        assert_eq!(read("settings.cfg", "home"), Some(json!("/usr/bin")));
        assert_eq!(read("settings.cfg", "long"), None, "a paragraph travelled under an ini key");

        set_project_root(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PV9: a program is started from an empty directory of its own, and the
    /// directory is gone when the program is. The working directory is an
    /// argument to every tool that reads its surroundings, and it was
    /// inherited from wherever the client had been launched.
    #[test]
    fn a_program_runs_in_an_empty_directory_of_its_own() {
        let (exe, args): (PathBuf, Vec<&str>) = if cfg!(windows) {
            (PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
                .join("System32").join("cmd.exe"), vec!["/c", "cd"])
        } else {
            (find_on_path("pwd").expect("no pwd"), vec![])
        };
        let (ok, out, _) = run_bounded_split(&exe, &args).expect("the program did not run");
        assert!(ok);
        let cwd = PathBuf::from(out.trim());
        assert!(cwd.starts_with(std::env::temp_dir()) || cwd.to_string_lossy().contains("podshl-run-"),
                "the program was not started from a scratch directory: {}", cwd.display());
        assert_ne!(cwd, std::env::current_dir().unwrap(), "the program inherited the client's working directory");
        assert!(!cwd.exists(), "the scratch directory outlived the run: {}", cwd.display());
    }

    /// A development root is a development feature. A release build must not
    /// read it at all, or one environment variable widens every publisher's
    /// reach on a shipped binary.
    #[test]
    fn the_development_root_is_only_read_in_a_development_build() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/reads.rs"))
            .unwrap()
            .replace("\r\n", "\n");
        let reads_var: Vec<&str> = src.lines()
            .filter(|l| l.contains("std::env::var(\"VS_EXTRA_ROOT\")") && !l.trim_start().starts_with("//"))
            .collect();
        assert_eq!(reads_var.len(), 1, "VS_EXTRA_ROOT is read in more than one place: {reads_var:?}");
        let body = src.split("fn extra_root()").nth(1).expect("extra_root is gone");
        let body = &body[..body.find("\n}\n").unwrap()];
        assert!(body.contains("cfg!(debug_assertions)"), "the development root is not gated on the build");
    }
}
