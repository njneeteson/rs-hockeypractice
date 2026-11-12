//! Tiny helpers for writing SVG markup by hand.
//!
//! Every element the app draws carries both a `class` (so the web stylesheet
//! can theme it with CSS tokens) and presentation attributes with print
//! colours (used as-is in the PDF, and overridden by CSS in the browser).

use std::fmt::Write;

/// Print colours for the rink and drill diagrams.
pub mod colour {
    pub const ICE: &str = "#ffffff";
    pub const BOARDS: &str = "#2b2b2b";
    pub const RED: &str = "#d0312d";
    pub const BLUE: &str = "#1f57c3";
    pub const CREASE: &str = "#cfe6fb";
    pub const NET: &str = "#f4f6f8";
    pub const NET_MESH: &str = "#9aa4ae";
    pub const BARRIER: &str = "#f2c230";
    pub const DANGLER: &str = "#7b5cd6";
    pub const INK: &str = "#151515";
    pub const CONE: &str = "#f08a24";
    pub const SLOT: &str = "#6b6b6b";
    pub const MUTED: &str = "#8a8a8a";
    pub const BADGE: &str = "#151515";
    pub const BADGE_TEXT: &str = "#ffffff";
    pub const STRIP: &str = "#eef1f4";
    pub const RULE: &str = "#b9bec4";
}

/// The font family embedded in PDFs (see `pdf.rs`).
pub const FONT: &str = "Noto Sans";

/// A number with at most two decimals and no trailing zeros.
pub fn n(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// Escape text for use in SVG content or attribute values.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// `M x y L x y …` for a polyline.
pub fn polyline_d(pts: &[(f32, f32)]) -> String {
    let mut d = String::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        let _ = write!(d, "{}{} {} ", if i == 0 { 'M' } else { 'L' }, n(*x), n(*y));
    }
    d.trim_end().to_string()
}

/// A text element: `anchor` is `start`, `middle` or `end`.
#[allow(clippy::too_many_arguments)]
pub fn text(
    out: &mut String,
    class: &str,
    x: f32,
    y: f32,
    size: f32,
    weight: u16,
    fill: &str,
    anchor: &str,
    content: &str,
) {
    let _ = write!(
        out,
        r#"<text class="{class}" x="{}" y="{}" font-family="{FONT}" font-size="{}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}">{}</text>"#,
        n(x),
        n(y),
        n(size),
        esc(content)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_escaping() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(1.256), "1.26");
        assert_eq!(n(-0.001), "0");
        assert_eq!(esc("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
    }
}
