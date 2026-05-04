use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use crate::error::{IoPathContext, RabbitError, Result};
use crate::package::PackageSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedUserPlugin {
    pub source_archive: PathBuf,
    pub entry_name: String,
    pub extracted_path: PathBuf,
    pub file_name: String,
}

pub fn extract_user_plugin_from_archive(
    archive_path: &Path,
    spec: &PackageSpec,
    extract_dir: &Path,
) -> Result<ExtractedUserPlugin> {
    let file = fs::File::open(archive_path).with_path(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(BufReader::new(file)).map_err(|source| RabbitError::ArchiveRead {
            archive: archive_path.to_path_buf(),
            message: source.to_string(),
        })?;

    let mut selected: Option<(usize, String, String)> = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|source| RabbitError::ArchiveRead {
                archive: archive_path.to_path_buf(),
                message: source.to_string(),
            })?;
        if !entry.is_file() {
            continue;
        }
        let entry_name = entry.name().to_string();
        let Some(basename) = Path::new(&entry_name)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
        else {
            continue;
        };
        if matches_user_plugin_file(&basename, spec) {
            selected = Some((index, entry_name, basename));
            break;
        }
    }

    let (index, entry_name, basename) =
        selected.ok_or_else(|| RabbitError::ArchiveMissingExtensionBinary {
            archive: archive_path.to_path_buf(),
            package_id: spec.id.clone(),
        })?;

    fs::create_dir_all(extract_dir).with_path(extract_dir)?;
    let extracted_path = extract_dir.join(&basename);
    if extracted_path.exists() {
        fs::remove_file(&extracted_path).with_path(&extracted_path)?;
    }

    let mut entry = archive
        .by_index(index)
        .map_err(|source| RabbitError::ArchiveRead {
            archive: archive_path.to_path_buf(),
            message: source.to_string(),
        })?;
    let mut output = fs::File::create(&extracted_path).with_path(&extracted_path)?;
    std::io::copy(&mut entry, &mut output).with_path(&extracted_path)?;
    output.flush().with_path(&extracted_path)?;

    Ok(ExtractedUserPlugin {
        source_archive: archive_path.to_path_buf(),
        entry_name,
        extracted_path,
        file_name: basename,
    })
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

pub fn extract_all_files_flat(archive_path: &Path, extract_dir: &Path) -> Result<Vec<PathBuf>> {
    let file = fs::File::open(archive_path).with_path(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(BufReader::new(file)).map_err(|source| RabbitError::ArchiveRead {
            archive: archive_path.to_path_buf(),
            message: source.to_string(),
        })?;

    fs::create_dir_all(extract_dir).with_path(extract_dir)?;
    let mut extracted = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|source| RabbitError::ArchiveRead {
                archive: archive_path.to_path_buf(),
                message: source.to_string(),
            })?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_string();
        let Some(basename) = Path::new(&name)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        let target = extract_dir.join(&basename);
        if target.exists() {
            fs::remove_file(&target).with_path(&target)?;
        }
        let mut output = fs::File::create(&target).with_path(&target)?;
        std::io::copy(&mut entry, &mut output).with_path(&target)?;
        output.flush().with_path(&target)?;
        extracted.push(target);
    }
    extracted.sort();
    Ok(extracted)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedOsaraAssets {
    pub source_archive: PathBuf,
    pub installed_files: Vec<PathBuf>,
}

const OSARA_INSTALLER_RESOURCES_PREFIX: &str = "OSARAInstaller.app/Contents/Resources/";
const OSARA_DYLIB_BASENAME: &str = "reaper_osara.dylib";
const OSARA_KEYMAP_BASENAME: &str = "OSARA.ReaperKeyMap";
const OSARA_LOCALE_PREFIX: &str = "locale/";
const OSARA_LOCALE_EXTENSION: &str = ".po";

pub fn extract_osara_macos_assets(
    archive_path: &Path,
    resource_path: &Path,
) -> Result<ExtractedOsaraAssets> {
    let file = fs::File::open(archive_path).with_path(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(BufReader::new(file)).map_err(|source| RabbitError::ArchiveRead {
            archive: archive_path.to_path_buf(),
            message: source.to_string(),
        })?;

    let user_plugins = resource_path.join("UserPlugins");
    let key_maps = resource_path.join("KeyMaps");
    let osara_locale = resource_path.join("osara").join("locale");

    let mut installed_files = Vec::new();
    let mut found_dylib = false;
    let mut found_keymap = false;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|source| RabbitError::ArchiveRead {
                archive: archive_path.to_path_buf(),
                message: source.to_string(),
            })?;
        if !entry.is_file() {
            continue;
        }
        let entry_name = entry.name().to_string();
        let Some(suffix) = entry_name.strip_prefix(OSARA_INSTALLER_RESOURCES_PREFIX) else {
            continue;
        };

        let target = if suffix == OSARA_DYLIB_BASENAME {
            found_dylib = true;
            user_plugins.join(OSARA_DYLIB_BASENAME)
        } else if suffix == OSARA_KEYMAP_BASENAME {
            found_keymap = true;
            key_maps.join(OSARA_KEYMAP_BASENAME)
        } else if let Some(locale_suffix) = suffix.strip_prefix(OSARA_LOCALE_PREFIX) {
            if !locale_suffix.ends_with(OSARA_LOCALE_EXTENSION) || locale_suffix.contains('/') {
                continue;
            }
            osara_locale.join(locale_suffix)
        } else {
            continue;
        };

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_path(parent)?;
        }
        let mut output = fs::File::create(&target).with_path(&target)?;
        std::io::copy(&mut entry, &mut output).with_path(&target)?;
        output.flush().with_path(&target)?;
        installed_files.push(target);
    }

    if !found_dylib || !found_keymap {
        return Err(RabbitError::OsaraArchiveMissingAssets {
            archive: archive_path.to_path_buf(),
        });
    }

    installed_files.sort();
    Ok(ExtractedOsaraAssets {
        source_archive: archive_path.to_path_buf(),
        installed_files,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    use super::{extract_osara_macos_assets, extract_user_plugin_from_archive};
    use crate::error::RabbitError;
    use crate::model::Platform;
    use crate::package::{PACKAGE_REAKONTROL, package_specs_by_id};

    #[test]
    fn extracts_matching_user_plugin_binary_from_zip() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("reaKontrol_windows_test.zip");
        write_test_archive(
            &archive_path,
            &[
                ("README.md", b"docs"),
                ("reaper_kontrol.dll", b"plugin-bytes"),
            ],
        );

        let spec = package_specs_by_id(Platform::Windows)
            .remove(PACKAGE_REAKONTROL)
            .unwrap();
        let extract_dir = dir.path().join("extract");
        let extracted =
            extract_user_plugin_from_archive(&archive_path, &spec, &extract_dir).unwrap();

        assert_eq!(extracted.file_name, "reaper_kontrol.dll");
        assert_eq!(
            std::fs::read(&extracted.extracted_path).unwrap(),
            b"plugin-bytes"
        );
    }

    #[test]
    fn errors_when_archive_lacks_user_plugin_binary() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("empty.zip");
        write_test_archive(&archive_path, &[("README.md", b"docs")]);

        let spec = package_specs_by_id(Platform::Windows)
            .remove(PACKAGE_REAKONTROL)
            .unwrap();
        let error = extract_user_plugin_from_archive(&archive_path, &spec, dir.path()).unwrap_err();

        assert!(matches!(
            error,
            RabbitError::ArchiveMissingExtensionBinary { .. }
        ));
    }

    #[test]
    fn finds_binary_inside_nested_directory() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("nested.zip");
        write_test_archive(
            &archive_path,
            &[("subdir/reaper_kontrol.dylib", b"mac-plugin")],
        );

        let spec = package_specs_by_id(Platform::MacOs)
            .remove(PACKAGE_REAKONTROL)
            .unwrap();
        let extracted = extract_user_plugin_from_archive(&archive_path, &spec, dir.path()).unwrap();

        assert_eq!(extracted.file_name, "reaper_kontrol.dylib");
        assert_eq!(
            std::fs::read(&extracted.extracted_path).unwrap(),
            b"mac-plugin"
        );
    }

    #[test]
    fn extracts_osara_macos_assets_into_resource_path() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("osara_test.zip");
        write_test_archive(
            &archive_path,
            &[
                ("OSARAInstaller.app/Contents/MacOS/applet", b"applet-binary"),
                (
                    "OSARAInstaller.app/Contents/Resources/reaper_osara.dylib",
                    b"osara-plugin",
                ),
                (
                    "OSARAInstaller.app/Contents/Resources/OSARA.ReaperKeyMap",
                    b"keymap-content",
                ),
                (
                    "OSARAInstaller.app/Contents/Resources/locale/de_DE.po",
                    b"de-locale",
                ),
                (
                    "OSARAInstaller.app/Contents/Resources/locale/fr_FR.po",
                    b"fr-locale",
                ),
                (
                    "OSARAInstaller.app/Contents/Resources/copying.txt",
                    b"license-text",
                ),
            ],
        );
        let resource_path = dir.path().join("REAPER");

        let report = extract_osara_macos_assets(&archive_path, &resource_path).unwrap();

        let dylib = resource_path.join("UserPlugins").join("reaper_osara.dylib");
        let keymap = resource_path.join("KeyMaps").join("OSARA.ReaperKeyMap");
        let de_locale = resource_path.join("osara").join("locale").join("de_DE.po");
        let fr_locale = resource_path.join("osara").join("locale").join("fr_FR.po");
        assert_eq!(std::fs::read(&dylib).unwrap(), b"osara-plugin");
        assert_eq!(std::fs::read(&keymap).unwrap(), b"keymap-content");
        assert_eq!(std::fs::read(&de_locale).unwrap(), b"de-locale");
        assert_eq!(std::fs::read(&fr_locale).unwrap(), b"fr-locale");
        assert!(report.installed_files.contains(&dylib));
        assert!(report.installed_files.contains(&keymap));
        assert!(report.installed_files.contains(&de_locale));
        assert!(report.installed_files.contains(&fr_locale));
        assert!(!resource_path.join("copying.txt").exists());
        assert!(!resource_path.join("Contents").exists());
    }

    #[test]
    fn errors_when_osara_archive_is_missing_dylib_or_keymap() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("partial.zip");
        write_test_archive(
            &archive_path,
            &[(
                "OSARAInstaller.app/Contents/Resources/locale/en_US.po",
                b"en-locale",
            )],
        );
        let resource_path = dir.path().join("REAPER");

        let error = extract_osara_macos_assets(&archive_path, &resource_path).unwrap_err();
        assert!(matches!(
            error,
            RabbitError::OsaraArchiveMissingAssets { .. }
        ));
    }

    fn write_test_archive(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }
}
