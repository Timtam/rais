use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::process::Command;

use crate::archive::ExtractedUserPlugin;
use crate::error::{IoPathContext, RaisError, Result};
use crate::package::PackageSpec;

const DIRECTORY_SEARCH_MAX_DEPTH: usize = 6;

pub struct MountedDiskImage {
    image_path: PathBuf,
    mount_point: PathBuf,
    detached: bool,
}

impl MountedDiskImage {
    pub fn mount_point(&self) -> &Path {
        &self.mount_point
    }

    pub fn detach(mut self) -> Result<()> {
        self.detach_inner()
    }

    fn detach_inner(&mut self) -> Result<()> {
        if self.detached {
            return Ok(());
        }
        self.detached = true;
        run_hdiutil_detach(&self.mount_point, &self.image_path)
    }
}

impl Drop for MountedDiskImage {
    fn drop(&mut self) {
        if !self.detached {
            let _ = self.detach_inner();
        }
    }
}

pub fn mount_disk_image(image_path: &Path) -> Result<MountedDiskImage> {
    let mount_point = run_hdiutil_attach(image_path)?;
    Ok(MountedDiskImage {
        image_path: image_path.to_path_buf(),
        mount_point,
        detached: false,
    })
}

pub fn install_app_bundle_from_disk_image(
    image_path: &Path,
    install_destination_dir: &Path,
    bundle_basename: &str,
) -> Result<PathBuf> {
    let mount = mount_disk_image(image_path)?;
    let source =
        find_app_bundle_in_directory(mount.mount_point(), bundle_basename)?.ok_or_else(|| {
            RaisError::DiskImageMissingAppBundle {
                image: image_path.to_path_buf(),
                bundle: bundle_basename.to_string(),
            }
        })?;

    fs::create_dir_all(install_destination_dir).with_path(install_destination_dir)?;
    let target = install_destination_dir.join(bundle_basename);
    if target.exists() {
        remove_path_recursive(&target)?;
    }
    copy_directory_recursive(&source, &target)?;

    mount.detach()?;
    Ok(target)
}

pub(crate) fn find_app_bundle_in_directory(root: &Path, basename: &str) -> Result<Option<PathBuf>> {
    let target = basename.to_ascii_lowercase();
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
            if !file_type.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.to_ascii_lowercase() == target {
                return Ok(Some(path));
            }
            if !skip_directory(name) {
                child_dirs.push(path);
            }
        }
        for child in child_dirs {
            stack.push((child, depth + 1));
        }
    }
    Ok(None)
}

pub(crate) fn copy_directory_recursive(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_path(dest)?;
    for entry in fs::read_dir(source).with_path(source)? {
        let entry = entry.with_path(source)?;
        let entry_path = entry.path();
        let entry_name = entry.file_name();
        let target_path = dest.join(&entry_name);
        let file_type = entry.file_type().with_path(&entry_path)?;
        if file_type.is_dir() {
            copy_directory_recursive(&entry_path, &target_path)?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let link_target = fs::read_link(&entry_path).with_path(&entry_path)?;
                std::os::unix::fs::symlink(&link_target, &target_path).with_path(&target_path)?;
            }
            #[cfg(not(unix))]
            {
                fs::copy(&entry_path, &target_path).with_path(&target_path)?;
            }
        } else {
            fs::copy(&entry_path, &target_path).with_path(&target_path)?;
        }
    }
    Ok(())
}

fn remove_path_recursive(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path).with_path(path)
    } else if path.exists() {
        fs::remove_file(path).with_path(path)
    } else {
        Ok(())
    }
}

pub fn extract_user_plugin_from_disk_image(
    image_path: &Path,
    spec: &PackageSpec,
    extract_dir: &Path,
) -> Result<ExtractedUserPlugin> {
    let mount = mount_disk_image(image_path)?;
    let source = find_user_plugin_in_directory(mount.mount_point(), spec)?.ok_or_else(|| {
        RaisError::DiskImageMissingExtensionBinary {
            image: image_path.to_path_buf(),
            package_id: spec.id.clone(),
        }
    })?;

    let basename = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| RaisError::DiskImageMissingExtensionBinary {
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

    mount.detach()?;

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

#[cfg(target_os = "macos")]
fn run_hdiutil_attach(image_path: &Path) -> Result<PathBuf> {
    let output = Command::new("hdiutil")
        .arg("attach")
        .arg("-nobrowse")
        .arg("-quiet")
        .arg("-readonly")
        .arg(image_path)
        .output()
        .map_err(|source| RaisError::Io {
            path: image_path.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return Err(RaisError::DiskImageMount {
            image: image_path.to_path_buf(),
            message: format!(
                "hdiutil attach exited with status {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    parse_hdiutil_attach_output(&stdout).ok_or_else(|| RaisError::DiskImageMount {
        image: image_path.to_path_buf(),
        message: "hdiutil attach produced no /Volumes mount point".to_string(),
    })
}

#[cfg(not(target_os = "macos"))]
fn run_hdiutil_attach(image_path: &Path) -> Result<PathBuf> {
    Err(RaisError::DiskImageMount {
        image: image_path.to_path_buf(),
        message: "disk image mounting is only supported on macOS".to_string(),
    })
}

#[cfg(target_os = "macos")]
fn run_hdiutil_detach(mount_point: &Path, image_path: &Path) -> Result<()> {
    let output = Command::new("hdiutil")
        .arg("detach")
        .arg("-force")
        .arg(mount_point)
        .output()
        .map_err(|source| RaisError::Io {
            path: image_path.to_path_buf(),
            source,
        })?;
    if !output.status.success() {
        return Err(RaisError::DiskImageMount {
            image: image_path.to_path_buf(),
            message: format!(
                "hdiutil detach exited with status {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn run_hdiutil_detach(_mount_point: &Path, _image_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn parse_hdiutil_attach_output(stdout: &str) -> Option<PathBuf> {
    for line in stdout.lines() {
        if let Some(start) = line.find("/Volumes/") {
            let candidate = line[start..].trim_end();
            if !candidate.is_empty() {
                return Some(PathBuf::from(candidate));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        copy_directory_recursive, find_app_bundle_in_directory, find_user_plugin_in_directory,
        parse_hdiutil_attach_output,
    };
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

    #[test]
    fn parses_volumes_line_from_hdiutil_attach_output() {
        let output = "/dev/disk5          \tApple_partition_scheme\t\n\
                      /dev/disk5s1        \tApple_partition_map   \t\n\
                      /dev/disk5s2        \tApple_HFS             \t/Volumes/SWS Extension\n";
        let mount = parse_hdiutil_attach_output(output).unwrap();
        assert_eq!(mount.to_str().unwrap(), "/Volumes/SWS Extension");
    }

    #[test]
    fn returns_no_mount_point_for_unrelated_output() {
        assert!(parse_hdiutil_attach_output("hdiutil: attach: error\n").is_none());
    }

    #[test]
    fn finds_app_bundle_at_root_of_directory_tree() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("REAPER.app");
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        fs::write(bundle.join("Contents").join("Info.plist"), b"<plist/>").unwrap();

        let found = find_app_bundle_in_directory(dir.path(), "REAPER.app").unwrap();
        assert_eq!(found.as_deref(), Some(bundle.as_path()));
    }

    #[test]
    fn matches_app_bundle_basename_case_insensitively() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("Reaper.app");
        fs::create_dir_all(&bundle).unwrap();

        let found = find_app_bundle_in_directory(dir.path(), "REAPER.app").unwrap();
        assert_eq!(found.as_deref(), Some(bundle.as_path()));
    }

    #[test]
    fn returns_none_for_missing_app_bundle() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("README")).unwrap();
        let found = find_app_bundle_in_directory(dir.path(), "REAPER.app").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn copies_directory_tree_with_nested_files() {
        let source_root = tempdir().unwrap();
        let source = source_root.path().join("REAPER.app");
        fs::create_dir_all(source.join("Contents").join("MacOS")).unwrap();
        fs::write(source.join("Contents").join("Info.plist"), b"<plist/>").unwrap();
        fs::write(
            source.join("Contents").join("MacOS").join("REAPER"),
            b"binary",
        )
        .unwrap();

        let dest_root = tempdir().unwrap();
        let dest = dest_root.path().join("REAPER.app");
        copy_directory_recursive(&source, &dest).unwrap();

        assert_eq!(
            fs::read(dest.join("Contents").join("MacOS").join("REAPER")).unwrap(),
            b"binary"
        );
        assert_eq!(
            fs::read(dest.join("Contents").join("Info.plist")).unwrap(),
            b"<plist/>"
        );
    }
}
