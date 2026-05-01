//! Platform-specific OS API bindings for RAIS.
//!
//! This crate isolates the Windows/macOS native-API code (file-version probes,
//! and in future slices: disk-image mounting, code-signing verification) from
//! the cross-platform package engine in `rais-core`. Functions here return
//! plain Rust types (no `RaisError`, no `Version`) so the boundary stays one
//! way: `rais-core` depends on `rais-platform`, never the reverse.
//!
//! The first slice exports `read_file_version_parts`, which wraps the Windows
//! VersionInfo APIs. On macOS and other targets it is a no-op that returns
//! `None` so callers don't have to spread `cfg(windows)` everywhere.

pub mod disk_image;
pub mod file_version;
pub mod locale;
pub mod paths;
pub mod registry;
pub mod signature;

pub use disk_image::{
    DiskImageError, MountedDiskImage, copy_directory_recursive, find_app_bundle_in_directory,
    install_app_bundle_from_disk_image, mount_disk_image,
};
pub use file_version::read_file_version_parts;
pub use locale::os_default_locale;
pub use paths::{
    user_appdata_dir, user_home_dir, user_local_appdata_dir, windows_program_files_dirs,
};
pub use registry::{read_uninstall_display_version, read_uninstall_install_location};
pub use signature::{SignatureVerdict, verify_executable_signature};
