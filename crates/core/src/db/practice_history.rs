//! The `practice_history` table: every change to a practice's parts, with
//! what the parts looked like just before it, so any change can be undone.
//!
//! A change made through [`Database::tracked`] saves a snapshot of the
//! practice's ice and parts (titles, minutes, layouts, notes, goalie warm-up
//! and the drill in each station) before it runs. Going back to a snapshot is
//! itself a change, recorded the same way, so undoing an undo is just another
//! step back. The newest [`KEEP`] changes of each practice are kept.

use super::Database;
use crate::owner::MemberId;
use crate::practice::GoalieWarmup;
use crate::rink::{Layout, PracticeIce};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Changes kept per practice; older ones are forgotten.
pub const KEEP: usize = 200;

/// One recorded change.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub id: i64,
    /// What was done ("Placed “Weave” in the left half.").
    pub label: String,
    pub member: Option<MemberId>,
    /// UTC, "YYYY-MM-DD HH:MM:SS".
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Snapshot {
    ice: String,
    segments: Vec<SegmentSnap>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SegmentSnap {
    id: i64,
    title: String,
    minutes: u32,
    layout: String,
    rotation: Option<u32>,
    notes: String,
    /// x, y, rot.
    goalies: Option<[f32; 3]>,
    stations: Vec<StationSnap>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct StationSnap {
    slot: usize,
    drill: i64,
    note: String,
    flip: bool,
}

impl Database {
    fn snapshot(&self, practice_id: i64) -> anyhow::Result<Snapshot> {
        let p = self
            .practice(practice_id)?
            .ok_or_else(|| anyhow::anyhow!("That practice doesn't exist."))?;
        let segments = self
            .segments(practice_id)?
            .into_iter()
            .map(|s| SegmentSnap {
                id: s.id,
                title: s.title,
                minutes: s.minutes,
                layout: s.layout.key(),
                rotation: s.rotation,
                notes: s.notes,
                goalies: s.goalies.map(|g| [g.x, g.y, g.rot]),
                stations: s
                    .stations
                    .into_iter()
                    .map(|st| StationSnap {
                        slot: st.slot,
                        drill: st.drill_id,
                        note: st.note,
                        flip: st.flip,
                    })
                    .collect(),
            })
            .collect();
        Ok(Snapshot {
            ice: p.ice.key().to_string(),
            segments,
        })
    }

    /// Run a change to a practice and record it for undo, described by what
    /// `f` returns (the message shown to the coach). Nothing is recorded if
    /// the change fails or leaves the practice as it was. All or nothing.
    pub fn tracked<T: std::fmt::Display>(
        &self,
        practice_id: i64,
        by: MemberId,
        f: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.atomic(|| {
            let before = self.snapshot(practice_id)?;
            let out = f()?;
            // The practice may be gone (deleted); then there's nothing to record.
            if self.practice(practice_id)?.is_some() && self.snapshot(practice_id)? != before {
                self.conn.execute(
                    "INSERT INTO practice_history (practice_id, member_id, label, snapshot)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        practice_id,
                        by.0,
                        out.to_string(),
                        serde_json::to_string(&before)?
                    ],
                )?;
                self.conn.execute(
                    "DELETE FROM practice_history WHERE practice_id = ?1 AND id NOT IN
                     (SELECT id FROM practice_history WHERE practice_id = ?1
                      ORDER BY id DESC LIMIT ?2)",
                    params![practice_id, KEEP],
                )?;
            }
            Ok(out)
        })
    }

    /// A practice's changes, newest first.
    pub fn history(&self, practice_id: i64, limit: usize) -> anyhow::Result<Vec<Change>> {
        self.query_all(
            "SELECT id, label, member_id, created_at FROM practice_history
             WHERE practice_id = ?1 ORDER BY id DESC LIMIT ?2",
            params![practice_id, limit],
            |r| {
                Ok(Change {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    member: r.get::<_, Option<i64>>(2)?.map(MemberId),
                    at: r.get(3)?,
                })
            },
        )
    }

    /// Put the practice back as it was just before change `change_id` (one
    /// of its own). This is recorded as a change too, so it can be undone.
    /// Returns the message for the coach.
    pub fn restore(
        &self,
        practice_id: i64,
        change_id: i64,
        by: MemberId,
    ) -> anyhow::Result<String> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT label, snapshot FROM practice_history WHERE id = ?1 AND practice_id = ?2",
                params![change_id, practice_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (label, snapshot) =
            row.ok_or_else(|| anyhow::anyhow!("That change isn't in this practice's history."))?;
        let snapshot: Snapshot = serde_json::from_str(&snapshot)?;
        let label = label.trim_end_matches('.');
        self.tracked(practice_id, by, || {
            let missing = self.apply_snapshot(practice_id, &snapshot)?;
            Ok(match missing {
                0 => format!("Undid: {label}."),
                n => format!(
                    "Undid: {label}. {n} drill(s) have been deleted or changed size since, \
                     so their stations are empty."
                ),
            })
        })
    }

    /// Undo the practice's latest change (`None` if there's nothing to undo).
    pub fn undo(&self, practice_id: i64, by: MemberId) -> anyhow::Result<Option<String>> {
        match self.history(practice_id, 1)?.first() {
            Some(c) => self.restore(practice_id, c.id, by).map(Some),
            None => Ok(None),
        }
    }

    /// Make the practice's parts match a snapshot. Parts keep their ids where
    /// they can (so links to them still work). Returns how many drills
    /// couldn't be put back.
    fn apply_snapshot(&self, practice_id: i64, s: &Snapshot) -> anyhow::Result<usize> {
        let p = self
            .practice(practice_id)?
            .ok_or_else(|| anyhow::anyhow!("That practice doesn't exist."))?;
        let ice = PracticeIce::from_key(&s.ice).unwrap_or(p.ice);
        if ice != p.ice {
            self.conn.execute(
                "UPDATE practices SET ice = ?2 WHERE id = ?1",
                params![practice_id, ice.key()],
            )?;
        }
        let keep: Vec<i64> = s.segments.iter().map(|g| g.id).collect();
        for seg in self.segments(practice_id)? {
            if !keep.contains(&seg.id) {
                self.conn
                    .execute("DELETE FROM segments WHERE id = ?1", [seg.id])?;
            }
        }
        let mut missing = 0;
        for (pos, g) in s.segments.iter().enumerate() {
            let layout = Layout::from_key(&g.layout)
                .filter(|l| l.ice() == ice)
                .unwrap_or(Layout::default_for(ice));
            let owner: Option<i64> = self
                .conn
                .query_row(
                    "SELECT practice_id FROM segments WHERE id = ?1",
                    [g.id],
                    |r| r.get(0),
                )
                .optional()?;
            let fields = params![
                pos,
                g.title,
                g.minutes,
                layout.key(),
                g.rotation,
                g.notes,
                g.goalies.map(|v| v[0]),
                g.goalies.map(|v| v[1]),
                g.goalies.map(|v| v[2]),
            ];
            let sid = match owner {
                Some(pid) if pid == practice_id => {
                    self.conn.execute(
                        "UPDATE segments SET position = ?1, title = ?2, minutes = ?3, layout = ?4,
                         rotation = ?5, notes = ?6, goalie_x = ?7, goalie_y = ?8, goalie_rot = ?9
                         WHERE id = ?10",
                        rusqlite::params_from_iter(fields.iter().copied().chain([&g.id as _])),
                    )?;
                    g.id
                }
                // Its id is free again: reuse it. Taken by another practice's
                // part since: a new id.
                other => {
                    let id = if other.is_none() { Some(g.id) } else { None };
                    self.conn.execute(
                        "INSERT INTO segments (position, title, minutes, layout, rotation, notes,
                         goalie_x, goalie_y, goalie_rot, practice_id, id)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                        rusqlite::params_from_iter(
                            fields.iter().copied().chain([&practice_id as _, &id as _]),
                        ),
                    )?;
                    self.conn.last_insert_rowid()
                }
            };
            self.conn
                .execute("DELETE FROM stations WHERE segment_id = ?1", [sid])?;
            let slots = layout.slots();
            for st in &g.stations {
                let fits = match (slots.get(st.slot), self.drill(st.drill)?) {
                    (Some(slot), Some(d)) => d.ice.fits_in(slot.size),
                    _ => false,
                };
                if !fits {
                    missing += 1;
                    continue;
                }
                // The practice may have moved to another team since: its own copy.
                let drill = self.team_drill_for(st.drill, p.team_id)?;
                self.conn.execute(
                    "INSERT INTO stations (segment_id, slot, drill_id, note, flip)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![sid, st.slot, drill, st.note, st.flip],
                )?;
            }
            if let Some([x, y, rot]) = g.goalies {
                // Re-check it's on the ice (the ice may have changed back and forth).
                let ok = GoalieWarmup { x, y, rot }.validate(ice).is_ok();
                if !ok {
                    self.conn.execute(
                        "UPDATE segments SET goalie_x = NULL, goalie_y = NULL, goalie_rot = NULL
                         WHERE id = ?1",
                        [sid],
                    )?;
                }
            }
        }
        self.touch_practice(practice_id)?;
        Ok(missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::drills::tests::{ME, sample};
    use crate::db::practices::tests::{practice, team_lib};
    use crate::practice::SegmentFields;
    use crate::rink::{End, IceSize};

    fn part(title: &str) -> SegmentFields {
        SegmentFields {
            title: title.into(),
            minutes: 10,
            layout: Layout::Half(End {
                zone: false,
                lanes: 2,
            }),
            rotation: None,
            notes: String::new(),
        }
    }

    #[test]
    fn any_change_can_be_undone_and_undoing_is_a_change() {
        let db = Database::in_memory().unwrap();
        let p = practice(&db, PracticeIce::Half);
        let d = db
            .create_drill(team_lib(&db), sample("Quarter", IceSize::QUARTER))
            .unwrap();
        let a = db
            .tracked(p, ME, || {
                db.add_segment(p, part("Warm-up")).map(|_| "Added A")
            })
            .unwrap();
        assert_eq!(a, "Added A");
        let sid = db.segments(p).unwrap()[0].id;
        db.tracked(p, ME, || {
            db.set_station(sid, 1, d, "Sam").map(|()| "Placed")
        })
        .unwrap();
        db.tracked(p, ME, || {
            db.add_segment(p, part("Games")).map(|_| "Added B")
        })
        .unwrap();
        // No change, nothing recorded.
        db.tracked(p, ME, || Ok("Nothing")).unwrap();
        let h = db.history(p, 10).unwrap();
        assert_eq!(
            h.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["Added B", "Placed", "Added A"]
        );

        // Back to before "Placed": one part, no drill; the part keeps its id.
        let msg = db.restore(p, h[1].id, ME).unwrap();
        assert!(msg.contains("Placed"));
        let segs = db.segments(p).unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].id, sid);
        assert!(segs[0].stations.is_empty());

        // Undoing the undo brings everything back, ids included.
        db.undo(p, ME).unwrap().unwrap();
        let segs = db.segments(p).unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].id, sid);
        assert_eq!(segs[0].stations[0].note, "Sam");

        // A failed change records nothing and changes nothing.
        let n = db.history(p, 100).unwrap().len();
        assert!(
            db.tracked(p, ME, || -> anyhow::Result<&str> {
                db.clear_station(sid, 1)?;
                anyhow::bail!("no")
            })
            .is_err()
        );
        assert_eq!(db.history(p, 100).unwrap().len(), n);
        assert_eq!(db.segments(p).unwrap()[0].stations.len(), 1);

        // Restoring with a drill that has since been deleted leaves its station empty.
        db.tracked(p, ME, || db.clear_station(sid, 1).map(|()| "Removed"))
            .unwrap();
        db.delete_drill(d).unwrap();
        let msg = db.undo(p, ME).unwrap().unwrap();
        assert!(msg.contains("1 drill(s)"), "{msg}");
        assert!(db.segments(p).unwrap()[0].stations.is_empty());

        // Someone else's practice's changes can't be restored here.
        let other = practice(&db, PracticeIce::Half);
        let h = db.history(p, 1).unwrap();
        assert!(db.restore(other, h[0].id, ME).is_err());
    }
}
