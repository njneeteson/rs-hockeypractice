//! One-shot messages shown on the next page ("Saved", "Couldn't save: …").
//!
//! A handler that changes something redirects (POST → 303 → GET) and stores the
//! message in a short-lived cookie. The next full page shows it; the
//! middleware then deletes the cookie.

use crate::auth::session::{append_cookie, cookie, read_cookie};
use crate::state::Shared;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const FLASH_COOKIE: &str = "localapp_flash";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flash {
    pub kind: Kind,
    pub message: String,
}

/// The incoming flash message plus a "was shown" marker set by the page layout.
#[derive(Clone, Default)]
pub struct FlashSlot {
    pub flash: Option<Flash>,
    pub shown: Arc<AtomicBool>,
}

impl FlashSlot {
    /// Take the message for display (marks it as shown).
    pub fn take(&self) -> Option<Flash> {
        self.flash
            .clone()
            .inspect(|_| self.shown.store(true, Ordering::Relaxed))
    }
}

fn encode(f: &Flash) -> String {
    let tag = if f.kind == Kind::Error { 'e' } else { 'i' };
    let msg: String = f.message.chars().take(600).collect();
    format!("{tag}{}", utf8_percent_encode(&msg, NON_ALPHANUMERIC))
}

fn decode(raw: &str) -> Option<Flash> {
    let kind = match raw.chars().next()? {
        'e' => Kind::Error,
        'i' => Kind::Info,
        _ => return None,
    };
    let message = percent_decode_str(&raw[1..])
        .decode_utf8()
        .ok()?
        .into_owned();
    Some(Flash { kind, message })
}

/// A `303 See Other` redirect to `to` (a full path) carrying a message.
pub fn redirect_with(st: &Shared, to: &str, kind: Kind, message: impl Into<String>) -> Response {
    let mut resp = redirect(to);
    let value = encode(&Flash {
        kind,
        message: message.into(),
    });
    append_cookie(&mut resp, &cookie(&st.cfg, FLASH_COOKIE, &value, Some(60)));
    resp
}

pub fn info(st: &Shared, to: &str, message: impl Into<String>) -> Response {
    redirect_with(st, to, Kind::Info, message)
}

pub fn error(st: &Shared, to: &str, message: impl Into<String>) -> Response {
    redirect_with(st, to, Kind::Error, message)
}

/// Redirect to `to` with the success message, or the error as an error message.
pub fn report<M: Into<String>>(st: &Shared, to: &str, result: anyhow::Result<M>) -> Response {
    match result {
        Ok(msg) => info(st, to, msg),
        Err(e) => error(st, to, format!("{e:#}")),
    }
}

/// A plain `303 See Other` redirect.
pub fn redirect(to: &str) -> Response {
    let mut resp = StatusCode::SEE_OTHER.into_response();
    if let Ok(v) = HeaderValue::from_str(to) {
        resp.headers_mut().insert(header::LOCATION, v);
    }
    resp
}

pub async fn layer(State(st): State<Shared>, mut req: Request<Body>, next: Next) -> Response {
    let slot = FlashSlot {
        flash: read_cookie(req.headers(), FLASH_COOKIE).and_then(|v| decode(&v)),
        ..Default::default()
    };
    let had_flash = slot.flash.is_some();
    let shown = slot.shown.clone();
    req.extensions_mut().insert(slot);
    let mut resp = next.run(req).await;
    let sets_new = resp.headers().get_all(header::SET_COOKIE).iter().any(|v| {
        v.to_str()
            .is_ok_and(|s| s.starts_with(&format!("{FLASH_COOKIE}=")))
    });
    if had_flash && shown.load(Ordering::Relaxed) && !sets_new {
        append_cookie(&mut resp, &cookie(&st.cfg, FLASH_COOKIE, "", Some(0)));
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let f = Flash {
            kind: Kind::Error,
            message: "Couldn't save: “bad”; try again".into(),
        };
        assert_eq!(decode(&encode(&f)), Some(f));
        assert_eq!(decode("x123"), None);
    }
}
