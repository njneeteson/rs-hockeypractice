//! Who may see and change what: teams, their practices and drill libraries.
//! Everything that isn't yours is "404 Not Found", exactly like something
//! that doesn't exist.

mod common;

use axum::http::StatusCode;
use common::{TestApp, url};
use localapp_core::owner::{Library, MemberId};
use localapp_server::auth::store::Role;

fn id_from(location: &str, prefix: &str) -> i64 {
    let rest = location.split(prefix).nth(1).expect(location);
    rest.split(['/', '#', '?'])
        .next()
        .unwrap()
        .parse()
        .expect(location)
}

const DIAGRAM: &str = r#"{"items":[{"t":"cone","x":30,"y":30}]}"#;

async fn new_drill(app: &mut TestApp, lib: &str, name: &str) -> i64 {
    let r = app
        .post(
            &url("/drills"),
            &[
                ("lib", lib),
                ("name", name),
                ("category", "passing"),
                ("ice", "half"),
                ("minutes", "8"),
                ("description", "Pass and follow."),
                ("points", ""),
                ("diagram", DIAGRAM),
                ("focus_technical", "100"),
            ],
        )
        .await;
    id_from(r.redirect(), "/drills/")
}

/// One user's team, with a practice, a part and a drill placed in it.
struct Setup {
    team: i64,
    practice: i64,
    seg: i64,
    personal: i64,
    team_drill: i64,
}

async fn setup(app: &mut TestApp, team_name: &str) -> Setup {
    let r = app.post(&url("/teams"), &[("name", team_name)]).await;
    let team = id_from(r.redirect(), "/teams/");
    let r = app
        .post(
            &url("/practices"),
            &[
                ("team_id", &team.to_string()),
                ("date", "2026-12-01"),
                ("ice", "half"),
            ],
        )
        .await;
    let practice = id_from(r.redirect(), "/practices/");
    let r = app
        .post(
            &url(&format!("/practices/{practice}/segments")),
            &[("minutes", "10"), ("left", "h1")],
        )
        .await;
    let seg = id_from(r.redirect(), "#seg-");
    let personal = new_drill(app, "me", &format!("{team_name} personal")).await;
    let team_drill = new_drill(
        app,
        &Library::Team(team).key(),
        &format!("{team_name} team drill"),
    )
    .await;
    app.post(
        &url(&format!("/segments/{seg}/stations/0")),
        &[("drill_id", &team_drill.to_string())],
    )
    .await;
    Setup {
        team,
        practice,
        seg,
        personal,
        team_drill,
    }
}

#[tokio::test]
async fn other_peoples_things_are_not_found() {
    let mut alice = TestApp::new();
    alice.with_admin().await;
    let a = setup(&mut alice, "Ravens").await;
    alice.create_user("bob", Role::User);
    let mut bob = alice.client();
    bob.login("bob").await;
    let b = setup(&mut bob, "Eagles").await;

    let (t, p, s) = (a.team, a.practice, a.seg);
    let team_lib = Library::Team(t).key();
    let gets = [
        format!("/teams/{t}"),
        format!("/practices/{p}"),
        format!("/practices/{p}/edit"),
        format!("/practices/{p}/plan.pdf"),
        format!("/practices/{p}/preview/{s}"),
        format!("/drills/{}", a.personal),
        format!("/drills/{}", a.team_drill),
        format!("/drills?lib={team_lib}"),
        format!("/drills/new?lib={team_lib}"),
    ];
    for path in &gets {
        let r = bob.get(&url(path)).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "GET {path}");
    }
    let posts: Vec<(String, Vec<(&str, String)>)> = vec![
        (format!("/teams/{t}"), vec![("name", "Mine now".into())]),
        (format!("/teams/{t}/delete"), vec![]),
        (
            format!("/teams/{t}/members"),
            vec![("username", "bob".into()), ("role", "owner".into())],
        ),
        (
            format!("/teams/{t}/members/1/role"),
            vec![("role", "coach".into())],
        ),
        (format!("/teams/{t}/members/1/remove"), vec![]),
        (format!("/teams/{t}/leave"), vec![]),
        (
            format!("/teams/{t}/development"),
            vec![("age", "u7".into()), ("targets", "hockey_canada".into())],
        ),
        (
            format!("/practices/{p}"),
            vec![("date", "2026-12-02".into()), ("ice", "half".into())],
        ),
        (format!("/practices/{p}/delete"), vec![]),
        (
            format!("/practices/{p}/duplicate"),
            vec![("date", "2026-12-08".into())],
        ),
        (
            format!("/practices/{p}/segments"),
            vec![("minutes", "5".into()), ("left", "h1".into())],
        ),
        (
            format!("/segments/{s}"),
            vec![("minutes", "5".into()), ("left", "h1".into())],
        ),
        (format!("/segments/{s}/move"), vec![("dir", "up".into())]),
        (format!("/segments/{s}/duplicate"), vec![]),
        (format!("/segments/{s}/delete"), vec![]),
        (
            format!("/segments/{s}/stations/0"),
            vec![("drill_id", b.team_drill.to_string())],
        ),
        (
            format!("/segments/{s}/stations/0/note"),
            vec![("note", "x".into())],
        ),
        (
            format!("/segments/{s}/stations/0/flip"),
            vec![("flip", "1".into())],
        ),
        (format!("/segments/{s}/stations/0/clear"), vec![]),
        (
            format!("/segments/{s}/goalies"),
            vec![("spot", "none".into())],
        ),
        (
            format!("/drills/{}", a.personal),
            vec![("name", "Mine".into())],
        ),
        (format!("/drills/{}/duplicate", a.personal), vec![]),
        (
            format!("/drills/{}/copy", a.personal),
            vec![("to", "me".into())],
        ),
        (format!("/drills/{}/delete", a.team_drill), vec![]),
        ("/drills/samples".into(), vec![("lib", team_lib.clone())]),
        (
            "/drills".into(),
            vec![
                ("lib", team_lib.clone()),
                ("name", "Sneaky".into()),
                ("category", "passing".into()),
                ("ice", "half".into()),
                ("minutes", "5".into()),
                ("diagram", DIAGRAM.into()),
            ],
        ),
    ];
    for (path, form) in &posts {
        let form: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let r = bob.post(&url(path), &form).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "POST {path}: {}", r.body);
    }

    // Bob can't use Alice's things from his own pages either.
    let r = bob
        .post(
            &url(&format!("/segments/{}/stations/1", b.seg)),
            &[("drill_id", &a.personal.to_string())],
        )
        .await;
    assert!(bob.get(r.redirect()).await.body.contains("doesn't exist"));
    let r = bob
        .post(
            &url(&format!("/drills/{}/copy", b.personal)),
            &[("to", &team_lib)],
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "copying into her team");
    let r = bob
        .post(
            &url("/practices"),
            &[
                ("team_id", &t.to_string()),
                ("date", "2026-12-01"),
                ("ice", "half"),
            ],
        )
        .await;
    assert!(
        bob.get(r.redirect())
            .await
            .body
            .contains("Choose one of your teams")
    );
    let r = bob
        .post(
            &url(&format!("/practices/{}", b.practice)),
            &[
                ("team_id", &t.to_string()),
                ("date", "2026-12-01"),
                ("ice", "half"),
            ],
        )
        .await;
    assert!(
        bob.get(r.redirect())
            .await
            .body
            .contains("Choose one of your teams")
    );

    // Nothing of Alice's shows in Bob's lists, and nothing changed.
    for path in ["/", "/teams", "/practices", "/drills", "/practices/new"] {
        let body = bob.get(&url(path)).await.body;
        assert!(!body.contains("Ravens"), "{path} shows Alice's team");
    }
    let page = alice.get(&url(&format!("/practices/{p}"))).await.body;
    assert!(page.contains("Ravens team drill"));
    assert!(!page.contains("Mine now"));

    // Being an admin gives no access to other people's teams either.
    let r = alice.get(&url(&format!("/practices/{}", b.practice))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = alice.get(&url(&format!("/drills/{}", b.personal))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn team_members_plan_together() {
    let mut alice = TestApp::new();
    alice.with_admin().await;
    let a = setup(&mut alice, "Ravens").await;
    let bob_id = alice.create_user("bob", Role::User);
    let mut bob = alice.client();
    bob.login("bob").await;
    let t = a.team;

    // Alice adds Bob as a coach (an unknown name is refused).
    let add = url(&format!("/teams/{t}/members"));
    let r = alice
        .post(&add, &[("username", "nobody"), ("role", "coach")])
        .await;
    assert!(
        alice
            .get(r.redirect())
            .await
            .body
            .contains("no account called")
    );
    let r = alice
        .post(&add, &[("username", "bob"), ("role", "coach")])
        .await;
    assert!(alice.get(r.redirect()).await.body.contains("Added bob."));

    // Bob sees the practice and edits the team's drills…
    let page = bob.get(&url(&format!("/practices/{}", a.practice))).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Ravens team drill"));
    let r = bob
        .post(
            &url(&format!("/drills/{}", a.team_drill)),
            &[
                ("name", "Ravens team drill v2"),
                ("category", "passing"),
                ("ice", "half"),
                ("minutes", "8"),
                ("diagram", DIAGRAM),
                ("focus_technical", "100"),
            ],
        )
        .await;
    assert!(r.status.is_redirection(), "{}", r.body);
    // …but not Alice's own drills, and he can't manage the team.
    let r = bob.get(&url(&format!("/drills/{}", a.personal))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    for (path, form) in [
        (format!("/teams/{t}"), vec![("name", "Bob's")]),
        (format!("/teams/{t}/delete"), vec![]),
        (
            format!("/teams/{t}/members"),
            vec![("username", "admin"), ("role", "coach")],
        ),
        (
            format!("/teams/{t}/members/{bob_id}/role"),
            vec![("role", "owner")],
        ),
    ] {
        let r = bob.post(&url(&path), &form).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "POST {path}");
    }

    // Bob places one of his own drills: the team gets a copy, and Bob's
    // later edits to his drill don't change the practice.
    let mine = new_drill(&mut bob, "me", "Bob's breakout").await;
    bob.post(
        &url(&format!("/segments/{}/stations/0", a.seg)),
        &[("drill_id", &mine.to_string())],
    )
    .await;
    bob.post(
        &url(&format!("/drills/{mine}")),
        &[
            ("name", "Bob's breakout, changed"),
            ("category", "passing"),
            ("ice", "half"),
            ("minutes", "8"),
            ("diagram", DIAGRAM),
            ("focus_technical", "100"),
        ],
    )
    .await;
    let page = alice
        .get(&url(&format!("/practices/{}", a.practice)))
        .await
        .body;
    assert!(page.contains("Bob&#x27;s breakout") || page.contains("Bob's breakout"));
    assert!(!page.contains("breakout, changed"));
    let team_drills = alice
        .st
        .db(move |db| db.drill_count(Library::Team(t)))
        .await
        .unwrap();
    assert_eq!(team_drills, 2);

    // Copying a team drill to his own library makes Bob his own version.
    let r = bob
        .post(
            &url(&format!("/drills/{}/copy", a.team_drill)),
            &[("to", "me")],
        )
        .await;
    let copy = id_from(r.redirect(), "/drills/");
    let owner = alice
        .st
        .db(move |db| Ok(db.drill(copy)?.unwrap().library))
        .await
        .unwrap();
    assert_eq!(owner, Library::Personal(MemberId(bob_id)));

    // The last owner can't leave; Bob, a coach, can.
    let r = alice.post(&url(&format!("/teams/{t}/leave")), &[]).await;
    assert!(
        alice
            .get(r.redirect())
            .await
            .body
            .contains("needs an owner")
    );
    let r = bob.post(&url(&format!("/teams/{t}/leave")), &[]).await;
    assert!(
        bob.get(r.redirect())
            .await
            .body
            .contains("You left Ravens.")
    );
    let r = bob.get(&url(&format!("/practices/{}", a.practice))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Deleting an account that is a team's only owner is refused; others go,
    // with their own drills.
    let carol = alice.create_user("carol", Role::User);
    let mut c = alice.client();
    c.login("carol").await;
    c.post(&url("/teams"), &[("name", "Owls")]).await;
    let r = alice
        .post(&url(&format!("/admin/users/{carol}/delete")), &[])
        .await;
    assert!(
        alice
            .get(r.redirect())
            .await
            .body
            .contains("only owner of Owls")
    );
    let r = alice
        .post(&url(&format!("/admin/users/{bob_id}/delete")), &[])
        .await;
    assert!(r.status.is_redirection());
    let gone = alice
        .st
        .db(move |db| Ok(db.drill(mine)?.is_none()))
        .await
        .unwrap();
    assert!(gone, "Bob's own drills go with his account");
}
