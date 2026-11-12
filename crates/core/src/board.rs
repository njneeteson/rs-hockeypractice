//! Complete diagrams as standalone SVG documents: one drill on its own frame,
//! or a practice page with every station's drill on the shared ice.
//!
//! The same SVG goes into web pages (themed by CSS classes) and into the PDF
//! (using the print colours in the presentation attributes).

use crate::diagram::{self, Diagram, LegendEntry};
use crate::practice::{Plan, Segment};
use crate::rink::{self, IceSize, PracticeIce, Rect};
use crate::svg::{colour as c, esc, n, text};
use std::fmt::Write;

/// Padding around the ice, so the boards aren't cut off.
const PAD: f32 = 1.5;

fn open(out: &mut String, view: Rect, class: &str, label: &str, extra: &str) {
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{}" class="{class}" role="img" aria-label="{}"{extra}>"#,
        view.view_box(),
        esc(label)
    );
}

/// The visible area for a drill frame or the practice ice.
pub fn view(frame: Rect) -> Rect {
    frame.pad(PAD)
}

/// One drill on its canonical frame. `extra` is added to the `<svg>` tag.
pub fn drill_svg(ice: IceSize, diagram: &Diagram, label: &str, extra: &str) -> String {
    let mut out = String::new();
    open(&mut out, view(ice.frame()), "rink-svg", label, extra);
    rink::markings(&mut out);
    diagram.render(&mut out, rink::Xf::IDENTITY);
    out.push_str("</svg>");
    out
}

/// The empty rink cropped to the whole sheet, with `extra` on the `<svg>` tag
/// (the browser editor draws on top of it and changes the `viewBox`).
pub fn rink_svg(frame: Rect, extra: &str) -> String {
    let mut out = String::new();
    open(&mut out, view(frame), "rink-svg", "Rink", extra);
    rink::markings(&mut out);
    out.push_str("</svg>");
    out
}

/// A practice page: the practice ice split into the segment's stations.
///
/// With `interactive`, each slot gets a transparent `drop-slot` rectangle that
/// the practice builder uses as a drop target.
pub fn segment_svg(plan: &Plan, seg: &Segment, interactive: bool) -> String {
    let mut out = String::new();
    let label = format!("Ice diagram for {}", seg.display_title());
    open(
        &mut out,
        view(plan.practice.ice.frame()),
        "rink-svg board",
        &label,
        "",
    );
    out.push_str(&segment_inner(plan, seg, interactive));
    out.push_str("</svg>");
    out
}

/// The contents of [`segment_svg`] without the `<svg>` wrapper, in rink feet
/// (its visible area is `view(plan.practice.ice.frame())`).
pub fn segment_inner(plan: &Plan, seg: &Segment, interactive: bool) -> String {
    let ice = plan.practice.ice;
    let mut out = String::new();
    rink::markings(&mut out);

    let placed = plan.slots(seg);
    let many = placed.len() > 1;
    for p in &placed {
        match p.drill {
            Some(d) if d.ice.fits_in(p.slot.size) => d.diagram.render(&mut out, p.slot.xf),
            Some(d) => {
                let msg = format!(
                    "“{}” is {}, this slot is {}",
                    d.name,
                    d.ice.label(),
                    p.slot.size.label()
                );
                centre_note(&mut out, p.slot.area, &msg, c::RED);
            }
            // An empty zone is labelled only in the builder (where it is a
            // place to drop a drill); the printed sheet leaves it blank.
            None if !interactive => {}
            None => {
                // Narrow slots (the 25 ft neutral strip) just say "Open".
                let msg = if p.slot.area.w < 40.0 {
                    "Open".to_string()
                } else {
                    format!("Open: {}", p.slot.size.label().to_lowercase())
                };
                centre_note(&mut out, p.slot.area, &msg, c::MUTED);
            }
        }
    }

    if many {
        dividers(&mut out, ice, placed.iter().map(|p| p.slot.area));
        // Badges already placed count as busy, so neighbours don't pile up on a
        // divider; so does the goalie warm-up.
        let mut taken: Vec<(f32, f32)> = seg
            .goalies
            .map(|g| g.diagram().points())
            .unwrap_or_default();
        // Only stations with a drill get a number (see `PlacedSlot::number`).
        for p in &placed {
            let (Some(number), Some(d)) = (p.number, p.drill) else {
                continue;
            };
            let mut busy: Vec<(f32, f32)> = d
                .diagram
                .points()
                .into_iter()
                .map(|(x, y)| p.slot.xf.apply(x, y))
                .collect();
            busy.extend(&taken);
            let (x, y) = badge_spot(p.slot.area, &busy);
            taken.push((x, y));
            badge(&mut out, x, y, number);
        }
    }

    if interactive {
        for p in &placed {
            let a = p.slot.area;
            let _ = write!(
                out,
                r#"<rect class="drop-slot" data-slot="{}" data-size="{}" data-accepts="{}" x="{}" y="{}" width="{}" height="{}" fill="transparent"><title>{} · {}</title></rect>"#,
                p.index,
                p.slot.size.key(),
                p.slot
                    .size
                    .accepts()
                    .iter()
                    .map(|s| s.key())
                    .collect::<Vec<_>>()
                    .join(" "),
                n(a.x),
                n(a.y),
                n(a.w),
                n(a.h),
                esc(&p.slot.name),
                esc(&p.slot.size.label())
            );
        }
    }

    // The goalie warm-up goes on top (above the drop targets, so the builder
    // can drag it), with an invisible outline that makes it easy to grab.
    if let Some(g) = seg.goalies {
        let _ = write!(
            out,
            r#"<g class="goalie-spot" data-x="{}" data-y="{}" data-rot="{}">"#,
            n(g.x),
            n(g.y),
            n(g.rot)
        );
        g.diagram().render(&mut out, rink::Xf::IDENTITY);
        if interactive {
            let pts: Vec<String> = g
                .outline()
                .iter()
                .map(|[x, y]| format!("{},{}", n(*x), n(*y)))
                .collect();
            let _ = write!(
                out,
                r#"<polygon class="goalie-hit" points="{}" fill="transparent"><title>Goalie warm-up: drag to move (R turns it while dragging)</title></polygon>"#,
                pts.join(" ")
            );
            // A round handle past the coach: drag it to turn the warm-up.
            let [hx, hy] = g.handle();
            let _ = write!(
                out,
                r#"<g class="goalie-rot"><path d="M {} {} L {} {}" fill="none"/><circle cx="{}" cy="{}" r="1.6"><title>Drag to turn the goalie warm-up</title></circle></g>"#,
                n(g.x),
                n(g.y),
                n(hx),
                n(hy),
                n(hx),
                n(hy)
            );
        }
        out.push_str("</g>");
    }
    out
}

/// The legend for a practice page: every symbol used by the drills on its
/// ice, and by the goalie warm-up.
pub fn segment_legend(plan: &Plan, seg: &Segment) -> Vec<LegendEntry> {
    let placed = plan.slots(seg);
    let goalies = seg.goalies.map(|g| g.diagram());
    diagram::legend(
        placed
            .iter()
            .filter_map(|p| p.drill.filter(|d| d.ice.fits_in(p.slot.size)))
            .map(|d| &d.diagram)
            .chain(goalies.as_ref()),
    )
}

fn centre_note(out: &mut String, area: Rect, msg: &str, fill: &str) {
    let size = if area.w < 60.0 { 2.6 } else { 3.4 };
    text(
        out,
        "b-note",
        area.x + area.w / 2.0,
        area.y + area.h / 2.0 + 1.0,
        size,
        600,
        fill,
        "middle",
        msg,
    );
}

/// Dashed lines along slot edges that aren't boards.
fn dividers(out: &mut String, ice: PracticeIce, areas: impl Iterator<Item = Rect>) {
    let f = ice.frame();
    let mut d = String::new();
    let inner_x = |x: f32| x > f.x + 0.1 && x < f.x + f.w - 0.1;
    let inner_y = |y: f32| y > f.y + 0.1 && y < f.y + f.h - 0.1;
    for a in areas {
        for x in [a.x, a.x + a.w] {
            if inner_x(x) {
                let _ = write!(d, "M {} {} L {} {} ", n(x), n(a.y), n(x), n(a.y + a.h));
            }
        }
        for y in [a.y, a.y + a.h] {
            if inner_y(y) {
                let _ = write!(d, "M {} {} L {} {} ", n(a.x), n(y), n(a.x + a.w), n(y));
            }
        }
    }
    if !d.is_empty() {
        let _ = write!(
            out,
            // Light and thin, so the blue and red lines show through.
            r#"<path class="b-divider" d="{}" fill="none" stroke="{}" stroke-width="0.35" stroke-opacity="0.55" stroke-dasharray="1.6 1.2"/>"#,
            d.trim_end(),
            c::SLOT
        );
    }
}

/// Whether a point is on the ice, at least `margin` ft from the boards.
fn on_ice(x: f32, y: f32, margin: f32) -> bool {
    let r = rink::CORNER;
    if x < margin || y < margin || x > rink::LENGTH - margin || y > rink::WIDTH - margin {
        return false;
    }
    // Inside the rounded corners: distance to the nearest corner centre.
    let cx = x.clamp(r, rink::LENGTH - r);
    let cy = y.clamp(r, rink::WIDTH - r);
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= r - margin
}

/// Where to put a slot's number badge: the candidate spot along the slot's
/// edges that is farthest from the drill's players, lines and equipment.
fn badge_spot(area: Rect, busy: &[(f32, f32)]) -> (f32, f32) {
    let inset = 4.2;
    let mut candidates = Vec::new();
    for fx in [0.5f32, 0.25, 0.75] {
        candidates.push((area.x + area.w * fx, area.y + inset));
        candidates.push((area.x + area.w * fx, area.y + area.h - inset));
    }
    for fy in [0.5f32, 0.25, 0.75] {
        candidates.push((area.x + inset, area.y + area.h * fy));
        candidates.push((area.x + area.w - inset, area.y + area.h * fy));
    }
    let first = candidates[0];
    candidates
        .into_iter()
        .filter(|(x, y)| on_ice(*x, *y, 3.5))
        .map(|(x, y)| {
            let clearance = busy
                .iter()
                .map(|(bx, by)| ((bx - x).powi(2) + (by - y).powi(2)).sqrt())
                .fold(f32::INFINITY, f32::min);
            // Prefer the first candidates (top middle) unless they're crowded.
            ((x, y), clearance.min(12.0))
        })
        .fold(None, |best: Option<((f32, f32), f32)>, c| match best {
            Some(b) if b.1 >= c.1 => Some(b),
            _ => Some(c),
        })
        .map(|(p, _)| p)
        .unwrap_or(first)
}

/// A numbered station badge centred on (x, y).
fn badge(out: &mut String, x: f32, y: f32, number: usize) {
    let _ = write!(
        out,
        r#"<circle class="b-badge" cx="{}" cy="{}" r="3" fill="{}"/>"#,
        n(x),
        n(y),
        c::BADGE
    );
    text(
        out,
        "b-badge-text",
        x,
        y + 1.35,
        3.8,
        700,
        c::BADGE_TEXT,
        "middle",
        &number.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::Item;

    #[test]
    fn drill_svg_is_cropped_to_its_frame() {
        let d = Diagram {
            items: vec![Item::Cone { x: 10.0, y: 10.0 }],
        };
        let svg = drill_svg(IceSize::QUARTER, &d, "Test <drill>", "");
        assert!(svg.contains(r#"viewBox="-1.5 -1.5 103 45.5""#), "{svg}");
        assert!(svg.contains("Test &lt;drill&gt;"));
        assert!(svg.contains("d-cone"));
    }
}
