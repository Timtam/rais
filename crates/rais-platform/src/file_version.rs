//! Per-binary version probe.
//!
//! Returns the 4-tuple `(major, minor, build, revision)` so callers in
//! `rais-core::metadata` can format it however they like. Implementations:
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
