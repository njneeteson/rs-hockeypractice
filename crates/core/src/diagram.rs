//! A drill's diagram: players, equipment and movement lines, drawn on the
//! drill's canonical frame (see [`crate::rink`]).
//!
//! Stored as JSON. The browser editor (`assets/app.js`) reads and writes the
//! same format and draws it the same way; keep the two in step.
//!
//! ```json
//! {"items": [
//!   {"t": "player", "x": 30, "y": 20, "label": "X"},
//!   {"t": "cone", "x": 40, "y": 30},
//!   {"t": "line", "style": "skate", "end": "arrow", "pts": [[30, 20], [50, 30], [60, 15]]}
//! ]}
//! ```

use crate::rink::Xf;
use crate::svg::{colour as c, esc, n, polyline_d, text};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// The most items one diagram may hold.
pub const MAX_ITEMS: usize = 400;
/// The most points one line may hold.
pub const MAX_POINTS: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Item {
    /// A player or coach, drawn as its label (X, O, C, G, F1, …).
    Player {
        x: f32,
        y: f32,
        label: String,
    },
    Cone {
        x: f32,
        y: f32,
    },
    Puck {
        x: f32,
        y: f32,
    },
    /// A pile of pucks.
    Pucks {
        x: f32,
        y: f32,
    },
    Tire {
        x: f32,
        y: f32,
    },
    /// An extra net; `rot` is the direction its mouth faces, in degrees.
    Net {
        x: f32,
        y: f32,
        #[serde(default)]
        rot: f32,
    },
    /// A free text label.
    Text {
        x: f32,
        y: f32,
        text: String,
    },
    Line {
        style: LineStyle,
        #[serde(default)]
        end: LineEnd,
        pts: Vec<[f32; 2]>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineStyle {
    /// Forward skating: solid line.
    Skate,
    /// Backward skating: solid line with cross ticks.
    Backward,
    /// Skating with the puck: wavy line.
    Carry,
    /// Pass: dashed line.
    Pass,
    /// Shot: double line.
    Shot,
    /// Lateral / crossovers: zig-zag line.
    Lateral,
}

impl LineStyle {
    pub const ALL: [LineStyle; 6] = [
        LineStyle::Skate,
        LineStyle::Backward,
        LineStyle::Carry,
        LineStyle::Pass,
        LineStyle::Shot,
        LineStyle::Lateral,
    ];

    pub fn key(self) -> &'static str {
        match self {
            LineStyle::Skate => "skate",
            LineStyle::Backward => "backward",
            LineStyle::Carry => "carry",
            LineStyle::Pass => "pass",
            LineStyle::Shot => "shot",
            LineStyle::Lateral => "lateral",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineStyle::Skate => "Skate",
            LineStyle::Backward => "Skate backward",
            LineStyle::Carry => "Carry puck",
            LineStyle::Pass => "Pass",
            LineStyle::Shot => "Shot",
            LineStyle::Lateral => "Crossovers / lateral",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnd {
    #[default]
    Arrow,
    /// A hard stop: a bar across the end.
    Stop,
    None,
}

impl Diagram {
    /// Parse and check a diagram (from the database or a form).
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let json = json.trim();
        if json.is_empty() {
            return Ok(Self::default());
        }
        let d: Diagram = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("The diagram couldn't be read: {e}"))?;
        d.validate()?;
        Ok(d)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"items":[]}"#.into())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.items.len() <= MAX_ITEMS,
            "A diagram can hold at most {MAX_ITEMS} items."
        );
        let ok = |v: f32| v.is_finite() && (-50.0..=250.0).contains(&v);
        for item in &self.items {
            match item {
                Item::Line { pts, .. } => {
                    anyhow::ensure!(
                        (2..=MAX_POINTS).contains(&pts.len()),
                        "A line needs 2 to {MAX_POINTS} points."
                    );
                    anyhow::ensure!(
                        pts.iter().all(|p| ok(p[0]) && ok(p[1])),
                        "A line has a point outside the rink."
                    );
                }
                Item::Player { label, .. } | Item::Text { text: label, .. } => {
                    anyhow::ensure!(
                        label.chars().count() <= 60,
                        "Diagram labels are at most 60 characters."
                    );
                }
                Item::Net { rot, .. } => anyhow::ensure!(rot.is_finite(), "A net has a bad angle."),
                _ => {}
            }
            let (x, y) = item.anchor();
            anyhow::ensure!(ok(x) && ok(y), "An item is outside the rink.");
        }
        Ok(())
    }

    /// How many of each equipment item the diagram shows: (cones, pucks, nets, tires).
    pub fn equipment_counts(&self) -> (u32, u32, u32, u32) {
        let mut counts = (0, 0, 0, 0);
        for item in &self.items {
            match item {
                Item::Cone { .. } => counts.0 += 1,
                Item::Puck { .. } => counts.1 += 1,
                Item::Pucks { .. } => counts.1 += 10,
                Item::Net { .. } => counts.2 += 1,
                Item::Tire { .. } => counts.3 += 1,
                _ => {}
            }
        }
        counts
    }

    /// Points the diagram occupies (item positions and points along lines).
    pub fn points(&self) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for item in &self.items {
            match item {
                Item::Line { pts, .. } => {
                    let pts: Vec<(f32, f32)> = pts.iter().map(|p| (p[0], p[1])).collect();
                    out.extend(smooth(&pts).into_iter().step_by(4));
                }
                other => out.push(other.anchor()),
            }
        }
        out
    }

    /// Draw every item into `out`, moved onto a slot with `xf`.
    pub fn render(&self, out: &mut String, xf: Xf) {
        out.push_str(r#"<g class="drill">"#);
        // Lines under the symbols.
        for item in self.items.iter().filter(|i| matches!(i, Item::Line { .. })) {
            item.render(out, xf);
        }
        for item in self
            .items
            .iter()
            .filter(|i| !matches!(i, Item::Line { .. }))
        {
            item.render(out, xf);
        }
        out.push_str("</g>");
    }
}

impl Item {
    fn anchor(&self) -> (f32, f32) {
        match self {
            Item::Player { x, y, .. }
            | Item::Cone { x, y }
            | Item::Puck { x, y }
            | Item::Pucks { x, y }
            | Item::Tire { x, y }
            | Item::Net { x, y, .. }
            | Item::Text { x, y, .. } => (*x, *y),
            Item::Line { pts, .. } => pts.first().map(|p| (p[0], p[1])).unwrap_or((0.0, 0.0)),
        }
    }

    pub fn render(&self, out: &mut String, xf: Xf) {
        match self {
            Item::Player { x, y, label } => {
                let (x, y) = xf.apply(*x, *y);
                let label = if label.trim().is_empty() {
                    "X"
                } else {
                    label.trim()
                };
                text(
                    out,
                    "d-player",
                    x,
                    y + 1.8,
                    5.0,
                    700,
                    c::INK,
                    "middle",
                    label,
                );
            }
            Item::Cone { x, y } => {
                let (x, y) = xf.apply(*x, *y);
                let d = format!(
                    "M {} {} L {} {} L {} {} Z",
                    n(x),
                    n(y - 1.6),
                    n(x + 1.4),
                    n(y + 1.1),
                    n(x - 1.4),
                    n(y + 1.1)
                );
                let _ = write!(
                    out,
                    r#"<path class="d-cone" d="{d}" fill="{}" stroke="{}" stroke-width="0.2"/>"#,
                    c::CONE,
                    c::INK
                );
            }
            Item::Puck { x, y } => {
                let (x, y) = xf.apply(*x, *y);
                puck(out, x, y);
            }
            Item::Pucks { x, y } => {
                let (x, y) = xf.apply(*x, *y);
                for (dx, dy) in [(-1.1, 0.6), (1.1, 0.6), (0.0, -0.9), (0.0, 1.5)] {
                    puck(out, x + dx, y + dy);
                }
            }
            Item::Tire { x, y } => {
                let (x, y) = xf.apply(*x, *y);
                let _ = write!(
                    out,
                    r#"<circle class="d-tire" cx="{}" cy="{}" r="1.8" fill="none" stroke="{}" stroke-width="0.7"/>"#,
                    n(x),
                    n(y),
                    c::INK
                );
            }
            Item::Net { x, y, rot } => {
                let (x, y) = xf.apply(*x, *y);
                net(out, x, y, xf.angle(*rot));
            }
            Item::Text { x, y, text: t } => {
                let (x, y) = xf.apply(*x, *y);
                text(out, "d-text", x, y + 1.0, 3.2, 600, c::INK, "middle", t);
            }
            Item::Line { style, end, pts } => {
                let pts: Vec<(f32, f32)> = pts.iter().map(|p| xf.apply(p[0], p[1])).collect();
                line(out, *style, *end, &pts);
            }
        }
    }
}

fn puck(out: &mut String, x: f32, y: f32) {
    let _ = write!(
        out,
        r#"<circle class="d-puck" cx="{}" cy="{}" r="0.75" fill="{}"/>"#,
        n(x),
        n(y),
        c::INK
    );
}

/// A net whose mouth faces `deg` degrees: a 6 ft wide, 3.3 ft deep cage.
fn net(out: &mut String, x: f32, y: f32, deg: f32) {
    let (s, co) = deg.to_radians().sin_cos();
    // Local coordinates: mouth along +u at u=0, back at u=-3.3, width along v.
    let p = |u: f32, v: f32| (x + u * co - v * s, y + u * s + v * co);
    let pts = [
        p(0.0, -3.0),
        p(-2.6, -2.6),
        p(-3.3, 0.0),
        p(-2.6, 2.6),
        p(0.0, 3.0),
    ];
    let d = format!("{} Z", polyline_d(&pts));
    let _ = write!(
        out,
        r#"<path class="d-net" d="{d}" fill="{}" stroke="{}" stroke-width="0.35" stroke-linejoin="round"/>"#,
        c::NET,
        c::INK
    );
    let (a, b) = (p(0.0, -3.0), p(0.0, 3.0));
    let _ = write!(
        out,
        r#"<path class="d-red" d="M {} {} L {} {}" stroke="{}" stroke-width="0.35"/>"#,
        n(a.0),
        n(a.1),
        n(b.0),
        n(b.1),
        c::RED
    );
}

// ---- Movement lines ----------------------------------------------------------

/// Points along a smooth curve through `pts` (Catmull-Rom), about every 0.5 ft.
pub fn smooth(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut dense = Vec::new();
    for i in 0..pts.len() - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < pts.len() {
            pts[i + 2]
        } else {
            pts[i + 1]
        };
        let steps = 16;
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            dense.push((f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    dense.push(*pts.last().unwrap());
    resample(&dense, 0.5)
}

/// Evenly spaced points (`step` ft apart) along a polyline, keeping both ends.
fn resample(pts: &[(f32, f32)], step: f32) -> Vec<(f32, f32)> {
    let mut out = vec![pts[0]];
    let mut carry = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        if len == 0.0 {
            continue;
        }
        let mut d = step - carry;
        while d <= len {
            let t = d / len;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            d += step;
        }
        carry = len - (d - step);
    }
    let last = *pts.last().unwrap();
    if out.last() != Some(&last) {
        out.push(last);
    }
    out
}

/// Unit normal at point `i` of a sampled curve.
fn normal(pts: &[(f32, f32)], i: usize) -> (f32, f32) {
    let a = pts[i.saturating_sub(1)];
    let b = pts[(i + 1).min(pts.len() - 1)];
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    (-dy / len, dx / len)
}

fn offset(pts: &[(f32, f32)], by: impl Fn(usize) -> f32) -> Vec<(f32, f32)> {
    (0..pts.len())
        .map(|i| {
            let (nx, ny) = normal(pts, i);
            let o = by(i);
            (pts[i].0 + nx * o, pts[i].1 + ny * o)
        })
        .collect()
}

const STROKE: f32 = 0.4;

fn stroke_path(out: &mut String, d: &str, extra: &str) {
    let _ = write!(
        out,
        r#"<path class="d-line" d="{d}" fill="none" stroke="{}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"{extra}/>"#,
        c::INK,
        n(STROKE)
    );
}

/// Draw a movement line through `pts` (already on the rink).
pub fn line(out: &mut String, style: LineStyle, end: LineEnd, pts: &[(f32, f32)]) {
    if pts.len() < 2 {
        return;
    }
    let path = smooth(pts);
    let count = path.len();
    // Leave the last ~2 ft plain so the arrow head sits on a clean line.
    let plain_from = count.saturating_sub(5);
    match style {
        LineStyle::Skate => stroke_path(out, &polyline_d(&path), ""),
        LineStyle::Pass => stroke_path(out, &polyline_d(&path), r#" stroke-dasharray="1.6 1.2""#),
        LineStyle::Shot => {
            for side in [-0.55f32, 0.55] {
                let o = offset(&path, |_| side);
                stroke_path(out, &polyline_d(&o[..plain_from.max(2)]), "");
            }
        }
        LineStyle::Carry => {
            let w = offset(&path, |i| {
                if i >= plain_from {
                    0.0
                } else {
                    0.8 * (i as f32 * 0.5 * std::f32::consts::TAU / 3.0).sin()
                }
            });
            stroke_path(out, &polyline_d(&w), "");
        }
        LineStyle::Lateral => {
            let z = offset(&path, |i| {
                if i >= plain_from {
                    0.0
                } else {
                    // Triangle wave, 3 ft period.
                    let phase = (i % 6) as f32 / 6.0;
                    0.9 * (1.0 - 4.0 * (phase - 0.5).abs())
                }
            });
            stroke_path(out, &polyline_d(&z), "");
        }
        LineStyle::Backward => {
            stroke_path(out, &polyline_d(&path), "");
            let mut d = String::new();
            for i in (4..plain_from).step_by(6) {
                let (nx, ny) = normal(&path, i);
                let (x, y) = path[i];
                let _ = write!(
                    d,
                    "M {} {} L {} {} ",
                    n(x - nx),
                    n(y - ny),
                    n(x + nx),
                    n(y + ny)
                );
            }
            if !d.is_empty() {
                stroke_path(out, d.trim_end(), "");
            }
        }
    }
    let tip = path[count - 1];
    let back = path[count.saturating_sub(4)];
    let (dx, dy) = (tip.0 - back.0, tip.1 - back.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    match end {
        LineEnd::Arrow => {
            let (bx, by) = (tip.0 - ux * 2.2, tip.1 - uy * 2.2);
            let d = format!(
                "M {} {} L {} {} L {} {} Z",
                n(tip.0),
                n(tip.1),
                n(bx - uy * 1.0),
                n(by + ux * 1.0),
                n(bx + uy * 1.0),
                n(by - ux * 1.0)
            );
            let _ = write!(out, r#"<path class="d-head" d="{d}" fill="{}"/>"#, c::INK);
        }
        LineEnd::Stop => {
            let d = format!(
                "M {} {} L {} {}",
                n(tip.0 - uy * 1.4),
                n(tip.1 + ux * 1.4),
                n(tip.0 + uy * 1.4),
                n(tip.1 - ux * 1.4)
            );
            let _ = write!(
                out,
                r#"<path class="d-line" d="{d}" fill="none" stroke="{}" stroke-width="0.7"/>"#,
                c::INK
            );
        }
        LineEnd::None => {}
    }
}

/// A small sample of each line style, for legends: `(label, svg)`.
pub fn legend() -> Vec<(&'static str, String)> {
    LineStyle::ALL
        .iter()
        .map(|s| {
            let mut out = String::new();
            line(&mut out, *s, LineEnd::Arrow, &[(1.0, 2.0), (13.0, 2.0)]);
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 4" class="legend-line" role="img" aria-label="{}">{out}</svg>"#,
                esc(s.label())
            );
            (s.label(), svg)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_checks() {
        let json = r#"{"items":[
            {"t":"player","x":30,"y":20,"label":"X"},
            {"t":"net","x":40,"y":40},
            {"t":"line","style":"carry","pts":[[30,20],[50,30],[60,15]]}
        ]}"#;
        let d = Diagram::from_json(json).unwrap();
        assert_eq!(d.items.len(), 3);
        assert_eq!(Diagram::from_json(&d.to_json()).unwrap(), d);
        assert!(Diagram::from_json("").unwrap().items.is_empty());
        assert!(Diagram::from_json(r#"{"items":[{"t":"cone","x":1e9,"y":0}]}"#).is_err());
        assert!(
            Diagram::from_json(r#"{"items":[{"t":"line","style":"skate","pts":[[1,1]]}]}"#)
                .is_err()
        );
        assert!(Diagram::from_json("nonsense").is_err());
    }

    #[test]
    fn every_style_renders() {
        for style in LineStyle::ALL {
            for end in [LineEnd::Arrow, LineEnd::Stop, LineEnd::None] {
                let mut out = String::new();
                line(
                    &mut out,
                    style,
                    end,
                    &[(0.0, 0.0), (20.0, 10.0), (30.0, 0.0)],
                );
                assert!(out.contains("<path"), "{style:?}");
                assert!(!out.contains("NaN"), "{style:?}");
            }
        }
        // Degenerate lines don't panic.
        let mut out = String::new();
        line(
            &mut out,
            LineStyle::Carry,
            LineEnd::Arrow,
            &[(5.0, 5.0), (5.0, 5.0)],
        );
    }

    #[test]
    fn counts_equipment() {
        let d = Diagram {
            items: vec![
                Item::Cone { x: 1.0, y: 1.0 },
                Item::Cone { x: 2.0, y: 1.0 },
                Item::Pucks { x: 3.0, y: 3.0 },
                Item::Net {
                    x: 3.0,
                    y: 3.0,
                    rot: 0.0,
                },
            ],
        };
        assert_eq!(d.equipment_counts(), (2, 10, 1, 0));
    }
}
