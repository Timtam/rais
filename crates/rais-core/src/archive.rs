use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use crate::error::{IoPathContext, RaisError, Result};
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
        zip::ZipArchive::new(BufReader::new(file)).map_err(|source| RaisError::ArchiveRead {
            archive: archive_path.to_path_buf(),
            message: source.to_string(),
        })?;

    let mut selected: Option<(usize, String, String)> = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|source| RaisError::ArchiveRead {
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
        selected.ok_or_else(|| RaisError::ArchiveMissingExtensionBinary {
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
        .map_err(|source| RaisError::ArchiveRead {
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    use super::extract_user_plugin_from_archive;
    use crate::error::RaisError;
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
            RaisError::ArchiveMissingExtensionBinary { .. }
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
