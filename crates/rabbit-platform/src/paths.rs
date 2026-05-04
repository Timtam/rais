//! Per-user OS-specific path lookups.
//!
//! These helpers wrap the env-var probes that used to be scattered across
//! `rabbit-core` (`APPDATA`, `LOCALAPPDATA`, `HOME`, `ProgramFiles*`). Each
//! function is callable on any platform and returns `None`/empty when the
//! variable isn't set, so callers don't have to spread `cfg(target_os)` around
//! their own code paths.

use std::env;
use std::path::PathBuf;

/// `%APPDATA%` on Windows (`Roaming` per-user app-data root). Returns `None`
/// when the variable isn't exported (most non-Windows hosts).
pub fn user_appdata_dir() -> Option<PathBuf> {
    env::var_os("APPDATA").map(PathBuf::from)
}

/// `%LOCALAPPDATA%` on Windows (per-user, machine-local app-data root).
pub fn user_local_appdata_dir() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// `$HOME` on Unix-like hosts.
pub fn user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

/// `%ProgramFiles%` and `%ProgramFiles(x86)%` (in that order) on Windows.
/// Empty on hosts where neither variable is set.
pub fn windows_program_files_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(program_files) = env::var_os("ProgramFiles") {
        dirs.push(PathBuf::from(program_files));
    }
    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        dirs.push(PathBuf::from(program_files_x86));
    }
    dirs
}
