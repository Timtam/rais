use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, RaisError>;

#[derive(Debug, Error)]
pub enum RaisError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON error at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("SQLite error at {path}: {source}")]
    Sqlite {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("HTTP error for {url}: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("remote data error for {url}: {message}")]
    RemoteData { url: String, message: String },

    #[error("invalid artifact URL {url}: {message}")]
    InvalidArtifactUrl { url: String, message: String },

    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("invalid backup id: {0}")]
    InvalidBackupId(String),

    #[error("backup not found at {0}")]
    BackupNotFound(PathBuf),

    #[error("no artifact found for {package_id} on {platform:?}/{architecture:?}")]
    NoArtifactFound {
        package_id: String,
        platform: crate::model::Platform,
        architecture: crate::model::Architecture,
    },

    #[error("artifact kind {kind:?} for {package_id} is not supported by this installer step")]
    UnsupportedArtifactKind {
        package_id: String,
        kind: crate::artifact::ArtifactKind,
    },

    #[error("preflight failed: {message}")]
    PreflightFailed { message: String },

    #[error("invalid version string: {0}")]
    InvalidVersion(String),

    #[error("localization error: {message}")]
    Localization {
        path: Option<PathBuf>,
        message: String,
    },

    #[error("unsupported platform")]
    UnsupportedPlatform,
}

pub trait IoPathContext<T> {
    fn with_path(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoPathContext<T> for std::io::Result<T> {
    fn with_path(self, path: impl Into<PathBuf>) -> Result<T> {
        let path = path.into();
        self.map_err(|source| RaisError::Io { path, source })
    }
}

pub trait JsonPathContext<T> {
    fn with_json_path(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> JsonPathContext<T> for serde_json::Result<T> {
    fn with_json_path(self, path: impl Into<PathBuf>) -> Result<T> {
        let path = path.into();
        self.map_err(|source| RaisError::Json { path, source })
    }
}

pub trait SqlitePathContext<T> {
    fn with_sqlite_path(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> SqlitePathContext<T> for rusqlite::Result<T> {
    fn with_sqlite_path(self, path: impl Into<PathBuf>) -> Result<T> {
        let path = path.into();
        self.map_err(|source| RaisError::Sqlite { path, source })
    }
}
