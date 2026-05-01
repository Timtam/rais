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

/// On Windows, the binary uses the GUI subsystem (so no console pops up on
/// double-click). When the user runs RAIS from `cmd.exe` / PowerShell with
/// arguments, attach to the parent console so stdout/stderr land where the
/// user expects them. No-op on non-Windows / debug builds where the binary
/// already targets the console subsystem.
#[cfg(all(windows, not(debug_assertions), feature = "gui"))]
fn attach_parent_console_on_windows() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        // Best-effort: ignore the result. Failure means no parent console
        // exists (e.g., launched from Explorer with arguments via a shortcut)
        // and that is fine — stdout will simply not be visible.
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(all(windows, not(debug_assertions), feature = "gui")))]
fn attach_parent_console_on_windows() {}
