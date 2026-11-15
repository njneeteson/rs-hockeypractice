//! Hockey Canada's development model: drill splits, team age divisions and
//! targets, the builder's comparison, season totals and the guide.

mod common;

use axum::http::StatusCode;
use common::{TestApp, url};
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

async fn drill(app: &mut TestApp, name: &str, split: &[(&str, &str)]) -> axum::http::StatusCode {
    let mut form = vec![
        ("name", name),
        ("category", "passing"),
        ("ice", "quarter"),
        ("minutes", "8"),
        ("diagram", DIAGRAM),
    ];
    form.extend_from_slice(split);
    app.post(&url("/drills"), &form).await.status
}

async fn drill_id(app: &mut TestApp, name: &str, split: &[(&str, &str)]) -> i64 {
    let mut form = vec![
        ("name", name),
        ("category", "passing"),
        ("ice", "quarter"),
        ("minutes", "8"),
        ("diagram", DIAGRAM),
    ];
    form.extend_from_slice(split);
    let r = app.post(&url("/drills"), &form).await;
    id_from(r.redirect(), "/drills/")
}

#[tokio::test]
async fn drills_must_say_how_their_time_splits() {
    let mut app = TestApp::new();
    app.with_admin().await;
    // Missing, not adding up to 100, or off the 5% steps: the form comes back
    // with what was typed and why.
    let r = app
        .post(
            &url("/drills"),
            &[
                ("name", "No split"),
                ("category", "passing"),
                ("ice", "quarter"),
                ("minutes", "8"),
                ("diagram", DIAGRAM),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("development levels") && r.body.contains("No split"));
    let short = [("focus_technical", "50"), ("focus_individual", "45")];
    assert_eq!(
        drill(&mut app, "95%", &short).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let steps = [("focus_technical", "52"), ("focus_individual", "48")];
    assert_eq!(
        drill(&mut app, "Odd steps", &steps).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let id = drill_id(
        &mut app,
        "2-on-1 rush",
        &[
            ("focus_technical", "40"),
            ("focus_individual", "30"),
            ("focus_team_tactics", "30"),
        ],
    )
    .await;
    let page = app.get(&url(&format!("/drills/{id}"))).await.body;
    assert!(page.contains(r#"name="focus_team_tactics""#));
    assert!(page.contains(r#"value="30""#));
    let list = app.get(&url("/drills")).await.body;
    assert!(list.contains("Team tactics 30%"), "the card's split bar");

    // Example drills come with their splits.
    app.post(&url("/drills/samples"), &[]).await;
    let list = app.get(&url("/drills")).await.body;
    assert!(!list.contains("Development split not set"));
}

#[tokio::test]
async fn practices_and_seasons_are_compared_with_the_teams_targets() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let r = app
        .post(&url("/teams"), &[("name", "Hawks"), ("age", "u11")])
        .await;
    let team = id_from(r.redirect(), "/teams/");
    let page = app.get(&url(&format!("/teams/{team}"))).await.body;
    assert!(
        page.contains("Hockey Canada&#x27;s U11 targets")
            || page.contains("Hockey Canada's U11 targets")
    );

    let skills = drill_id(&mut app, "Skills", &[("focus_technical", "100")]).await;
    let battle = drill_id(&mut app, "Battle", &[("focus_individual", "100")]).await;
    let r = app
        .post(
            &url("/practices"),
            &[
                ("team_id", &team.to_string()),
                ("date", "2020-01-07"),
                ("ice", "half"),
            ],
        )
        .await;
    let practice = id_from(r.redirect(), "/practices/");
    let r = app
        .post(
            &url(&format!("/practices/{practice}/segments")),
            &[("minutes", "20"), ("left", "h2"), ("rotation", "10")],
        )
        .await;
    let seg = id_from(r.redirect(), "#seg-");
    for (slot, d) in [(0, skills), (1, battle)] {
        app.post(
            &url(&format!("/segments/{seg}/stations/{slot}")),
            &[("drill_id", &d.to_string())],
        )
        .await;
    }

    // The builder: 50% technical against U11's 50%, 50% individual tactics
    // against 20% (above), nothing at the other levels.
    let page = app.get(&url(&format!("/practices/{practice}"))).await.body;
    let panel = page.split("id=\"practice-focus\"").nth(1).unwrap();
    let panel = panel.split("id=\"builder-main\"").next().unwrap();
    assert!(panel.contains("Development focus"));
    assert!(
        panel.contains(r#"class="focus-row on""#),
        "technical on target"
    );
    assert!(
        panel.contains(r#"class="focus-row over""#),
        "individual tactics above"
    );
    assert!(
        panel.contains(r#"class="focus-row under""#),
        "team tactics below"
    );
    assert!(panel.contains("10 min"));

    // The team page adds up the practices so far (this one is in the past).
    let page = app.get(&url(&format!("/teams/{team}"))).await.body;
    assert!(page.contains("Practices so far (1)"));
    assert!(page.contains(r#"class="focus-row over""#));

    // The owner sets the team's own targets; the comparison follows them.
    let r = app
        .post(
            &url(&format!("/teams/{team}/development")),
            &[
                ("age", "u11"),
                ("targets", "custom"),
                ("target_technical", "50"),
                ("target_individual", "50"),
                ("target_team_tactics", "0"),
                ("target_team_play", "0"),
                ("target_strategy", "0"),
            ],
        )
        .await;
    assert!(app.get(r.redirect()).await.body.contains("targets saved"));
    let page = app.get(&url(&format!("/practices/{practice}"))).await.body;
    let panel = page.split("id=\"practice-focus\"").nth(1).unwrap();
    let panel = panel.split("id=\"builder-main\"").next().unwrap();
    assert!(!panel.contains("focus-row over") && !panel.contains("focus-row under"));
    assert!(
        panel.contains("the team&#x27;s own targets") || panel.contains("the team's own targets")
    );
    // Targets that don't add up are refused.
    let r = app
        .post(
            &url(&format!("/teams/{team}/development")),
            &[
                ("age", "u11"),
                ("targets", "custom"),
                ("target_technical", "90"),
            ],
        )
        .await;
    assert!(
        app.get(r.redirect())
            .await
            .body
            .contains("Targets not saved")
    );

    // Coaches in the team see all this but can't change the targets.
    app.create_user("coach", Role::User);
    app.post(
        &url(&format!("/teams/{team}/members")),
        &[("username", "coach"), ("role", "coach")],
    )
    .await;
    let mut coach = app.client();
    coach.login("coach").await;
    let r = coach
        .post(
            &url(&format!("/teams/{team}/development")),
            &[("age", "u7"), ("targets", "hockey_canada")],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_guide_shows_every_division_and_your_teams() {
    let mut app = TestApp::new();
    app.with_admin().await;
    app.post(&url("/teams"), &[("name", "Hawks"), ("age", "u13")])
        .await;
    let page = app.get(&url("/development")).await;
    assert_eq!(page.status, StatusCode::OK);
    for id in [
        "u7",
        "u9",
        "u11",
        "u13",
        "u15",
        "u18",
        "technical",
        "strategy",
    ] {
        assert!(page.body.contains(&format!("id=\"{id}\"")), "{id}");
    }
    assert!(page.body.contains(r#"id="u13" class="mine""#));
    assert!(page.body.contains("https://www.hockeycanada.ca/"));
}
