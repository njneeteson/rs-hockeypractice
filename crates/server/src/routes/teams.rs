//! Teams: who plans together. Each practice belongs to a team (its name goes
//! in the footer of every page), and each team has its own drill library.
//! Owners manage the team and its members; every member plans practices and
//! edits the team's drills.

use crate::error::{AppError, AppResult};
use crate::extract::{Ctx, TeamCtx, TeamOwnerCtx};
use crate::flash;
use crate::form;
use crate::state::Shared;
use crate::views::components::{Num, Text, card, empty, heading, link_button, thead};
use crate::views::development::{
    GUIDE_PATH, SplitInputs, composition_panel, parse_targets, split_values, targets_source,
};
use crate::views::forms::{action_button, field};
use crate::views::{Nav, Page, page};
use axum::extract::Path;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use localapp_core::development::{AgeDivision, Composition};
use localapp_core::owner::{Library, MemberId, TeamRole};
use maud::{Markup, html};
use serde::Deserialize;
use std::collections::HashMap;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/teams", get(list).post(create))
        .route("/teams/{id}", get(show).post(update))
        .route("/teams/{id}/delete", post(delete))
        .route("/teams/{id}/members", post(add_member))
        .route("/teams/{id}/members/{member}/role", post(set_role))
        .route("/teams/{id}/members/{member}/remove", post(remove_member))
        .route("/teams/{id}/leave", post(leave))
        .route("/teams/{id}/development", post(set_development))
}

fn team_url(cx: &Ctx, id: i64) -> String {
    cx.url(&format!("/teams/{id}"))
}

async fn list(cx: Ctx) -> AppResult<Page> {
    let me = cx.member();
    let teams = cx.db(move |db| db.teams_for(me)).await?;
    let body = html! {
        (heading("Teams", "Each practice belongs to a team. Its members plan practices together and share the team's drills.", html! {}))
        (card("Your teams", "", html! {
            @if teams.is_empty() {
                (empty("You're not in a team yet. Add one below, or ask a team's owner to add you."))
            } @else {
                div class="table-wrap" {
                    table {
                        (thead(&[("Team", Text), ("Age", Text), ("You are", Text), ("Members", Num), ("Practices", Num), ("Drills", Num)]))
                        tbody {
                            @for (t, role) in &teams {
                                tr {
                                    td { a href=(team_url(&cx, t.id)) { (t.name) } }
                                    td { (t.age.map(|a| a.label()).unwrap_or("–")) }
                                    td { (role.label()) }
                                    td class="num" { (t.members) }
                                    td class="num" {
                                        a href=(cx.url(&format!("/practices?team={}", t.id))) { (t.practices) }
                                    }
                                    td class="num" {
                                        a href=(cx.url(&format!("/drills?lib={}", Library::Team(t.id).key()))) { (t.drills) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }))
        (card("Add a team", "You'll be its owner, and can add other coaches to it.", html! {
            form method="post" action=(cx.url("/teams")) class="form-grid" {
                (field("Name", "For example “U13 A Hawks”.", html! {
                    input type="text" name="name" required maxlength="80";
                }))
                (field("Age division", "Sets Hockey Canada's targets for how practice time splits across the development levels.", age_select(None)))
                (field("Notes", "Optional: level, ice times, coaches.", html! {
                    input type="text" name="notes" maxlength="500";
                }))
                div { button type="submit" class="primary" { "Add team" } }
            }
        }))
    };
    Ok(page(&cx, Nav::Teams, "Teams", body))
}

/// Account names by id, for showing members.
fn names(cx: &Ctx) -> AppResult<HashMap<i64, (String, String)>> {
    Ok(cx
        .st
        .auth
        .users()?
        .into_iter()
        .map(|u| (u.id, (u.name().to_string(), u.username.clone())))
        .collect())
}

fn role_select(role: TeamRole) -> Markup {
    html! {
        select name="role" aria-label="Role" data-autosubmit {
            @for r in TeamRole::ALL {
                option value=(r.key()) selected[*r == role] { (r.label()) }
            }
        }
    }
}

async fn show(tc: TeamCtx) -> AppResult<Page> {
    let id = tc.team.id;
    let members = tc.db(move |db| db.members(id)).await?;
    let today = chrono::Local::now().date_naive();
    let (so_far, all) = tc
        .db(move |db| {
            Ok((
                db.team_composition(id, Some(today))?,
                db.team_composition(id, None)?,
            ))
        })
        .await?;
    let names = names(&tc)?;
    let owner = tc.role == TeamRole::Owner;
    let me = tc.member();
    let t = &tc.team;
    let base = format!("/teams/{id}");
    let sub = format!(
        "{} member(s) · {} practice(s) · {} drill(s). You're {}.",
        t.members,
        t.practices,
        t.drills,
        if owner { "an owner" } else { "a coach" }
    );
    let body = html! {
        (heading(&t.name, &sub, html! {
            (link_button(&tc.url(&format!("/practices?team={id}")), "Practices", false))
            (link_button(&tc.url(&format!("/drills?lib={}", Library::Team(id).key())), "Team drills", false))
            (link_button(&tc.url("/teams"), "← Teams", false))
        }))
        (development_card(&tc, so_far, all))
        div class="grid" {
            (card("Members", "Owners manage the team and who's in it. Everyone plans practices and edits the team's drills.", html! {
                div class="table-wrap" {
                    table {
                        (thead(&[("Name", Text), ("Username", Text), ("Role", Text), ("", Text)]))
                        tbody {
                            @for m in &members {
                                @let (name, username) = names.get(&m.id.0).cloned()
                                    .unwrap_or_else(|| ("(deleted account)".into(), String::new()));
                                @let murl = format!("{base}/members/{}", m.id.0);
                                tr {
                                    td { (name) @if m.id == me { " " span class="muted small" { "(you)" } } }
                                    td class="muted" { (username) }
                                    td {
                                        @if owner {
                                            form method="post" action=(tc.url(&format!("{murl}/role"))) class="inline-form" {
                                                (role_select(m.role))
                                                button type="submit" class="small no-js-only" { "Set" }
                                            }
                                        } @else {
                                            (m.role.label())
                                        }
                                    }
                                    td class="nowrap" {
                                        @if owner && m.id != me {
                                            (action_button(&tc.url(&format!("{murl}/remove")), "Remove", "small danger",
                                                Some("Take them out of this team? The team's practices and drills stay.")))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                @if owner {
                    form method="post" action=(tc.url(&format!("{base}/members"))) class="form-grid" {
                        (field("Add someone", "Their username in this app. An admin creates accounts.", html! {
                            input type="text" name="username" required maxlength="64" autocomplete="off";
                        }))
                        (field("As", "", role_select_plain()))
                        div { button type="submit" class="primary" { "Add to team" } }
                    }
                }
            }))
            div class="stack" {
                @if owner {
                    (card("Details", "", html! {
                        form method="post" action=(tc.url(&base)) class="form-stack" {
                            (field("Name", "Printed in the footer of every page.", html! {
                                input type="text" name="name" value=(t.name) required maxlength="80";
                            }))
                            (field("Notes", "", html! {
                                input type="text" name="notes" value=(t.notes) maxlength="500";
                            }))
                            div { button type="submit" { "Save" } }
                        }
                    }))
                    (card("Delete the team", "", html! {
                        @if t.practices > 0 {
                            p class="muted" { "Delete its " (t.practices) " practice(s) first." }
                        } @else {
                            p { "Deletes the team and its " (t.drills) " drill(s). Copy any you want to keep to your own library first." }
                            (action_button(&tc.url(&format!("{base}/delete")), "Delete team", "danger",
                                Some("Delete this team and its drills? This can't be undone.")))
                        }
                    }))
                } @else if !t.notes.is_empty() {
                    (card("Notes", "", html! { p { (t.notes) } }))
                }
                (card("Leave the team", "", html! {
                    p { "You'll no longer see its practices or drills. They stay with the team." }
                    (action_button(&tc.url(&format!("{base}/leave")), "Leave team", "danger",
                        Some("Leave this team?")))
                }))
            }
        }
    };
    Ok(page(&tc, Nav::Teams, &tc.team.name, body))
}

/// The role select without autosubmit (for the add form).
fn role_select_plain() -> Markup {
    html! {
        select name="role" {
            option value=(TeamRole::Coach.key()) { (TeamRole::Coach.label()) }
            option value=(TeamRole::Owner.key()) { (TeamRole::Owner.label()) }
        }
    }
}

#[derive(Deserialize)]
struct TeamForm {
    name: String,
    #[serde(default)]
    notes: String,
    /// Only on the "Add a team" form.
    #[serde(default)]
    age: String,
}

async fn create(cx: Ctx, Form(f): Form<TeamForm>) -> AppResult<Response> {
    let me = cx.member();
    let age = match f.age.trim() {
        "" => None,
        k => Some(form::key::<AgeDivision>(k, "age division")?),
    };
    let result = cx
        .db(move |db| {
            let id = db.create_team(me, &f.name, &f.notes)?;
            db.set_team_development(id, age, None)?;
            Ok(id)
        })
        .await;
    match result {
        Ok(id) => Ok(flash::info(
            &cx.st,
            &team_url(&cx, id),
            "Team added. Add the other coaches below.",
        )),
        Err(e) => Ok(flash::error(&cx.st, &cx.url("/teams"), format!("{e:#}"))),
    }
}

async fn update(tc: TeamOwnerCtx, Form(f): Form<TeamForm>) -> AppResult<Response> {
    let (id, me) = (tc.team.id, tc.member());
    let result = tc
        .db(move |db| {
            db.update_team(id, me, &f.name, &f.notes)
                .map(|()| "Team saved.")
        })
        .await;
    Ok(flash::report(&tc.st, &team_url(&tc, id), result))
}

async fn delete(tc: TeamOwnerCtx) -> AppResult<Response> {
    let id = tc.team.id;
    match tc.db(move |db| db.delete_team(id)).await {
        Ok(()) => Ok(flash::info(&tc.st, &tc.url("/teams"), "Team deleted.")),
        Err(e) => Ok(flash::error(&tc.st, &team_url(&tc, id), format!("{e:#}"))),
    }
}

#[derive(Deserialize)]
struct AddForm {
    username: String,
    #[serde(default)]
    role: String,
}

async fn add_member(tc: TeamOwnerCtx, Form(f): Form<AddForm>) -> AppResult<Response> {
    let id = tc.team.id;
    let to = team_url(&tc, id);
    let role: TeamRole = form::key(&f.role, "role")?;
    let found = tc.st.auth.credentials(f.username.trim())?;
    let Some((user, _)) = found.filter(|(u, _)| !u.disabled) else {
        return Ok(flash::error(
            &tc.st,
            &to,
            format!("There's no account called “{}”.", f.username.trim()),
        ));
    };
    let name = user.name().to_string();
    let result = tc
        .db(move |db| {
            db.add_member(id, MemberId(user.id), role)
                .map(|()| format!("Added {name}."))
        })
        .await;
    Ok(flash::report(&tc.st, &to, result))
}

#[derive(Deserialize)]
struct RoleForm {
    role: String,
}

async fn set_role(
    tc: TeamOwnerCtx,
    Path((_, member)): Path<(i64, i64)>,
    Form(f): Form<RoleForm>,
) -> AppResult<Response> {
    let id = tc.team.id;
    let role: TeamRole = form::key(&f.role, "role")?;
    let result = tc
        .db(move |db| {
            db.set_role(id, MemberId(member), role)
                .map(|()| "Role changed.")
        })
        .await;
    Ok(flash::report(&tc.st, &team_url(&tc, id), result))
}

async fn remove_member(
    tc: TeamOwnerCtx,
    Path((_, member)): Path<(i64, i64)>,
) -> AppResult<Response> {
    let id = tc.team.id;
    if MemberId(member) == tc.member() {
        return Err(AppError::bad_request(
            "To leave the team yourself, use “Leave team”.",
        ));
    }
    let result = tc
        .db(move |db| {
            db.remove_member(id, MemberId(member))
                .map(|()| "Taken out of the team.")
        })
        .await;
    Ok(flash::report(&tc.st, &team_url(&tc, id), result))
}

async fn leave(tc: TeamCtx) -> AppResult<Response> {
    let (id, me) = (tc.team.id, tc.member());
    let name = tc.team.name.clone();
    match tc.db(move |db| db.remove_member(id, me)).await {
        Ok(()) => Ok(flash::info(
            &tc.st,
            &tc.url("/teams"),
            format!("You left {name}."),
        )),
        Err(e) => Ok(flash::error(&tc.st, &team_url(&tc, id), format!("{e:#}"))),
    }
}

/// A select of the age divisions, with "not set" first.
fn age_select(selected: Option<AgeDivision>) -> Markup {
    html! {
        select name="age" {
            option value="" selected[selected.is_none()] { "Not set" }
            @for a in AgeDivision::ALL {
                option value=(a.key()) selected[selected == Some(*a)] {
                    (a.label())
                    @if !a.ages().is_empty() { " (ages " (a.ages()) ")" }
                }
            }
        }
    }
}

/// The team's development targets and how its practices measure up; owners
/// also set the age division and, if they want, their own targets.
fn development_card(tc: &TeamCtx, so_far: (Composition, u32), all: (Composition, u32)) -> Markup {
    let t = &tc.team;
    let guide = tc.url(GUIDE_PATH);
    let targets = t.targets();
    let custom = t.custom_targets.is_some();
    let owner = tc.role == TeamRole::Owner;
    let caption = match t.age {
        Some(a) => format!(
            "{} · compared with {}.",
            a.label(),
            targets_source(Some(a), custom)
        ),
        None if custom => "Compared with the team's own targets.".to_string(),
        None => "No age division set, so there are no targets to compare with.".to_string(),
    };
    html! {
        (card("Development", "Per player: each drill counts for its share of a part's time (the part's minutes ÷ its drills), split by its development levels.", html! {
            p class="muted" {
                (caption) " "
                a href={ (guide) @if let Some(a) = t.age { "#" (a.key()) } } { "Hockey Canada's guidance" }
            }
            h3 { "Practices so far (" (so_far.1) ")" }
            @if so_far.1 == 0 {
                p class="muted small" { "None up to today yet." }
            } @else {
                (composition_panel(&so_far.0, targets, &guide))
            }
            @if all.1 > so_far.1 {
                details class="season-all" {
                    summary { "Including planned practices (" (all.1) ")" }
                    (composition_panel(&all.0, targets, &guide))
                }
            }
            @if owner {
                details class="team-targets" open[t.age.is_none()] {
                    summary { "Age division and targets" }
                    form method="post" action=(tc.url(&format!("/teams/{}/development", t.id))) class="form-stack" {
                        (field("Age division", "", age_select(t.age)))
                        fieldset {
                            legend { "Targets" }
                            label class="check" {
                                input type="radio" name="targets" value="hockey_canada" checked[!custom];
                                " Hockey Canada's for the age division"
                                @if let Some(s) = t.age.and_then(AgeDivision::hockey_canada) {
                                    span class="muted small" { " (" (s) ")" }
                                }
                            }
                            label class="check" {
                                input type="radio" name="targets" value="custom" checked[custom];
                                " Our own (e.g. what your association asks for):"
                            }
                            (SplitInputs {
                                prefix: "target_",
                                values: split_values(targets),
                                guide: None,
                                implies: Some("targets=custom"),
                                tips: false,
                            }.render())
                        }
                        div { button type="submit" { "Save" } }
                    }
                }
            }
        }))
    }
}

#[derive(Deserialize)]
struct DevelopmentForm {
    #[serde(default)]
    age: String,
    #[serde(default)]
    targets: String,
    #[serde(flatten)]
    rest: HashMap<String, String>,
}

async fn set_development(tc: TeamOwnerCtx, Form(f): Form<DevelopmentForm>) -> AppResult<Response> {
    let id = tc.team.id;
    let to = team_url(&tc, id);
    let age = match f.age.trim() {
        "" => None,
        k => Some(form::key::<AgeDivision>(k, "age division")?),
    };
    let targets = match f.targets.as_str() {
        "custom" => match parse_targets(|k| f.rest.get(k).cloned().unwrap_or_default()) {
            Ok(s) => Some(s),
            Err(e) => return Ok(flash::error(&tc.st, &to, format!("Targets not saved: {e}"))),
        },
        _ => None,
    };
    let result = tc
        .db(move |db| {
            db.set_team_development(id, age, targets)
                .map(|()| "Development targets saved.")
        })
        .await;
    Ok(flash::report(&tc.st, &to, result))
}
