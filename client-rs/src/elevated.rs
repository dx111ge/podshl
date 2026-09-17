//! The actions that need administrator rights, shared by the client and by the
//! helper that performs them (`src/bin/podshl-elevate.rs`).
//!
//! **Why a separate program.** An application that can raise its own
//! privileges can do anything it is later talked into, so it cannot honestly
//! claim bounded effect. The client therefore never writes to a service or to
//! the machine's registry. It asks Windows to start `podshl-elevate.exe` as
//! administrator — Windows shows the person its own prompt — and that program
//! knows these three actions and nothing else, checks every parameter again
//! with the same rules, does the one thing, and exits.
//!
//! **No channel but the command line.** The request is the program's arguments,
//! which nothing can change once Windows has started it, and the answer is its
//! exit code. A request or result file would sit in a folder the person's
//! other programs can write to, and a program running as administrator must
//! not read instructions from, or write into, such a place. What the change
//! did is read back by the client itself, without privilege: a service's state
//! and start type, and the machine's environment, are readable by anybody.
//!
//! Everything here is plain data and checks, plus the unprivileged reads. The
//! writing is in the helper only, and `L5` holds that.

use std::collections::BTreeMap;

pub const SERVICE: &str = r"[A-Za-z0-9_.-]{1,80}";
pub const START: &str = r"auto|demand|disabled";
pub const ENV_NAME: &str = r"[A-Za-z_][A-Za-z0-9_]{0,63}";
/// Empty removes the variable. No spaces, quotes or `;`: a value that needs
/// quoting on a command line is refused rather than quoted, and a list is not
/// something these examples set.
pub const ENV_VALUE: &str = r"[\w.:\\/+-]{0,200}";

/// The ids the helper performs, with their parameters, in the order the
/// command line carries them.
pub const ACTIONS: &[(&str, &[(&str, &str)])] = &[
    ("restart_service", &[("service", SERVICE)]),
    ("set_service_start", &[("service", SERVICE), ("start", START)]),
    ("set_machine_env", &[("name", ENV_NAME), ("value", ENV_VALUE)]),
];

/// Services these examples will not touch: stopping or disabling them stops
/// Windows, its updates or its defences. Case does not matter to Windows, so
/// it does not matter here.
pub const PROTECTED_SERVICES: &[&str] = &[
    "BFE", "BrokerInfrastructure", "CryptSvc", "DcomLaunch", "EventLog", "gpsvc",
    "LSM", "mpssvc", "PlugPlay", "Power", "ProfSvc", "RpcEptMapper", "RpcSs",
    "SamSs", "Schedule", "SecurityHealthService", "SENS", "SystemEventsBroker",
    "TrustedInstaller", "WdNisSvc", "WinDefend", "Winmgmt", "wscsvc", "wuauserv",
];

/// Variables Windows and every program read to find themselves.
pub const PROTECTED_ENV: &[&str] = &[
    "ALLUSERSPROFILE", "APPDATA", "COMPUTERNAME", "COMSPEC", "DRIVERDATA",
    "NUMBER_OF_PROCESSORS", "OS", "PATH", "PATHEXT", "PROCESSOR_ARCHITECTURE",
    "PROGRAMDATA", "PROGRAMFILES", "PROGRAMW6432", "PSMODULEPATH", "PUBLIC",
    "SYSTEMDRIVE", "SYSTEMROOT", "TEMP", "TMP", "USERNAME", "USERPROFILE", "WINDIR",
];

/// Why a request is refused. The helper exits with the code, the client says
/// the sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Unknown,
    BadParams,
    Protected,
}

pub fn is_elevated(id: &str) -> bool {
    ACTIONS.iter().any(|(a, _)| *a == id)
}

fn full(pattern: &str, value: &str) -> bool {
    regex::Regex::new(&format!("^(?:{pattern})$")).map(|re| re.is_match(value)).unwrap_or(false)
}

/// The same check on both sides of the prompt: the client before asking, the
/// helper before doing.
pub fn check(id: &str, params: &BTreeMap<String, String>) -> Result<(), Refusal> {
    let (_, declared) = ACTIONS.iter().find(|(a, _)| *a == id).ok_or(Refusal::Unknown)?;
    if params.len() != declared.len() {
        return Err(Refusal::BadParams);
    }
    for (name, pattern) in declared.iter() {
        let v = params.get(*name).ok_or(Refusal::BadParams)?;
        if !full(pattern, v) {
            return Err(Refusal::BadParams);
        }
    }
    if let Some(s) = params.get("service") {
        if PROTECTED_SERVICES.iter().any(|p| p.eq_ignore_ascii_case(s)) {
            return Err(Refusal::Protected);
        }
    }
    if let Some(n) = params.get("name") {
        if PROTECTED_ENV.iter().any(|p| p.eq_ignore_ascii_case(n)) {
            return Err(Refusal::Protected);
        }
    }
    Ok(())
}

/// The helper's arguments: the action, then `key=value` in declared order.
pub fn to_args(id: &str, params: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = vec![id.to_string()];
    if let Some((_, declared)) = ACTIONS.iter().find(|(a, _)| *a == id) {
        for (name, _) in declared.iter() {
            out.push(format!("{name}={}", params.get(*name).map(String::as_str).unwrap_or("")));
        }
    }
    out
}

pub fn from_args(args: &[String]) -> Result<(String, BTreeMap<String, String>), Refusal> {
    let (id, rest) = args.split_first().ok_or(Refusal::Unknown)?;
    let mut params = BTreeMap::new();
    for a in rest {
        let (k, v) = a.split_once('=').ok_or(Refusal::BadParams)?;
        if params.insert(k.to_string(), v.to_string()).is_some() {
            return Err(Refusal::BadParams);
        }
    }
    check(id, &params)?;
    Ok((id.clone(), params))
}

/// The helper's exit codes. `0` is done; the rest say which kind of not.
pub mod exit {
    pub const DONE: i32 = 0;
    pub const REFUSED: i32 = 10;
    pub const DENIED: i32 = 11;
    pub const NOT_FOUND: i32 = 12;
    pub const TIMED_OUT: i32 = 13;
    pub const FAILED: i32 = 14;
    pub const NOT_WINDOWS: i32 = 15;
    pub const START_TYPE_NOT_OURS: i32 = 16;
}

/// `HKLM\...\Session Manager\Environment`, where the machine's variables live.
pub const ENV_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// What can be read about the thing an action changes, without privilege.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// A service: whether it runs, and how it starts.
    Service { running: bool, start: String },
    /// A machine variable, or `None` where it is not set.
    Env(Option<String>),
    Missing,
}

#[cfg(windows)]
pub fn read(id: &str, params: &BTreeMap<String, String>) -> State {
    match id {
        "restart_service" | "set_service_start" => read_service(&params["service"]),
        "set_machine_env" => read_env(&params["name"]),
        _ => State::Missing,
    }
}

#[cfg(not(windows))]
pub fn read(_id: &str, _params: &BTreeMap<String, String>) -> State {
    State::Missing
}

#[cfg(windows)]
pub fn start_name(t: u32) -> &'static str {
    use windows_sys::Win32::System::Services::*;
    match t {
        SERVICE_AUTO_START => "auto",
        SERVICE_DEMAND_START => "demand",
        SERVICE_DISABLED => "disabled",
        SERVICE_BOOT_START => "boot",
        SERVICE_SYSTEM_START => "system",
        _ => "other",
    }
}

#[cfg(windows)]
fn read_service(name: &str) -> State {
    use windows_sys::Win32::System::Services::*;
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return State::Missing;
        }
        let svc = OpenServiceW(scm, wide(name).as_ptr(), SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG);
        if svc.is_null() {
            CloseServiceHandle(scm);
            return State::Missing;
        }
        let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let mut needed = 0u32;
        let ok = QueryServiceStatusEx(
            svc,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        );
        // Room for the fixed part and every string it points into, aligned.
        let mut buf = vec![0u64; 1024];
        let cfg = buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;
        let got = QueryServiceConfigW(svc, cfg, (buf.len() * 8) as u32, &mut needed);
        let state = if ok != 0 && got != 0 {
            State::Service {
                running: status.dwCurrentState == SERVICE_RUNNING,
                start: start_name((*cfg).dwStartType).to_string(),
            }
        } else {
            State::Missing
        };
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        state
    }
}

#[cfg(windows)]
fn read_env(name: &str) -> State {
    use windows_sys::Win32::System::Registry::*;
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(ENV_KEY).as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return State::Missing;
        }
        let mut kind = 0u32;
        let mut len = 0u32;
        let n = wide(name);
        let first = RegQueryValueExW(key, n.as_ptr(), std::ptr::null(), &mut kind,
                                     std::ptr::null_mut(), &mut len);
        let state = if first != 0 {
            State::Env(None)
        } else {
            let mut data = vec![0u16; (len as usize).div_ceil(2) + 1];
            let mut size = (data.len() * 2) as u32;
            if RegQueryValueExW(key, n.as_ptr(), std::ptr::null(), &mut kind,
                                data.as_mut_ptr() as *mut u8, &mut size) == 0
                && (kind == REG_SZ || kind == REG_EXPAND_SZ)
            {
                let end = data.iter().position(|&c| c == 0).unwrap_or(data.len());
                State::Env(Some(String::from_utf16_lossy(&data[..end])))
            } else {
                State::Missing
            }
        };
        RegCloseKey(key);
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn only_the_three_actions_with_their_parameters_pass() {
        assert_eq!(check("restart_service", &p(&[("service", "Spooler")])), Ok(()));
        assert_eq!(check("set_service_start", &p(&[("service", "Spooler"), ("start", "demand")])), Ok(()));
        assert_eq!(check("set_machine_env", &p(&[("name", "OLLAMA_HOST"), ("value", "127.0.0.1:11434")])), Ok(()));
        assert_eq!(check("set_machine_env", &p(&[("name", "OLLAMA_HOST"), ("value", "")])), Ok(()),
                   "an empty value is how a variable is removed");

        assert_eq!(check("run_powershell", &p(&[])), Err(Refusal::Unknown));
        for bad in [
            ("restart_service", p(&[("service", "Spooler & calc")])),
            ("restart_service", p(&[("service", "Spooler"), ("extra", "1")])),
            ("restart_service", p(&[])),
            ("set_service_start", p(&[("service", "Spooler"), ("start", "boot")])),
            ("set_machine_env", p(&[("name", "X"), ("value", "a b")])),
            ("set_machine_env", p(&[("name", "X"), ("value", "a\"b")])),
            ("set_machine_env", p(&[("name", "X"), ("value", "a;b")])),
            ("set_machine_env", p(&[("name", "1X"), ("value", "a")])),
        ] {
            assert_eq!(check(bad.0, &bad.1), Err(Refusal::BadParams), "{bad:?}");
        }
    }

    #[test]
    fn what_windows_needs_to_run_is_not_touched() {
        for s in ["RpcSs", "rpcss", "WinDefend", "wuauserv", "EventLog"] {
            assert_eq!(check("restart_service", &p(&[("service", s)])), Err(Refusal::Protected), "{s}");
            assert_eq!(check("set_service_start", &p(&[("service", s), ("start", "disabled")])),
                       Err(Refusal::Protected), "{s}");
        }
        for n in ["PATH", "Path", "ComSpec", "SystemRoot", "TEMP", "PSModulePath"] {
            assert_eq!(check("set_machine_env", &p(&[("name", n), ("value", "x")])),
                       Err(Refusal::Protected), "{n}");
        }
    }

    #[test]
    fn the_command_line_says_exactly_the_request_and_nothing_else_passes() {
        let params = p(&[("value", "C:\\ollama\\models"), ("name", "OLLAMA_MODELS")]);
        let args = to_args("set_machine_env", &params);
        assert_eq!(args, ["set_machine_env", "name=OLLAMA_MODELS", "value=C:\\ollama\\models"]);
        assert_eq!(from_args(&args), Ok(("set_machine_env".to_string(), params)));

        let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(from_args(&own(&[])), Err(Refusal::Unknown));
        assert_eq!(from_args(&own(&["set_machine_env", "name=A", "name=B", "value=x"])),
                   Err(Refusal::BadParams), "a repeated key was taken");
        assert_eq!(from_args(&own(&["set_machine_env", "name=A"])), Err(Refusal::BadParams));
        assert_eq!(from_args(&own(&["set_machine_env", "nameA", "value=x"])), Err(Refusal::BadParams));
        assert_eq!(from_args(&own(&["restart_service", "service=RpcSs"])), Err(Refusal::Protected));
    }
}
