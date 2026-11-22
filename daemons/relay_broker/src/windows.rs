// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Windows broker: a named pipe in place of the Unix socket, the pipe client's user SID in
//! place of `SO_PEERCRED`, and `SetSystemTime` in place of `clock_settime`. It serves only the two
//! clock requests (`time_status`, `set_system_time`); every other request is answered `Denied`
//! because the device, group, hub and package actions are Linux actions. The clock ceilings are
//! the same `time_guard` the Linux broker uses.
//!
//! Who may connect is decided twice: the pipe's DACL (built from the `allow_sid` lines, plus
//! SYSTEM and Administrators) stops everyone else at open time, and after the first bytes arrive the
//! broker impersonates the client, reads its user SID and refuses any SID not on the list, so a
//! wrong ACL alone does not open the broker. Remote clients are rejected by the pipe itself.
//!
//! The command line and config file are the ones `install_relay_broker.ps1` writes:
//! `hams-relay-broker.exe --config C:\ProgramData\hams-relay-broker\broker.conf`, keys `allow_sid`
//! (repeatable), `set_time`, `idle_exit_secs`, `max_unproven_step_secs`, `max_time_steps_per_hour`
//! and the Linux keys `apply_groups` and `update_package` (accepted, ignored).

// The Win32 calls below are the only unsafe code in the crate on Windows: each block says what it
// hands the OS and why that is sound.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE, LUID,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, RevertToSelf, TokenUser,
    LUID_AND_ATTRIBUTES, SECURITY_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, ImpersonateNamedPipeClient,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerW, SetServiceStatus, StartServiceCtrlDispatcherW,
    SERVICE_ACCEPT_STOP, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_STATUS,
    SERVICE_STATUS_HANDLE, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
};
use windows_sys::Win32::System::SystemInformation::SetSystemTime;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

use crate::time_guard::{TimeGuard, BUILD_DATE_UNIX_SECS};
use crate::wire::{frame_body_len, frame_encode, Request, Response, Status};

/// The pipe the relay connects to.
pub const PIPE_NAME: &str = r"\\.\pipe\hams-relay-broker";
/// Where `install_relay_broker.ps1` puts the config.
pub const DEFAULT_CONFIG: &str = r"C:\ProgramData\hams-relay-broker\broker.conf";

/// What the config file says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// User SIDs (`S-1-5-21-...`) allowed to connect. Empty: nobody (besides the pipe's own SYSTEM and Administrators entries, which the SID check still refuses).
    pub allow_sids: Vec<String>,
    /// Whether `set_system_time` is served (default: refused).
    pub set_time: bool,
    /// The largest unproven clock step in seconds.
    pub max_unproven_step_secs: u64,
    /// Clock steps allowed per hour before the breaker opens.
    pub max_time_steps_per_hour: u32,
    /// Exit after this many idle seconds (0: never).
    pub idle_exit_secs: u64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            allow_sids: Vec::new(),
            set_time: false,
            max_unproven_step_secs: 30,
            max_time_steps_per_hour: 4,
            idle_exit_secs: 300,
        }
    }
}

// [@ANCHOR: relay_broker_windows_valid_sid]
/// True for a string that can only be a SID: `S-` then digits and dashes. Anything else is never put into a security descriptor.
pub fn valid_sid(sid: &str) -> bool {
    sid.len() <= 184
        && sid.starts_with("S-1-")
        && sid.len() > 4
        && sid.bytes().skip(1).all(|b| b.is_ascii_digit() || b == b'-')
        && !sid.ends_with('-')
        && !sid.contains("--")
}

// [@ANCHOR: relay_broker_windows_parse_settings]
/// Parses the config text: `key = value` lines, `#` comments, unknown keys are an error.
pub fn parse_settings(text: &str) -> Result<Settings, String> {
    let mut s = Settings::default();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |why: &str| format!("line {}: {why}", n + 1);
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| at("expected key = value"))?;
        let (key, value) = (key.trim(), value.trim());
        let flag = |v: &str| match v {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(at("expected true or false")),
        };
        match key {
            "allow_sid" => {
                if !valid_sid(value) {
                    return Err(at("not a SID"));
                }
                s.allow_sids.push(value.to_string());
            }
            "set_time" => s.set_time = flag(value)?,
            "apply_groups" | "update_package" => {
                flag(value)?;
            }
            "idle_exit_secs" => s.idle_exit_secs = value.parse().map_err(|_| at("bad number"))?,
            "max_unproven_step_secs" => {
                s.max_unproven_step_secs = value.parse().map_err(|_| at("bad number"))?
            }
            "max_time_steps_per_hour" => {
                s.max_time_steps_per_hour = value.parse().map_err(|_| at("bad number"))?
            }
            other => return Err(at(&format!("unknown key {other}"))),
        }
    }
    Ok(s)
}

// [@ANCHOR: relay_broker_windows_pipe_sddl]
/// The pipe's DACL: SYSTEM and Administrators full control, each allowed SID read and write. Protected, so nothing is inherited.
pub fn pipe_sddl(allow_sids: &[String]) -> Result<String, String> {
    let mut sddl = String::from("D:P(A;;GA;;;SY)(A;;GA;;;BA)");
    for sid in allow_sids {
        if !valid_sid(sid) {
            return Err(format!("not a SID: {sid}"));
        }
        sddl.push_str(&format!("(A;;GRGW;;;{sid})"));
    }
    Ok(sddl)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// [@ANCHOR: relay_broker_windows_civil_from_unix]
/// `(year, month, day, hour, minute, second)` of a Unix time in UTC (Howard Hinnant's civil-from-days).
pub fn civil_from_unix(secs: u64) -> (u16, u16, u16, u16, u16, u16) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u16;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u16;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as u16;
    (
        y,
        m,
        d,
        (rem / 3_600) as u16,
        (rem % 3_600 / 60) as u16,
        (rem % 60) as u16,
    )
}

// [@ANCHOR: relay_broker_windows_system_time_unix_secs]
/// The system clock now, Unix seconds.
pub fn system_time_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// [@ANCHOR: relay_broker_windows_set_system_time]
/// Steps the clock to `unix_secs` (UTC) with `SetSystemTime`, after enabling `SeSystemtimePrivilege` in this process's token (a
/// service holds it disabled).
pub fn set_system_time(unix_secs: u64) -> io::Result<()> {
    enable_privilege("SeSystemtimePrivilege")?;
    let (year, month, day, hour, minute, second) = civil_from_unix(unix_secs);
    let st = windows_sys::Win32::Foundation::SYSTEMTIME {
        wYear: year,
        wMonth: month,
        wDayOfWeek: 0,
        wDay: day,
        wHour: hour,
        wMinute: minute,
        wSecond: second,
        wMilliseconds: 0,
    };
    // SAFETY: `st` is a valid SYSTEMTIME that lives for the call; the call only reads it.
    if unsafe { SetSystemTime(&st) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Enables one named privilege in the current process token.
fn enable_privilege(name: &str) -> io::Result<()> {
    let mut token: HANDLE = null_mut();
    // SAFETY: GetCurrentProcess is a pseudo handle; `token` receives a handle we close below.
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let name = wide(name);
    let mut luid = LUID {
        LowPart: 0,
        HighPart: 0,
    };
    // SAFETY: `name` is NUL-terminated and outlives the call; `luid` is a valid out parameter.
    let found = unsafe { LookupPrivilegeValueW(null(), name.as_ptr(), &mut luid) } != 0;
    let mut result = if found {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    };
    if found {
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // SAFETY: `token` is a valid token handle opened with TOKEN_ADJUST_PRIVILEGES; `privileges` is one entry and lives for the call.
        let adjusted =
            unsafe { AdjustTokenPrivileges(token, 0, &privileges, 0, null_mut(), null_mut()) };
        // AdjustTokenPrivileges succeeds even when it assigned nothing; ERROR_NOT_ALL_ASSIGNED (1300) says so.
        // SAFETY: reads the calling thread's last-error value.
        if adjusted == 0 || unsafe { GetLastError() } == 1300 {
            result = Err(io::Error::other(
                "the service token does not hold the privilege",
            ));
        }
    }
    // SAFETY: `token` came from OpenProcessToken and is closed once.
    unsafe { CloseHandle(token) };
    result
}

/// The user SID, as `S-1-5-21-...`, of the token in `token`.
fn token_user_sid(token: HANDLE) -> io::Result<String> {
    let mut needed = 0u32;
    // SAFETY: the first call only asks how large the buffer must be.
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // 8-byte aligned storage for the TOKEN_USER the call writes.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buffer` is at least `needed` bytes and suitably aligned for TOKEN_USER.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast::<c_void>(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = buffer.as_ptr().cast::<TOKEN_USER>();
    let mut text: *mut u16 = null_mut();
    // SAFETY: GetTokenInformation filled a TOKEN_USER at the start of `buffer`; its SID pointer points inside `buffer`.
    if unsafe { ConvertSidToStringSidW((*user).User.Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `text` is a NUL-terminated UTF-16 string allocated by the call; copied, then freed with LocalFree as documented.
    let sid = unsafe {
        let mut len = 0;
        while *text.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast::<c_void>());
        s
    };
    Ok(sid)
}

// [@ANCHOR: relay_broker_windows_pipe_client_sid]
/// The user SID of the client of a connected pipe instance. Impersonates the client for the one query and reverts; the caller must
/// have read from the pipe first (Windows refuses to impersonate a client that has sent nothing).
fn pipe_client_sid(pipe: HANDLE) -> io::Result<String> {
    // SAFETY: `pipe` is a connected server-side pipe handle.
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut token: HANDLE = null_mut();
    // SAFETY: the thread is impersonating; `token` receives a handle closed below.
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } != 0;
    let open_error = io::Error::last_os_error();
    // SAFETY: ends the impersonation started above, always, before anything else runs on this thread.
    unsafe { RevertToSelf() };
    if !opened {
        return Err(open_error);
    }
    let sid = token_user_sid(token);
    // SAFETY: `token` was opened above and is closed once.
    unsafe { CloseHandle(token) };
    sid
}

/// The clock the handler reads and steps; the real one is the system's, the tests' is a variable.
pub trait Clock {
    /// Now, Unix seconds.
    fn now_unix_secs(&self) -> u64;
    /// Step the clock.
    fn set_unix_secs(&mut self, secs: u64) -> io::Result<()>;
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_secs(&self) -> u64 {
        system_time_unix_secs()
    }
    fn set_unix_secs(&mut self, secs: u64) -> io::Result<()> {
        set_system_time(secs)
    }
}

/// Everything the handler keeps between requests.
#[derive(Debug)]
pub struct State {
    settings: Settings,
    guard: TimeGuard,
}

impl State {
    /// State for `settings`.
    pub fn new(settings: Settings) -> State {
        let guard = TimeGuard::new(
            settings.max_unproven_step_secs,
            settings.max_time_steps_per_hour,
        );
        State { settings, guard }
    }
}

// [@ANCHOR: relay_broker_windows_handle_request]
/// Answers one request from a client whose SID is `sid`. A SID not on the list gets `Denied` and nothing else is looked at; the
/// device, group, hub and package requests are `Denied` on Windows; the two clock requests behave as on Linux.
pub fn handle_request(
    state: &mut State,
    clock: &mut dyn Clock,
    sid: &str,
    request: Request,
    now: Instant,
) -> Response {
    if !state.settings.allow_sids.iter().any(|s| s == sid) {
        return Response::error(Status::Denied, "caller is not on the allow list");
    }
    match request {
        Request::TimeStatus => {
            let (steps_last_hour, breaker_open) = state.guard.status(now);
            Response::TimeStatus {
                now_unix_secs: clock.now_unix_secs(),
                steps_last_hour,
                breaker_open,
                min_target_unix_secs: BUILD_DATE_UNIX_SECS,
            }
        }
        Request::SetSystemTime {
            target_unix_secs,
            source,
            agreeing,
        } => {
            if !state.settings.set_time {
                return Response::error(Status::Denied, "clock steps not enabled");
            }
            let before = clock.now_unix_secs();
            if let Err(denial) = state
                .guard
                .check(now, before, target_unix_secs, source, agreeing)
            {
                return Response::error(Status::Denied, denial.reason());
            }
            match clock.set_unix_secs(target_unix_secs) {
                Ok(()) => {
                    state.guard.record_step(now);
                    Response::TimeSet {
                        before_unix_secs: before,
                        after_unix_secs: clock.now_unix_secs(),
                    }
                }
                Err(e) => Response::error(Status::Failed, format!("set time: {e}")),
            }
        }
        _ => Response::error(Status::Denied, "not available on Windows"),
    }
}

/// One server-side pipe instance, closed on drop.
struct Instance(HANDLE);

impl Instance {
    /// Creates an instance of `name` with the DACL in `sddl`.
    fn create(name: &str, sddl: &str, first: bool) -> io::Result<Instance> {
        let sddl = wide(sddl);
        let mut descriptor: *mut c_void = null_mut();
        // SAFETY: `sddl` is NUL-terminated and outlives the call; `descriptor` receives a LocalAlloc'd descriptor freed below.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let name = wide(name);
        let open_mode = PIPE_ACCESS_DUPLEX
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: `name` is NUL-terminated; `attributes` and its descriptor live until after the call.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                255,
                8192,
                8192,
                0,
                &attributes,
            )
        };
        let created = io::Error::last_os_error();
        // SAFETY: the descriptor was allocated by ConvertStringSecurityDescriptor...; the pipe keeps its own copy.
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(created);
        }
        Ok(Instance(handle))
    }

    /// Waits for one client.
    fn connect(&self) -> io::Result<()> {
        // SAFETY: a valid pipe handle, no overlapped I/O.
        if unsafe { ConnectNamedPipe(self.0, null_mut()) } == 0 {
            // A client that connected between create and connect is not an error.
            // SAFETY: reads the last-error value.
            if unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// The handle as a `File` (borrowed: `ManuallyDrop` keeps `Drop` of `Instance` the only closer).
    fn file(&self) -> std::mem::ManuallyDrop<File> {
        // SAFETY: the handle is valid and stays owned by `self`; the File is never dropped.
        std::mem::ManuallyDrop::new(unsafe { File::from_raw_handle(self.0 as RawHandle) })
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this Instance and closed once; the client is disconnected first.
        unsafe {
            DisconnectNamedPipe(self.0);
            CloseHandle(self.0);
        }
    }
}

// [@ANCHOR: relay_broker_windows_serve_one]
/// Serves one connection on a new pipe instance: header, SID check, body, one request, one answer.
pub fn serve_one(
    name: &str,
    sddl: &str,
    first: bool,
    state: &mut State,
    clock: &mut dyn Clock,
) -> io::Result<String> {
    let instance = Instance::create(name, sddl, first)?;
    instance.connect()?;
    let mut file = instance.file();
    let mut header = [0u8; 4];
    file.read_exact(&mut header)?;
    // The client has sent data, so it can now be identified, and nothing past the length is parsed before that.
    let sid = pipe_client_sid(instance.0)?;
    let response = if !state.settings.allow_sids.contains(&sid) {
        Response::error(Status::Denied, "caller is not on the allow list")
    } else {
        match frame_body_len(header) {
            Err(_) => Response::error(Status::Malformed, "bad frame length"),
            Ok(len) => {
                let mut body = vec![0u8; len];
                file.read_exact(&mut body)?;
                match Request::decode(&body) {
                    Err(e) => Response::error(Status::Malformed, format!("{e:?}")),
                    Ok(request) => handle_request(state, clock, &sid, request, Instant::now()),
                }
            }
        }
    };
    file.write_all(&frame_encode(&response.encode()))?;
    // FlushFileBuffers on a pipe waits until the client has read everything; disconnecting first would discard the answer.
    file.sync_all()?;
    Ok(sid)
}

/// Opens `name`, sends one framed request and returns the decoded response (the client side, used by the tests and by `--probe`).
pub fn call(name: &str, request: &Request) -> io::Result<Response> {
    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name)?;
    pipe.write_all(&frame_encode(&request.encode()))?;
    let mut header = [0u8; 4];
    pipe.read_exact(&mut header)?;
    let len = frame_body_len(header).map_err(|e| io::Error::other(format!("{e:?}")))?;
    let mut body = vec![0u8; len];
    pipe.read_exact(&mut body)?;
    Response::decode(&body).map_err(|e| io::Error::other(format!("{e:?}")))
}

struct Args {
    config: PathBuf,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        config: PathBuf::from(DEFAULT_CONFIG),
    };
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => args.config = PathBuf::from(it.next().ok_or("--config needs a value")?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(args)
}

/// Serves connections one after another until the process is stopped. `idle_exit_secs` is read for config compatibility but a
/// blocking pipe accept cannot time out, so the process stays up until the service control manager stops it.
fn serve_forever(path: &Path) -> io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let settings = parse_settings(&text).map_err(io::Error::other)?;
    let sddl = pipe_sddl(&settings.allow_sids).map_err(io::Error::other)?;
    let mut state = State::new(settings);
    let mut clock = SystemClock;
    let mut first = true;
    loop {
        match serve_one(PIPE_NAME, &sddl, first, &mut state, &mut clock) {
            Ok(sid) => eprintln!("served {sid}"),
            Err(e) if first => return Err(e),
            Err(e) => {
                eprintln!("connection failed: {e}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        first = false;
    }
}

/// The service name `install_relay_broker.ps1` creates.
const SERVICE_NAME: &str = "HamsRelayBroker";
/// `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT`: the process was not started by the service control manager.
const NOT_A_SERVICE: u32 = 1063;
/// The config path, for `service_main`, which the control manager calls with no way to pass it.
static CONFIG: OnceLock<PathBuf> = OnceLock::new();
/// The handle `SetServiceStatus` needs, set once by `service_main`.
static STATUS_HANDLE: OnceLock<usize> = OnceLock::new();

/// Reports `state` to the service control manager.
fn report(state: u32, accept: u32) {
    let Some(handle) = STATUS_HANDLE.get() else {
        return;
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accept,
        dwWin32ExitCode: 0,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: 0,
    };
    // SAFETY: the handle came from RegisterServiceCtrlHandlerW and `status` lives for the call.
    unsafe { SetServiceStatus(*handle as SERVICE_STATUS_HANDLE, &status) };
}

/// The control handler: a stop request ends the process (the pipe accept cannot be interrupted, and the broker keeps no state to save).
unsafe extern "system" fn control_handler(control: u32) {
    if control == SERVICE_CONTROL_STOP {
        report(SERVICE_STOPPED, 0);
        std::process::exit(0);
    }
}

/// The service entry the control manager calls on its own thread.
unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
    let name = wide(SERVICE_NAME);
    // SAFETY: `name` is NUL-terminated and outlives the call; the handler is a valid function for the life of the process.
    let handle = unsafe { RegisterServiceCtrlHandlerW(name.as_ptr(), Some(control_handler)) };
    if handle.is_null() {
        return;
    }
    let _ = STATUS_HANDLE.set(handle as usize);
    report(SERVICE_RUNNING, SERVICE_ACCEPT_STOP);
    let path = CONFIG
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    if let Err(e) = serve_forever(&path) {
        eprintln!("hams-relay-broker: {e}");
    }
    report(SERVICE_STOPPED, 0);
}

/// Hands this process to the service control manager. Returns the Win32 error when it was not started as a service.
fn run_as_service() -> u32 {
    let mut name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: null_mut(),
            lpServiceProc: None,
        },
    ];
    // SAFETY: `table` is a NULL-terminated array that lives for the whole (blocking) call.
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        // SAFETY: reads the last-error value.
        return unsafe { GetLastError() };
    }
    0
}

/// The program: parse the command line, then run as the `HamsRelayBroker` service, or in the console when it was started by hand.
pub fn run_main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hams-relay-broker: {e}");
            return ExitCode::from(2);
        }
    };
    let _ = CONFIG.set(args.config.clone());
    match run_as_service() {
        0 => return ExitCode::SUCCESS,
        NOT_A_SERVICE => {}
        other => {
            eprintln!("hams-relay-broker: service dispatcher failed ({other})");
            return ExitCode::from(1);
        }
    }
    match serve_forever(&args.config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hams-relay-broker: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::TimeSource;

    // Tests [@ANCHOR: relay_broker_windows_valid_sid]
    #[test]
    fn only_a_plain_sid_is_accepted() {
        assert!(valid_sid("S-1-5-21-1-2-3-1001"));
        assert!(valid_sid("S-1-5-18"));
        for bad in [
            "",
            "S-1-",
            "S-1-5-",
            "S-2-5-18",
            "S-1-5--18",
            "S-1-5-18)(A;;GA;;;WD",
            "S-1-5-1a",
            " S-1-5-18",
        ] {
            assert!(!valid_sid(bad), "{bad:?}");
        }
    }

    // Tests [@ANCHOR: relay_broker_windows_parse_settings]
    #[test]
    fn the_installer_written_config_parses_and_junk_is_refused() {
        let s = parse_settings(
            "# comment\napply_groups = false\nupdate_package = false\nset_time = true\nidle_exit_secs = 300\nallow_sid = S-1-5-21-1-2-3-1001\nallow_sid = S-1-5-32-544\n",
        )
        .expect("installer config");
        assert!(s.set_time);
        assert_eq!(s.allow_sids.len(), 2);
        assert_eq!(s.idle_exit_secs, 300);
        assert_eq!(parse_settings(""), Ok(Settings::default()));
        for bad in [
            "allow_sid = bob",
            "set_time = yes",
            "bogus = 1",
            "set_time",
            "idle_exit_secs = x",
        ] {
            assert!(parse_settings(bad).is_err(), "{bad}");
        }
    }

    // Tests [@ANCHOR: relay_broker_windows_pipe_sddl]
    #[test]
    fn the_acl_is_system_admins_and_each_listed_sid_only() {
        assert_eq!(pipe_sddl(&[]).as_deref(), Ok("D:P(A;;GA;;;SY)(A;;GA;;;BA)"));
        assert_eq!(
            pipe_sddl(&["S-1-5-21-1-2-3-1001".to_string()]).as_deref(),
            Ok("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;S-1-5-21-1-2-3-1001)")
        );
        assert!(pipe_sddl(&["S-1-5-18)(A;;GA;;;WD".to_string()]).is_err());
    }

    // Tests [@ANCHOR: relay_broker_windows_civil_from_unix]
    #[test]
    fn unix_seconds_become_the_right_utc_date() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_unix(951_782_400 + 3_661), (2000, 2, 29, 1, 1, 1));
        assert_eq!(
            civil_from_unix(BUILD_DATE_UNIX_SECS),
            (2026, 11, 1, 0, 0, 0)
        );
        assert_eq!(civil_from_unix(1_798_761_599), (2026, 12, 31, 23, 59, 59));
    }

    struct FakeClock {
        now: u64,
        fail: bool,
    }

    impl Clock for FakeClock {
        fn now_unix_secs(&self) -> u64 {
            self.now
        }
        fn set_unix_secs(&mut self, secs: u64) -> io::Result<()> {
            if self.fail {
                return Err(io::Error::other("no privilege"));
            }
            self.now = secs;
            Ok(())
        }
    }

    const SID: &str = "S-1-5-21-1-2-3-1001";

    fn state(set_time: bool) -> State {
        State::new(Settings {
            allow_sids: vec![SID.to_string()],
            set_time,
            ..Settings::default()
        })
    }

    fn step(target: u64) -> Request {
        Request::SetSystemTime {
            target_unix_secs: target,
            source: TimeSource::Gps,
            agreeing: 1,
        }
    }

    // Tests [@ANCHOR: relay_broker_windows_handle_request]
    #[test]
    fn an_unlisted_sid_gets_nothing_and_the_clock_rules_match_linux() {
        let base = BUILD_DATE_UNIX_SECS + 1_000;
        let mut clock = FakeClock {
            now: base,
            fail: false,
        };
        let now = Instant::now();
        let denied = |r: Response| {
            matches!(
                r,
                Response::Error {
                    status: Status::Denied,
                    ..
                }
            )
        };
        let mut st = state(true);
        assert!(denied(handle_request(
            &mut st,
            &mut clock,
            "S-1-5-21-9",
            Request::TimeStatus,
            now
        )));
        assert!(denied(handle_request(
            &mut st,
            &mut clock,
            "S-1-5-21-9",
            step(base + 5),
            now
        )));
        assert_eq!(clock.now, base);
        // Linux-only requests are refused for a listed caller.
        assert!(denied(handle_request(
            &mut st,
            &mut clock,
            SID,
            Request::HubStop,
            now
        )));
        assert!(matches!(
            handle_request(&mut st, &mut clock, SID, Request::TimeStatus, now),
            Response::TimeStatus { now_unix_secs, steps_last_hour: 0, breaker_open: false, min_target_unix_secs: BUILD_DATE_UNIX_SECS } if now_unix_secs == base
        ));
        // A step before the build date is refused; a good step is made and counted.
        assert!(denied(handle_request(
            &mut st,
            &mut clock,
            SID,
            step(BUILD_DATE_UNIX_SECS - 1),
            now
        )));
        assert_eq!(
            handle_request(&mut st, &mut clock, SID, step(base + 5), now),
            Response::TimeSet {
                before_unix_secs: base,
                after_unix_secs: base + 5
            }
        );
        assert!(matches!(
            handle_request(&mut st, &mut clock, SID, Request::TimeStatus, now),
            Response::TimeStatus {
                steps_last_hour: 1,
                ..
            }
        ));
        // Clock steps off in the config, and an OS failure is reported as Failed.
        let mut off = state(false);
        assert!(denied(handle_request(
            &mut off,
            &mut clock,
            SID,
            step(base + 9),
            now
        )));
        let mut broken = FakeClock {
            now: base,
            fail: true,
        };
        assert!(matches!(
            handle_request(&mut state(true), &mut broken, SID, step(base + 9), now),
            Response::Error {
                status: Status::Failed,
                ..
            }
        ));
    }

    fn own_sid() -> String {
        let mut token: HANDLE = null_mut();
        // SAFETY: the pseudo handle of this process; `token` is closed below.
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) },
            0
        );
        let sid = token_user_sid(token).expect("own sid");
        // SAFETY: opened above, closed once.
        unsafe { CloseHandle(token) };
        sid
    }

    fn unique_pipe(tag: &str) -> String {
        format!(
            r"\\.\pipe\hams-relay-broker-test-{tag}-{}",
            std::process::id()
        )
    }

    /// Runs one `serve_one` on a thread and calls it once from here.
    fn round_trip(
        tag: &str,
        sddl: &str,
        allow: Vec<String>,
        set_time: bool,
        request: Request,
    ) -> (io::Result<Response>, Option<String>) {
        let name = unique_pipe(tag);
        let server_name = name.clone();
        let sddl = sddl.to_string();
        let server = std::thread::spawn(move || {
            let mut st = State::new(Settings {
                allow_sids: allow,
                set_time,
                ..Settings::default()
            });
            let mut clock = FakeClock {
                now: BUILD_DATE_UNIX_SECS + 1_000,
                fail: false,
            };
            serve_one(&server_name, &sddl, true, &mut st, &mut clock)
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let answer = loop {
            match call(&name, &request) {
                Err(e) if e.kind() == io::ErrorKind::NotFound && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                other => break other,
            }
        };
        // A client the ACL refused never reaches the server thread, which is then left waiting; do not join it.
        let served = if answer.is_ok() {
            server.join().ok().and_then(Result::ok)
        } else {
            None
        };
        (answer, served)
    }

    // Tests [@ANCHOR: relay_broker_windows_serve_one]
    // Tests [@ANCHOR: relay_broker_windows_pipe_client_sid]
    // Tests [@ANCHOR: relay_broker_windows_system_time_unix_secs]
    #[test]
    fn a_listed_user_is_identified_over_the_real_pipe_and_served() {
        let me = own_sid();
        let sddl = pipe_sddl(std::slice::from_ref(&me)).expect("sddl");
        let (answer, served) = round_trip("ok", &sddl, vec![me.clone()], true, Request::TimeStatus);
        assert!(
            matches!(answer, Ok(Response::TimeStatus { now_unix_secs, .. }) if now_unix_secs == BUILD_DATE_UNIX_SECS + 1_000),
            "{answer:?}"
        );
        assert_eq!(served, Some(me));
        assert!(system_time_unix_secs() >= BUILD_DATE_UNIX_SECS);
    }

    #[test]
    fn an_account_the_acl_admits_but_the_sid_list_does_not_is_refused_by_the_sid_check() {
        let me = own_sid();
        // The administrators entry lets an elevated test user open the pipe, so only the SID check can refuse.
        let sddl = pipe_sddl(&[me]).expect("sddl");
        let (answer, _) = round_trip(
            "sid",
            &sddl,
            vec!["S-1-5-21-9-9-9-1".to_string()],
            true,
            Request::TimeStatus,
        );
        assert!(
            matches!(
                answer,
                Ok(Response::Error {
                    status: Status::Denied,
                    ..
                })
            ),
            "{answer:?}"
        );
    }

    #[test]
    fn an_account_outside_the_acl_cannot_open_the_pipe() {
        let me = own_sid();
        let (answer, _) = round_trip(
            "acl",
            "D:P(A;;GA;;;SY)",
            vec![me],
            true,
            Request::TimeStatus,
        );
        let e = answer.expect_err("the ACL must refuse this account");
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied, "{e}");
    }

    // Tests [@ANCHOR: relay_broker_windows_set_system_time]
    #[test]
    fn the_real_clock_can_be_set_to_the_time_it_already_has() {
        // Needs an elevated account (SeSystemtimePrivilege); stepping to "now" changes nothing.
        let now = system_time_unix_secs();
        set_system_time(now).expect("SetSystemTime (run this test elevated)");
        let after = system_time_unix_secs();
        assert!(after.abs_diff(now) <= 3, "{now} -> {after}");
    }
}
