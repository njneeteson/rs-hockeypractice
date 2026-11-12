//! The rink: a North American 200 × 85 ft sheet, its markings, and how the ice
//! is divided between drills.
//!
//! Coordinates are in feet: `x` runs 0–200 along the length (left end boards to
//! right end boards), `y` runs 0–85 across (top boards to bottom boards).
//!
//! A drill needs an [`IceSize`]: an [`Area`] of the rink (full ice, half ice,
//! an end zone, the neutral zone, …), either at full width or as one of 2–4
//! lanes running along it. Each size has a *canonical frame*, where the drill
//! is drawn: the area on the left of the rink, its top lane when split.
//!
//! A [`Layout`] splits a page's ice into [`Slot`]s. Each slot holds one drill
//! of a given size and carries the [`Xf`] (shift, mirror or half turn) that
//! moves the canonical frame onto the slot. Edge lanes and the right-hand end
//! use mirrors and turns, so markings such as faceoff circles line up.

use crate::svg::{colour as c, n};
use std::fmt::Write;

pub const LENGTH: f32 = 200.0;
pub const WIDTH: f32 = 85.0;
pub const CORNER: f32 = 28.0;
pub const GOAL_LINE: f32 = 11.0;
pub const BLUE_LINE: f32 = 75.0;
pub const CENTER_X: f32 = LENGTH / 2.0;
pub const CENTER_Y: f32 = WIDTH / 2.0;
/// The most lanes an area can be split into.
pub const MAX_LANES: u8 = 4;

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
    /// A part of the rink a drill can use.
    pub enum Area {
        Full => ("full", "Full ice"),
        Half => ("half", "Half ice"),
        EndZone => ("zone", "End zone"),
        Neutral => ("neutral", "Neutral zone"),
        NeutralHalf => ("neutral_half", "Neutral zone to the red line"),
    }
}

impl Area {
    /// Where the area is drawn (the left-hand one when there are two).
    pub fn rect(self) -> Rect {
        match self {
            Area::Full => Rect::new(0.0, 0.0, LENGTH, WIDTH),
            Area::Half => Rect::new(0.0, 0.0, CENTER_X, WIDTH),
            Area::EndZone => Rect::new(0.0, 0.0, BLUE_LINE, WIDTH),
            Area::Neutral => Rect::new(BLUE_LINE, 0.0, LENGTH - 2.0 * BLUE_LINE, WIDTH),
            Area::NeutralHalf => Rect::new(BLUE_LINE, 0.0, CENTER_X - BLUE_LINE, WIDTH),
        }
    }

    /// Whether the area can be split into lanes.
    pub fn has_lanes(self) -> bool {
        matches!(self, Area::Full | Area::Half | Area::EndZone)
    }

    /// One-line explanation for forms.
    pub fn hint(self) -> &'static str {
        match self {
            Area::Full => "The whole sheet, end boards to end boards.",
            Area::Half => "One end, from the end boards to the red line.",
            Area::EndZone => "One end zone, from the end boards to the blue line.",
            Area::Neutral => "Between the blue lines.",
            Area::NeutralHalf => "From one blue line to the red line (half-ice practices).",
        }
    }
}

/// How much ice a drill needs: an area, at full width or as one of `lanes`
/// lanes running along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IceSize {
    pub area: Area,
    pub lanes: u8,
}

impl IceSize {
    pub const FULL: IceSize = IceSize::whole(Area::Full);
    pub const HALF: IceSize = IceSize::whole(Area::Half);
    pub const END_ZONE: IceSize = IceSize::whole(Area::EndZone);
    pub const NEUTRAL: IceSize = IceSize::whole(Area::Neutral);
    pub const NEUTRAL_HALF: IceSize = IceSize::whole(Area::NeutralHalf);
    /// Half of a half: one of two lanes along half ice.
    pub const QUARTER: IceSize = IceSize::lane(Area::Half, 2);

    pub const fn whole(area: Area) -> Self {
        Self { area, lanes: 1 }
    }

    pub const fn lane(area: Area, lanes: u8) -> Self {
        Self { area, lanes }
    }

    /// A checked size: lanes 1–4, and only on areas that have lanes.
    pub fn new(area: Area, lanes: u8) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_LANES).contains(&lanes),
            "An area can be split into at most {MAX_LANES} lanes."
        );
        anyhow::ensure!(
            lanes == 1 || area.has_lanes(),
            "The {} can't be split into lanes.",
            area.label().to_lowercase()
        );
        Ok(Self { area, lanes })
    }

    /// Every size, in the order forms list them.
    pub fn all() -> Vec<IceSize> {
        let mut out = Vec::new();
        for area in Area::ALL {
            let most = if area.has_lanes() { MAX_LANES } else { 1 };
            for lanes in 1..=most {
                out.push(IceSize::lane(*area, lanes));
            }
        }
        out
    }

    /// Stable key: `"half"`, or `"half-3"` for one of three lanes.
    pub fn key(self) -> String {
        if self.lanes == 1 {
            self.area.key().to_string()
        } else {
            format!("{}-{}", self.area.key(), self.lanes)
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim();
        if key == "quarter" {
            // The name used before lanes existed.
            return Some(IceSize::QUARTER);
        }
        let (area, lanes) = match key.rsplit_once('-') {
            Some((a, l)) => (a, l.parse().ok()?),
            None => (key, 1),
        };
        IceSize::new(Area::from_key(area)?, lanes).ok()
    }

    pub fn label(self) -> String {
        match (self.area, self.lanes) {
            (_, 1) => self.area.label().to_string(),
            (Area::Half, 2) => "Quarter ice".to_string(),
            (a, n) => format!("{}, 1 of {n} lanes", a.label()),
        }
    }

    pub fn hint(self) -> String {
        if self.lanes == 1 {
            self.area.hint().to_string()
        } else {
            format!(
                "One of {} lanes running along the {}.",
                self.lanes,
                self.area.label().to_lowercase()
            )
        }
    }

    /// The part of the rink a drill of this size is drawn on: the area, or
    /// its top lane.
    pub fn frame(self) -> Rect {
        let r = self.area.rect();
        Rect::new(r.x, r.y, r.w, r.h / f32::from(self.lanes))
    }

    /// Whether a drill of this size can run in a station of size `slot`: its
    /// frame lies inside the slot's frame, at the same place on the rink. Both
    /// frames are drawn from the top boards, so the slot's transform places
    /// the drill as it is, against the boards side of the station.
    pub fn fits_in(self, slot: IceSize) -> bool {
        let (d, s) = (self.frame(), slot.frame());
        const EPS: f32 = 1e-3;
        d.x >= s.x - EPS && d.x + d.w <= s.x + s.w + EPS && d.h <= s.h + EPS
    }

    /// The drill sizes that fit in a station of this size, largest first.
    pub fn accepts(self) -> Vec<IceSize> {
        let mut v: Vec<IceSize> = IceSize::all()
            .into_iter()
            .filter(|d| d.fits_in(self))
            .collect();
        v.sort_by(|a, b| {
            let area = |s: IceSize| s.frame().w * s.frame().h;
            area(*b).total_cmp(&area(*a))
        });
        v
    }

    /// Whether some layout on `ice` has a slot this size fits in.
    pub fn fits(self, ice: PracticeIce) -> bool {
        match ice {
            PracticeIce::Full => true,
            PracticeIce::Half => {
                matches!(self.area, Area::Half | Area::EndZone | Area::NeutralHalf)
            }
        }
    }
}

/// Moves a drill's canonical frame onto a slot: `x' = sx·x + dx`, `y' = sy·y + dy`
/// with `sx`, `sy` = ±1. Points are moved one by one (text stays upright).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub sx: f32,
    pub sy: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Xf {
    pub const IDENTITY: Xf = Xf::new(1.0, 1.0, 0.0, 0.0);
    /// Left ↔ right.
    pub const MIRROR_X: Xf = Xf::new(-1.0, 1.0, LENGTH, 0.0);
    /// Top ↔ bottom.
    pub const MIRROR_Y: Xf = Xf::new(1.0, -1.0, 0.0, WIDTH);
    /// Half a turn about centre ice.
    pub const ROT180: Xf = Xf::new(-1.0, -1.0, LENGTH, WIDTH);

    pub const fn new(sx: f32, sy: f32, dx: f32, dy: f32) -> Self {
        Self { sx, sy, dx, dy }
    }

    pub const fn shift(dx: f32, dy: f32) -> Self {
        Self::new(1.0, 1.0, dx, dy)
    }

    /// This transform followed by `next`.
    pub fn then(self, next: Xf) -> Xf {
        Xf::new(
            self.sx * next.sx,
            self.sy * next.sy,
            next.sx * self.dx + next.dx,
            next.sy * self.dy + next.dy,
        )
    }

    pub fn apply(self, x: f32, y: f32) -> (f32, f32) {
        (self.sx * x + self.dx, self.sy * y + self.dy)
    }

    /// Transform a direction angle in degrees (0° points towards +x, 90° towards +y).
    pub fn angle(self, deg: f32) -> f32 {
        let (s, c) = deg.to_radians().sin_cos();
        (self.sy * s).atan2(self.sx * c).to_degrees()
    }

    /// Where `r` lands.
    pub fn rect(self, r: Rect) -> Rect {
        let (x0, y0) = self.apply(r.x, r.y);
        let (x1, y1) = self.apply(r.x + r.w, r.y + r.h);
        Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
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
            PracticeIce::Full => Area::Full.rect(),
            PracticeIce::Half => Area::Half.rect(),
        }
    }
}

/// One drill's share of the ice in a [`Layout`].
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub size: IceSize,
    /// Where the slot is on the rink.
    pub area: Rect,
    pub xf: Xf,
    pub name: String,
}

impl Slot {
    /// The same slot with the drill turned over top ↔ bottom inside it, so
    /// what ran along the boards runs through open ice and the reverse.
    pub fn flipped(mut self) -> Slot {
        let a = self.area;
        self.xf = self.xf.then(Xf::new(1.0, -1.0, 0.0, 2.0 * a.y + a.h));
        self
    }
}

/// The transform that moves the top lane of `n` onto lane `i` (0 = top).
/// Lanes in the top half are shifted down; lanes in the bottom half are
/// mirrored, so the outside lanes keep their markings.
fn lane_xf(i: u8, n: u8) -> Xf {
    let h = WIDTH / f32::from(n);
    let j = n - 1 - i;
    if i > j {
        Xf::shift(0.0, f32::from(j) * h).then(Xf::MIRROR_Y)
    } else {
        Xf::shift(0.0, f32::from(i) * h)
    }
}

fn lane_name(i: u8, n: u8) -> String {
    match (n, i) {
        (1, _) => String::new(),
        (2, 0) | (3, 0) => "top lane".into(),
        (2, 1) | (3, 2) => "bottom lane".into(),
        (3, 1) => "middle lane".into(),
        (_, i) => format!("lane {} of {n}", i + 1),
    }
}

/// How one end of the ice (one half) is split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    /// Split off the neutral zone (blue line to red line) as its own station;
    /// the lanes then cover the end zone only.
    pub zone: bool,
    /// Lanes along the half (or end zone): 1–4.
    pub lanes: u8,
}

impl End {
    pub const WHOLE: End = End {
        zone: false,
        lanes: 1,
    };

    /// Every way to split an end, in the order forms list them.
    pub fn all() -> Vec<End> {
        [false, true]
            .into_iter()
            .flat_map(|zone| (1..=MAX_LANES).map(move |lanes| End { zone, lanes }))
            .collect()
    }

    /// `"h1"`–`"h4"` (half in lanes) or `"z1"`–`"z4"` (end zone in lanes + neutral zone).
    pub fn key(self) -> String {
        format!("{}{}", if self.zone { 'z' } else { 'h' }, self.lanes)
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim();
        let zone = match key.get(..1)? {
            "h" => false,
            "z" => true,
            _ => return None,
        };
        let lanes: u8 = key.get(1..)?.parse().ok()?;
        (1..=MAX_LANES)
            .contains(&lanes)
            .then_some(End { zone, lanes })
    }

    pub fn label(self) -> String {
        let main = match (self.zone, self.lanes) {
            (false, 1) => "Whole half".to_string(),
            (false, n) => format!("Half in {n} lanes"),
            (true, 1) => "End zone".to_string(),
            (true, n) => format!("End zone in {n} lanes"),
        };
        if self.zone {
            format!("{main} + neutral zone")
        } else {
            main
        }
    }

    fn area(self) -> Area {
        if self.zone { Area::EndZone } else { Area::Half }
    }

    /// The lane slots of this end, on the left (`right` = false) or right.
    /// `side` names the end in slot names ("Left", "Right" or "").
    fn slots(self, right: bool, side: &str) -> Vec<Slot> {
        let size = IceSize::lane(self.area(), self.lanes);
        let n = self.lanes;
        let what = if self.zone { "end zone" } else { "half" };
        (0..n)
            .map(|i| {
                let xf = if right {
                    lane_xf(n - 1 - i, n).then(Xf::ROT180)
                } else {
                    lane_xf(i, n)
                };
                let mut name = if side.is_empty() {
                    if self.zone { "End zone" } else { "Half ice" }.to_string()
                } else {
                    format!("{side} {what}")
                };
                let lane = lane_name(i, n);
                if !lane.is_empty() {
                    name = format!("{name}, {lane}");
                }
                Slot {
                    size,
                    area: xf.rect(size.frame()),
                    xf,
                    name,
                }
            })
            .collect()
    }
}

/// How a page's ice is divided into stations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Full ice as one area, or in 2–4 lanes running the length of the rink.
    Full(u8),
    /// Full ice split at the red line; each end split its own way.
    Ends(End, End),
    /// A half-ice practice.
    Half(End),
}

impl Layout {
    /// The practice ice this layout belongs to.
    pub fn ice(self) -> PracticeIce {
        match self {
            Layout::Full(_) | Layout::Ends(..) => PracticeIce::Full,
            Layout::Half(_) => PracticeIce::Half,
        }
    }

    /// The default layout for `ice`.
    pub fn default_for(ice: PracticeIce) -> Layout {
        match ice {
            PracticeIce::Full => Layout::Full(1),
            PracticeIce::Half => Layout::Half(End::WHOLE),
        }
    }

    /// Every layout for `ice`.
    pub fn all_for(ice: PracticeIce) -> Vec<Layout> {
        match ice {
            PracticeIce::Full => {
                let mut v: Vec<Layout> = (1..=MAX_LANES).map(Layout::Full).collect();
                for l in End::all() {
                    for r in End::all() {
                        v.push(Layout::Ends(l, r));
                    }
                }
                v
            }
            PracticeIce::Half => End::all().into_iter().map(Layout::Half).collect(),
        }
    }

    /// Stable key: `"full-1"`, `"ends-h1-z3"`, `"half-z2"`.
    pub fn key(self) -> String {
        match self {
            Layout::Full(n) => format!("full-{n}"),
            Layout::Ends(l, r) => format!("ends-{}-{}", l.key(), r.key()),
            Layout::Half(e) => format!("half-{}", e.key()),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let parts: Vec<&str> = key.trim().split('-').collect();
        let end = End::from_key;
        match parts.as_slice() {
            ["full", n] => {
                let n: u8 = n.parse().ok()?;
                (1..=MAX_LANES).contains(&n).then_some(Layout::Full(n))
            }
            ["ends", l, r] => Some(Layout::Ends(end(l)?, end(r)?)),
            ["half", e] => Some(Layout::Half(end(e)?)),
            // Names used before lanes existed.
            ["full"] => Some(Layout::Full(1)),
            ["halves"] => Some(Layout::Ends(End::WHOLE, End::WHOLE)),
            ["zones"] => Some(Layout::Ends(
                End {
                    zone: true,
                    lanes: 1,
                },
                End {
                    zone: true,
                    lanes: 1,
                },
            )),
            ["half"] => Some(Layout::Half(End::WHOLE)),
            _ => None,
        }
    }

    pub fn label(self) -> String {
        match self {
            Layout::Full(1) => "Full ice".into(),
            Layout::Full(n) => format!("Full ice in {n} lanes"),
            Layout::Half(e) if e == End::WHOLE => "Half ice".into(),
            Layout::Half(e) => e.label(),
            Layout::Ends(l, r) if l == r => {
                if l == End::WHOLE {
                    "Two halves".into()
                } else {
                    format!("Each end: {}", l.label().to_lowercase())
                }
            }
            Layout::Ends(l, r) => format!(
                "Left: {} · Right: {}",
                l.label().to_lowercase(),
                r.label().to_lowercase()
            ),
        }
    }

    pub fn slots(self) -> Vec<Slot> {
        match self {
            Layout::Full(n) => (0..n)
                .map(|i| {
                    let size = IceSize::lane(Area::Full, n);
                    let xf = lane_xf(i, n);
                    let lane = lane_name(i, n);
                    Slot {
                        size,
                        area: xf.rect(size.frame()),
                        xf,
                        name: if lane.is_empty() {
                            "Full ice".into()
                        } else {
                            format!("Full ice, {lane}")
                        },
                    }
                })
                .collect(),
            Layout::Half(e) => {
                let mut v = e.slots(false, "");
                if e.zone {
                    v.push(neutral_half(false, "Neutral zone"));
                }
                v
            }
            Layout::Ends(l, r) => {
                let mut v = l.slots(false, "Left");
                match (l.zone, r.zone) {
                    (true, true) => {
                        let size = IceSize::NEUTRAL;
                        v.push(Slot {
                            size,
                            area: size.frame(),
                            xf: Xf::IDENTITY,
                            name: "Neutral zone".into(),
                        });
                    }
                    (true, false) => v.push(neutral_half(false, "Left neutral zone")),
                    (false, true) => v.push(neutral_half(true, "Right neutral zone")),
                    (false, false) => {}
                }
                v.extend(r.slots(true, "Right"));
                v
            }
        }
    }
}

fn neutral_half(right: bool, name: &str) -> Slot {
    let size = IceSize::NEUTRAL_HALF;
    let xf = if right { Xf::ROT180 } else { Xf::IDENTITY };
    Slot {
        size,
        area: xf.rect(size.frame()),
        xf,
        name: name.into(),
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

    fn all_layouts() -> Vec<Layout> {
        PracticeIce::ALL
            .iter()
            .flat_map(|i| Layout::all_for(*i))
            .collect()
    }

    #[test]
    fn slots_fit_their_frames_and_cover_the_ice() {
        for layout in all_layouts() {
            let ice = layout.ice().frame();
            let slots = layout.slots();
            let mut covered = 0.0;
            for s in &slots {
                let f = s.size.frame();
                assert!((f.w - s.area.w).abs() < 0.01 && (f.h - s.area.h).abs() < 0.01);
                // The transform maps the canonical frame onto the slot.
                let got = s.xf.rect(f);
                for (a, b) in [
                    (got.x, s.area.x),
                    (got.y, s.area.y),
                    (got.w, s.area.w),
                    (got.h, s.area.h),
                ] {
                    assert!((a - b).abs() < 0.01, "{layout:?} {}", s.name);
                }
                let r = s.area.pad(-0.01);
                assert!(ice.contains(r.x, r.y), "{layout:?} {}", s.name);
                assert!(ice.contains(r.x + r.w, r.y + r.h), "{layout:?} {}", s.name);
                assert!(s.size.fits(layout.ice()), "{layout:?} {}", s.name);
                covered += s.area.w * s.area.h;
            }
            // No overlaps, no gaps.
            assert!((covered - ice.w * ice.h).abs() < 1.0, "{layout:?}");
            for (i, a) in slots.iter().enumerate() {
                for b in &slots[i + 1..] {
                    let (a, b) = (a.area.pad(-0.01), b.area.pad(-0.01));
                    let apart = a.x + a.w <= b.x
                        || b.x + b.w <= a.x
                        || a.y + a.h <= b.y
                        || b.y + b.h <= a.y;
                    assert!(apart, "{layout:?} overlaps");
                }
            }
            assert_eq!(Layout::from_key(&layout.key()), Some(layout));
        }
    }

    #[test]
    fn half_ice_can_hold_five_stations() {
        let l = Layout::Half(End {
            zone: true,
            lanes: 4,
        });
        assert_eq!(l.slots().len(), 5);
        assert_eq!(l.slots()[4].size, IceSize::NEUTRAL_HALF);
        assert_eq!(l.label(), "End zone in 4 lanes + neutral zone");
    }

    #[test]
    fn sizes_and_keys() {
        for s in IceSize::all() {
            assert_eq!(IceSize::from_key(&s.key()), Some(s));
        }
        assert_eq!(IceSize::from_key("quarter"), Some(IceSize::QUARTER));
        assert_eq!(IceSize::from_key("neutral-2"), None);
        assert_eq!(IceSize::from_key("half-5"), None);
        assert_eq!(IceSize::QUARTER.label(), "Quarter ice");
        assert_eq!(
            Layout::from_key("halves"),
            Some(Layout::Ends(End::WHOLE, End::WHOLE))
        );
    }

    #[test]
    fn transforms() {
        assert_eq!(Xf::MIRROR_X.angle(0.0).round(), 180.0);
        assert_eq!(Xf::MIRROR_Y.angle(90.0).round(), -90.0);
        assert_eq!(Xf::ROT180.apply(0.0, 0.0), (LENGTH, WIDTH));
        // Quarters keep the old mirrors: top-right mirrors left-right.
        let q = Layout::Ends(
            End::WHOLE,
            End {
                zone: false,
                lanes: 2,
            },
        )
        .slots();
        assert_eq!(q[1].xf, Xf::MIRROR_X);
        assert_eq!(q[2].xf, Xf::ROT180);
    }

    #[test]
    fn smaller_drills_fit_larger_stations() {
        let k = |s: &str| IceSize::from_key(s).unwrap();
        // Narrower lanes fit wider lanes and the whole area.
        assert!(k("half-4").fits_in(k("half-3")));
        assert!(k("half-3").fits_in(k("half-2")));
        assert!(k("half-2").fits_in(k("half")));
        assert!(!k("half-2").fits_in(k("half-3")));
        // End-zone drills fit half ice, and half-ice drills fit full-ice lanes.
        assert!(k("zone").fits_in(k("half")));
        assert!(k("zone-2").fits_in(k("half-2")));
        assert!(k("zone-3").fits_in(k("half-2")));
        assert!(!k("zone").fits_in(k("half-2")));
        assert!(k("half").fits_in(k("full")));
        assert!(k("half-2").fits_in(k("full-2")));
        assert!(k("neutral").fits_in(k("full")));
        assert!(!k("neutral").fits_in(k("full-4")));
        assert!(k("neutral_half").fits_in(k("half")));
        assert!(k("neutral_half").fits_in(k("neutral")));
        // But not a different part of the rink, or a bigger shape.
        assert!(!k("neutral").fits_in(k("half")));
        assert!(!k("half").fits_in(k("zone")));
        assert!(!k("full").fits_in(k("half")));
        for s in IceSize::all() {
            assert!(s.fits_in(s));
            assert_eq!(s.accepts()[0], s, "the slot's own size comes first");
        }
        // Every drawing that fits stays inside the station once placed.
        for layout in Layout::all_for(PracticeIce::Full) {
            for slot in layout.slots() {
                for d in slot.size.accepts() {
                    let r = slot.xf.rect(d.frame());
                    let a = slot.area.pad(1e-3);
                    assert!(a.contains(r.x, r.y) && a.contains(r.x + r.w, r.y + r.h));
                }
            }
        }
    }

    #[test]
    fn flipped_slots_stay_in_place() {
        for layout in Layout::all_for(PracticeIce::Full) {
            for s in layout.slots() {
                let f = s.clone().flipped();
                let frame = s.size.frame();
                let near = |a: Xf, b: Xf| {
                    [a.sx - b.sx, a.sy - b.sy, a.dx - b.dx, a.dy - b.dy]
                        .iter()
                        .all(|d| d.abs() < 1e-3)
                };
                let (a, b) = (f.xf.rect(frame), s.xf.rect(frame));
                let key = layout.key();
                assert!(
                    (a.y - b.y).abs() < 1e-3 && (a.h - b.h).abs() < 1e-3,
                    "{key}"
                );
                assert_eq!((a.x, a.w), (b.x, b.w), "{key}");
                assert!(near(f.clone().flipped().xf, s.xf), "{key}");
                // Top and bottom swap.
                let (_, y0) = s.xf.apply(frame.x, frame.y);
                let (_, y1) = f.xf.apply(frame.x, frame.y);
                assert!((y0 + y1 - (2.0 * s.area.y + s.area.h)).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn markings_are_well_formed() {
        let mut s = String::new();
        markings(&mut s);
        assert!(s.starts_with("<g") && s.ends_with("</g>"));
        assert!(!s.contains("NaN"));
        assert!(!s.contains("rk-net"), "nets are placed by drills");
    }
}
