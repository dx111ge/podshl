//! The command-line path, exercised as a process rather than as a function.
//!
//! These need the built binary, which is why they are here rather than in a
//! unit test: `CARGO_BIN_EXE_` exists only for integration tests.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_podshl-client"));
    c.current_dir(env!("CARGO_MANIFEST_DIR"));
    c.env("VS_ROOT", "../var");
    c
}

/// A reader that goes away must not kill the program.
///
/// The Rust runtime sets SIGPIPE to SIG_IGN, so writing to a closed pipe returns
/// EPIPE rather than ending the process — and `println!` turns that into a
/// panic, which `panic = "abort"` turns into SIGABRT and a multi-megabyte core
/// dump. `podshl-client demo | head` did exactly that, once per invocation, and
/// the only evidence was a core file in a directory nobody reads: the output
/// looked right and the pipeline exited 0, because `head` did.
///
/// **This test has to use `demo`, and that is the interesting part.** An earlier
/// version used `doctor` and passed whether the bug was present or not: its
/// whole output fits in the 64 KB pipe buffer, so the child finishes writing and
/// exits cleanly before the reader is gone. Only a program that writes more than
/// the buffer, or writes slowly enough to still be going when the reader leaves,
/// can demonstrate this at all.
#[test]
fn closing_the_pipe_early_does_not_kill_it() {
    let mut child = bin()
        .arg("demo")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("could not start the binary");

    // Read a couple of lines, then drop the pipe — which is what `| head` does.
    {
        let out = child.stdout.take().unwrap();
        let mut reader = BufReader::new(out);
        let mut line = String::new();
        for _ in 0..2 {
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
        }
    }

    let status = child.wait().expect("could not wait for the binary");

    // A panic looks different in the two profiles, and the test has to catch it
    // in both: `cargo test` builds dev, where a panic unwinds to exit code 101,
    // while the shipped release build has `panic = "abort"` and turns the same
    // panic into SIGABRT and a core dump. Checking only for the signal made this
    // test pass against the very bug it was written for.
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let signal = status.signal();
        let code = status.code();

        assert_ne!(
            signal,
            Some(6),
            "the demo aborted when its reader went away. The SIGPIPE disposition \
was not restored on the command-line path, so every truncated pipe leaves a \
core dump."
        );
        assert_ne!(
            code,
            Some(101),
            "the demo panicked when its reader went away — `failed printing to \
stdout: Broken pipe`. In the release profile that same panic becomes SIGABRT \
and a core file."
        );

        // 13 is SIGPIPE: the ordinary, quiet way a Unix program ends when its
        // reader leaves. Anything else means it never reached the closed pipe,
        // which proves nothing — so say so rather than passing quietly.
        if signal != Some(13) {
            assert!(
                code.is_some_and(|c| c != 0),
                "the demo neither hit the closed pipe nor refused: signal={signal:?} \
code={code:?}. Start the counterparty with `mise run services`, or this test is \
vacuous."
            );
        }
    }
    let _ = status;
}

/// L8: the client can say which version it is. It now asks other programs
/// exactly that question — `program_version` — and a support tool that cannot
/// answer it about itself would be asking for something it does not give.
/// Answered in the shape its own reading understands: the name, then the number.
#[test]
fn it_says_which_version_it_is() {
    let out = bin()
        .arg("--version")
        .output()
        .expect("could not start the binary");
    assert!(out.status.success(), "--version was refused");
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        s.trim(),
        format!("podshl-client {}", env!("CARGO_PKG_VERSION")),
        "not the name and the version: {s:?}"
    );
}

/// An unknown subcommand is refused, and names what is possible. A tool that
/// answers a typo with a stack trace teaches people to stop reading errors.
#[test]
fn an_unknown_subcommand_is_refused_by_name() {
    let out = bin()
        .arg("definitely-not-a-command")
        .output()
        .expect("could not start the binary");
    assert!(!out.status.success(), "an unknown subcommand was accepted");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("doctor") && err.contains("demo"),
        "the refusal does not name what is possible: {err}"
    );
}

/// RR13: `restore` is the one repairs command that writes to disk, and it said
/// nothing at all — exit 0, and a file quietly rewritten.
///
/// **As a process, and that is the point.** The first version of this test
/// lived beside the code and built the sentence itself: it asserted that
/// `m_repair_cli_restored` exists and formats a path into it. That passes
/// whether or not `restore` ever prints it — and it did pass, with the
/// printing removed again. What has to be observed is the command's own
/// output, which means running the command.
#[test]
fn restore_says_which_file_it_put_back() {
    let dir = std::env::temp_dir().join(format!("podshl-restore-said-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("could not make a state directory");
    let file = dir.join("settings.ini");
    std::fs::write(&file, "scale=1\n").unwrap();

    let repairs = |args: &[&str]| {
        let out = bin()
            .env("VS_ROOT", &dir)
            .arg("repairs")
            .args(args)
            .output()
            .expect("could not start the binary");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };

    let (code, said, err) = repairs(&[
        "begin",
        "--kind",
        "file",
        "--by",
        "walk",
        "--path",
        &file.display().to_string(),
    ]);
    assert_eq!(code, Some(0), "begin was refused: {err}");
    let id = said.trim().to_string();
    assert!(!id.is_empty(), "begin printed no record id");

    std::fs::write(&file, "scale=1.25\n").unwrap();
    let (code, _, err) = repairs(&["done", &id]);
    assert_eq!(code, Some(0), "done was refused: {err}");

    let (code, said, err) = repairs(&["restore", &id]);
    assert_eq!(code, Some(0), "restore was refused: {err}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "scale=1\n",
        "restore did not put the copy back"
    );
    assert!(
        !said.trim().is_empty(),
        "restore rewrote the file and printed nothing at all"
    );
    assert!(
        said.contains("settings.ini"),
        "restore does not name the file it put back: {said:?}"
    );

    // The two that only keep the record straight stay quiet — the point is
    // that the one which touches a file is distinguishable from them.
    let (_, quiet, _) = repairs(&["keep", &id]);
    assert!(quiet.trim().is_empty(), "keep now prints too: {quiet:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// RR14: what the repair record did is in the log, including the one thing it
/// does over the network.
///
/// The feature wrote nothing at all. That matters most where it runs
/// unattended: the hook reviews after every update behind `|| true`, so a run
/// that found something, asked GitHub about it and notified had left no trace
/// but an exit code the hook throws away. And watching is the only part of
/// this client that talks to GitHub — a person told in a panel that each check
/// tells GitHub what this computer follows should be able to see afterwards
/// that it happened, without having been at the window.
#[test]
fn what_the_repair_record_did_is_in_the_log() {
    let dir = std::env::temp_dir().join(format!("podshl-repairlog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("client.log");
    let file = dir.join("settings.ini");
    std::fs::write(&file, "scale=1\n").unwrap();

    let repairs = |args: &[&str]| {
        let out = bin()
            .env("VS_ROOT", &dir)
            .env("PODSHL_LOG", &log)
            .arg("repairs")
            .args(args)
            .output()
            .expect("could not start the binary");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let id = repairs(&[
        "begin",
        "--kind",
        "file",
        "--by",
        "log walk",
        "--path",
        &file.display().to_string(),
    ])
    .trim()
    .to_string();
    std::fs::write(&file, "scale=1.25\n").unwrap();
    repairs(&["done", &id]);
    std::fs::write(&file, "scale=2\n").unwrap();
    // `--offline` so the case asks nobody: what is checked here is that the
    // review says what it concluded, not what GitHub answers today.
    repairs(&["review", "--offline"]);
    repairs(&["restore", &id]);

    let text = std::fs::read_to_string(&log).expect("nothing was logged at all");
    for want in [
        "repairs begin",
        "repairs done",
        "repairs review",
        "repairs restore",
    ] {
        assert!(
            text.contains(want),
            "the log does not say {want:?}:\n{text}"
        );
    }
    assert!(text.contains(&id), "the log names no record id:\n{text}");
    // The flag is named by its own name, not by a translated sentence.
    assert!(
        text.contains("file_changed"),
        "the review logged no flag:\n{text}"
    );
    assert!(
        text.contains("1 want another look") || text.contains("want another look"),
        "the review logged no conclusion:\n{text}"
    );
    // A log is the thing people paste into an issue, so it is anonymised like
    // a report. The account name must not be in it.
    if let Ok(user) = std::env::var("USERNAME").or_else(|_| std::env::var("USER")) {
        if user.len() > 2 {
            assert!(
                !text.contains(&user),
                "the log carries the account name:\n{text}"
            );
        }
    }
    // `PODSHL_LOG=0` is off, and off means nothing written.
    let off = dir.join("off.log");
    let _ = bin()
        .env("VS_ROOT", &dir)
        .env("PODSHL_LOG", "0")
        .args(["repairs", "review", "--offline"])
        .output();
    assert!(!off.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

/// RR16: `forget` never removes a record unattended.
///
/// **The one property worth asserting against the real process**, and the one
/// that does not depend on how the machine running it is set up. A test is not
/// privileged on a developer's Windows box and is root in the CI container, so
/// *which* gate refuses differs — that both of them cannot be got past by
/// something with no person behind it does not.
///
/// This is what the record is for. The agents, skills and scripts it keeps
/// track of run as the person; a removal they could perform is a removal the
/// thing being recorded could perform, and the entry would be worth nothing
/// the moment it mattered.
#[test]
fn forget_refuses_when_nobody_is_there_whatever_the_privilege() {
    let dir = std::env::temp_dir().join(format!("podshl-forget-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.conf");
    std::fs::write(&file, "before\n").unwrap();

    let repairs = |args: &[&str]| {
        let out = bin()
            .env("VS_ROOT", &dir)
            .arg("repairs")
            .args(args)
            .output()
            .expect("could not start the binary");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };

    let (_, id, _) = repairs(&[
        "begin",
        "--kind",
        "file",
        "--by",
        "an agent",
        "--path",
        &file.display().to_string(),
    ]);
    let id = id.trim().to_string();
    std::fs::write(&file, "after\n").unwrap();
    repairs(&["done", &id]);

    // stdin here is not a terminal, and a test process is not elevated on
    // Windows. Either way: refused, by name, and the record survives.
    let (code, _, err) = repairs(&["forget", &id]);
    assert_eq!(code, Some(1), "forget did not refuse: {err}");
    assert!(!err.trim().is_empty(), "forget refused without saying why");

    let ledger = std::fs::read_to_string(dir.join("repairs.json")).unwrap();
    assert!(
        ledger.contains(&id),
        "the record was removed with nobody there:\n{ledger}"
    );
    assert!(
        !ledger.contains("\"forgotten\""),
        "the record was removed with nobody there:\n{ledger}"
    );

    // Feeding it the id on stdin is not being there either: stdin is a pipe.
    let piped = bin()
        .env("VS_ROOT", &dir)
        .args(["repairs", "forget", &id])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("could not start the binary");
    let mut piped = piped;
    use std::io::Write as _;
    if let Some(mut si) = piped.stdin.take() {
        let _ = si.write_all(format!("{id}\n").as_bytes());
    }
    let out = piped.wait_with_output().expect("could not wait");
    assert!(
        !out.status.success(),
        "forget accepted an id typed by a pipe"
    );
    let ledger = std::fs::read_to_string(dir.join("repairs.json")).unwrap();
    assert!(
        !ledger.contains("\"forgotten\""),
        "a pipe removed a record:\n{ledger}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The same record from both programs: `podshl-repairs` is the record without
/// the window, not a second record.
fn standalone() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_podshl-repairs"));
    c.current_dir(env!("CARGO_MANIFEST_DIR"));
    c.env("VS_ROOT", "../var");
    c
}

/// RR17: a fix recorded by one program is seen, flagged and put back by the
/// other.
///
/// The two share code, and that is not what is being claimed here — they
/// could share code and still disagree about where the record lives, or how
/// it is written. Claimed is that somebody who starts with `podshl-repairs`
/// and later installs the client, or the other way round, has one record.
/// So each step is taken by the program that did not take the last one.
#[test]
fn a_fix_recorded_by_one_program_is_seen_by_the_other() {
    let dir = std::env::temp_dir().join(format!("podshl-standalone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("could not make a state directory");
    let file = dir.join("app.conf");
    std::fs::write(&file, "mode=old\n").unwrap();

    let run = |mut c: Command, args: &[&str]| {
        let out = c
            .env("VS_ROOT", &dir)
            .args(args)
            .output()
            .expect("could not start");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let client = |args: &[&str]| {
        let mut c = bin();
        c.arg("repairs");
        run(c, args)
    };
    let alone = |args: &[&str]| run(standalone(), args);

    let path = file.display().to_string();
    let (code, id, err) = alone(&["begin", "--kind", "file", "--by", "walk", "--path", &path]);
    assert_eq!(code, Some(0), "podshl-repairs begin was refused: {err}");
    let id = id.trim().to_string();
    assert!(!id.is_empty(), "begin printed no record id");

    std::fs::write(&file, "mode=new\n").unwrap();
    let (code, _, err) = client(&["done", &id]);
    assert_eq!(
        code,
        Some(0),
        "the client does not know the record podshl-repairs began: {err}"
    );

    // Changed again behind both programs' backs: something to look at.
    std::fs::write(&file, "mode=other\n").unwrap();
    let (code, said, err) = alone(&["review", "--offline"]);
    assert_eq!(
        code,
        Some(3),
        "podshl-repairs saw nothing to look at: {said} {err}"
    );
    assert!(
        said.contains("app.conf"),
        "the review does not name the file: {said:?}"
    );

    let (_, listed_alone, _) = alone(&["list", "--json"]);
    let (_, listed_client, _) = client(&["list", "--json"]);
    assert_eq!(
        listed_alone, listed_client,
        "the two programs list different records"
    );

    let (code, _, err) = client(&["restore", &id]);
    assert_eq!(
        code,
        Some(0),
        "the client could not restore what podshl-repairs kept: {err}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mode=old\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// RR18: `podshl-repairs` carries no window.
///
/// The reason it exists is what it does not need: WebKitGTK on Linux is a
/// dependency people on a terminal do not want for a record of their fixes,
/// and on Windows the client is a window program, which a shell does not wait
/// for — the exit code a hook reads needs `Start-Process -Wait` there. Both
/// are properties of the file that is built, so the file is what is checked:
/// on Linux, the libraries it asks the loader for; on Windows, the subsystem
/// in its header. A debug build of the client is a console program too, so on
/// Windows this says nothing about the client, only about this one.
#[test]
fn the_standalone_record_carries_no_window() {
    let exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_podshl-repairs"));

    #[cfg(target_os = "linux")]
    {
        let out = Command::new("ldd")
            .arg(&exe)
            .output()
            .expect("ldd is not available");
        let needed = String::from_utf8_lossy(&out.stdout).to_lowercase();
        assert!(
            out.status.success(),
            "ldd could not read the binary: {needed}"
        );
        for window in ["webkit", "gtk", "javascriptcore", "soup", "gdk"] {
            assert!(
                !needed.contains(window),
                "podshl-repairs links {window}:\n{needed}"
            );
        }
    }

    #[cfg(windows)]
    {
        let pe = std::fs::read(&exe).unwrap();
        let at = u32::from_le_bytes(pe[0x3c..0x40].try_into().unwrap()) as usize;
        assert_eq!(&pe[at..at + 4], b"PE\0\0", "not a PE file");
        // The optional header follows the 4-byte signature and the 20-byte
        // file header; `Subsystem` is at offset 68 in it, for PE32 and PE32+.
        let sub = u16::from_le_bytes(pe[at + 24 + 68..at + 24 + 70].try_into().unwrap());
        assert_eq!(
            sub, 3,
            "podshl-repairs is not a console program (subsystem {sub})"
        );
    }

    // And it answers as a program on its own, with the same usage as the
    // client's `repairs`.
    let out = standalone()
        .arg("help")
        .output()
        .expect("could not start podshl-repairs");
    let said =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("review"),
        "podshl-repairs does not say what it does: {said}"
    );
    // Under its own name: it began with "podshl-client repairs <command>",
    // a program somebody with only the record does not have.
    assert!(
        said.starts_with("podshl-repairs <command>"),
        "podshl-repairs introduces itself as another program: {said}"
    );
}

/// RR19: the hook `podshl-repairs` writes is one it can run.
///
/// `install-hook` puts the program that ran it into the hook, followed by
/// `repairs review --notify` — the client's spelling. Written by
/// `podshl-repairs`, that line was `podshl-repairs repairs review --notify`,
/// which it refused as an unknown command: every day, behind the hook's
/// `|| true`, with no one to see it.
///
/// So the hook is not compared with a string. The plan `install-hook` would
/// carry out on this system is asked for — the unit file on Linux, the plist
/// on macOS, the task's command on Windows, contents and all — the line that
/// starts this program is taken apart, and what it passes is run.
#[test]
fn the_hook_podshl_repairs_writes_is_one_it_can_run() {
    use podshl_repairs::repairs_cli::{hook_plan, Step};

    let dir = std::env::temp_dir().join(format!("podshl-hookline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_podshl-repairs"));
    let exe_s = exe.to_string_lossy().into_owned();
    let plan = hook_plan(&exe, true).expect("no hook plan on this system");
    let text: Vec<String> = plan
        .iter()
        .map(|s| match s {
            Step::Write { body, .. } => body.clone(),
            Step::Run { program, args } => format!("{program} {}", args.join(" ")),
            Step::Remove { .. } => String::new(),
        })
        .collect();
    let text = text.join("\n");
    let line = text
        .lines()
        .find(|l| l.contains(&exe_s) && l.contains("review"))
        .unwrap_or_else(|| panic!("nothing in the hook starts {exe_s}:\n{text}"));
    // What follows the program on that line is what the hook will pass it.
    let after = &line[line.find(&exe_s).unwrap() + exe_s.len()..];
    let words: Vec<&str> = after
        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>')
        .take_while(|w| *w != "||")
        .filter(|w| ["repairs", "review", "--notify"].contains(w))
        .collect();
    assert!(
        words.contains(&"review"),
        "no review in the hook line: {line}"
    );

    // Run it as the hook will, less the notification, which would reach this
    // desktop. Nothing is recorded, so the answer is "nothing to look at".
    let args: Vec<&str> = words.iter().copied().filter(|w| *w != "--notify").collect();
    let out = standalone()
        .env("VS_ROOT", &dir)
        .args(&args)
        .arg("--offline")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "the hook's own line fails: {args:?} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
