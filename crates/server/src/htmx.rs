//! Recognising htmx requests.
//!
//! The whole page is "boosted": htmx loads links and form results in place,
//! but the server still answers with full pages and redirects, exactly as for
//! a browser without JavaScript. A few controls ask for just a fragment (one
//! table row, say); those requests carry `HX-Request` without `HX-Boosted`.

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

/// An htmx request for a page fragment, rather than a boosted page load.
pub fn wants_fragment(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request") && !headers.contains_key("hx-boosted")
}

/// Tell htmx to load `to` as a whole page (e.g. the sign-in page when a
/// session expires during a fragment request).
pub fn redirect_page(to: &str) -> Response {
    let mut resp = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = HeaderValue::from_str(to) {
        resp.headers_mut().insert("hx-redirect", v);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_vs_boosted_pages() {
        let mut h = HeaderMap::new();
        assert!(!wants_fragment(&h));
        h.insert("hx-request", HeaderValue::from_static("true"));
        assert!(wants_fragment(&h));
        h.insert("hx-boosted", HeaderValue::from_static("true"));
        assert!(!wants_fragment(&h));
    }
}
