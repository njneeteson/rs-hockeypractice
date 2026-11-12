//! The `drills` table: the shared drill library.

use super::Database;
use crate::diagram::Diagram;
use crate::drill::{Category, Drill, DrillFields};
use crate::rink::IceSize;
use rusqlite::{OptionalExtension, Row, params};

const SELECT: &str = "SELECT id, name, ice, category, minutes, description, points, diagram,
    updated_at FROM drills";

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
    })
}

impl Database {
    pub fn drills(&self, filter: &DrillFilter) -> anyhow::Result<Vec<Drill>> {
        let mut drills = self.query_all(
            &format!("{SELECT} ORDER BY name COLLATE NOCASE, id"),
            [],
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

    pub fn create_drill(&self, f: DrillFields) -> anyhow::Result<i64> {
        let f = f.validate()?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO drills (name, ice, category, minutes, description, points, diagram)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                f.name,
                f.ice.key(),
                f.category.key(),
                f.minutes,
                f.description,
                f.points,
                f.diagram.to_json()
            ],
        )?;
        let id = tx.last_insert_rowid();
        self.set_equipment(id, &f.equipment)?;
        tx.commit()?;
        Ok(id)
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
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE drills SET name = ?2, ice = ?3, category = ?4, minutes = ?5, description = ?6,
                points = ?7, diagram = ?8, updated_at = datetime('now')
             WHERE id = ?1",
            params![
                id,
                f.name,
                f.ice.key(),
                f.category.key(),
                f.minutes,
                f.description,
                f.points,
                f.diagram.to_json()
            ],
        )?;
        self.set_equipment(id, &f.equipment)?;
        tx.commit()?;
        Ok(())
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

    /// Copy a drill as "<name> (copy)"; returns the new id.
    pub fn duplicate_drill(&self, id: i64) -> anyhow::Result<i64> {
        let d = self
            .drill(id)?
            .ok_or_else(|| anyhow::anyhow!("That drill doesn't exist."))?;
        let mut f = d.fields();
        f.name = format!("{} (copy)", d.name).chars().take(80).collect();
        self.create_drill(f)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::diagram::Item;
    use crate::drill::Equipment;

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
        }
    }

    #[test]
    fn crud_and_filters() {
        let db = Database::in_memory().unwrap();
        let a = db
            .create_drill(sample("Pass and follow", IceSize::Half))
            .unwrap();
        let b = db
            .create_drill(sample("Angling", IceSize::Quarter))
            .unwrap();
        let d = db.drill(a).unwrap().unwrap();
        assert_eq!(d.equipment_qty(Equipment::Pucks), 20);
        assert_eq!(d.diagram.items.len(), 1);

        let all = db.drills(&DrillFilter::default()).unwrap();
        assert_eq!(all.iter().map(|d| d.id).collect::<Vec<_>>(), vec![b, a]);
        let f = DrillFilter {
            ice: Some(IceSize::Quarter),
            ..Default::default()
        };
        assert_eq!(db.drills(&f).unwrap().len(), 1);
        let f = DrillFilter {
            search: "ANGLING pass".into(),
            ..Default::default()
        };
        let found = db.drills(&f).unwrap();
        assert_eq!(found.iter().map(|d| d.id).collect::<Vec<_>>(), vec![b]);

        let mut changed = sample("Pass and follow 2", IceSize::Full);
        changed.equipment = vec![(Equipment::Nets, 2)];
        db.update_drill(a, changed).unwrap();
        let d = db.drill(a).unwrap().unwrap();
        assert_eq!(
            (d.ice, d.equipment.clone()),
            (IceSize::Full, vec![(Equipment::Nets, 2)])
        );

        let c = db.duplicate_drill(a).unwrap();
        assert_eq!(
            db.drill(c).unwrap().unwrap().name,
            "Pass and follow 2 (copy)"
        );
        db.delete_drill(c).unwrap();
        assert!(db.drill(c).unwrap().is_none());
    }
}
