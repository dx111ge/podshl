//! `L5`: the helper, run as the program it is.
//!
//! Without administrator rights it does nothing and says so; a request it does
//! not know is refused before anything is touched. The success path needs
//! Windows' prompt answered by a person, so it is walked by hand.

#[path = "../src/elevated.rs"]
#[allow(dead_code)]
mod elevated;

use elevated::exit;

/// Without administrator rights the helper does nothing and says so; with
/// a request it does not know, it refuses before touching anything.
#[test]
fn a_refused_or_unprivileged_request_changes_nothing() {
    let exe = env!("CARGO_BIN_EXE_podshl-elevate");
    let run = |args: &[&str]| {
        std::process::Command::new(exe)
            .args(args)
            .status()
            .expect("the helper did not start")
            .code()
    };
    assert_eq!(run(&[]), Some(exit::REFUSED));
    assert_eq!(
        run(&["run_powershell", "command=calc"]),
        Some(exit::REFUSED)
    );
    assert_eq!(
        run(&["set_machine_env", "name=PATH", "value=x"]),
        Some(exit::REFUSED)
    );

    #[cfg(windows)]
    {
        let name = "PODSHL_ELEVATE_TEST";
        let before = elevated::read(
            "set_machine_env",
            &[("name".to_string(), name.to_string())]
                .into_iter()
                .collect(),
        );
        let got = run(&["set_machine_env", &format!("name={name}"), "value=1"]);
        if is_elevated() {
            eprintln!("running as administrator; the unprivileged half is not checked");
        } else {
            assert_eq!(
                got,
                Some(exit::DENIED),
                "an unprivileged helper did not report denial"
            );
            let after = elevated::read(
                "set_machine_env",
                &[("name".to_string(), name.to_string())]
                    .into_iter()
                    .collect(),
            );
            assert_eq!(before, after, "a denied request changed the machine");
        }
        assert_eq!(
            run(&["restart_service", "service=PodshlNoSuchService"])
                .map(|c| c == exit::DENIED || c == exit::NOT_FOUND),
            Some(true)
        );
    }
    #[cfg(not(windows))]
    assert_eq!(
        run(&["restart_service", "service=cups"]),
        Some(exit::NOT_WINDOWS)
    );
}

#[cfg(windows)]
fn is_elevated() -> bool {
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
        let mut e: TOKEN_ELEVATION = std::mem::zeroed();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut e as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && e.TokenIsElevated != 0
    }
}
