use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{IoPathContext, Result, SqlitePathContext};
use crate::version::Version;

pub const REAPACK_REGISTRY_RELATIVE_PATH: &str = "ReaPack/registry.db";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReapackEntry {
    pub remote: String,
    pub category: String,
    pub package: String,
    pub version: Version,
}

pub fn registry_path(resource_path: &Path) -> PathBuf {
    resource_path.join(REAPACK_REGISTRY_RELATIVE_PATH)
}

pub fn package_owner_for_file(
    resource_path: &Path,
    absolute_file: &Path,
) -> Result<Option<ReapackEntry>> {
    let db_path = registry_path(resource_path);
    if !db_path.is_file() {
        return Ok(None);
    }

    let Some(relative_path) = relative_reapack_path(resource_path, absolute_file) else {
        return Ok(None);
    };

    query_owner(&db_path, &relative_path)
}

pub fn list_entries(resource_path: &Path) -> Result<Vec<ReapackEntry>> {
    let db_path = registry_path(resource_path);
    if !db_path.is_file() {
        return Ok(Vec::new());
    }

    let connection = open_read_only(&db_path)?;
    let mut statement = connection
        .prepare("SELECT remote, category, package, version FROM entries ORDER BY remote, category, package")
        .with_sqlite_path(&db_path)?;

    let rows = statement
        .query_map([], |row| {
            let version_text: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                version_text,
            ))
        })
        .with_sqlite_path(&db_path)?;

    let mut entries = Vec::new();
    for row in rows {
        let (remote, category, package, version_text) = row.with_sqlite_path(&db_path)?;
        let Ok(version) = Version::parse(&version_text) else {
            continue;
        };
        entries.push(ReapackEntry {
            remote,
            category,
            package,
            version,
        });
    }

    Ok(entries)
}

fn query_owner(db_path: &Path, relative_path: &str) -> Result<Option<ReapackEntry>> {
    let connection = open_read_only(db_path)?;
    let sql = "SELECT e.remote, e.category, e.package, e.version \
               FROM entries e JOIN files f ON f.entry = e.id \
               WHERE f.path = ?1 LIMIT 1";
    let mut statement = connection.prepare(sql).with_sqlite_path(db_path)?;

    let entry = statement
        .query_row([relative_path], |row| {
            let version_text: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                version_text,
            ))
        })
        .optional()
        .with_sqlite_path(db_path)?;

    let Some((remote, category, package, version_text)) = entry else {
        return Ok(None);
    };
    let Ok(version) = Version::parse(&version_text) else {
        return Ok(None);
    };

    Ok(Some(ReapackEntry {
        remote,
        category,
        package,
        version,
    }))
}

fn open_read_only(db_path: &Path) -> Result<Connection> {
    Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).with_sqlite_path(db_path)
}

fn relative_reapack_path(resource_path: &Path, absolute_file: &Path) -> Option<String> {
    let canonical_resource = fs::canonicalize(resource_path)
        .with_path(resource_path)
        .ok()?;
    let canonical_file = fs::canonicalize(absolute_file)
        .with_path(absolute_file)
        .ok()?;
    let relative = canonical_file.strip_prefix(canonical_resource).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{list_entries, package_owner_for_file};

    #[test]
    fn reads_package_owner_for_registered_file() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        let reapack = dir.path().join("ReaPack");
        fs::create_dir_all(&plugins).unwrap();
        fs::create_dir_all(&reapack).unwrap();
        let sws_file = plugins.join("reaper_sws-x64.dll");
        fs::write(&sws_file, b"").unwrap();

        let connection = Connection::open(reapack.join("registry.db")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE entries (
                    id INTEGER PRIMARY KEY,
                    remote TEXT NOT NULL,
                    category TEXT NOT NULL,
                    package TEXT NOT NULL,
                    desc TEXT NOT NULL,
                    type INTEGER NOT NULL,
                    version TEXT NOT NULL,
                    author TEXT NOT NULL,
                    flags INTEGER DEFAULT 0,
                    UNIQUE(remote, category, package)
                );
                CREATE TABLE files (
                    id INTEGER PRIMARY KEY,
                    entry INTEGER NOT NULL,
                    path TEXT UNIQUE NOT NULL,
                    main INTEGER NOT NULL,
                    type INTEGER NOT NULL,
                    FOREIGN KEY(entry) REFERENCES entries(id)
                );
                INSERT INTO entries(id, remote, category, package, desc, type, version, author, flags)
                VALUES(1, 'ReaTeam Extensions', 'Extensions', 'SWS/S&M Extension', '', 0, '2.14.0.7', '', 0);
                INSERT INTO files(id, entry, path, main, type)
                VALUES(1, 1, 'UserPlugins/reaper_sws-x64.dll', 1, 0);",
            )
            .unwrap();

        let owner = package_owner_for_file(dir.path(), &sws_file)
            .unwrap()
            .unwrap();
        assert_eq!(owner.package, "SWS/S&M Extension");
        assert_eq!(owner.version.raw(), "2.14.0.7");

        let entries = list_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
    }
}
