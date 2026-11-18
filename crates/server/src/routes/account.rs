//! "My account": name, theme, password, other sessions.

use crate::auth::session::append_cookie;
use crate::auth::store::{BuilderExtra, Theme};
use crate::auth::{password, start_session};
use crate::error::AppResult;
use crate::extract::Ctx;
use crate::flash;
use crate::form::key;
use crate::state::Shared;
use crate::views::components::{card, heading, notice};
use crate::views::forms::{enum_select, field};
use crate::views::{Nav, Page, page};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::{Markup, html};
use serde::Deserialize;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/account", get(show).post(save_preferences))
        .route(
            "/account/password",
            get(password_page).post(change_password),
        )
        .route("/account/sessions/revoke", post(revoke_other_sessions))
        .route("/account/builder", post(save_builder))
}

fn password_form(cx: &Ctx) -> Markup {
    let hint = format!("At least {} characters.", password::MIN_LENGTH);
    html! {
        form method="post" action=(cx.url("/account/password")) class="form-stack" {
            (field("Current password", "", html! {
                input type="password" name="current" autocomplete="current-password" required;
            }))
            (field("New password", &hint, html! {
                input type="password" name="password" autocomplete="new-password" required;
            }))
            (field("Confirm new password", "", html! {
                input type="password" name="confirm" autocomplete="new-password" required;
            }))
            div { button type="submit" class="primary" { "Change password" } }
        }
    }
}

fn preferences_form(cx: &Ctx) -> Markup {
    html! {
        form method="post" action=(cx.url("/account")) class="form-grid" {
            (field("Name", "Shown in the menu instead of your username.", html! {
                input type="text" name="display_name" value=(cx.user.display_name);
            }))
            (field("Theme", "", enum_select("theme", cx.user.theme, false)))
            div { button type="submit" class="primary" { "Save" } }
        }
    }
}

async fn show(cx: Ctx) -> AppResult<Page> {
    let others = cx.st.auth.session_count(cx.user.id)? - 1;
    let signed_in_as = format!(
        "Signed in as {} ({}).",
        cx.user.username,
        cx.user.role.label()
    );
    let body = html! {
        (heading("My account", &signed_in_as, html! {}))
        div class="grid" {
            (card("Preferences", "", preferences_form(&cx)))
            (card("Practice builder", "Extras that take room on the screen. Turn off any you don't use.", builder_form(&cx)))
            (card("Password", "Changing your password signs you out on every other device.",
                password_form(&cx)))
            (card("Other devices", "", html! {
                p { "You're signed in on " (others.max(0)) " other device(s) or browser(s)." }
                form method="post" action=(cx.url("/account/sessions/revoke")) {
                    button type="submit" disabled[others <= 0] { "Sign out everywhere else" }
                }
            }))
        }
    };
    Ok(page(&cx, Nav::Account, "My account", body))
}

async fn password_page(cx: Ctx) -> Page {
    let body = html! {
        (heading("Choose a new password", "", html! {}))
        @if cx.user.must_change_password {
            (notice("warn", html! {
                "You signed in with a temporary password. Choose your own password to continue."
            }))
        }
        (card("", "", password_form(&cx)))
    };
    page(&cx, Nav::Account, "Change password", body)
}

#[derive(Deserialize)]
pub struct PreferencesForm {
    display_name: String,
    theme: String,
}

async fn save_preferences(cx: Ctx, Form(f): Form<PreferencesForm>) -> AppResult<Response> {
    let theme: Theme = key(&f.theme, "theme")?;
    cx.st
        .auth
        .set_preferences(cx.user.id, &f.display_name, theme)?;
    Ok(flash::info(
        &cx.st,
        &cx.url("/account"),
        "Preferences saved.",
    ))
}

#[derive(Deserialize)]
pub struct PasswordForm {
    current: String,
    password: String,
    confirm: String,
}

async fn change_password(cx: Ctx, Form(f): Form<PasswordForm>) -> AppResult<Response> {
    let back = cx.url("/account/password");
    let stored = cx.st.auth.password_hash(cx.user.id)?.unwrap_or_default();
    let current = f.current.clone();
    let ok = tokio::task::spawn_blocking(move || password::verify(&current, &stored))
        .await
        .map_err(anyhow::Error::from)?;
    if !ok {
        return Ok(flash::error(
            &cx.st,
            &back,
            "Your current password isn't right.",
        ));
    }
    if let Err(msg) = password::validate_new(&f.password, &f.confirm) {
        return Ok(flash::error(&cx.st, &back, msg));
    }
    if f.password == f.current {
        return Ok(flash::error(
            &cx.st,
            &back,
            "Choose a password different from the current one.",
        ));
    }
    // Signs out every session, then starts a fresh one for this browser.
    cx.st
        .auth
        .set_password(cx.user.id, &password::hash(&f.password)?, false)?;
    let cookie = start_session(&cx.st, cx.user.id, false)?;
    let done = "Password changed. Other devices have been signed out.";
    let mut resp = flash::info(&cx.st, &cx.url("/account"), done);
    append_cookie(&mut resp, &cookie);
    Ok(resp)
}

async fn revoke_other_sessions(cx: Ctx) -> AppResult<Response> {
    cx.st
        .auth
        .delete_sessions_for(cx.user.id, Some(&cx.session))?;
    Ok(flash::info(
        &cx.st,
        &cx.url("/account"),
        "Signed out of all other devices.",
    ))
}

fn builder_form(cx: &Ctx) -> Markup {
    html! {
        form method="post" action=(cx.url("/account/builder")) class="form-stack" {
            fieldset class="plain checks" aria-label="Show in the practice builder" {
                @for e in BuilderExtra::ALL {
                    label class="check" {
                        input type="checkbox" name={ "show_" (e.key()) } value="1" checked[cx.user.shows(*e)];
                        " " (e.label())
                    }
                }
            }
            div { button type="submit" class="primary" { "Save" } }
        }
    }
}

/// Ticked boxes arrive as `show_<key>=1`; the rest are turned off.
async fn save_builder(
    cx: Ctx,
    Form(f): Form<std::collections::HashMap<String, String>>,
) -> AppResult<Response> {
    let hidden: Vec<BuilderExtra> = BuilderExtra::ALL
        .iter()
        .copied()
        .filter(|e| !f.contains_key(&format!("show_{}", e.key())))
        .collect();
    cx.st.auth.set_hidden_extras(cx.user.id, &hidden)?;
    Ok(flash::info(
        &cx.st,
        &cx.url("/account"),
        "Builder settings saved.",
    ))
}
