//! Per-binary version probe.
//!
//! Returns the 4-tuple `(major, minor, build, revision)` so callers in
//! `rabbit-core::metadata` can format it however they like. Implementations:
//!
//! * Windows — reads `VS_FIXEDFILEINFO` off the binary.
//! * macOS — when the path is a `.app` bundle directory, parses
//!   `Contents/Info.plist` for `CFBundleShortVersionString`
//!   (falling back to `CFBundleVersion`) and pads the dotted version into the
//!   4-tuple shape the API expects.
//! * Other platforms — `None`.

use std::path::Path;

pub fn read_file_version_parts(path: &Path) -> Option<[u32; 4]> {
    platform_read_file_version_parts(path)
}

/// Read the StringFileInfo "FileVersion" field off a Windows binary.
///
/// This is the *string-form* version a vendor sets via NSIS's
/// `VIAddVersionKey "FileVersion" "<value>"` (or its MSVC equivalent) — it
/// can be a free-form string like "89" or "1.2.3" that doesn't have to match
/// the binary VS_FIXEDFILEINFO returned by [`read_file_version_parts`]. RABBIT
/// uses this for packages whose canonical version lives only in the string
/// resource (the JAWS-for-REAPER scripts' Uninstall.exe is the current
/// example).
///
/// Returns `None` when the file has no version resource, no StringFileInfo
/// block, or no `FileVersion` entry. Always `None` on non-Windows hosts.
pub fn read_file_version_string(path: &Path) -> Option<String> {
    platform_read_file_version_string(path)
}

#[cfg(windows)]
fn platform_read_file_version_parts(path: &Path) -> Option<[u32; 4]> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = 0_u32;
    let size = unsafe { GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }

    let mut data = vec![0_u8; size as usize];
    let ok = unsafe {
        GetFileVersionInfoW(
            wide_path.as_ptr(),
            0,
            size,
            data.as_mut_ptr().cast::<c_void>(),
        )
    };
    if ok == 0 {
        return None;
    }

    let root: Vec<u16> = "\\".encode_utf16().chain(Some(0)).collect();
    let mut value: *mut c_void = std::ptr::null_mut();
    let mut len = 0_u32;
    let ok = unsafe {
        VerQueryValueW(
            data.as_ptr().cast::<c_void>(),
            root.as_ptr(),
            &mut value,
            &mut len,
        )
    };
    if ok == 0 || value.is_null() || len < std::mem::size_of::<VS_FIXEDFILEINFO>() as u32 {
        return None;
    }

    let info = unsafe { &*(value.cast::<VS_FIXEDFILEINFO>()) };
    if info.dwSignature != 0xFEEF04BD {
        return None;
    }

    Some([
        (info.dwFileVersionMS >> 16) & 0xffff,
        info.dwFileVersionMS & 0xffff,
        (info.dwFileVersionLS >> 16) & 0xffff,
        info.dwFileVersionLS & 0xffff,
    ])
}

#[cfg(target_os = "macos")]
fn platform_read_file_version_parts(path: &Path) -> Option<[u32; 4]> {
    if !path.is_dir() {
        return None;
    }
    let plist_path = path.join("Contents").join("Info.plist");
    if !plist_path.is_file() {
        return None;
    }
    let value = plist::Value::from_file(&plist_path).ok()?;
    let dict = value.as_dictionary()?;
    let raw = dict
        .get("CFBundleShortVersionString")
        .or_else(|| dict.get("CFBundleVersion"))?
        .as_string()?;
    parse_dotted_version_parts(raw)
}

#[cfg(target_os = "macos")]
fn parse_dotted_version_parts(version: &str) -> Option<[u32; 4]> {
    let mut parts = [0u32; 4];
    let mut count = 0;
    for component in version.split('.').map(str::trim) {
        if count >= parts.len() {
            break;
        }
        let parsed: u32 = component.parse().ok()?;
        parts[count] = parsed;
        count += 1;
    }
    if count == 0 { None } else { Some(parts) }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn platform_read_file_version_parts(_path: &Path) -> Option<[u32; 4]> {
    None
}

#[cfg(windows)]
fn platform_read_file_version_string(path: &Path) -> Option<String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = 0_u32;
    let size = unsafe { GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }

    let mut data = vec![0_u8; size as usize];
    let ok = unsafe {
        GetFileVersionInfoW(
            wide_path.as_ptr(),
            0,
            size,
            data.as_mut_ptr().cast::<c_void>(),
        )
    };
    if ok == 0 {
        return None;
    }

    // Look up `\VarFileInfo\Translation` to find the (language, codepage) pair
    // the file actually uses. Each translation entry is a packed
    // `[u16 lang, u16 codepage]` value; we just need the first pair to build
    // the StringFileInfo subkey.
    let translation_key: Vec<u16> = "\\VarFileInfo\\Translation"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut value: *mut c_void = std::ptr::null_mut();
    let mut len: u32 = 0;
    let ok = unsafe {
        VerQueryValueW(
            data.as_ptr().cast::<c_void>(),
            translation_key.as_ptr(),
            &mut value,
            &mut len,
        )
    };
    let (lang, codepage) = if ok != 0 && !value.is_null() && len >= 4 {
        let translations = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), 2) };
        (translations[0], translations[1])
    } else {
        // Fall back to the most common US-English / Unicode pair so we still
        // probe a likely subkey when the file omits the Translation block.
        (0x0409, 0x04B0)
    };

    // The subkey path is `\StringFileInfo\<lang><codepage>\FileVersion` with
    // the language/codepage written as 8 lowercase hex digits.
    let subkey_str = format!("\\StringFileInfo\\{lang:04x}{codepage:04x}\\FileVersion");
    let subkey: Vec<u16> = subkey_str.encode_utf16().chain(Some(0)).collect();

    let mut value: *mut c_void = std::ptr::null_mut();
    let mut len: u32 = 0;
    let ok = unsafe {
        VerQueryValueW(
            data.as_ptr().cast::<c_void>(),
            subkey.as_ptr(),
            &mut value,
            &mut len,
        )
    };
    if ok == 0 || value.is_null() || len == 0 {
        return None;
    }

    // `len` is character count including the trailing NUL.
    let chars = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), len as usize) };
    let trimmed: Vec<u16> = chars.iter().take_while(|&&c| c != 0).copied().collect();
    let raw = String::from_utf16(&trimmed).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(not(windows))]
fn platform_read_file_version_string(_path: &Path) -> Option<String> {
    None
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::parse_dotted_version_parts;

    #[test]
    fn parses_short_versions() {
        assert_eq!(parse_dotted_version_parts("7.69"), Some([7, 69, 0, 0]));
        assert_eq!(parse_dotted_version_parts("7.69.0.0"), Some([7, 69, 0, 0]));
        assert_eq!(parse_dotted_version_parts("7"), Some([7, 0, 0, 0]));
        assert_eq!(
            parse_dotted_version_parts("7.69.1.2.3"),
            Some([7, 69, 1, 2])
        );
    }

    #[test]
    fn rejects_non_numeric() {
        assert_eq!(parse_dotted_version_parts(""), None);
        assert_eq!(parse_dotted_version_parts("abc"), None);
        assert_eq!(parse_dotted_version_parts("7.x"), None);
    }
}
