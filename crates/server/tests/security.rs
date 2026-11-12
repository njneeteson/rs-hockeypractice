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
