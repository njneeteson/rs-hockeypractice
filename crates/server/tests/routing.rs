//! Base path, sign-in gate, static assets and security headers.

mod common;

use axum::http::StatusCode;
use common::{TestApp, url};

#[tokio::test]
async fn health_and_assets_are_public() {
    let mut app = TestApp::new();
    assert_eq!(app.get(&url("/healthz")).await.body, "ok");
    let css = app.get(&url("/static/app.css")).await;
    assert_eq!(css.status, StatusCode::OK);
    assert!(css.body.contains("light-dark("));
    for name in ["htmx.min.js", "app.js", "planner.js", "favicon.svg"] {
        let r = app.get(&url(&format!("/static/{name}"))).await;
        assert_eq!(r.status, StatusCode::OK, "{name}");
    }
    assert_eq!(
        app.get(&url("/static/nope.js")).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn everything_lives_under_the_base_path() {
    let mut app = TestApp::new();
    assert_eq!(app.get("/").await.redirect(), "/localapp");
    assert_eq!(app.get("/healthz").await.status, StatusCode::NOT_FOUND);
    for path in ["/localapp", "/localapp/"] {
        let r = app.get(path).await;
        assert!(
            r.redirect().starts_with("/localapp/login?next="),
            "{path} → {:?}",
            r.location
        );
    }
}

#[tokio::test]
async fn pages_need_sign_in_and_links_carry_the_base_path() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let mut anon = app.client();
    let r = anon.get(&url("/account?tab=x")).await;
    assert_eq!(
        r.redirect(),
        "/localapp/login?next=%2Flocalapp%2Faccount%3Ftab%3Dx"
    );

    for path in ["", "/", "/account", "/admin/users"] {
        let r = app.get(&url(path)).await;
        assert_eq!(r.status, StatusCode::OK, "{path}: {}", r.body);
        for href in r
            .body
            .split("href=\"")
            .skip(1)
            .map(|s| s.split('"').next().unwrap())
        {
            assert!(
                href.starts_with("/localapp")
                    || href.starts_with('#')
                    || href.starts_with("javascript:"),
                "{path}: link {href} escapes the base path"
            );
        }
    }
}
