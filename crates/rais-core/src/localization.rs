use std::fs;
use std::path::{Path, PathBuf};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

use crate::error::{IoPathContext, RaisError, Result};

pub const DEFAULT_LOCALE: &str = "en-US";
pub const LOCALE_FILE_NAME: &str = "rais.ftl";

const DEFAULT_LOCALE_SOURCE: &str = include_str!("../../../locales/en-US/rais.ftl");
const EMBEDDED_LOCALES: &[&str] = &[DEFAULT_LOCALE];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedText {
    pub id: String,
    pub value: String,
    pub locale: String,
    pub fallback_used: bool,
    pub missing: bool,
    pub formatting_errors: Vec<String>,
}

pub struct Localizer {
    requested_locale: String,
    active_locale: String,
    fallback_used: bool,
    source_path: Option<PathBuf>,
    bundle: FluentBundle<FluentResource>,
}

impl Localizer {
    pub fn embedded(requested_locale: &str) -> Result<Self> {
        let requested_locale = parse_locale(requested_locale, None)?.to_string();
        let fallback_used = requested_locale != DEFAULT_LOCALE;
        let source =
            embedded_locale_source(DEFAULT_LOCALE).ok_or_else(|| RaisError::Localization {
                path: None,
                message: format!("embedded default locale {DEFAULT_LOCALE} is missing"),
            })?;
        build_bundle(
            requested_locale,
            DEFAULT_LOCALE.to_string(),
            fallback_used,
            None,
            source,
        )
    }

    pub fn from_locale_dir(locales_dir: &Path, requested_locale: &str) -> Result<Self> {
        let requested_locale = parse_locale(requested_locale, None)?.to_string();
        let requested_path = locale_file_path(locales_dir, &requested_locale);
        if requested_path.is_file() {
            let source = fs::read_to_string(&requested_path).with_path(&requested_path)?;
            return build_bundle(
                requested_locale.clone(),
                requested_locale,
                false,
                Some(requested_path),
                &source,
            );
        }

        let default_path = locale_file_path(locales_dir, DEFAULT_LOCALE);
        if default_path.is_file() {
            let source = fs::read_to_string(&default_path).with_path(&default_path)?;
            return build_bundle(
                requested_locale,
                DEFAULT_LOCALE.to_string(),
                true,
                Some(default_path),
                &source,
            );
        }

        Self::embedded(&requested_locale)
    }

    pub fn requested_locale(&self) -> &str {
        &self.requested_locale
    }

    pub fn active_locale(&self) -> &str {
        &self.active_locale
    }

    pub fn fallback_used(&self) -> bool {
        self.fallback_used
    }

    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    pub fn text(&self, id: &str) -> LocalizedText {
        self.format(id, &[])
    }

    pub fn format(&self, id: &str, arguments: &[(&str, &str)]) -> LocalizedText {
        let Some(message) = self.bundle.get_message(id) else {
            return self.missing_text(id);
        };

        let Some(pattern) = message.value() else {
            return self.missing_text(id);
        };

        let mut args = FluentArgs::new();
        for (name, value) in arguments {
            args.set(*name, *value);
        }

        let mut formatting_errors = Vec::new();
        let value = self
            .bundle
            .format_pattern(pattern, Some(&args), &mut formatting_errors)
            .into_owned();

        LocalizedText {
            id: id.to_string(),
            value,
            locale: self.active_locale.clone(),
            fallback_used: self.fallback_used,
            missing: false,
            formatting_errors: formatting_errors
                .into_iter()
                .map(|error| format!("{error:?}"))
                .collect(),
        }
    }

    fn missing_text(&self, id: &str) -> LocalizedText {
        LocalizedText {
            id: id.to_string(),
            value: id.to_string(),
            locale: self.active_locale.clone(),
            fallback_used: self.fallback_used,
            missing: true,
            formatting_errors: Vec::new(),
        }
    }
}

pub fn embedded_locales() -> &'static [&'static str] {
    EMBEDDED_LOCALES
}

pub fn embedded_locale_source(locale: &str) -> Option<&'static str> {
    match locale {
        DEFAULT_LOCALE => Some(DEFAULT_LOCALE_SOURCE),
        _ => None,
    }
}

pub fn available_locales(locales_dir: &Path) -> Result<Vec<String>> {
    let mut locales = Vec::new();
    if locales_dir.is_dir() {
        for entry in fs::read_dir(locales_dir).with_path(locales_dir)? {
            let entry = entry.with_path(locales_dir)?;
            let path = entry.path();
            if !path.join(LOCALE_FILE_NAME).is_file() {
                continue;
            }

            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if parse_locale(name, Some(&path)).is_ok() {
                locales.push(name.to_string());
            }
        }
    }

    if !locales.iter().any(|locale| locale == DEFAULT_LOCALE) {
        locales.push(DEFAULT_LOCALE.to_string());
    }

    locales.sort();
    locales.dedup();
    Ok(locales)
}

fn build_bundle(
    requested_locale: String,
    active_locale: String,
    fallback_used: bool,
    source_path: Option<PathBuf>,
    source: &str,
) -> Result<Localizer> {
    let language_id = parse_locale(&active_locale, source_path.as_deref())?;
    let resource = FluentResource::try_new(source.to_string()).map_err(|(_, errors)| {
        RaisError::Localization {
            path: source_path.clone(),
            message: format!("failed to parse Fluent resource: {errors:?}"),
        }
    })?;

    let mut bundle = FluentBundle::new(vec![language_id]);
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .map_err(|errors| RaisError::Localization {
            path: source_path.clone(),
            message: format!("failed to add Fluent resource: {errors:?}"),
        })?;

    Ok(Localizer {
        requested_locale,
        active_locale,
        fallback_used,
        source_path,
        bundle,
    })
}

fn locale_file_path(locales_dir: &Path, locale: &str) -> PathBuf {
    locales_dir.join(locale).join(LOCALE_FILE_NAME)
}

fn parse_locale(locale: &str, path: Option<&Path>) -> Result<LanguageIdentifier> {
    locale
        .parse::<LanguageIdentifier>()
        .map_err(|error| RaisError::Localization {
            path: path.map(Path::to_path_buf),
            message: format!("invalid locale {locale}: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{DEFAULT_LOCALE, LOCALE_FILE_NAME, Localizer, available_locales};
    use super::{embedded_locale_source, embedded_locales};

    #[test]
    fn loads_embedded_english_messages() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();

        let message = localizer.text("app-title");

        assert_eq!(message.value, "REAPER Accessibility Installation Software");
        assert_eq!(message.locale, DEFAULT_LOCALE);
        assert!(!message.fallback_used);
        assert!(!message.missing);
    }

    #[test]
    fn exposes_embedded_default_locale_source() {
        assert_eq!(embedded_locales(), &[DEFAULT_LOCALE]);
        assert!(
            embedded_locale_source(DEFAULT_LOCALE)
                .unwrap()
                .contains("app-title")
        );
    }

    #[test]
    fn formats_messages_with_arguments() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();

        let message = localizer.format("status-package-installed", &[("package", "ReaPack")]);

        assert_eq!(message.value, "ReaPack installed");
        assert!(message.formatting_errors.is_empty());
    }

    #[test]
    fn reports_missing_messages_without_returning_empty_text() {
        let localizer = Localizer::embedded(DEFAULT_LOCALE).unwrap();

        let message = localizer.text("missing-message-id");

        assert_eq!(message.value, "missing-message-id");
        assert!(message.missing);
    }

    #[test]
    fn falls_back_to_embedded_default_when_locale_directory_is_missing() {
        let dir = tempdir().unwrap();
        let missing_dir = dir.path().join("missing-locales");

        let localizer = Localizer::from_locale_dir(&missing_dir, DEFAULT_LOCALE).unwrap();

        assert_eq!(
            localizer.text("app-title").value,
            "REAPER Accessibility Installation Software"
        );
        assert_eq!(localizer.active_locale(), DEFAULT_LOCALE);
        assert!(localizer.source_path().is_none());
    }

    #[test]
    fn loads_requested_locale_from_directory() {
        let dir = tempdir().unwrap();
        let locale_dir = dir.path().join("de-DE");
        fs::create_dir_all(&locale_dir).unwrap();
        fs::write(
            locale_dir.join(LOCALE_FILE_NAME),
            "app-title = RAIS auf Deutsch\n",
        )
        .unwrap();

        let localizer = Localizer::from_locale_dir(dir.path(), "de-DE").unwrap();

        assert_eq!(localizer.requested_locale(), "de-DE");
        assert_eq!(localizer.active_locale(), "de-DE");
        assert!(!localizer.fallback_used());
        assert!(localizer.source_path().is_some());
        assert_eq!(localizer.text("app-title").value, "RAIS auf Deutsch");
    }

    #[test]
    fn falls_back_to_default_locale_when_requested_locale_is_missing() {
        let dir = tempdir().unwrap();
        let locale_dir = dir.path().join(DEFAULT_LOCALE);
        fs::create_dir_all(&locale_dir).unwrap();
        fs::write(
            locale_dir.join(LOCALE_FILE_NAME),
            "app-title = Default RAIS\n",
        )
        .unwrap();

        let localizer = Localizer::from_locale_dir(dir.path(), "fr-FR").unwrap();

        assert_eq!(localizer.requested_locale(), "fr-FR");
        assert_eq!(localizer.active_locale(), DEFAULT_LOCALE);
        assert!(localizer.fallback_used());
        assert_eq!(localizer.text("app-title").value, "Default RAIS");
    }

    #[test]
    fn lists_locale_directories_with_locale_files_plus_embedded_default() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("de-DE")).unwrap();
        fs::create_dir_all(dir.path().join("not_a_locale")).unwrap();
        fs::write(dir.path().join("de-DE").join(LOCALE_FILE_NAME), "").unwrap();
        fs::write(dir.path().join("not_a_locale").join(LOCALE_FILE_NAME), "").unwrap();

        let locales = available_locales(dir.path()).unwrap();

        assert_eq!(
            locales,
            vec!["de-DE".to_string(), DEFAULT_LOCALE.to_string()]
        );
    }
}
