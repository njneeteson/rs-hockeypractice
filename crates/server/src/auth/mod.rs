//! Local accounts: password sign-in, sessions and the "must be signed in" gate.

pub mod origin;
pub mod password;
pub mod ratelimit;
pub mod session;
pub mod store;

use crate::flash;
use crate::state::Shared;
use axum::body::Body;
use axum::extract::{OriginalUri, State};
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use session::{SESSION_COOKIE, TOUCH_INTERVAL_SECS, hash_token, lifetime_secs, read_cookie};
use std::sync::Arc;
use store::User;

/// The signed-in user, inserted into request extensions by [`require_login`].
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub user: Arc<User>,
    /// Hash of this request's session token (to keep this session when signing out others).
    pub session: Arc<str>,
}

/// Paths (relative to the base path) a user who must change their password may still use.
const PASSWORD_CHANGE_PATHS: &[&str] = &["/account/password", "/logout"];

/// Full path + query of the request, for the `next=` parameter of the sign-in page.
fn original_path(req: &Request<Body>) -> String {
    let uri = req
        .extensions()
        .get::<OriginalUri>()
        .map_or_else(|| req.uri().clone(), |o| o.0.clone());
    uri.path_and_query()
        .map_or_else(|| uri.path().to_string(), |p| p.as_str().to_string())
}

/// Everything but RFC 3986 "unreserved" characters is encoded in a query value.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Send the browser to the sign-in page, remembering where it was going.
fn to_login(st: &Shared, req: &Request<Body>) -> Response {
    let path = original_path(req);
    let next = utf8_percent_encode(&path, QUERY_VALUE);
    let login = format!("{}?next={next}", st.url("/login"));
    // A fragment request (one table row…) must not get the sign-in page as its fragment.
    if crate::htmx::wants_fragment(req.headers()) {
        return crate::htmx::redirect_page(&login);
    }
    flash::redirect(&login)
}

/// Create a session for a user; returns the `Set-Cookie` value for the browser.
pub fn start_session(st: &Shared, user_id: i64, persistent: bool) -> anyhow::Result<String> {
    let token = session::new_token();
    let expires = chrono::Utc::now().timestamp() + lifetime_secs(&st.cfg, persistent);
    st.auth
        .insert_session(&hash_token(&token), user_id, persistent, expires)?;
    Ok(session::session_cookie(&st.cfg, &token, persistent))
}

/// Middleware for every page behind sign-in.
pub async fn require_login(
    State(st): State<Shared>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let now = chrono::Utc::now().timestamp();
    let found = read_cookie(req.headers(), SESSION_COOKIE).and_then(|token| {
        let hash = hash_token(&token);
        match st.auth.session_user(&hash, now) {
            Ok(found) => found.map(|f| (hash, f)),
            Err(e) => {
                tracing::error!("session lookup failed: {e:#}");
                None
            }
        }
    });
    let Some((hash, (user, persistent, last_seen))) = found else {
        return to_login(&st, &req);
    };
    if now - last_seen >= TOUCH_INTERVAL_SECS
        && let Err(e) = st
            .auth
            .touch_session(&hash, now, now + lifetime_secs(&st.cfg, persistent))
    {
        tracing::warn!("couldn't renew session: {e:#}");
    }
    if user.must_change_password && !PASSWORD_CHANGE_PATHS.contains(&req.uri().path()) {
        return flash::redirect(&st.url("/account/password"));
    }
    req.extensions_mut().insert(CurrentUser {
        user: Arc::new(user),
        session: hash.into(),
    });
    next.run(req).await
}
