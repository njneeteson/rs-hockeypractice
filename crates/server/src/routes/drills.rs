//! The drill library: list, create, edit (with the diagram editor), copy, delete.

use crate::error::{AppError, AppResult};
use crate::extract::{Ctx, DrillCtx};
use crate::flash;
use crate::form;
use crate::state::Shared;
use crate::views::components::{card, empty, heading, legend_list, link_button, notice, svg};
use crate::views::development::{GUIDE_PATH, focus_fields, parse_focus, split_bar};
use crate::views::forms::{Live, LiveTarget, action_button, field, live_form};
use crate::views::{Nav, Page, page};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use localapp_core::board;
use localapp_core::db::DrillFilter;
use localapp_core::diagram::{self, Diagram};
use localapp_core::drill::{Category, Drill, DrillFields, Equipment};
use localapp_core::owner::Library;
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
        .route("/drills/{id}/copy", post(copy))
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
    /// Which library: `me` (default) or `team-<id>`.
    #[serde(default)]
    lib: String,
}

/// The libraries a user may use: their own, then each of their teams', with
/// a name for each.
pub async fn libraries(cx: &Ctx) -> AppResult<Vec<(Library, String)>> {
    let me = cx.member();
    let teams = cx.db(move |db| db.teams_for(me)).await?;
    let mut libs = vec![(Library::Personal(me), "My drills".to_string())];
    libs.extend(
        teams
            .into_iter()
            .map(|(t, _)| (Library::Team(t.id), format!("{} (team)", t.name))),
    );
    Ok(libs)
}

/// A library from a form or query value, if the user may use it (else 404).
pub async fn library_param(cx: &Ctx, key: &str) -> AppResult<(Library, String)> {
    let lib = Library::from_key(key, cx.member()).ok_or(AppError::NotFound)?;
    libraries(cx)
        .await?
        .into_iter()
        .find(|(l, _)| *l == lib)
        .ok_or(AppError::NotFound)
}

fn library_url(cx: &Ctx, lib: Library) -> String {
    match lib {
        Library::Personal(_) => cx.url("/drills"),
        _ => cx.url(&format!("/drills?lib={}", lib.key())),
    }
}

/// Where a drill form goes back to: the practice it was opened from (`from`,
/// a practice id), else the library. With the back link's label.
fn back_to(cx: &Ctx, from: Option<i64>, lib: Library) -> (String, &'static str) {
    match from {
        Some(p) => (cx.url(&format!("/practices/{p}")), "← Back to the practice"),
        None => (library_url(cx, lib), "← Drill library"),
    }
}

fn library_hint(lib: Library) -> &'static str {
    match lib {
        Library::Personal(_) => {
            "Your own drills: only you see them. Placing one in a practice copies it into the \
             team's library."
        }
        Library::Team(_) => {
            "Shared by everyone in the team, and used by its practices. Copy a drill to your own \
             library to make your own version."
        }
    }
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
                div class="drill-focus" {
                    (split_bar(d.focus))
                    @if d.focus.is_none() {
                        span class="small warn-text" { "Development split not set" }
                    }
                }
            }
        }
    }
}

async fn list(cx: Ctx, Query(q): Query<ListQuery>) -> AppResult<Page> {
    let (lib, lib_name) = library_param(&cx, &q.lib).await?;
    let libs = libraries(&cx).await?;
    let filter = DrillFilter {
        search: q.q.clone(),
        ice: IceSize::from_key(&q.ice),
        category: Category::from_key(&q.category),
    };
    let (drills, total) = cx
        .db(move |db| Ok((db.drills(&filter, lib)?, db.drill_count(lib)?)))
        .await?;
    let mut new_url = format!("/drills/new?lib={}", lib.key());
    if let Some(s) = IceSize::from_key(&q.ice) {
        new_url.push_str(&format!("&ice={}", s.key()));
    }
    let new_url = cx.url(&new_url);
    let body = html! {
        div id="library-head" {
            (heading(&lib_name, library_hint(lib), link_button(&new_url, "New drill", true)))
        }
        (live_form(&cx.url("/drills"), "toolbar", LiveTarget { id: "drill-results", also: &["library-head"] }, Live::Filter, html! {
            @if libs.len() > 1 {
                (field("Library", "", html! {
                    select name="lib" {
                        @for (l, name) in &libs {
                            option value=(l.key()) selected[*l == lib] { (name) }
                        }
                    }
                }))
            }
            (field("Search", "", html! { input type="search" name="q" value=(q.q) placeholder="Name or text"; }))
            (field("Ice", "", size_select("ice", "Any size", IceSize::from_key(&q.ice))))
            (field("Category", "", filter_select("category", "Any category", Category::from_key(&q.category))))
            noscript { button type="submit" { "Apply" } }
        }))
        div id="drill-results" class="refreshing" {
            @if total == 0 {
                (card("This library is empty", "", html! {
                    p { "Create your first drill, or start with a few examples (one for each ice size) that you can edit." }
                    div class="actions" {
                        (link_button(&new_url, "New drill", true))
                        form method="post" action=(cx.url("/drills/samples")) class="inline" {
                            input type="hidden" name="lib" value=(lib.key());
                            button type="submit" class="btn" { "Add example drills" }
                        }
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

#[derive(Deserialize)]
struct LibForm {
    #[serde(default)]
    lib: String,
}

async fn add_samples(cx: Ctx, Form(f): Form<LibForm>) -> AppResult<Response> {
    let (lib, _) = library_param(&cx, &f.lib).await?;
    let result = cx
        .db(move |db| {
            let mut n = 0;
            for d in localapp_core::samples::drills() {
                db.create_drill(lib, d)?;
                n += 1;
            }
            Ok(format!("Added {n} example drills."))
        })
        .await;
    Ok(flash::report(&cx.st, &library_url(&cx, lib), result))
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

/// The drill editor. `new_in`: the library a new drill goes in (a hidden field).
fn drill_form(
    action: &str,
    f: &DrillFields,
    used: u32,
    back: &str,
    new_in: Option<Library>,
    guide: &str,
    from: Option<i64>,
) -> Markup {
    let qty = |e: Equipment| {
        f.equipment
            .iter()
            .find(|(k, _)| *k == e)
            .map(|(_, q)| *q)
            .unwrap_or(0)
    };
    html! {
        form method="post" action=(action) class="drill-form" data-drill-form {
            @if let Some(lib) = new_in {
                input type="hidden" name="lib" value=(lib.key());
            }
            @if let Some(p) = from {
                input type="hidden" name="from" value=(p);
            }
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
                        "Used on " (used) " of the team's practice page(s). Changes show up in those practices; the ice size can't change while it's in use."
                    }
                }
            }))
            (card("Development", "How the drill's time splits across Hockey Canada's development levels. Practices add these up and compare them with the team's targets.",
                focus_fields(f.focus, guide)))
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
                a class="btn" href=(back) { "Cancel" }
            }
        }
    }
}

#[derive(Deserialize)]
struct NewQuery {
    #[serde(default)]
    ice: String,
    #[serde(default)]
    lib: String,
    #[serde(default)]
    from: String,
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
        focus: None,
    }
}

async fn new_page(cx: Ctx, Query(q): Query<NewQuery>) -> AppResult<Page> {
    let (lib, name) = library_param(&cx, &q.lib).await?;
    let f = blank(IceSize::from_key(&q.ice).unwrap_or(IceSize::HALF));
    let from = form::opt_id(&q.from);
    let (back, back_label) = back_to(&cx, from, lib);
    let body = html! {
        (heading("New drill", &format!("In {name}."), link_button(&back, back_label, false)))
        (drill_form(&cx.url("/drills"), &f, 0, &back, Some(lib), &cx.url(GUIDE_PATH), from))
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

/// Read the drill form (equipment fields are `eq_<key>`, the development split
/// `focus_<level>`). A missing or wrong split comes back as a message beside the
/// fields, so the form can be shown again with nothing lost.
fn parse(pairs: &[(String, String)]) -> AppResult<(DrillFields, Option<String>)> {
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
    let (focus, focus_error) = match parse_focus(|k| get(k).to_string()) {
        Ok(s) => (Some(s), None),
        Err(e) => (None, Some(e)),
    };
    let fields = DrillFields {
        name: form::text(get("name"), "Name")?,
        ice: ice_size(get("ice"), get("area"), get("lanes"))?,
        category: form::key(get("category"), "category")?,
        minutes: form::number(get("minutes"), "Minutes")?,
        description: get("description").to_string(),
        points: get("points").to_string(),
        diagram,
        equipment,
        focus,
    };
    Ok((fields, focus_error))
}

/// The form again with the submitted values and what went wrong (nothing drawn is lost).
struct Rejected<'a> {
    title: &'a str,
    action: &'a str,
    back: &'a str,
    used: u32,
    new_in: Option<Library>,
    from: Option<i64>,
}

fn rejected(cx: &Ctx, r: Rejected<'_>, f: &DrillFields, error: &str) -> Response {
    let body = html! {
        (heading(r.title, "", link_button(r.back, "← Back", false)))
        (notice("error", html! { "Not saved: " (error) }))
        (drill_form(r.action, f, r.used, r.back, r.new_in, &cx.url(GUIDE_PATH), r.from))
    };
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        page(cx, Nav::Drills, r.title, body),
    )
        .into_response()
}

fn field_value<'a>(pairs: &'a [(String, String)], key: &str) -> &'a str {
    pairs
        .iter()
        .find(|(n, _)| n == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

async fn create(cx: Ctx, Form(pairs): Form<Vec<(String, String)>>) -> AppResult<Response> {
    let (lib, _) = library_param(&cx, field_value(&pairs, "lib")).await?;
    let from = form::opt_id(field_value(&pairs, "from"));
    let (f, focus_error) = parse(&pairs)?;
    let save = f.clone();
    let result = match focus_error {
        Some(e) => Err(anyhow::anyhow!(e)),
        None => cx.db(move |db| db.create_drill(lib, save)).await,
    };
    match result {
        Ok(id) => Ok(flash::info(
            &cx.st,
            &match from {
                Some(p) => cx.url(&format!("/practices/{p}")),
                None => cx.url(&format!("/drills/{id}")),
            },
            "Drill saved.",
        )),
        Err(e) => {
            let (back, _) = back_to(&cx, from, lib);
            let r = Rejected {
                title: "New drill",
                action: &cx.url("/drills"),
                back: &back,
                used: 0,
                new_in: Some(lib),
                from,
            };
            Ok(rejected(&cx, r, &f, &format!("{e:#}")))
        }
    }
}

#[derive(Deserialize, Default)]
struct FromQuery {
    #[serde(default)]
    from: String,
}

async fn edit_page(dc: DrillCtx, Query(q): Query<FromQuery>) -> AppResult<Page> {
    let from = form::opt_id(&q.from);
    let d = &dc.drill;
    let id = d.id;
    let used = dc.db(move |db| db.drill_usage(id)).await?;
    let libs = libraries(&dc).await?;
    let here = libs
        .iter()
        .find(|(l, _)| *l == d.library)
        .map(|(_, n)| n.clone())
        .unwrap_or_default();
    let elsewhere: Vec<&(Library, String)> = libs.iter().filter(|(l, _)| *l != d.library).collect();
    let base = format!("/drills/{id}");
    let (back, back_label) = back_to(&dc, from, d.library);
    let mut sub = format!(
        "In {here} · {} · {} · last changed {}",
        d.ice.label(),
        d.category.label(),
        d.updated_at
    );
    if d.copied_from.is_some() {
        sub.push_str(" · a copy");
    }
    let body = html! {
        (heading(&d.name, &sub, html! {
            a class="btn" href=(back) { (back_label) }
            (action_button(&dc.url(&format!("{base}/duplicate")), "Duplicate", "", None))
            @if used == 0 {
                (action_button(&dc.url(&format!("{base}/delete")), "Delete", "danger",
                    Some("Delete this drill? This can't be undone.")))
            }
        }))
        @if !elsewhere.is_empty() {
            form method="post" action=(dc.url(&format!("{base}/copy"))) class="inline-form copy-to" {
                label for="copy-to" { "Copy to" }
                select id="copy-to" name="to" {
                    @for (l, name) in &elsewhere {
                        option value=(l.key()) { (name) }
                    }
                }
                button type="submit" class="small" { "Copy" }
                span class="muted small" { "The copy is separate: changing one never changes the other." }
            }
        }
        @if d.focus.is_none() {
            (notice("warn", html! {
                "This drill doesn't say yet how its time splits across the development levels, "
                "so practices can't count it. Set it under "
                strong { "Development" } " and save."
            }))
        }
        (drill_form(&dc.url(&base), &d.fields(), used, &back, None, &dc.url(GUIDE_PATH), from))
    };
    Ok(page(&dc, Nav::Drills, &d.name, body))
}

async fn update(dc: DrillCtx, Form(pairs): Form<Vec<(String, String)>>) -> AppResult<Response> {
    let id = dc.drill.id;
    let from = form::opt_id(field_value(&pairs, "from"));
    let (f, focus_error) = parse(&pairs)?;
    let save = f.clone();
    let result = dc
        .db(move |db| {
            let used = db.drill_usage(id)?;
            let saved = match focus_error {
                Some(e) => Err(anyhow::anyhow!(e)),
                None => db.update_drill(id, save),
            };
            Ok((saved, used))
        })
        .await?;
    match result {
        (Ok(()), _) => Ok(flash::info(
            &dc.st,
            &match from {
                Some(p) => dc.url(&format!("/practices/{p}")),
                None => dc.url(&format!("/drills/{id}")),
            },
            "Drill saved.",
        )),
        (Err(e), used) => {
            let (back, _) = back_to(&dc, from, dc.drill.library);
            let r = Rejected {
                title: "Edit drill",
                action: &dc.url(&format!("/drills/{id}")),
                back: &back,
                used,
                new_in: None,
                from,
            };
            Ok(rejected(&dc, r, &f, &format!("{e:#}")))
        }
    }
}

async fn duplicate(dc: DrillCtx) -> AppResult<Response> {
    let (id, lib) = (dc.drill.id, dc.drill.library);
    match dc.db(move |db| db.copy_drill(id, lib)).await {
        Ok(new) => Ok(flash::info(
            &dc.st,
            &dc.url(&format!("/drills/{new}")),
            "Copied. You're editing the copy.",
        )),
        Err(e) => Ok(flash::error(
            &dc.st,
            &dc.url(&format!("/drills/{id}")),
            format!("{e:#}"),
        )),
    }
}

#[derive(Deserialize)]
struct CopyForm {
    to: String,
}

/// Copy a drill into another library the user may use.
async fn copy(dc: DrillCtx, Form(f): Form<CopyForm>) -> AppResult<Response> {
    let id = dc.drill.id;
    let (to, name) = library_param(&dc, &f.to).await?;
    match dc.db(move |db| db.copy_drill(id, to)).await {
        Ok(new) => Ok(flash::info(
            &dc.st,
            &dc.url(&format!("/drills/{new}")),
            format!("Copied to {name}. You're looking at the copy."),
        )),
        Err(e) => Ok(flash::error(
            &dc.st,
            &dc.url(&format!("/drills/{id}")),
            format!("{e:#}"),
        )),
    }
}

async fn delete(dc: DrillCtx) -> AppResult<Response> {
    let (id, lib) = (dc.drill.id, dc.drill.library);
    match dc.db(move |db| db.delete_drill(id)).await {
        Ok(()) => Ok(flash::info(
            &dc.st,
            &library_url(&dc, lib),
            "Drill deleted.",
        )),
        Err(e) => Ok(flash::error(
            &dc.st,
            &dc.url(&format!("/drills/{id}")),
            format!("{e:#}"),
        )),
    }
}

/// Used by the practice builder: a short note when no drills are available.
pub fn empty_library_note(cx: &Ctx) -> Markup {
    notice(
        "info",
        html! {
            "You don't have any drills yet. "
            a href=(cx.url("/drills")) { "Add drills" } " first, then place them on the ice here."
        },
    )
}
