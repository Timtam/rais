use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

use crate::error::{IoPathContext, RaisError, Result};

const LOCK_FILE_NAME: &str = "package-install.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageInstallLockMetadata {
    pub pid: u32,
    pub started_at: String,
}

#[derive(Debug)]
pub struct PackageInstallLock {
    path: PathBuf,
    metadata: PackageInstallLockMetadata,
}

impl PackageInstallLock {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn metadata(&self) -> &PackageInstallLockMetadata {
        &self.metadata
    }
}

impl Drop for PackageInstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn default_package_install_lock_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        if let Some(local_app_data) = rais_platform::user_local_appdata_dir() {
            return local_app_data
                .join("RAIS")
                .join("locks")
                .join(LOCK_FILE_NAME);
        }
    }

    if cfg!(target_os = "macos") {
        if let Some(home) = rais_platform::user_home_dir() {
            return home
                .join("Library")
                .join("Caches")
                .join("RAIS")
                .join("locks")
                .join(LOCK_FILE_NAME);
        }
    }

    env::temp_dir().join("rais-locks").join(LOCK_FILE_NAME)
}

pub fn acquire_package_install_lock() -> Result<PackageInstallLock> {
    acquire_package_install_lock_at(&default_package_install_lock_path())
}

pub fn acquire_package_install_lock_at(path: &Path) -> Result<PackageInstallLock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_path(parent)?;
    }

    if path.exists() {
        match read_lock_holder(path) {
            Some(holder) if pid_is_alive(holder.pid) => {
                return Err(RaisError::PackageInstallInProgress {
                    lock_path: path.to_path_buf(),
                    pid: holder.pid,
                });
            }
            _ => {
                fs::remove_file(path).with_path(path)?;
            }
        }
    }

    let metadata = PackageInstallLockMetadata {
        pid: process::id(),
        started_at: lock_timestamp(),
    };
    let serialized = serde_json::to_string(&metadata).map_err(|source| RaisError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    let file = options.open(path).with_path(path)?;
    let mut writer = std::io::BufWriter::new(file);
    use std::io::Write;
    writer.write_all(serialized.as_bytes()).with_path(path)?;
    writer.flush().with_path(path)?;

    Ok(PackageInstallLock {
        path: path.to_path_buf(),
        metadata,
    })
}

pub fn package_install_lock_active() -> Result<Option<PackageInstallLockMetadata>> {
    package_install_lock_active_at(&default_package_install_lock_path())
}

pub fn package_install_lock_active_at(path: &Path) -> Result<Option<PackageInstallLockMetadata>> {
    if !path.exists() {
        return Ok(None);
    }
    match read_lock_holder(path) {
        Some(holder) if pid_is_alive(holder.pid) => Ok(Some(holder)),
        _ => Ok(None),
    }
}

fn read_lock_holder(path: &Path) -> Option<PackageInstallLockMetadata> {
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn pid_is_alive(pid: u32) -> bool {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, false);
    system.process(Pid::from(pid as usize)).is_some()
}

fn lock_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix-{seconds}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        PackageInstallLockMetadata, acquire_package_install_lock_at, package_install_lock_active_at,
    };
    use crate::error::RaisError;

    #[test]
    fn acquires_lock_and_releases_on_drop() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("install.lock");

        {
            let lock = acquire_package_install_lock_at(&path).unwrap();
            assert!(path.exists());
            assert_eq!(lock.metadata().pid, std::process::id());
        }
        assert!(!path.exists());
    }

    #[test]
    fn second_acquire_fails_when_holder_is_alive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("install.lock");

        let _first = acquire_package_install_lock_at(&path).unwrap();
        let error = acquire_package_install_lock_at(&path).unwrap_err();
        assert!(matches!(error, RaisError::PackageInstallInProgress { .. }));
    }

    #[test]
    fn stale_lock_with_dead_pid_is_replaced() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("install.lock");

        let stale = PackageInstallLockMetadata {
            pid: u32::MAX - 1,
            started_at: "unix-0".to_string(),
        };
        fs::write(&path, serde_json::to_string(&stale).unwrap()).unwrap();

        let lock = acquire_package_install_lock_at(&path).unwrap();
        assert_eq!(lock.metadata().pid, std::process::id());
    }

    #[test]
    fn package_install_lock_active_reports_other_holder() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("install.lock");

        assert!(package_install_lock_active_at(&path).unwrap().is_none());

        let _lock = acquire_package_install_lock_at(&path).unwrap();
        let holder = package_install_lock_active_at(&path).unwrap().unwrap();
        assert_eq!(holder.pid, std::process::id());
    }

    #[test]
    fn package_install_lock_active_ignores_stale_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("install.lock");

        let stale = PackageInstallLockMetadata {
            pid: u32::MAX - 1,
            started_at: "unix-0".to_string(),
        };
        fs::write(&path, serde_json::to_string(&stale).unwrap()).unwrap();

        assert!(package_install_lock_active_at(&path).unwrap().is_none());
    }
}
