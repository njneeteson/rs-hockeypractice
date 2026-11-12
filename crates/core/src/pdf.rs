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
//! krilla-svg, with Noto Sans embedded so text stays selectable. When a page's
//! drill text doesn't fit even at the smallest size, it continues on extra
//! pages with the same header.

use crate::board;
use crate::diagram::{LEGEND_H, LEGEND_W, LegendEntry};
use crate::drill::equipment_summary;
use crate::practice::{Plan, Segment};
use crate::svg::{FONT, colour as c, n, text};
use std::fmt::Write;
use std::sync::{Arc, OnceLock};

static REGULAR: &[u8] = include_bytes!("../fonts/NotoSans-Regular.ttf");
static BOLD: &[u8] = include_bytes!("../fonts/NotoSans-Bold.ttf");

const W: f32 = 612.0;
const H: f32 = 792.0;
const MARGIN: f32 = 36.0;
const CONTENT_W: f32 = W - 2.0 * MARGIN;
const HEADER_BOTTOM: f32 = 84.0;
const DIAGRAM_TOP: f32 = 92.0;
const DIAGRAM_MAX_H: f32 = 250.0;
/// Width of the legend column beside the diagram, and the gap before it.
const LEGEND_COL: f32 = 128.0;
const LEGEND_GAP: f32 = 10.0;
const FOOTER_TOP: f32 = H - MARGIN - 20.0;
const COL_GAP: f32 = 18.0;
const COL_W: f32 = (CONTENT_W - COL_GAP) / 2.0;
/// Body text sizes to try, largest first.
const SIZES: [f32; 5] = [10.0, 9.5, 9.0, 8.5, 8.0];

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

fn drill_lines(plan: &Plan, seg: &Segment, size: f32) -> Vec<Line> {
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
        let indent = if many { size * 2.0 } else { 0.0 };
        let hsize = size * 1.25;
        for (i, l) in wrap(&d.name, hsize, true, COL_W - indent)
            .into_iter()
            .enumerate()
        {
            let badge = (i == 0 && many).then_some(p.index + 1);
            push(&mut lines, Kind::Heading(badge), l, hsize * 1.3, true);
        }
        let mut sub = vec![format!("{} min", d.minutes), d.category.label().to_string()];
        if many {
            sub.insert(0, p.slot.name.to_string());
        }
        if !p.note.is_empty() {
            sub.push(p.note.to_string());
        }
        for l in wrap(&sub.join(" · "), size * 0.88, false, COL_W) {
            push(&mut lines, Kind::Sub, l, size * 0.88 * 1.4, true);
        }
        for para in d.paragraphs() {
            for l in wrap(para, size, false, COL_W) {
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
                for (i, l) in wrap(pt, size, false, COL_W - size).into_iter().enumerate() {
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

/// Flow `lines` into two columns per area. `areas` gives each page's
/// (top, bottom); the last area repeats. Returns lines per page.
fn flow(lines: &[Line], first: (f32, f32), rest: (f32, f32)) -> Vec<Vec<Placed>> {
    let mut pages: Vec<Vec<Placed>> = vec![Vec::new()];
    let mut area = first;
    let mut col = 0;
    let mut y = area.0;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let at_top = y == area.0;
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
        if y + need > area.1 && !at_top {
            col += 1;
            if col == 2 {
                col = 0;
                pages.push(Vec::new());
                area = rest;
            }
            y = area.0;
            continue;
        }
        let x = MARGIN + col as f32 * (COL_W + COL_GAP);
        pages.last_mut().unwrap().push(Placed {
            x,
            y,
            line: line.clone(),
        });
        y += line.height;
        i += 1;
    }
    pages
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
/// stops at [`DIAGRAM_MAX_H`] with a note of how many entries didn't fit.
fn draw_legend(out: &mut String, entries: &[LegendEntry], x: f32, y: f32) -> f32 {
    let w = LEGEND_COL - LEGEND_GAP;
    let icon_scale = 2.0;
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
        let h = (lines.len() as f32 * size * 1.25).max(LEGEND_H * icon_scale) + 3.0;
        if cy + h > y + DIAGRAM_MAX_H - 12.0 && i + 1 < entries.len() {
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

struct PageParts {
    seg_index: usize,
    continued: bool,
    body: String,
}

fn header(
    out: &mut String,
    plan: &Plan,
    seg_index: usize,
    continued: bool,
    page: usize,
    pages: usize,
) {
    let p = &plan.practice;
    let seg = &plan.segments[seg_index];
    let date = p.date.format("%A, %B %-d, %Y").to_string();
    text(
        out,
        "",
        MARGIN,
        MARGIN + 13.0,
        11.0,
        700,
        c::INK,
        "start",
        &date,
    );
    let mut sub = plan.time_range(seg_index);
    if !p.title.is_empty() {
        sub = format!("{sub} · {}", p.title);
    }
    text(
        out,
        "",
        MARGIN,
        MARGIN + 28.0,
        9.0,
        400,
        c::MUTED,
        "start",
        &sub,
    );

    let mut title = seg.display_title();
    if continued {
        title.push_str(" (continued)");
    }
    let mid = W / 2.0;
    let title = fit(&title, 15.0, true, 240.0);
    text(
        out,
        "",
        mid,
        MARGIN + 16.0,
        15.0,
        700,
        c::INK,
        "middle",
        &title,
    );
    text(
        out,
        "",
        mid,
        MARGIN + 29.0,
        9.0,
        400,
        c::MUTED,
        "middle",
        &format!("Part {} of {}", seg_index + 1, plan.segments.len()),
    );

    text(
        out,
        "",
        W - MARGIN,
        MARGIN + 15.0,
        15.0,
        700,
        c::INK,
        "end",
        &format!("{} min", seg.minutes),
    );
    text(
        out,
        "",
        W - MARGIN,
        MARGIN + 29.0,
        9.0,
        400,
        c::MUTED,
        "end",
        &format!("Page {page} of {pages}"),
    );
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

fn footer(out: &mut String, plan: &Plan) {
    let _ = write!(
        out,
        r#"<path d="M {} {} L {} {}" stroke="{}" stroke-width="0.6"/>"#,
        n(MARGIN),
        n(FOOTER_TOP),
        n(W - MARGIN),
        n(FOOTER_TOP),
        c::RULE
    );
    let name = fit(&plan.practice.team_name, 11.0, true, CONTENT_W);
    text(
        out,
        "",
        W / 2.0,
        FOOTER_TOP + 15.0,
        11.0,
        700,
        c::INK,
        "middle",
        &name,
    );
}

/// The page parts for one segment (one page, plus any continuation pages).
fn segment_pages(plan: &Plan, seg_index: usize) -> Vec<PageParts> {
    let seg = &plan.segments[seg_index];
    let mut first = String::new();

    // Diagram, scaled to fit beside the legend and centred in its space.
    let legend = board::segment_legend(plan, seg);
    let room = if legend.is_empty() {
        CONTENT_W
    } else {
        CONTENT_W - LEGEND_COL
    };
    let view = board::view(plan.practice.ice.frame());
    let scale = (room / view.w).min(DIAGRAM_MAX_H / view.h);
    let (dw, dh) = (view.w * scale, view.h * scale);
    let dx = MARGIN + (room - dw) / 2.0;
    let legend_h = if legend.is_empty() {
        0.0
    } else {
        draw_legend(&mut first, &legend, MARGIN + room + LEGEND_GAP, DIAGRAM_TOP)
    };
    let _ = write!(
        first,
        r#"<g transform="translate({} {}) scale({}) translate({} {})">{}</g>"#,
        n(dx),
        n(DIAGRAM_TOP),
        scale,
        n(-view.x),
        n(-view.y),
        board::segment_inner(plan, seg, false)
    );

    let strip_top = DIAGRAM_TOP + dh.max(legend_h) + 8.0;
    let strip_h = draw_strip(&mut first, &strip_lines(plan, seg), strip_top);
    let text_top = strip_top + strip_h + 12.0;
    let bottom = FOOTER_TOP - 8.0;

    // Largest text size that fits on the page; else continue at the smallest.
    let mut chosen = None;
    for size in SIZES {
        let lines = drill_lines(plan, seg, size);
        let pages = flow(&lines, (text_top, bottom), (DIAGRAM_TOP, bottom));
        if pages.len() == 1 {
            chosen = Some((size, pages));
            break;
        }
    }
    let (size, pages) = chosen.unwrap_or_else(|| {
        let size = SIZES[SIZES.len() - 1];
        let lines = drill_lines(plan, seg, size);
        (
            size,
            flow(&lines, (text_top, bottom), (DIAGRAM_TOP, bottom)),
        )
    });

    if pages.iter().all(|p| p.is_empty()) && seg.stations.is_empty() {
        text(
            &mut first,
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

    pages
        .into_iter()
        .enumerate()
        .map(|(i, placed)| {
            let mut body = if i == 0 {
                std::mem::take(&mut first)
            } else {
                String::new()
            };
            for p in &placed {
                draw_line(&mut body, p, size);
            }
            PageParts {
                seg_index,
                continued: i > 0,
                body,
            }
        })
        .collect()
}

/// Every page of the plan as an SVG document in PDF points.
pub fn page_svgs(plan: &Plan) -> Vec<String> {
    let parts: Vec<PageParts> = (0..plan.segments.len())
        .flat_map(|i| segment_pages(plan, i))
        .collect();
    let total = parts.len();
    parts
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let mut out = String::new();
            let _ = write!(
                out,
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="{FONT}"><rect width="{W}" height="{H}" fill="{}"/>"#,
                c::ICE
            );
            header(&mut out, plan, p.seg_index, p.continued, i + 1, total);
            out.push_str(&p.body);
            footer(&mut out, plan);
            out.push_str("</svg>");
            out
        })
        .collect()
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
    let opts = usvg::Options {
        fontdb: fontdb(),
        font_family: FONT.into(),
        ..Default::default()
    };
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
    fn long_text_continues_on_another_page() {
        let long = "Players skate hard through the neutral zone and support the puck. ".repeat(60);
        let plan = plan_with_text(&long);
        let svgs = page_svgs(&plan);
        assert!(svgs.len() > 2, "{} pages", svgs.len());
        assert!(svgs[1].contains("(continued)"));
        render(&plan).unwrap();
    }
}
