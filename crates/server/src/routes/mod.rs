//! URL routing. Everything is mounted under the configured base path.
//!
//! ```text
//! /healthz, /static/*              public
//! /login, /logout, /setup          sign-in (public)
//! /, /account                      signed in
//! /admin/*                         admins (enforced by `AdminCtx`)
//! ```
//!
//! To add a page: a module here with a `routes()` function, merged into
//! `signed_in` below, and an entry in `views/nav.rs`.

pub mod account;
pub mod admin;
pub mod home;
pub mod signin;

use crate::state::Shared;
use crate::{assets, auth, flash};
use axum::Router;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::get;
use tower::Layer;
use tower_http::compression::CompressionLayer;
use tower_http::normalize_path::{NormalizePath, NormalizePathLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

/// Content Security Policy: only this server's own scripts, styles and images.
const CSP: &str = concat!(
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; ",
    "img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
);

fn default_header(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::if_not_present(name, HeaderValue::from_static(value))
}

/// The complete app: routes behind trailing-slash normalisation, so
/// `/localapp`, `/localapp/` and `/localapp/account/` all resolve.
pub type App = NormalizePath<Router>;

pub fn router(st: Shared) -> App {
    NormalizePathLayer::trim_trailing_slash().layer(routes(st))
}

fn routes(st: Shared) -> Router {
    let signed_in = Router::new()
        .route("/", get(home::show))
        .merge(account::routes())
        .merge(admin::routes())
        .route_layer(from_fn_with_state(st.clone(), auth::require_login));

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/static/{name}", get(assets::serve))
        .merge(signin::routes())
        .merge(signed_in)
        .layer(from_fn_with_state(st.clone(), flash::layer))
        .layer(from_fn(auth::origin::guard))
        .layer(default_header(header::CONTENT_SECURITY_POLICY, CSP))
        .layer(default_header(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(default_header(header::REFERRER_POLICY, "same-origin"))
        .layer(default_header(header::X_FRAME_OPTIONS, "DENY"))
        // Pages show private data: never cache them in the browser or a proxy.
        // (Static assets set their own long-lived Cache-Control.)
        .layer(default_header(header::CACHE_CONTROL, "no-store"))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(st.clone());

    let base = st.cfg.base_path.clone();
    if base.is_empty() {
        return app;
    }
    let home = base.clone();
    Router::new()
        .route("/", get(move || async move { flash::redirect(&home) }))
        .nest(&base, app)
}
