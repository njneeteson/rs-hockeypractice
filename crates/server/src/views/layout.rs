//! The page shell: document head, top bar, side menu and flash message.

use super::components::notice;
use super::nav::{Nav, side_menu};
use crate::auth::store::Theme;
use crate::extract::Ctx;
use crate::flash::Kind;
use crate::state::Shared;
use axum::response::Html;
use maud::{DOCTYPE, Markup, html};

pub type Page = Html<String>;

/// The app's name in titles and the top bar.
pub const APP_NAME: &str = "Practice Planner";

fn asset(st: &Shared, name: &str) -> String {
    st.url(&format!("/static/{name}?v={}", crate::assets::version()))
}

/// htmx settings: no `eval` (the CSP forbids it anyway), no inline scripts from
/// responses, no page snapshots in browser storage (page data stays out of
/// localStorage), and error pages are shown rather than silently ignored.
const HTMX_CONFIG: &str = r#"{"allowEval":false,"allowScriptTags":false,
"includeIndicatorStyles":false,"historyCacheSize":0,"refreshOnHistoryMiss":true,
"responseHandling":[{"code":"204","swap":false},{"code":"...","swap":true}]}"#;

fn document(st: &Shared, theme: Theme, title: &str, content: Markup) -> Page {
    // On <body>, not <html>: htmx swaps the body when navigating, so a theme
    // change (or signing in as someone else) takes effect immediately.
    let data_theme = match theme {
        Theme::System => None,
        t => Some(t.key()),
    };
    let doc = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light dark";
                meta name="htmx-config" content=(HTMX_CONFIG);
                title { (title) " · " (APP_NAME) }
                link rel="icon" href=(asset(st, "favicon.svg")) type="image/svg+xml";
                link rel="stylesheet" href=(asset(st, "app.css"));
                @for script in crate::assets::SCRIPTS {
                    script src=(asset(st, script)) defer {}
                }
            }
            // Links and forms load in place (htmx); the bar shows while loading.
            body data-theme=[data_theme] hx-boost="true" hx-indicator="#loading" {
                div id="loading" class="loading-bar" aria-hidden="true" {}
                (content)
            }
        }
    };
    Html(doc.into_string())
}

fn brand(st: &Shared) -> Markup {
    html! {
        a class="brand" href=(st.url("/")) {
            img src=(asset(st, "favicon.svg")) alt="";
            (APP_NAME)
        }
    }
}

fn user_menu(cx: &Ctx) -> Markup {
    html! {
        details class="menu" {
            summary { (cx.user.name()) }
            div class="menu-body" {
                div class="menu-note" {
                    "Signed in as " (cx.user.username) " · " (cx.user.role.label())
                }
                a href=(cx.url("/account")) { "My account" }
                @if cx.user.is_admin() {
                    a href=(cx.url("/admin/users")) { "Manage users" }
                }
                form method="post" action=(cx.url("/logout")) {
                    button type="submit" { "Sign out" }
                }
            }
        }
    }
}

fn top_bar(cx: &Ctx) -> Markup {
    html! {
        header class="topbar" {
            (brand(&cx.st))
            div class="spacer" {}
            (user_menu(cx))
        }
    }
}

/// The pending message as a toast (fades after a few seconds unless it's an error).
fn flash(cx: &Ctx) -> Markup {
    html! {
        @if let Some(f) = cx.take_flash() {
            div class="toasts" aria-live="polite" {
                @let kind = if f.kind == Kind::Error { "error" } else { "info" };
                (notice(kind, html! { (f.message) }))
            }
        }
    }
}

/// A signed-in page: top bar, side menu (with `active` highlighted) and `body`.
pub fn page(cx: &Ctx, active: Nav, title: &str, body: Markup) -> Page {
    let content = html! {
        (top_bar(cx))
        div class="shell" {
            (side_menu(cx, active))
            main id="main" { (flash(cx)) (body) }
        }
    };
    document(&cx.st, cx.user.theme, title, content)
}

/// A page for signed-out visitors (sign-in, first-run setup).
pub fn bare_page(st: &Shared, title: &str, error: Option<&str>, body: Markup) -> Page {
    let content = html! {
        main class="narrow" {
            div class="auth-card" {
                (brand(st))
                @if let Some(e) = error { (notice("error", html! { (e) })) }
                (body)
            }
        }
    };
    document(st, Theme::System, title, content)
}
