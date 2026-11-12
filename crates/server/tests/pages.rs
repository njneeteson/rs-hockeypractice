//! Every page renders, and the admin and account forms work end to end.

mod common;

use axum::http::StatusCode;
use common::{TestApp, url};
use localapp_server::auth::store::{Role, Theme};

#[tokio::test]
async fn every_page_renders() {
    let mut app = TestApp::new();
    let id = app.with_admin().await;
    for path in [
        "/",
        "/account",
        "/account/password",
        "/admin/users",
        &format!("/admin/users/{id}"),
    ] {
        let r = app.get(&url(path)).await;
        assert_eq!(r.status, StatusCode::OK, "{path}: {}", r.body);
        assert!(r.body.contains("<title>"), "{path}");
    }
    assert_eq!(
        app.get(&url("/admin/users/9999")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(app.get(&url("/nope")).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn preferences_are_saved_and_flashed() {
    let mut app = TestApp::new();
    let id = app.with_admin().await;
    let r = app
        .post(
            &url("/account"),
            &[("display_name", "Ada"), ("theme", "dark")],
        )
        .await;
    let page = app.get(r.redirect()).await.body;
    assert!(page.contains("Preferences saved."));
    assert!(page.contains(r#"data-theme="dark""#));
    // The flash message is shown once.
    assert!(
        !app.get(&url("/account"))
            .await
            .body
            .contains("Preferences saved.")
    );
    let u = app.st.auth.user(id).unwrap().unwrap();
    assert_eq!((u.display_name.as_str(), u.theme), ("Ada", Theme::Dark));

    let bad = app
        .post(&url("/account"), &[("display_name", ""), ("theme", "neon")])
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admins_manage_accounts() {
    let mut app = TestApp::new();
    let me = app.with_admin().await;
    let bob = app.create_user("bob", Role::User);

    let r = app
        .post(
            &url(&format!("/admin/users/{bob}")),
            &[
                ("username", "bobby"),
                ("display_name", "Bob"),
                ("role", "admin"),
            ],
        )
        .await;
    assert!(app.get(r.redirect()).await.body.contains("User saved."));
    assert_eq!(app.st.auth.user(bob).unwrap().unwrap().role, Role::Admin);

    // You can't demote or delete yourself through the form.
    app.post(
        &url(&format!("/admin/users/{me}")),
        &[("username", "admin"), ("role", "user"), ("disabled", "1")],
    )
    .await;
    let u = app.st.auth.user(me).unwrap().unwrap();
    assert!(u.is_admin() && !u.disabled);
    assert_eq!(
        app.post(&url(&format!("/admin/users/{me}/delete")), &[])
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let r = app
        .post(&url(&format!("/admin/users/{bob}/delete")), &[])
        .await;
    assert_eq!(r.redirect(), "/localapp/admin/users");
    assert!(app.st.auth.user(bob).unwrap().is_none());
}
