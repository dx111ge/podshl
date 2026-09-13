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
            signal, Some(6),
            "the demo aborted when its reader went away. The SIGPIPE disposition \
was not restored on the command-line path, so every truncated pipe leaves a \
core dump."
        );
        assert_ne!(
            code, Some(101),
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
    let out = bin().arg("--version").output().expect("could not start the binary");
    assert!(out.status.success(), "--version was refused");
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(s.trim(), format!("podshl-client {}", env!("CARGO_PKG_VERSION")),
               "not the name and the version: {s:?}");
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
