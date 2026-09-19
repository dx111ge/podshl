//! `podshl-elevate`: the only program in PODSHL that changes anything which
//! needs administrator rights. Started by the client through Windows' own
//! prompt, with the request as its arguments, and nothing else.
//!
//!     podshl-elevate.exe restart_service service=<name>
//!     podshl-elevate.exe set_service_start service=<name> start=auto|demand|disabled
//!     podshl-elevate.exe set_machine_env name=<NAME> value=<value, empty removes it>
//!
//! It checks the request again with the client's own rules
//! (`src/elevated.rs`), does that one thing, and exits with a code. It reads no
//! file, writes no file, opens no connection and starts no program.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "../elevated.rs"]
#[allow(dead_code)]
mod elevated;

use elevated::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match elevated::from_args(&args) {
        Err(_) => exit::REFUSED,
        Ok((id, params)) => perform(&id, &params),
    };
    std::process::exit(code);
}

#[cfg(not(windows))]
fn perform(_id: &str, _params: &std::collections::BTreeMap<String, String>) -> i32 {
    exit::NOT_WINDOWS
}

#[cfg(windows)]
fn perform(id: &str, params: &std::collections::BTreeMap<String, String>) -> i32 {
    match id {
        "restart_service" => win::restart_service(&params["service"]),
        "set_service_start" => win::set_service_start(&params["service"], &params["start"]),
        "set_machine_env" => win::set_machine_env(&params["name"], &params["value"]),
        _ => exit::REFUSED,
    }
}

#[cfg(windows)]
mod win {
    use super::elevated::{exit, wide, ENV_KEY};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::System::Services::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const WAIT: Duration = Duration::from_secs(30);

    fn code_for(err: u32) -> i32 {
        match err {
            ERROR_ACCESS_DENIED => exit::DENIED,
            ERROR_SERVICE_DOES_NOT_EXIST | ERROR_FILE_NOT_FOUND => exit::NOT_FOUND,
            _ => exit::FAILED,
        }
    }

    struct Handle(SC_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseServiceHandle(self.0) };
            }
        }
    }

    fn open(name: &str, access: u32) -> Result<(Handle, Handle), i32> {
        unsafe {
            let scm = Handle(OpenSCManagerW(
                std::ptr::null(),
                std::ptr::null(),
                SC_MANAGER_CONNECT,
            ));
            if scm.0.is_null() {
                return Err(code_for(GetLastError()));
            }
            let svc = Handle(OpenServiceW(scm.0, wide(name).as_ptr(), access));
            if svc.0.is_null() {
                return Err(code_for(GetLastError()));
            }
            Ok((scm, svc))
        }
    }

    fn state(svc: &Handle) -> Option<u32> {
        unsafe {
            let mut s: SERVICE_STATUS_PROCESS = std::mem::zeroed();
            let mut needed = 0u32;
            (QueryServiceStatusEx(
                svc.0,
                SC_STATUS_PROCESS_INFO,
                &mut s as *mut _ as *mut u8,
                std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut needed,
            ) != 0)
                .then_some(s.dwCurrentState)
        }
    }

    fn wait_for(svc: &Handle, want: u32) -> bool {
        let until = Instant::now() + WAIT;
        while Instant::now() < until {
            if state(svc) == Some(want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    }

    pub fn restart_service(name: &str) -> i32 {
        let (_scm, svc) = match open(name, SERVICE_QUERY_STATUS | SERVICE_STOP | SERVICE_START) {
            Ok(h) => h,
            Err(c) => return c,
        };
        unsafe {
            if state(&svc) != Some(SERVICE_STOPPED) {
                let mut s: SERVICE_STATUS = std::mem::zeroed();
                if ControlService(svc.0, SERVICE_CONTROL_STOP, &mut s) == 0 {
                    let e = GetLastError();
                    if e != ERROR_SERVICE_NOT_ACTIVE {
                        return code_for(e);
                    }
                }
                if !wait_for(&svc, SERVICE_STOPPED) {
                    return exit::TIMED_OUT;
                }
            }
            if StartServiceW(svc.0, 0, std::ptr::null()) == 0 {
                return code_for(GetLastError());
            }
        }
        if wait_for(&svc, SERVICE_RUNNING) {
            exit::DONE
        } else {
            exit::TIMED_OUT
        }
    }

    pub fn set_service_start(name: &str, start: &str) -> i32 {
        let wanted = match start {
            "auto" => SERVICE_AUTO_START,
            "demand" => SERVICE_DEMAND_START,
            "disabled" => SERVICE_DISABLED,
            _ => return exit::REFUSED,
        };
        let (_scm, svc) = match open(name, SERVICE_QUERY_CONFIG | SERVICE_CHANGE_CONFIG) {
            Ok(h) => h,
            Err(c) => return c,
        };
        unsafe {
            // A driver or a service Windows starts at boot is not one these
            // examples move, whatever its name.
            let mut buf = vec![0u64; 1024];
            let cfg = buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;
            let mut needed = 0u32;
            if QueryServiceConfigW(svc.0, cfg, (buf.len() * 8) as u32, &mut needed) == 0 {
                return code_for(GetLastError());
            }
            if ![SERVICE_AUTO_START, SERVICE_DEMAND_START, SERVICE_DISABLED]
                .contains(&(*cfg).dwStartType)
            {
                return exit::START_TYPE_NOT_OURS;
            }
            if ChangeServiceConfigW(
                svc.0,
                SERVICE_NO_CHANGE,
                wanted,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            ) == 0
            {
                return code_for(GetLastError());
            }
        }
        exit::DONE
    }

    pub fn set_machine_env(name: &str, value: &str) -> i32 {
        unsafe {
            let mut key = std::ptr::null_mut();
            let opened = RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(ENV_KEY).as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut key,
            );
            if opened != 0 {
                return code_for(opened);
            }
            let n = wide(name);
            let result = if value.is_empty() {
                match RegDeleteValueW(key, n.as_ptr()) {
                    ERROR_FILE_NOT_FOUND => 0,
                    other => other,
                }
            } else {
                // The existing type is kept: a variable Windows expands stays
                // one it expands.
                let mut kind = 0u32;
                let mut len = 0u32;
                if RegQueryValueExW(
                    key,
                    n.as_ptr(),
                    std::ptr::null(),
                    &mut kind,
                    std::ptr::null_mut(),
                    &mut len,
                ) != 0
                    || (kind != REG_SZ && kind != REG_EXPAND_SZ)
                {
                    kind = REG_SZ;
                }
                let data = wide(value);
                RegSetValueExW(
                    key,
                    n.as_ptr(),
                    0,
                    kind,
                    data.as_ptr() as *const u8,
                    (data.len() * 2) as u32,
                )
            };
            RegCloseKey(key);
            if result != 0 {
                return code_for(result);
            }
            // Programs started from now on see it; running ones are told.
            let env = wide("Environment");
            let mut _r = 0usize;
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                env.as_ptr() as LPARAM,
                SMTO_ABORTIFHUNG,
                2000,
                &mut _r,
            );
        }
        exit::DONE
    }
}
