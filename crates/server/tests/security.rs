//! Sign-in, sessions, roles and cross-site protection.

mod common;

use axum::http::StatusCode;
use common::{PASSWORD, TestApp, url};
use localapp_server::auth::store::Role;

#[tokio::test]
async fn wrong_password_fails_and_is_throttled() {
    let mut app = TestApp::new();
    app.create_user("alice", Role::Admin);
    for _ in 0..5 {
        let r = app
            .post(
                &url("/login"),
                &[("username", "alice"), ("password", "nope")],
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert!(r.body.contains("Wrong username or password."));
    }
    // Throttled now, even with the right password.
    let r = app
        .post(
            &url("/login"),
            &[("username", "alice"), ("password", PASSWORD)],
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn login_redirects_only_inside_the_app() {
    let mut app = TestApp::new();
    app.create_user("alice", Role::User);
    let next = [
        ("username", "alice"),
        ("password", PASSWORD),
        ("next", "https://evil.example/"),
    ];
    assert_eq!(
        app.post(&url("/login"), &next).await.redirect(),
        "/localapp/"
    );
    let mut b = app.client();
    let next = [
        ("username", "alice"),
        ("password", PASSWORD),
        ("next", "/localapp/account"),
    ];
    assert_eq!(
        b.post(&url("/login"), &next).await.redirect(),
        "/localapp/account"
    );
}

#[tokio::test]
async fn logout_ends_the_session() {
    let mut app = TestApp::new();
    app.with_admin().await;
    assert_eq!(app.get(&url("/account")).await.status, StatusCode::OK);
    app.post(&url("/logout"), &[]).await;
    assert!(
        app.get(&url("/account"))
            .await
            .redirect()
            .starts_with("/localapp/login")
    );
}

#[tokio::test]
async fn cross_site_posts_are_blocked() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let path = url("/account");
    let form = [("display_name", "Mallory"), ("theme", "dark")];
    for site in ["cross-site", "same-site"] {
        assert_eq!(
            app.post_from(&path, &form, site).await.status,
            StatusCode::FORBIDDEN,
            "{site}"
        );
    }
    assert!(app.post(&path, &form).await.status.is_redirection());
}

#[tokio::test]
async fn only_admins_manage_users() {
    let mut admin = TestApp::new();
    admin.with_admin().await;
    let mut user = admin.client();
    let bob = admin.create_user("bob", Role::User);
    user.login("bob").await;

    assert_eq!(user.get(&url("/")).await.status, StatusCode::OK);
    assert!(!user.get(&url("/")).await.body.contains("/admin/users"));
    assert_eq!(
        user.get(&url("/admin/users")).await.status,
        StatusCode::FORBIDDEN
    );
    let r = user
        .post(
            &url(&format!("/admin/users/{bob}")),
            &[("username", "bob"), ("role", "admin")],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(admin.st.auth.user(bob).unwrap().unwrap().role, Role::User);
}

#[tokio::test]
async fn new_users_must_replace_their_temporary_password() {
    let mut admin = TestApp::new();
    admin.with_admin().await;
    let r = admin
        .post(
            &url("/admin/users"),
            &[("username", "carol"), ("role", "user")],
        )
        .await;
    assert!(r.status.is_redirection());
    let flash = admin.get(r.redirect()).await.body;
    let temp = flash
        .split("Temporary password for carol: ")
        .nth(1)
        .and_then(|s| s.split('.').next())
        .expect("temporary password in the flash message")
        .to_string();

    let mut carol = admin.client();
    let r = carol
        .post(
            &url("/login"),
            &[("username", "carol"), ("password", &temp)],
        )
        .await;
    assert!(r.status.is_redirection());
    assert_eq!(
        carol.get(&url("/")).await.redirect(),
        "/localapp/account/password"
    );
    let new = "a brand new password";
    let r = carol
        .post(
            &url("/account/password"),
            &[("current", &temp), ("password", new), ("confirm", new)],
        )
        .await;
    assert_eq!(r.redirect(), "/localapp/account");
    assert_eq!(carol.get(&url("/")).await.status, StatusCode::OK);
}

#[tokio::test]
async fn first_admin_needs_the_setup_token() {
    let mut app = TestApp::new();
    let token = app
        .st
        .setup_token()
        .expect("no admin yet, so a setup token");
    assert_eq!(
        app.get(&url("/setup?token=wrong")).await.status,
        StatusCode::FORBIDDEN
    );
    let form = |t: &'static str| {
        [
            ("token", t),
            ("username", "root"),
            ("display_name", ""),
            ("password", PASSWORD),
            ("confirm", PASSWORD),
        ]
    };
    app.post(&url("/setup"), &form("wrong")).await;
    assert_eq!(app.st.auth.count_admins().unwrap(), 0);

    let token: &'static str = Box::leak(token.into_boxed_str());
    assert_eq!(
        app.get(&url(&format!("/setup?token={token}"))).await.status,
        StatusCode::OK
    );
    let r = app.post(&url("/setup"), &form(token)).await;
    assert_eq!(r.redirect(), "/localapp/");
    assert_eq!(app.st.auth.count_admins().unwrap(), 1);
    assert!(app.st.setup_token().is_none());
    assert_eq!(app.get(&url("/admin/users")).await.status, StatusCode::OK);
}

#[tokio::test]
async fn pages_send_security_headers() {
    use tower::ServiceExt;
    let app = TestApp::new();
    let resp = localapp_server::router(app.st.clone())
        .oneshot(
            axum::http::Request::get(url("/login"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let h = resp.headers();
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("script-src 'self'")
    );
    assert_eq!(h["x-frame-options"], "DENY");
    assert_eq!(h["cache-control"], "no-store");
}

/// Fail a sign-in as a different user each time, claiming a different
/// address each time in `header`; returns the status of the sixth try.
async fn sixth_attempt(app: &mut TestApp, header: &str) -> StatusCode {
    let mut last = StatusCode::OK;
    for i in 0..6 {
        let user = format!("user{i}");
        let ip = format!("203.0.113.{i}");
        let r = app
            .post_with(
                &url("/login"),
                &[("username", &user), ("password", "nope")],
                &[("sec-fetch-site", "same-origin"), (header, &ip)],
            )
            .await;
        last = r.status;
    }
    last
}

#[tokio::test]
async fn forged_forwarding_headers_dont_dodge_the_throttle() {
    // No proxy configured: X-Forwarded-For is ignored, so changing it on
    // every try doesn't make each one look like a new client.
    let mut app = TestApp::new();
    assert_eq!(
        sixth_attempt(&mut app, "x-forwarded-for").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Behind a configured proxy, its header names the client.
    let mut app = TestApp::with_config(|c| c.client_ip_header = Some("cf-connecting-ip".into()));
    assert_eq!(
        sixth_attempt(&mut app, "cf-connecting-ip").await,
        StatusCode::UNAUTHORIZED,
        "different clients aren't throttled together"
    );
    assert_eq!(
        sixth_attempt(&mut app, "x-forwarded-for").await,
        StatusCode::TOO_MANY_REQUESTS,
        "other headers still don't count"
    );
}

#[test]
fn the_client_address_comes_from_the_configured_header_only() {
    use axum::http::HeaderMap;
    use localapp_server::routes::signin::client_ip;
    let peer = Some("10.0.0.5:4000".parse().unwrap());
    let mut h = HeaderMap::new();
    h.insert("x-forwarded-for", "1.2.3.4, 172.18.0.2".parse().unwrap());
    assert_eq!(client_ip(None, &h, peer), "10.0.0.5");
    assert_eq!(client_ip(Some("x-forwarded-for"), &h, peer), "172.18.0.2");
    assert_eq!(client_ip(Some("cf-connecting-ip"), &h, peer), "unknown");
}

#[tokio::test]
async fn big_requests_are_refused() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let big = "x".repeat(localapp_server::routes::MAX_BODY + 1);
    let r = app.post(&url("/teams"), &[("name", &big)]).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn printing_is_limited_per_user() {
    let mut app = TestApp::new();
    let me = app.with_admin().await;
    let r = app.post(&url("/teams"), &[("name", "Hawks")]).await;
    let team = r.redirect().rsplit('/').next().unwrap().to_string();
    let r = app
        .post(
            &url("/practices"),
            &[("team_id", &team), ("date", "2026-12-01"), ("ice", "half")],
        )
        .await;
    let practice = r.redirect().rsplit('/').next().unwrap().to_string();
    app.post(
        &url(&format!("/practices/{practice}/segments")),
        &[("minutes", "10"), ("left", "h1")],
    )
    .await;
    let pdf = url(&format!("/practices/{practice}/plan.pdf"));
    assert_eq!(app.get(&pdf).await.status, StatusCode::OK);
    // Use up this minute's allowance.
    let now = std::time::Instant::now();
    while app.st.renders.allow(me, now) {}
    assert_eq!(app.get(&pdf).await.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn https_sites_ask_browsers_to_stay_on_https() {
    use tower::ServiceExt;
    let get = |app: &TestApp| {
        localapp_server::router(app.st.clone()).oneshot(
            axum::http::Request::get(url("/login"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
    };
    let plain = TestApp::new();
    let resp = get(&plain).await.unwrap();
    assert!(!resp.headers().contains_key("strict-transport-security"));
    assert!(
        resp.headers()["permissions-policy"]
            .to_str()
            .unwrap()
            .contains("camera=()")
    );
    assert_eq!(resp.headers()["cross-origin-opener-policy"], "same-origin");
    let secure = TestApp::with_config(|c| c.secure_cookies = true);
    let resp = get(&secure).await.unwrap();
    assert_eq!(
        resp.headers()["strict-transport-security"],
        "max-age=31536000"
    );
}
