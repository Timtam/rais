//! `PackageSpec`-aware disk-image helpers.
//!
//! The OS-API mount/copy/find-bundle layer lives in `rabbit-platform`. This
//! module wraps those calls with `rabbit-core` types: it converts
//! [`rabbit_platform::DiskImageError`] into [`RabbitError`] and adds the
//! `PackageSpec`-driven user-plugin search that needs to know each package's
//! prefix/suffix patterns.

use std::fs;
use std::path::{Path, PathBuf};

use rabbit_platform::DiskImageError;
pub use rabbit_platform::{MountedDiskImage, mount_disk_image as platform_mount_disk_image};

use crate::archive::ExtractedUserPlugin;
use crate::error::{IoPathContext, RabbitError, Result};
use crate::package::PackageSpec;

const DIRECTORY_SEARCH_MAX_DEPTH: usize = 6;

pub fn install_app_bundle_from_disk_image(
    image_path: &Path,
    install_destination_dir: &Path,
    bundle_basename: &str,
) -> Result<PathBuf> {
    rabbit_platform::install_app_bundle_from_disk_image(
        image_path,
        install_destination_dir,
        bundle_basename,
    )
    .map_err(rabbit_error_from_disk_image_error)
}

pub fn extract_user_plugin_from_disk_image(
    image_path: &Path,
    spec: &PackageSpec,
    extract_dir: &Path,
) -> Result<ExtractedUserPlugin> {
    let mount =
        platform_mount_disk_image(image_path).map_err(rabbit_error_from_disk_image_error)?;
    let source = find_user_plugin_in_directory(mount.mount_point(), spec)?.ok_or_else(|| {
        RabbitError::DiskImageMissingExtensionBinary {
            image: image_path.to_path_buf(),
            package_id: spec.id.clone(),
        }
    })?;

    let basename = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| RabbitError::DiskImageMissingExtensionBinary {
            image: image_path.to_path_buf(),
            package_id: spec.id.clone(),
        })?
        .to_string();

    fs::create_dir_all(extract_dir).with_path(extract_dir)?;
    let extracted_path = extract_dir.join(&basename);
    if extracted_path.exists() {
        fs::remove_file(&extracted_path).with_path(&extracted_path)?;
    }
    fs::copy(&source, &extracted_path).with_path(&extracted_path)?;

    let entry_name = source
        .strip_prefix(mount.mount_point())
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| source.display().to_string());

    mount.detach().map_err(rabbit_error_from_disk_image_error)?;

    Ok(ExtractedUserPlugin {
        source_archive: image_path.to_path_buf(),
        entry_name,
        extracted_path,
        file_name: basename,
    })
}

pub fn find_user_plugin_in_directory(root: &Path, spec: &PackageSpec) -> Result<Option<PathBuf>> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > DIRECTORY_SEARCH_MAX_DEPTH {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut child_dirs = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if !skip_directory(name) {
                    child_dirs.push(path);
                }
            } else if file_type.is_file() {
                let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if matches_user_plugin_file(file_name, spec) {
                    return Ok(Some(path));
                }
            }
        }
        for child in child_dirs {
            stack.push((child, depth + 1));
        }
    }
    Ok(None)
}

fn matches_user_plugin_file(file_name: &str, spec: &PackageSpec) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let prefix_match = spec
        .user_plugin_prefixes
        .iter()
        .any(|prefix| lower.starts_with(&prefix.to_ascii_lowercase()));
    let suffix_match = spec
        .user_plugin_suffixes
        .iter()
        .any(|suffix| lower.ends_with(&suffix.to_ascii_lowercase()));
    prefix_match && suffix_match
}

fn skip_directory(name: &str) -> bool {
    matches!(
        name,
        ".Trashes" | ".fseventsd" | ".Spotlight-V100" | ".DocumentRevisions-V100"
    )
}

fn rabbit_error_from_disk_image_error(error: DiskImageError) -> RabbitError {
    match error {
        DiskImageError::Io { path, source } => RabbitError::Io { path, source },
        DiskImageError::HdiutilFailed {
            phase,
            image,
            code,
            stderr,
            stdout,
        } => RabbitError::DiskImageMount {
            image,
            message: format!(
                "hdiutil {phase} exited with status {code:?}; stderr: {stderr}; stdout: {stdout}"
            ),
        },
        DiskImageError::NoMountPoint { image, stdout } => RabbitError::DiskImageMount {
            image,
            message: format!("hdiutil attach produced no /Volumes mount point; stdout: {stdout}"),
        },
        DiskImageError::AppBundleNotFound { image, bundle } => {
            RabbitError::DiskImageMissingAppBundle { image, bundle }
        }
        DiskImageError::Unsupported { image, message } => {
            RabbitError::DiskImageMount { image, message }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::find_user_plugin_in_directory;
    use crate::model::Platform;
    use crate::package::{PACKAGE_SWS, package_specs_by_id};

    #[test]
    fn finds_user_plugin_at_root_of_directory_tree() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.txt"), b"docs").unwrap();
        let plugin = dir.path().join("reaper_sws-x86_64.dylib");
        fs::write(&plugin, b"sws").unwrap();

        let spec = package_specs_by_id(Platform::MacOs)
            .remove(PACKAGE_SWS)
            .unwrap();
        let found = find_user_plugin_in_directory(dir.path(), &spec).unwrap();
        assert_eq!(found.as_deref(), Some(plugin.as_path()));
    }

    #[test]
    fn finds_user_plugin_inside_subdirectory() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("Plugins").join("64-bit");
        fs::create_dir_all(&nested).unwrap();
        let plugin = nested.join("reaper_sws-arm64.dylib");
        fs::write(&plugin, b"sws-arm").unwrap();

        let spec = package_specs_by_id(Platform::MacOs)
            .remove(PACKAGE_SWS)
            .unwrap();
        let found = find_user_plugin_in_directory(dir.path(), &spec).unwrap();
        assert_eq!(found.as_deref(), Some(plugin.as_path()));
    }

    #[test]
    fn returns_none_when_no_matching_file_is_present() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.txt"), b"docs").unwrap();
        let spec = package_specs_by_id(Platform::MacOs)
            .remove(PACKAGE_SWS)
            .unwrap();
        let found = find_user_plugin_in_directory(dir.path(), &spec).unwrap();
        assert!(found.is_none());
    }
}
