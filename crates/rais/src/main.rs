// Suppress the Windows console window when launched as a GUI. The CLI path
// uses `AttachConsole(ATTACH_PARENT_PROCESS)` (or, when launched standalone
// from File Explorer, allocates a fresh console) so help/version output is
// still visible. Without this attribute the same binary would briefly pop a
// console window on every GUI start.
#![cfg_attr(
    all(windows, not(debug_assertions), feature = "gui"),
    windows_subsystem = "windows"
)]

fn main() -> std::process::ExitCode {
    // No arguments → run the GUI wizard (when the gui feature is on).
    // Anything else, including `--help`, hands off to the CLI subcommand
    // parser. `args_os().count() == 1` covers the program-name-only case
    // since clap counts argv positions the same way.
    #[cfg(feature = "gui")]
    {
        if std::env::args_os().count() <= 1 {
            rais_ui_wxdragon::run_gui();
            return std::process::ExitCode::SUCCESS;
        }
    }

    attach_parent_console_on_windows();
    match rais_cli::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// On Windows the release binary targets the GUI subsystem (no console
/// pop-up on double-click). When the user runs RAIS from `cmd.exe` /
/// PowerShell with arguments, attach to the parent console *and* redirect
/// the std file handles so `println!` / `eprintln!` output lands where the
/// user expects. No-op on non-Windows builds and on debug builds, where the
/// binary already targets the console subsystem.
#[cfg(all(windows, not(debug_assertions), feature = "gui"))]
fn attach_parent_console_on_windows() {
    use std::ptr;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };

    unsafe {
        // Best-effort: failure means no parent console exists (e.g., launched
        // from Explorer with arguments via a shortcut). In that case stdout
        // simply has nowhere to go; we still want the CLI subcommand to run.
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }

        // Only bind CONOUT$/CONIN$ when nothing was inherited. If the user
        // redirected stdout/stderr (`rais.exe ... | tee out.txt`,
        // `rais.exe ... > log`), the OS already inherits the pipe/file
        // handles from the parent process even for a GUI-subsystem binary;
        // overwriting them would silently send output to the console
        // instead of the redirect target.
        let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        bind_if_unset(STD_OUTPUT_HANDLE, &conout, GENERIC_WRITE, FILE_SHARE_WRITE);
        bind_if_unset(STD_ERROR_HANDLE, &conout, GENERIC_WRITE, FILE_SHARE_WRITE);
        let conin: Vec<u16> = "CONIN$\0".encode_utf16().collect();
        bind_if_unset(STD_INPUT_HANDLE, &conin, GENERIC_READ, FILE_SHARE_READ);
    }

    /// Bind `device` (`CONOUT$` or `CONIN$`) to `which` only when the std
    /// handle is currently null/invalid. Preserves whatever the parent
    /// process inherited (pipes for `|`, files for `>`, etc.).
    unsafe fn bind_if_unset(which: u32, device: &[u16], access: u32, share: u32) {
        unsafe {
            let existing = GetStdHandle(which);
            if !existing.is_null() && existing != INVALID_HANDLE_VALUE {
                return;
            }
            let h = CreateFileW(
                device.as_ptr(),
                access,
                share,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            );
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                SetStdHandle(which, h);
            }
        }
    }
}

#[cfg(not(all(windows, not(debug_assertions), feature = "gui")))]
fn attach_parent_console_on_windows() {}
