//! Drills: what they need (ice, equipment), how to run them and their diagram.

use crate::diagram::Diagram;
use crate::rink::IceSize;

str_enum! {
    /// What a drill mainly works on.
    pub enum Category {
        WarmUp => ("warmup", "Warm-up"),
        Skating => ("skating", "Skating"),
        PuckHandling => ("puckhandling", "Puck handling"),
        Passing => ("passing", "Passing"),
        Shooting => ("shooting", "Shooting"),
        Battles => ("battles", "Battles and checking"),
        TeamPlay => ("team", "Team play"),
        SmallArea => ("small_area", "Small-area game"),
        Goalies => ("goalies", "Goalies"),
        Conditioning => ("conditioning", "Conditioning"),
        Other => ("other", "Other"),
    }
}

str_enum! {
    /// Equipment a drill needs on the ice.
    pub enum Equipment {
        Pucks => ("pucks", "Pucks"),
        Cones => ("cones", "Cones"),
        Nets => ("nets", "Nets"),
        Tires => ("tires", "Tires"),
        Pinnies => ("pinnies", "Pinnies"),
        Sticks => ("sticks", "Extra sticks"),
        Hurdles => ("hurdles", "Hurdles"),
        Ladder => ("ladder", "Agility ladder"),
        Passers => ("passers", "Passing aids"),
        Bumpers => ("bumpers", "Bumpers or pads"),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drill {
    pub id: i64,
    pub name: String,
    pub ice: IceSize,
    pub category: Category,
    /// Suggested length in minutes.
    pub minutes: u32,
    /// How to run the drill.
    pub description: String,
    /// Key teaching points, one per line.
    pub points: String,
    pub diagram: Diagram,
    /// Equipment with quantities, in [`Equipment::ALL`] order.
    pub equipment: Vec<(Equipment, u32)>,
    pub updated_at: String,
}

/// Everything about a drill that a form edits.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillFields {
    pub name: String,
    pub ice: IceSize,
    pub category: Category,
    pub minutes: u32,
    pub description: String,
    pub points: String,
    pub diagram: Diagram,
    pub equipment: Vec<(Equipment, u32)>,
}

impl DrillFields {
    /// Trim text and check limits.
    pub fn validate(mut self) -> anyhow::Result<Self> {
        self.name = self.name.trim().to_string();
        anyhow::ensure!(!self.name.is_empty(), "The drill needs a name.");
        anyhow::ensure!(
            self.name.chars().count() <= 80,
            "Drill names are at most 80 characters."
        );
        anyhow::ensure!(
            (1..=120).contains(&self.minutes),
            "Minutes must be between 1 and 120."
        );
        self.description = self.description.trim().replace("\r\n", "\n");
        self.points = self.points.trim().replace("\r\n", "\n");
        anyhow::ensure!(
            self.description.len() <= 5000 && self.points.len() <= 3000,
            "The description or key points are too long."
        );
        self.diagram.validate()?;
        self.equipment.retain(|(_, q)| *q > 0);
        self.equipment.sort_by_key(|(e, _)| *e);
        self.equipment.dedup_by_key(|(e, _)| *e);
        Ok(self)
    }
}

impl Drill {
    /// Key teaching points, one per non-empty line, without list bullets.
    pub fn key_points(&self) -> Vec<&str> {
        key_points(&self.points)
    }

    /// Description paragraphs (split on line breaks).
    pub fn paragraphs(&self) -> Vec<&str> {
        self.description
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect()
    }

    pub fn fields(&self) -> DrillFields {
        DrillFields {
            name: self.name.clone(),
            ice: self.ice,
            category: self.category,
            minutes: self.minutes,
            description: self.description.clone(),
            points: self.points.clone(),
            diagram: self.diagram.clone(),
            equipment: self.equipment.clone(),
        }
    }

    pub fn equipment_qty(&self, e: Equipment) -> u32 {
        self.equipment
            .iter()
            .find(|(k, _)| *k == e)
            .map(|(_, q)| *q)
            .unwrap_or(0)
    }

    /// "Pucks 20 · Cones 6"
    pub fn equipment_summary(&self) -> String {
        equipment_summary(&self.equipment)
    }
}

pub fn key_points(text: &str) -> Vec<&str> {
    text.lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim())
        .filter(|l| !l.is_empty())
        .collect()
}

pub fn equipment_summary(items: &[(Equipment, u32)]) -> String {
    items
        .iter()
        .map(|(e, q)| format!("{} {q}", e.label()))
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> DrillFields {
        DrillFields {
            name: "  Figure eights ".into(),
            ice: IceSize::Half,
            category: Category::Skating,
            minutes: 8,
            description: "Line up.\r\nGo.".into(),
            points: "- Knees bent\n\n• Heads up\n".into(),
            diagram: Diagram::default(),
            equipment: vec![
                (Equipment::Pucks, 0),
                (Equipment::Cones, 4),
                (Equipment::Cones, 2),
            ],
        }
    }

    #[test]
    fn validation_cleans_up() {
        let f = fields().validate().unwrap();
        assert_eq!(f.name, "Figure eights");
        assert_eq!(f.description, "Line up.\nGo.");
        assert_eq!(f.equipment, vec![(Equipment::Cones, 4)]);
        assert_eq!(key_points(&f.points), vec!["Knees bent", "Heads up"]);
        let bad = DrillFields {
            name: " ".into(),
            ..fields()
        };
        assert!(bad.validate().is_err());
        let bad = DrillFields {
            minutes: 0,
            ..fields()
        };
        assert!(bad.validate().is_err());
    }
}
