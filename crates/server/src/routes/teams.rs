//! Teams: the name printed in the footer of each practice plan.

use crate::error::AppResult;
use crate::extract::Ctx;
use crate::flash;
use crate::state::Shared;
use crate::views::components::{Num, Text, card, empty, heading, thead};
use crate::views::forms::{action_button, field};
use crate::views::{Nav, Page, page};
use axum::extract::Path;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::html;
use serde::Deserialize;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/teams", get(list).post(create))
        .route("/teams/{id}", post(update))
        .route("/teams/{id}/delete", post(delete))
}

async fn list(cx: Ctx) -> AppResult<Page> {
    let teams = cx.db(|db| db.teams()).await?;
    let body = html! {
        (heading("Teams", "Each practice belongs to a team; its name goes in the footer of every page.", html! {}))
        (card("Teams", "", html! {
            @if teams.is_empty() {
                (empty("No teams yet. Add one below."))
            } @else {
                div class="table-wrap" {
                    table {
                        (thead(&[("Name", Text), ("Notes", Text), ("Practices", Num), ("", Text)]))
                        tbody {
                            @for t in &teams {
                                @let form_id = format!("team-{}", t.id);
                                tr {
                                    td {
                                        input type="text" name="name" value=(t.name) form=(form_id)
                                            aria-label="Team name" required;
                                    }
                                    td {
                                        input type="text" name="notes" value=(t.notes) form=(form_id)
                                            aria-label="Notes";
                                    }
                                    td class="num" {
                                        a href=(cx.url(&format!("/practices?team={}", t.id))) { (t.practices) }
                                    }
                                    td class="nowrap" {
                                        form id=(form_id) method="post" class="inline"
                                            action=(cx.url(&format!("/teams/{}", t.id))) {
                                            button type="submit" class="small" { "Save" }
                                        }
                                        " "
                                        @if t.practices == 0 {
                                            (action_button(&cx.url(&format!("/teams/{}/delete", t.id)),
                                                "Delete", "small danger", Some("Delete this team?")))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }))
        (card("Add a team", "", html! {
            form method="post" action=(cx.url("/teams")) class="form-grid" {
                (field("Name", "For example “U13 A Hawks”.", html! {
                    input type="text" name="name" required maxlength="80";
                }))
                (field("Notes", "Optional: level, ice times, coaches.", html! {
                    input type="text" name="notes";
                }))
                div { button type="submit" class="primary" { "Add team" } }
            }
        }))
    };
    Ok(page(&cx, Nav::Teams, "Teams", body))
}

#[derive(Deserialize)]
struct TeamForm {
    name: String,
    #[serde(default)]
    notes: String,
}

async fn create(cx: Ctx, Form(f): Form<TeamForm>) -> AppResult<Response> {
    let result = cx
        .db(move |db| {
            db.create_team(&f.name, &f.notes)
                .map(|_| format!("Added {}.", f.name.trim()))
        })
        .await;
    Ok(flash::report(&cx.st, &cx.url("/teams"), result))
}

async fn update(cx: Ctx, Path(id): Path<i64>, Form(f): Form<TeamForm>) -> AppResult<Response> {
    let result = cx
        .db(move |db| {
            db.update_team(id, &f.name, &f.notes)
                .map(|()| "Team saved.")
        })
        .await;
    Ok(flash::report(&cx.st, &cx.url("/teams"), result))
}

async fn delete(cx: Ctx, Path(id): Path<i64>) -> AppResult<Response> {
    let result = cx
        .db(move |db| db.delete_team(id).map(|()| "Team deleted."))
        .await;
    Ok(flash::report(&cx.st, &cx.url("/teams"), result))
}
