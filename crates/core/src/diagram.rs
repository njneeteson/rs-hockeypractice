//! A drill's diagram: players, equipment and movement lines, drawn on the
//! drill's canonical frame (see [`crate::rink`]).
//!
//! Stored as JSON. The browser editor (`assets/planner.js`) reads and writes
//! the same format and draws it the same way; keep the two in step.
//!
//! ```json
//! {"items": [
//!   {"t": "player", "x": 30, "y": 20, "label": "X"},
//!   {"t": "net", "x": 11, "y": 42.5, "rot": 0},
//!   {"t": "line", "style": "skate", "end": "arrow", "pts": [[30, 20], [50, 30], [60, 15]]}
//! ]}
//! ```
//!
//! Symbols follow the usual drill-sheet conventions (Hockey Canada, IIHF):
//! coach and goalie letters are circled, a pass is dashed, a shot is a double
//! line, backward skating is a row of cups, lateral steps are ticks,
//! crossovers are X's and a stop is two bars across the end of a line.

use crate::drill::Equipment;
use crate::rink::Xf;
use crate::svg::{colour as c, esc, n, polyline_d, text};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
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

/// Rotatable items carry `rot`: the direction they face, in degrees
/// (0° points along +x, 90° along +y, i.e. down the screen).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Item {
    /// A player or coach, drawn as its label (X, O, C, G, F1, …). Coaches and
    /// goalies (C, G, C2, …) are circled.
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
    /// A net; `rot` is the direction its mouth faces.
    Net {
        x: f32,
        y: f32,
        #[serde(default)]
        rot: f32,
    },
    /// A foam barrier, 8 ft long; `rot` is the direction of its length.
    Barrier {
        x: f32,
        y: f32,
        #[serde(default)]
        rot: f32,
    },
    /// A stickhandling dangler: a raised bar on two feet, 4 ft long.
    Dangler {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineStyle {
    /// Forward skating: solid line.
    Skate,
    /// Skating with the puck: wavy line.
    Carry,
    /// Pass: dashed line.
    Pass,
    /// Shot: double line.
    Shot,
    /// Backward skating: a row of cups.
    Backward,
    /// Backward skating with the puck: cups with a dot between them.
    BackCarry,
    /// Lateral steps (side-steps): ticks across the path.
    Lateral,
    /// Crossovers: a row of X's.
    Crossover,
    /// Goalie pad slide: a loop, a bump, then a line.
    PadSlide,
    /// Goalie butterfly slide: a loop, then a line.
    Butterfly,
}

impl LineStyle {
    pub const ALL: [LineStyle; 10] = [
        LineStyle::Skate,
        LineStyle::Carry,
        LineStyle::Pass,
        LineStyle::Shot,
        LineStyle::Backward,
        LineStyle::BackCarry,
        LineStyle::Lateral,
        LineStyle::Crossover,
        LineStyle::PadSlide,
        LineStyle::Butterfly,
    ];

    pub fn key(self) -> &'static str {
        match self {
            LineStyle::Skate => "skate",
            LineStyle::Carry => "carry",
            LineStyle::Pass => "pass",
            LineStyle::Shot => "shot",
            LineStyle::Backward => "backward",
            LineStyle::BackCarry => "backcarry",
            LineStyle::Lateral => "lateral",
            LineStyle::Crossover => "crossover",
            LineStyle::PadSlide => "padslide",
            LineStyle::Butterfly => "butterfly",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineStyle::Skate => "Skate",
            LineStyle::Carry => "Skate with puck",
            LineStyle::Pass => "Pass",
            LineStyle::Shot => "Shot",
            LineStyle::Backward => "Skate backward",
            LineStyle::BackCarry => "Backward with puck",
            LineStyle::Lateral => "Lateral steps",
            LineStyle::Crossover => "Crossovers",
            LineStyle::PadSlide => "Goalie pad slide",
            LineStyle::Butterfly => "Goalie butterfly slide",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnd {
    #[default]
    Arrow,
    /// A hard stop: two bars across the end.
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
                Item::Net { rot, .. } | Item::Barrier { rot, .. } | Item::Dangler { rot, .. } => {
                    anyhow::ensure!(rot.is_finite(), "An item has a bad angle.")
                }
                _ => {}
            }
            let (x, y) = item.anchor();
            anyhow::ensure!(ok(x) && ok(y), "An item is outside the rink.");
        }
        Ok(())
    }

    /// How much equipment the diagram shows (a pile of pucks counts as 10).
    pub fn equipment_counts(&self) -> Vec<(Equipment, u32)> {
        let mut counts: Vec<(Equipment, u32)> = Vec::new();
        let mut add = |e: Equipment, q: u32| match counts.iter_mut().find(|(k, _)| *k == e) {
            Some((_, n)) => *n += q,
            None => counts.push((e, q)),
        };
        for item in &self.items {
            match item {
                Item::Cone { .. } => add(Equipment::Cones, 1),
                Item::Puck { .. } => add(Equipment::Pucks, 1),
                Item::Pucks { .. } => add(Equipment::Pucks, 10),
                Item::Net { .. } => add(Equipment::Nets, 1),
                Item::Tire { .. } => add(Equipment::Tires, 1),
                Item::Barrier { .. } => add(Equipment::Barriers, 1),
                Item::Dangler { .. } => add(Equipment::Danglers, 1),
                _ => {}
            }
        }
        counts.sort_by_key(|(e, _)| *e);
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
        // Nets and barriers first, then lines, then players and small gear.
        for pass in 0..3 {
            for item in self.items.iter().filter(|i| i.layer() == pass) {
                item.render(out, xf);
            }
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
            | Item::Barrier { x, y, .. }
            | Item::Dangler { x, y, .. }
            | Item::Text { x, y, .. } => (*x, *y),
            Item::Line { pts, .. } => pts.first().map(|p| (p[0], p[1])).unwrap_or((0.0, 0.0)),
        }
    }

    /// Drawing order: 0 = large equipment, 1 = lines, 2 = everything else.
    fn layer(&self) -> u8 {
        match self {
            Item::Net { .. } | Item::Barrier { .. } | Item::Dangler { .. } => 0,
            Item::Line { .. } => 1,
            _ => 2,
        }
    }

    pub fn render(&self, out: &mut String, xf: Xf) {
        match self {
            Item::Player { x, y, label } => {
                let (x, y) = xf.apply(*x, *y);
                player(out, x, y, label);
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
            Item::Barrier { x, y, rot } => {
                let (x, y) = xf.apply(*x, *y);
                barrier(out, x, y, xf.angle(*rot));
            }
            Item::Dangler { x, y, rot } => {
                let (x, y) = xf.apply(*x, *y);
                dangler(out, x, y, xf.angle(*rot));
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

/// Whether a player label is drawn in a circle: coaches and goalies (C, G, C2, …).
pub fn circled(label: &str) -> bool {
    let mut chars = label.chars();
    matches!(chars.next(), Some('C' | 'G')) && chars.all(|c| c.is_ascii_digit())
}

fn player(out: &mut String, x: f32, y: f32, label: &str) {
    let label = match label.trim() {
        "" => "X",
        l => l,
    };
    if circled(label) {
        let r = 2.6 + 0.9 * (label.chars().count() as f32 - 1.0);
        let _ = write!(
            out,
            r#"<circle class="d-ring" cx="{}" cy="{}" r="{}" fill="{}" stroke="{}" stroke-width="0.35"/>"#,
            n(x),
            n(y),
            n(r),
            c::ICE,
            c::INK
        );
        text(
            out,
            "d-player",
            x,
            y + 1.45,
            4.0,
            700,
            c::INK,
            "middle",
            label,
        );
    } else {
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

/// Local coordinates for a rotated item at (x, y): `u` along `deg`, `v` across.
fn local(x: f32, y: f32, deg: f32) -> impl Fn(f32, f32) -> (f32, f32) {
    let (s, co) = deg.to_radians().sin_cos();
    move |u: f32, v: f32| (x + u * co - v * s, y + u * s + v * co)
}

// Net shape: a 6 ft mouth between the posts, a base that flares slightly and
// rounds off 3.4 ft back (a superellipse).
const NET_DEPTH: f32 = 3.4;
const NET_HALF: f32 = 3.3;
const NET_FLARE: f32 = 0.5;
const NET_P: f32 = 2.6;

/// How far back the net's base is at `v` across.
fn net_back(v: f32) -> f32 {
    let t = (v.abs() / NET_HALF).min(1.0);
    -NET_FLARE - (NET_DEPTH - NET_FLARE) * (1.0 - t.powf(NET_P)).max(0.0).powf(1.0 / NET_P)
}

/// How far across the base reaches `u` back from the mouth.
fn net_side(u: f32) -> f32 {
    let g = ((-u - NET_FLARE) / (NET_DEPTH - NET_FLARE)).clamp(0.0, 1.0);
    NET_HALF * (1.0 - g.powf(NET_P)).max(0.0).powf(1.0 / NET_P)
}

/// A net seen from above whose mouth faces `deg` degrees: red frame, white
/// mesh with grey netting, a crossbar and posts.
fn net(out: &mut String, x: f32, y: f32, deg: f32) {
    let p = local(x, y, deg);
    let mut outline = vec![p(0.0, -3.0)];
    for k in 0..=20 {
        let v = -NET_HALF + 2.0 * NET_HALF * k as f32 / 20.0;
        outline.push(p(net_back(v), v));
    }
    outline.push(p(0.0, 3.0));
    let _ = write!(
        out,
        r#"<path class="d-net" d="{} Z" fill="{}" stroke="{}" stroke-width="0.3" stroke-linejoin="round"/>"#,
        polyline_d(&outline),
        c::NET,
        c::RED
    );
    let mut mesh = String::new();
    for v in [-2.2f32, -1.1, 0.0, 1.1, 2.2] {
        let (a, b) = (p(0.0, v), p(net_back(v), v));
        let _ = write!(mesh, "M {} {} L {} {} ", n(a.0), n(a.1), n(b.0), n(b.1));
    }
    for u in [-1.2f32, -2.3] {
        let s = net_side(u);
        let (a, b) = (p(u, -s), p(u, s));
        let _ = write!(mesh, "M {} {} L {} {} ", n(a.0), n(a.1), n(b.0), n(b.1));
    }
    let _ = write!(
        out,
        r#"<path class="d-mesh" d="{}" fill="none" stroke="{}" stroke-width="0.12"/>"#,
        mesh.trim_end(),
        c::NET_MESH
    );
    let (a, b) = (p(0.0, -3.0), p(0.0, 3.0));
    let _ = write!(
        out,
        r#"<path class="d-red" d="M {} {} L {} {}" stroke="{}" stroke-width="0.6" stroke-linecap="round"/>"#,
        n(a.0),
        n(a.1),
        n(b.0),
        n(b.1),
        c::RED
    );
    for post in [a, b] {
        let _ = write!(
            out,
            r#"<circle class="d-red-fill" cx="{}" cy="{}" r="0.4" fill="{}"/>"#,
            n(post.0),
            n(post.1),
            c::RED
        );
    }
}

/// An 8 ft foam barrier along `deg`, striped like a safety pad.
fn barrier(out: &mut String, x: f32, y: f32, deg: f32) {
    let p = local(x, y, deg);
    let body = [p(-4.0, -0.55), p(4.0, -0.55), p(4.0, 0.55), p(-4.0, 0.55)];
    let _ = write!(
        out,
        r#"<path class="d-barrier" d="{} Z" fill="{}" stroke="{}" stroke-width="0.2" stroke-linejoin="round"/>"#,
        polyline_d(&body),
        c::BARRIER,
        c::INK
    );
    let mut stripes = String::new();
    for k in -3..=3 {
        let u = k as f32 * 1.1;
        let (a, b) = (p(u - 0.45, 0.55), p(u + 0.45, -0.55));
        let _ = write!(stripes, "M {} {} L {} {} ", n(a.0), n(a.1), n(b.0), n(b.1));
    }
    let _ = write!(
        out,
        r#"<path class="d-stripe" d="{}" fill="none" stroke="{}" stroke-width="0.2"/>"#,
        stripes.trim_end(),
        c::INK
    );
}

/// A 4 ft dangler along `deg`: a raised bar between two feet.
fn dangler(out: &mut String, x: f32, y: f32, deg: f32) {
    let p = local(x, y, deg);
    let bar = [p(-2.0, -0.3), p(2.0, -0.3), p(2.0, 0.3), p(-2.0, 0.3)];
    let _ = write!(
        out,
        r#"<path class="d-dangler" d="{} Z" fill="{}" stroke="{}" stroke-width="0.15"/>"#,
        polyline_d(&bar),
        c::DANGLER,
        c::INK
    );
    let mut feet = String::new();
    for u in [-2.0f32, 2.0] {
        let (a, b) = (p(u, -1.0), p(u, 1.0));
        let _ = write!(feet, "M {} {} L {} {} ", n(a.0), n(a.1), n(b.0), n(b.1));
    }
    let _ = write!(
        out,
        r#"<path class="d-foot" d="{}" fill="none" stroke="{}" stroke-width="0.5" stroke-linecap="round"/>"#,
        feet.trim_end(),
        c::INK
    );
}

// ---- Movement lines ----------------------------------------------------------

/// Spacing of the points along a smoothed line, in feet.
const STEP: f32 = 0.5;

/// Points along a smooth curve through `pts` (Catmull-Rom), every [`STEP`] ft.
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
    resample(&dense, STEP)
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

/// The point and unit normal at fractional index `s` of a sampled curve.
fn at(pts: &[(f32, f32)], s: f32) -> ((f32, f32), (f32, f32)) {
    let last = pts.len() - 1;
    let s = s.clamp(0.0, last as f32);
    let i = (s.floor() as usize).min(last.saturating_sub(1));
    let t = s - i as f32;
    let (a, b) = (pts[i], pts[(i + 1).min(last)]);
    let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    (p, normal(pts, if t < 0.5 { i } else { i + 1 }))
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

/// `M a L b` for each segment.
/// A straight segment between two points.
type Seg = ((f32, f32), (f32, f32));

fn segments_d(segs: &[Seg]) -> String {
    let mut d = String::new();
    for (a, b) in segs {
        let _ = write!(d, "M {} {} L {} {} ", n(a.0), n(a.1), n(b.0), n(b.1));
    }
    d.trim_end().to_string()
}

/// Cups along the path (backward skating): 2 ft cups every 3 ft, up to `until`.
fn cups(out: &mut String, path: &[(f32, f32)], until: usize, dots: bool) {
    let mut start = 0;
    while start + 4 <= until {
        let unit: Vec<(f32, f32)> = (0..=8)
            .map(|k| {
                let u = k as f32 / 8.0;
                let (p, nn) = at(path, start as f32 + 4.0 * u);
                let o = 0.8 * (PI * u).sin();
                (p.0 + nn.0 * o, p.1 + nn.1 * o)
            })
            .collect();
        stroke_path(out, &polyline_d(&unit), "");
        if dots && start + 5 < until {
            let (p, _) = at(path, start as f32 + 5.0);
            let _ = write!(
                out,
                r#"<circle class="d-dot" cx="{}" cy="{}" r="0.35" fill="{}"/>"#,
                n(p.0),
                n(p.1),
                c::INK
            );
        }
        start += 6;
    }
}

/// A small loop at the start of a goalie slide.
fn curl(out: &mut String, path: &[(f32, f32)]) {
    let (p, nn) = at(path, 0.0);
    let r = 0.7;
    let _ = write!(
        out,
        r#"<circle class="d-line" cx="{}" cy="{}" r="{}" fill="none" stroke="{}" stroke-width="{}"/>"#,
        n(p.0 - nn.0 * r),
        n(p.1 - nn.1 * r),
        n(r),
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
    // Leave the last ~2 ft plain so the line end sits on a clean line.
    let plain_from = count.saturating_sub(5);
    let tail = |out: &mut String| {
        if count - plain_from >= 2 {
            stroke_path(out, &polyline_d(&path[plain_from..]), "");
        }
    };
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
            // A 3 ft wave, sampled finely, easing into the plain end.
            let mut w: Vec<(f32, f32)> = (0..=plain_from * 4)
                .map(|k| {
                    let s = k as f32 / 4.0;
                    let ((x, y), (nx, ny)) = at(&path, s);
                    let fade = ((plain_from as f32 - s) / 2.0).min(1.0);
                    let o = 0.8 * fade * (s * STEP * 2.0 * PI / 3.0).sin();
                    (x + nx * o, y + ny * o)
                })
                .collect();
            w.extend_from_slice(&path[plain_from + 1..]);
            stroke_path(out, &polyline_d(&w), "");
        }
        LineStyle::Backward | LineStyle::BackCarry => {
            cups(out, &path, plain_from, style == LineStyle::BackCarry);
            tail(out);
        }
        LineStyle::Lateral => {
            let ticks: Vec<_> = (1..plain_from)
                .step_by(2)
                .map(|i| {
                    let ((x, y), (nx, ny)) = (path[i], normal(&path, i));
                    ((x - nx, y - ny), (x + nx, y + ny))
                })
                .collect();
            if !ticks.is_empty() {
                stroke_path(out, &segments_d(&ticks), "");
            }
            tail(out);
        }
        LineStyle::Crossover => {
            let mut arms = Vec::new();
            let mut s = 2;
            while s + 1 < plain_from {
                let ((x, y), (nx, ny)) = (path[s], normal(&path, s));
                let (tx, ty) = (ny, -nx);
                let a = 0.6;
                arms.push((
                    (x - (tx + nx) * a, y - (ty + ny) * a),
                    (x + (tx + nx) * a, y + (ty + ny) * a),
                ));
                arms.push((
                    (x - (tx - nx) * a, y - (ty - ny) * a),
                    (x + (tx - nx) * a, y + (ty - ny) * a),
                ));
                s += 4;
            }
            if !arms.is_empty() {
                stroke_path(out, &segments_d(&arms), "");
            }
            tail(out);
        }
        LineStyle::PadSlide | LineStyle::Butterfly => {
            curl(out, &path);
            let bump = style == LineStyle::PadSlide;
            let w = offset(&path, |i| {
                if bump && (2..=8).contains(&i) {
                    0.9 * (PI * (i - 2) as f32 / 6.0).sin()
                } else {
                    0.0
                }
            });
            stroke_path(out, &polyline_d(&w), "");
        }
    }
    line_end(out, end, &path);
}

fn line_end(out: &mut String, end: LineEnd, path: &[(f32, f32)]) {
    let count = path.len();
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
            let bars: Vec<_> = [0.0f32, 0.9]
                .iter()
                .map(|b| {
                    let (x, y) = (tip.0 - ux * b, tip.1 - uy * b);
                    ((x - uy * 1.4, y + ux * 1.4), (x + uy * 1.4, y - ux * 1.4))
                })
                .collect();
            let _ = write!(
                out,
                r#"<path class="d-line" d="{}" fill="none" stroke="{}" stroke-width="0.5"/>"#,
                segments_d(&bars),
                c::INK
            );
        }
        LineEnd::None => {}
    }
}

// ---- Legends -------------------------------------------------------------------

/// Size of a legend icon's drawing area, in feet (`0 0 W H`).
pub const LEGEND_W: f32 = 14.0;
pub const LEGEND_H: f32 = 6.0;

/// One row of a legend: an icon (SVG content in a `0 0 14 6` box) and what it means.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendEntry {
    pub icon: String,
    pub label: String,
}

impl LegendEntry {
    /// The icon as a standalone `<svg>` (for web pages).
    pub fn svg(&self) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {LEGEND_W} {LEGEND_H}" class="legend-icon" aria-hidden="true">{}</svg>"#,
            self.icon
        )
    }
}

/// What a player letter means; labels such as F1, F2 share their letter's meaning.
fn player_meaning(label: &str) -> Option<&'static str> {
    let mut chars = label.chars();
    let first = chars.next()?;
    if !chars.all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(match first {
        'X' => "Player",
        'O' => "Player, other group",
        'F' => "Forward",
        'D' => "Defence",
        'G' => "Goalie",
        'C' => "Coach",
        _ => return None,
    })
}

const PLAYER_ORDER: [&str; 6] = ["X", "O", "F", "D", "G", "C"];

fn icon(item: &Item) -> String {
    let mut out = String::new();
    item.render(&mut out, Xf::IDENTITY);
    out
}

fn line_icon(style: LineStyle, end: LineEnd) -> String {
    let mut out = String::new();
    line(&mut out, style, end, &[(1.0, 3.0), (13.0, 3.0)]);
    out
}

fn equipment_entries() -> Vec<(&'static str, &'static str, Item)> {
    let (x, y) = (LEGEND_W / 2.0, LEGEND_H / 2.0);
    vec![
        (
            "net",
            "Net",
            Item::Net {
                x,
                y: y + 1.9,
                rot: 90.0,
            },
        ),
        ("cone", "Cone", Item::Cone { x, y: y + 0.2 }),
        ("puck", "Puck", Item::Puck { x, y }),
        ("pucks", "Pile of pucks", Item::Pucks { x, y: y - 0.3 }),
        ("tire", "Tire", Item::Tire { x, y }),
        ("barrier", "Barrier", Item::Barrier { x, y, rot: 0.0 }),
        ("dangler", "Dangler", Item::Dangler { x, y, rot: 0.0 }),
    ]
}

fn kind(item: &Item) -> &'static str {
    match item {
        Item::Player { .. } => "player",
        Item::Cone { .. } => "cone",
        Item::Puck { .. } => "puck",
        Item::Pucks { .. } => "pucks",
        Item::Tire { .. } => "tire",
        Item::Net { .. } => "net",
        Item::Barrier { .. } => "barrier",
        Item::Dangler { .. } => "dangler",
        Item::Text { .. } => "text",
        Item::Line { .. } => "line",
    }
}

/// The legend for everything drawn in `diagrams`: one entry per kind of
/// player, piece of equipment and line style used (text labels explain
/// themselves and are left out).
pub fn legend<'a>(diagrams: impl IntoIterator<Item = &'a Diagram>) -> Vec<LegendEntry> {
    // Player groups: letter → labels seen; other labels on their own.
    let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
    let mut others: Vec<String> = Vec::new();
    let mut kinds: Vec<&'static str> = Vec::new();
    let mut styles: Vec<LineStyle> = Vec::new();
    let mut stop = false;
    for d in diagrams {
        for item in &d.items {
            match item {
                Item::Player { label, .. } => {
                    let label = match label.trim() {
                        "" => "X",
                        l => l,
                    };
                    match player_meaning(label) {
                        Some(_) => {
                            let letter = PLAYER_ORDER
                                .iter()
                                .find(|l| label.starts_with(**l))
                                .copied()
                                .unwrap_or("X");
                            match groups.iter_mut().find(|(l, _)| *l == letter) {
                                Some((_, seen)) => {
                                    if !seen.iter().any(|s| s == label) {
                                        seen.push(label.to_string());
                                    }
                                }
                                None => groups.push((letter, vec![label.to_string()])),
                            }
                        }
                        None => {
                            if !others.iter().any(|o| o == label) {
                                others.push(label.to_string());
                            }
                        }
                    }
                }
                Item::Line { style, end, .. } => {
                    if !styles.contains(style) {
                        styles.push(*style);
                    }
                    stop |= *end == LineEnd::Stop;
                }
                other => {
                    let k = kind(other);
                    if !kinds.contains(&k) {
                        kinds.push(k);
                    }
                }
            }
        }
    }

    let (cx, cy) = (LEGEND_W / 2.0, LEGEND_H / 2.0);
    let mut out = Vec::new();
    for letter in PLAYER_ORDER {
        let Some((_, mut seen)) = groups.iter().find(|(l, _)| *l == letter).cloned() else {
            continue;
        };
        seen.sort();
        let meaning = player_meaning(letter).unwrap_or("Player");
        let label = if seen.len() == 1 && seen[0] == letter {
            meaning.to_string()
        } else {
            let mut shown: Vec<&str> = seen.iter().map(String::as_str).take(4).collect();
            if seen.len() > 4 {
                shown.push("…");
            }
            format!("{meaning} ({})", shown.join(", "))
        };
        let sample = Item::Player {
            x: cx,
            y: cy,
            label: if seen.iter().any(|s| s == letter) {
                letter.to_string()
            } else {
                seen[0].clone()
            },
        };
        out.push(LegendEntry {
            icon: icon(&sample),
            label,
        });
    }
    others.sort();
    for label in others {
        let short: String = label.chars().take(4).collect();
        out.push(LegendEntry {
            icon: icon(&Item::Player {
                x: cx,
                y: cy,
                label: short,
            }),
            label: format!("Player ({label})"),
        });
    }
    for (key, label, sample) in equipment_entries() {
        if kinds.contains(&key) {
            out.push(LegendEntry {
                icon: icon(&sample),
                label: label.into(),
            });
        }
    }
    styles.sort();
    for style in styles {
        out.push(LegendEntry {
            icon: line_icon(style, LineEnd::Arrow),
            label: style.label().into(),
        });
    }
    if stop {
        out.push(LegendEntry {
            icon: line_icon(LineStyle::Skate, LineEnd::Stop),
            label: "Stop".into(),
        });
    }
    out
}

/// A legend of every symbol the editor can draw.
pub fn legend_all() -> Vec<LegendEntry> {
    let (x, y) = (LEGEND_W / 2.0, LEGEND_H / 2.0);
    let mut items: Vec<Item> = PLAYER_ORDER
        .iter()
        .map(|l| Item::Player {
            x,
            y,
            label: (*l).into(),
        })
        .collect();
    items.extend(equipment_entries().into_iter().map(|(_, _, i)| i));
    for style in LineStyle::ALL {
        items.push(Item::Line {
            style,
            end: LineEnd::Arrow,
            pts: vec![[0.0, 0.0], [1.0, 0.0]],
        });
    }
    items.push(Item::Line {
        style: LineStyle::Skate,
        end: LineEnd::Stop,
        pts: vec![[0.0, 0.0], [1.0, 0.0]],
    });
    legend(&[Diagram { items }])
}

/// The legend icon for a kind of equipment (`"net"`, `"cone"`, `"barrier"`, …), as a
/// standalone `<svg>` for the editor's toolbar buttons.
pub fn equipment_icon(kind: &str) -> Option<String> {
    equipment_entries()
        .into_iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, label, item)| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {LEGEND_W} {LEGEND_H}" class="tool-icon" role="img" aria-label="{label}">{}</svg>"#,
                icon(&item)
            )
        })
}

/// A sample of a line style for the editor's toolbar buttons.
pub fn line_sample(style: LineStyle) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {LEGEND_W} {LEGEND_H}" class="legend-line" role="img" aria-label="{}">{}</svg>"#,
        esc(style.label()),
        line_icon(style, LineEnd::Arrow)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_checks() {
        let json = r#"{"items":[
            {"t":"player","x":30,"y":20,"label":"X"},
            {"t":"net","x":40,"y":40},
            {"t":"barrier","x":40,"y":40,"rot":45},
            {"t":"dangler","x":40,"y":40},
            {"t":"line","style":"backcarry","pts":[[30,20],[50,30],[60,15]]}
        ]}"#;
        let d = Diagram::from_json(json).unwrap();
        assert_eq!(d.items.len(), 5);
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
                for pts in [
                    vec![(0.0, 0.0), (20.0, 10.0), (30.0, 0.0)],
                    vec![(0.0, 0.0), (1.0, 0.5)],
                ] {
                    let mut out = String::new();
                    line(&mut out, style, end, &pts);
                    assert!(out.contains("<path"), "{style:?}");
                    assert!(!out.contains("NaN"), "{style:?}");
                }
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
                Item::Barrier {
                    x: 3.0,
                    y: 3.0,
                    rot: 0.0,
                },
                Item::Net {
                    x: 3.0,
                    y: 3.0,
                    rot: 0.0,
                },
            ],
        };
        assert_eq!(
            d.equipment_counts(),
            vec![
                (Equipment::Pucks, 10),
                (Equipment::Cones, 2),
                (Equipment::Nets, 1),
                (Equipment::Barriers, 1)
            ]
        );
    }

    #[test]
    fn coaches_and_goalies_are_circled() {
        assert!(circled("C") && circled("G2"));
        assert!(!circled("X") && !circled("Coach") && !circled("D1"));
        let mut out = String::new();
        player(&mut out, 5.0, 5.0, "G");
        assert!(out.contains("d-ring"));
    }

    #[test]
    fn legend_lists_what_is_drawn() {
        let a = Diagram {
            items: vec![
                Item::Player {
                    x: 1.0,
                    y: 1.0,
                    label: "F1".into(),
                },
                Item::Player {
                    x: 1.0,
                    y: 1.0,
                    label: "F2".into(),
                },
                Item::Player {
                    x: 1.0,
                    y: 1.0,
                    label: "C".into(),
                },
                Item::Text {
                    x: 1.0,
                    y: 1.0,
                    text: "Start".into(),
                },
            ],
        };
        let b = Diagram {
            items: vec![
                Item::Net {
                    x: 1.0,
                    y: 1.0,
                    rot: 0.0,
                },
                Item::Line {
                    style: LineStyle::Backward,
                    end: LineEnd::Stop,
                    pts: vec![[0.0, 0.0], [5.0, 0.0]],
                },
                Item::Line {
                    style: LineStyle::Backward,
                    end: LineEnd::Arrow,
                    pts: vec![[0.0, 0.0], [5.0, 0.0]],
                },
            ],
        };
        let labels: Vec<String> = legend([&a, &b]).into_iter().map(|e| e.label).collect();
        assert_eq!(
            labels,
            vec!["Forward (F1, F2)", "Coach", "Net", "Skate backward", "Stop"]
        );
        assert!(legend([&Diagram::default()]).is_empty());
        assert!(legend_all().len() > 20);
    }

    #[test]
    fn rotation_turns_items() {
        let mut a = String::new();
        let mut b = String::new();
        barrier(&mut a, 10.0, 10.0, 0.0);
        barrier(&mut b, 10.0, 10.0, 90.0);
        assert_ne!(a, b);
        // A mirrored net faces the other way.
        assert_eq!(Xf::MIRROR_X.angle(0.0).round(), 180.0);
    }
}
