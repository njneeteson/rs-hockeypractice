//! `auth.sqlite`: user accounts and sessions.
//!
//! Kept apart from the app's own data (`app.sqlite`, in `localapp-core`) so the
//! core crate never deals with users, and either file can be backed up or
//! reset on its own.

use anyhow::{Context, bail};
use chrono::Utc;
use localapp_core::str_enum;
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

str_enum! {
    /// Admins manage user accounts; users use the app.
    pub enum Role {
        User => ("user", "User"),
        Admin => ("admin", "Admin"),
    }
}

str_enum! {
    pub enum Theme {
        System => ("system", "Match my device"),
        Light => ("light", "Light"),
        Dark => ("dark", "Dark"),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub disabled: bool,
    pub must_change_password: bool,
    pub theme: Theme,
    pub created_at: String,
    pub last_login: Option<String>,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// Display name, falling back to the username.
    pub fn name(&self) -> &str {
        if self.display_name.trim().is_empty() {
            &self.username
        } else {
            &self.display_name
        }
    }
}

/// Fields an admin sets when creating or editing a user.
#[derive(Clone, Debug, Default)]
pub struct UserFields {
    pub username: String,
    pub display_name: String,
    pub role: Option<Role>,
    pub disabled: bool,
}

pub struct AuthDb {
    conn: Mutex<Connection>,
}

const V1: &str = "
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    disabled INTEGER NOT NULL DEFAULT 0,
    must_change_password INTEGER NOT NULL DEFAULT 0,
    theme TEXT NOT NULL DEFAULT 'system',
    created_at TEXT NOT NULL,
    last_login TEXT
);
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    persistent INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_seen INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
";

/// Ordered migrations; index + 1 is the resulting `user_version`. Append, never edit.
const MIGRATIONS: &[&str] = &[V1];

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let version: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}

const USER_COLS: &str = "u.id, u.username, u.display_name, u.role, u.disabled, \
    u.must_change_password, u.theme, u.created_at, u.last_login";
/// Number of columns in [`USER_COLS`]; extra selected columns start at this index.
const USER_COL_COUNT: usize = 9;

fn user_from_row(r: &Row<'_>) -> rusqlite::Result<User> {
    let role: String = r.get(3)?;
    let theme: String = r.get(6)?;
    Ok(User {
        id: r.get(0)?,
        username: r.get(1)?,
        display_name: r.get(2)?,
        role: Role::from_key(&role).unwrap_or(Role::User),
        disabled: r.get(4)?,
        must_change_password: r.get(5)?,
        theme: Theme::from_key(&theme).unwrap_or(Theme::System),
        created_at: r.get(7)?,
        last_login: r.get(8)?,
    })
}

fn now_text() -> String {
    Utc::now().format("%Y-%m-%d %H:%M UTC").to_string()
}

/// Usernames: 1–40 of letters, digits, `.`, `-`, `_`, `@`.
pub fn validate_username(name: &str) -> anyhow::Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@'));
    if !ok {
        bail!("Usernames use 1–40 letters, digits, dots, dashes, underscores or @.");
    }
    Ok(())
}

impl AuthDb {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> anyhow::Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn).context("upgrading auth tables")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

mod sessions;
mod users;

#[cfg(test)]
mod tests;
