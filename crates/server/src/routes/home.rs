//! The landing page after sign-in: upcoming practices and shortcuts.

use crate::error::AppResult;
use crate::extract::Ctx;
use crate::routes::practices::practice_table;
use crate::views::components::{card, empty, heading, link_button, stat_tile};
use crate::views::{Nav, Page, page};
use chrono::Local;
use localapp_core::db::DrillFilter;
use maud::html;

pub async fn show(cx: Ctx) -> AppResult<Page> {
    let (practices, drills, teams) = cx
        .db(|db| {
            Ok((
                db.practices()?,
                db.drills(&DrillFilter::default())?.len(),
                db.teams()?.len(),
            ))
        })
        .await?;
    let today = Local::now().date_naive();
    let mut upcoming: Vec<_> = practices
        .iter()
        .filter(|s| s.practice.date >= today)
        .cloned()
        .collect();
    upcoming.reverse();
    upcoming.truncate(8);
    let body = html! {
        (heading(&format!("Welcome, {}", cx.user.name()), "Plan practices from your drill library and print them as PDFs.",
            html! {
                (link_button(&cx.url("/practices/new"), "New practice", true))
                (link_button(&cx.url("/drills/new"), "New drill", false))
            }))
        div class="tiles" {
            (stat_tile("Upcoming practices", html! { (upcoming.len()) }, "", ""))
            (stat_tile("Drills in the library", html! { (drills) }, "", ""))
            (stat_tile("Teams", html! { (teams) }, "", ""))
        }
        (card("Coming up", "", html! {
            @if upcoming.is_empty() {
                (empty("No practices planned."))
            } @else {
                (practice_table(&cx, &upcoming))
            }
        }))
        @if drills == 0 || teams == 0 {
            (card("Getting started", "", html! {
                ol {
                    li { a href=(cx.url("/teams")) { "Add your team" } ": its name goes in the footer of each page." }
                    li { a href=(cx.url("/drills")) { "Build your drill library" } ": draw each drill once, with its ice size, equipment and teaching points." }
                    li { a href=(cx.url("/practices/new")) { "Plan a practice" } ": add parts, drop drills onto the rink, and download the PDF." }
                }
            }))
        }
    };
    Ok(page(&cx, Nav::Home, "Home", body))
}
