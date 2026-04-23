use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{IoPathContext, Result};
use crate::metadata::file_version;
use crate::model::{
    Architecture, ComponentDetection, Confidence, Evidence, Installation, InstallationKind,
    Platform,
};
use crate::package::{PACKAGE_OSARA, PackageSpec, builtin_package_specs};
use crate::reapack::package_owner_for_file;
use crate::receipt::{ReceiptVerification, load_install_state, verify_package_receipt};

#[derive(Debug, Clone, Default)]
pub struct DiscoveryOptions {
    pub include_standard: bool,
    pub portable_roots: Vec<PathBuf>,
}

impl DiscoveryOptions {
    pub fn standard() -> Self {
        Self {
            include_standard: true,
            portable_roots: Vec::new(),
        }
    }
}

pub fn discover_installations(options: &DiscoveryOptions) -> Result<Vec<Installation>> {
    let Some(platform) = Platform::current() else {
        return Ok(Vec::new());
    };

    let mut installations = Vec::new();

    if options.include_standard {
        if let Some(standard) = discover_standard_installation(platform) {
            installations.push(standard);
        }
    }

    for portable_root in &options.portable_roots {
        if let Some(portable) = discover_portable_installation(platform, portable_root) {
            installations.push(portable);
        }
    }

    Ok(installations)
}

pub fn detect_components(
    resource_path: &Path,
    platform: Platform,
) -> Result<Vec<ComponentDetection>> {
    let state = load_install_state(resource_path)?;
    let mut detections = Vec::new();

    for spec in builtin_package_specs(platform) {
        detections.push(detect_component(
            resource_path,
            platform,
            &spec,
            state.as_ref(),
        )?);
    }

    Ok(detections)
}

pub fn detect_component(
    resource_path: &Path,
    platform: Platform,
    spec: &PackageSpec,
    state: Option<&crate::receipt::InstallState>,
) -> Result<ComponentDetection> {
    match verify_package_receipt(resource_path, state, &spec.id)? {
        ReceiptVerification::Verified(receipt) => {
            let files = receipt
                .installed_files
                .iter()
                .map(|file| resource_path.join(&file.path))
                .collect();
            return Ok(ComponentDetection {
                package_id: spec.id.clone(),
                display_name: spec.display_name.clone(),
                installed: true,
                version: receipt.version,
                detector: "rais-receipt".to_string(),
                confidence: Confidence::High,
                files,
                notes: Vec::new(),
            });
        }
        ReceiptVerification::Mismatch(receipt) => {
            let files = matching_user_plugin_files(resource_path, platform, spec)?;
            if !files.is_empty() {
                return Ok(ComponentDetection {
                    package_id: spec.id.clone(),
                    display_name: spec.display_name.clone(),
                    installed: true,
                    version: receipt.version,
                    detector: "rais-receipt-mismatch".to_string(),
                    confidence: Confidence::Medium,
                    files,
                    notes: vec![
                        "RAIS has a receipt for this package, but installed files do not match it."
                            .to_string(),
                    ],
                });
            }
        }
        ReceiptVerification::MissingReceipt | ReceiptVerification::MissingPackage => {}
    }

    let files = matching_user_plugin_files(resource_path, platform, spec)?;
    if files.is_empty() {
        return Ok(ComponentDetection::not_installed(
            spec.id.clone(),
            spec.display_name.clone(),
        ));
    }

    if let Some((version, detector, confidence, notes)) =
        detect_version_from_files(resource_path, &files, &spec.id)?
    {
        return Ok(ComponentDetection {
            package_id: spec.id.clone(),
            display_name: spec.display_name.clone(),
            installed: true,
            version: Some(version),
            detector,
            confidence,
            files,
            notes,
        });
    }

    Ok(ComponentDetection {
        package_id: spec.id.clone(),
        display_name: spec.display_name.clone(),
        installed: true,
        version: None,
        detector: "userplugins-file-presence".to_string(),
        confidence: Confidence::Medium,
        files,
        notes: vec!["Package is present, but this RAIS version cannot reliably read its version without a RAIS receipt.".to_string()],
    })
}

fn detect_version_from_files(
    resource_path: &Path,
    files: &[PathBuf],
    package_id: &str,
) -> Result<Option<(crate::version::Version, String, Confidence, Vec<String>)>> {
    for file in files {
        if let Some(version) = file_version(file)? {
            return Ok(Some((
                version,
                "file-version-metadata".to_string(),
                Confidence::High,
                Vec::new(),
            )));
        }
    }

    for file in files {
        if let Some(owner) = package_owner_for_file(resource_path, file)? {
            return Ok(Some((
                owner.version,
                "reapack-registry".to_string(),
                Confidence::High,
                vec![format!(
                    "Version came from ReaPack registry entry {}/{}/{}.",
                    owner.remote, owner.category, owner.package
                )],
            )));
        }
    }

    if package_id == PACKAGE_OSARA {
        for file in files {
            if let Some(version) = osara_version_from_binary(file)? {
                return Ok(Some((
                    version,
                    "osara-binary-version-string".to_string(),
                    Confidence::Medium,
                    vec![
                        "Version came from a best-effort scan for OSARA's embedded version string."
                            .to_string(),
                    ],
                )));
            }
        }
    }

    Ok(None)
}

fn osara_version_from_binary(path: &Path) -> Result<Option<crate::version::Version>> {
    let bytes = fs::read(path).with_path(path)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(osara_version_from_text(&text))
}

fn osara_version_from_text(text: &str) -> Option<crate::version::Version> {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        if !bytes[start].is_ascii_digit() {
            continue;
        }

        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'.' | b'-'))
        {
            end += 1;
        }

        let candidate = &text[start..end];
        if !candidate.starts_with("20") || candidate.matches('.').count() < 2 {
            continue;
        }

        if let Ok(version) = crate::version::Version::parse(candidate) {
            return Some(version);
        }
    }

    None
}

fn matching_user_plugin_files(
    resource_path: &Path,
    _platform: Platform,
    spec: &PackageSpec,
) -> Result<Vec<PathBuf>> {
    let user_plugins = resource_path.join("UserPlugins");
    if !user_plugins.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(&user_plugins).with_path(&user_plugins)? {
        let entry = entry.with_path(&user_plugins)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower_name = file_name.to_ascii_lowercase();

        let prefix_matches = spec
            .user_plugin_prefixes
            .iter()
            .any(|prefix| lower_name.starts_with(&prefix.to_ascii_lowercase()));
        let suffix_matches = spec
            .user_plugin_suffixes
            .iter()
            .any(|suffix| lower_name.ends_with(&suffix.to_ascii_lowercase()));

        if prefix_matches && suffix_matches {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

fn discover_standard_installation(platform: Platform) -> Option<Installation> {
    match platform {
        Platform::Windows => discover_standard_windows(),
        Platform::MacOs => discover_standard_macos(),
    }
}

fn discover_portable_installation(platform: Platform, root: &Path) -> Option<Installation> {
    match platform {
        Platform::Windows => discover_portable_windows(root),
        Platform::MacOs => discover_portable_macos(root),
    }
}

fn discover_standard_windows() -> Option<Installation> {
    let resource_path = env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("REAPER"))?;

    let app_path = windows_reaper_app_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\REAPER\reaper.exe"));

    if !app_path.exists() && !resource_path.exists() {
        return None;
    }

    let mut evidence = Vec::new();
    if app_path.exists() {
        evidence.push(Evidence::new(
            "standard-windows-app-path",
            Some(app_path.clone()),
            "Found reaper.exe in a standard application directory.",
        ));
    }
    if resource_path.exists() {
        evidence.push(Evidence::new(
            "standard-windows-resource-path",
            Some(resource_path.clone()),
            "Found the standard REAPER resource path.",
        ));
    }

    let version = file_version(&app_path).ok().flatten();
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "standard-windows-file-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from executable metadata."),
        ));
    }

    Some(Installation {
        kind: InstallationKind::Standard,
        platform: Platform::Windows,
        app_path,
        resource_path: resource_path.clone(),
        version,
        architecture: Some(Architecture::current()),
        writable: is_probably_writable(&resource_path),
        confidence: if evidence.len() > 1 {
            Confidence::High
        } else {
            Confidence::Medium
        },
        evidence,
    })
}

fn windows_reaper_app_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("REAPER")
                .join("reaper.exe"),
        );
    }
    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(program_files_x86)
                .join("REAPER")
                .join("reaper.exe"),
        );
    }
    candidates
}

fn discover_portable_windows(root: &Path) -> Option<Installation> {
    let app_path = root.join("reaper.exe");
    let ini_path = root.join("reaper.ini");
    if !app_path.is_file() || !ini_path.is_file() {
        return None;
    }

    let version = file_version(&app_path).ok().flatten();
    let mut evidence = vec![
        Evidence::new(
            "portable-windows-app-path",
            Some(app_path.clone()),
            "Found reaper.exe in the selected portable folder.",
        ),
        Evidence::new(
            "portable-windows-reaper-ini",
            Some(ini_path),
            "Found reaper.ini in the selected portable folder.",
        ),
    ];
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "portable-windows-file-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from executable metadata."),
        ));
    }

    Some(Installation {
        kind: InstallationKind::Portable,
        platform: Platform::Windows,
        app_path: app_path.clone(),
        resource_path: root.to_path_buf(),
        version,
        architecture: Some(Architecture::current()),
        writable: is_probably_writable(root),
        confidence: Confidence::High,
        evidence,
    })
}

fn discover_standard_macos() -> Option<Installation> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let resource_path = home
        .join("Library")
        .join("Application Support")
        .join("REAPER");
    let app_path = [
        "/Applications/REAPER.app",
        "/Applications/REAPER64.app",
        "/Applications/REAPER-ARM.app",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.exists())
    .unwrap_or_else(|| PathBuf::from("/Applications/REAPER.app"));

    if !app_path.exists() && !resource_path.exists() {
        return None;
    }

    let mut evidence = Vec::new();
    if app_path.exists() {
        evidence.push(Evidence::new(
            "standard-macos-app-path",
            Some(app_path.clone()),
            "Found REAPER.app in /Applications.",
        ));
    }
    if resource_path.exists() {
        evidence.push(Evidence::new(
            "standard-macos-resource-path",
            Some(resource_path.clone()),
            "Found the standard REAPER resource path.",
        ));
    }

    let version = file_version(&app_path).ok().flatten();
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "standard-macos-app-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from app metadata."),
        ));
    }

    Some(Installation {
        kind: InstallationKind::Standard,
        platform: Platform::MacOs,
        app_path,
        resource_path: resource_path.clone(),
        version,
        architecture: Some(Architecture::current()),
        writable: is_probably_writable(&resource_path),
        confidence: if evidence.len() > 1 {
            Confidence::High
        } else {
            Confidence::Medium
        },
        evidence,
    })
}

fn discover_portable_macos(root: &Path) -> Option<Installation> {
    let app_path = fs::read_dir(root)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_ascii_lowercase().contains("reaper"))
        })?;

    let ini_path = root.join("reaper.ini");
    let mut evidence = vec![Evidence::new(
        "portable-macos-app-bundle",
        Some(app_path.clone()),
        "Found a REAPER app bundle in the selected portable folder.",
    )];
    let confidence = if ini_path.exists() {
        evidence.push(Evidence::new(
            "portable-macos-reaper-ini",
            Some(ini_path),
            "Found reaper.ini in the selected portable folder.",
        ));
        Confidence::High
    } else {
        Confidence::Medium
    };

    let version = file_version(&app_path).ok().flatten();
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "portable-macos-app-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from app metadata."),
        ));
    }

    Some(Installation {
        kind: InstallationKind::Portable,
        platform: Platform::MacOs,
        app_path,
        resource_path: root.to_path_buf(),
        version,
        architecture: Some(Architecture::current()),
        writable: is_probably_writable(root),
        confidence,
        evidence,
    })
}

fn is_probably_writable(path: &Path) -> bool {
    let existing_path = if path.exists() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    fs::metadata(existing_path)
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        DiscoveryOptions, detect_components, discover_installations, osara_version_from_text,
    };
    use crate::model::Platform;
    use crate::package::{PACKAGE_OSARA, PACKAGE_REAPACK, PACKAGE_SWS};

    #[test]
    fn detects_extensions_by_user_plugin_prefix() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(plugins.join("reaper_osara64.dll"), b"").unwrap();
        fs::write(plugins.join("reaper_sws-x64.dll"), b"").unwrap();
        fs::write(plugins.join("reaper_reapack-x64.dll"), b"").unwrap();

        let detections = detect_components(dir.path(), Platform::Windows).unwrap();
        let installed: Vec<_> = detections
            .iter()
            .filter(|detection| detection.installed)
            .map(|detection| detection.package_id.as_str())
            .collect();

        assert!(installed.contains(&PACKAGE_OSARA));
        assert!(installed.contains(&PACKAGE_SWS));
        assert!(installed.contains(&PACKAGE_REAPACK));
    }

    #[test]
    fn detects_windows_portable_installation_from_selected_folder() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("reaper.exe"), b"").unwrap();
        fs::write(dir.path().join("reaper.ini"), b"").unwrap();

        let installations = discover_installations(&DiscoveryOptions {
            include_standard: false,
            portable_roots: vec![dir.path().to_path_buf()],
        })
        .unwrap();

        if cfg!(target_os = "windows") {
            assert_eq!(installations.len(), 1);
        } else {
            assert!(installations.is_empty());
        }
    }

    #[test]
    fn parses_osara_snapshot_version_from_binary_text() {
        let version = osara_version_from_text("OSARA 2024.3.6.1332,13560ef7").unwrap();
        assert_eq!(version.raw(), "2024.3.6.1332");
    }

    #[test]
    fn detects_osara_version_by_binary_scan_when_metadata_is_unavailable() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(
            plugins.join("reaper_osara64.dll"),
            b"OSARA\0snapshot\0 2024.3.6.1332,13560ef7\0",
        )
        .unwrap();

        let detections = detect_components(dir.path(), Platform::Windows).unwrap();
        let osara = detections
            .iter()
            .find(|detection| detection.package_id == PACKAGE_OSARA)
            .unwrap();

        assert_eq!(osara.version.as_ref().unwrap().raw(), "2024.3.6.1332");
        assert_eq!(osara.detector, "osara-binary-version-string");
    }
}
