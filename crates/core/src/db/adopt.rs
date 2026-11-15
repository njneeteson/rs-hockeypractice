//! Giving owners to data from before teams had members and drills had
//! libraries (schema V6). Not a table: a one-time move across several.

use super::Database;
use crate::owner::{Library, MemberId, TeamRole};
use rusqlite::params;

/// What [`Database::adopt_unowned`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Adopted {
    pub teams: usize,
    pub team_drills: usize,
    pub personal_drills: usize,
}

impl Adopted {
    pub fn is_empty(&self) -> bool {
        *self == Adopted::default()
    }
}

impl Database {
    /// Whether any team has no members or any drill has no library.
    pub fn has_unowned(&self) -> anyhow::Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM teams
                            WHERE id NOT IN (SELECT team_id FROM team_members))
                 OR EXISTS (SELECT 1 FROM drills
                            WHERE owner_member IS NULL AND owner_team IS NULL)",
            [],
            |r| r.get(0),
        )?)
    }

    /// Give everything without an owner to someone, as it was shared before:
    ///
    /// * a team without members gets everyone, with `owner` as its owner;
    /// * each drill a team's practices use is copied into that team's
    ///   library and its stations point at the copy;
    /// * every drill still without a library goes to `owner`'s own.
    ///
    /// Safe to run again: it only touches what has no owner.
    pub fn adopt_unowned(&self, owner: MemberId, everyone: &[MemberId]) -> anyhow::Result<Adopted> {
        self.atomic(|| {
            let mut done = Adopted::default();
            let teams: Vec<i64> = self.query_all(
                "SELECT id FROM teams WHERE id NOT IN (SELECT team_id FROM team_members)",
                [],
                |r| r.get(0),
            )?;
            for team in teams {
                for m in everyone.iter().chain([&owner]) {
                    let role = if *m == owner {
                        TeamRole::Owner
                    } else {
                        TeamRole::Coach
                    };
                    self.conn.execute(
                        "INSERT OR IGNORE INTO team_members (team_id, member_id, role)
                         VALUES (?1, ?2, ?3)",
                        params![team, m.0, role.key()],
                    )?;
                }
                done.teams += 1;
            }

            let used: Vec<(i64, i64)> = self.query_all(
                "SELECT DISTINCT p.team_id, st.drill_id FROM stations st
                 JOIN segments s ON s.id = st.segment_id
                 JOIN practices p ON p.id = s.practice_id
                 JOIN drills d ON d.id = st.drill_id
                 WHERE d.owner_member IS NULL AND d.owner_team IS NULL
                 ORDER BY p.team_id, st.drill_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            for (team, drill) in used {
                let copy = self.copy_drill(drill, Library::Team(team))?;
                self.conn.execute(
                    "UPDATE stations SET drill_id = ?3 WHERE drill_id = ?2 AND segment_id IN
                        (SELECT s.id FROM segments s JOIN practices p ON p.id = s.practice_id
                         WHERE p.team_id = ?1)",
                    params![team, drill, copy],
                )?;
                done.team_drills += 1;
            }

            done.personal_drills = self.conn.execute(
                "UPDATE drills SET owner_member = ?1
                 WHERE owner_member IS NULL AND owner_team IS NULL",
                [owner.0],
            )?;
            Ok(done)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DrillFilter;
    use rusqlite::Connection;

    /// A database as V5 left it: shared teams and drills, no owners.
    fn legacy() -> Database {
        let db = Database::in_memory().unwrap();
        db.conn
            .execute_batch(
                "INSERT INTO teams (id, name) VALUES (1, 'U11'), (2, 'U13');
                 INSERT INTO drills (id, name, ice, category) VALUES
                    (1, 'Shared', 'half-1', 'skating'),
                    (2, 'U13 only', 'half-1', 'skating'),
                    (3, 'Unused', 'half-1', 'skating');
                 INSERT INTO drill_equipment (drill_id, item, qty) VALUES (1, 'pucks', 20);
                 INSERT INTO practices (id, team_id, date, ice) VALUES
                    (1, 1, '2026-11-11', 'half'), (2, 2, '2026-11-12', 'half');
                 INSERT INTO segments (id, practice_id, position, minutes, layout) VALUES
                    (1, 1, 0, 10, 'half-h1'), (2, 2, 0, 10, 'half-h2');
                 INSERT INTO stations (segment_id, slot, drill_id) VALUES
                    (1, 0, 1), (2, 0, 1), (2, 1, 2);",
            )
            .unwrap();
        db
    }

    #[test]
    fn existing_data_gets_owners() {
        let db = legacy();
        assert!(db.has_unowned().unwrap());
        let (alice, bob) = (MemberId(1), MemberId(2));
        let done = db.adopt_unowned(alice, &[alice, bob]).unwrap();
        assert_eq!(
            done,
            Adopted {
                teams: 2,
                team_drills: 3,
                personal_drills: 3
            }
        );
        assert!(!db.has_unowned().unwrap());

        for team in [1, 2] {
            assert_eq!(db.member_role(team, alice).unwrap(), Some(TeamRole::Owner));
            assert_eq!(db.member_role(team, bob).unwrap(), Some(TeamRole::Coach));
        }
        // Each team has its own copy of what its practices use, and the
        // practices still show the same drills.
        let u11 = db
            .drills(&DrillFilter::default(), Library::Team(1))
            .unwrap();
        assert_eq!(
            u11.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["Shared"]
        );
        assert_eq!(u11[0].equipment.len(), 1, "copied with its equipment");
        let u13 = db
            .drills(&DrillFilter::default(), Library::Team(2))
            .unwrap();
        assert_eq!(u13.len(), 2);
        let plan = db.plan(2).unwrap().unwrap();
        let names: Vec<&str> = plan.drills.values().map(|d| d.name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(plan.drills.values().all(|d| d.library == Library::Team(2)));
        // The originals are Alice's.
        let mine = db
            .drills(&DrillFilter::default(), Library::Personal(alice))
            .unwrap();
        assert_eq!(mine.len(), 3);

        // Running it again changes nothing.
        assert!(db.adopt_unowned(alice, &[alice, bob]).unwrap().is_empty());
    }

    #[test]
    fn upgrades_a_v5_file() {
        // The migration keeps team ids, so practices still point at them.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", true).unwrap();
            let tx = conn.transaction().unwrap();
            for sql in &super::super::schema::tests::first(5) {
                tx.execute_batch(sql).unwrap();
            }
            tx.pragma_update(None, "user_version", 5).unwrap();
            tx.execute_batch(
                "INSERT INTO teams (id, name) VALUES (7, 'U11');
                 INSERT INTO practices (id, team_id, date) VALUES (1, 7, '2026-11-11');",
            )
            .unwrap();
            tx.commit().unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.practice(1).unwrap().unwrap().team_name, "U11");
        assert!(db.delete_team(7).is_err(), "still linked to its practice");
    }
}
