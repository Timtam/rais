use std::path::Path;

use crate::Result;
use crate::version::Version;

pub fn file_version(path: &Path) -> Result<Option<Version>> {
    platform_file_version(path)
}

#[cfg(windows)]
fn platform_file_version(path: &Path) -> Result<Option<Version>> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = 0_u32;
    let size = unsafe { GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut handle) };
    if size == 0 {
        return Ok(None);
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
        return Ok(None);
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
        return Ok(None);
    }

    let info = unsafe { &*(value.cast::<VS_FIXEDFILEINFO>()) };
    if info.dwSignature != 0xFEEF04BD {
        return Ok(None);
    }

    let parts = [
        (info.dwFileVersionMS >> 16) & 0xffff,
        info.dwFileVersionMS & 0xffff,
        (info.dwFileVersionLS >> 16) & 0xffff,
        info.dwFileVersionLS & 0xffff,
    ];
    let version = trim_trailing_zero_parts(&parts)
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");

    Version::parse(version).map(Some)
}

#[cfg(not(windows))]
fn platform_file_version(_path: &Path) -> Result<Option<Version>> {
    Ok(None)
}

fn trim_trailing_zero_parts(parts: &[u32; 4]) -> &[u32] {
    let mut len = parts.len();
    while len > 2 && parts[len - 1] == 0 {
        len -= 1;
    }
    &parts[..len]
}

#[cfg(test)]
mod tests {
    use super::trim_trailing_zero_parts;

    #[test]
    fn trims_trailing_zero_parts_but_keeps_major_minor() {
        assert_eq!(trim_trailing_zero_parts(&[7, 69, 0, 0]), &[7, 69]);
        assert_eq!(trim_trailing_zero_parts(&[2, 14, 0, 7]), &[2, 14, 0, 7]);
        assert_eq!(trim_trailing_zero_parts(&[1, 2, 6, 0]), &[1, 2, 6]);
    }
}
