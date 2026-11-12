//! Static files compiled into the binary (nothing is fetched from a CDN).
//!
//! To add a file, put it in `crates/server/assets/` and list it in [`ASSETS`]
//! (and in [`SCRIPTS`] if every page should load it).

use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

struct Asset {
    name: &'static str,
    mime: &'static str,
    bytes: &'static [u8],
}

const fn js(name: &'static str, bytes: &'static [u8]) -> Asset {
    Asset {
        name,
        mime: "text/javascript; charset=utf-8",
        bytes,
    }
}

/// Scripts in the order pages load them (libraries before the code using them).
pub const SCRIPTS: &[&str] = &["htmx.min.js", "app.js", "planner.js"];

const ASSETS: &[Asset] = &[
    Asset {
        name: "app.css",
        mime: "text/css; charset=utf-8",
        bytes: include_bytes!("../assets/app.css"),
    },
    Asset {
        name: "favicon.svg",
        mime: "image/svg+xml",
        bytes: include_bytes!("../assets/favicon.svg"),
    },
    // Vendored library: htmx 2.0.4 (MIT).
    js("htmx.min.js", include_bytes!("../assets/htmx.min.js")),
    js("app.js", include_bytes!("../assets/app.js")),
    js("planner.js", include_bytes!("../assets/planner.js")),
];

/// Short content hash appended to asset URLs, so browsers can cache them forever.
pub fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let mut h = Sha256::new();
        for a in ASSETS {
            h.update(a.bytes);
        }
        crate::auth::session::hex(&h.finalize()[..6])
    })
}

pub async fn serve(Path(name): Path<String>) -> Response {
    let Some(a) = ASSETS.iter().find(|a| a.name == name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut resp = a.bytes.into_response();
    let headers = resp.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(a.mime));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    resp
}
