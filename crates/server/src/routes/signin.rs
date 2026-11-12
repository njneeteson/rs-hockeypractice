//! Sign in, sign out, and first-run creation of the admin account.

use crate::auth::session::{
    SESSION_COOKIE, append_cookie, clear_session_cookie, hash_token, read_cookie,
};
use crate::auth::store::{Role, User, UserFields};
use crate::auth::{password, start_session};
use crate::error::AppResult;
use crate::flash;
use crate::form::checked;
use crate::state::Shared;
use crate::views::bare_page;
use crate::views::components::notice;
use crate::views::forms::{checkbox, field};
use crate::views::layout::APP_NAME;
use axum::extract::{ConnectInfo, FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use maud::{Markup, html};
use serde::Deserialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Instant;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/setup", get(setup_page).post(setup))
}

/// Best-effort client address: the first `X-Forwarded-For` hop (set by the
/// reverse proxy), else the TCP peer. Only used for sign-in throttling.
pub struct ClientIp(pub String);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(|v| v.trim().to_string());
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string());
        Ok(Self(forwarded.or(peer).unwrap_or_default()))
    }
}

/// Only follow `next=` to pages of this app, never to another site.
fn safe_next(st: &Shared, next: Option<&str>) -> String {
    let home = st.url("/");
    match next {
        Some(n) if n.starts_with(&home) && !n.starts_with("//") && !n.contains("://") => {
            n.to_string()
        }
        Some(n) if !st.cfg.base_path.is_empty() && n == st.cfg.base_path => n.to_string(),
        _ => home,
    }
}

/// Start a session for `user` and send them on to `to`.
fn sign_in(st: &Shared, user: &User, persistent: bool, to: &str) -> AppResult<Response> {
    let cookie = start_session(st, user.id, persistent)?;
    st.auth.record_login(user.id)?;
    let mut resp = flash::redirect(to);
    append_cookie(&mut resp, &cookie);
    Ok(resp)
}

#[derive(Deserialize, Default)]
pub struct NextQuery {
    next: Option<String>,
}

/// Secure cookies are on but the browser reached us over plain HTTP: the
/// session cookie would be dropped and sign-in would silently fail.
fn cookies_will_be_dropped(st: &Shared, headers: &HeaderMap) -> bool {
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok());
    st.cfg.secure_cookies && proto != Some("https")
}

const INSECURE_NOTE: &str = "This page was opened over plain HTTP, but the sign-in cookie is only \
    sent over HTTPS. Open it through your HTTPS reverse proxy, or set \
    LOCALAPP_SECURE_COOKIES=false if you really serve it over HTTP.";

fn login_form(st: &Shared, headers: &HeaderMap, username: &str, next: &str) -> Markup {
    html! {
        h1 { "Sign in" }
        @if cookies_will_be_dropped(st, headers) {
            (notice("warn", html! { (INSECURE_NOTE) }))
        }
        form method="post" action=(st.url("/login")) class="form-stack" {
            input type="hidden" name="next" value=(next);
            (field("Username", "", html! {
                input type="text" name="username" value=(username)
                    autocomplete="username" required autofocus;
            }))
            (field("Password", "", html! {
                input type="password" name="password" autocomplete="current-password" required;
            }))
            (checkbox("remember", false, html! { "Keep me signed in on this device" }))
            button type="submit" class="primary" { "Sign in" }
        }
        @if st.setup_token().is_some() {
            p class="muted" {
                "No admin account exists yet. Use the setup link printed in the server log "
                "(" code { "docker compose logs localapp" } ") to create one."
            }
        } @else {
            p class="muted" { "Forgotten your password? Ask an admin to reset it." }
        }
    }
}

async fn login_page(
    State(st): State<Shared>,
    Query(q): Query<NextQuery>,
    headers: HeaderMap,
) -> Response {
    let next = safe_next(&st, q.next.as_deref());
    let signed_in = read_cookie(&headers, SESSION_COOKIE).and_then(|t| {
        st.auth
            .session_user(&hash_token(&t), chrono::Utc::now().timestamp())
            .ok()
            .flatten()
    });
    if signed_in.is_some() {
        return flash::redirect(&next);
    }
    bare_page(&st, "Sign in", None, login_form(&st, &headers, "", &next)).into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    remember: Option<String>,
    next: Option<String>,
}

async fn login(
    State(st): State<Shared>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Form(f): Form<LoginForm>,
) -> AppResult<Response> {
    let next = safe_next(&st, f.next.as_deref());
    let username = f.username.trim().to_string();
    let keys = vec![
        format!("user:{}", username.to_lowercase()),
        format!("ip:{ip}"),
    ];
    let fail = |status: StatusCode, msg: &str| {
        let form = login_form(&st, &headers, &username, &next);
        (status, bare_page(&st, "Sign in", Some(msg), form)).into_response()
    };
    if let Err(wait) = st.limiter.check(&keys, Instant::now()) {
        let msg = format!(
            "Too many attempts. Try again in {} seconds.",
            wait.as_secs().max(1)
        );
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, &msg));
    }
    let creds = st.auth.credentials(&username)?;
    let pw = f.password;
    // Argon2 is deliberately slow: keep it off the async runtime.
    let user = tokio::task::spawn_blocking(move || match creds {
        Some((user, hash)) if password::verify(&pw, &hash) && !user.disabled => Some(user),
        Some(_) => None,
        None => {
            password::verify_dummy(&pw);
            None
        }
    })
    .await
    .map_err(anyhow::Error::from)?;
    let Some(user) = user else {
        st.limiter.record_failure(&keys, Instant::now());
        tracing::info!("failed sign-in for {username:?} from {ip}");
        return Ok(fail(
            StatusCode::UNAUTHORIZED,
            "Wrong username or password.",
        ));
    };
    st.limiter.record_success(&keys);
    tracing::info!("{} signed in from {ip}", user.username);
    sign_in(&st, &user, checked(&f.remember), &next)
}

async fn logout(State(st): State<Shared>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(token) = read_cookie(&headers, SESSION_COOKIE) {
        st.auth.delete_session(&hash_token(&token))?;
    }
    let mut resp = flash::redirect(&st.url("/login"));
    append_cookie(&mut resp, &clear_session_cookie(&st.cfg));
    Ok(resp)
}

// ---- First-run setup -----------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct SetupQuery {
    token: Option<String>,
}

fn setup_form(st: &Shared, token: &str, username: &str) -> Markup {
    html! {
        h1 { "Welcome to " (APP_NAME) }
        p { "Create the first admin account. Admins add the other people who use the app." }
        form method="post" action=(st.url("/setup")) class="form-stack" {
            input type="hidden" name="token" value=(token);
            (field("Username", "Letters, digits, dots, dashes, underscores or @.", html! {
                input type="text" name="username" value=(username)
                    autocomplete="username" required autofocus;
            }))
            (field("Your name", "Shown in the menu. Optional.", html! {
                input type="text" name="display_name" autocomplete="name";
            }))
            (field("Password", &format!("At least {} characters.", password::MIN_LENGTH), html! {
                input type="password" name="password" autocomplete="new-password" required;
            }))
            (field("Confirm password", "", html! {
                input type="password" name="confirm" autocomplete="new-password" required;
            }))
            button type="submit" class="primary" { "Create admin account" }
        }
    }
}

fn token_matches(st: &Shared, given: Option<&str>) -> bool {
    st.setup_token()
        .is_some_and(|t| given.is_some_and(|g| g.as_bytes() == t.as_bytes()))
}

async fn setup_page(State(st): State<Shared>, Query(q): Query<SetupQuery>) -> Response {
    if st.setup_token().is_none() {
        return flash::redirect(&st.url("/login"));
    }
    if !token_matches(&st, q.token.as_deref()) {
        let body = html! {
            h1 { "Setup link needed" }
            p { "Open the setup link printed in the server log at start-up, or run "
                code { "localapp-server admin create <name>" } " inside the container." }
        };
        return (StatusCode::FORBIDDEN, bare_page(&st, "Setup", None, body)).into_response();
    }
    let token = q.token.unwrap_or_default();
    bare_page(&st, "Setup", None, setup_form(&st, &token, "")).into_response()
}

#[derive(Deserialize)]
pub struct SetupForm {
    token: String,
    username: String,
    display_name: String,
    password: String,
    confirm: String,
}

async fn setup(State(st): State<Shared>, Form(f): Form<SetupForm>) -> AppResult<Response> {
    if !token_matches(&st, Some(&f.token)) {
        return Ok(flash::redirect(&st.url("/login")));
    }
    let retry = |msg: &str| {
        let body = setup_form(&st, &f.token, &f.username);
        (
            StatusCode::BAD_REQUEST,
            bare_page(&st, "Setup", Some(msg), body),
        )
            .into_response()
    };
    if let Err(msg) = password::validate_new(&f.password, &f.confirm) {
        return Ok(retry(&msg));
    }
    let fields = UserFields {
        username: f.username.clone(),
        display_name: f.display_name.clone(),
        role: Some(Role::Admin),
        disabled: false,
    };
    let hash = password::hash(&f.password)?;
    let id = match st.auth.create_user(&fields, &hash, false) {
        Ok(id) => id,
        Err(e) => return Ok(retry(&e.to_string())),
    };
    st.clear_setup_token();
    let user = st
        .auth
        .user(id)?
        .ok_or_else(|| anyhow::anyhow!("new admin vanished"))?;
    tracing::info!("first admin {:?} created via setup page", user.username);
    sign_in(&st, &user, false, &st.url("/"))
}
