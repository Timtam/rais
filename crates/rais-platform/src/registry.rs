//! Windows registry probes for non-RAIS-managed installs.
//!
//! Today this exposes `read_uninstall_display_version`, which reads the
//! `DisplayVersion` REG_SZ value from
//! `(HKCU|HKLM)\Software\Microsoft\Windows\CurrentVersion\Uninstall\<key_name>`,
//! the standard location vendor installers write to so Windows' Programs and
//! Features dialog can show the version. We probe `HKCU` first because
//! per-user installers (OSARA's NSIS installer among them) record there;
//! `HKLM` (with both 64-bit and `WoW6432Node` views) is queried as a fallback
//! for machine-wide installs.
//!
//! On non-Windows platforms the function returns `None`.

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_SZ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};

const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

#[cfg_attr(not(windows), allow(unused_variables))]
pub fn read_uninstall_display_version(key_name: &str) -> Option<String> {
    read_uninstall_display_version_impl(key_name)
}

#[cfg(windows)]
fn read_uninstall_display_version_impl(key_name: &str) -> Option<String> {
    let subkey = format!("{UNINSTALL_KEY}\\{key_name}");
    // HKCU has no 32/64-bit redirection, so a single view suffices there.
    // HKLM gets both views to cover 32-bit installers on 64-bit Windows.
    let candidates: [(HKEY, u32); 3] = [
        (HKEY_CURRENT_USER, 0),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    ];
    for (root, view) in candidates {
        if let Some(version) = query_display_version(root, &subkey, view) {
            return Some(version);
        }
    }
    None
}

#[cfg(not(windows))]
fn read_uninstall_display_version_impl(_key_name: &str) -> Option<String> {
    None
}

#[cfg(windows)]
fn query_display_version(root: HKEY, subkey: &str, view: u32) -> Option<String> {
    let subkey_w = wide_string(subkey);
    let value_w = wide_string("DisplayVersion");
    let mut hkey = std::ptr::null_mut();
    let access = KEY_QUERY_VALUE | view;
    let status = unsafe { RegOpenKeyExW(root, subkey_w.as_ptr(), 0, access, &mut hkey) };
    if status != 0 || hkey.is_null() {
        return None;
    }

    let result = read_string_value(hkey, value_w.as_ptr());
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    result
}

#[cfg(windows)]
fn read_string_value(
    hkey: windows_sys::Win32::System::Registry::HKEY,
    value_name: *const u16,
) -> Option<String> {
    let mut value_type = 0u32;
    let mut data_size = 0u32;
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            value_name,
            std::ptr::null_mut(),
            &mut value_type,
            std::ptr::null_mut(),
            &mut data_size,
        )
    };
    if status != 0 || value_type != REG_SZ || data_size == 0 {
        return None;
    }

    // data_size is in bytes; allocate as u16 buffer with rounding up.
    let chars = ((data_size as usize) + 1) / 2;
    let mut buffer = vec![0u16; chars];
    let mut data_size_inout = (chars * 2) as u32;
    let status = unsafe {
        RegQueryValueExW(
            hkey,
            value_name,
            std::ptr::null_mut(),
            &mut value_type,
            buffer.as_mut_ptr().cast::<u8>(),
            &mut data_size_inout,
        )
    };
    if status != 0 || value_type != REG_SZ {
        return None;
    }

    // Trim trailing NUL terminator(s) if present.
    while buffer.last().copied() == Some(0) {
        buffer.pop();
    }
    String::from_utf16(&buffer).ok().filter(|s| !s.is_empty())
}

#[cfg(windows)]
fn wide_string(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}
