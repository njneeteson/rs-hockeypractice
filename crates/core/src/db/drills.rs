//! The `drills` table: drill libraries, one per person and one per team.

use super::Database;
use crate::development::Split;
use crate::diagram::Diagram;
use crate::drill::{Category, Drill, DrillFields};
use crate::owner::{Library, MemberId, limits};
use crate::rink::IceSize;
use rusqlite::{OptionalExtension, Row, params};

const SELECT: &str = "SELECT id, name, ice, category, minutes, description, points, diagram,
    updated_at, owner_member, owner_team, copied_from, focus FROM drills";

/// Which drills to list.
#[derive(Clone, Debug, Default)]
pub struct DrillFilter {
    /// Words that must all appear in the name, description or key points.
    pub search: String,
    pub ice: Option<IceSize>,
    pub category: Option<Category>,
}

fn row(r: &Row<'_>) -> rusqlite::Result<Drill> {
    let bad = |i: usize, what: &str, v: String| {
        rusqlite::Error::FromSqlConversionFailure(
            i,
            rusqlite::types::Type::Text,
            format!("unknown {what} {v:?}").into(),
        )
    };
    let ice: String = r.get(2)?;
    let category: String = r.get(3)?;
    let diagram: String = r.get(7)?;
    let member: Option<i64> = r.get(9)?;
    let team: Option<i64> = r.get(10)?;
    Ok(Drill {
        id: r.get(0)?,
        name: r.get(1)?,
        ice: IceSize::from_key(&ice).ok_or_else(|| bad(2, "ice size", ice.clone()))?,
        category: Category::from_key(&category).unwrap_or(Category::Other),
        minutes: r.get(4)?,
        description: r.get(5)?,
        points: r.get(6)?,
        // A diagram that no longer parses shows as empty rather than breaking the page.
        diagram: Diagram::from_json(&diagram).unwrap_or_default(),
        equipment: Vec::new(),
        updated_at: r.get(8)?,
        // Drills from before owners existed belong to nobody (member 0, which
        // no account has) until `adopt_unowned` gives them to someone.
        library: match team {
            Some(t) => Library::Team(t),
            None => Library::Personal(MemberId(member.unwrap_or(0))),
        },
        copied_from: r.get(11)?,
        focus: r
            .get::<_, Option<String>>(12)?
            .and_then(|s| Split::from_key(&s)),
    })
}

/// The `owner_member` and `owner_team` columns for a library.
fn owner_cols(lib: Library) -> (Option<i64>, Option<i64>) {
    match lib {
        Library::Personal(m) => (Some(m.0), None),
        Library::Team(t) => (None, Some(t)),
    }
}

impl Database {
    /// The drills in one library, by name.
    pub fn drills(&self, filter: &DrillFilter, lib: Library) -> anyhow::Result<Vec<Drill>> {
        let (member, team) = owner_cols(lib);
        let mut drills = self.query_all(
            &format!(
                "{SELECT} WHERE owner_member IS ?1 AND owner_team IS ?2
                 ORDER BY name COLLATE NOCASE, id"
            ),
            params![member, team],
            row,
        )?;
        let words: Vec<String> = filter
            .search
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        drills.retain(|d| {
            let text = format!("{} {} {}", d.name, d.description, d.points).to_lowercase();
            filter.ice.is_none_or(|i| d.ice == i)
                && filter.category.is_none_or(|c| d.category == c)
                && words.iter().all(|w| text.contains(w.as_str()))
        });
        self.attach_equipment(&mut drills)?;
        Ok(drills)
    }

    /// How many drills a library holds.
    pub fn drill_count(&self, lib: Library) -> anyhow::Result<u32> {
        let (member, team) = owner_cols(lib);
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM drills WHERE owner_member IS ?1 AND owner_team IS ?2",
            params![member, team],
            |r| r.get(0),
        )?)
    }

    pub fn drill(&self, id: i64) -> anyhow::Result<Option<Drill>> {
        let d = self
            .conn
            .query_row(&format!("{SELECT} WHERE id = ?1"), [id], row)
            .optional()?;
        let Some(d) = d else { return Ok(None) };
        let mut v = vec![d];
        self.attach_equipment(&mut v)?;
        Ok(v.pop())
    }

    /// Whether `member` may see and change drills in `lib`: their own
    /// library, or a team they belong to.
    pub fn can_use_library(&self, member: MemberId, lib: Library) -> anyhow::Result<bool> {
        Ok(match lib {
            Library::Personal(m) => m == member,
            Library::Team(t) => self.member_role(t, member)?.is_some(),
        })
    }

    fn check_room(&self, lib: Library) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.drill_count(lib)? < limits::DRILLS_PER_LIBRARY,
            "A library holds at most {} drills. Delete some first.",
            limits::DRILLS_PER_LIBRARY
        );
        Ok(())
    }

    pub fn create_drill(&self, lib: Library, f: DrillFields) -> anyhow::Result<i64> {
        self.insert_drill(lib, f, None)
    }

    fn insert_drill(
        &self,
        lib: Library,
        f: DrillFields,
        copied_from: Option<i64>,
    ) -> anyhow::Result<i64> {
        let f = f.validate()?;
        self.check_room(lib)?;
        let (member, team) = owner_cols(lib);
        self.atomic(|| {
            self.conn.execute(
                "INSERT INTO drills (name, ice, category, minutes, description, points, diagram,
                owner_member, owner_team, copied_from, focus)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    f.name,
                    f.ice.key(),
                    f.category.key(),
                    f.minutes,
                    f.description,
                    f.points,
                    f.diagram.to_json(),
                    member,
                    team,
                    copied_from,
                    f.focus.map(|s| s.key())
                ],
            )?;
            let id = self.conn.last_insert_rowid();
            self.set_equipment(id, &f.equipment)?;
            Ok(id)
        })
    }

    /// Save a drill. Its ice size can't change while practices use it.
    pub fn update_drill(&self, id: i64, f: DrillFields) -> anyhow::Result<()> {
        let f = f.validate()?;
        let before = self
            .drill(id)?
            .ok_or_else(|| anyhow::anyhow!("That drill doesn't exist."))?;
        if before.ice != f.ice {
            let used = self.drill_usage(id)?;
            anyhow::ensure!(
                used == 0,
                "This drill is placed in {used} practice page(s) as {}. Duplicate it to make a {} version.",
                before.ice.label().to_lowercase(),
                f.ice.label().to_lowercase()
            );
        }
        self.atomic(|| {
            self.conn.execute(
            "UPDATE drills SET name = ?2, ice = ?3, category = ?4, minutes = ?5, description = ?6,
                points = ?7, diagram = ?8, focus = ?9, updated_at = datetime('now')
             WHERE id = ?1",
            params![
                id,
                f.name,
                f.ice.key(),
                f.category.key(),
                f.minutes,
                f.description,
                f.points,
                f.diagram.to_json(),
                f.focus.map(|s| s.key())
            ],
        )?;
            self.set_equipment(id, &f.equipment)?;
            Ok(())
        })
    }

    /// How many practice pages use the drill.
    pub fn drill_usage(&self, id: i64) -> anyhow::Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM stations WHERE drill_id = ?1",
            [id],
            |r| r.get(0),
        )?)
    }

    /// Delete a drill no practice uses.
    pub fn delete_drill(&self, id: i64) -> anyhow::Result<()> {
        let used = self.drill_usage(id)?;
        anyhow::ensure!(
            used == 0,
            "This drill is used on {used} practice page(s). Remove it from those first."
        );
        let n = self
            .conn
            .execute("DELETE FROM drills WHERE id = ?1", [id])?;
        anyhow::ensure!(n == 1, "That drill doesn't exist.");
        Ok(())
    }

    /// Copy a drill into a library (its own, or another); returns the new id.
    /// The copy is independent: changing either never changes the other. In
    /// the same library it's named "<name> (copy)".
    pub fn copy_drill(&self, id: i64, to: Library) -> anyhow::Result<i64> {
        let d = self
            .drill(id)?
            .ok_or_else(|| anyhow::anyhow!("That drill doesn't exist."))?;
        let mut f = d.fields();
        if d.library == to {
            f.name = format!("{} (copy)", d.name).chars().take(80).collect();
        }
        self.insert_drill(to, f, Some(id))
    }

    /// The newest copy of drill `source` in a team's library, if it has one.
    pub fn team_copy_of(&self, source: i64, team: i64) -> anyhow::Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM drills WHERE owner_team = ?1 AND copied_from = ?2
                 ORDER BY id DESC LIMIT 1",
                params![team, source],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Delete everything in a person's library (their account is going away).
    /// Personal drills are never placed in practices, so nothing breaks.
    pub fn delete_personal_drills(&self, member: MemberId) -> anyhow::Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM drills WHERE owner_member = ?1", [member.0])?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::diagram::Item;
    use crate::drill::Equipment;

    pub const ME: MemberId = MemberId(1);

    pub fn sample(name: &str, ice: IceSize) -> DrillFields {
        DrillFields {
            name: name.into(),
            ice,
            category: Category::Passing,
            minutes: 10,
            description: "Pass and follow.".into(),
            points: "Tape to tape\nHeads up".into(),
            diagram: Diagram {
                items: vec![Item::Cone { x: 20.0, y: 20.0 }],
            },
            equipment: vec![(Equipment::Pucks, 20), (Equipment::Cones, 4)],
            focus: Some(Split([60, 40, 0, 0, 0])),
        }
    }

    #[test]
    fn crud_and_filters() {
        let db = Database::in_memory().unwrap();
        let mine = Library::Personal(ME);
        let a = db
            .create_drill(mine, sample("Pass and follow", IceSize::HALF))
            .unwrap();
        let b = db
            .create_drill(mine, sample("Angling", IceSize::QUARTER))
            .unwrap();
        let d = db.drill(a).unwrap().unwrap();
        assert_eq!(d.equipment_qty(Equipment::Pucks), 20);
        assert_eq!(d.diagram.items.len(), 1);
        assert_eq!(d.library, mine);

        let all = db.drills(&DrillFilter::default(), mine).unwrap();
        assert_eq!(all.iter().map(|d| d.id).collect::<Vec<_>>(), vec![b, a]);
        let f = DrillFilter {
            ice: Some(IceSize::QUARTER),
            ..Default::default()
        };
        assert_eq!(db.drills(&f, mine).unwrap().len(), 1);
        let f = DrillFilter {
            search: "ANGLING pass".into(),
            ..Default::default()
        };
        let found = db.drills(&f, mine).unwrap();
        assert_eq!(found.iter().map(|d| d.id).collect::<Vec<_>>(), vec![b]);
        // Someone else's library is separate.
        let theirs = Library::Personal(MemberId(2));
        assert!(
            db.drills(&DrillFilter::default(), theirs)
                .unwrap()
                .is_empty()
        );

        let mut changed = sample("Pass and follow 2", IceSize::FULL);
        changed.equipment = vec![(Equipment::Nets, 2)];
        db.update_drill(a, changed).unwrap();
        let d = db.drill(a).unwrap().unwrap();
        assert_eq!(
            (d.ice, d.equipment.clone()),
            (IceSize::FULL, vec![(Equipment::Nets, 2)])
        );

        let c = db.copy_drill(a, mine).unwrap();
        assert_eq!(
            db.drill(c).unwrap().unwrap().name,
            "Pass and follow 2 (copy)"
        );
        db.delete_drill(c).unwrap();
        assert!(db.drill(c).unwrap().is_none());
    }

    #[test]
    fn copies_are_independent() {
        let db = Database::in_memory().unwrap();
        let mine = Library::Personal(ME);
        let theirs = Library::Personal(MemberId(2));
        let a = db
            .create_drill(mine, sample("Angling", IceSize::QUARTER))
            .unwrap();
        let b = db.copy_drill(a, theirs).unwrap();
        let copy = db.drill(b).unwrap().unwrap();
        assert_eq!(
            (copy.name.as_str(), copy.library, copy.copied_from),
            ("Angling", theirs, Some(a))
        );
        assert_eq!(copy.equipment, db.drill(a).unwrap().unwrap().equipment);
        // Editing the original leaves the copy alone, and the other way round.
        db.update_drill(a, sample("Angling, harder", IceSize::QUARTER))
            .unwrap();
        db.update_drill(b, sample("Angling, easier", IceSize::HALF))
            .unwrap();
        assert_eq!(db.drill(a).unwrap().unwrap().name, "Angling, harder");
        assert_eq!(db.drill(a).unwrap().unwrap().ice, IceSize::QUARTER);
        assert_eq!(db.drill(b).unwrap().unwrap().name, "Angling, easier");
        // Deleting the original keeps the copy.
        db.delete_drill(a).unwrap();
        assert!(db.drill(b).unwrap().is_some());
        assert_eq!(db.delete_personal_drills(MemberId(2)).unwrap(), 1);
    }
}
