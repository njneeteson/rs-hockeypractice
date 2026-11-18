//! Practices: the list, the details form, the builder (pages of drills on the
//! ice) and the PDF download.

use crate::auth::store::BuilderExtra;
use crate::error::{AppError, AppResult};
use crate::extract::{Ctx, PracticeCtx};
use crate::flash;
use crate::form;
use crate::routes::drills::empty_library_note;
use crate::state::Shared;
use crate::views::components::{
    Num, Text, badge, card, empty, heading, legend_list, link_button, notice, stat_tile, svg, thead,
};
use crate::views::development::{
    GUIDE_PATH, composition_bar, composition_panel, split_bar, targets_source,
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
use localapp_core::db::practice_history::Change;
use localapp_core::db::{DrillFilter, PracticeSummary};
use localapp_core::development::{Composition, Level, Verdict, closer_by};
use localapp_core::drill::{Category, Drill, equipment_summary};
use localapp_core::owner::{Library, MemberId};
use localapp_core::practice::{
    GOALIE_WARMUP, GoalieWarmup, PlacedSlot, Plan, PracticeFields, Segment, Team,
};
use localapp_core::rink::{End, IceSize, Layout, MAX_LANES, PracticeIce};
use maud::{Markup, html};
use serde::Deserialize;
use std::collections::HashMap;

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
        .route("/practices/{id}/undo", post(undo))
        .route("/practices/{id}/restore", post(restore))
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
    let me = cx.member();
    let (mut all, teams) = cx
        .db(move |db| Ok((db.practices_for(me)?, my_teams(db, me)?)))
        .await?;
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

/// The teams a member belongs to.
fn my_teams(db: &localapp_core::db::Database, me: MemberId) -> anyhow::Result<Vec<Team>> {
    Ok(db.teams_for(me)?.into_iter().map(|(t, _)| t).collect())
}

/// The practice's team: one of the member's, or a new one they own.
fn resolve_team(
    db: &localapp_core::db::Database,
    me: MemberId,
    fields: &mut PracticeFields,
    new_team: &str,
) -> anyhow::Result<()> {
    if fields.team_id == 0 {
        fields.team_id = db.create_team(me, new_team, "")?;
    } else {
        anyhow::ensure!(
            db.member_role(fields.team_id, me)?.is_some(),
            "Choose one of your teams."
        );
    }
    Ok(())
}

fn details_form(
    cx: &Ctx,
    action: &str,
    p: Option<&localapp_core::practice::Practice>,
    teams: &[Team],
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
    let me = cx.member();
    let teams = cx.db(move |db| my_teams(db, me)).await?;
    let body = html! {
        (heading("New practice", "Set the date and ice, then add parts and drop drills onto the rink.", html! {}))
        (card("", "", details_form(&cx, &cx.url("/practices"), None, &teams)))
    };
    Ok(page(&cx, Nav::Practices, "New practice", body))
}

async fn create(cx: Ctx, Form(f): Form<PracticeForm>) -> AppResult<Response> {
    let mut fields = f.fields()?;
    let new_team = f.new_team.clone();
    let me = cx.member();
    let result = cx
        .db(move |db| {
            resolve_team(db, me, &mut fields, &new_team)?;
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

async fn edit_page(pc: PracticeCtx) -> AppResult<Page> {
    let cx = &pc.cx;
    let p = &pc.practice;
    let (id, me) = (p.id, cx.member());
    let teams = cx.db(move |db| my_teams(db, me)).await?;
    let base = format!("/practices/{id}");
    let tomorrow = (p.date + chrono::Duration::days(7)).to_string();
    let body = html! {
        (heading(&format!("Edit {}", p.display_title()), "", link_button(&cx.url(&base), "← Back to the plan", false)))
        div class="grid" {
            (card("Details", "", details_form(cx, &cx.url(&base), Some(p), &teams)))
            div class="stack" {
                (card("Copy this practice", "Copies every part and drill to a new date.", html! {
                    form method="post" action=(cx.url(&format!("{base}/duplicate"))) class="form-grid" {
                        (field("New date", "", html! { input type="date" name="date" value=(tomorrow) required; }))
                        div { button type="submit" { "Copy" } }
                    }
                }))
                (card("Delete", "", html! {
                    p { "Deletes the practice and its pages. The drills stay in the team's library." }
                    (action_button(&cx.url(&format!("{base}/delete")), "Delete practice", "danger",
                        Some("Delete this practice? This can't be undone.")))
                }))
            }
        }
    };
    Ok(page(cx, Nav::Practices, "Edit practice", body))
}

async fn update(pc: PracticeCtx, Form(f): Form<PracticeForm>) -> AppResult<Response> {
    let cx = &pc.cx;
    let (id, me) = (pc.practice.id, cx.member());
    let mut fields = f.fields()?;
    let new_team = f.new_team.clone();
    let result = cx
        .db(move |db| {
            // Recorded for undo when it changes the parts (a new ice size resets them).
            db.tracked(id, me, || {
                resolve_team(db, me, &mut fields, &new_team)?;
                db.update_practice(id, fields).map(|reset| match reset {
                    0 => "Practice saved.".to_string(),
                    n => format!(
                        "Practice saved. {n} part(s) didn't fit the new ice and were reset."
                    ),
                })
            })
        })
        .await;
    Ok(flash::report(
        &cx.st,
        &cx.url(&format!("/practices/{id}")),
        result,
    ))
}

async fn delete(pc: PracticeCtx) -> AppResult<Response> {
    let (cx, id) = (&pc.cx, pc.practice.id);
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

async fn duplicate(pc: PracticeCtx, Form(f): Form<DuplicateForm>) -> AppResult<Response> {
    let (cx, id) = (&pc.cx, pc.practice.id);
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

async fn pdf(pc: PracticeCtx) -> AppResult<Response> {
    let (cx, id) = (&pc.cx, pc.practice.id);
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
    let _slot = cx.st.render_permit(cx.user.id).await?;
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
async fn preview(pc: PracticeCtx, Path((_, sid)): Path<(i64, i64)>) -> AppResult<Response> {
    let (cx, id) = (&pc.cx, pc.practice.id);
    let plan = cx
        .db(move |db| db.plan(id))
        .await?
        .ok_or(AppError::NotFound)?;
    let i = plan
        .segments
        .iter()
        .position(|s| s.id == sid)
        .ok_or(AppError::NotFound)?;
    let _slot = cx.st.render_permit(cx.user.id).await?;
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
    // So is the page layout itself: a new version of the app (which means a
    // restart) mustn't show previews cached from the old one.
    static STARTED: std::sync::OnceLock<std::time::SystemTime> = std::sync::OnceLock::new();
    STARTED.get_or_init(std::time::SystemTime::now).hash(&mut h);
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
    html! {
        ol class="stations" {
            @for p in &placed {
                li class="station" {
                    div class="station-head" {
                        // The same number as on the rink and the printed page.
                        @if let Some(number) = p.number { (badge(number)) }
                        strong { (p.slot.name) }
                        @if p.slot.name != p.slot.size.label() { span class="muted small" { (p.slot.size.label()) } }
                    }
                    @if let Some(d) = p.drill {
                        div class="station-drill" {
                            a href=(cx.url(&format!("/drills/{}", d.id))) { (d.name) }
                            span class="muted small" { " · " (seg.station_minutes()) " min · " (d.category.label()) }
                            (split_bar(d.focus))
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
                            option value=(d.id) selected[p.drill.is_some_and(|x| x.id == d.id)] { (offered_name(d)) }
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
                span class="muted small js-only" { "Drag it on the rink to move it; drag its blue handle (or press R while moving it) to turn it." }
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

/// A part's card. Its main content is the rink, where drills are dropped; the
/// stations' controls, the part's settings and the printed page are on other
/// tabs (with JavaScript; without it they all show, one under the other).
fn segment_card(cx: &Ctx, b: &Builder<'_>, i: usize) -> Markup {
    let plan = b.plan;
    let seg = &plan.segments[i];
    let base = format!("/segments/{}", seg.id);
    let last = i + 1 == plan.segments.len();
    let eq = plan.segment_equipment(seg);
    let placed = plan.slots(seg);
    let filled = placed.iter().filter(|p| p.drill.is_some()).count();
    let legend = board::segment_legend(plan, seg);
    let tab = |key: &str, label: Markup| {
        html! {
            button type="button" role="tab" class="part-tab" data-tab=(key)
                aria-selected=(if key == "rink" { "true" } else { "false" })
                aria-controls={ "seg-" (seg.id) "-" (key) } { (label) }
        }
    };
    html! {
        section class="card segment" id={ "seg-" (seg.id) } data-seg-id=(seg.id) data-index=(i)
            data-tab="rink" aria-label=(seg.display_title()) {
            div class="segment-head" {
                span class="grip js-only" draggable="true" title="Drag to move this part"
                    aria-hidden="true" { "⠿" }
                div class="head-main" {
                    div class="head-top" {
                        span class="muted small" { "Page " (i + 1) " · " (plan.time_range(i)) }
                    @if b.shows(BuilderExtra::FitCounts) {
                        span class="fit-count small" aria-live="polite" {}
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
                    (action_button(&cx.url(&format!("{base}/delete")), "Delete", "small danger", None))
                }
                    }
                    form method="post" action=(cx.url(&format!("{base}/head"))) class="head-form" {
                        input type="text" name="title" value=(seg.title) maxlength="80"
                            class="head-title" placeholder=(seg.display_title())
                            aria-label="Title of this part" data-autosubmit;
                        span class="pill head-minutes" {
                            input type="number" name="minutes" value=(seg.minutes) min="1" max="180"
                                required aria-label="Minutes" data-autosubmit;
                            " min"
                        }
                        button type="submit" class="small no-js-only" { "Save" }
                    }
                    @if b.shows(BuilderExtra::PartFocus) {
                        @let c = plan.segment_composition(seg);
                        @if c.classified() > 0.0 { (composition_bar(&c)) }
                    }
                }
            }
            div class="part-tabs js-only" role="tablist" aria-label="Part" {
                (tab("rink", html! { "Rink" }))
                (tab("stations", html! { "Stations " span class="muted" { (filled) "/" (placed.len()) } }))
                (tab("settings", html! { "Settings" }))
                (tab("page", html! { "Page" }))
            }
            div class="tab-panel" role="tabpanel" data-panel="rink" id={ "seg-" (seg.id) "-rink" } {
                div class="board-wrap" data-assign-url=(cx.url(&format!("{base}/stations/")))
                    data-goalie-url=(cx.url(&format!("{base}/goalies"))) {
                    (svg(board::segment_svg(plan, seg, true)))
                }
                @if !legend.is_empty() {
                    div class="board-legend compact" aria-label="Legend" { (legend_list(&legend)) }
                }
                div class="strip" {
                    span { strong { "Ice: " } (seg.layout.label()) }
                    @if let Some(r) = seg.rotation_text() { span { strong { "Rotation: " } (r) } }
                    @if seg.goalies.is_some() { span { strong { "Goalies: " } (GOALIE_WARMUP) } }
                    @if !eq.is_empty() { span { strong { "Equipment: " } (equipment_summary(&eq)) } }
                    @if !seg.notes.is_empty() { span { strong { "Notes: " } (seg.notes) } }
                }
            }
            div class="tab-panel" role="tabpanel" data-panel="stations" id={ "seg-" (seg.id) "-stations" } {
                h3 class="no-js-only" { "Stations" }
                (station_list(cx, plan, seg, b.library))
                (goalie_row(cx, plan, seg))
            }
            div class="tab-panel" role="tabpanel" data-panel="settings" id={ "seg-" (seg.id) "-settings" } {
                h3 class="no-js-only" { "Settings" }
                (segment_settings(cx, plan, seg))
            }
            div class="tab-panel" role="tabpanel" data-panel="page" id={ "seg-" (seg.id) "-page" } {
                h3 class="no-js-only" { "Printed page" }
                // Lazy: loaded only once the tab is shown (or scrolled to, without JavaScript).
                img class="page-img" src=(preview_url(cx, plan, seg)) loading="lazy" width="612" height="792"
                    alt={ "Printed page for " (seg.display_title()) };
            }
        }
    }
}

/// A "+" that opens a small form to add a part at position `at`.
fn add_between(cx: &Ctx, plan: &Plan, at: usize) -> Markup {
    let n = plan.segments.len();
    let open = n == 0;
    let suggestion = if at == 0 && n == 0 { "Warm-up" } else { "" };
    let label = match (at, n) {
        (_, 0) => "Add the first part".to_string(),
        (0, _) => "Add a part at the start".to_string(),
        (a, n) if a >= n => "Add a part at the end".to_string(),
        (a, _) => format!("Add a part after part {a}"),
    };
    html! {
        details class="add-between" open[open] {
            summary title=(label) aria-label=(label) {
                span class="plus" aria-hidden="true" { "+" }
                span class="add-label" { (label) }
            }
            div class="card add-card" {
                form method="post" action=(cx.url(&format!("/practices/{}/segments", plan.practice.id))) class="form-grid" {
                    input type="hidden" name="at" value=(at);
                    (field("Title", "", html! { input type="text" name="title" value=(suggestion) maxlength="80" placeholder="e.g. Skill stations"; }))
                    (field("Minutes", "", html! { input type="number" name="minutes" value="10" min="1" max="180" required; }))
                    (layout_fields(plan.practice.ice, Layout::default_for(plan.practice.ice)))
                    div { button type="submit" class="primary" { "Add part" } }
                }
            }
        }
    }
}

/// Undo, the change history, and the view toggles, above the parts.
fn builder_tools(cx: &Ctx, b: &Builder<'_>) -> Markup {
    let base = format!("/practices/{}", b.plan.practice.id);
    html! {
        div class="builder-tools" {
            @if let Some(last) = b.history.first() {
                form method="post" action=(cx.url(&format!("{base}/undo"))) class="inline" {
                    button type="submit" class="small" title={ "Undo: " (last.label) } {
                        "↶ Undo"
                    }
                }
                span class="muted small last-change" { "Last: " (last.label) }
            } @else {
                span class="muted small" { "Changes you make here can be undone." }
            }
            @if !b.history.is_empty() {
                details class="history menu-like" data-keep="history" {
                    summary class="btn small" { "History" }
                    div class="history-list card" {
                        p class="muted small" {
                            "Go back to how the practice was before any change. Going back is a change "
                            "too, so it can be undone."
                        }
                        ol {
                            @for c in b.history {
                                li {
                                    div {
                                        (c.label)
                                        div class="muted small" {
                                            (b.when(&c.at))
                                            @if let Some(who) = c.member.and_then(|m| b.names.get(&m.0)) { " · " (who) }
                                        }
                                    }
                                    form method="post" action=(cx.url(&format!("{base}/restore"))) class="inline" {
                                        input type="hidden" name="change" value=(c.id);
                                        button type="submit" class="small" { "Go back to before this" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            span class="spacer" {}
            @if !b.plan.segments.is_empty() {
                button type="button" class="small js-only" data-overview-toggle aria-pressed="false"
                    title="Shrink every part to its header and rink" { "Overview" }
            }
            button type="button" class="small js-only library-toggle" data-library-toggle
                aria-controls="library" aria-expanded="false" { "Drills" }
        }
    }
}

/// One block per part, sized by its minutes and coloured by its main
/// development level. A link to the part; with JavaScript, drag to reorder.
fn timeline(cx: &Ctx, plan: &Plan) -> Markup {
    html! {
        @if !plan.segments.is_empty() {
            nav class="timeline" aria-label="Parts of the practice"
                data-move-url=(cx.url("/segments/")) {
                @for (i, seg) in plan.segments.iter().enumerate() {
                    @let c = plan.segment_composition(seg);
                    @let top = top_level(&c);
                    a class={ "tl-block" @if let Some(l) = top { " lvl-" (l.key()) } @else { " tl-none" } }
                        href={ "#seg-" (seg.id) } draggable="true" data-seg-id=(seg.id) data-index=(i)
                        style={ "flex-grow:" (seg.minutes) }
                        title={ (seg.display_title()) " · " (seg.minutes) " min"
                            @if let Some(l) = top { " · mostly " (l.label().to_lowercase()) } } {
                        span class="tl-title" { (seg.display_title()) }
                        span class="tl-min" { (seg.minutes) "′" }
                    }
                }
            }
        }
    }
}

/// The level a part spends most of its counted time on.
fn top_level(c: &Composition) -> Option<Level> {
    if c.classified() <= 0.0 {
        return None;
    }
    Level::ALL
        .iter()
        .copied()
        .max_by(|a, b| c.minutes_at(*a).total_cmp(&c.minutes_at(*b)))
}

/// A slim bar that stays at the top while scrolling through the parts: the
/// length and each level's share against its target.
fn sticky_summary(plan: &Plan, team: &Team) -> Markup {
    let c = plan.composition();
    let targets = team.targets();
    html! {
        div class="sticky-summary js-only" data-sticky-summary hidden {
            strong { (plan.total_minutes()) " min" }
            span class="muted" { (plan.segments.len()) " parts" }
            @if c.classified() > 0.0 {
                @for l in Level::ALL {
                    @let pct = c.percent(*l).unwrap_or(0.0);
                    @let verdict = targets.map(|t| Verdict::of(pct, t.get(*l)));
                    span class={ "sum-level" @if let Some(v) = verdict { " " (v.key()) } }
                        title={ (l.label()) @if let Some(t) = targets { ", target " (t.get(*l)) "%" } } {
                        span class={ "swatch lvl-" (l.key()) } {}
                        (format!("{pct:.0}")) "%"
                        @match verdict {
                            Some(Verdict::Under) => { span class="verdict under" { "▼" } }
                            Some(Verdict::Over) => { span class="verdict over" { "▲" } }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

/// The drill library beside the parts: a scroll box of drill cards to drag
/// onto the rinks (or click, then click a station). On narrow screens it's a
/// drawer.
fn library_panel(cx: &Ctx, b: &Builder<'_>) -> Markup {
    let ice = b.plan.practice.ice;
    let usable: Vec<&Drill> = b.library.iter().filter(|d| d.ice.fits(ice)).collect();
    let categories: Vec<Category> = Category::ALL
        .iter()
        .copied()
        .filter(|c| usable.iter().any(|d| d.category == *c))
        .collect();
    let team = b.plan.practice.team_id;
    html! {
        aside class="library" id="library" data-library aria-label="Drill library" {
            div class="library-head" {
                h2 { "Drills" }
                a class="small" href=(cx.url(&format!("/drills/new?lib={}&from={}", Library::Team(team).key(), b.plan.practice.id))) { "+ New" }
                button type="button" class="small js-only library-close" data-library-toggle aria-label="Close the drill library" { "×" }
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
                select data-library-category aria-label="Category" {
                    option value="" { "All categories" }
                    @for c in &categories { option value=(c.key()) { (c.label()) } }
                }
                select data-library-level aria-label="Main development level" {
                    option value="" { "Any level" }
                    @for l in Level::ALL { option value=(l.key()) { "Mostly " (l.label().to_lowercase()) } }
                }
                @if b.team.targets().is_some() {
                    label class="check small" title="Drills that bring this practice's development focus closer to the team's targets, best first" {
                        input type="checkbox" data-library-closer;
                        " Closer to the targets"
                    }
                }
            }
            p class="muted small js-only library-hint" {
                "Drag a drill onto a station it fits (they light up), or click a drill, then a station. "
                "Your own drills are copied into the team's library when placed."
            }
            div class="library-scroll" { (library_cards(cx, b, &usable)) }
        }
    }
}

/// The drill cards (swapped after every change, so "used" counts and the
/// development suggestions stay current).
fn library_cards(cx: &Ctx, b: &Builder<'_>, usable: &[&Drill]) -> Markup {
    let plan = b.plan;
    let mut used: HashMap<i64, usize> = HashMap::new();
    for seg in &plan.segments {
        for st in &seg.stations {
            *used.entry(st.drill_id).or_default() += 1;
        }
    }
    let composition = plan.composition();
    let targets = b.team.targets();
    let from = plan.practice.id;
    html! {
        div id="library-cards" {
            @if usable.is_empty() {
                (empty("No drills fit this ice yet."))
            }
            ul class="library-grid" {
                @for (order, d) in usable.iter().enumerate() {
                    @let n = used.get(&d.id).copied().unwrap_or(0);
                    @let top = d.focus.map(|s| s.top());
                    @let help = match (targets, d.focus) {
                        (Some(t), Some(s)) => Some(closer_by(&composition, t, s, d.minutes as f32)),
                        _ => None,
                    };
                    li class="drill-chip drill-tile" draggable="true" tabindex="0" data-drill-id=(d.id)
                        data-size=(d.ice.key()) data-name=(d.name.to_lowercase())
                        data-category=(d.category.key()) data-level=[top.map(|l| l.key())]
                        data-help=[help.map(|h| format!("{h:.1}"))] data-order=(order)
                        aria-label={ (d.name) ", " (d.ice.label()) } {
                        div class="tile-thumb" { (svg(board::drill_svg(d.ice, &d.diagram, &d.name, ""))) }
                        div class="tile-body" {
                            div class="tile-title" {
                                strong { (d.name) }
                                a class="tile-edit" href=(cx.url(&format!("/drills/{}?from={from}", d.id)))
                                    title={ "Edit “" (d.name) "”" } aria-label={ "Edit " (d.name) } { "✎" }
                            }
                            (split_bar(d.focus))
                            div class="muted small" {
                                (d.category.label()) " · " (d.ice.label())
                                @if d.library.team().is_none() { " · yours" }
                            }
                            @if n > 0 { span class="used small" { "used ×" (n) } }
                        }
                        @let points = d.key_points();
                        @if !points.is_empty() {
                            ul class="peek-points" hidden { @for p in points { li { (p) } } }
                        }
                    }
                }
            }
        }
    }
}

/// Everything the builder's pieces draw from.
struct Builder<'a> {
    plan: &'a Plan,
    library: &'a [Drill],
    team: &'a Team,
    history: &'a [Change],
    /// Names of the people in the history, by user id.
    names: &'a HashMap<i64, String>,
    hidden: &'a [BuilderExtra],
}

impl Builder<'_> {
    fn shows(&self, e: BuilderExtra) -> bool {
        !self.hidden.contains(&e)
    }

    /// A history time (UTC from the database) in local time.
    fn when(&self, at: &str) -> String {
        chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S")
            .map(|t| {
                t.and_utc()
                    .with_timezone(&Local)
                    .format("%b %-d, %-I:%M %p")
                    .to_string()
            })
            .unwrap_or_else(|_| at.to_string())
    }
}

async fn show(pc: PracticeCtx) -> AppResult<Page> {
    let cx = pc.cx.clone();
    let (id, team, me) = (pc.practice.id, pc.practice.team_id, cx.member());
    let (plan, library, team, history) = cx
        .db(move |db| {
            Ok((
                db.plan(id)?,
                builder_library(db, team, me)?,
                db.team(team)?,
                db.history(id, HISTORY_SHOWN)?,
            ))
        })
        .await?;
    let team = team.ok_or(AppError::NotFound)?;
    let plan = plan.ok_or(AppError::NotFound)?;
    let mut names = HashMap::new();
    for m in history.iter().filter_map(|c| c.member) {
        if let std::collections::hash_map::Entry::Vacant(e) = names.entry(m.0)
            && let Some(u) = cx.st.auth.user(m.0)?
        {
            e.insert(u.name().to_string());
        }
    }
    let b = Builder {
        plan: &plan,
        library: &library,
        team: &team,
        history: &history,
        names: &names,
        hidden: &cx.user.hidden,
    };
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
    let extras: Vec<&str> = BuilderExtra::ALL
        .iter()
        .filter(|e| b.shows(**e))
        .map(|e| e.key())
        .collect();
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
        div id="practice-focus" {
            @if b.shows(BuilderExtra::Focus) { (focus_card(&cx, &plan, &team)) }
        }
        div id="practice-timeline" {
            @if b.shows(BuilderExtra::Timeline) { (timeline(&cx, &plan)) }
        }
        div id="practice-sticky" {
            @if b.shows(BuilderExtra::Sticky) { (sticky_summary(&plan, &team)) }
        }
        @if library.is_empty() { (empty_library_note(&cx)) }
        div class="builder" data-builder data-extras=(extras.join(" ")) {
            // With htmx, every change in the parts (placing a drill, notes,
            // settings, reordering) swaps just this column, the summary above
            // it and the drill cards, keeping the scroll position. Without it,
            // the forms post and redirect back as usual.
            section class="plan-card" aria-labelledby="plan-title" {
            h2 id="plan-title" class="plan-title" { "Practice plan" }
            div class="builder-main" id="builder-main" hx-target="#builder-main" hx-select="#builder-main"
                hx-swap="outerHTML show:none" hx-push-url="false"
                hx-select-oob="#practice-head,#practice-tiles,#practice-focus,#practice-timeline,#practice-sticky,#library-cards,#toasts" {
                (builder_tools(&cx, &b))
                div class="parts" data-move-url=(cx.url("/segments/")) {
                    @if plan.segments.is_empty() {
                        (notice("info", html! { "This practice has no parts yet. Each part is a block of time and becomes one page of the plan." }))
                    } @else {
                        (add_between(&cx, &plan, 0))
                    }
                    @for i in 0..plan.segments.len() {
                        (segment_card(&cx, &b, i))
                        (add_between(&cx, &plan, i + 1))
                    }
                    @if plan.segments.is_empty() { (add_between(&cx, &plan, 0)) }
                }
            }
            }
            div class="divider js-only" role="separator" aria-orientation="vertical" tabindex="0"
                aria-label="Resize the drill library (arrow keys)" data-divider {}
            (library_panel(&cx, &b))
        }
    };
    Ok(page(&cx, Nav::Practices, p.display_title(), body))
}

/// How many recent changes the History menu lists.
const HISTORY_SHOWN: usize = 30;

#[derive(Deserialize)]
struct RestoreForm {
    change: String,
}

/// Undo the practice's latest change.
async fn undo(pc: PracticeCtx) -> AppResult<Response> {
    let (cx, id, me) = (&pc.cx, pc.practice.id, pc.cx.member());
    let result = cx
        .db(move |db| {
            db.undo(id, me)
                .map(|m| m.unwrap_or_else(|| "Nothing to undo.".to_string()))
        })
        .await;
    Ok(flash::report(
        &cx.st,
        &cx.url(&format!("/practices/{id}")),
        result,
    ))
}

/// Go back to how the practice was before one of its changes. The change is
/// looked up within this practice only.
async fn restore(pc: PracticeCtx, Form(f): Form<RestoreForm>) -> AppResult<Response> {
    let (cx, id, me) = (&pc.cx, pc.practice.id, pc.cx.member());
    let change: i64 = form::number(&f.change, "Change")?;
    let result = cx.db(move |db| db.restore(id, change, me)).await;
    Ok(flash::report(
        &cx.st,
        &cx.url(&format!("/practices/{id}")),
        result,
    ))
}

/// The drills the builder offers: the team's, then the user's own (placing
/// one copies it into the team's library). Own drills the team already has a
/// copy of are left out, so each shows once.
fn builder_library(
    db: &localapp_core::db::Database,
    team: i64,
    me: MemberId,
) -> anyhow::Result<Vec<Drill>> {
    let all = DrillFilter::default();
    let mut drills = db.drills(&all, Library::Team(team))?;
    let copied: std::collections::HashSet<i64> =
        drills.iter().filter_map(|d| d.copied_from).collect();
    drills.extend(
        db.drills(&all, Library::Personal(me))?
            .into_iter()
            .filter(|d| !copied.contains(&d.id)),
    );
    Ok(drills)
}

/// A drill's name in the builder, marking the user's own (not yet the team's).
fn offered_name(d: &Drill) -> String {
    match d.library {
        Library::Personal(_) => format!("{} (yours)", d.name),
        Library::Team(_) => d.name.clone(),
    }
}

/// How this practice's time splits across the development levels, per
/// player, against the team's targets. Swapped with the builder (htmx), so it
/// follows every drill placed or removed.
fn focus_card(cx: &Ctx, plan: &Plan, team: &Team) -> Markup {
    let guide = cx.url(GUIDE_PATH);
    let targets = team.targets();
    let source = targets_source(team.age, team.custom_targets.is_some());
    html! {
        details class="card focus-card" data-keep="practice-focus" open {
            summary {
                h2 { "Development focus" }
                span class="muted small" {
                    @if targets.is_some() {
                        " compared with " (source)
                    } @else {
                        " · "
                        a href=(cx.url(&format!("/teams/{}", team.id))) { "set the team's age division" }
                        " to compare with Hockey Canada's targets"
                    }
                }
            }
            (composition_panel(&plan.composition(), targets, &guide))
            p class="muted small" {
                "Per player: a part's minutes are shared equally by its drills (each player does them in turn, or one of them). "
                a href={ (guide) @if let Some(a) = team.age { "#" (a.key()) } } { "Hockey Canada's guidance" }
            }
        }
    }
}
