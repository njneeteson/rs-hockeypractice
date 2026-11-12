//! Session tokens and cookies.
//!
//! The browser holds a random 256-bit token; the database stores only its
//! SHA-256, so a copy of `auth.sqlite` can't be used to hijack sessions.

use crate::config::Config;
use axum::http::header::{self, COOKIE};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::Response;
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

pub const SESSION_COOKIE: &str = "localapp_session";
/// Server-side lifetime of a session without "keep me signed in" (renewed on activity).
pub const SHORT_SESSION_SECS: i64 = 12 * 3600;
/// Sessions are renewed at most this often, to avoid a write on every request.
pub const TOUCH_INTERVAL_SECS: i64 = 300;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

pub fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn lifetime_secs(cfg: &Config, persistent: bool) -> i64 {
    if persistent {
        cfg.session_days * 86_400
    } else {
        SHORT_SESSION_SECS
    }
}

/// Value of the named cookie in the request, if present.
pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// A `Set-Cookie` value. `max_age: None` makes a browser-session cookie; `Some(0)` deletes it.
pub fn cookie(cfg: &Config, name: &str, value: &str, max_age: Option<i64>) -> String {
    let mut c = format!(
        "{name}={value}; Path={}; HttpOnly; SameSite=Lax",
        cfg.cookie_path()
    );
    if cfg.secure_cookies {
        c.push_str("; Secure");
    }
    if let Some(age) = max_age {
        c.push_str(&format!("; Max-Age={age}"));
    }
    c
}

pub fn session_cookie(cfg: &Config, token: &str, persistent: bool) -> String {
    cookie(
        cfg,
        SESSION_COOKIE,
        token,
        persistent.then(|| lifetime_secs(cfg, true)),
    )
}

pub fn clear_session_cookie(cfg: &Config) -> String {
    cookie(cfg, SESSION_COOKIE, "", Some(0))
}

/// Add a `Set-Cookie` header to a response.
pub fn append_cookie(resp: &mut Response, set_cookie: &str) {
    if let Ok(v) = HeaderValue::from_str(set_cookie) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_cookies() {
        let t = new_token();
        assert_eq!(t.len(), 64);
        assert_ne!(t, new_token());
        assert_eq!(hash_token(&t).len(), 64);
        let mut cfg = Config::defaults();
        cfg.base_path = "/localapp".into();
        let c = session_cookie(&cfg, "abc", false);
        assert_eq!(
            c,
            "localapp_session=abc; Path=/localapp; HttpOnly; SameSite=Lax; Secure"
        );
        assert!(session_cookie(&cfg, "abc", true).contains("Max-Age=2592000"));
        let mut h = HeaderMap::new();
        h.append(
            COOKIE,
            HeaderValue::from_static("a=1; localapp_session=xyz"),
        );
        assert_eq!(read_cookie(&h, SESSION_COOKIE).as_deref(), Some("xyz"));
        assert_eq!(read_cookie(&h, "b"), None);
    }
}
