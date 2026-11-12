//! Teams, practices and their pages.
//!
//! A practice is a list of *segments*. Each segment becomes one page of the
//! plan: a block of time whose ice is split by a [`Layout`] into stations,
//! each running one drill.

use crate::drill::{Drill, Equipment};
use crate::rink::{Layout, PracticeIce, Slot};
use chrono::{NaiveDate, NaiveTime, Timelike};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub notes: String,
    /// Number of practices for this team.
    pub practices: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Practice {
    pub id: i64,
    pub team_id: i64,
    pub team_name: String,
    pub title: String,
    pub date: NaiveDate,
    pub start: Option<NaiveTime>,
    pub ice: PracticeIce,
    pub notes: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PracticeFields {
    pub team_id: i64,
    pub title: String,
    pub date: NaiveDate,
    pub start: Option<NaiveTime>,
    pub ice: PracticeIce,
    pub notes: String,
}

impl PracticeFields {
    pub fn validate(mut self) -> anyhow::Result<Self> {
        self.title = self.title.trim().to_string();
        self.notes = self.notes.trim().replace("\r\n", "\n");
        anyhow::ensure!(
            self.title.chars().count() <= 100,
            "Titles are at most 100 characters."
        );
        anyhow::ensure!(self.notes.len() <= 3000, "The notes are too long.");
        Ok(self)
    }
}

impl Practice {
    /// "Practice" when no title was given.
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "Practice"
        } else {
            &self.title
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub id: i64,
    pub practice_id: i64,
    pub position: u32,
    pub title: String,
    pub minutes: u32,
    pub layout: Layout,
    /// Minutes at each station before groups rotate.
    pub rotation: Option<u32>,
    /// Notes for the strip between the diagram and the drill text.
    pub notes: String,
    pub stations: Vec<Station>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentFields {
    pub title: String,
    pub minutes: u32,
    pub layout: Layout,
    pub rotation: Option<u32>,
    pub notes: String,
}

impl SegmentFields {
    pub fn validate(mut self) -> anyhow::Result<Self> {
        self.title = self.title.trim().to_string();
        self.notes = self.notes.trim().replace("\r\n", "\n");
        anyhow::ensure!(
            self.title.chars().count() <= 80,
            "Titles are at most 80 characters."
        );
        anyhow::ensure!(
            (1..=180).contains(&self.minutes),
            "Minutes must be between 1 and 180."
        );
        if let Some(r) = self.rotation {
            anyhow::ensure!(
                (1..=60).contains(&r),
                "Rotation must be between 1 and 60 minutes."
            );
        }
        anyhow::ensure!(self.notes.len() <= 1500, "The notes are too long.");
        Ok(self)
    }
}

/// A drill placed in one slot of a segment's layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub slot: usize,
    pub drill_id: i64,
    /// Optional note for this station (coach, group, variation).
    pub note: String,
}

impl Segment {
    pub fn display_title(&self) -> String {
        if self.title.is_empty() {
            format!("Part {}", self.position + 1)
        } else {
            self.title.clone()
        }
    }

    pub fn station(&self, slot: usize) -> Option<&Station> {
        self.stations.iter().find(|s| s.slot == slot)
    }

    /// The text for the strip's rotation entry, e.g. "Rotate every 6 min (3 stations)".
    pub fn rotation_text(&self) -> Option<String> {
        let r = self.rotation?;
        let k = self.layout.slots().len();
        Some(if k > 1 {
            format!(
                "Rotate every {r} min ({k} stations, {} min round)",
                r * k as u32
            )
        } else {
            format!("Switch groups every {r} min")
        })
    }
}

/// A practice with its segments and every drill they use.
#[derive(Clone, Debug)]
pub struct Plan {
    pub practice: Practice,
    pub segments: Vec<Segment>,
    pub drills: HashMap<i64, Drill>,
}

/// One slot of a segment with the drill placed there (if any).
pub struct PlacedSlot<'a> {
    pub index: usize,
    pub slot: &'a Slot,
    pub drill: Option<&'a Drill>,
    pub note: &'a str,
}

impl Plan {
    pub fn total_minutes(&self) -> u32 {
        self.segments.iter().map(|s| s.minutes).sum()
    }

    /// Minutes from the start of practice to the start of segment `i`.
    pub fn offset(&self, i: usize) -> u32 {
        self.segments[..i].iter().map(|s| s.minutes).sum()
    }

    /// "5:40–5:52 PM" when the start time is known, else "0:10–0:22".
    pub fn time_range(&self, i: usize) -> String {
        let from = self.offset(i);
        let to = from + self.segments[i].minutes;
        match self.practice.start {
            Some(t) => {
                let (a, ap) = clock(t, from);
                let (b, bp) = clock(t, to);
                if ap == bp {
                    format!("{a}–{b} {bp}")
                } else {
                    format!("{a} {ap}–{b} {bp}")
                }
            }
            None => format!("{}–{}", elapsed(from), elapsed(to)),
        }
    }

    pub fn slots<'a>(&'a self, seg: &'a Segment) -> Vec<PlacedSlot<'a>> {
        seg.layout
            .slots()
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let st = seg.station(index);
                PlacedSlot {
                    index,
                    slot,
                    drill: st.and_then(|s| self.drills.get(&s.drill_id)),
                    note: st.map(|s| s.note.as_str()).unwrap_or(""),
                }
            })
            .collect()
    }

    /// Equipment for one segment: every station's needs added up.
    pub fn segment_equipment(&self, seg: &Segment) -> Vec<(Equipment, u32)> {
        let mut totals: HashMap<Equipment, u32> = HashMap::new();
        for st in &seg.stations {
            if let Some(d) = self.drills.get(&st.drill_id) {
                for (e, q) in &d.equipment {
                    *totals.entry(*e).or_default() += q;
                }
            }
        }
        sorted(totals)
    }

    /// Equipment to bring for the whole practice: the most any one segment needs.
    pub fn equipment(&self) -> Vec<(Equipment, u32)> {
        let mut most: HashMap<Equipment, u32> = HashMap::new();
        for seg in &self.segments {
            for (e, q) in self.segment_equipment(seg) {
                let m = most.entry(e).or_default();
                *m = (*m).max(q);
            }
        }
        sorted(most)
    }
}

fn sorted(map: HashMap<Equipment, u32>) -> Vec<(Equipment, u32)> {
    let mut v: Vec<_> = map.into_iter().collect();
    v.sort_by_key(|(e, _)| *e);
    v
}

/// Wall-clock time `minutes` after `start`: ("5:40", "PM").
pub fn clock(start: NaiveTime, minutes: u32) -> (String, &'static str) {
    let total = (start.hour() * 60 + start.minute() + minutes) % (24 * 60);
    let (h, m) = (total / 60, total % 60);
    let ap = if h < 12 { "AM" } else { "PM" };
    let h12 = match h % 12 {
        0 => 12,
        h => h,
    };
    (format!("{h12}:{m:02}"), ap)
}

fn elapsed(minutes: u32) -> String {
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_times() {
        let t = NaiveTime::from_hms_opt(11, 50, 0).unwrap();
        assert_eq!(clock(t, 0), ("11:50".into(), "AM"));
        assert_eq!(clock(t, 15), ("12:05".into(), "PM"));
        assert_eq!(elapsed(75), "1:15");
    }
}
