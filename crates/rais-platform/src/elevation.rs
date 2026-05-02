//! Elevated process launcher (Windows).
//!
//! Some vendor installers — notably the JAWS-for-REAPER scripts NSIS package
//! — declare `RequestExecutionLevel admin` in their script because they need
//! to write into `C:\Program Files (x86)\…`. A normal `CreateProcess` call
//! from an unelevated parent never triggers UAC, so on default Windows the
//! installer silently no-ops in `/S` silent mode. We work around that by
//! launching the installer through `ShellExecuteExW` with the `runas` verb,
//! which always raises the UAC consent dialog when the user is not already
//! elevated, then waiting on the returned process handle for the exit code.
//!
//! Non-Windows builds compile to a stub that returns an `Unsupported` error.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ElevationError {
    /// `ShellExecuteExW` failed before producing a process handle. `code` is
    /// the OS-reported `GetLastError()` value.
    LaunchFailed { program: PathBuf, code: u32 },
    /// `WaitForSingleObject` or `GetExitCodeProcess` failed, or the process
    /// terminated abnormally so we have no exit code to report.
    WaitFailed { program: PathBuf, message: String },
    /// The user dismissed the UAC consent prompt. Distinct from a generic
    /// launch failure so the caller can surface a clearer message.
    UserCancelledElevation { program: PathBuf },
    /// Compiled on a target that has no elevation primitive (only Windows
    /// supports this code path today).
    Unsupported,
}

impl std::fmt::Display for ElevationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LaunchFailed { program, code } => write!(
                f,
                "ShellExecuteExW(runas) failed for {} (Win32 error {code})",
                program.display()
            ),
            Self::WaitFailed { program, message } => write!(
                f,
                "could not read exit status for elevated process {}: {message}",
                program.display()
            ),
            Self::UserCancelledElevation { program } => write!(
                f,
                "the Windows administrator approval prompt for {} was cancelled or declined",
                program.display()
            ),
            Self::Unsupported => write!(
                f,
                "elevated process launch is not supported on this platform"
            ),
        }
    }
}

impl std::error::Error for ElevationError {}

/// Launch `program` with `arguments` under UAC elevation and block until it
/// exits. Returns the process exit code (`Some(n)`) on a clean exit, or
/// `None` if the OS could not return one (rare). Working directory may be
/// `None` to inherit the current directory.
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn run_elevated_and_wait(
    program: &Path,
    arguments: &[String],
    working_directory: Option<&Path>,
) -> Result<Option<i32>, ElevationError> {
    platform_run_elevated_and_wait(program, arguments, working_directory)
}

#[cfg(windows)]
fn platform_run_elevated_and_wait(
    program: &Path,
    arguments: &[String],
    working_directory: Option<&Path>,
) -> Result<Option<i32>, ElevationError> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, GetLastError, WAIT_FAILED};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb_w: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
    let program_w: Vec<u16> = program.as_os_str().encode_wide().chain(Some(0)).collect();
    let parameters = quote_arguments(arguments);
    let parameters_w: Vec<u16> = OsStr::new(&parameters)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let working_directory_w: Option<Vec<u16>> =
        working_directory.map(|path| path.as_os_str().encode_wide().chain(Some(0)).collect());

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        lpVerb: verb_w.as_ptr(),
        lpFile: program_w.as_ptr(),
        lpParameters: if parameters.is_empty() {
            std::ptr::null()
        } else {
            parameters_w.as_ptr()
        },
        lpDirectory: working_directory_w
            .as_ref()
            .map(|w| w.as_ptr())
            .unwrap_or(std::ptr::null()),
        nShow: SW_SHOWNORMAL,
        hInstApp: std::ptr::null_mut(),
        lpIDList: std::ptr::null_mut(),
        lpClass: std::ptr::null(),
        hkeyClass: std::ptr::null_mut(),
        dwHotKey: 0,
        Anonymous: unsafe { std::mem::zeroed() },
        hProcess: std::ptr::null_mut(),
    };

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        // Windows returns ERROR_CANCELLED (1223) when the user dismisses the
        // UAC consent prompt; surface that as a distinct error so the wizard
        // can prompt them to re-run and approve.
        if code == ERROR_CANCELLED {
            return Err(ElevationError::UserCancelledElevation {
                program: program.to_path_buf(),
            });
        }
        return Err(ElevationError::LaunchFailed {
            program: program.to_path_buf(),
            code,
        });
    }

    if info.hProcess.is_null() {
        // Some shell verbs return success without a process handle (e.g.
        // when the file extension was handled by the shell instead of by
        // CreateProcess). For our use case — launching a real `.exe` — that
        // would be a misconfiguration; treat it as a wait failure so the
        // caller doesn't silently report success.
        return Err(ElevationError::WaitFailed {
            program: program.to_path_buf(),
            message: "ShellExecuteExW returned no process handle".to_string(),
        });
    }

    let wait_status = unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
    if wait_status == WAIT_FAILED {
        let code = unsafe { GetLastError() };
        unsafe {
            CloseHandle(info.hProcess);
        }
        return Err(ElevationError::WaitFailed {
            program: program.to_path_buf(),
            message: format!("WaitForSingleObject failed (Win32 error {code})"),
        });
    }

    let mut exit_code: u32 = 0;
    let ok = unsafe { GetExitCodeProcess(info.hProcess, &mut exit_code) };
    unsafe {
        CloseHandle(info.hProcess);
    }
    if ok == 0 {
        let code = unsafe { GetLastError() };
        return Err(ElevationError::WaitFailed {
            program: program.to_path_buf(),
            message: format!("GetExitCodeProcess failed (Win32 error {code})"),
        });
    }

    Ok(Some(exit_code as i32))
}

#[cfg(not(windows))]
fn platform_run_elevated_and_wait(
    _program: &Path,
    _arguments: &[String],
    _working_directory: Option<&Path>,
) -> Result<Option<i32>, ElevationError> {
    Err(ElevationError::Unsupported)
}

/// Quote each argument the way `ShellExecuteEx` expects (one space-joined
/// command-line string), wrapping arguments containing whitespace or quotes
/// in double-quotes and escaping internal quotes.
fn quote_arguments(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| quote_one(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_one(argument: &str) -> String {
    if !argument.is_empty()
        && !argument.contains(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\\')
    {
        return argument.to_string();
    }
    let escaped = argument.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::quote_arguments;

    #[test]
    fn quotes_arguments_with_whitespace() {
        let line = quote_arguments(&[
            "/S".to_string(),
            "/D=C:\\Program Files (x86)\\Foo".to_string(),
        ]);
        assert_eq!(line, "/S \"/D=C:\\\\Program Files (x86)\\\\Foo\"");
    }

    #[test]
    fn skips_quoting_for_simple_arguments() {
        assert_eq!(quote_arguments(&["/S".to_string()]), "/S");
        assert_eq!(quote_arguments(&[]), "");
    }
}
