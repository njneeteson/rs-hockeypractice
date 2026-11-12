//! A few example drills to start the library with (one of each ice size).

use crate::diagram::{Diagram, Item, LineEnd, LineStyle};
use crate::drill::{Category, DrillFields, Equipment};
use crate::rink::IceSize;

fn player(x: f32, y: f32, label: &str) -> Item {
    Item::Player {
        x,
        y,
        label: label.into(),
    }
}

fn line(style: LineStyle, pts: &[(f32, f32)]) -> Item {
    Item::Line {
        style,
        end: LineEnd::Arrow,
        pts: pts.iter().map(|(x, y)| [*x, *y]).collect(),
    }
}

pub fn drills() -> Vec<DrillFields> {
    vec![
        DrillFields {
            name: "Full-ice flow warm-up".into(),
            ice: IceSize::Full,
            category: Category::WarmUp,
            minutes: 6,
            description: "Two lines in opposite corners. On the whistle the first player in each line \
                skates the length of the ice, around the far circle, and back up the middle.\n\
                Add a puck after two laps; finish each lap with a shot."
                .into(),
            points: "Long, full strides\nCrossovers through the circles\nHeads up through the middle".into(),
            diagram: Diagram {
                items: vec![
                    player(12.0, 72.0, "X"),
                    player(16.0, 75.0, "X"),
                    player(188.0, 13.0, "X"),
                    player(184.0, 10.0, "X"),
                    line(LineStyle::Skate, &[(20.0, 74.0), (100.0, 74.0), (165.0, 72.0), (182.0, 58.0), (170.0, 48.0), (100.0, 40.0)]),
                    line(LineStyle::Skate, &[(180.0, 11.0), (100.0, 11.0), (35.0, 13.0), (18.0, 27.0), (30.0, 37.0), (100.0, 45.0)]),
                ],
            },
            equipment: vec![(Equipment::Pucks, 20)],
        },
        DrillFields {
            name: "Pass, skate and shoot".into(),
            ice: IceSize::Half,
            category: Category::Shooting,
            minutes: 10,
            description: "Shooters line up at the red line with pucks; a passer stands at the far \
                faceoff dot. The shooter passes, skates hard around the top of the circle, \
                takes the return pass in stride and shoots."
                .into(),
            points: "Give and go: pass and move\nStick on the ice, ready to receive\nShoot in stride, pick a corner"
                .into(),
            diagram: Diagram {
                items: vec![
                    player(90.0, 14.0, "X"),
                    player(94.0, 11.0, "X"),
                    Item::Pucks { x: 96.0, y: 18.0 },
                    player(31.0, 64.5, "C"),
                    line(LineStyle::Pass, &[(87.0, 16.0), (35.0, 61.0)]),
                    line(LineStyle::Skate, &[(88.0, 14.0), (55.0, 16.0), (40.0, 26.0), (30.0, 38.0)]),
                    line(LineStyle::Pass, &[(33.0, 61.0), (30.0, 41.0)]),
                    line(LineStyle::Shot, &[(28.0, 39.0), (13.0, 42.0)]),
                ],
            },
            equipment: vec![(Equipment::Pucks, 25)],
        },
        DrillFields {
            name: "1-on-1 corner battles".into(),
            ice: IceSize::Quarter,
            category: Category::Battles,
            minutes: 8,
            description: "Forwards and defence start on the boards at the hash marks. The coach \
                dumps a puck into the corner; both players race for it. The winner tries to \
                score, the other defends. Play until a goal, a save, or the coach's whistle."
                .into(),
            points: "Low centre of gravity on the puck\nStick on puck, body on body\nDefenders keep the inside position"
                .into(),
            diagram: Diagram {
                items: vec![
                    player(55.0, 6.0, "X"),
                    player(48.0, 6.0, "O"),
                    player(70.0, 30.0, "C"),
                    Item::Pucks { x: 74.0, y: 33.0 },
                    line(LineStyle::Pass, &[(67.0, 28.0), (25.0, 12.0), (8.0, 20.0)]),
                    line(LineStyle::Skate, &[(55.0, 9.0), (25.0, 9.0), (12.0, 18.0)]),
                    line(LineStyle::Backward, &[(46.0, 9.0), (30.0, 22.0), (20.0, 32.0)]),
                ],
            },
            equipment: vec![(Equipment::Pucks, 15)],
        },
        DrillFields {
            name: "Neutral-zone puck control weave".into(),
            ice: IceSize::Neutral,
            category: Category::PuckHandling,
            minutes: 8,
            description: "Set two rows of cones between the blue lines. Players carry a puck \
                through the cones, alternating forehand and backhand turns, then cross back \
                along the other row."
                .into(),
            points: "Soft hands, puck in front of the body\nEyes up between the cones\nTight turns: lead with the head"
                .into(),
            diagram: Diagram {
                items: vec![
                    Item::Cone { x: 88.0, y: 20.0 },
                    Item::Cone { x: 100.0, y: 26.0 },
                    Item::Cone { x: 112.0, y: 20.0 },
                    Item::Cone { x: 88.0, y: 65.0 },
                    Item::Cone { x: 100.0, y: 59.0 },
                    Item::Cone { x: 112.0, y: 65.0 },
                    player(80.0, 10.0, "X"),
                    Item::Pucks { x: 80.0, y: 4.5 },
                    line(LineStyle::Carry, &[(82.0, 13.0), (88.0, 25.0), (100.0, 21.0), (112.0, 26.0), (119.0, 36.0)]),
                    line(LineStyle::Carry, &[(119.0, 49.0), (112.0, 60.0), (100.0, 64.0), (88.0, 60.0), (82.0, 72.0)]),
                ],
            },
            equipment: vec![(Equipment::Pucks, 20), (Equipment::Cones, 6)],
        },
        DrillFields {
            name: "3-on-2 end-zone small-area game".into(),
            ice: IceSize::EndZone,
            category: Category::SmallArea,
            minutes: 10,
            description: "Put a second net facing the goal at the blue line. Three attackers play \
                against two defenders. Defenders who win the puck score on the blue-line net. \
                Shifts of 40 seconds; the coach puts in a new puck after each goal."
                .into(),
            points: "Attack with speed, use the width\nSupport the puck carrier: triangle\nDefenders: stay between the puck and the net"
                .into(),
            diagram: Diagram {
                items: vec![
                    Item::Net { x: 68.0, y: 42.5, rot: 180.0 },
                    player(45.0, 20.0, "X"),
                    player(45.0, 65.0, "X"),
                    player(55.0, 42.5, "X"),
                    player(30.0, 35.0, "O"),
                    player(30.0, 52.0, "O"),
                    player(62.0, 76.0, "C"),
                    Item::Pucks { x: 66.0, y: 79.0 },
                    line(LineStyle::Carry, &[(52.0, 41.0), (40.0, 39.0), (32.0, 44.0)]),
                    line(LineStyle::Pass, &[(43.0, 22.0), (32.0, 28.0), (20.0, 33.0)]),
                    Item::Text { x: 58.0, y: 56.0, text: "40 s shifts".into() },
                ],
            },
            equipment: vec![(Equipment::Pucks, 15), (Equipment::Nets, 1), (Equipment::Pinnies, 5)],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_are_valid() {
        let drills = drills();
        for d in &drills {
            d.clone().validate().unwrap();
            let frame = d.ice.frame().pad(2.0);
            for item in &d.diagram.items {
                if let Item::Line { pts, .. } = item {
                    assert!(pts.iter().all(|p| frame.contains(p[0], p[1])), "{}", d.name);
                }
            }
        }
        for size in IceSize::ALL {
            assert!(drills.iter().any(|d| d.ice == *size), "{size:?}");
        }
    }
}
