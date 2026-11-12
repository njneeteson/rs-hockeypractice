//! The `segments` table: the timed parts of a practice, one page each.

use super::Database;
use crate::practice::{GoalieWarmup, Segment, SegmentFields};
use crate::rink::Layout;
use rusqlite::{OptionalExtension, Row, params};

const SELECT: &str = "SELECT id, practice_id, position, title, minutes, layout, rotation, notes,
    goalie_x, goalie_y, goalie_rot FROM segments";

fn row(r: &Row<'_>) -> rusqlite::Result<Segment> {
    let layout: String = r.get(5)?;
    let goalies = match (r.get(8)?, r.get(9)?, r.get(10)?) {
        (Some(x), Some(y), Some(rot)) => Some(GoalieWarmup { x, y, rot }),
        _ => None,
    };
    Ok(Segment {
        id: r.get(0)?,
        practice_id: r.get(1)?,
        position: r.get(2)?,
        title: r.get(3)?,
        minutes: r.get(4)?,
        layout: Layout::from_key(&layout).unwrap_or(Layout::Full(1)),
        rotation: r.get(6)?,
        notes: r.get(7)?,
        stations: Vec::new(),
        goalies,
    })
}

impl Database {
    /// A practice's segments in order, with their stations.
    pub fn segments(&self, practice_id: i64) -> anyhow::Result<Vec<Segment>> {
        let mut segs = self.query_all(
            &format!("{SELECT} WHERE practice_id = ?1 ORDER BY position, id"),
            [practice_id],
            row,
        )?;
        for s in &mut segs {
            s.stations = self.stations(s.id)?;
        }
        Ok(segs)
    }

    pub fn segment(&self, id: i64) -> anyhow::Result<Option<Segment>> {
        let seg = self
            .conn
            .query_row(&format!("{SELECT} WHERE id = ?1"), [id], row)
            .optional()?;
        let Some(mut seg) = seg else { return Ok(None) };
        seg.stations = self.stations(id)?;
        Ok(Some(seg))
    }

    fn checked_layout(&self, practice_id: i64, layout: Layout) -> anyhow::Result<()> {
        let p = self
            .practice(practice_id)?
            .ok_or_else(|| anyhow::anyhow!("That practice doesn't exist."))?;
        anyhow::ensure!(
            layout.ice() == p.ice,
            "“{}” doesn't fit a {} practice.",
            layout.label(),
            p.ice.label().to_lowercase()
        );
        Ok(())
    }

    /// Add a segment at the end of the practice.
    pub fn add_segment(&self, practice_id: i64, f: SegmentFields) -> anyhow::Result<i64> {
        let f = f.validate()?;
        self.checked_layout(practice_id, f.layout)?;
        let position: u32 = self.conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM segments WHERE practice_id = ?1",
            [practice_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO segments (practice_id, position, title, minutes, layout, rotation, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                practice_id,
                position,
                f.title,
                f.minutes,
                f.layout.key(),
                f.rotation,
                f.notes
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.touch_practice(practice_id)?;
        Ok(id)
    }

    /// Save a segment. When the layout changes, drills that no longer fit
    /// their slot are removed; returns how many.
    pub fn update_segment(&self, id: i64, f: SegmentFields) -> anyhow::Result<usize> {
        let f = f.validate()?;
        let seg = self
            .segment(id)?
            .ok_or_else(|| anyhow::anyhow!("That page doesn't exist."))?;
        self.checked_layout(seg.practice_id, f.layout)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE segments SET title = ?2, minutes = ?3, layout = ?4, rotation = ?5, notes = ?6
             WHERE id = ?1",
            params![id, f.title, f.minutes, f.layout.key(), f.rotation, f.notes],
        )?;
        let mut dropped = 0;
        if f.layout != seg.layout {
            let slots = f.layout.slots();
            for st in &seg.stations {
                let fits = match (slots.get(st.slot), self.drill(st.drill_id)?) {
                    (Some(slot), Some(d)) => d.ice.fits_in(slot.size),
                    _ => false,
                };
                if !fits {
                    self.clear_station(id, st.slot)?;
                    dropped += 1;
                }
            }
        }
        self.touch_practice(seg.practice_id)?;
        tx.commit()?;
        Ok(dropped)
    }

    /// Move a segment one place earlier (`up`) or later.
    pub fn move_segment(&self, id: i64, up: bool) -> anyhow::Result<()> {
        let seg = self
            .segment(id)?
            .ok_or_else(|| anyhow::anyhow!("That page doesn't exist."))?;
        let segs = self.segments(seg.practice_id)?;
        let i = segs.iter().position(|s| s.id == id).unwrap_or(0);
        let j = if up {
            i.checked_sub(1)
        } else {
            Some(i + 1).filter(|j| *j < segs.len())
        };
        let Some(j) = j else { return Ok(()) };
        let mut order: Vec<i64> = segs.iter().map(|s| s.id).collect();
        order.swap(i, j);
        let tx = self.conn.unchecked_transaction()?;
        self.renumber(&order)?;
        self.touch_practice(seg.practice_id)?;
        tx.commit()?;
        Ok(())
    }

    fn renumber(&self, order: &[i64]) -> anyhow::Result<()> {
        for (pos, sid) in order.iter().enumerate() {
            self.conn.execute(
                "UPDATE segments SET position = ?2 WHERE id = ?1",
                params![sid, pos],
            )?;
        }
        Ok(())
    }

    pub fn delete_segment(&self, id: i64) -> anyhow::Result<()> {
        let seg = self
            .segment(id)?
            .ok_or_else(|| anyhow::anyhow!("That page doesn't exist."))?;
        let tx = self.conn.unchecked_transaction()?;
        self.conn
            .execute("DELETE FROM segments WHERE id = ?1", [id])?;
        let order: Vec<i64> = self
            .segments(seg.practice_id)?
            .iter()
            .map(|s| s.id)
            .collect();
        self.renumber(&order)?;
        self.touch_practice(seg.practice_id)?;
        tx.commit()?;
        Ok(())
    }

    /// Copy a segment (with its drills) right after itself; returns the new id.
    pub fn duplicate_segment(&self, id: i64) -> anyhow::Result<i64> {
        let seg = self
            .segment(id)?
            .ok_or_else(|| anyhow::anyhow!("That page doesn't exist."))?;
        let tx = self.conn.unchecked_transaction()?;
        let new_id = self.copy_segment(&seg, seg.practice_id, seg.position + 1)?;
        let mut order: Vec<i64> = self
            .segments(seg.practice_id)?
            .iter()
            .map(|s| s.id)
            .filter(|s| *s != new_id)
            .collect();
        let at = order
            .iter()
            .position(|s| *s == id)
            .map_or(order.len(), |i| i + 1);
        order.insert(at, new_id);
        self.renumber(&order)?;
        self.touch_practice(seg.practice_id)?;
        tx.commit()?;
        Ok(new_id)
    }

    /// Place (or with `None`, remove) a segment's goalie warm-up. It must be
    /// on the practice's ice.
    pub fn set_goalie_warmup(&self, id: i64, g: Option<GoalieWarmup>) -> anyhow::Result<()> {
        let seg = self
            .segment(id)?
            .ok_or_else(|| anyhow::anyhow!("That page doesn't exist."))?;
        let g = match g {
            Some(g) => Some(g.validate(seg.layout.ice())?),
            None => None,
        };
        self.write_goalies(id, g)?;
        self.touch_practice(seg.practice_id)
    }

    fn write_goalies(&self, id: i64, g: Option<GoalieWarmup>) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE segments SET goalie_x = ?2, goalie_y = ?3, goalie_rot = ?4 WHERE id = ?1",
            params![id, g.map(|g| g.x), g.map(|g| g.y), g.map(|g| g.rot)],
        )?;
        Ok(())
    }

    /// Insert a copy of `seg` into `practice_id` (no transaction of its own).
    pub(super) fn copy_segment(
        &self,
        seg: &Segment,
        practice_id: i64,
        position: u32,
    ) -> anyhow::Result<i64> {
        self.conn.execute(
            "INSERT INTO segments (practice_id, position, title, minutes, layout, rotation, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                practice_id,
                position,
                seg.title,
                seg.minutes,
                seg.layout.key(),
                seg.rotation,
                seg.notes
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.write_goalies(id, seg.goalies)?;
        for st in &seg.stations {
            self.conn.execute(
                "INSERT INTO stations (segment_id, slot, drill_id, note, flip)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, st.slot, st.drill_id, st.note, st.flip],
            )?;
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::drills::tests::sample;
    use crate::db::practices::tests::practice;
    use crate::rink::{End, IceSize, PracticeIce};
    const QUARTERS: End = End {
        zone: false,
        lanes: 2,
    };
    use chrono::NaiveDate;

    fn fields(title: &str, layout: Layout) -> SegmentFields {
        SegmentFields {
            title: title.into(),
            minutes: 12,
            layout,
            rotation: Some(4),
            notes: "Whistle to rotate".into(),
        }
    }

    #[test]
    fn goalie_warmup_is_placed_copied_and_drawn() {
        let db = Database::in_memory().unwrap();
        let p = practice(&db, PracticeIce::Half);
        let s = db
            .add_segment(p, fields("Stations", Layout::Half(QUARTERS)))
            .unwrap();
        let presets = GoalieWarmup::presets(PracticeIce::Half);
        assert!(presets.iter().all(|(_, _, g)| g.x < 100.0), "half ice only");
        let corner = presets[0].2;
        db.set_goalie_warmup(s, Some(corner)).unwrap();
        let seg = db.segment(s).unwrap().unwrap();
        assert_eq!(seg.goalies, Some(corner));
        assert_eq!(corner.preset_key(PracticeIce::Half), Some("left-top"));

        // Off the half: refused. Angles are kept in (-180, 180].
        let off = GoalieWarmup {
            x: 150.0,
            y: 40.0,
            rot: 0.0,
        };
        assert!(db.set_goalie_warmup(s, Some(off)).is_err());
        let turned = GoalieWarmup {
            x: 50.2,
            y: 40.0,
            rot: 270.0,
        };
        db.set_goalie_warmup(s, Some(turned)).unwrap();
        let g = db.segment(s).unwrap().unwrap().goalies.unwrap();
        assert_eq!((g.x, g.rot), (50.0, -90.0));
        assert_eq!(g.preset_key(PracticeIce::Half), None);

        // Two nets, two goalies and a coach; in the drawing, legend and equipment.
        let d = g.diagram();
        let nets = d
            .items
            .iter()
            .filter(|i| matches!(i, crate::diagram::Item::Net { .. }));
        assert_eq!(nets.count(), 2);
        let copy = db.duplicate_segment(s).unwrap();
        assert_eq!(db.segment(copy).unwrap().unwrap().goalies, Some(g));
        let plan = db.plan(p).unwrap().unwrap();
        let svg = crate::board::segment_svg(&plan, &plan.segments[0], true);
        assert!(svg.contains("goalie-spot") && svg.contains("goalie-hit"));
        let legend = crate::board::segment_legend(&plan, &plan.segments[0]);
        assert!(legend.iter().any(|e| e.label.starts_with("Goalie")));
        let eq = plan.segment_equipment(&plan.segments[0]);
        assert!(eq.contains(&(crate::drill::Equipment::Nets, 2)));

        db.set_goalie_warmup(s, None).unwrap();
        assert_eq!(db.segment(s).unwrap().unwrap().goalies, None);
    }

    #[test]
    fn add_move_copy_delete() {
        let db = Database::in_memory().unwrap();
        let p = practice(&db, PracticeIce::Full);
        let a = db
            .add_segment(p, fields("Warm-up", Layout::Full(1)))
            .unwrap();
        let b = db
            .add_segment(p, fields("Stations", Layout::Ends(QUARTERS, QUARTERS)))
            .unwrap();
        assert!(
            db.add_segment(p, fields("Bad", Layout::Half(QUARTERS)))
                .is_err()
        );

        db.move_segment(b, true).unwrap();
        let order: Vec<i64> = db.segments(p).unwrap().iter().map(|s| s.id).collect();
        assert_eq!(order, vec![b, a]);

        let q = db
            .create_drill(sample("Quarter drill", IceSize::QUARTER))
            .unwrap();
        db.set_station(b, 3, q, "Coach Sam").unwrap();
        let c = db.duplicate_segment(b).unwrap();
        let segs = db.segments(p).unwrap();
        assert_eq!(segs.iter().map(|s| s.id).collect::<Vec<_>>(), vec![b, c, a]);
        assert_eq!(segs[1].stations.len(), 1);
        assert_eq!(segs[1].stations[0].note, "Coach Sam");

        // Changing the layout drops drills that no longer fit.
        assert_eq!(
            db.update_segment(c, fields("Halves", Layout::Ends(End::WHOLE, End::WHOLE)))
                .unwrap(),
            1
        );
        assert!(db.segment(c).unwrap().unwrap().stations.is_empty());

        db.delete_segment(b).unwrap();
        let segs = db.segments(p).unwrap();
        assert_eq!(
            segs.iter().map(|s| s.position).collect::<Vec<_>>(),
            vec![0, 1]
        );

        // Copy the whole practice; switching it to half ice resets full-ice pages.
        let copy = db
            .duplicate_practice(p, NaiveDate::from_ymd_opt(2026, 11, 24).unwrap())
            .unwrap();
        assert_eq!(db.segments(copy).unwrap().len(), 2);
        let mut f = crate::practice::PracticeFields {
            team_id: db.practice(copy).unwrap().unwrap().team_id,
            title: "Half".into(),
            date: NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
            start: None,
            ice: PracticeIce::Half,
            notes: String::new(),
        };
        assert_eq!(db.update_practice(copy, f.clone()).unwrap(), 2);
        assert!(
            db.segments(copy)
                .unwrap()
                .iter()
                .all(|s| s.layout == Layout::Half(End::WHOLE))
        );
        f.ice = PracticeIce::Half;
        assert_eq!(db.update_practice(copy, f).unwrap(), 0);
        assert_eq!(
            db.segments(p).unwrap().len(),
            2,
            "the original is untouched"
        );
    }
}
