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

/// Lanes and a split-off neutral zone: drill sizes and page layouts get new
/// keys. Old four-quarter pages listed the top quarters first; now each end is
/// listed top to bottom, so their second and third stations swap.
const V3: &str = r#"
UPDATE drills SET ice = 'half-2' WHERE ice = 'quarter';

UPDATE stations SET slot = slot + 100
 WHERE slot IN (1, 2)
   AND segment_id IN (SELECT id FROM segments WHERE layout = 'quarters');
UPDATE stations SET slot = CASE slot WHEN 101 THEN 2 ELSE 1 END
 WHERE slot IN (101, 102)
   AND segment_id IN (SELECT id FROM segments WHERE layout = 'quarters');

UPDATE segments SET layout = CASE layout
    WHEN 'full' THEN 'full-1'
    WHEN 'halves' THEN 'ends-h1-h1'
    WHEN 'zones' THEN 'ends-z1-z1'
    WHEN 'quarters' THEN 'ends-h2-h2'
    WHEN 'half_quarters' THEN 'ends-h1-h2'
    WHEN 'quarters_half' THEN 'ends-h2-h1'
    WHEN 'half' THEN 'half-h1'
    WHEN 'half_split' THEN 'half-h2'
    ELSE layout END;
"#;

/// Ordered migrations; index + 1 is the resulting schema version.
const MIGRATIONS: &[&str] = &[V1, V2, V3];

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

    #[test]
    fn v3_renames_sizes_and_layouts() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V1).unwrap();
        conn.execute_batch(V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(
            "INSERT INTO teams (id, name) VALUES (1, 'T');
             INSERT INTO drills (id, name, ice, category) VALUES (1, 'Q', 'quarter', 'other');
             INSERT INTO practices (id, team_id, date) VALUES (1, 1, '2026-11-11');
             INSERT INTO segments (id, practice_id, position, minutes, layout)
                VALUES (1, 1, 0, 10, 'quarters'), (2, 1, 1, 10, 'zones');
             INSERT INTO stations (segment_id, slot, drill_id)
                VALUES (1, 0, 1), (1, 1, 1), (1, 3, 1);",
        )
        .unwrap();
        conn.execute("UPDATE stations SET note = 'top right' WHERE slot = 1", [])
            .unwrap();
        migrate(&mut conn).unwrap();
        let ice: String = conn
            .query_row("SELECT ice FROM drills", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ice, "half-2");
        let layouts: Vec<String> = conn
            .prepare("SELECT layout FROM segments ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(layouts, vec!["ends-h2-h2", "ends-z1-z1"]);
        // The top-right quarter is now the third station (left top, left bottom, right top…).
        let slot: i64 = conn
            .query_row(
                "SELECT slot FROM stations WHERE note = 'top right'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(slot, 2);
    }
}
