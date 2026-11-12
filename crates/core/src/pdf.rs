//! The printable practice plan: one US Letter page per segment.
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ date / time        segment title    length   │  header
//! │                                     page n/m │
//! ├──────────────────────────────────────────────┤
//! │        rink diagram (top half)     │ legend  │
//! │ ▒ stations · rotation · equipment · notes ▒  │  strip
//! │ ┌─────────────────────┬────────────────────┐ │
//! │ │ ① drill name        │ Key points         │ │  one block per drill,
//! │ │   zone · min · type │ • …                │ │  never split
//! │ └─────────────────────┴────────────────────┘ │
//! │   how to run it, full width                  │
//! │ ② next drill …                               │
//! ├──────────────────────────────────────────────┤
//! │                   team name                  │  footer
//! └──────────────────────────────────────────────┘
//! ```
//!
//! Each page is built as one SVG document (in PDF points) and converted with
//! krilla-svg, with Noto Sans embedded so text stays selectable. A part always
//! gets one page: the diagram and then the text get smaller until it fits (see
//! [`fits`]).

use crate::board;
use crate::diagram::{LEGEND_H, LEGEND_W, LegendEntry};
use crate::drill::equipment_summary;
use crate::practice::{GOALIE_WARMUP, Plan, Segment};
use crate::svg::{FONT, colour as c, n, text};
use std::fmt::Write;
use std::sync::{Arc, OnceLock};

static REGULAR: &[u8] = include_bytes!("../fonts/NotoSans-Regular.ttf");
static BOLD: &[u8] = include_bytes!("../fonts/NotoSans-Bold.ttf");

const W: f32 = 612.0;
const H: f32 = 792.0;
const MARGIN: f32 = 28.0;
const CONTENT_W: f32 = W - 2.0 * MARGIN;
const HEADER_BOTTOM: f32 = MARGIN + 21.0;
const DIAGRAM_TOP: f32 = HEADER_BOTTOM + 8.0;
/// Width of the legend column beside the diagram, and the gap before it.
const LEGEND_COL: f32 = 128.0;
const LEGEND_GAP: f32 = 10.0;
const FOOTER_TOP: f32 = H - MARGIN - 11.0;

/// One way to lay out a page: body text size and diagram height.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fit {
    size: f32,
    diagram_h: f32,
}

/// Layouts to try, most readable first: at each text size, a smaller diagram
/// before smaller text. Below 7 pt only the most compact diagram.
/// A part always fits on one page (coaches read it on the bench), so if even
/// the last one overflows the text is cut short with a note.
fn fits() -> Vec<Fit> {
    let mut v = Vec::new();
    for size in [9.0, 8.5, 8.0, 7.5, 7.0] {
        for diagram_h in [250.0, 215.0, 185.0, 165.0] {
            v.push(Fit { size, diagram_h });
        }
    }
    for size in [6.5, 6.0, 5.5] {
        v.push(Fit {
            size,
            diagram_h: 165.0,
        });
    }
    v
}

// ---- Fonts and measuring ------------------------------------------------------

struct Fonts {
    regular: ttf_parser::Face<'static>,
    bold: ttf_parser::Face<'static>,
}

fn fonts() -> &'static Fonts {
    static F: OnceLock<Fonts> = OnceLock::new();
    F.get_or_init(|| Fonts {
        regular: ttf_parser::Face::parse(REGULAR, 0).expect("embedded font"),
        bold: ttf_parser::Face::parse(BOLD, 0).expect("embedded font"),
    })
}

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(REGULAR.to_vec());
        db.load_font_data(BOLD.to_vec());
        db.set_sans_serif_family(FONT);
        Arc::new(db)
    })
    .clone()
}

/// Width of `s` in points.
fn width(s: &str, size: f32, bold: bool) -> f32 {
    let f = fonts();
    let face = if bold { &f.bold } else { &f.regular };
    let upem = face.units_per_em() as f32;
    let fallback = face.glyph_index('?');
    s.chars()
        .map(|ch| {
            let g = face.glyph_index(ch).or(fallback);
            g.and_then(|g| face.glyph_hor_advance(g)).unwrap_or(0) as f32
        })
        .sum::<f32>()
        * size
        / upem
}

/// Break `s` into lines no wider than `max` points.
fn wrap(s: &str, size: f32, bold: bool, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if width(&candidate, size, bold) <= max {
            cur = candidate;
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        // A single word wider than the line: break it by characters.
        let mut piece = String::new();
        for ch in word.chars() {
            piece.push(ch);
            if width(&piece, size, bold) > max && piece.chars().count() > 1 {
                piece.pop();
                lines.push(std::mem::take(&mut piece));
                piece.push(ch);
            }
        }
        cur = piece;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

// ---- Drill text: one block per drill, stacked in a single column -------------------

/// One drill's text, wrapped for a body size. It is drawn as a title bar split
/// in half (number and name on the left, key points on the right) with the
/// description below, and is never split across columns or pages.
struct Block {
    number: Option<usize>,
    title: Vec<String>,
    sub: Vec<String>,
    /// Where `sub` starts when it sits on the name's line (offset from the
    /// name), instead of below it.
    sub_inline: Option<f32>,
    /// Each key point's wrapped lines.
    points: Vec<Vec<String>>,
    /// Each description paragraph's wrapped lines.
    body: Vec<Vec<String>>,
}

/// Sizes and spacing derived from the body text size.
#[derive(Clone, Copy)]
struct Metrics {
    size: f32,
    hsize: f32,
    title_h: f32,
    sub_size: f32,
    sub_h: f32,
    label_h: f32,
    lead: f32,
    pad: f32,
    badge_w: f32,
}

impl Metrics {
    fn new(size: f32) -> Metrics {
        let hsize = size * 1.25;
        let sub_size = size * 0.88;
        Metrics {
            size,
            hsize,
            title_h: hsize * 1.25,
            sub_size,
            sub_h: sub_size * 1.4,
            label_h: size * 1.35,
            lead: size * 1.32,
            pad: size * 0.5,
            badge_w: size * 2.0,
        }
    }

    /// Text width of the bar's left half (or the whole bar without key points).
    fn left_w(self, has_points: bool) -> f32 {
        let w = if has_points {
            CONTENT_W / 2.0
        } else {
            CONTENT_W
        };
        w - 2.0 * self.pad
    }

    fn right_w(self) -> f32 {
        CONTENT_W / 2.0 - 2.0 * self.pad
    }

    fn body_w(self) -> f32 {
        CONTENT_W - 2.0 * self.pad
    }
}

fn drill_blocks(plan: &Plan, seg: &Segment, m: Metrics) -> Vec<Block> {
    let placed = plan.slots(seg);
    let many = placed.len() > 1;
    let mut blocks = Vec::new();
    for p in &placed {
        let Some(d) = p.drill else { continue };
        let points = d.key_points();
        let left = m.left_w(!points.is_empty());
        let indent = if p.number.is_some() { m.badge_w } else { 0.0 };
        let mut sub = vec![
            format!("{} min", seg.station_minutes()),
            d.category.label().to_string(),
        ];
        if many {
            sub.insert(0, p.slot.name.to_string());
        }
        if !p.note.is_empty() {
            sub.push(p.note.to_string());
        }
        let title = wrap(&d.name, m.hsize, true, left - indent);
        let sub = sub.join(" · ");
        // Beside a one-line name when there's room, otherwise below it.
        let after = title.first().map_or(0.0, |t| width(t, m.hsize, true)) + m.size;
        let sub_inline = (title.len() == 1
            && after + width(&sub, m.sub_size, false) <= left - indent)
            .then_some(after);
        blocks.push(Block {
            number: p.number,
            title,
            sub: wrap(&sub, m.sub_size, false, left - indent),
            sub_inline,
            points: points
                .iter()
                .map(|pt| wrap(pt, m.size, false, m.right_w() - m.size))
                .collect(),
            body: d
                .paragraphs()
                .iter()
                .map(|para| wrap(para, m.size, false, m.body_w()))
                .collect(),
        });
    }
    blocks
}

/// Draw `b` with its top at `top`, stopping at `bottom`. Returns where the
/// block ends and whether all of it fitted. The title bar is drawn whole or
/// not at all; description lines that don't fit are left off.
fn draw_block(out: &mut String, b: &Block, top: f32, bottom: f32, m: Metrics) -> (f32, bool) {
    let has_points = !b.points.is_empty();
    let sub_h = if b.sub_inline.is_some() {
        0.0
    } else {
        b.sub.len() as f32 * m.sub_h
    };
    let left_h = b.title.len() as f32 * m.title_h + sub_h;
    let right_h = if has_points {
        m.label_h + b.points.iter().map(|p| p.len()).sum::<usize>() as f32 * m.lead
    } else {
        0.0
    };
    let bar_h = left_h.max(right_h) + 2.0 * m.pad;
    if top + bar_h > bottom {
        return (top, false);
    }

    // The bar, split down the middle when there are key points.
    let _ = write!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="3" fill="{}" stroke="{}" stroke-width="0.6"/>"#,
        n(MARGIN),
        n(top),
        n(CONTENT_W),
        n(bar_h),
        c::STRIP,
        c::RULE
    );
    let mid = MARGIN + CONTENT_W / 2.0;
    if has_points {
        let _ = write!(
            out,
            r#"<path d="M {} {} L {} {}" stroke="{}" stroke-width="0.6"/>"#,
            n(mid),
            n(top),
            n(mid),
            n(top + bar_h),
            c::RULE
        );
    }

    // Left: number badge, name, then zone · minutes · category · note.
    let lx = MARGIN + m.pad;
    let tx = if b.number.is_some() {
        lx + m.badge_w
    } else {
        lx
    };
    let mut y = top + m.pad;
    for (i, l) in b.title.iter().enumerate() {
        let base = y + m.hsize;
        if let (0, Some(num)) = (i, b.number) {
            let r = m.size * 0.78;
            let cy = base - m.hsize * 0.36;
            let _ = write!(
                out,
                r#"<circle cx="{}" cy="{}" r="{}" fill="{}"/>"#,
                n(lx + r),
                n(cy),
                n(r),
                c::BADGE
            );
            text(
                out,
                "",
                lx + r,
                cy + m.size * 0.34,
                m.size * 0.95,
                700,
                c::BADGE_TEXT,
                "middle",
                &num.to_string(),
            );
        }
        text(out, "", tx, base, m.hsize, 700, c::INK, "start", l);
        if let Some(dx) = b.sub_inline {
            let s = b.sub.join(" ");
            text(
                out,
                "",
                tx + dx,
                base,
                m.sub_size,
                400,
                c::MUTED,
                "start",
                &s,
            );
        }
        y += m.title_h;
    }
    for l in b.sub.iter().filter(|_| b.sub_inline.is_none()) {
        text(
            out,
            "",
            tx,
            y + m.sub_size * 1.05,
            m.sub_size,
            400,
            c::MUTED,
            "start",
            l,
        );
        y += m.sub_h;
    }

    // Right: key points.
    if has_points {
        let rx = mid + m.pad;
        let mut y = top + m.pad;
        text(
            out,
            "",
            rx,
            y + m.size * 1.0,
            m.size * 0.95,
            700,
            c::INK,
            "start",
            "Key points",
        );
        y += m.label_h;
        for point in &b.points {
            for (i, l) in point.iter().enumerate() {
                let base = y + m.size * 1.02;
                if i == 0 {
                    text(
                        out,
                        "",
                        rx + m.size * 0.15,
                        base,
                        m.size,
                        700,
                        c::INK,
                        "start",
                        "•",
                    );
                }
                text(out, "", rx + m.size, base, m.size, 400, c::INK, "start", l);
                y += m.lead;
            }
        }
    }

    // Below: the description at full width.
    let mut y = top + bar_h;
    if !b.body.is_empty() {
        y += m.size * 0.45;
    }
    for (pi, para) in b.body.iter().enumerate() {
        if pi > 0 {
            y += m.size * 0.35;
        }
        for l in para {
            if y + m.lead > bottom {
                return (y, false);
            }
            text(
                out,
                "",
                lx,
                y + m.size * 1.02,
                m.size,
                400,
                c::INK,
                "start",
                l,
            );
            y += m.lead;
        }
    }
    (y, true)
}

/// Stack the blocks from `top` down to `bottom`. Returns whether all fitted;
/// whatever fits is drawn either way.
fn draw_blocks(out: &mut String, blocks: &[Block], top: f32, bottom: f32, m: Metrics) -> bool {
    let mut y = top;
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            y += m.size * 1.0;
        }
        let (end, ok) = draw_block(out, b, y, bottom, m);
        if !ok {
            return false;
        }
        y = end;
    }
    true
}

// ---- The strip between diagram and drill text ------------------------------------

/// Lines of the strip: `(bold label, value)` pairs, then wrapped notes.
fn strip_lines(plan: &Plan, seg: &Segment) -> Vec<Vec<(String, String)>> {
    let mut facts: Vec<(String, String)> = Vec::new();
    let k = seg.layout.slots().len();
    let used = seg.stations.len();
    facts.push((
        "Ice".into(),
        if k > 1 {
            format!("{} ({used} of {k} stations in use)", seg.layout.label())
        } else {
            seg.layout.label().to_string()
        },
    ));
    match seg.rotation_text() {
        Some(r) => facts.push(("Rotation".into(), r)),
        None if k > 1 => facts.push(("Rotation".into(), "None set".into())),
        None => {}
    }
    if seg.goalies.is_some() {
        facts.push(("Goalies".into(), GOALIE_WARMUP.into()));
    }
    let eq = plan.segment_equipment(seg);
    if !eq.is_empty() {
        facts.push(("Equipment".into(), equipment_summary(&eq)));
    }
    // Pack facts onto rows that fit the strip's width.
    let inner = CONTENT_W - 20.0;
    let mut rows: Vec<Vec<(String, String)>> = vec![Vec::new()];
    let mut used_w = 0.0;
    for (label, value) in facts {
        let w = width(&format!("{label}: "), 8.5, true) + width(&value, 8.5, false) + 18.0;
        if used_w + w > inner && !rows.last().unwrap().is_empty() {
            rows.push(Vec::new());
            used_w = 0.0;
        }
        used_w += w;
        rows.last_mut().unwrap().push((label, value));
    }
    for (i, para) in seg
        .notes
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .enumerate()
    {
        for (j, l) in wrap(para, 8.5, false, inner - 40.0).into_iter().enumerate() {
            let label = if i == 0 && j == 0 { "Notes" } else { "" };
            rows.push(vec![(label.into(), l)]);
        }
    }
    rows
}

fn draw_strip(out: &mut String, rows: &[Vec<(String, String)>], top: f32) -> f32 {
    let row_h = 12.0;
    let h = 10.0 + rows.len() as f32 * row_h;
    let _ = write!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="4" fill="{}" stroke="{}" stroke-width="0.6"/>"#,
        n(MARGIN),
        n(top),
        n(CONTENT_W),
        n(h),
        c::STRIP,
        c::RULE
    );
    for (r, row) in rows.iter().enumerate() {
        let y = top + 5.0 + row_h * r as f32 + 9.0;
        let mut x = MARGIN + 10.0;
        let is_note = row.len() == 1 && (row[0].0 == "Notes" || row[0].0.is_empty()) && r > 0;
        for (label, value) in row {
            if is_note {
                if !label.is_empty() {
                    text(out, "", x, y, 8.5, 700, c::INK, "start", label);
                }
                text(out, "", x + 40.0, y, 8.5, 400, c::INK, "start", value);
                continue;
            }
            let lw = width(&format!("{label}: "), 8.5, true);
            text(
                out,
                "",
                x,
                y,
                8.5,
                700,
                c::INK,
                "start",
                &format!("{label}:"),
            );
            text(out, "", x + lw, y, 8.5, 400, c::INK, "start", value);
            x += lw + width(value, 8.5, false) + 18.0;
        }
    }
    h
}

// ---- The legend beside the diagram -------------------------------------------------

/// Draw the legend with its top-left corner at (x, y); returns its height. It
/// stops at `max_h` with a note of how many entries didn't fit.
fn draw_legend(out: &mut String, entries: &[LegendEntry], x: f32, y: f32, max_h: f32) -> f32 {
    let w = LEGEND_COL - LEGEND_GAP;
    let icon_scale = 1.7;
    let icon_w = LEGEND_W * icon_scale;
    let label_w = w - icon_w - 12.0;
    let size = 7.5;
    text(
        out,
        "",
        x + 6.0,
        y + 9.0,
        8.5,
        700,
        c::INK,
        "start",
        "Legend",
    );
    let mut cy = y + 15.0;
    for (i, e) in entries.iter().enumerate() {
        let lines = wrap(&e.label, size, false, label_w);
        let h = (lines.len() as f32 * size * 1.25).max(LEGEND_H * icon_scale) + 2.5;
        if cy + h > y + max_h - 12.0 && i + 1 < entries.len() {
            let more = format!("+ {} more", entries.len() - i);
            text(
                out,
                "",
                x + 6.0,
                cy + 9.0,
                size,
                400,
                c::MUTED,
                "start",
                &more,
            );
            cy += 12.0;
            break;
        }
        let _ = write!(
            out,
            r#"<g transform="translate({} {}) scale({icon_scale})">{}</g>"#,
            n(x + 6.0),
            n(cy),
            e.icon
        );
        let text_h = lines.len() as f32 * size * 1.25;
        let mut ty = cy + (LEGEND_H * icon_scale - text_h) / 2.0 + size;
        for l in &lines {
            text(
                out,
                "",
                x + 10.0 + icon_w,
                ty,
                size,
                400,
                c::INK,
                "start",
                l,
            );
            ty += size * 1.25;
        }
        cy += h;
    }
    let h = cy - y + 3.0;
    let _ = write!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="4" fill="none" stroke="{}" stroke-width="0.6"/>"#,
        n(x),
        n(y),
        n(w),
        n(h),
        c::RULE
    );
    h
}

// ---- Pages -----------------------------------------------------------------------

/// One line: date and clock time, the part's title, its length.
fn header(out: &mut String, plan: &Plan, seg_index: usize) {
    let p = &plan.practice;
    let seg = &plan.segments[seg_index];
    let base = MARGIN + 14.0;
    let date = p.date.format("%a, %b %-d, %Y").to_string();
    text(out, "", MARGIN, base, 10.0, 700, c::INK, "start", &date);
    let time = plan.time_range(seg_index);
    let tx = MARGIN + width(&date, 10.0, true) + 6.0;
    text(out, "", tx, base, 9.0, 400, c::MUTED, "start", &time);

    let left_w = tx - MARGIN + width(&time, 9.0, false);
    let title = fit(
        &seg.display_title(),
        14.0,
        true,
        CONTENT_W - 2.0 * left_w - 24.0,
    );
    text(out, "", W / 2.0, base, 14.0, 700, c::INK, "middle", &title);

    let length = format!("{} min", seg.minutes);
    text(out, "", W - MARGIN, base, 14.0, 700, c::INK, "end", &length);
    let _ = write!(
        out,
        r#"<path d="M {} {} L {} {}" stroke="{}" stroke-width="0.8"/>"#,
        n(MARGIN),
        n(HEADER_BOTTOM),
        n(W - MARGIN),
        n(HEADER_BOTTOM),
        c::INK
    );
}

/// Shorten `s` with an ellipsis to fit `max` points.
fn fit(s: &str, size: f32, bold: bool, max: f32) -> String {
    if width(s, size, bold) <= max {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while !t.is_empty() && width(&format!("{t}…"), size, bold) > max {
        t.pop();
    }
    format!("{}…", t.trim_end())
}

/// One small line: team, practice title, page number.
fn footer(out: &mut String, plan: &Plan, page: usize, pages: usize) {
    let _ = write!(
        out,
        r#"<path d="M {} {} L {} {}" stroke="{}" stroke-width="0.6"/>"#,
        n(MARGIN),
        n(FOOTER_TOP),
        n(W - MARGIN),
        n(FOOTER_TOP),
        c::RULE
    );
    let y = FOOTER_TOP + 9.0;
    let third = CONTENT_W / 3.0 - 6.0;
    let team = fit(&plan.practice.team_name, 8.0, true, third);
    text(out, "", MARGIN, y, 8.0, 700, c::INK, "start", &team);
    let title = fit(&plan.practice.title, 8.0, false, third);
    text(out, "", W / 2.0, y, 8.0, 400, c::MUTED, "middle", &title);
    let page = format!("Page {page} of {pages}");
    text(out, "", W - MARGIN, y, 8.0, 400, c::MUTED, "end", &page);
}

/// The top of a part's page (diagram, legend and strip) for a diagram at most
/// `diagram_h` tall. Returns the markup and where the drill text starts.
fn page_head(plan: &Plan, seg: &Segment, legend: &[LegendEntry], diagram_h: f32) -> (String, f32) {
    let mut first = String::new();

    // Diagram, scaled to fit beside the legend and centred in its space.
    let room = if legend.is_empty() {
        CONTENT_W
    } else {
        CONTENT_W - LEGEND_COL
    };
    let view = board::view(plan.practice.ice.frame());
    let scale = (room / view.w).min(diagram_h / view.h);
    let (dw, dh) = (view.w * scale, view.h * scale);
    let dx = MARGIN + (room - dw) / 2.0;
    let legend_h = if legend.is_empty() {
        0.0
    } else {
        let x = MARGIN + room + LEGEND_GAP;
        draw_legend(&mut first, legend, x, DIAGRAM_TOP, dh.max(150.0) + 40.0)
    };
    // Clipped to the practice's ice: on half ice the rest of the rink is cut
    // off, as on the web board (whose viewBox does the same).
    let _ = write!(
        first,
        r#"<defs><clipPath id="rink-clip"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath></defs><g transform="translate({} {}) scale({}) translate({} {})"><g clip-path="url(#rink-clip)">{}</g></g>"#,
        n(view.x),
        n(view.y),
        n(view.w),
        n(view.h),
        n(dx),
        n(DIAGRAM_TOP),
        scale,
        n(-view.x),
        n(-view.y),
        board::segment_inner(plan, seg, false)
    );

    let strip_top = DIAGRAM_TOP + dh.max(legend_h) + 8.0;
    let strip_h = draw_strip(&mut first, &strip_lines(plan, seg), strip_top);
    (first, strip_top + strip_h + 12.0)
}

/// The body of one part's page (between header and footer). A part always
/// gets exactly one page: the first layout in [`fits`] that holds all the
/// drill text is used; failing that, the text is cut short with a note.
fn segment_page(plan: &Plan, seg_index: usize) -> (String, Fit) {
    let seg = &plan.segments[seg_index];
    let legend = board::segment_legend(plan, seg);
    let bottom = FOOTER_TOP - 6.0;
    let all = fits();
    let mut chosen = None;
    let mut no_drills = true;
    for fit in &all {
        let (mut head, text_top) = page_head(plan, seg, &legend, fit.diagram_h);
        let m = Metrics::new(fit.size);
        let blocks = drill_blocks(plan, seg, m);
        no_drills = blocks.is_empty();
        if draw_blocks(&mut head, &blocks, text_top, bottom, m) {
            chosen = Some((head, text_top, *fit, false));
            break;
        }
    }
    let (mut body, text_top, fit, cut) = chosen.unwrap_or_else(|| {
        let fit = all[all.len() - 1];
        let (mut head, text_top) = page_head(plan, seg, &legend, fit.diagram_h);
        let m = Metrics::new(fit.size);
        let blocks = drill_blocks(plan, seg, m);
        draw_blocks(&mut head, &blocks, text_top, bottom - 12.0, m);
        (head, text_top, fit, true)
    });
    if cut {
        text(
            &mut body,
            "",
            MARGIN + CONTENT_W,
            bottom,
            7.0,
            400,
            c::MUTED,
            "end",
            "Some drill text didn't fit on this page: see the drills in the app.",
        );
    }
    if no_drills && seg.stations.is_empty() {
        text(
            &mut body,
            "",
            W / 2.0,
            text_top + 20.0,
            10.0,
            400,
            c::MUTED,
            "middle",
            "No drills on this part.",
        );
    }
    (body, fit)
}

/// One part's page as an SVG document in PDF points.
fn page_svg(plan: &Plan, seg_index: usize) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="{FONT}"><rect width="{W}" height="{H}" fill="{}"/>"#,
        c::ICE
    );
    let total = plan.segments.len();
    header(&mut out, plan, seg_index);
    out.push_str(&segment_page(plan, seg_index).0);
    footer(&mut out, plan, seg_index + 1, total);
    out.push_str("</svg>");
    out
}

/// Every page of the plan (one per part) as an SVG document in PDF points.
pub fn page_svgs(plan: &Plan) -> Vec<String> {
    (0..plan.segments.len())
        .map(|i| page_svg(plan, i))
        .collect()
}

fn usvg_options() -> usvg::Options<'static> {
    usvg::Options {
        fontdb: fontdb(),
        font_family: FONT.into(),
        ..Default::default()
    }
}

/// One part's printed page as a standalone SVG for previewing in the browser:
/// laid out exactly as in the PDF, with the text turned into outlines so it
/// looks the same without the embedded font.
pub fn page_preview(plan: &Plan, seg_index: usize) -> anyhow::Result<String> {
    anyhow::ensure!(seg_index < plan.segments.len(), "That page doesn't exist.");
    let tree = usvg::Tree::from_str(&page_svg(plan, seg_index), &usvg_options())
        .map_err(|e| anyhow::anyhow!("laying out the page: {e}"))?;
    Ok(tree.to_string(&usvg::WriteOptions::default()))
}

/// The finished PDF.
pub fn render(plan: &Plan) -> anyhow::Result<Vec<u8>> {
    use krilla::Document;
    use krilla::geom::Size;
    use krilla::metadata::Metadata;
    use krilla::page::PageSettings;
    use krilla_svg::{SurfaceExt, SvgSettings};

    anyhow::ensure!(
        !plan.segments.is_empty(),
        "Add at least one part to the practice first."
    );
    let opts = usvg_options();
    let mut doc = Document::new();
    let title = format!(
        "{} – {} – {}",
        plan.practice.team_name,
        plan.practice.display_title(),
        plan.practice.date
    );
    doc.set_metadata(Metadata::new().title(title));
    let size = Size::from_wh(W, H).expect("page size");
    for svg in page_svgs(plan) {
        let tree = usvg::Tree::from_str(&svg, &opts)
            .map_err(|e| anyhow::anyhow!("laying out the page: {e}"))?;
        let mut page = doc.start_page_with(PageSettings::new(size));
        let mut surface = page.surface();
        surface.draw_svg(&tree, size, SvgSettings::default());
        surface.finish();
        page.finish();
    }
    doc.finish()
        .map_err(|e| anyhow::anyhow!("writing the PDF: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::db::drills::tests::sample;
    use crate::db::practices::tests::practice;
    use crate::practice::SegmentFields;
    use crate::rink::{End, IceSize, Layout, PracticeIce};
    const QUARTERS: End = End {
        zone: false,
        lanes: 2,
    };

    #[test]
    fn wrapping_respects_width() {
        let lines = wrap(
            "The quick brown fox jumps over the lazy dog again and again",
            10.0,
            false,
            80.0,
        );
        assert!(lines.len() > 2);
        assert!(lines.iter().all(|l| width(l, 10.0, false) <= 80.0));
        let long = wrap("Supercalifragilisticexpialidocious", 10.0, true, 40.0);
        assert!(long.len() > 1);
    }

    pub fn plan_with_text(paragraph: &str) -> crate::practice::Plan {
        let db = Database::in_memory().unwrap();
        let p = practice(&db, PracticeIce::Full);
        let seg = db
            .add_segment(
                p,
                SegmentFields {
                    title: "Stations".into(),
                    minutes: 24,
                    layout: Layout::Ends(QUARTERS, QUARTERS),
                    rotation: Some(6),
                    notes: "Whistle to rotate. Goalies stay.".into(),
                },
            )
            .unwrap();
        for slot in 0..4 {
            let mut f = sample(&format!("Drill {slot}"), IceSize::QUARTER);
            f.description = paragraph.into();
            let d = db.create_drill(f).unwrap();
            db.set_station(seg, slot, d, "").unwrap();
        }
        db.add_segment(
            p,
            SegmentFields {
                title: "Scrimmage".into(),
                minutes: 10,
                layout: Layout::Full(1),
                rotation: None,
                notes: String::new(),
            },
        )
        .unwrap();
        db.plan(p).unwrap().unwrap()
    }

    #[test]
    fn renders_a_pdf() {
        let plan = plan_with_text("Skate to the cone, pivot, and take a pass.");
        let svgs = page_svgs(&plan);
        assert_eq!(svgs.len(), 2);
        assert!(svgs[0].contains("Page 1 of 2"));
        assert!(svgs[0].contains("U13"), "team name in the footer");
        assert!(svgs[0].contains("24 min"));
        assert!(svgs[0].contains("Rotate every 6 min"));
        let pdf = render(&plan).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn a_part_always_fits_on_one_page() {
        let sentence = "Players skate hard through the neutral zone and support the puck. ";
        // Short text: the roomiest layout.
        let plan = plan_with_text(sentence);
        assert_eq!(segment_page(&plan, 0).1, fits()[0]);
        // Longer text: smaller diagram or text, but still one page each.
        let plan = plan_with_text(&sentence.repeat(14));
        let fit = segment_page(&plan, 0).1;
        assert!(fit != fits()[0] && fit.size >= 7.0, "{fit:?}");
        assert_eq!(page_svgs(&plan).len(), 2);
        assert!(!page_svgs(&plan)[0].contains("Some drill text"));
        // Far too much: still one page, cut short with a note.
        let plan = plan_with_text(&sentence.repeat(60));
        let svgs = page_svgs(&plan);
        assert_eq!(svgs.len(), 2);
        assert!(svgs[0].contains("Some drill text"));
        render(&plan).unwrap();
    }

    #[test]
    fn previews_a_page_with_outlined_text() {
        let plan = plan_with_text("Skate to the cone.");
        let svg = page_preview(&plan, 0).unwrap();
        assert!(svg.starts_with("<svg") && svg.contains("<path"));
        assert!(!svg.contains("<text"), "text is drawn as outlines");
        assert!(page_preview(&plan, 9).is_err());
    }
}
