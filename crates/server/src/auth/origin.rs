//! Cross-site request forgery protection.
//!
//! Every state-changing request (anything but GET/HEAD/OPTIONS) must come from
//! a page on this same site. Browsers say where a request came from in the
//! `Sec-Fetch-Site` header (all current browsers) and in `Origin`. A forged
//! form on another site is rejected before it reaches a handler. Session
//! cookies are also `SameSite=Lax`, so the browser wouldn't send them with a
//! cross-site POST anyway; this check is the second layer.

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

fn host_of(origin: &str) -> Option<&str> {
    let rest = origin.split_once("://")?.1;
    Some(rest.split('/').next().unwrap_or(rest))
}

/// Whether a request with these headers is allowed to change state.
pub fn same_origin(method: &Method, headers: &HeaderMap) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(site) = get("sec-fetch-site") {
        return site == "same-origin" || site == "none";
    }
    match get(header::ORIGIN.as_str()) {
        // No browser provenance headers at all: not a browser (curl, scripts). Allowed;
        // such clients still need a valid session cookie.
        None => true,
        Some(origin) => {
            let Some(origin_host) = host_of(origin) else {
                return false;
            };
            let host = get("x-forwarded-host").or_else(|| get(header::HOST.as_str()));
            host.is_some_and(|h| {
                h.split(',')
                    .next()
                    .unwrap_or(h)
                    .trim()
                    .eq_ignore_ascii_case(origin_host)
            })
        }
    }
}

pub async fn guard(req: Request<Body>, next: Next) -> Response {
    if same_origin(req.method(), req.headers()) {
        next.run(req).await
    } else {
        tracing::warn!("blocked cross-site {} {}", req.method(), req.uri().path());
        (StatusCode::FORBIDDEN, "Cross-site request blocked.").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn decides_by_fetch_metadata_then_origin() {
        let post = Method::POST;
        assert!(same_origin(
            &Method::GET,
            &headers(&[("sec-fetch-site", "cross-site")])
        ));
        assert!(same_origin(
            &post,
            &headers(&[("sec-fetch-site", "same-origin")])
        ));
        assert!(!same_origin(
            &post,
            &headers(&[("sec-fetch-site", "cross-site")])
        ));
        assert!(!same_origin(
            &post,
            &headers(&[("sec-fetch-site", "same-site")])
        ));
        assert!(same_origin(
            &post,
            &headers(&[
                ("origin", "https://miniserver.local"),
                ("host", "miniserver.local")
            ])
        ));
        assert!(!same_origin(
            &post,
            &headers(&[
                ("origin", "https://evil.example"),
                ("host", "miniserver.local")
            ])
        ));
        assert!(same_origin(
            &post,
            &headers(&[
                ("origin", "https://miniserver.local"),
                ("host", "localapp:8080"),
                ("x-forwarded-host", "miniserver.local")
            ])
        ));
        assert!(!same_origin(
            &post,
            &headers(&[("origin", "null"), ("host", "miniserver.local")])
        ));
        assert!(same_origin(&post, &headers(&[])));
    }
}
