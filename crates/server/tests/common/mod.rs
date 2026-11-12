//! Test harness: the whole app under `/localapp`, with a cookie jar.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use localapp_server::auth::password;
use localapp_server::auth::store::{Role, UserFields};
use localapp_server::{AppState, Config, Shared, router};
use std::collections::HashMap;
use tower::ServiceExt;

pub const BASE: &str = "/localapp";
pub const PASSWORD: &str = "correct horse battery";

pub struct TestApp {
    pub st: Shared,
    app: localapp_server::routes::App,
    cookies: HashMap<String, String>,
    _dir: std::sync::Arc<tempfile::TempDir>,
}

pub struct Resp {
    pub status: StatusCode,
    pub location: Option<String>,
    pub body: String,
}

impl Resp {
    /// Assert a redirect and return its target.
    pub fn redirect(&self) -> &str {
        assert!(
            self.status.is_redirection(),
            "expected a redirect, got {} {}",
            self.status,
            self.body
        );
        self.location.as_deref().expect("Location header")
    }
}

impl TestApp {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::defaults();
        cfg.base_path = BASE.into();
        cfg.data_dir = dir.path().to_path_buf();
        cfg.secure_cookies = false;
        let st = AppState::new(cfg).unwrap();
        Self {
            app: router(st.clone()),
            st,
            cookies: HashMap::new(),
            _dir: std::sync::Arc::new(dir),
        }
    }

    /// A new client (empty cookie jar) for the same server.
    pub fn client(&self) -> Self {
        Self {
            st: self.st.clone(),
            app: self.app.clone(),
            cookies: HashMap::new(),
            _dir: self._dir.clone(),
        }
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub async fn send(&mut self, req: Request<Body>) -> Resp {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        for v in resp.headers().get_all(header::SET_COOKIE) {
            let s = v.to_str().unwrap();
            let (pair, attrs) = s.split_once(';').unwrap_or((s, ""));
            let (name, value) = pair.split_once('=').unwrap();
            if attrs.contains("Max-Age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.into(), value.into());
            }
        }
        let status = resp.status();
        let location = resp
            .headers()
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string());
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        Resp {
            status,
            location,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    pub async fn get(&mut self, path: &str) -> Resp {
        let req = Request::get(path)
            .header(header::COOKIE, self.cookie_header())
            .body(Body::empty())
            .unwrap();
        self.send(req).await
    }

    /// POST a form as a same-origin browser would.
    pub async fn post(&mut self, path: &str, form: &[(&str, &str)]) -> Resp {
        self.post_from(path, form, "same-origin").await
    }

    pub async fn post_from(&mut self, path: &str, form: &[(&str, &str)], fetch_site: &str) -> Resp {
        self.post_with(path, form, &[("sec-fetch-site", fetch_site)])
            .await
    }

    /// POST as htmx does for a fragment (not a boosted page load).
    pub async fn post_htmx(&mut self, path: &str, form: &[(&str, &str)]) -> Resp {
        let headers = [("sec-fetch-site", "same-origin"), ("hx-request", "true")];
        self.post_with(path, form, &headers).await
    }

    async fn post_with(
        &mut self,
        path: &str,
        form: &[(&str, &str)],
        headers: &[(&str, &str)],
    ) -> Resp {
        let body = serde_html_form::to_string(form).unwrap();
        let mut req = Request::post(path)
            .header(header::COOKIE, self.cookie_header())
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        self.send(req.body(Body::from(body)).unwrap()).await
    }

    pub fn create_user(&self, name: &str, role: Role) -> i64 {
        let f = UserFields {
            username: name.into(),
            role: Some(role),
            ..Default::default()
        };
        self.st
            .auth
            .create_user(&f, &password::hash(PASSWORD).unwrap(), false)
            .unwrap()
    }

    pub async fn login(&mut self, name: &str) {
        let r = self
            .post(
                &format!("{BASE}/login"),
                &[("username", name), ("password", PASSWORD)],
            )
            .await;
        assert!(
            r.status.is_redirection(),
            "login failed: {} {}",
            r.status,
            r.body
        );
    }

    /// An admin account, signed in.
    pub async fn with_admin(&mut self) -> i64 {
        let id = self.create_user("admin", Role::Admin);
        self.login("admin").await;
        id
    }
}

pub fn url(path: &str) -> String {
    format!("{BASE}{path}")
}
