//! The printable practice plan: one US Letter page per segment.
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ date / time        segment title    length   │  header
//! │                                     page n/m │
//! ├──────────────────────────────────────────────┤
//! │        rink diagram (top half)     │ legend  │
//! │ ▒ stations · rotation · equipment · notes ▒  │  strip
//! │ ① drill name            ② drill name         │
//! │   how to run it           how to run it      │  drill text, two columns
//! │   key points              key points         │
//! ├──────────────────────────────────────────────┤
//! │                   team name                  │  footer
//! └──────────────────────────────────────────────┘
//! ```
//!
//! Each page is built as one SVG document (in PDF points) and converted with
//! krilla-svg, with Noto Sans embedded so text stays selectable. A part always
//! gets one page: the text shrinks, flows into three columns and the diagram
//! gets smaller until it fits (see [`fits`]).

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
const COL_GAP: f32 = 14.0;

/// One way to lay out a page: body text size, text columns, diagram height.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fit {
    size: f32,
    cols: usize,
    diagram_h: f32,
}

impl Fit {
    fn col_w(self) -> f32 {
        (CONTENT_W - COL_GAP * (self.cols - 1) as f32) / self.cols as f32
    }
}

/// Layouts to try, most readable first: at each text size, two columns, then
/// three, then a smaller diagram. Below 7 pt only the most compact layout.
/// A part always fits on one page (coaches read it on the bench), so if even
/// the last one overflows the text is cut short with a note.
fn fits() -> Vec<Fit> {
    let mut v = Vec::new();
    for size in [9.0, 8.5, 8.0, 7.5, 7.0] {
        for (cols, diagram_h) in [(2, 250.0), (3, 250.0), (3, 215.0), (3, 185.0)] {
            v.push(Fit {
                size,
                cols,
                diagram_h,
            });
        }
    }
    for size in [6.5, 6.0, 5.5] {
        v.push(Fit {
            size,
            cols: 3,
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

// ---- Drill text: lines that flow through columns --------------------------------

#[derive(Clone, Debug)]
enum Kind {
    /// A drill name, with its station number when the page has several.
    Heading(Option<usize>),
    Sub,
    Body,
    Label,
    Bullet {
        first: bool,
    },
    Gap,
}

#[derive(Clone, Debug)]
struct Line {
    kind: Kind,
    text: String,
    height: f32,
    /// Don't leave this line at the bottom of a column without the next one.
    keep_next: bool,
}

fn drill_lines(plan: &Plan, seg: &Segment, size: f32, col_w: f32) -> Vec<Line> {
    let placed = plan.slots(seg);
    let many = placed.len() > 1;
    let lead = size * 1.32;
    let mut lines = Vec::new();
    let push = |lines: &mut Vec<Line>, kind: Kind, text: String, height: f32, keep_next: bool| {
        lines.push(Line {
            kind,
            text,
            height,
            keep_next,
        });
    };
    for p in placed.iter() {
        let Some(d) = p.drill else { continue };
        if !lines.is_empty() {
            push(&mut lines, Kind::Gap, String::new(), size * 1.1, false);
        }
        let indent = if p.number.is_some() { size * 2.0 } else { 0.0 };
        let hsize = size * 1.25;
        for (i, l) in wrap(&d.name, hsize, true, col_w - indent)
            .into_iter()
            .enumerate()
        {
            let badge = p.number.filter(|_| i == 0);
            push(&mut lines, Kind::Heading(badge), l, hsize * 1.3, true);
        }
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
        for l in wrap(&sub.join(" · "), size * 0.88, false, col_w) {
            push(&mut lines, Kind::Sub, l, size * 0.88 * 1.4, true);
        }
        for para in d.paragraphs() {
            for l in wrap(para, size, false, col_w) {
                push(&mut lines, Kind::Body, l, lead, false);
            }
            push(&mut lines, Kind::Gap, String::new(), size * 0.3, false);
        }
        let points = d.key_points();
        if !points.is_empty() {
            push(
                &mut lines,
                Kind::Label,
                "Key points".into(),
                size * 1.4,
                true,
            );
            for pt in points {
                for (i, l) in wrap(pt, size, false, col_w - size).into_iter().enumerate() {
                    push(&mut lines, Kind::Bullet { first: i == 0 }, l, lead, false);
                }
            }
        }
    }
    lines
}

/// One column's worth of lines, positioned.
struct Placed {
    x: f32,
    y: f32,
    line: Line,
}

/// Flow `lines` into the fit's columns between `top` and `bottom`. Returns the
/// lines that fit and whether all of them did.
fn flow(lines: &[Line], top: f32, bottom: f32, fit: Fit) -> (Vec<Placed>, bool) {
    let mut placed = Vec::new();
    let mut col = 0;
    let mut y = top;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let at_top = y == top;
        if at_top && matches!(line.kind, Kind::Gap) {
            i += 1;
            continue;
        }
        // Height this line needs to stay with what follows it.
        let mut need = line.height;
        let mut j = i;
        while lines[j].keep_next && j + 1 < lines.len() {
            j += 1;
            need += lines[j].height;
        }
        let need = if at_top { line.height } else { need };
        if y + need > bottom {
            col += 1;
            if col == fit.cols || at_top {
                return (placed, false);
            }
            y = top;
            continue;
        }
        let x = MARGIN + col as f32 * (fit.col_w() + COL_GAP);
        placed.push(Placed {
            x,
            y,
            line: line.clone(),
        });
        y += line.height;
        i += 1;
    }
    (placed, true)
}

fn draw_line(out: &mut String, p: &Placed, size: f32) {
    let l = &p.line;
    match l.kind {
        Kind::Heading(badge) => {
            let hsize = size * 1.25;
            let base = p.y + hsize;
            let mut x = p.x;
            if let Some(num) = badge {
                let r = size * 0.78;
                let _ = write!(
                    out,
                    r#"<circle cx="{}" cy="{}" r="{}" fill="{}"/>"#,
                    n(x + r),
                    n(base - hsize * 0.36),
                    n(r),
                    c::BADGE
                );
                text(
                    out,
                    "",
                    x + r,
                    base - hsize * 0.36 + size * 0.34,
                    size * 0.95,
                    700,
                    c::BADGE_TEXT,
                    "middle",
                    &num.to_string(),
                );
                x += size * 2.0;
            }
            text(out, "", x, base, hsize, 700, c::INK, "start", &l.text);
        }
        Kind::Sub => text(
            out,
            "",
            p.x,
            p.y + size * 0.95,
            size * 0.88,
            400,
            c::MUTED,
            "start",
            &l.text,
        ),
        Kind::Body => text(
            out,
            "",
            p.x,
            p.y + size * 1.02,
            size,
            400,
            c::INK,
            "start",
            &l.text,
        ),
        Kind::Label => text(
            out,
            "",
            p.x,
            p.y + size * 1.15,
            size * 0.95,
            700,
            c::INK,
            "start",
            &l.text,
        ),
        Kind::Bullet { first } => {
            if first {
                text(
                    out,
                    "",
                    p.x + size * 0.15,
                    p.y + size * 1.02,
                    size,
                    700,
                    c::INK,
                    "start",
                    "•",
                );
            }
            text(
                out,
                "",
                p.x + size,
                p.y + size * 1.02,
                size,
                400,
                c::INK,
                "start",
                &l.text,
            );
        }
        Kind::Gap => {}
    }
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
    for fit in &all {
        let (head, text_top) = page_head(plan, seg, &legend, fit.diagram_h);
        let lines = drill_lines(plan, seg, fit.size, fit.col_w());
        let (placed, ok) = flow(&lines, text_top, bottom, *fit);
        if ok {
            chosen = Some((head, text_top, placed, *fit, false));
            break;
        }
    }
    let (mut body, text_top, placed, fit, cut) = chosen.unwrap_or_else(|| {
        let fit = all[all.len() - 1];
        let (head, text_top) = page_head(plan, seg, &legend, fit.diagram_h);
        let lines = drill_lines(plan, seg, fit.size, fit.col_w());
        let (placed, _) = flow(&lines, text_top, bottom - 12.0, fit);
        (head, text_top, placed, fit, true)
    });
    for p in &placed {
        draw_line(&mut body, p, fit.size);
    }
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
    if placed.is_empty() && seg.stations.is_empty() {
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
        // Longer text: smaller or more columns, but still one page each.
        let plan = plan_with_text(&sentence.repeat(18));
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
