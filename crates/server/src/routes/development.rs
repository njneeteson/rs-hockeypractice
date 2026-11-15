//! The development guide: Hockey Canada's skill development model in brief
//! (the levels, the targets by age division, and practice-design pointers),
//! in our own words with links to Hockey Canada's documents.

use crate::error::AppResult;
use crate::extract::Ctx;
use crate::state::Shared;
use crate::views::components::{card, heading};
use crate::views::{Nav, Page, page};
use axum::Router;
use axum::routing::get;
use localapp_core::development::{AgeDivision, Level, TOLERANCE};
use maud::html;

pub fn routes() -> Router<Shared> {
    Router::new().route("/development", get(show))
}

/// Hockey Canada pages the guide points to: `(title, url)`.
const SOURCES: &[(&str, &str)] = &[
    (
        "Hockey Canada: Under-9 age division",
        "https://www.hockeycanada.ca/en-ca/hockey-programs/players/essentials/age-divisions/under-9",
    ),
    (
        "Hockey Canada: Under-11 age division",
        "https://www.hockeycanada.ca/en-ca/hockey-programs/players/essentials/age-divisions/under-11",
    ),
    (
        "Hockey Canada: Under-13 age division",
        "https://www.hockeycanada.ca/en-ca/hockey-programs/players/essentials/age-divisions/under-13",
    ),
    (
        "Hockey Canada: Under-15 age division",
        "https://www.hockeycanada.ca/en-ca/hockey-programs/players/essentials/age-divisions/under-15",
    ),
    (
        "Hockey Canada Skills Development Program: definitions of the levels",
        "https://cdn.hockeycanada.ca/hockey-canada/Hockey-Programs/Players/Downloads/2020/player-development-definitions-e.pdf",
    ),
    (
        "Hockey Canada: Long-Term Player Development",
        "https://www.hockeycanada.ca/en-ca/hockey-programs/coaching/ltpd/ltpd-players",
    ),
];

async fn show(cx: Ctx) -> AppResult<Page> {
    let me = cx.member();
    let teams = cx.db(move |db| db.teams_for(me)).await?;
    let mine: Vec<AgeDivision> = teams.iter().filter_map(|(t, _)| t.age).collect();
    let body = html! {
        (heading("Development guide",
            "Hockey Canada's skill development model: what players should spend practice time on at each age.",
            html! {}))
        div class="grid" {
            (card("The pyramid", "", html! {
                p {
                    "Hockey Canada builds a player's development like a pyramid. The base is "
                    "technical skill; each level above it uses the ones below. Young players spend "
                    "nearly all their time at the base, and climb higher as they get older, so a "
                    "drill aimed above the team's level takes time away from what they need most."
                }
                div class="pyramid" role="img" aria-label="The five levels, from technical skills at the base to strategy at the top" {
                    @for l in Level::ALL.iter().rev() {
                        a class={ "tier lvl-" (l.key()) } href={ "#" (l.key()) } { (l.label()) }
                    }
                }
            }))
            (card("How the app counts", "", html! {
                p {
                    "Each drill says how its time splits across the five levels (in 5% steps). "
                    "A practice adds up what one player does: a part's minutes are shared equally "
                    "by its drills, since players either go through every station in turn or each "
                    "spend the part at one of them. A part without drills doesn't count."
                }
                p {
                    "The builder compares each practice with the team's targets, and a team's page "
                    "adds up all its practices so far. A level within " (TOLERANCE as u32)
                    " percentage points of its target counts as on target. One practice can lean "
                    "one way; aim for the season to balance out."
                }
                p {
                    "A team's owner picks its age division under " a href=(cx.url("/teams")) { "Teams" }
                    ", and can enter the team's own targets if the association uses different ones."
                }
            }))
        }
        (card("The levels", "", html! {
            dl class="levels" {
                @for l in Level::ALL {
                    dt id=(l.key()) { span class={ "swatch lvl-" (l.key()) } {} (l.label()) }
                    dd { (l.about()) }
                }
            }
        }))
        (card("Targets by age division", "Share of practice time at each level. Your teams' divisions are highlighted.", html! {
            div class="table-wrap" {
                table class="targets" {
                    thead {
                        tr {
                            th scope="col" { "Division" }
                            @for l in Level::ALL {
                                th scope="col" class="num" { span class={ "swatch lvl-" (l.key()) } {} (l.label()) }
                            }
                            th scope="col" { "Climbs to" }
                        }
                    }
                    tbody {
                        @for a in AgeDivision::with_targets() {
                            @let s = a.hockey_canada().unwrap_or(localapp_core::development::Split::TECHNICAL);
                            tr id=(a.key()) class=[mine.contains(&a).then_some("mine")] {
                                th scope="row" { (a.label()) " " span class="muted small" { "ages " (a.ages()) } }
                                @for l in Level::ALL {
                                    td class="num" { @if s.get(*l) > 0 { (s.get(*l)) "%" } @else { span class="muted" { "–" } } }
                                }
                                td { (s.top().label()) }
                            }
                        }
                    }
                }
            }
            p class="muted small" {
                "From Hockey Canada's age-division pages and Skills Development Program. Its "
                "documents differ slightly between editions; check the latest from Hockey Canada or "
                "your association, and set your own targets on the team page if they differ."
            }
        }))
        (card("Designing a practice", "", html! {
            ul class="tips" {
                li { strong { "Start from the targets. " } "At U7 and U9 almost everything is skating, puck control, passing and shooting, with some 1-on-1 play. Team play and systems come later." }
                li { strong { "One or two themes. " } "Pick what the practice is for, and choose drills that build on each other, from the skill on its own to using it under pressure." }
                li { strong { "Keep everyone moving. " } "Stations and small-area games on part of the ice give each player far more puck touches than one full-ice drill with long lines." }
                li { strong { "Few words, more reps. " } "Two or three key points per drill, a short demonstration, then let them play." }
                li { strong { "Games are part of it. " } "Small-area games teach individual and team tactics at once; set their split to match." }
                li { strong { "Look at the season. " } "Check the team page now and then: if one level keeps coming up short, plan the next practices around it." }
            }
        }))
        (card("Sources", "", html! {
            ul {
                @for (title, url) in SOURCES {
                    li { a href=(url) target="_blank" rel="noopener" { (title) } }
                }
            }
        }))
    };
    Ok(page(&cx, Nav::Guidelines, "Development guide", body))
}
