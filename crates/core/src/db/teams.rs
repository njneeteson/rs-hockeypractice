//! The `teams` table. A team's name goes in the footer of its practice plans.

use super::Database;
use crate::practice::Team;
use rusqlite::{OptionalExtension, Row, params};

const SELECT: &str = "SELECT t.id, t.name, t.notes,
    (SELECT COUNT(*) FROM practices p WHERE p.team_id = t.id) FROM teams t";

fn row(r: &Row<'_>) -> rusqlite::Result<Team> {
    Ok(Team {
        id: r.get(0)?,
        name: r.get(1)?,
        notes: r.get(2)?,
        practices: r.get(3)?,
    })
}

fn clean(name: &str, notes: &str) -> anyhow::Result<(String, String)> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "The team needs a name.");
    anyhow::ensure!(
        name.chars().count() <= 80,
        "Team names are at most 80 characters."
    );
    Ok((name.to_string(), notes.trim().to_string()))
}

fn unique(e: rusqlite::Error, name: &str) -> anyhow::Error {
    if let rusqlite::Error::SqliteFailure(f, _) = &e
        && f.code == rusqlite::ErrorCode::ConstraintViolation
    {
        return anyhow::anyhow!("There's already a team called “{name}”.");
    }
    e.into()
}

impl Database {
    pub fn teams(&self) -> anyhow::Result<Vec<Team>> {
        self.query_all(&format!("{SELECT} ORDER BY t.name"), [], row)
    }

    pub fn team(&self, id: i64) -> anyhow::Result<Option<Team>> {
        Ok(self
            .conn
            .query_row(&format!("{SELECT} WHERE t.id = ?1"), [id], row)
            .optional()?)
    }

    pub fn create_team(&self, name: &str, notes: &str) -> anyhow::Result<i64> {
        let (name, notes) = clean(name, notes)?;
        self.conn
            .execute(
                "INSERT INTO teams (name, notes) VALUES (?1, ?2)",
                params![name, notes],
            )
            .map_err(|e| unique(e, &name))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_team(&self, id: i64, name: &str, notes: &str) -> anyhow::Result<()> {
        let (name, notes) = clean(name, notes)?;
        let n = self
            .conn
            .execute(
                "UPDATE teams SET name = ?2, notes = ?3 WHERE id = ?1",
                params![id, name, notes],
            )
            .map_err(|e| unique(e, &name))?;
        anyhow::ensure!(n == 1, "That team doesn't exist.");
        Ok(())
    }

    /// Delete a team that has no practices.
    pub fn delete_team(&self, id: i64) -> anyhow::Result<()> {
        let team = self
            .team(id)?
            .ok_or_else(|| anyhow::anyhow!("That team doesn't exist."))?;
        anyhow::ensure!(
            team.practices == 0,
            "{} has {} practice(s); delete those first.",
            team.name,
            team.practices
        );
        self.conn.execute("DELETE FROM teams WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rename_delete() {
        let db = Database::in_memory().unwrap();
        let id = db.create_team(" U11 A ", "").unwrap();
        assert!(
            db.create_team("u11 a", "").is_err(),
            "names are unique, ignoring case"
        );
        db.update_team(id, "U11 AA", "Tuesday ice").unwrap();
        let t = db.team(id).unwrap().unwrap();
        assert_eq!(
            (t.name.as_str(), t.notes.as_str(), t.practices),
            ("U11 AA", "Tuesday ice", 0)
        );
        assert_eq!(db.teams().unwrap().len(), 1);
        db.delete_team(id).unwrap();
        assert!(db.team(id).unwrap().is_none());
        assert!(db.create_team("  ", "").is_err());
    }
}
