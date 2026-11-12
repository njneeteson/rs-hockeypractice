//! The `stations` table: which drill runs in which slot of a segment.

use super::Database;
use crate::practice::Station;
use rusqlite::params;

impl Database {
    pub(super) fn stations(&self, segment_id: i64) -> anyhow::Result<Vec<Station>> {
        self.query_all(
            "SELECT slot, drill_id, note, flip FROM stations WHERE segment_id = ?1 ORDER BY slot",
            [segment_id],
            |r| {
                Ok(Station {
                    slot: r.get(0)?,
                    drill_id: r.get(1)?,
                    note: r.get(2)?,
                    flip: r.get(3)?,
                })
            },
        )
    }

    /// Put a drill in a slot (replacing what was there). The drill's ice size
    /// must match the slot's. The note is kept; a flip is kept only for the
    /// same drill.
    pub fn set_station(
        &self,
        segment_id: i64,
        slot: usize,
        drill_id: i64,
        note: &str,
    ) -> anyhow::Result<()> {
        let seg = self
            .segment(segment_id)?
            .ok_or_else(|| anyhow::anyhow!("That page doesn't exist."))?;
        let slots = seg.layout.slots();
        let s = slots
            .get(slot)
            .ok_or_else(|| anyhow::anyhow!("That station doesn't exist on this page."))?;
        let d = self
            .drill(drill_id)?
            .ok_or_else(|| anyhow::anyhow!("That drill doesn't exist."))?;
        anyhow::ensure!(
            d.ice.fits_in(s.size),
            "“{}” is a {} drill; it doesn't fit in the {} ({}).",
            d.name,
            d.ice.label().to_lowercase(),
            s.name.to_lowercase(),
            s.size.label().to_lowercase()
        );
        let note = note.trim();
        anyhow::ensure!(
            note.chars().count() <= 200,
            "Station notes are at most 200 characters."
        );
        self.conn.execute(
            "INSERT INTO stations (segment_id, slot, drill_id, note) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(segment_id, slot) DO UPDATE SET drill_id = excluded.drill_id,
                note = CASE WHEN excluded.note = '' THEN stations.note ELSE excluded.note END,
                flip = CASE WHEN excluded.drill_id = stations.drill_id THEN stations.flip ELSE 0 END",
            params![segment_id, slot, drill_id, note],
        )?;
        self.touch_practice(seg.practice_id)?;
        Ok(())
    }

    /// Change the note on an occupied station.
    pub fn set_station_note(&self, segment_id: i64, slot: usize, note: &str) -> anyhow::Result<()> {
        let note = note.trim();
        anyhow::ensure!(
            note.chars().count() <= 200,
            "Station notes are at most 200 characters."
        );
        let n = self.conn.execute(
            "UPDATE stations SET note = ?3 WHERE segment_id = ?1 AND slot = ?2",
            params![segment_id, slot, note],
        )?;
        anyhow::ensure!(n == 1, "Put a drill in the station before adding a note.");
        Ok(())
    }

    /// Flip (or unflip) the drill in an occupied station top ↔ bottom.
    pub fn set_station_flip(&self, segment_id: i64, slot: usize, flip: bool) -> anyhow::Result<()> {
        let n = self.conn.execute(
            "UPDATE stations SET flip = ?3 WHERE segment_id = ?1 AND slot = ?2",
            params![segment_id, slot, flip],
        )?;
        anyhow::ensure!(n == 1, "Put a drill in the station before flipping it.");
        if let Some(seg) = self.segment(segment_id)? {
            self.touch_practice(seg.practice_id)?;
        }
        Ok(())
    }

    pub fn clear_station(&self, segment_id: i64, slot: usize) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM stations WHERE segment_id = ?1 AND slot = ?2",
            params![segment_id, slot],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::drills::tests::sample;
    use crate::db::practices::tests::practice;
    use crate::practice::SegmentFields;
    use crate::rink::{End, IceSize, Layout, PracticeIce};
    const QUARTERS: End = End {
        zone: false,
        lanes: 2,
    };

    #[test]
    fn sizes_must_match() {
        let db = Database::in_memory().unwrap();
        let p = practice(&db, PracticeIce::Half);
        let seg = db
            .add_segment(
                p,
                SegmentFields {
                    title: String::new(),
                    minutes: 10,
                    layout: Layout::Half(QUARTERS),
                    rotation: None,
                    notes: String::new(),
                },
            )
            .unwrap();
        let half = db.create_drill(sample("Half", IceSize::HALF)).unwrap();
        let quarter = db
            .create_drill(sample("Quarter", IceSize::QUARTER))
            .unwrap();
        assert!(db.set_station(seg, 0, half, "").is_err());
        assert!(db.set_station(seg, 5, quarter, "").is_err());
        db.set_station(seg, 1, quarter, "Group B").unwrap();
        // Replacing the drill keeps the note.
        db.set_station(seg, 1, quarter, "").unwrap();
        assert_eq!(db.stations(seg).unwrap()[0].note, "Group B");
        assert!(
            db.delete_drill(quarter).is_err(),
            "drills in use can't be deleted"
        );
        assert!(
            db.update_drill(quarter, sample("Quarter", IceSize::HALF))
                .is_err()
        );

        let plan = db.plan(p).unwrap().unwrap();
        assert_eq!(plan.segment_equipment(&plan.segments[0]).len(), 2);
        assert_eq!(plan.total_minutes(), 10);
        assert_eq!(plan.time_range(0), "5:30–5:40 PM");

        // Flipping changes the drawing; a different drill starts unflipped.
        db.set_station_flip(seg, 1, true).unwrap();
        let plan = db.plan(p).unwrap().unwrap();
        let placed = plan.slots(&plan.segments[0]);
        assert!(placed[1].flip && !placed[0].flip);
        assert_ne!(placed[1].slot.xf, plan.segments[0].layout.slots()[1].xf);
        db.set_station(seg, 1, quarter, "").unwrap();
        assert!(
            db.stations(seg).unwrap()[0].flip,
            "same drill keeps its flip"
        );
        let other = db.create_drill(sample("Other", IceSize::QUARTER)).unwrap();
        db.set_station(seg, 1, other, "").unwrap();
        assert!(!db.stations(seg).unwrap()[0].flip);
        assert!(db.set_station_flip(seg, 3, true).is_err());

        db.clear_station(seg, 1).unwrap();
        assert!(db.stations(seg).unwrap().is_empty());
        assert!(db.set_station_note(seg, 1, "x").is_err());
    }
}
