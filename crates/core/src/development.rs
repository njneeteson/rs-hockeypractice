//! Hockey Canada's skill development model: what share of practice time goes
//! to each level of the development pyramid, by age division, and how a
//! practice measures up.
//!
//! The pyramid, from its base: technical skills, individual tactics, team
//! tactics, team play, strategy. Younger players spend nearly all their time
//! near the base; older ones climb higher. Each drill says how its time splits
//! across the levels ([`Split`]); a practice adds up the time each player
//! spends at each level ([`Composition`]) and compares it with the team's
//! targets.

use std::fmt;

str_enum! {
    /// A level of the development pyramid, from its base up.
    pub enum Level {
        Technical => ("technical", "Technical skills"),
        IndividualTactics => ("individual", "Individual tactics"),
        TeamTactics => ("team_tactics", "Team tactics"),
        TeamPlay => ("team_play", "Team play"),
        Strategy => ("strategy", "Strategy"),
    }
}

impl Level {
    /// What the level covers, in a sentence.
    pub fn about(self) -> &'static str {
        match self {
            Level::Technical => {
                "The mechanics of the game, one player at a time: skating, puck control, \
                 passing and receiving, shooting, checking skills and goaltending technique."
            }
            Level::IndividualTactics => {
                "One player using those skills against an opponent or to get open: \
                 protecting the puck, beating a defender 1-on-1, angling, gap control, \
                 getting open for a pass."
            }
            Level::TeamTactics => {
                "Two or three players working together in a small part of the ice: 2-on-1s, \
                 give-and-gos, support, double teams, short regroups."
            }
            Level::TeamPlay => {
                "The whole unit with set roles: breakouts, forechecks, neutral-zone play, \
                 defensive-zone coverage, power plays and penalty kills."
            }
            Level::Strategy => {
                "The game plan: adjusting systems to the score, the clock, the opponent and \
                 the players on the ice."
            }
        }
    }

    /// The level's position in [`Level::ALL`] (and in a [`Split`]).
    pub fn index(self) -> usize {
        Level::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }
}

/// How 100% divides across the five levels, in steps of [`Split::STEP`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Split(pub [u8; 5]);

impl Split {
    pub const STEP: u8 = 5;

    /// All technical skills.
    pub const TECHNICAL: Split = Split([100, 0, 0, 0, 0]);

    /// A checked split: whole multiples of [`Split::STEP`] adding to 100.
    pub fn new(parts: [u8; 5]) -> anyhow::Result<Split> {
        let total: u32 = parts.iter().map(|p| *p as u32).sum();
        anyhow::ensure!(
            parts.iter().all(|p| p % Self::STEP == 0),
            "Use steps of {}% for the development split.",
            Self::STEP
        );
        anyhow::ensure!(
            total == 100,
            "The development split adds up to {total}%; it needs to make 100%."
        );
        Ok(Split(parts))
    }

    pub fn get(self, level: Level) -> u8 {
        self.0[level.index()]
    }

    /// Stored as `"50,40,10,0,0"`.
    pub fn key(self) -> String {
        self.0
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn from_key(s: &str) -> Option<Split> {
        let parts: Vec<u8> = s
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect::<Option<_>>()?;
        Split::new(parts.try_into().ok()?).ok()
    }

    /// The highest level with any time: how far up the pyramid it reaches.
    pub fn top(self) -> Level {
        Level::ALL
            .iter()
            .rev()
            .copied()
            .find(|l| self.get(*l) > 0)
            .unwrap_or(Level::Technical)
    }
}

impl fmt::Display for Split {
    /// "50 / 40 / 10 / 0 / 0"
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(|p| p.to_string()).collect();
        f.write_str(&parts.join(" / "))
    }
}

str_enum! {
    /// A team's age division, which sets its development targets.
    pub enum AgeDivision {
        U7 => ("u7", "U7"),
        U9 => ("u9", "U9"),
        U11 => ("u11", "U11"),
        U13 => ("u13", "U13"),
        U15 => ("u15", "U15"),
        U18 => ("u18", "U18"),
        Other => ("other", "Other (no targets)"),
    }
}

impl AgeDivision {
    /// Hockey Canada's recommended split of practice time for the division
    /// (its age-division pages and Skills Development Program; see the
    /// guidelines page for links). `None` for [`AgeDivision::Other`].
    pub fn hockey_canada(self) -> Option<Split> {
        Some(Split(match self {
            AgeDivision::U7 => [85, 15, 0, 0, 0],
            AgeDivision::U9 => [75, 15, 10, 0, 0],
            AgeDivision::U11 => [50, 20, 15, 10, 5],
            AgeDivision::U13 => [45, 25, 10, 10, 10],
            AgeDivision::U15 => [40, 15, 20, 15, 10],
            // Some summaries give U18 as 35/15/20/15/10, which makes 95%; the
            // Skills Development Program document's figures add up.
            AgeDivision::U18 => [35, 15, 20, 15, 15],
            AgeDivision::Other => return None,
        }))
    }

    /// Players' ages in the division.
    pub fn ages(self) -> &'static str {
        match self {
            AgeDivision::U7 => "5 and 6",
            AgeDivision::U9 => "7 and 8",
            AgeDivision::U11 => "9 and 10",
            AgeDivision::U13 => "11 and 12",
            AgeDivision::U15 => "13 and 14",
            AgeDivision::U18 => "15 to 17",
            AgeDivision::Other => "",
        }
    }

    /// The divisions with targets.
    pub fn with_targets() -> impl Iterator<Item = AgeDivision> {
        AgeDivision::ALL
            .iter()
            .copied()
            .filter(|a| a.hockey_canada().is_some())
    }
}

/// Minutes each player spends at each level, plus minutes in drills nobody
/// has classified yet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Composition {
    pub minutes: [f32; 5],
    pub unclassified: f32,
}

impl Composition {
    /// Count `minutes` of a drill with this split (or unclassified).
    pub fn add(&mut self, split: Option<Split>, minutes: f32) {
        match split {
            Some(s) => {
                for (m, p) in self.minutes.iter_mut().zip(s.0) {
                    *m += minutes * p as f32 / 100.0;
                }
            }
            None => self.unclassified += minutes,
        }
    }

    pub fn merge(&mut self, other: &Composition) {
        for (m, o) in self.minutes.iter_mut().zip(other.minutes) {
            *m += o;
        }
        self.unclassified += other.unclassified;
    }

    /// Minutes in classified drills.
    pub fn classified(&self) -> f32 {
        self.minutes.iter().sum()
    }

    pub fn total(&self) -> f32 {
        self.classified() + self.unclassified
    }

    pub fn minutes_at(&self, level: Level) -> f32 {
        self.minutes[level.index()]
    }

    /// Share of the classified time at a level, 0–100 (`None` with no
    /// classified time).
    pub fn percent(&self, level: Level) -> Option<f32> {
        let c = self.classified();
        (c > 0.0).then(|| self.minutes_at(level) * 100.0 / c)
    }
}

/// How far a share may stray from its target and still count as on target
/// (percentage points).
pub const TOLERANCE: f32 = 5.0;

/// How a level's share compares with its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    OnTarget,
    Under,
    Over,
}

impl Verdict {
    pub fn of(actual: f32, target: u8) -> Verdict {
        let t = target as f32;
        if actual > t + TOLERANCE || (target == 0 && actual > 0.5) {
            Verdict::Over
        } else if actual < t - TOLERANCE {
            Verdict::Under
        } else {
            Verdict::OnTarget
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Verdict::OnTarget => "on",
            Verdict::Under => "under",
            Verdict::Over => "over",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::OnTarget => "on target",
            Verdict::Under => "below target",
            Verdict::Over => "above target",
        }
    }
}

/// How many percentage points closer to `targets` a practice with
/// composition `c` gets by adding `minutes` of a drill split `drill`
/// (positive: closer; negative: further). Distance is the sum, over the
/// levels, of how far each share is from its target (0 to 200). A practice
/// with nothing counted yet starts at the furthest, so a drill scores by
/// how well its own split matches the targets.
pub fn closer_by(c: &Composition, targets: Split, drill: Split, minutes: f32) -> f32 {
    let distance = |c: &Composition| -> f32 {
        Level::ALL
            .iter()
            .map(|l| {
                c.percent(*l)
                    .map(|p| (p - f32::from(targets.get(*l))).abs())
            })
            .sum::<Option<f32>>()
            .unwrap_or(200.0)
    };
    let mut after = *c;
    after.add(Some(drill), minutes);
    distance(c) - distance(&after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_add_to_100_in_steps() {
        assert!(Split::new([50, 40, 10, 0, 0]).is_ok());
        assert!(Split::new([50, 40, 5, 0, 0]).is_err(), "95%");
        assert!(Split::new([52, 38, 10, 0, 0]).is_err(), "not in 5% steps");
        let s = Split::new([45, 25, 10, 10, 10]).unwrap();
        assert_eq!(Split::from_key(&s.key()), Some(s));
        assert_eq!(Split::from_key("1,2"), None);
        assert_eq!(Split::from_key("100,0,0,0,5"), None);
        assert_eq!(s.top(), Level::Strategy);
        assert_eq!(Split::TECHNICAL.top(), Level::Technical);
        assert_eq!(s.to_string(), "45 / 25 / 10 / 10 / 10");
    }

    #[test]
    fn every_division_target_adds_to_100() {
        for a in AgeDivision::with_targets() {
            let s = a.hockey_canada().unwrap();
            assert_eq!(Split::new(s.0).unwrap(), s, "{a:?}");
        }
        assert_eq!(AgeDivision::Other.hockey_canada(), None);
    }

    #[test]
    fn compositions_add_up() {
        let mut c = Composition::default();
        c.add(Some(Split([50, 50, 0, 0, 0])), 10.0);
        c.add(Some(Split::TECHNICAL), 10.0);
        c.add(None, 5.0);
        assert_eq!(c.minutes_at(Level::Technical), 15.0);
        assert_eq!(c.classified(), 20.0);
        assert_eq!(c.total(), 25.0);
        assert_eq!(c.percent(Level::IndividualTactics), Some(25.0));
        assert_eq!(Composition::default().percent(Level::Technical), None);

        assert_eq!(Verdict::of(48.0, 50), Verdict::OnTarget);
        assert_eq!(Verdict::of(40.0, 50), Verdict::Under);
        assert_eq!(Verdict::of(60.0, 50), Verdict::Over);
        assert_eq!(Verdict::of(3.0, 0), Verdict::Over, "none at all expected");
    }
    #[test]
    fn drills_that_bring_a_practice_closer_to_its_targets() {
        let u9 = AgeDivision::U9.hockey_canada().unwrap();
        let mut c = Composition::default();
        c.add(Some(Split::TECHNICAL), 40.0);
        let tactics = Split::new([0, 0, 100, 0, 0]).unwrap();
        assert!(closer_by(&c, u9, tactics, 5.0) > 0.0, "the missing level");
        assert!(
            closer_by(&c, u9, Split::TECHNICAL, 5.0) <= 0.0,
            "more of the same"
        );
        // Nothing counted yet: the closer a drill's own split, the better.
        let empty = Composition::default();
        assert!(closer_by(&empty, u9, u9, 10.0) > closer_by(&empty, u9, tactics, 10.0));
    }
}
