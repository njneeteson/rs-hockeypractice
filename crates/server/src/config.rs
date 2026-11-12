//! Server configuration, read from `LOCALAPP_*` environment variables.
//!
//! To add a setting: a field here, an `ENV_*` constant, a default in
//! [`Config::defaults`], parsing in [`Config::from_lookup`], and a line in the
//! README's configuration table.

use anyhow::{Context, bail};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    /// URL prefix the app is served under, normalised to `""` (root) or
    /// `"/localapp"` (no trailing slash).
    pub base_path: String,
    pub bind: SocketAddr,
    /// Holds `auth.sqlite` (accounts, sessions) and `app.sqlite` (app data).
    pub data_dir: PathBuf,
    /// Mark cookies `Secure` (HTTPS only). Disable only when testing over plain HTTP.
    pub secure_cookies: bool,
    /// Lifetime of a "keep me signed in" session, renewed on activity.
    pub session_days: i64,
    /// Optional first admin, created at start-up when no admin exists.
    pub bootstrap_admin: Option<(String, String)>,
}

pub const ENV_BASE_PATH: &str = "LOCALAPP_BASE_PATH";
pub const ENV_BIND: &str = "LOCALAPP_BIND";
pub const ENV_DATA_DIR: &str = "LOCALAPP_DATA_DIR";
pub const ENV_SECURE_COOKIES: &str = "LOCALAPP_SECURE_COOKIES";
pub const ENV_SESSION_DAYS: &str = "LOCALAPP_SESSION_DAYS";
pub const ENV_ADMIN_USER: &str = "LOCALAPP_ADMIN_USER";
pub const ENV_ADMIN_PASSWORD: &str = "LOCALAPP_ADMIN_PASSWORD";

/// `"/"`, `""`, `"localapp"`, `"/localapp/"` → `""` or `"/localapp"`.
pub fn normalize_base_path(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let ok = trimmed.split('/').all(|seg| {
        !seg.is_empty()
            && seg != "."
            && seg != ".."
            && seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.~".contains(c))
    });
    if !ok {
        bail!(
            "{ENV_BASE_PATH} must be a URL path such as /localapp (letters, digits, - _ . ~), got \
                {raw:?}"
        );
    }
    Ok(format!("/{trimmed}"))
}

fn parse_bool(name: &str, value: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => bail!("{name} must be true or false, got {other:?}"),
    }
}

impl Config {
    /// Defaults suitable for the container: `/data`, port 8080, served at the root.
    pub fn defaults() -> Self {
        Self {
            base_path: String::new(),
            bind: SocketAddr::from(([0, 0, 0, 0], 8080)),
            data_dir: PathBuf::from("/data"),
            secure_cookies: true,
            session_days: 30,
            bootstrap_admin: None,
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Build from any key lookup (the environment in production, a map in tests).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let mut c = Self::defaults();
        if let Some(v) = get(ENV_BASE_PATH) {
            c.base_path = normalize_base_path(&v)?;
        }
        if let Some(v) = get(ENV_BIND) {
            c.bind = v
                .parse()
                .with_context(|| format!("{ENV_BIND} must look like 0.0.0.0:8080, got {v:?}"))?;
        }
        if let Some(v) = get(ENV_DATA_DIR).filter(|v| !v.trim().is_empty()) {
            c.data_dir = PathBuf::from(v);
        }
        if let Some(v) = get(ENV_SECURE_COOKIES) {
            c.secure_cookies = parse_bool(ENV_SECURE_COOKIES, &v)?;
        }
        if let Some(v) = get(ENV_SESSION_DAYS) {
            c.session_days = v
                .trim()
                .parse()
                .ok()
                .filter(|d| (1..=365).contains(d))
                .with_context(|| {
                    format!("{ENV_SESSION_DAYS} must be a whole number of days, 1–365, got {v:?}")
                })?;
        }
        c.bootstrap_admin = match (get(ENV_ADMIN_USER), get(ENV_ADMIN_PASSWORD)) {
            (Some(u), Some(p)) if !u.trim().is_empty() && !p.is_empty() => {
                Some((u.trim().to_string(), p))
            }
            (Some(_), _) | (_, Some(_)) => {
                bail!("set both {ENV_ADMIN_USER} and {ENV_ADMIN_PASSWORD}, or neither")
            }
            _ => None,
        };
        Ok(c)
    }

    pub fn auth_db_path(&self) -> PathBuf {
        self.data_dir.join("auth.sqlite")
    }

    pub fn app_db_path(&self) -> PathBuf {
        self.data_dir.join(localapp_core::db::DB_FILE)
    }

    /// Cookie `Path` attribute: the base path, or `/` when served at the root.
    pub fn cookie_path(&self) -> &str {
        if self.base_path.is_empty() {
            "/"
        } else {
            &self.base_path
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn base_path_normalisation() {
        assert_eq!(normalize_base_path("/").unwrap(), "");
        assert_eq!(normalize_base_path("").unwrap(), "");
        assert_eq!(normalize_base_path("localapp").unwrap(), "/localapp");
        assert_eq!(normalize_base_path("/localapp/").unwrap(), "/localapp");
        assert_eq!(normalize_base_path("/apps/mine").unwrap(), "/apps/mine");
        assert!(normalize_base_path("/../etc").is_err());
        assert!(normalize_base_path("/a b").is_err());
        assert!(normalize_base_path("/a//b").is_err());
    }

    #[test]
    fn reads_environment() {
        let env: HashMap<&str, &str> = [
            (ENV_BASE_PATH, "/localapp/"),
            (ENV_BIND, "127.0.0.1:9000"),
            (ENV_SECURE_COOKIES, "false"),
            (ENV_SESSION_DAYS, "7"),
            (ENV_ADMIN_USER, "alice"),
            (ENV_ADMIN_PASSWORD, "correct horse"),
        ]
        .into();
        let c = Config::from_lookup(|k| env.get(k).map(|v| v.to_string())).unwrap();
        assert_eq!(c.base_path, "/localapp");
        assert_eq!(c.cookie_path(), "/localapp");
        assert_eq!(c.bind.port(), 9000);
        assert!(!c.secure_cookies);
        assert_eq!(c.session_days, 7);
        assert_eq!(
            c.bootstrap_admin,
            Some(("alice".into(), "correct horse".into()))
        );
        assert!(Config::from_lookup(|k| (k == ENV_ADMIN_USER).then(|| "bob".to_string())).is_err());
        assert!(Config::from_lookup(|k| (k == ENV_SESSION_DAYS).then(|| "0".to_string())).is_err());
    }
}
