//! The `stations` table: which drill runs in which slot of a segment.

use super::Database;
use crate::owner::Library;
use crate::practice::Station;
use rusqlite::{OptionalExtension, params};

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

    /// The practice's team owning a segment (`None` if it doesn't exist).
    pub fn segment_team(&self, segment_id: i64) -> anyhow::Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT p.team_id FROM segments s JOIN practices p ON p.id = s.practice_id
                 WHERE s.id = ?1",
                [segment_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The team's own version of a drill: the drill itself if it's in the
    /// team's library, else the team's copy of it, made now if there isn't
    /// one yet. The caller has checked the drill may be used at all.
    pub fn team_drill_for(&self, drill_id: i64, team: i64) -> anyhow::Result<i64> {
        let d = self
            .drill(drill_id)?
            .ok_or_else(|| anyhow::anyhow!("That drill doesn't exist."))?;
        if d.library == Library::Team(team) {
            return Ok(d.id);
        }
        match self.team_copy_of(d.id, team)? {
            Some(copy) => Ok(copy),
            None => self.copy_drill(d.id, Library::Team(team)),
        }
    }

    /// Put a drill from any library the member may use in a slot. A drill
    /// that isn't the team's own is copied into the team's library first (or
    /// its earlier copy is used), so later edits to the original never change
    /// this practice. Returns the drill that was placed.
    pub fn place_drill(
        &self,
        segment_id: i64,
        slot: usize,
        drill_id: i64,
        note: &str,
    ) -> anyhow::Result<i64> {
        let team = self
            .segment_team(segment_id)?
            .ok_or_else(|| anyhow::anyhow!("That page doesn't exist."))?;
        self.atomic(|| {
            let placed = self.team_drill_for(drill_id, team)?;
            self.set_station(segment_id, slot, placed, note)?;
            Ok(placed)
        })
    }

    /// Put one of the team's drills in a slot (replacing what was there). The
    /// drill must fit the slot. The note is kept; a flip is kept only for the
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
        let team = self.segment_team(segment_id)?;
        anyhow::ensure!(
            team.is_some_and(|t| d.library == Library::Team(t)),
            "“{}” isn't in this team's drill library.",
            d.name
        );
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
    use crate::db::practices::tests::{practice, team_lib};
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
        let half = db
            .create_drill(team_lib(&db), sample("Half", IceSize::HALF))
            .unwrap();
        let quarter = db
            .create_drill(team_lib(&db), sample("Quarter", IceSize::QUARTER))
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
        let other = db
            .create_drill(team_lib(&db), sample("Other", IceSize::QUARTER))
            .unwrap();
        db.set_station(seg, 1, other, "").unwrap();
        assert!(!db.stations(seg).unwrap()[0].flip);
        assert!(db.set_station_flip(seg, 3, true).is_err());

        db.clear_station(seg, 1).unwrap();
        assert!(db.stations(seg).unwrap().is_empty());
        assert!(db.set_station_note(seg, 1, "x").is_err());
    }

    #[test]
    fn practices_use_their_teams_drills() {
        use crate::db::drills::tests::ME;
        use crate::owner::Library;
        let db = Database::in_memory().unwrap();
        let p = practice(&db, PracticeIce::Half);
        let team = team_lib(&db).team().unwrap();
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
        let mine = db
            .create_drill(Library::Personal(ME), sample("Mine", IceSize::QUARTER))
            .unwrap();
        assert!(
            db.set_station(seg, 0, mine, "").is_err(),
            "only the team's own drills go on its pages"
        );

        // Placing a personal drill copies it into the team, once.
        let placed = db.place_drill(seg, 0, mine, "").unwrap();
        assert_ne!(placed, mine);
        let copy = db.drill(placed).unwrap().unwrap();
        assert_eq!(
            (copy.library, copy.copied_from),
            (Library::Team(team), Some(mine))
        );
        assert_eq!(db.place_drill(seg, 1, mine, "").unwrap(), placed);
        assert_eq!(db.place_drill(seg, 1, placed, "").unwrap(), placed);

        // Editing the original doesn't change the practice.
        db.update_drill(mine, sample("Mine, changed", IceSize::QUARTER))
            .unwrap();
        let plan = db.plan(p).unwrap().unwrap();
        assert_eq!(plan.drills[&placed].name, "Mine");

        // Moving the practice to another team copies its drills there.
        let other = db.create_team(ME, "U15", "").unwrap();
        let mut f = crate::practice::PracticeFields {
            team_id: other,
            title: String::new(),
            date: plan.practice.date,
            start: None,
            ice: PracticeIce::Half,
            notes: String::new(),
        };
        db.update_practice(p, f.clone()).unwrap();
        let plan = db.plan(p).unwrap().unwrap();
        assert!(
            plan.drills
                .values()
                .all(|d| d.library == Library::Team(other))
        );
        assert_eq!(plan.segments[0].stations.len(), 2);
        // A failed placement leaves nothing behind (no stray copy).
        let before = db.drill_count(Library::Team(other)).unwrap();
        let half = db
            .create_drill(Library::Personal(ME), sample("Half", IceSize::HALF))
            .unwrap();
        assert!(db.place_drill(seg, 2, half, "").is_err(), "too big");
        assert_eq!(db.drill_count(Library::Team(other)).unwrap(), before);
        f.team_id = 999;
        assert!(db.update_practice(p, f).is_err());
    }
}
