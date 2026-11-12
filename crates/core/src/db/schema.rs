//! Schema creation and migrations, tracked with `PRAGMA user_version`.
//!
//! To change the schema, append a new migration to [`MIGRATIONS`]. Never edit
//! one that has shipped: existing databases have already run it.

use rusqlite::Connection;

const V1: &str = r#"
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Teams, the drill library and practice plans.
const V2: &str = r#"
CREATE TABLE teams (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    notes      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE drills (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    ice         TEXT NOT NULL,
    category    TEXT NOT NULL,
    minutes     INTEGER NOT NULL DEFAULT 10,
    description TEXT NOT NULL DEFAULT '',
    points      TEXT NOT NULL DEFAULT '',
    diagram     TEXT NOT NULL DEFAULT '{"items":[]}',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE drill_equipment (
    drill_id INTEGER NOT NULL REFERENCES drills(id) ON DELETE CASCADE,
    item     TEXT NOT NULL,
    qty      INTEGER NOT NULL CHECK (qty > 0),
    PRIMARY KEY (drill_id, item)
);

CREATE TABLE practices (
    id         INTEGER PRIMARY KEY,
    team_id    INTEGER NOT NULL REFERENCES teams(id) ON DELETE RESTRICT,
    title      TEXT NOT NULL DEFAULT '',
    date       TEXT NOT NULL,
    start_time TEXT NOT NULL DEFAULT '',
    ice        TEXT NOT NULL DEFAULT 'full',
    notes      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX practices_date ON practices(date);

CREATE TABLE segments (
    id          INTEGER PRIMARY KEY,
    practice_id INTEGER NOT NULL REFERENCES practices(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    title       TEXT NOT NULL DEFAULT '',
    minutes     INTEGER NOT NULL,
    layout      TEXT NOT NULL,
    rotation    INTEGER,
    notes       TEXT NOT NULL DEFAULT ''
);
CREATE INDEX segments_practice ON segments(practice_id, position);

CREATE TABLE stations (
    segment_id INTEGER NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
    slot       INTEGER NOT NULL,
    drill_id   INTEGER NOT NULL REFERENCES drills(id) ON DELETE RESTRICT,
    note       TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (segment_id, slot)
);
CREATE INDEX stations_drill ON stations(drill_id);
"#;

/// Ordered migrations; index + 1 is the resulting schema version.
const MIGRATIONS: &[&str] = &[V1, V2];

pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", true)?;
    let version: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let version: usize = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len());
    }
}
