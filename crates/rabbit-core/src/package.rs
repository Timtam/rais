use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Architecture, Platform};

pub const PACKAGE_REAPER: &str = "reaper";
pub const PACKAGE_OSARA: &str = "osara";
pub const PACKAGE_SWS: &str = "sws";
pub const PACKAGE_REAPACK: &str = "reapack";
pub const PACKAGE_REAKONTROL: &str = "reakontrol";
pub const PACKAGE_JAWS_SCRIPTS: &str = "jaws-scripts";

pub const BUILTIN_PACKAGE_MANIFEST_ID: &str = "builtin-packages.json";
const BUILTIN_PACKAGE_MANIFEST: &str = include_str!("../embedded/packages/builtin-packages.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSpec {
    pub id: String,
    pub display_name: String,
    pub display_name_key: String,
    pub display_description_key: String,
    pub package_kind: PackageKind,
    pub required: bool,
    pub recommended: bool,
    /// When `true`, the wizard must show a package-specific acknowledgement
    /// page and the CLI must require an explicit `--accept-<package>-notice`
    /// flag before RABBIT stages or launches the install of this package.
    /// Used today by ReaPack to surface its donation notice; defaults to
    /// `false` for everything else.
    pub requires_user_acknowledgement: bool,
    pub supported_platforms: Vec<SupportedPlatform>,
    pub supported_architectures: Vec<Architecture>,
    pub latest_version_provider: Option<LatestVersionProvider>,
    pub artifact_provider: Option<ArtifactProvider>,
    pub detectors: Vec<PackageDetector>,
    pub install_steps: Vec<InstallStep>,
    pub uninstall_steps: Vec<UninstallStep>,
    pub backup_policy: BackupPolicy,
    pub user_plugin_prefixes: Vec<String>,
    pub user_plugin_suffixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub schema_version: u32,
    pub packages: Vec<EmbeddedPackageSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedPackageSpec {
    pub id: String,
    pub display_name: String,
    pub display_name_key: String,
    pub display_description_key: String,
    #[serde(default)]
    pub package_kind: PackageKind,
    #[serde(default)]
    pub required: bool,
    pub recommended: bool,
    #[serde(default)]
    pub requires_user_acknowledgement: bool,
    #[serde(default = "all_supported_platforms")]
    pub supported_platforms: Vec<SupportedPlatform>,
    #[serde(default = "all_supported_architectures")]
    pub supported_architectures: Vec<Architecture>,
    pub latest_version_provider: Option<LatestVersionProvider>,
    pub artifact_provider: Option<ArtifactProvider>,
    #[serde(default)]
    pub detectors: Vec<PackageDetector>,
    #[serde(default)]
    pub install_steps: Vec<InstallStep>,
    #[serde(default)]
    pub uninstall_steps: Vec<UninstallStep>,
    #[serde(default)]
    pub backup_policy: BackupPolicy,
    pub user_plugin_prefixes: Vec<String>,
    pub user_plugin_suffixes: PlatformSuffixes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    ReaperApp,
    UserPluginBinary,
    Keymap,
    ReapackPackage,
    /// Drop-in script files (e.g. `.jss`/`.jsb`) that a screen reader loads
    /// from a known per-user directory. Platform-gated: a package of this
    /// kind only appears in the wizard when the relevant screen reader is
    /// detected on the host (e.g. JAWS-for-REAPER scripts on Windows).
    ScreenReaderScripts,
}

impl Default for PackageKind {
    fn default() -> Self {
        Self::UserPluginBinary
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportedPlatform {
    Windows,
    Macos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LatestVersionProvider {
    ReaperDownloadPage,
    OsaraUpdateJson,
    SwsHomePage,
    ReapackGithubRelease,
    ReakontrolGithubSnapshots,
    /// rejetto HFS file listing at `hoard.reaperaccessibility.com` for the
    /// JAWS-for-REAPER scripts; the highest-version `*.zip` in the folder
    /// wins.
    JawsForReaperScriptsHoard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactProvider {
    ReaperDownloadPage,
    OsaraSnapshots,
    SwsDownloadPage,
    ReapackGithubReleaseAssets,
    ReakontrolGithubSnapshots,
    /// HFS folder listing on `hoard.reaperaccessibility.com`: same listing
    /// the latest-version provider hits, but the artifact resolver also
    /// captures the file URL for download.
    JawsForReaperScriptsHoard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageDetector {
    RabbitReceipt,
    UserPluginFile,
    FileVersionMetadata,
    ReapackRegistry,
    OsaraBinaryVersionString,
    /// Detect a JAWS-for-REAPER scripts install by following the
    /// `Reaper_JawsScripts` Programs-and-Features uninstall key to the
    /// vendor-installed `Uninstall.exe` and reading its StringFileInfo
    /// "FileVersion" resource. Lets RABBIT report a version even for users
    /// who installed the scripts before RABBIT existed (no receipt yet).
    JawsScriptsUninstallExe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStep {
    RunUpstreamInstaller,
    CopyUserPluginBinary,
    CopyKeymap,
    InstallReapackPackage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallStep {
    RemoveUserPluginBinary,
    RemoveKeymap,
    RemoveReapackPackage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupPolicy {
    None,
    BackupOverwrittenFiles,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformSuffixes {
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub macos: Vec<String>,
}

pub fn builtin_package_specs(platform: Platform) -> Vec<PackageSpec> {
    embedded_package_manifest()
        .packages
        .iter()
        .filter(|package| package.supports_platform(platform))
        .map(|package| package.to_package_spec(platform))
        .collect()
}

pub fn default_desired_package_ids() -> Vec<String> {
    embedded_package_manifest()
        .packages
        .iter()
        .filter(|package| package.recommended)
        .map(|package| package.id.clone())
        .collect()
}

/// Host-side facts the wizard consults before showing platform-conditional
/// packages — currently just "is JAWS installed?", which gates the
/// JAWS-for-REAPER scripts package. Lives in `rabbit-core` so callers (CLI,
/// GUI) can share one detection path. Probed via [`detect_host_capabilities`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostCapabilities {
    pub jaws_installed: bool,
}

/// Snapshot the runtime host facts that gate optional packages.
pub fn detect_host_capabilities() -> HostCapabilities {
    HostCapabilities {
        jaws_installed: rabbit_platform::is_jaws_installed(),
    }
}

/// `true` when the host can meaningfully receive `spec`. Returns `false` for
/// packages whose `package_kind` requires a host facility that isn't present
/// — today only [`PackageKind::ScreenReaderScripts`] needs this filter, and
/// it's a JAWS-presence check.
pub fn host_supports_package(spec: &PackageSpec, host: &HostCapabilities) -> bool {
    match spec.package_kind {
        PackageKind::ScreenReaderScripts => host.jaws_installed,
        PackageKind::ReaperApp
        | PackageKind::UserPluginBinary
        | PackageKind::Keymap
        | PackageKind::ReapackPackage => true,
    }
}

pub fn embedded_package_manifest() -> PackageManifest {
    parse_package_manifest(BUILTIN_PACKAGE_MANIFEST)
        .expect("embedded package manifest should parse")
}

pub fn embedded_package_manifest_source() -> &'static str {
    BUILTIN_PACKAGE_MANIFEST
}

pub fn parse_package_manifest(source: &str) -> Result<PackageManifest, serde_json::Error> {
    serde_json::from_str(source)
}

pub fn package_specs_by_id(platform: Platform) -> BTreeMap<String, PackageSpec> {
    builtin_package_specs(platform)
        .into_iter()
        .map(|spec| (spec.id.clone(), spec))
        .collect()
}

impl EmbeddedPackageSpec {
    pub fn supports_platform(&self, platform: Platform) -> bool {
        self.supported_platforms
            .iter()
            .any(|supported| supported.matches_platform(platform))
    }

    fn to_package_spec(&self, platform: Platform) -> PackageSpec {
        PackageSpec {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            display_name_key: self.display_name_key.clone(),
            display_description_key: self.display_description_key.clone(),
            package_kind: self.package_kind,
            required: self.required,
            recommended: self.recommended,
            requires_user_acknowledgement: self.requires_user_acknowledgement,
            supported_platforms: self.supported_platforms.clone(),
            supported_architectures: self.supported_architectures.clone(),
            latest_version_provider: self.latest_version_provider,
            artifact_provider: self.artifact_provider,
            detectors: self.detectors.clone(),
            install_steps: self.install_steps.clone(),
            uninstall_steps: self.uninstall_steps.clone(),
            backup_policy: self.backup_policy,
            user_plugin_prefixes: self.user_plugin_prefixes.clone(),
            user_plugin_suffixes: self.user_plugin_suffixes.for_platform(platform),
        }
    }
}

impl PackageSpec {
    pub fn supports_platform(&self, platform: Platform) -> bool {
        self.supported_platforms
            .iter()
            .any(|supported| supported.matches_platform(platform))
    }

    pub fn supports_architecture(&self, architecture: Architecture) -> bool {
        self.supported_architectures.contains(&architecture)
            || self
                .supported_architectures
                .contains(&Architecture::Universal)
    }
}

impl SupportedPlatform {
    pub fn matches_platform(self, platform: Platform) -> bool {
        matches!(
            (self, platform),
            (Self::Windows, Platform::Windows) | (Self::Macos, Platform::MacOs)
        )
    }
}

impl PlatformSuffixes {
    fn for_platform(&self, platform: Platform) -> Vec<String> {
        match platform {
            Platform::Windows => self.windows.clone(),
            Platform::MacOs => self.macos.clone(),
        }
    }
}

fn all_supported_platforms() -> Vec<SupportedPlatform> {
    vec![SupportedPlatform::Windows, SupportedPlatform::Macos]
}

fn all_supported_architectures() -> Vec<Architecture> {
    vec![
        Architecture::X86,
        Architecture::X64,
        Architecture::Arm64,
        Architecture::Arm64Ec,
        Architecture::Universal,
    ]
}

#[cfg(test)]
mod tests {
    use crate::model::{Architecture, Platform};
    use crate::package::{
        ArtifactProvider, BackupPolicy, InstallStep, LatestVersionProvider, PACKAGE_JAWS_SCRIPTS,
        PACKAGE_OSARA, PACKAGE_REAKONTROL, PACKAGE_REAPACK, PACKAGE_REAPER, PACKAGE_SWS,
        PackageDetector, PackageKind, SupportedPlatform, builtin_package_specs,
        default_desired_package_ids, embedded_package_manifest, embedded_package_manifest_source,
        package_specs_by_id, parse_package_manifest,
    };

    #[test]
    fn parses_embedded_package_manifest() {
        let manifest = embedded_package_manifest();

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.packages.len(), 6);
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.id == PACKAGE_REAPER)
        );
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.id == PACKAGE_OSARA)
        );
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.id == PACKAGE_REAKONTROL)
        );
        let reakontrol = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_REAKONTROL)
            .unwrap();
        assert_eq!(reakontrol.package_kind, PackageKind::UserPluginBinary);
        assert_eq!(
            reakontrol.latest_version_provider,
            Some(LatestVersionProvider::ReakontrolGithubSnapshots)
        );
        assert_eq!(
            reakontrol.artifact_provider,
            Some(ArtifactProvider::ReakontrolGithubSnapshots)
        );
        assert_eq!(reakontrol.user_plugin_prefixes, vec!["reaper_kontrol"]);
        assert!(
            reakontrol
                .install_steps
                .contains(&InstallStep::CopyUserPluginBinary)
        );
        assert!(
            reakontrol
                .detectors
                .contains(&PackageDetector::UserPluginFile)
        );
        let reaper = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_REAPER)
            .unwrap();
        assert_eq!(reaper.package_kind, PackageKind::ReaperApp);
        assert_eq!(
            reaper.latest_version_provider,
            Some(LatestVersionProvider::ReaperDownloadPage)
        );
        assert_eq!(
            reaper.artifact_provider,
            Some(ArtifactProvider::ReaperDownloadPage)
        );
        assert_eq!(reaper.backup_policy, BackupPolicy::None);
        assert!(
            reaper
                .install_steps
                .contains(&InstallStep::RunUpstreamInstaller)
        );
        let osara = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_OSARA)
            .unwrap();
        assert_eq!(osara.package_kind, PackageKind::UserPluginBinary);
        assert_eq!(
            osara.latest_version_provider,
            Some(LatestVersionProvider::OsaraUpdateJson)
        );
        assert_eq!(
            osara.artifact_provider,
            Some(ArtifactProvider::OsaraSnapshots)
        );
        assert_eq!(osara.backup_policy, BackupPolicy::BackupOverwrittenFiles);
        assert!(osara.detectors.contains(&PackageDetector::UserPluginFile));
        assert!(
            osara
                .install_steps
                .contains(&InstallStep::CopyUserPluginBinary)
        );
        assert!(embedded_package_manifest_source().contains("\"packages\""));
        let jaws = manifest
            .packages
            .iter()
            .find(|package| package.id == PACKAGE_JAWS_SCRIPTS)
            .unwrap();
        assert_eq!(jaws.package_kind, PackageKind::ScreenReaderScripts);
        assert_eq!(jaws.supported_platforms, vec![SupportedPlatform::Windows]);
        assert!(jaws.recommended);
        assert_eq!(
            jaws.latest_version_provider,
            Some(LatestVersionProvider::JawsForReaperScriptsHoard)
        );
        assert_eq!(
            jaws.artifact_provider,
            Some(ArtifactProvider::JawsForReaperScriptsHoard)
        );
    }

    #[test]
    fn builds_platform_specific_package_specs_from_manifest() {
        let windows = package_specs_by_id(Platform::Windows);
        let macos = package_specs_by_id(Platform::MacOs);

        assert_eq!(
            windows[PACKAGE_REAPACK].user_plugin_suffixes,
            vec![".dll".to_string()]
        );
        assert_eq!(
            macos[PACKAGE_REAPACK].user_plugin_suffixes,
            vec![".dylib".to_string()]
        );
        assert_eq!(windows[PACKAGE_SWS].display_name, "SWS Extension");
        assert_eq!(
            windows[PACKAGE_SWS].package_kind,
            PackageKind::UserPluginBinary
        );
        assert!(windows[PACKAGE_SWS].supports_platform(Platform::Windows));
        assert!(windows[PACKAGE_SWS].supports_architecture(Architecture::X64));
    }

    #[test]
    fn default_desired_packages_are_recommended_manifest_packages() {
        assert_eq!(
            default_desired_package_ids(),
            vec![
                PACKAGE_OSARA.to_string(),
                PACKAGE_SWS.to_string(),
                PACKAGE_REAPACK.to_string(),
                PACKAGE_REAKONTROL.to_string(),
                PACKAGE_JAWS_SCRIPTS.to_string(),
            ]
        );
    }

    #[test]
    fn can_parse_manifest_fixtures_without_code_changes() {
        let manifest = parse_package_manifest(
            r#"{
                "schema_version": 1,
                "packages": [{
                    "id": "example",
                    "display_name": "Example",
                    "display_name_key": "package-example",
                    "display_description_key": "package-example-description",
                    "package_kind": "user_plugin_binary",
                    "required": false,
                    "recommended": false,
                    "supported_platforms": ["windows", "macos"],
                    "supported_architectures": ["x64", "universal"],
                    "latest_version_provider": "sws_home_page",
                    "artifact_provider": "sws_download_page",
                    "detectors": ["user_plugin_file"],
                    "install_steps": ["copy_user_plugin_binary"],
                    "uninstall_steps": ["remove_user_plugin_binary"],
                    "backup_policy": "backup_overwritten_files",
                    "user_plugin_prefixes": ["reaper_example"],
                    "user_plugin_suffixes": {
                        "windows": [".dll"],
                        "macos": [".dylib"]
                    }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(manifest.packages[0].id, "example");
        assert_eq!(
            manifest.packages[0].supported_platforms,
            vec![SupportedPlatform::Windows, SupportedPlatform::Macos]
        );
        assert!(builtin_package_specs(Platform::Windows).len() >= 3);
    }

    #[test]
    fn manifest_defaults_support_older_minimal_entries() {
        let manifest = parse_package_manifest(
            r#"{
                "schema_version": 1,
                "packages": [{
                    "id": "minimal",
                    "display_name": "Minimal",
                    "display_name_key": "package-minimal",
                    "display_description_key": "package-minimal-description",
                    "recommended": false,
                    "user_plugin_prefixes": ["reaper_minimal"],
                    "user_plugin_suffixes": {
                        "windows": [".dll"],
                        "macos": [".dylib"]
                    }
                }]
            }"#,
        )
        .unwrap();

        let package = &manifest.packages[0];
        assert_eq!(package.package_kind, PackageKind::UserPluginBinary);
        assert!(!package.required);
        assert_eq!(package.backup_policy, BackupPolicy::None);
        assert!(package.supports_platform(Platform::MacOs));
    }
}
