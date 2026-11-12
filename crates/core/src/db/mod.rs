//! Local SQLite storage for the app's data.
//!
//! One file (`app.sqlite` in the data directory) holds everything. The schema
//! is versioned with `PRAGMA user_version` (see `schema.rs`). Give each table
//! its own submodule that adds methods to [`Database`], like `settings.rs`.

mod drill_equipment;
pub mod drills;
pub mod practices;
mod schema;
mod segments;
mod settings;
mod stations;
mod teams;

pub use drills::DrillFilter;
pub use practices::PracticeSummary;

use anyhow::Context;
use rusqlite::{Connection, Params, Row};
use std::path::{Path, PathBuf};

pub const DB_FILE: &str = "app.sqlite";

pub struct Database {
    conn: Connection,
    path: Option<PathBuf>,
}

impl Database {
    /// Open the database file, creating it (and its folder) if needed, and
    /// bring its schema up to date.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        // WAL lets readers proceed while a write is in progress.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn, Some(path.to_path_buf()))
    }

    /// A throwaway database, for tests.
    pub fn in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    fn init(mut conn: Connection, path: Option<PathBuf>) -> anyhow::Result<Self> {
        // A second writer waits instead of failing with SQLITE_BUSY.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        schema::migrate(&mut conn).context("upgrading database schema")?;
        Ok(Self { conn, path })
    }

    /// The database file (`None` for an in-memory database).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Write a consistent snapshot of the database to `dest` (a `.sqlite` file).
    pub fn backup_to(&self, dest: &Path) -> anyhow::Result<()> {
        if dest.exists() {
            std::fs::remove_file(dest).with_context(|| format!("replacing {}", dest.display()))?;
        }
        self.conn
            .execute("VACUUM INTO ?1", [dest.to_string_lossy()])
            .context("writing backup")?;
        Ok(())
    }

    /// Run a query and collect every row.
    fn query_all<T, P: Params>(
        &self,
        sql: &str,
        params: P,
        map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
    ) -> anyhow::Result<Vec<T>> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(params, map)?;
        Ok(rows.collect::<rusqlite::Result<Vec<T>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_reopen_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(DB_FILE);
        let db = Database::open(&path).unwrap();
        db.set_setting("greeting", "hello").unwrap();
        let backup = dir.path().join("backup.sqlite");
        db.backup_to(&backup).unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        assert_eq!(db.setting("greeting").unwrap().as_deref(), Some("hello"));
        let copy = Database::open(&backup).unwrap();
        assert_eq!(copy.setting("greeting").unwrap().as_deref(), Some("hello"));
    }
}
