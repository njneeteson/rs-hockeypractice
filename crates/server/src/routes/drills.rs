//! The drill library: list, create, edit (with the diagram editor), copy, delete.

use crate::error::{AppError, AppResult};
use crate::extract::Ctx;
use crate::flash;
use crate::form;
use crate::state::Shared;
use crate::views::components::{card, empty, heading, legend_list, link_button, notice, svg};
use crate::views::forms::{Live, LiveTarget, action_button, field, live_form};
use crate::views::{Nav, Page, page};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use localapp_core::board;
use localapp_core::db::DrillFilter;
use localapp_core::diagram::{self, Diagram};
use localapp_core::drill::{Category, Drill, DrillFields, Equipment};
use localapp_core::rink::{Area, IceSize, MAX_LANES, PracticeIce};
use maud::{Markup, html};
use serde::Deserialize;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/drills", get(list).post(create))
        .route("/drills/new", get(new_page))
        .route("/drills/samples", post(add_samples))
        .route("/drills/{id}", get(edit_page).post(update))
        .route("/drills/{id}/duplicate", post(duplicate))
        .route("/drills/{id}/delete", post(delete))
}

// ---- List -------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct ListQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    ice: String,
    #[serde(default)]
    category: String,
}

/// A `<select>` over an enum with an "any" first option.
fn filter_select<T: localapp_core::enum_macro::KeyEnum + PartialEq>(
    name: &str,
    any: &str,
    selected: Option<T>,
) -> Markup {
    html! {
        select name=(name) {
            option value="" { (any) }
            @for v in T::all() {
                option value=(v.key()) selected[selected == Some(*v)] { (v.label()) }
            }
        }
    }
}

/// A `<select>` over every ice size with an "any" first option.
fn size_select(name: &str, any: &str, selected: Option<IceSize>) -> Markup {
    html! {
        select name=(name) {
            option value="" { (any) }
            @for s in IceSize::all() {
                option value=(s.key()) selected[selected == Some(s)] { (s.label()) }
            }
        }
    }
}

pub fn drill_card(cx: &Ctx, d: &Drill) -> Markup {
    html! {
        a class="drill-card" href=(cx.url(&format!("/drills/{}", d.id))) {
            (svg(board::drill_svg(d.ice, &d.diagram, &d.name, "")))
            div class="drill-card-body" {
                strong { (d.name) }
                div class="muted small" {
                    (d.ice.label()) " · " (d.category.label()) " · " (d.minutes) " min"
                }
                @if !d.equipment.is_empty() {
                    div class="small" { (d.equipment_summary()) }
                }
            }
        }
    }
}

async fn list(cx: Ctx, Query(q): Query<ListQuery>) -> AppResult<Page> {
    let filter = DrillFilter {
        search: q.q.clone(),
        ice: IceSize::from_key(&q.ice),
        category: Category::from_key(&q.category),
    };
    let (drills, total) = cx
        .db(move |db| {
            let all = db.drills(&DrillFilter::default())?.len();
            Ok((db.drills(&filter)?, all))
        })
        .await?;
    let new_url = match IceSize::from_key(&q.ice) {
        Some(s) => cx.url(&format!("/drills/new?ice={}", s.key())),
        None => cx.url("/drills/new"),
    };
    let body = html! {
        (heading("Drill library", "Drills are shared by everyone and can be used in any practice.",
            link_button(&new_url, "New drill", true)))
        (live_form(&cx.url("/drills"), "toolbar", LiveTarget { id: "drill-results", also: &[] }, Live::Filter, html! {
            (field("Search", "", html! { input type="search" name="q" value=(q.q) placeholder="Name or text"; }))
            (field("Ice", "", size_select("ice", "Any size", IceSize::from_key(&q.ice))))
            (field("Category", "", filter_select("category", "Any category", Category::from_key(&q.category))))
            noscript { button type="submit" { "Apply" } }
        }))
        div id="drill-results" class="refreshing" {
            @if total == 0 {
                (card("Your library is empty", "", html! {
                    p { "Create your first drill, or start with a few examples (one for each ice size) that you can edit." }
                    div class="actions" {
                        (link_button(&new_url, "New drill", true))
                        (action_button(&cx.url("/drills/samples"), "Add example drills", "", None))
                    }
                }))
            } @else if drills.is_empty() {
                (empty("No drills match."))
            } @else {
                p class="muted" { (drills.len()) " of " (total) " drills" }
                div class="drill-grid" {
                    @for d in &drills { (drill_card(&cx, d)) }
                }
            }
        }
    };
    Ok(page(&cx, Nav::Drills, "Drill library", body))
}

async fn add_samples(cx: Ctx) -> AppResult<Response> {
    let result = cx
        .db(|db| {
            let mut n = 0;
            for d in localapp_core::samples::drills() {
                db.create_drill(d)?;
                n += 1;
            }
            Ok(format!("Added {n} example drills."))
        })
        .await;
    Ok(flash::report(&cx.st, &cx.url("/drills"), result))
}

// ---- Editor -------------------------------------------------------------------

const DIAGRAM_HELP: &str = "Pick a tool, then click the rink. For lines, drag to draw a curve, or \
    click several points and double-click (or press Enter) to finish. With Select, click an item \
    or drag a box on empty ice to select several (Shift or Ctrl+click adds or removes one, Ctrl+A \
    selects all); drag to move them together, arrow keys nudge (Shift: 5 ft), Delete removes \
    them; double-click text to edit it. Ctrl+C, Ctrl+X, Ctrl+V and Ctrl+D copy, cut, paste and \
    duplicate, also between drills. Nets, barriers and danglers turn with the round handle; ⟲ ⟳ \
    or R / Shift+R turn the selection 15° (hold Shift on the handle to turn freely). A net dropped \
    near a goal line snaps onto it. Zoom with + / − (or Ctrl + mouse wheel, 0 to fit); when \
    zoomed in, scroll or hold Space and drag to move around.";

/// The browser drawing editor (shown only with JavaScript) and its no-JS fallback.
fn diagram_editor(ice: IceSize, diagram: &Diagram, name: &str) -> Markup {
    let frames: Vec<String> = IceSize::all()
        .into_iter()
        .map(|s| {
            let v = board::view(s.frame());
            format!(r#""{}":[{},{},{},{}]"#, s.key(), v.x, v.y, v.w, v.h)
        })
        .collect();
    let frames = format!("{{{}}}", frames.join(","));
    let rink = board::rink_svg(PracticeIce::Full.frame(), r#" data-editor-svg="""#);
    let players = [
        ("X", "Player X"),
        ("O", "Player O"),
        ("C", "Coach"),
        ("G", "Goalie"),
        ("F", "Forward"),
        ("D", "Defence"),
    ];
    let gear = [
        ("net", "Net", "⊐"),
        ("cone", "Cone", "▲"),
        ("puck", "Puck", "●"),
        ("pucks", "Pile of pucks", "⁘"),
        ("tire", "Tire", "◯"),
        ("barrier", "Barrier", "▬"),
        ("dangler", "Dangler", "⊢⊣"),
    ];
    html! {
        div class="diagram-editor js-only" data-diagram-editor data-frames=(frames) data-size=(ice.key()) {
            div class="editor-tools" role="toolbar" aria-label="Drawing tools" {
                div class="tool-group" {
                    button type="button" class="tool" data-tool="select" aria-pressed="true" title="Select and move (V)" { "↖ Select" }
                }
                div class="tool-group" aria-label="Players" {
                    @for (label, title) in players {
                        button type="button" class="tool sym" data-tool="player" data-label=(label) title=(title) { (label) }
                    }
                    button type="button" class="tool" data-tool="player" data-label="" title="Player with your own label" { "A…" }
                }
                div class="tool-group" aria-label="Equipment" {
                    @for (key, title, icon) in gear {
                        button type="button" class={ "tool gear-tool gear-" (key) } data-tool=(key) title=(title) aria-label=(title) {
                            @match diagram::equipment_icon(key) {
                                Some(i) => (svg(i)),
                                None => (icon),
                            }
                        }
                    }
                    button type="button" class="tool" data-tool="text" title="Text label" { "T" }
                }
                div class="tool-group" aria-label="Lines" {
                    @for style in diagram::LineStyle::ALL {
                        button type="button" class="tool line-tool" data-tool="line" data-style=(style.key()) title=(style.label()) {
                            (svg(diagram::line_sample(style)))
                        }
                    }
                    select data-line-end aria-label="Line end" title="How new lines end" {
                        option value="arrow" { "→ Arrow" }
                        option value="stop" { "‖ Stop" }
                        option value="none" { "— None" }
                    }
                }
                div class="tool-group" {
                    button type="button" class="tool" data-action="rotateLeft" title="Turn the selection left (Shift+R)" { "⟲" }
                    button type="button" class="tool" data-action="rotateRight" title="Turn the selection right (R)" { "⟳" }
                    button type="button" class="tool" data-action="copy" title="Copy the selection (Ctrl+C)" { "Copy" }
                    button type="button" class="tool" data-action="paste" title="Paste (Ctrl+V)" { "Paste" }
                    button type="button" class="tool" data-action="duplicate" title="Duplicate the selection (Ctrl+D)" { "⧉" }
                    button type="button" class="tool" data-action="delete" title="Delete the selection (Del)" { "🗑" }
                    button type="button" class="tool" data-action="undo" title="Undo (Ctrl+Z)" { "↶" }
                    button type="button" class="tool danger" data-action="clear" title="Remove everything" { "Clear" }
                }
                div class="tool-group" aria-label="Zoom" {
                    button type="button" class="tool" data-action="zoomOut" title="Zoom out (−)" aria-label="Zoom out" { "−" }
                    button type="button" class="tool zoom-level" data-action="zoomFit" data-zoom-level title="Fit the whole ice (0)" { "100%" }
                    button type="button" class="tool" data-action="zoomIn" title="Zoom in (+)" aria-label="Zoom in" { "+" }
                }
            }
            div class="editor-canvas" { (svg(rink)) }
            p class="muted small" { (DIAGRAM_HELP) }
        }
        div class="no-js-only" {
            (svg(board::drill_svg(ice, diagram, name, "")))
            (field("Diagram data (JSON)", "Drawing needs JavaScript; without it you can edit the diagram's data here.",
                html! { textarea name="diagram" rows="6" class="mono diagram-json" data-diagram-json { (diagram.to_json()) } }))
        }
        details class="legend-key" {
            summary { "What the symbols mean" }
            (legend_list(&diagram::legend_all()))
        }
    }
}

fn drill_form(cx: &Ctx, action: &str, f: &DrillFields, used: u32) -> Markup {
    let qty = |e: Equipment| {
        f.equipment
            .iter()
            .find(|(k, _)| *k == e)
            .map(|(_, q)| *q)
            .unwrap_or(0)
    };
    html! {
        form method="post" action=(action) class="drill-form" data-drill-form {
            (card("Drill", "", html! {
                div class="form-grid" {
                    (field("Name", "", html! { input type="text" name="name" value=(f.name) required maxlength="80"; }))
                    (field("Category", "", html! {
                        select name="category" {
                            @for c in Category::ALL {
                                option value=(c.key()) selected[*c == f.category] { (c.label()) }
                            }
                        }
                    }))
                    (field("Ice needed", "Which part of the ice the drill is drawn on. Practice pages place it in a station of this size.", html! {
                        select name="area" data-editor-area {
                            @for a in Area::ALL {
                                option value=(a.key()) selected[*a == f.ice.area] title=(a.hint()) { (a.label()) }
                            }
                        }
                    }))
                    (field("Width", "A lane drill uses one of 2–4 lanes running along the area (full ice, half ice or end zone).", html! {
                        select name="lanes" data-editor-lanes {
                            @for l in 1..=MAX_LANES {
                                option value=(l) selected[l == f.ice.lanes] {
                                    @if l == 1 { "Whole width" } @else { "1 of " (l) " lanes" }
                                }
                            }
                        }
                    }))
                    (field("Minutes", "Suggested length.", html! {
                        input type="number" name="minutes" value=(f.minutes) min="1" max="120" required;
                    }))
                }
                @if used > 0 {
                    p class="muted small" {
                        "Used on " (used) " practice page(s). Changes show up in those practices; the ice size can't change while it's in use."
                    }
                }
            }))
            (card("Diagram", "", diagram_editor(f.ice, &f.diagram, &f.name)))
            div class="grid" {
                (card("How to run it", "Shown under the rink on the printed plan.", html! {
                    textarea name="description" rows="8" placeholder="Set-up, what players do, how it restarts…" { (f.description) }
                }))
                (card("Key teaching points", "One per line. Printed as a bulleted list.", html! {
                    textarea name="points" rows="8" placeholder="Knees bent\nHeads up\nStick on the ice" { (f.points) }
                }))
            }
            (card("Equipment", "Quantities for one group running this drill.", html! {
                div class="equipment-grid" {
                    @for e in Equipment::ALL {
                        (field(e.label(), "", html! {
                            input type="number" name={ "eq_" (e.key()) } value=(qty(*e)) min="0" max="999"
                                data-equipment=(e.key());
                        }))
                    }
                }
                p class="js-only" {
                    button type="button" data-count-equipment { "Count equipment from the diagram" }
                }
            }))
            div class="form-actions" {
                button type="submit" class="primary" { "Save drill" }
                a class="btn" href=(cx.url("/drills")) { "Cancel" }
            }
        }
    }
}

#[derive(Deserialize)]
struct NewQuery {
    #[serde(default)]
    ice: String,
}

fn blank(ice: IceSize) -> DrillFields {
    DrillFields {
        name: String::new(),
        ice,
        category: Category::Skating,
        minutes: 10,
        description: String::new(),
        points: String::new(),
        diagram: Diagram::default(),
        equipment: Vec::new(),
    }
}

async fn new_page(cx: Ctx, Query(q): Query<NewQuery>) -> AppResult<Page> {
    let f = blank(IceSize::from_key(&q.ice).unwrap_or(IceSize::HALF));
    let body = html! {
        (heading("New drill", "", link_button(&cx.url("/drills"), "← Drill library", false)))
        (drill_form(&cx, &cx.url("/drills"), &f, 0))
    };
    Ok(page(&cx, Nav::Drills, "New drill", body))
}

/// The drill's size: a size key (`ice`), or an area and a lane count.
fn ice_size(key: &str, area: &str, lanes: &str) -> AppResult<IceSize> {
    if !key.trim().is_empty() {
        return IceSize::from_key(key)
            .ok_or_else(|| AppError::bad_request(format!("Unknown ice size “{key}”.")));
    }
    let area: Area = form::key(area, "area")?;
    let lanes: u8 = match lanes.trim() {
        "" => 1,
        l => form::number(l, "Lanes")?,
    };
    IceSize::new(area, lanes).map_err(|e| AppError::bad_request(e.to_string()))
}

/// Read the drill form (equipment fields are `eq_<key>`).
fn parse(pairs: &[(String, String)]) -> AppResult<DrillFields> {
    let get = |k: &str| {
        pairs
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    };
    let mut equipment = Vec::new();
    for e in Equipment::ALL {
        let v = get(&format!("eq_{}", e.key()));
        if !v.trim().is_empty() {
            let q: u32 = form::number(v, e.label())?;
            equipment.push((*e, q));
        }
    }
    let diagram =
        Diagram::from_json(get("diagram")).map_err(|e| AppError::bad_request(e.to_string()))?;
    Ok(DrillFields {
        name: form::text(get("name"), "Name")?,
        ice: ice_size(get("ice"), get("area"), get("lanes"))?,
        category: form::key(get("category"), "category")?,
        minutes: form::number(get("minutes"), "Minutes")?,
        description: get("description").to_string(),
        points: get("points").to_string(),
        diagram,
        equipment,
    })
}

/// The form again with the submitted values and what went wrong (nothing drawn is lost).
fn rejected(
    cx: &Ctx,
    title: &str,
    action: &str,
    f: &DrillFields,
    used: u32,
    error: &str,
) -> Response {
    let body = html! {
        (heading(title, "", link_button(&cx.url("/drills"), "← Drill library", false)))
        (notice("error", html! { "Not saved: " (error) }))
        (drill_form(cx, action, f, used))
    };
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        page(cx, Nav::Drills, title, body),
    )
        .into_response()
}

async fn create(cx: Ctx, Form(pairs): Form<Vec<(String, String)>>) -> AppResult<Response> {
    let f = parse(&pairs)?;
    let save = f.clone();
    match cx.db(move |db| db.create_drill(save)).await {
        Ok(id) => Ok(flash::info(
            &cx.st,
            &cx.url(&format!("/drills/{id}")),
            "Drill saved.",
        )),
        Err(e) => Ok(rejected(
            &cx,
            "New drill",
            &cx.url("/drills"),
            &f,
            0,
            &format!("{e:#}"),
        )),
    }
}

async fn edit_page(cx: Ctx, Path(id): Path<i64>) -> AppResult<Page> {
    let (d, used) = cx
        .db(move |db| Ok((db.drill(id)?, db.drill_usage(id)?)))
        .await?;
    let d = d.ok_or(AppError::NotFound)?;
    let base = format!("/drills/{id}");
    let body = html! {
        (heading(&d.name, &format!("{} · {} · last changed {}", d.ice.label(), d.category.label(), d.updated_at),
            html! {
                a class="btn" href=(cx.url("/drills")) { "← Drill library" }
                (action_button(&cx.url(&format!("{base}/duplicate")), "Duplicate", "", None))
                @if used == 0 {
                    (action_button(&cx.url(&format!("{base}/delete")), "Delete", "danger",
                        Some("Delete this drill? This can't be undone.")))
                }
            }))
        (drill_form(&cx, &cx.url(&base), &d.fields(), used))
    };
    Ok(page(&cx, Nav::Drills, &d.name, body))
}

async fn update(
    cx: Ctx,
    Path(id): Path<i64>,
    Form(pairs): Form<Vec<(String, String)>>,
) -> AppResult<Response> {
    let f = parse(&pairs)?;
    let save = f.clone();
    let result = cx
        .db(move |db| {
            let used = db.drill_usage(id)?;
            Ok((db.update_drill(id, save), used))
        })
        .await?;
    match result {
        (Ok(()), _) => Ok(flash::info(
            &cx.st,
            &cx.url(&format!("/drills/{id}")),
            "Drill saved.",
        )),
        (Err(e), used) => {
            let action = cx.url(&format!("/drills/{id}"));
            Ok(rejected(
                &cx,
                "Edit drill",
                &action,
                &f,
                used,
                &format!("{e:#}"),
            ))
        }
    }
}

async fn duplicate(cx: Ctx, Path(id): Path<i64>) -> AppResult<Response> {
    match cx.db(move |db| db.duplicate_drill(id)).await {
        Ok(new) => Ok(flash::info(
            &cx.st,
            &cx.url(&format!("/drills/{new}")),
            "Copied. You're editing the copy.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &cx.url(&format!("/drills/{id}")),
            format!("{e:#}"),
        )),
    }
}

async fn delete(cx: Ctx, Path(id): Path<i64>) -> AppResult<Response> {
    match cx.db(move |db| db.delete_drill(id)).await {
        Ok(()) => Ok(flash::info(&cx.st, &cx.url("/drills"), "Drill deleted.")),
        Err(e) => Ok(flash::error(
            &cx.st,
            &cx.url(&format!("/drills/{id}")),
            format!("{e:#}"),
        )),
    }
}

/// Used by the practice builder: a short note when the library is empty.
pub fn empty_library_note(cx: &Ctx) -> Markup {
    notice(
        "info",
        html! {
            "The drill library is empty. "
            a href=(cx.url("/drills")) { "Add drills" } " first, then place them on the ice here."
        },
    )
}
