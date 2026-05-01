//! Windows VersionInfo probe.
//!
//! Returns the 4-tuple `(major, minor, build, revision)` from a binary's
//! `VS_FIXEDFILEINFO` resource on Windows; returns `None` on every other
//! platform and on Windows when the binary lacks the resource. Conversion to
//! a printable version string (and the REAPER-special-case formatting) lives
//! in `rais-core::metadata` so this crate never imports `rais-core` types.

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

#[cfg(not(windows))]
fn platform_read_file_version_parts(_path: &Path) -> Option<[u32; 4]> {
    None
}
