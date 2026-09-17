//! Asking Windows to run `podshl-elevate` as administrator, and checking what
//! it did. The rules and the reads are in `elevated.rs`; the writing is in the
//! helper; this file only starts it and looks.
//!
//! The client reads the state before, starts the helper with the request as
//! its arguments — Windows shows the person its own prompt, and a refusal there
//! ends it — waits for the exit code, and reads the state again. A change is
//! reported as made only when the second reading says so, whatever the helper
//! answered.

use crate::elevated::{self, exit, Refusal, State};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const HELPER: &str = "podshl-elevate.exe";

/// Next to the client, where the installer puts it — and nowhere else. A
/// helper looked up on `PATH` is one anybody could have put there.
fn helper() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(HELPER)).filter(|p| p.is_file())
}

fn describe(s: &State) -> Value {
    match s {
        State::Service { running, start } => json!({"running": running, "start": start}),
        State::Env(v) => json!({"value": v}),
        State::Missing => Value::Null,
    }
}

/// The request the helper answers, checked with the helper's own rules first.
pub fn check(id: &str, p: &BTreeMap<String, String>) -> Result<(), String> {
    match elevated::check(id, p) {
        Ok(()) => Ok(()),
        Err(Refusal::Protected) => Err(m!("elevated_protected", id = id)),
        Err(_) => Err(m!("elevated_refused", id = id)),
    }
}

/// Whether the reading after the helper says the change is there.
fn holds(id: &str, p: &BTreeMap<String, String>, s: &State) -> Option<bool> {
    match (id, s) {
        (_, State::Missing) => None,
        ("restart_service", State::Service { running, .. }) => Some(*running),
        ("set_service_start", State::Service { start, .. }) => Some(start == &p["start"]),
        ("set_machine_env", State::Env(v)) => Some(v.as_deref().unwrap_or("") == p["value"]),
        _ => None,
    }
}

/// What reverses the change, as an action call, from the reading before it.
fn undo_for(id: &str, p: &BTreeMap<String, String>, before: &State) -> Value {
    match (id, before) {
        ("set_service_start", State::Service { start, .. })
            if ["auto", "demand", "disabled"].contains(&start.as_str()) =>
        {
            json!({"action": id, "params": {"service": p["service"], "start": start}})
        }
        ("set_machine_env", State::Env(v)) => {
            json!({"action": id, "params": {"name": p["name"], "value": v.clone().unwrap_or_default()}})
        }
        _ => Value::Null,
    }
}

/// Whether a change still says what it set — for the repair record's review.
pub fn still_holds(id: &str, p: &BTreeMap<String, String>) -> Option<bool> {
    match id {
        "restart_service" => None,
        _ => holds(id, p, &elevated::read(id, p)),
    }
}

pub fn run(id: &str, p: &BTreeMap<String, String>) -> Result<Value, String> {
    check(id, p)?;
    if !cfg!(windows) {
        return Err(m!("elevated_windows_only", id = id));
    }
    let before = elevated::read(id, p);
    if before == State::Missing {
        return Err(m!("elevated_not_found", id = id));
    }
    let exe = helper().ok_or_else(|| m!("elevated_helper_missing", h = HELPER))?;
    let code = launch(&exe, &elevated::to_args(id, p))?;
    match code {
        exit::DONE => {}
        exit::DENIED => return Err(m!("elevated_denied")),
        exit::NOT_FOUND => return Err(m!("elevated_not_found", id = id)),
        exit::TIMED_OUT => return Err(m!("elevated_timed_out")),
        exit::START_TYPE_NOT_OURS => return Err(m!("elevated_start_type")),
        exit::REFUSED => return Err(m!("elevated_refused", id = id)),
        other => return Err(m!("elevated_failed", c = other)),
    }
    let after = elevated::read(id, p);
    if holds(id, p, &after) != Some(true) {
        return Err(m!("elevated_not_applied"));
    }
    Ok(json!({
        "elevated": true,
        "before": describe(&before),
        "after": describe(&after),
        "undo": undo_for(id, p, &before),
    }))
}

#[cfg(not(windows))]
fn launch(_exe: &std::path::Path, _args: &[String]) -> Result<i32, String> {
    Err(m!("elevated_windows_only", id = ""))
}

/// `ShellExecuteEx` with the `runas` verb: Windows asks the person, and only
/// then starts the helper. Nothing here holds or asks for a privilege itself.
#[cfg(windows)]
fn launch(exe: &std::path::Path, args: &[String]) -> Result<i32, String> {
    use elevated::wide;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let file = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    // Each value was matched against a pattern with no space and no quote, so
    // joining with spaces is the whole of the quoting.
    let params = wide(&args.join(" "));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            let e = GetLastError();
            return Err(if e == ERROR_CANCELLED {
                m!("elevated_declined")
            } else {
                m!("elevated_failed", c = e)
            });
        }
        if info.hProcess.is_null() {
            return Err(m!("elevated_failed", c = "no process"));
        }
        // The helper gives up on a service after thirty seconds per step.
        let waited = WaitForSingleObject(info.hProcess, 120_000);
        let mut code = 0u32;
        let got = GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if waited != WAIT_OBJECT_0 || got == 0 {
            return Err(m!("elevated_timed_out"));
        }
        Ok(code as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// L5: the client never writes what needs privilege. The calls that do are
    /// in the helper's source and in no file the client is built from.
    #[test]
    fn nothing_privileged_is_written_in_process() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let writes = ["ChangeServiceConfigW", "ControlService(", "StartServiceW",
                      "RegSetValueExW", "RegDeleteValueW", "SERVICE_CHANGE_CONFIG"];
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // This test names them, and names them only in this list.
            let text = if path.ends_with("elevate.rs") {
                text.split("#[cfg(test)]").next().unwrap_or("").to_string()
            } else {
                text
            };
            for w in writes {
                assert!(!text.contains(w), "{} calls {w}, which only the helper may", path.display());
            }
        }
        let helper = std::fs::read_to_string(src.join("bin").join("podshl-elevate.rs")).unwrap();
        for w in ["ChangeServiceConfigW", "ControlService(", "StartServiceW", "RegSetValueExW"] {
            assert!(helper.contains(w), "the helper no longer performs {w}");
        }
        for never in ["Command::new", "std::fs::", "TcpStream", "reqwest", "File::"] {
            assert!(!helper.contains(never), "the helper uses {never}; it reads no file, \
                     writes none, starts nothing and opens no connection");
        }
    }

    /// The change counts as made only when the reading afterwards shows it,
    /// and its undo is the reading before.
    #[test]
    fn a_change_is_what_the_second_reading_shows_and_its_undo_is_the_first() {
        let svc = p(&[("service", "Spooler"), ("start", "disabled")]);
        let before = State::Service { running: true, start: "demand".into() };
        assert_eq!(holds("set_service_start", &svc, &before), Some(false));
        assert_eq!(holds("set_service_start", &svc,
                         &State::Service { running: false, start: "disabled".into() }), Some(true));
        assert_eq!(undo_for("set_service_start", &svc, &before),
                   json!({"action": "set_service_start", "params": {"service": "Spooler", "start": "demand"}}));
        assert_eq!(undo_for("set_service_start", &svc,
                            &State::Service { running: true, start: "boot".into() }), Value::Null,
                   "an undo into a start type the helper will not set was offered");

        let env = p(&[("name", "OLLAMA_HOST"), ("value", "0.0.0.0:11434")]);
        assert_eq!(undo_for("set_machine_env", &env, &State::Env(None)),
                   json!({"action": "set_machine_env", "params": {"name": "OLLAMA_HOST", "value": ""}}),
                   "undoing a variable that did not exist is removing it");
        assert_eq!(holds("set_machine_env", &p(&[("name", "X"), ("value", "")]), &State::Env(None)), Some(true));
        assert_eq!(holds("set_machine_env", &env, &State::Missing), None);
        assert_eq!(undo_for("restart_service", &p(&[("service", "Spooler")]), &before), Value::Null);
    }

    /// Refused before Windows is asked anything.
    #[test]
    fn a_refused_request_never_reaches_the_prompt() {
        let err = run("restart_service", &p(&[("service", "RpcSs")])).unwrap_err();
        assert!(crate::msg::is("elevated_protected", &err), "{err}");
        let err = run("set_machine_env", &p(&[("name", "X"), ("value", "a b")])).unwrap_err();
        assert!(crate::msg::is("elevated_refused", &err), "{err}");
    }

    /// EV2, walked by hand: Windows asks twice and a person says yes both
    /// times. Sets a variable nothing reads, reads it back, and undoes it.
    ///
    ///     cargo test walk_the_prompt -- --ignored --nocapture
    #[cfg(windows)]
    #[test]
    #[ignore = "needs a person to answer Windows' administrator prompt"]
    fn walk_the_prompt() {
        let name = "PODSHL_WALK";
        let set = p(&[("name", name), ("value", "walked")]);
        let before = elevated::read("set_machine_env", &set);
        // `cargo test` builds the helper beside the test program's folder, not
        // beside the test program itself; the walk uses the one that ships.
        let out = run("set_machine_env", &set).expect("the change was not made");
        println!("changed: {out}");
        assert_eq!(elevated::read("set_machine_env", &set), State::Env(Some("walked".into())));
        let undo = &out["undo"];
        let params: BTreeMap<String, String> = undo["params"].as_object().unwrap().iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
        run(undo["action"].as_str().unwrap(), &params).expect("the undo was not made");
        assert_eq!(elevated::read("set_machine_env", &set), before, "the undo did not restore it");
    }

    /// Reading needs no privilege, and a service that is not there is said.
    #[cfg(windows)]
    #[test]
    fn the_state_is_read_without_privilege() {
        match elevated::read("restart_service", &p(&[("service", "Spooler")])) {
            State::Service { start, .. } => assert!(!start.is_empty()),
            State::Missing => {} // a machine without the print spooler
            other => panic!("{other:?}"),
        }
        assert_eq!(elevated::read("restart_service", &p(&[("service", "PodshlNoSuchService")])),
                   State::Missing);
        let err = run("restart_service", &p(&[("service", "PodshlNoSuchService")])).unwrap_err();
        assert!(crate::msg::is("elevated_not_found", &err), "{err}");
        assert!(matches!(elevated::read("set_machine_env", &p(&[("name", "OS"), ("value", "")])),
                         State::Env(Some(_))), "the machine's OS variable was not read");
    }
}
