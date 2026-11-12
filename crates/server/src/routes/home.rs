//! The landing page after sign-in. Replace it with your app's main page.

use crate::extract::Ctx;
use crate::views::components::{card, heading};
use crate::views::{Nav, Page, page};
use maud::html;

pub async fn show(cx: Ctx) -> Page {
    let body = html! {
        (heading(&format!("Welcome, {}", cx.user.name()), "", html! {}))
        (card("Your app goes here", "", html! {
            p {
                "This page lives in " code { "crates/server/src/routes/home.rs" } ". "
                "Put the app's logic and storage in " code { "crates/core" } " and its pages in "
                code { "crates/server/src/routes" } "."
            }
        }))
    };
    page(&cx, Nav::Home, "Home", body)
}
