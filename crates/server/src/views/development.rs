//! Showing Hockey Canada's development model: a drill's split across the
//! levels, and how a practice or a season compares with a team's targets.
//! Colours come from the `--level-*` tokens in `app.css`.

use crate::views::components::tip;
use localapp_core::development::{AgeDivision, Composition, Level, Split, Verdict};
use maud::{Markup, html};

/// The URL path of the guidelines page (under the base path).
pub const GUIDE_PATH: &str = "/development";

/// A thin bar in the levels' colours, one segment per level with time.
pub fn split_bar(split: Option<Split>) -> Markup {
    match split {
        Some(s) => html! {
            span class="split-bar" role="img"
                aria-label={ "Development split: " (describe(s)) } title=(describe(s)) {
                @for l in Level::ALL {
                    @if s.get(*l) > 0 {
                        span class={ "lvl lvl-" (l.key()) } style={ "width:" (s.get(*l)) "%" } {}
                    }
                }
            }
        },
        None => html! {
            span class="split-bar unset" title="Development split not set yet" {}
        },
    }
}

/// "Technical skills 60%, Individual tactics 40%"
pub fn describe(s: Split) -> String {
    Level::ALL
        .iter()
        .filter(|l| s.get(**l) > 0)
        .map(|l| format!("{} {}%", l.label(), s.get(*l)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Minutes as "12 min" or "7.5 min".
fn minutes(m: f32) -> String {
    let r = (m * 2.0).round() / 2.0;
    if r.fract() == 0.0 {
        format!("{r:.0} min")
    } else {
        format!("{r:.1} min")
    }
}

/// Where the targets come from, for a caption.
pub fn targets_source(age: Option<AgeDivision>, custom: bool) -> String {
    match (age, custom) {
        (_, true) => "the team's own targets".into(),
        (Some(a), false) if a.hockey_canada().is_some() => {
            format!("Hockey Canada's {} targets", a.label())
        }
        _ => "no targets".into(),
    }
}

/// Each level's share of the time against its target, as bars with a target
/// marker. `targets: None` shows the shares alone.
pub fn composition_panel(c: &Composition, targets: Option<Split>, guide: &str) -> Markup {
    let classified = c.classified();
    html! {
        div class="focus-panel" {
            @if classified <= 0.0 {
                p class="muted small" {
                    @if c.unclassified > 0.0 {
                        "None of the drills here have a development split yet. Open a drill to set it."
                    } @else {
                        "No drills placed yet."
                    }
                }
            } @else {
                table class="focus-table" {
                    thead {
                        tr {
                            th scope="col" { "Level" }
                            th scope="col" class="bar-col" { "Share of time" }
                            th scope="col" class="num" { "Minutes" }
                            @if targets.is_some() { th scope="col" class="num" { "Target" } }
                        }
                    }
                    tbody {
                        @for l in Level::ALL {
                            @let pct = c.percent(*l).unwrap_or(0.0);
                            @let target = targets.map(|t| t.get(*l));
                            @let verdict = target.map(|t| Verdict::of(pct, t));
                            tr class={ "focus-row" @if let Some(v) = verdict { " " (v.key()) } } {
                                th scope="row" {
                                    span class={ "swatch lvl-" (l.key()) } {}
                                    a href={ (guide) "#" (l.key()) } { (l.label()) }
                                }
                                td class="bar-col" { div class="bar-wrap" {
                                    span class="focus-bar" {
                                        span class={ "fill lvl-" (l.key()) } style={ "width:" (format!("{pct:.0}")) "%" } {}
                                        @if let Some(t) = target {
                                            span class="target" style={ "left:" (t) "%" } title={ "Target " (t) "%" } {}
                                        }
                                    }
                                    span class="pct" { (format!("{pct:.0}")) "%" }
                                } }
                                td class="num" { (minutes(c.minutes_at(*l))) }
                                @if let (Some(t), Some(v)) = (target, verdict) {
                                    td class="num" {
                                        (t) "%"
                                        @if v != Verdict::OnTarget {
                                            " " span class={ "verdict " (v.key()) } title=(v.label()) {
                                                (if v == Verdict::Under { "▼" } else { "▲" })
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                @if c.unclassified > 0.0 {
                    p class="muted small" {
                        (minutes(c.unclassified)) " in drills without a development split aren't counted. "
                        "Open those drills to set it."
                    }
                }
            }
        }
    }
}

/// The drill form's five percentage fields (`focus_<level>`), in 5% steps.
pub fn focus_fields(split: Option<Split>, guide: &str) -> Markup {
    html! {
        div class="focus-fields" data-focus {
            @for l in Level::ALL {
                label class="field" {
                    span class="field-label" {
                        span class={ "swatch lvl-" (l.key()) } {}
                        (l.label()) " " (tip(l.about()))
                    }
                    input type="number" name={ "focus_" (l.key()) } min="0" max="100"
                        step=(Split::STEP) required
                        value=[split.map(|s| s.get(*l))] data-focus-part;
                }
            }
        }
        p class="small" {
            span class="js-only" { "Total: " strong data-focus-total { "–" } "%. " }
            "Must add up to 100%, in steps of 5. "
            a href=(guide) target="_blank" { "What the levels mean" }
        }
    }
}

/// Read the drill form's `focus_<level>` fields.
pub fn parse_focus(get: impl Fn(&str) -> String) -> Result<Split, String> {
    let mut parts = [0u8; 5];
    let mut any = false;
    for (i, l) in Level::ALL.iter().enumerate() {
        let v = get(&format!("focus_{}", l.key()));
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        any = true;
        parts[i] = v
            .parse::<u8>()
            .ok()
            .filter(|p| *p <= 100)
            .ok_or_else(|| format!("“{v}” isn't a percentage for {}.", l.label()))?;
    }
    if !any {
        return Err(
            "Set how the drill's time splits across the development levels (they add up to 100%)."
                .into(),
        );
    }
    Split::new(parts).map_err(|e| e.to_string())
}

/// Read a team's targets from `target_<level>` fields.
pub fn parse_targets(get: impl Fn(&str) -> String) -> Result<Split, String> {
    parse_focus(|k| get(&k.replacen("focus_", "target_", 1)))
}
