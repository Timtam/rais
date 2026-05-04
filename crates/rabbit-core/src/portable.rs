use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{IoPathContext, Result};
use crate::localization::{
    DEFAULT_LOCALE, LOCALE_FILE_NAME, Localizer, embedded_locale_source, embedded_locales,
};
use crate::package::{
    BUILTIN_PACKAGE_MANIFEST_ID, embedded_package_manifest, embedded_package_manifest_source,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortabilityReport {
    pub passed: bool,
    pub current_exe: Option<PathBuf>,
    pub current_dir: PathBuf,
    pub locales_dir: PathBuf,
    pub locales_dir_present: bool,
    pub embedded_resources: Vec<EmbeddedResource>,
    pub required_external_resources: Vec<String>,
    pub checks: Vec<PortabilityCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedResource {
    pub kind: String,
    pub id: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortabilityCheck {
    pub name: String,
    pub status: PortabilityCheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PortabilityCheckStatus {
    Passed,
    Warning,
    Failed,
}

pub fn check_portable_runtime(locales_dir: &Path) -> Result<PortabilityReport> {
    let current_exe = env::current_exe().ok();
    let current_dir = env::current_dir().with_path(".")?;
    let locales_dir_present = locales_dir.is_dir();
    let embedded_resources = embedded_resources();
    let required_external_resources = Vec::new();
    let mut checks = Vec::new();

    checks.push(check_current_exe(&current_exe));
    checks.push(check_embedded_default_locale()?);
    checks.push(check_embedded_package_manifest());
    checks.push(check_locale_directory_optional(
        locales_dir,
        locales_dir_present,
    )?);
    checks.push(PortabilityCheck {
        name: "required-external-resources".to_string(),
        status: PortabilityCheckStatus::Passed,
        message: "No external resource files are required for startup.".to_string(),
    });

    let passed = checks
        .iter()
        .all(|check| check.status != PortabilityCheckStatus::Failed);

    Ok(PortabilityReport {
        passed,
        current_exe,
        current_dir,
        locales_dir: locales_dir.to_path_buf(),
        locales_dir_present,
        embedded_resources,
        required_external_resources,
        checks,
    })
}

fn embedded_resources() -> Vec<EmbeddedResource> {
    embedded_locales()
        .iter()
        .filter_map(|locale| {
            embedded_locale_source(locale).map(|source| EmbeddedResource {
                kind: "locale".to_string(),
                id: format!("{locale}/{LOCALE_FILE_NAME}"),
                bytes: source.len(),
            })
        })
        .chain(std::iter::once(EmbeddedResource {
            kind: "package-manifest".to_string(),
            id: BUILTIN_PACKAGE_MANIFEST_ID.to_string(),
            bytes: embedded_package_manifest_source().len(),
        }))
        .collect()
}

fn check_current_exe(current_exe: &Option<PathBuf>) -> PortabilityCheck {
    match current_exe {
        Some(path) => PortabilityCheck {
            name: "current-executable".to_string(),
            status: PortabilityCheckStatus::Passed,
            message: format!("Running executable: {}", path.display()),
        },
        None => PortabilityCheck {
            name: "current-executable".to_string(),
            status: PortabilityCheckStatus::Warning,
            message: "The current executable path could not be resolved.".to_string(),
        },
    }
}

fn check_embedded_default_locale() -> Result<PortabilityCheck> {
    let localizer = Localizer::embedded(DEFAULT_LOCALE)?;
    let title = localizer.text("app-title");
    let status = if title.missing || title.value.is_empty() {
        PortabilityCheckStatus::Failed
    } else {
        PortabilityCheckStatus::Passed
    };

    Ok(PortabilityCheck {
        name: "embedded-default-locale".to_string(),
        status,
        message: if status == PortabilityCheckStatus::Passed {
            format!("Embedded {DEFAULT_LOCALE} localization is available.")
        } else {
            format!("Embedded {DEFAULT_LOCALE} localization is missing app-title.")
        },
    })
}

fn check_embedded_package_manifest() -> PortabilityCheck {
    let manifest = embedded_package_manifest();
    let status = if manifest.packages.is_empty() {
        PortabilityCheckStatus::Failed
    } else {
        PortabilityCheckStatus::Passed
    };

    PortabilityCheck {
        name: "embedded-package-manifest".to_string(),
        status,
        message: if status == PortabilityCheckStatus::Passed {
            format!(
                "Embedded package manifest contains {} packages.",
                manifest.packages.len()
            )
        } else {
            "Embedded package manifest contains no packages.".to_string()
        },
    }
}

fn check_locale_directory_optional(
    locales_dir: &Path,
    locales_dir_present: bool,
) -> Result<PortabilityCheck> {
    let localizer = Localizer::from_locale_dir(locales_dir, DEFAULT_LOCALE)?;
    if !locales_dir_present && localizer.source_path().is_none() {
        return Ok(PortabilityCheck {
            name: "external-locales-optional".to_string(),
            status: PortabilityCheckStatus::Passed,
            message: format!(
                "{} is absent; RABBIT falls back to embedded localization.",
                locales_dir.display()
            ),
        });
    }

    if locales_dir_present {
        return Ok(PortabilityCheck {
            name: "external-locales-optional".to_string(),
            status: PortabilityCheckStatus::Warning,
            message: format!(
                "{} exists and may override embedded localization, but it is not required.",
                locales_dir.display()
            ),
        });
    }

    Ok(PortabilityCheck {
        name: "external-locales-optional".to_string(),
        status: PortabilityCheckStatus::Failed,
        message: "Embedded localization fallback did not activate.".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{PortabilityCheckStatus, check_portable_runtime};

    #[test]
    fn passes_without_external_locale_directory() {
        let dir = tempdir().unwrap();
        let report = check_portable_runtime(&dir.path().join("missing-locales")).unwrap();

        assert!(report.passed);
        assert!(!report.locales_dir_present);
        assert!(report.required_external_resources.is_empty());
        assert!(
            report
                .embedded_resources
                .iter()
                .any(|resource| resource.id == "en-US/rabbit.ftl")
        );
        assert!(
            report
                .embedded_resources
                .iter()
                .any(|resource| resource.id == "builtin-packages.json")
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "embedded-package-manifest"
                    && check.status == PortabilityCheckStatus::Passed)
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "external-locales-optional"
                    && check.status == PortabilityCheckStatus::Passed)
        );
    }

    #[test]
    fn treats_external_locale_directory_as_optional_warning() {
        let dir = tempdir().unwrap();
        let report = check_portable_runtime(dir.path()).unwrap();

        assert!(report.passed);
        assert!(report.locales_dir_present);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "external-locales-optional"
                    && check.status == PortabilityCheckStatus::Warning)
        );
    }
}
