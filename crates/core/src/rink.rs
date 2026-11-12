//! The rink: a North American 200 × 85 ft sheet, its markings, and how the ice
//! is divided between drills.
//!
//! Coordinates are in feet: `x` runs 0–200 along the length (left end boards to
//! right end boards), `y` runs 0–85 across (top boards to bottom boards).
//!
//! Each [`IceSize`] has a *canonical frame*, the part of the rink a drill of
//! that size is drawn on (a half-ice drill is drawn on the left half, a
//! quarter-ice drill on the top-left quarter, …). A [`Layout`] splits the
//! practice ice into [`Slot`]s; each slot holds one drill of a given size and
//! carries the [`Xf`] (mirror or rotation) that moves the canonical frame onto
//! the slot. Because the rink is symmetric, those transforms map the markings
//! onto themselves, so a drill's nets and circles land on the right markings.

use crate::svg::{colour as c, n};
use std::fmt::Write;

pub const LENGTH: f32 = 200.0;
pub const WIDTH: f32 = 85.0;
pub const CORNER: f32 = 28.0;
pub const GOAL_LINE: f32 = 11.0;
pub const BLUE_LINE: f32 = 75.0;
pub const CENTER_X: f32 = LENGTH / 2.0;
pub const CENTER_Y: f32 = WIDTH / 2.0;

/// An axis-aligned rectangle in rink feet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Grow by `by` feet on every side.
    pub fn pad(self, by: f32) -> Self {
        Self::new(
            self.x - by,
            self.y - by,
            self.w + 2.0 * by,
            self.h + 2.0 * by,
        )
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    /// `"x y w h"` for an SVG `viewBox`.
    pub fn view_box(self) -> String {
        format!("{} {} {} {}", n(self.x), n(self.y), n(self.w), n(self.h))
    }
}

str_enum! {
    /// How much ice a drill needs.
    pub enum IceSize {
        Full => ("full", "Full ice"),
        Half => ("half", "Half ice"),
        EndZone => ("zone", "End zone"),
        Neutral => ("neutral", "Neutral zone"),
        Quarter => ("quarter", "Quarter ice"),
    }
}

impl IceSize {
    /// The part of the rink a drill of this size is drawn on.
    pub fn frame(self) -> Rect {
        match self {
            IceSize::Full => Rect::new(0.0, 0.0, LENGTH, WIDTH),
            IceSize::Half => Rect::new(0.0, 0.0, CENTER_X, WIDTH),
            IceSize::EndZone => Rect::new(0.0, 0.0, BLUE_LINE, WIDTH),
            IceSize::Neutral => Rect::new(BLUE_LINE, 0.0, LENGTH - 2.0 * BLUE_LINE, WIDTH),
            IceSize::Quarter => Rect::new(0.0, 0.0, CENTER_X, CENTER_Y),
        }
    }

    /// One-line explanation for forms.
    pub fn hint(self) -> &'static str {
        match self {
            IceSize::Full => "The whole sheet, end boards to end boards.",
            IceSize::Half => "One end, from the end boards to the red line.",
            IceSize::EndZone => "One end zone, from the end boards to the blue line.",
            IceSize::Neutral => "Between the blue lines.",
            IceSize::Quarter => "Half of a half: one side of one end, boards to the middle.",
        }
    }
}

/// Moves a drill's canonical frame onto a slot. Points are transformed one by
/// one (text stays upright).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Xf {
    Identity,
    /// Left ↔ right.
    MirrorX,
    /// Top ↔ bottom.
    MirrorY,
    /// Half a turn about centre ice.
    Rot180,
}

impl Xf {
    pub fn apply(self, x: f32, y: f32) -> (f32, f32) {
        match self {
            Xf::Identity => (x, y),
            Xf::MirrorX => (LENGTH - x, y),
            Xf::MirrorY => (x, WIDTH - y),
            Xf::Rot180 => (LENGTH - x, WIDTH - y),
        }
    }

    /// Transform a direction angle in degrees (0° points towards +x, 90° towards +y).
    pub fn angle(self, deg: f32) -> f32 {
        match self {
            Xf::Identity => deg,
            Xf::MirrorX => 180.0 - deg,
            Xf::MirrorY => -deg,
            Xf::Rot180 => deg + 180.0,
        }
    }
}

str_enum! {
    /// How much ice the whole practice has.
    pub enum PracticeIce {
        Full => ("full", "Full ice"),
        Half => ("half", "Half ice"),
    }
}

impl PracticeIce {
    pub fn frame(self) -> Rect {
        match self {
            PracticeIce::Full => IceSize::Full.frame(),
            PracticeIce::Half => IceSize::Half.frame(),
        }
    }
}

/// One drill's share of the ice in a [`Layout`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub size: IceSize,
    /// Where the slot is on the rink.
    pub area: Rect,
    pub xf: Xf,
    pub name: &'static str,
}

const fn slot(size: IceSize, area: Rect, xf: Xf, name: &'static str) -> Slot {
    Slot {
        size,
        area,
        xf,
        name,
    }
}

const FULL: Slot = slot(
    IceSize::Full,
    Rect::new(0.0, 0.0, LENGTH, WIDTH),
    Xf::Identity,
    "Full ice",
);
const HALF_L: Slot = slot(
    IceSize::Half,
    Rect::new(0.0, 0.0, CENTER_X, WIDTH),
    Xf::Identity,
    "Left half",
);
const HALF_R: Slot = slot(
    IceSize::Half,
    Rect::new(CENTER_X, 0.0, CENTER_X, WIDTH),
    Xf::Rot180,
    "Right half",
);
const ZONE_L: Slot = slot(
    IceSize::EndZone,
    Rect::new(0.0, 0.0, BLUE_LINE, WIDTH),
    Xf::Identity,
    "Left end zone",
);
const NEUTRAL: Slot = slot(
    IceSize::Neutral,
    Rect::new(BLUE_LINE, 0.0, LENGTH - 2.0 * BLUE_LINE, WIDTH),
    Xf::Identity,
    "Neutral zone",
);
const ZONE_R: Slot = slot(
    IceSize::EndZone,
    Rect::new(LENGTH - BLUE_LINE, 0.0, BLUE_LINE, WIDTH),
    Xf::Rot180,
    "Right end zone",
);
const Q_TL: Slot = slot(
    IceSize::Quarter,
    Rect::new(0.0, 0.0, CENTER_X, CENTER_Y),
    Xf::Identity,
    "Top-left quarter",
);
const Q_BL: Slot = slot(
    IceSize::Quarter,
    Rect::new(0.0, CENTER_Y, CENTER_X, CENTER_Y),
    Xf::MirrorY,
    "Bottom-left quarter",
);
const Q_TR: Slot = slot(
    IceSize::Quarter,
    Rect::new(CENTER_X, 0.0, CENTER_X, CENTER_Y),
    Xf::MirrorX,
    "Top-right quarter",
);
const Q_BR: Slot = slot(
    IceSize::Quarter,
    Rect::new(CENTER_X, CENTER_Y, CENTER_X, CENTER_Y),
    Xf::Rot180,
    "Bottom-right quarter",
);

str_enum! {
    /// How a page's ice is divided into stations.
    pub enum Layout {
        Full => ("full", "Full ice: one drill"),
        Halves => ("halves", "Two halves"),
        Zones => ("zones", "Three zones: end, neutral, end"),
        Quarters => ("quarters", "Four quarters"),
        HalfQuarters => ("half_quarters", "Left half + two quarters"),
        QuartersHalf => ("quarters_half", "Two quarters + right half"),
        Half => ("half", "Half ice: one drill"),
        HalfSplit => ("half_split", "Half ice split in two quarters"),
    }
}

impl Layout {
    /// The practice ice this layout belongs to.
    pub fn ice(self) -> PracticeIce {
        match self {
            Layout::Half | Layout::HalfSplit => PracticeIce::Half,
            _ => PracticeIce::Full,
        }
    }

    /// The layouts available on `ice`.
    pub fn for_ice(ice: PracticeIce) -> Vec<Layout> {
        Layout::ALL
            .iter()
            .copied()
            .filter(|l| l.ice() == ice)
            .collect()
    }

    /// The default layout for `ice`.
    pub fn default_for(ice: PracticeIce) -> Layout {
        match ice {
            PracticeIce::Full => Layout::Full,
            PracticeIce::Half => Layout::Half,
        }
    }

    pub fn slots(self) -> &'static [Slot] {
        match self {
            Layout::Full => &[FULL],
            Layout::Halves => &[HALF_L, HALF_R],
            Layout::Zones => &[ZONE_L, NEUTRAL, ZONE_R],
            Layout::Quarters => &[Q_TL, Q_TR, Q_BL, Q_BR],
            Layout::HalfQuarters => &[HALF_L, Q_TR, Q_BR],
            Layout::QuartersHalf => &[Q_TL, Q_BL, HALF_R],
            Layout::Half => &[HALF_L],
            Layout::HalfSplit => &[Q_TL, Q_BL],
        }
    }
}

/// Draw the rink markings for the whole sheet into `out`. Crop with the SVG
/// `viewBox` to show part of it.
pub fn markings(out: &mut String) {
    let line = |out: &mut String, class: &str, colour: &str, w: f32, d: &str| {
        let _ = write!(
            out,
            r#"<path class="{class}" d="{d}" fill="none" stroke="{colour}" stroke-width="{}"/>"#,
            n(w)
        );
    };
    let dot = |out: &mut String, class: &str, colour: &str, x: f32, y: f32, r: f32| {
        let _ = write!(
            out,
            r#"<circle class="{class}" cx="{}" cy="{}" r="{}" fill="{colour}"/>"#,
            n(x),
            n(y),
            n(r)
        );
    };
    let circle = |out: &mut String, x: f32, y: f32, r: f32| {
        let _ = write!(
            out,
            r#"<circle class="rk-red" cx="{}" cy="{}" r="{}" fill="none" stroke="{}" stroke-width="0.35"/>"#,
            n(x),
            n(y),
            n(r),
            c::RED
        );
    };

    out.push_str(r#"<g class="rink">"#);
    let _ = write!(
        out,
        r#"<rect class="rk-ice" x="0" y="0" width="{}" height="{}" rx="{CORNER}" ry="{CORNER}" fill="{}"/>"#,
        n(LENGTH),
        n(WIDTH),
        c::ICE
    );

    // Creases (drawn first so the goal lines sit on top).
    for side in [false, true] {
        let (gx, dir) = if side {
            (LENGTH - GOAL_LINE, -1.0)
        } else {
            (GOAL_LINE, 1.0)
        };
        let x1 = gx + dir * 4.47;
        let d = format!(
            "M {} {} L {} {} A 6 6 0 0 {} {} {} L {} {} Z",
            n(gx),
            n(CENTER_Y - 4.0),
            n(x1),
            n(CENTER_Y - 4.0),
            if side { 0 } else { 1 },
            n(x1),
            n(CENTER_Y + 4.0),
            n(gx),
            n(CENTER_Y + 4.0)
        );
        let _ = write!(
            out,
            r#"<path class="rk-crease" d="{d}" fill="{}" stroke="{}" stroke-width="0.25"/>"#,
            c::CREASE,
            c::RED
        );
    }

    // Goal lines, clipped to the rounded corners.
    let dy = (CORNER * CORNER - (CORNER - GOAL_LINE).powi(2)).sqrt();
    let (top, bottom) = (CORNER - dy, WIDTH - CORNER + dy);
    for gx in [GOAL_LINE, LENGTH - GOAL_LINE] {
        let d = format!("M {} {} L {} {}", n(gx), n(top), n(gx), n(bottom));
        line(out, "rk-red", c::RED, 0.35, &d);
    }

    // Trapezoids behind the nets.
    for (gx, ex) in [(GOAL_LINE, 0.0), (LENGTH - GOAL_LINE, LENGTH)] {
        for s in [-1.0f32, 1.0] {
            let d = format!(
                "M {} {} L {} {}",
                n(gx),
                n(CENTER_Y + s * 11.0),
                n(ex),
                n(CENTER_Y + s * 14.0)
            );
            line(out, "rk-red", c::RED, 0.25, &d);
        }
    }

    // Blue lines and the red line.
    for bx in [BLUE_LINE, LENGTH - BLUE_LINE] {
        line(
            out,
            "rk-blue",
            c::BLUE,
            1.0,
            &format!("M {} 0 L {} {}", n(bx), n(bx), n(WIDTH)),
        );
    }
    let _ = write!(
        out,
        r#"<path class="rk-red" d="M {cx} 0 L {cx} {w}" fill="none" stroke="{}" stroke-width="1" stroke-dasharray="3 1"/>"#,
        c::RED,
        cx = n(CENTER_X),
        w = n(WIDTH)
    );

    // Centre circle and dot, referee's crease.
    let _ = write!(
        out,
        r#"<circle class="rk-blue" cx="{}" cy="{}" r="15" fill="none" stroke="{}" stroke-width="0.35"/>"#,
        n(CENTER_X),
        n(CENTER_Y),
        c::BLUE
    );
    dot(out, "rk-blue-fill", c::BLUE, CENTER_X, CENTER_Y, 0.6);
    line(
        out,
        "rk-red",
        c::RED,
        0.25,
        &format!(
            "M {} {} A 10 10 0 0 1 {} {}",
            n(CENTER_X - 10.0),
            n(WIDTH),
            n(CENTER_X + 10.0),
            n(WIDTH)
        ),
    );

    // End-zone faceoff circles with hash marks; neutral-zone dots.
    for cx in [GOAL_LINE + 20.0, LENGTH - GOAL_LINE - 20.0] {
        for cy in [CENTER_Y - 22.0, CENTER_Y + 22.0] {
            circle(out, cx, cy, 15.0);
            dot(out, "rk-red-fill", c::RED, cx, cy, 1.0);
            for sx in [-2.9f32, 2.9] {
                for sy in [-1.0f32, 1.0] {
                    let d = format!(
                        "M {} {} L {} {}",
                        n(cx + sx),
                        n(cy + sy * 14.7),
                        n(cx + sx),
                        n(cy + sy * 17.0)
                    );
                    line(out, "rk-red", c::RED, 0.3, &d);
                }
            }
        }
    }
    for cx in [BLUE_LINE + 5.0, LENGTH - BLUE_LINE - 5.0] {
        for cy in [CENTER_Y - 22.0, CENTER_Y + 22.0] {
            dot(out, "rk-red-fill", c::RED, cx, cy, 1.0);
        }
    }

    // Nets.
    for (gx, dir) in [(GOAL_LINE, -1.0f32), (LENGTH - GOAL_LINE, 1.0)] {
        let back = gx + dir * 3.3;
        let d = format!(
            "M {gx} {t} L {bx} {t2} Q {bx2} {cy} {bx} {b2} L {gx} {b} Z",
            gx = n(gx),
            t = n(CENTER_Y - 3.0),
            b = n(CENTER_Y + 3.0),
            t2 = n(CENTER_Y - 2.4),
            b2 = n(CENTER_Y + 2.4),
            bx = n(back),
            bx2 = n(back + dir * 0.8),
            cy = n(CENTER_Y)
        );
        let _ = write!(
            out,
            r#"<path class="rk-net" d="{d}" fill="{}" stroke="{}" stroke-width="0.3"/>"#,
            c::NET,
            c::BOARDS
        );
    }

    // Boards last, on top of everything.
    let _ = write!(
        out,
        r#"<rect class="rk-boards" x="0" y="0" width="{}" height="{}" rx="{CORNER}" ry="{CORNER}" fill="none" stroke="{}" stroke-width="0.8"/>"#,
        n(LENGTH),
        n(WIDTH),
        c::BOARDS
    );
    out.push_str("</g>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_fit_their_frames() {
        for layout in Layout::ALL {
            let ice = layout.ice().frame();
            for s in layout.slots() {
                let f = s.size.frame();
                assert_eq!((f.w, f.h), (s.area.w, s.area.h), "{layout:?} {}", s.name);
                // The transform maps the canonical frame onto the slot.
                let (x0, y0) = s.xf.apply(f.x, f.y);
                let (x1, y1) = s.xf.apply(f.x + f.w, f.y + f.h);
                let got = Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
                assert_eq!(got, s.area, "{layout:?} {}", s.name);
                assert!(ice.contains(s.area.x, s.area.y), "{layout:?} {}", s.name);
                assert!(ice.contains(s.area.x + s.area.w, s.area.y + s.area.h));
            }
        }
    }

    #[test]
    fn transforms_flip_angles() {
        assert_eq!(Xf::MirrorX.angle(0.0), 180.0);
        assert_eq!(Xf::MirrorY.angle(90.0), -90.0);
        assert_eq!(Xf::Rot180.apply(0.0, 0.0), (LENGTH, WIDTH));
    }

    #[test]
    fn markings_are_well_formed() {
        let mut s = String::new();
        markings(&mut s);
        assert!(s.starts_with("<g") && s.ends_with("</g>"));
        assert!(!s.contains("NaN"));
    }
}
