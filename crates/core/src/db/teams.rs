//! The `teams` table. A team's name goes in the footer of its practice plans;
//! its members plan practices together and share the team's drill library.

use super::Database;
use crate::owner::{MemberId, TeamRole, limits};
use crate::practice::Team;
use rusqlite::{OptionalExtension, Row, params};

const SELECT: &str = "SELECT t.id, t.name, t.notes,
    (SELECT COUNT(*) FROM practices p WHERE p.team_id = t.id),
    (SELECT COUNT(*) FROM team_members m WHERE m.team_id = t.id),
    (SELECT COUNT(*) FROM drills d WHERE d.owner_team = t.id)
    FROM teams t";

fn row(r: &Row<'_>) -> rusqlite::Result<Team> {
    Ok(Team {
        id: r.get(0)?,
        name: r.get(1)?,
        notes: r.get(2)?,
        practices: r.get(3)?,
        members: r.get(4)?,
        drills: r.get(5)?,
    })
}

fn clean(name: &str, notes: &str) -> anyhow::Result<(String, String)> {
    let name = name.trim();
    anyhow::ensure!(!name.is_empty(), "The team needs a name.");
    anyhow::ensure!(
        name.chars().count() <= 80,
        "Team names are at most 80 characters."
    );
    anyhow::ensure!(
        notes.trim().chars().count() <= 500,
        "Team notes are at most 500 characters."
    );
    Ok((name.to_string(), notes.trim().to_string()))
}

impl Database {
    /// The teams a member belongs to, by name, with their role in each.
    pub fn teams_for(&self, member: MemberId) -> anyhow::Result<Vec<(Team, TeamRole)>> {
        let sql = SELECT.replacen(
            " FROM teams t",
            ", tm.role FROM teams t JOIN team_members tm ON tm.team_id = t.id",
            1,
        );
        self.query_all(
            &format!("{sql} WHERE tm.member_id = ?1 ORDER BY t.name COLLATE NOCASE, t.id"),
            [member.0],
            |r| {
                let role: String = r.get(6)?;
                Ok((
                    row(r)?,
                    TeamRole::from_key(&role).unwrap_or(TeamRole::Coach),
                ))
            },
        )
    }

    pub fn team(&self, id: i64) -> anyhow::Result<Option<Team>> {
        Ok(self
            .conn
            .query_row(&format!("{SELECT} WHERE t.id = ?1"), [id], row)
            .optional()?)
    }

    /// Whether `member` already belongs to a team called `name` (other than
    /// `except`): a person's teams need different names to tell them apart.
    fn name_taken(&self, member: MemberId, name: &str, except: i64) -> anyhow::Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM teams t JOIN team_members m ON m.team_id = t.id
                WHERE m.member_id = ?1 AND t.name = ?2 COLLATE NOCASE AND t.id != ?3)",
            params![member.0, name, except],
            |r| r.get(0),
        )?)
    }

    /// Create a team with `owner` as its first member.
    pub fn create_team(&self, owner: MemberId, name: &str, notes: &str) -> anyhow::Result<i64> {
        let (name, notes) = clean(name, notes)?;
        anyhow::ensure!(
            !self.name_taken(owner, &name, 0)?,
            "You already have a team called “{name}”."
        );
        anyhow::ensure!(
            self.teams_owned(owner)? < limits::TEAMS_OWNED,
            "You can own at most {} teams.",
            limits::TEAMS_OWNED
        );
        self.atomic(|| {
            self.conn.execute(
                "INSERT INTO teams (name, notes) VALUES (?1, ?2)",
                params![name, notes],
            )?;
            let id = self.conn.last_insert_rowid();
            self.conn.execute(
                "INSERT INTO team_members (team_id, member_id, role) VALUES (?1, ?2, ?3)",
                params![id, owner.0, TeamRole::Owner.key()],
            )?;
            Ok(id)
        })
    }

    /// Rename a team, or change its notes, as `by`.
    pub fn update_team(
        &self,
        id: i64,
        by: MemberId,
        name: &str,
        notes: &str,
    ) -> anyhow::Result<()> {
        let (name, notes) = clean(name, notes)?;
        anyhow::ensure!(
            !self.name_taken(by, &name, id)?,
            "You already have a team called “{name}”."
        );
        let n = self.conn.execute(
            "UPDATE teams SET name = ?2, notes = ?3 WHERE id = ?1",
            params![id, name, notes],
        )?;
        anyhow::ensure!(n == 1, "That team doesn't exist.");
        Ok(())
    }

    /// Delete a team that has no practices, with its drill library.
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
    use crate::db::drills::tests::ME;

    #[test]
    fn create_rename_delete() {
        let db = Database::in_memory().unwrap();
        let id = db.create_team(ME, " U11 A ", "").unwrap();
        assert!(
            db.create_team(ME, "u11 a", "").is_err(),
            "one person's teams have different names, ignoring case"
        );
        let other = MemberId(2);
        db.create_team(other, "U11 A", "")
            .expect("someone else may use the same name");
        db.update_team(id, ME, "U11 AA", "Tuesday ice").unwrap();
        let t = db.team(id).unwrap().unwrap();
        assert_eq!(
            (t.name.as_str(), t.notes.as_str(), t.practices, t.members),
            ("U11 AA", "Tuesday ice", 0, 1)
        );
        let mine = db.teams_for(ME).unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].1, TeamRole::Owner);
        db.delete_team(id).unwrap();
        assert!(db.team(id).unwrap().is_none());
        assert!(db.teams_for(ME).unwrap().is_empty());
        assert!(db.create_team(ME, "  ", "").is_err());
    }
}
