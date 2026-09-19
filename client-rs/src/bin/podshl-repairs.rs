//! `podshl-repairs`: the record of local fixes, without the rest of PODSHL.
//!
//! The same commands as `podshl-client repairs …`, from the same code
//! (`repairs_cli`), on the same `repairs.json`. What is different is what it
//! is not: no window, so no WebKitGTK or WebView2 to install, and on Windows a
//! console program, so a shell waits for it and sees its exit code without
//! `Start-Process -Wait`.
//!
//! Nothing from the protocol is linked in. That is not a promise this file
//! makes; it is what `src/lib.rs` contains, and `RR18` checks the binary.

fn main() {
    podshl_repairs::repairs_cli::run_as_standalone();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // **`repairs` in front is accepted, and ignored.** `install-hook` writes
    // the running program followed by `repairs review --notify` into a hook,
    // because that is how the client is called. Run from here, that line
    // names this program — and without this, the hook it wrote would fail
    // every day with "unknown repairs command", behind the `|| true` that
    // keeps an update from failing on it, so nobody would ever see it.
    if args.first().is_some_and(|a| a == "repairs") {
        args.remove(0);
    }
    podshl_repairs::repairs_cli::exit_with(args);
}
