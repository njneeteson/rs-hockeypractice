//! Practices: the list, the details form, the builder (pages of drills on the
//! ice) and the PDF download.

use crate::error::{AppError, AppResult};
use crate::extract::Ctx;
use crate::flash;
use crate::form;
use crate::routes::drills::empty_library_note;
use crate::state::Shared;
use crate::views::components::{
    Num, Text, badge, card, empty, heading, legend_list, link_button, notice, stat_tile, svg, thead,
};
use crate::views::forms::{action_button, field};
use crate::views::{Nav, Page, page};
use axum::extract::{Path, Query};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use chrono::{Local, NaiveDate, NaiveTime};
use localapp_core::board;
use localapp_core::db::{DrillFilter, PracticeSummary};
use localapp_core::drill::{Drill, equipment_summary};
use localapp_core::practice::{
    GOALIE_WARMUP, GoalieWarmup, PlacedSlot, Plan, PracticeFields, Segment,
};
use localapp_core::rink::{End, IceSize, Layout, MAX_LANES, PracticeIce};
use maud::{Markup, html};
use serde::Deserialize;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/practices", get(list).post(create))
        .route("/practices/new", get(new_page))
        .route("/practices/{id}", get(show).post(update))
        .route("/practices/{id}/edit", get(edit_page))
        .route("/practices/{id}/delete", post(delete))
        .route("/practices/{id}/duplicate", post(duplicate))
        .route("/practices/{id}/plan.pdf", get(pdf))
        .route("/practices/{id}/preview/{sid}", get(preview))
}

// ---- List -----------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct ListQuery {
    team: Option<String>,
}

pub fn practice_table(cx: &Ctx, items: &[PracticeSummary]) -> Markup {
    html! {
        div class="table-wrap" {
            table {
                (thead(&[("Date", Text), ("Practice", Text), ("Team", Text), ("Ice", Text),
                    ("Parts", Num), ("Minutes", Num), ("", Text)]))
                tbody {
                    @for s in items {
                        @let p = &s.practice;
                        tr {
                            td class="date" {
                                (p.date.format("%a %b %-d, %Y"))
                                @if let Some(t) = p.start { span class="sub" { (t.format("%-I:%M %p")) } }
                            }
                            td { a href=(cx.url(&format!("/practices/{}", p.id))) { (p.display_title()) } }
                            td { (p.team_name) }
                            td { (p.ice.label()) }
                            td class="num" { (s.pages) }
                            td class="num" { (s.minutes) }
                            td class="nowrap" {
                                @if s.pages > 0 {
                                    a class="btn small" hx-boost="false" target="_blank"
                                        href=(cx.url(&format!("/practices/{}/plan.pdf", p.id))) { "PDF" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn list(cx: Ctx, Query(q): Query<ListQuery>) -> AppResult<Page> {
    let team = q.team.as_deref().and_then(form::opt_id);
    let (mut all, teams) = cx.db(|db| Ok((db.practices()?, db.teams()?))).await?;
    if let Some(t) = team {
        all.retain(|s| s.practice.team_id == t);
    }
    let today = Local::now().date_naive();
    let (mut upcoming, past): (Vec<_>, Vec<_>) =
        all.into_iter().partition(|s| s.practice.date >= today);
    upcoming.reverse();
    let body = html! {
        (heading("Practices", "Plan a practice page by page, then print it as a PDF.",
            link_button(&cx.url("/practices/new"), "New practice", true)))
        @if teams.len() > 1 {
            form method="get" action=(cx.url("/practices")) class="toolbar" {
                (field("Team", "", html! {
                    select name="team" data-autosubmit {
                        option value="" { "All teams" }
                        @for t in &teams {
                            option value=(t.id) selected[team == Some(t.id)] { (t.name) }
                        }
                    }
                }))
                noscript { button type="submit" { "Show" } }
            }
        }
        (card("Upcoming", "", html! {
            @if upcoming.is_empty() { (empty("Nothing planned yet.")) } @else { (practice_table(&cx, &upcoming)) }
        }))
        @if !past.is_empty() {
            (card("Past", "", practice_table(&cx, &past)))
        }
    };
    Ok(page(&cx, Nav::Practices, "Practices", body))
}

// ---- Details form -------------------------------------------------------------------

#[derive(Deserialize)]
struct PracticeForm {
    #[serde(default)]
    team_id: String,
    #[serde(default)]
    new_team: String,
    #[serde(default)]
    title: String,
    date: String,
    #[serde(default)]
    start: String,
    ice: String,
    #[serde(default)]
    notes: String,
}

fn parse_date(v: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(v.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::bad_request(format!("“{v}” isn't a date (YYYY-MM-DD).")))
}

fn parse_time(v: &str) -> AppResult<Option<NaiveTime>> {
    let v = v.trim();
    if v.is_empty() {
        return Ok(None);
    }
    NaiveTime::parse_from_str(v, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(v, "%H:%M:%S"))
        .map(Some)
        .map_err(|_| AppError::bad_request(format!("“{v}” isn't a time (HH:MM).")))
}

impl PracticeForm {
    /// The fields, with the team id resolved later (a new team may need creating).
    fn fields(&self) -> AppResult<PracticeFields> {
        Ok(PracticeFields {
            team_id: form::opt_id(&self.team_id).unwrap_or(0),
            title: self.title.clone(),
            date: parse_date(&self.date)?,
            start: parse_time(&self.start)?,
            ice: form::key(&self.ice, "ice")?,
            notes: self.notes.clone(),
        })
    }
}

fn details_form(
    cx: &Ctx,
    action: &str,
    p: Option<&localapp_core::practice::Practice>,
    teams: &[localapp_core::practice::Team],
) -> Markup {
    let today = Local::now().date_naive();
    let date = p.map(|p| p.date).unwrap_or(today);
    let start = p
        .and_then(|p| p.start)
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default();
    let ice = p.map(|p| p.ice).unwrap_or(PracticeIce::Full);
    html! {
        form method="post" action=(action) class="form-stack" {
            div class="form-grid" {
                @if teams.is_empty() {
                    (field("Team", "You don't have any teams yet; this one is created with the practice.", html! {
                        input type="text" name="new_team" required placeholder="e.g. U13 A Hawks";
                    }))
                } @else {
                    (field("Team", "Printed in the footer of every page.", html! {
                        select name="team_id" {
                            @for t in teams {
                                option value=(t.id) selected[p.is_some_and(|p| p.team_id == t.id)] { (t.name) }
                            }
                        }
                    }))
                }
                (field("Title", "Optional, e.g. “Tuesday skills”.", html! {
                    input type="text" name="title" maxlength="100" value=(p.map(|p| p.title.as_str()).unwrap_or(""));
                }))
                (field("Date", "", html! { input type="date" name="date" value=(date) required; }))
                (field("Start time", "Optional. With it, each page shows clock times.", html! {
                    input type="time" name="start" value=(start);
                }))
                (field("Ice", "Changing this later resets pages whose layout doesn't fit.", html! {
                    select name="ice" {
                        @for i in PracticeIce::ALL {
                            option value=(i.key()) selected[*i == ice] { (i.label()) }
                        }
                    }
                }))
            }
            (field("Notes", "For you; not printed.", html! {
                textarea name="notes" rows="3" { (p.map(|p| p.notes.as_str()).unwrap_or("")) }
            }))
            div class="form-actions" {
                button type="submit" class="primary" { (if p.is_some() { "Save" } else { "Create practice" }) }
                a class="btn" href=(cx.url("/practices")) { "Cancel" }
            }
        }
    }
}

async fn new_page(cx: Ctx) -> AppResult<Page> {
    let teams = cx.db(|db| db.teams()).await?;
    let body = html! {
        (heading("New practice", "Set the date and ice, then add parts and drop drills onto the rink.", html! {}))
        (card("", "", details_form(&cx, &cx.url("/practices"), None, &teams)))
    };
    Ok(page(&cx, Nav::Practices, "New practice", body))
}

async fn create(cx: Ctx, Form(f): Form<PracticeForm>) -> AppResult<Response> {
    let mut fields = f.fields()?;
    let new_team = f.new_team.clone();
    let result = cx
        .db(move |db| {
            if fields.team_id == 0 {
                fields.team_id = db.create_team(&new_team, "")?;
            }
            db.create_practice(fields)
        })
        .await;
    match result {
        Ok(id) => Ok(flash::info(
            &cx.st,
            &cx.url(&format!("/practices/{id}")),
            "Practice created. Add its first part below.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &cx.url("/practices/new"),
            format!("{e:#}"),
        )),
    }
}

async fn edit_page(cx: Ctx, Path(id): Path<i64>) -> AppResult<Page> {
    let (p, teams) = cx.db(move |db| Ok((db.practice(id)?, db.teams()?))).await?;
    let p = p.ok_or(AppError::NotFound)?;
    let base = format!("/practices/{id}");
    let tomorrow = (p.date + chrono::Duration::days(7)).to_string();
    let body = html! {
        (heading(&format!("Edit {}", p.display_title()), "", link_button(&cx.url(&base), "← Back to the plan", false)))
        div class="grid" {
            (card("Details", "", details_form(&cx, &cx.url(&base), Some(&p), &teams)))
            div class="stack" {
                (card("Copy this practice", "Copies every part and drill to a new date.", html! {
                    form method="post" action=(cx.url(&format!("{base}/duplicate"))) class="form-grid" {
                        (field("New date", "", html! { input type="date" name="date" value=(tomorrow) required; }))
                        div { button type="submit" { "Copy" } }
                    }
                }))
                (card("Delete", "", html! {
                    p { "Deletes the practice and its pages. The drills stay in the library." }
                    (action_button(&cx.url(&format!("{base}/delete")), "Delete practice", "danger",
                        Some("Delete this practice? This can't be undone.")))
                }))
            }
        }
    };
    Ok(page(&cx, Nav::Practices, "Edit practice", body))
}

async fn update(cx: Ctx, Path(id): Path<i64>, Form(f): Form<PracticeForm>) -> AppResult<Response> {
    let mut fields = f.fields()?;
    let new_team = f.new_team.clone();
    let result = cx
        .db(move |db| {
            if fields.team_id == 0 {
                fields.team_id = db.create_team(&new_team, "")?;
            }
            db.update_practice(id, fields)
        })
        .await
        .map(|reset| match reset {
            0 => "Practice saved.".to_string(),
            n => format!("Practice saved. {n} part(s) didn't fit the new ice and were reset."),
        });
    Ok(flash::report(
        &cx.st,
        &cx.url(&format!("/practices/{id}")),
        result,
    ))
}

async fn delete(cx: Ctx, Path(id): Path<i64>) -> AppResult<Response> {
    match cx.db(move |db| db.delete_practice(id)).await {
        Ok(()) => Ok(flash::info(
            &cx.st,
            &cx.url("/practices"),
            "Practice deleted.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &cx.url(&format!("/practices/{id}/edit")),
            format!("{e:#}"),
        )),
    }
}

#[derive(Deserialize)]
struct DuplicateForm {
    date: String,
}

async fn duplicate(
    cx: Ctx,
    Path(id): Path<i64>,
    Form(f): Form<DuplicateForm>,
) -> AppResult<Response> {
    let date = parse_date(&f.date)?;
    match cx.db(move |db| db.duplicate_practice(id, date)).await {
        Ok(new) => Ok(flash::info(
            &cx.st,
            &cx.url(&format!("/practices/{new}")),
            "Copied. You're looking at the copy.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &cx.url(&format!("/practices/{id}/edit")),
            format!("{e:#}"),
        )),
    }
}

// ---- PDF ----------------------------------------------------------------------------------

fn file_name(plan: &Plan) -> String {
    let raw = format!(
        "{} {} practice",
        plan.practice.date, plan.practice.team_name
    );
    let slug: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("{slug}.pdf")
}

async fn pdf(cx: Ctx, Path(id): Path<i64>) -> AppResult<Response> {
    let plan = cx
        .db(move |db| db.plan(id))
        .await?
        .ok_or(AppError::NotFound)?;
    if plan.segments.is_empty() {
        return Ok(flash::error(
            &cx.st,
            &cx.url(&format!("/practices/{id}")),
            "Add at least one part before printing.",
        ));
    }
    let name = file_name(&plan);
    let bytes = tokio::task::spawn_blocking(move || localapp_core::pdf::render(&plan)).await??;
    let mut resp = bytes.into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("inline; filename=\"{name}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

/// One part's printed page as an SVG image (see `pdf::page_preview`).
async fn preview(cx: Ctx, Path((id, sid)): Path<(i64, i64)>) -> AppResult<Response> {
    let plan = cx
        .db(move |db| db.plan(id))
        .await?
        .ok_or(AppError::NotFound)?;
    let i = plan
        .segments
        .iter()
        .position(|s| s.id == sid)
        .ok_or(AppError::NotFound)?;
    let svg =
        tokio::task::spawn_blocking(move || localapp_core::pdf::page_preview(&plan, i)).await??;
    let mut resp = svg.into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml"),
    );
    // The URL carries a version of the part (see `preview_url`), so a cached
    // copy is never stale.
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    Ok(resp)
}

/// The preview image's URL, versioned by everything printed on the page so
/// the browser reloads it exactly when the page changes.
fn preview_url(cx: &Ctx, plan: &Plan, seg: &Segment) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    // The header shows the clock time and "page n of m", so earlier parts'
    // lengths and the number of parts count too.
    let minutes: Vec<u32> = plan.segments.iter().map(|s| s.minutes).collect();
    format!("{:?}{:?}{minutes:?}", plan.practice, seg).hash(&mut h);
    for st in &seg.stations {
        format!("{:?}", plan.drills.get(&st.drill_id)).hash(&mut h);
    }
    cx.url(&format!(
        "/practices/{}/preview/{}?v={:x}",
        plan.practice.id,
        seg.id,
        h.finish()
    ))
}

// ---- The builder ----------------------------------------------------------------------------

/// The ice-layout fields of a part's form. Full ice: one area or lanes, or split
/// at the red line with each end split its own way. Half ice: how the end is split.
/// With JavaScript, `planner.js` hides the end fields unless the ice is split.
fn layout_fields(ice: PracticeIce, selected: Layout) -> Markup {
    let (whole, left, right) = match selected {
        Layout::Full(n) => (n.to_string(), End::WHOLE, End::WHOLE),
        Layout::Ends(l, r) => ("ends".to_string(), l, r),
        Layout::Half(e) => (String::new(), e, End::WHOLE),
    };
    let end_select = |name: &str, sel: End| {
        html! {
            select name=(name) {
                @for e in End::all() {
                    option value=(e.key()) selected[e == sel] { (e.label()) }
                }
            }
        }
    };
    let ends_hint = "Lanes run along the ice. With the neutral zone split off, the lanes cover \
        the end zone and the neutral zone is one more station.";
    html! {
        @match ice {
            PracticeIce::Half => {
                (field("Ice layout", ends_hint, end_select("left", left)))
            }
            PracticeIce::Full => {
                (field("Ice layout", "Drills that don't fit a new layout are removed.", html! {
                    select name="whole" data-layout-whole {
                        option value="ends" selected[whole == "ends"] { "Split at the red line" }
                        @for n in 1..=MAX_LANES {
                            option value=(n) selected[whole == n.to_string()] { (Layout::Full(n).label()) }
                        }
                    }
                }))
                div data-layout-end { (field("Left end", ends_hint, end_select("left", left))) }
                div data-layout-end { (field("Right end", "Used when the ice is split at the red line.", end_select("right", right))) }
            }
        }
    }
}

fn drill_text(d: &Drill) -> Markup {
    html! {
        @for para in d.paragraphs() { p { (para) } }
        @let points = d.key_points();
        @if !points.is_empty() {
            p class="label" { "Key points" }
            ul { @for pt in points { li { (pt) } } }
        }
        @if !d.equipment.is_empty() {
            p class="muted small" { "Equipment: " (d.equipment_summary()) }
        }
    }
}

fn station_list(cx: &Ctx, plan: &Plan, seg: &Segment, library: &[Drill]) -> Markup {
    let base = format!("/segments/{}/stations", seg.id);
    let placed = plan.slots(seg);
    let many = placed.len() > 1;
    html! {
        ol class="stations" {
            @for p in &placed {
                li class="station" {
                    div class="station-head" {
                        @if many { (badge(p.index + 1)) }
                        strong { (p.slot.name) }
                        @if p.slot.name != p.slot.size.label() { span class="muted small" { (p.slot.size.label()) } }
                    }
                    @if let Some(d) = p.drill {
                        div class="station-drill" {
                            a href=(cx.url(&format!("/drills/{}", d.id))) { (d.name) }
                            span class="muted small" { " · " (d.minutes) " min · " (d.category.label()) }
                        }
                        details class="drill-text" data-keep={ "text-" (seg.id) "-" (p.index) } {
                            summary { "Text on the printed page" }
                            (drill_text(d))
                        }
                        div class="station-forms" {
                            form method="post" action=(cx.url(&format!("{base}/{}/note", p.index))) class="inline-form" {
                                input type="text" name="note" value=(p.note) maxlength="200" data-autosubmit
                                    placeholder="Note: coach, group, variation" aria-label="Station note";
                                button type="submit" class="small no-js-only" { "Save note" }
                            }
                            div class="station-actions" {
                                form method="post" action=(cx.url(&format!("{base}/{}/flip", p.index))) class="inline" {
                                    input type="hidden" name="flip" value=(if p.flip { "0" } else { "1" });
                                    button type="submit" class="btn small" aria-pressed=(p.flip)
                                        title="Turn the drill over top to bottom in this station: boards side ↔ open ice" {
                                        (if p.flip { "⇅ Flipped: undo" } else { "⇅ Flip top/bottom" })
                                    }
                                }
                                (action_button(&cx.url(&format!("{base}/{}/clear", p.index)), "Remove", "small danger", None))
                            }
                        }
                    } @else {
                        p class="muted small station-empty" {
                            "Empty. Drop a drill on the rink, or pick one: " (p.slot.size.label().to_lowercase()) " or anything that fits inside it."
                        }
                    }
                    (drill_select(cx, &format!("{base}/{}", p.index), library, p))
                }
            }
        }
    }
}

/// The station's drill picker: drills of the slot's size first, then smaller
/// ones that fit inside it. With JavaScript, choosing one places it at once.
fn drill_select(cx: &Ctx, action: &str, library: &[Drill], p: &PlacedSlot) -> Markup {
    let sizes = p.slot.size.accepts();
    let groups: Vec<(IceSize, Vec<&Drill>)> = sizes
        .iter()
        .map(|s| {
            (
                *s,
                library.iter().filter(|d| d.ice == *s).collect::<Vec<_>>(),
            )
        })
        .filter(|(_, ds)| !ds.is_empty())
        .collect();
    html! {
        form method="post" action=(cx.url(action)) class="inline-form" {
            select name="drill_id" aria-label="Drill for this station" required data-autosubmit {
                @if groups.is_empty() {
                    option value="" { "No drills fit here yet" }
                } @else {
                    option value="" { (if p.drill.is_some() { "Replace with…" } else { "Choose a drill…" }) }
                }
                @for (size, ds) in &groups {
                    optgroup label=(size.label()) {
                        @for d in ds {
                            option value=(d.id) selected[p.drill.is_some_and(|x| x.id == d.id)] { (d.name) }
                        }
                    }
                }
            }
            button type="submit" class="small no-js-only" disabled[groups.is_empty()] {
                (if p.drill.is_some() { "Replace" } else { "Place" })
            }
        }
    }
}

/// The goalie warm-up: none, or two nets with goalies and a coach at a spot
/// along the boards. With JavaScript it can also be dragged on the rink.
fn goalie_row(cx: &Ctx, plan: &Plan, seg: &Segment) -> Markup {
    let action = cx.url(&format!("/segments/{}/goalies", seg.id));
    let ice = plan.practice.ice;
    let current = seg.goalies.map(|g| g.preset_key(ice).unwrap_or("custom"));
    let turn = |deg: i32, label: &str, title: &str| {
        html! {
            form method="post" action=(action) class="inline" {
                input type="hidden" name="turn" value=(deg);
                button type="submit" class="btn small" title=(title) aria-label=(title) { (label) }
            }
        }
    };
    html! {
        div class="goalie-row" {
            form method="post" action=(action) class="inline-form" {
                label for={ "goalies-" (seg.id) } { strong { "Goalie warm-up" } }
                select id={ "goalies-" (seg.id) } name="spot" data-autosubmit {
                    option value="none" selected[current.is_none()] { "None" }
                    @for (key, label, _) in GoalieWarmup::presets(ice) {
                        option value=(key) selected[current == Some(key)] { (label) }
                    }
                    @if current == Some("custom") {
                        option value="custom" selected { "Where you dragged it" }
                    }
                }
                button type="submit" class="small no-js-only" { "Set" }
            }
            @if seg.goalies.is_some() {
                (turn(-45, "⟲", "Turn left"))
                (turn(45, "⟳", "Turn right"))
                span class="muted small js-only" { "Drag it on the rink to move it." }
            } @else {
                span class="muted small" { "Two nets with goalies and a coach, out of the drills' way." }
            }
        }
    }
}

fn segment_settings(cx: &Ctx, plan: &Plan, seg: &Segment) -> Markup {
    html! {
        form method="post" action=(cx.url(&format!("/segments/{}", seg.id))) class="form-stack" {
            div class="form-grid" {
                (field("Title", "Printed at the top of the page.", html! {
                    input type="text" name="title" value=(seg.title) maxlength="80" placeholder="e.g. Warm-up";
                }))
                (field("Minutes", "Length of this part; printed in the header.", html! {
                    input type="number" name="minutes" value=(seg.minutes) min="1" max="180" required;
                }))
                (layout_fields(plan.practice.ice, seg.layout))
                (field("Rotate every (min)", "Minutes per station before groups move on. Leave empty for no rotation.", html! {
                    input type="number" name="rotation" value=[seg.rotation] min="1" max="60";
                }))
            }
            (field("Notes for the strip", "Printed between the rink and the drill text: groups, whistle signals, goalie plan…", html! {
                textarea name="notes" rows="2" { (seg.notes) }
            }))
            div { button type="submit" class="primary" { "Save part" } }
        }
    }
}

fn segment_card(cx: &Ctx, plan: &Plan, i: usize, library: &[Drill]) -> Markup {
    let seg = &plan.segments[i];
    let base = format!("/segments/{}", seg.id);
    let last = i + 1 == plan.segments.len();
    let eq = plan.segment_equipment(seg);
    html! {
        section class="card segment" id={ "seg-" (seg.id) } {
            div class="segment-head" {
                div {
                    div class="muted small" { "Page " (i + 1) " · " (plan.time_range(i)) }
                    h2 { (seg.display_title()) " " span class="pill" { (seg.minutes) " min" } }
                }
                div class="actions" {
                    @if i > 0 {
                        form method="post" action=(cx.url(&format!("{base}/move"))) class="inline" {
                            input type="hidden" name="dir" value="up";
                            button type="submit" class="small" title="Move up" aria-label="Move up" { "↑" }
                        }
                    }
                    @if !last {
                        form method="post" action=(cx.url(&format!("{base}/move"))) class="inline" {
                            input type="hidden" name="dir" value="down";
                            button type="submit" class="small" title="Move down" aria-label="Move down" { "↓" }
                        }
                    }
                    (action_button(&cx.url(&format!("{base}/duplicate")), "Duplicate", "small", None))
                    (action_button(&cx.url(&format!("{base}/delete")), "Delete", "small danger",
                        Some("Delete this part of the practice?")))
                }
            }
            div class="segment-body" {
                @let legend = board::segment_legend(plan, seg);
                div class="board-row" {
                    div class="board-wrap" data-assign-url=(cx.url(&format!("{base}/stations/")))
                        data-goalie-url=(cx.url(&format!("{base}/goalies"))) {
                        (svg(board::segment_svg(plan, seg, true)))
                    }
                    @if !legend.is_empty() {
                        aside class="board-legend" aria-label="Legend" {
                            h3 { "Legend" }
                            (legend_list(&legend))
                        }
                    }
                }
                div class="strip" {
                    span { strong { "Ice: " } (seg.layout.label()) }
                    @if let Some(r) = seg.rotation_text() { span { strong { "Rotation: " } (r) } }
                    @if seg.goalies.is_some() { span { strong { "Goalies: " } (GOALIE_WARMUP) } }
                    @if !eq.is_empty() { span { strong { "Equipment: " } (equipment_summary(&eq)) } }
                    @if !seg.notes.is_empty() { span { strong { "Notes: " } (seg.notes) } }
                }
                (station_list(cx, plan, seg, library))
                (goalie_row(cx, plan, seg))
            }
            details class="segment-settings" data-keep={ "settings-" (seg.id) } open[seg.title.is_empty() && seg.stations.is_empty()] {
                summary { "Part settings: title, minutes, layout, rotation, notes" }
                (segment_settings(cx, plan, seg))
            }
            details class="page-preview" data-keep={ "preview-" (seg.id) } {
                summary { "Preview the printed page" }
                // Lazy: only loaded once the preview is opened.
                img src=(preview_url(cx, plan, seg)) loading="lazy" width="612" height="792"
                    alt={ "Printed page for " (seg.display_title()) };
            }
        }
    }
}

fn library_panel(cx: &Ctx, library: &[Drill], ice: PracticeIce) -> Markup {
    let usable: Vec<&Drill> = library.iter().filter(|d| d.ice.fits(ice)).collect();
    html! {
        aside class="library card" data-library aria-label="Drill library" {
            h2 { "Drills" }
            p class="muted small js-only" {
                "Drag a drill onto a station it fits on the rink (they light up), or click a drill and then a station."
            }
            div class="library-filters" {
                input type="search" data-library-search placeholder="Filter drills" aria-label="Filter drills";
                select data-library-size aria-label="Ice size" {
                    option value="" { "All sizes" }
                    @for s in IceSize::all() {
                        @if usable.iter().any(|d| d.ice == s) {
                            option value=(s.key()) { (s.label()) }
                        }
                    }
                }
            }
            @if usable.is_empty() {
                (empty("No drills fit this ice yet."))
            }
            ul class="library-list" {
                @for d in &usable {
                    li class="drill-chip" draggable="true" tabindex="0" data-drill-id=(d.id) data-size=(d.ice.key())
                        data-name=(d.name.to_lowercase()) aria-label={ (d.name) ", " (d.ice.label()) } {
                        span class="chip-name" { (d.name) }
                        span class="muted small" { (d.ice.label()) " · " (d.minutes) " min" }
                    }
                }
            }
            a class="small" href=(cx.url("/drills/new")) { "+ New drill" }
        }
    }
}

fn add_segment_form(cx: &Ctx, plan: &Plan) -> Markup {
    let n = plan.segments.len();
    let suggestion = match n {
        0 => "Warm-up",
        _ => "",
    };
    html! {
        form method="post" action=(cx.url(&format!("/practices/{}/segments", plan.practice.id))) class="form-grid" {
            (field("Title", "", html! { input type="text" name="title" value=(suggestion) maxlength="80" placeholder="e.g. Skill stations"; }))
            (field("Minutes", "", html! { input type="number" name="minutes" value="10" min="1" max="180" required; }))
            (layout_fields(plan.practice.ice, Layout::default_for(plan.practice.ice)))
            div { button type="submit" class="primary" { "Add part" } }
        }
    }
}

async fn show(cx: Ctx, Path(id): Path<i64>) -> AppResult<Page> {
    let (plan, library) = cx
        .db(move |db| Ok((db.plan(id)?, db.drills(&DrillFilter::default())?)))
        .await?;
    let plan = plan.ok_or(AppError::NotFound)?;
    let p = &plan.practice;
    let base = format!("/practices/{id}");
    let intro = format!(
        "{} · {}{} · {}",
        p.date.format("%A, %B %-d, %Y"),
        p.start
            .map(|t| format!("{} · ", t.format("%-I:%M %p")))
            .unwrap_or_default(),
        p.team_name,
        p.ice.label()
    );
    let eq = plan.equipment();
    let body = html! {
        div id="practice-head" {
            (heading(p.display_title(), &intro, html! {
                @if !plan.segments.is_empty() {
                    a class="btn primary" hx-boost="false" target="_blank" href=(cx.url(&format!("{base}/plan.pdf"))) { "Download PDF" }
                }
                a class="btn" href=(cx.url(&format!("{base}/edit"))) { "Details, copy, delete" }
            }))
        }
        div class="tiles" id="practice-tiles" {
            (stat_tile("Length", html! { (plan.total_minutes()) " min" }, "", ""))
            (stat_tile("Pages", html! { (plan.segments.len()) }, "One page per part", ""))
            (stat_tile("Equipment", html! { (eq.iter().map(|(_, q)| q).sum::<u32>()) " items" },
                &equipment_summary(&eq), "The most any one part needs, item by item."))
        }
        @if library.is_empty() { (empty_library_note(&cx)) }
        div class="builder" {
            // With htmx, every change in the parts (placing a drill, notes,
            // settings, reordering) swaps just this column and the summary
            // above it, keeping the scroll position. Without it, the forms post
            // and redirect back as usual.
            div class="builder-main" id="builder-main" hx-target="#builder-main" hx-select="#builder-main"
                hx-swap="outerHTML show:none" hx-push-url="false"
                hx-select-oob="#practice-head,#practice-tiles,#toasts" {
                @if plan.segments.is_empty() {
                    (notice("info", html! { "This practice has no parts yet. Each part is a block of time and becomes one page of the plan." }))
                }
                @for i in 0..plan.segments.len() {
                    (segment_card(&cx, &plan, i, &library))
                }
                (card("Add a part", "Each part is a block of time with its own page and rink diagram.", add_segment_form(&cx, &plan)))
            }
            (library_panel(&cx, &library, p.ice))
        }
    };
    Ok(page(&cx, Nav::Practices, p.display_title(), body))
}
