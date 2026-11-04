//! The official EPOS Gaming Suite next to eposctl: its background service and
//! its app. eposctl needs neither. The driver creates its shared memory itself,
//! with the same access rights the service would give it (both binaries use the
//! descriptor `D:(A;OICI;FA;;;BU)(A;OICI;FA;;;LS)`; see docs/protocol.md). Both
//! can get in the way: the service writes its own settings to the driver and
//! has made other programs crash, the app stalls on its dead cloud server. So
//! they can be switched off and back on. The driver is never touched.
//!
//! Switching needs administrator rights (service configuration, and the
//! all-users startup entry). Reading the state does not.

use std::io;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SERVICE_NOT_ACTIVE, HANDLE};
use windows_sys::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QUERY_SERVICE_CONFIGW,
    QueryServiceConfigW, QueryServiceStatus, SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
    SERVICE_CONTROL_STOP, SERVICE_DISABLED, SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS,
    SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS, SERVICE_STOP, StartServiceW,
};

use crate::win::{Hive, reg_binary, set_reg_binary, wide};

/// The background service the Gaming Suite installs.
pub const SERVICE: &str = "EPOSGamingSuiteService";
/// The Gaming Suite app.
pub const APP_EXE: &str = "EPOSGamingSuite.exe";
/// The app's entry in the all-users Startup folder.
const STARTUP_LINK: &str = "EPOS Gaming Suite.lnk";
/// Where Windows (Task Manager's Startup apps) records that a Startup-folder
/// entry is switched off, without deleting it.
const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";

/// What of the Gaming Suite is installed and active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The service exists.
    pub installed: bool,
    pub service_running: bool,
    /// The service is set to start with Windows (not disabled).
    pub service_autostart: bool,
    pub app_running: bool,
    /// The app's Startup-folder entry exists and is not switched off.
    pub app_autostart: bool,
}

impl Status {
    /// Anything of it runs now or would start with Windows.
    pub fn active(&self) -> bool {
        self.installed && (self.service_running || self.service_autostart || self.app_running || self.app_autostart)
    }
}

/// Read the current state. Needs no special rights.
pub fn status() -> Status {
    let svc = Service::open(SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG).ok();
    Status {
        installed: svc.is_some(),
        service_running: svc.as_ref().is_some_and(Service::running),
        service_autostart: svc.as_ref().and_then(Service::start_type).is_some_and(|t| t != SERVICE_DISABLED),
        app_running: !app_pids().is_empty(),
        app_autostart: startup_link().is_file() && !startup_switched_off(),
    }
}

/// Switch the service and the app's startup entry off (also quitting the app
/// and stopping the service) or back on. Needs administrator rights.
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let svc = Service::open(SERVICE_QUERY_STATUS | SERVICE_CHANGE_CONFIG | SERVICE_START | SERVICE_STOP)?;
    if enabled {
        svc.set_start_type(SERVICE_AUTO_START)?;
        svc.start()?;
    } else {
        svc.set_start_type(SERVICE_DISABLED)?;
        svc.stop()?;
        quit_app();
    }
    if startup_link().is_file() {
        // 02 = on, 03 = off, then the FILETIME of the change (left at zero, as
        // Windows accepts).
        let mut v = [0u8; 12];
        v[0] = if enabled { 2 } else { 3 };
        set_reg_binary(Hive::LocalMachine, APPROVED_KEY, STARTUP_LINK, &v)?;
    }
    Ok(())
}

/// End every running Gaming Suite app process (no special rights needed for
/// the user's own). Returns how many were ended.
pub fn quit_app() -> usize {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    let mut n = 0;
    for pid in app_pids() {
        // SAFETY: plain Win32 calls; the handle is checked and closed.
        unsafe {
            let h: HANDLE = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !h.is_null() {
                if TerminateProcess(h, 0) != 0 {
                    n += 1;
                }
                CloseHandle(h);
            }
        }
    }
    n
}

fn startup_link() -> PathBuf {
    let data = std::env::var_os("ProgramData").map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
    data.join(r"Microsoft\Windows\Start Menu\Programs\StartUp").join(STARTUP_LINK)
}

/// Task Manager's "disabled" flag: an odd first byte.
fn startup_switched_off() -> bool {
    reg_binary(Hive::LocalMachine, APPROVED_KEY, STARTUP_LINK).is_some_and(|v| v.first().is_some_and(|b| b & 1 == 1))
}

/// Process ids of running `EPOSGamingSuite.exe`.
fn app_pids() -> Vec<u32> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    let mut pids = Vec::new();
    // SAFETY: the snapshot handle is checked and closed; `entry` is a valid,
    // sized PROCESSENTRY32W for every call.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return pids;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(APP_EXE) {
                pids.push(entry.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    pids
}

/// An open handle to the Gaming Suite service (and its service manager).
struct Service {
    scm: SC_HANDLE,
    h: SC_HANDLE,
}

impl Service {
    fn open(access: u32) -> io::Result<Self> {
        let name = wide(SERVICE);
        // SAFETY: `name` is NUL-terminated and outlives the calls; handles are
        // checked here and closed in Drop.
        unsafe {
            let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
            if scm.is_null() {
                return Err(io::Error::last_os_error());
            }
            let h = OpenServiceW(scm, name.as_ptr(), access);
            if h.is_null() {
                let e = io::Error::last_os_error();
                CloseServiceHandle(scm);
                return Err(e);
            }
            Ok(Self { scm, h })
        }
    }

    fn running(&self) -> bool {
        // SAFETY: `self.h` is an open service handle; `st` is writable.
        unsafe {
            let mut st: SERVICE_STATUS = std::mem::zeroed();
            QueryServiceStatus(self.h, &mut st) != 0 && st.dwCurrentState == SERVICE_RUNNING
        }
    }

    fn start_type(&self) -> Option<u32> {
        let mut buf = vec![0u64; 1024]; // 8 KiB, aligned for QUERY_SERVICE_CONFIGW
        let mut needed = 0u32;
        // SAFETY: `buf` is writable for the size passed and aligned for the
        // struct the call writes at its start.
        unsafe {
            let cfg = buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
            let ok = QueryServiceConfigW(self.h, cfg, (buf.len() * 8) as u32, &mut needed) != 0;
            ok.then(|| (*cfg).dwStartType)
        }
    }

    fn set_start_type(&self, start: u32) -> io::Result<()> {
        let null = std::ptr::null();
        // SAFETY: `self.h` is open with SERVICE_CHANGE_CONFIG; null pointers
        // mean "leave unchanged".
        let ok = unsafe {
            ChangeServiceConfigW(
                self.h,
                SERVICE_NO_CHANGE,
                start,
                SERVICE_NO_CHANGE,
                null,
                null,
                std::ptr::null_mut(),
                null,
                null,
                null,
                null,
            )
        };
        if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    fn start(&self) -> io::Result<()> {
        if self.running() {
            return Ok(());
        }
        // SAFETY: `self.h` is open with SERVICE_START; no arguments.
        if unsafe { StartServiceW(self.h, 0, std::ptr::null()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn stop(&self) -> io::Result<()> {
        // SAFETY: `self.h` is open with SERVICE_STOP; `st` is writable.
        let ok = unsafe {
            let mut st: SERVICE_STATUS = std::mem::zeroed();
            ControlService(self.h, SERVICE_CONTROL_STOP, &mut st)
        };
        if ok == 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(ERROR_SERVICE_NOT_ACTIVE as i32) {
                return Err(e);
            }
        }
        Ok(())
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        // SAFETY: both handles were opened by `open` and are closed once.
        unsafe {
            CloseServiceHandle(self.h);
            CloseServiceHandle(self.scm);
        }
    }
}

/// This process runs with administrator rights (elevated token).
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: the token handle is checked and closed; `elev` is writable for
    // the size passed.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elev = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elev).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elev.TokenIsElevated != 0
    }
}

/// Run `exe args` with administrator rights (Windows asks the user), hidden,
/// and wait for it. Returns its exit code; `ErrorKind::PermissionDenied` if
/// the user said no.
pub fn run_elevated(exe: &Path, args: &str) -> io::Result<u32> {
    use windows_sys::Win32::Foundation::{ERROR_CANCELLED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let (verb, file, params) = (wide("runas"), wide(&exe.to_string_lossy()), wide(args));
    // SAFETY: the strings are NUL-terminated and outlive the call; `info` is
    // a zeroed, sized SHELLEXECUTEINFOW; the process handle is closed.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(ERROR_CANCELLED as i32) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "administrator rights were declined"));
            }
            return Err(e);
        }
        let mut code = 1u32;
        let waited = WaitForSingleObject(info.hProcess, INFINITE) == WAIT_OBJECT_0;
        if !waited || GetExitCodeProcess(info.hProcess, &mut code) == 0 {
            code = 1;
        }
        CloseHandle(info.hProcess);
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_needs_something_running_or_starting() {
        let mut s = Status { installed: true, ..Default::default() };
        assert!(!s.active());
        s.app_autostart = true;
        assert!(s.active());
        assert!(!Status { installed: false, service_running: true, ..Default::default() }.active());
    }

    #[test]
    fn startup_link_is_in_the_all_users_startup_folder() {
        let p = startup_link();
        assert!(p.ends_with(r"Start Menu\Programs\StartUp\EPOS Gaming Suite.lnk"), "{}", p.display());
    }
}
