//! The practice builder's layout and tools: parts added anywhere, moved to any
//! position, renamed in their header; every change undoable from the history;
//! the drill cards and their edit round trip; and the extras each person can
//! turn off.

mod common;

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

async fn drill(app: &mut TestApp, name: &str, split: (&str, &str)) -> i64 {
    let r = app
        .post(
            &url("/drills"),
            &[
                ("name", name),
                ("category", "passing"),
                ("ice", "quarter"),
                ("minutes", "8"),
                ("points", "Heads up\nTape to tape"),
                ("diagram", DIAGRAM),
                split,
            ],
        )
        .await;
    id_from(r.redirect(), "/drills/")
}

async fn practice(app: &mut TestApp) -> i64 {
    let r = app
        .post(
            &url("/practices"),
            &[
                ("new_team", "Hawks"),
                ("date", "2026-11-17"),
                ("ice", "half"),
            ],
        )
        .await;
    id_from(r.redirect(), "/practices/")
}

async fn add_part(app: &mut TestApp, p: i64, title: &str, at: &str) -> i64 {
    let r = app
        .post(
            &url(&format!("/practices/{p}/segments")),
            &[
                ("title", title),
                ("minutes", "10"),
                ("left", "h2"),
                ("at", at),
            ],
        )
        .await;
    id_from(r.redirect(), "#seg-")
}

/// The parts' titles in page order.
async fn titles(app: &mut TestApp, p: i64) -> Vec<String> {
    let page = app.get(&url(&format!("/practices/{p}"))).await.body;
    // The header's title field: its value comes just before its class.
    let pieces: Vec<&str> = page.split("class=\"head-title\"").collect();
    pieces[..pieces.len() - 1]
        .iter()
        .map(|s| {
            let v = s.rsplit("value=\"").next().unwrap_or("");
            v.split('"').next().unwrap().to_string()
        })
        .collect()
}

#[tokio::test]
async fn parts_go_anywhere_and_every_change_can_be_undone() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let p = practice(&mut app).await;

    // "+" at the end, at the start, and in between.
    let a = add_part(&mut app, p, "Games", "").await;
    add_part(&mut app, p, "Warm-up", "0").await;
    add_part(&mut app, p, "Skills", "1").await;
    assert_eq!(titles(&mut app, p).await, ["Warm-up", "Skills", "Games"]);

    // Dragged to the start.
    let r = app
        .post(&url(&format!("/segments/{a}/move")), &[("to", "0")])
        .await;
    assert!(app.get(r.redirect()).await.body.contains("Part moved."));
    assert_eq!(titles(&mut app, p).await, ["Games", "Warm-up", "Skills"]);

    // Renamed and lengthened in its header.
    app.post(
        &url(&format!("/segments/{a}/head")),
        &[("title", "Small games"), ("minutes", "14")],
    )
    .await;
    let page = app.get(&url(&format!("/practices/{p}"))).await.body;
    assert!(page.contains(r#"value="Small games""#) && page.contains(r#"value="14""#));

    // A drill placed, then removed: undo puts it back, and the history lists
    // what happened, newest first.
    let d = drill(&mut app, "Weave", ("focus_technical", "100")).await;
    app.post(
        &url(&format!("/segments/{a}/stations/0")),
        &[("drill_id", &d.to_string())],
    )
    .await;
    app.post(&url(&format!("/segments/{a}/stations/0/clear")), &[])
        .await;
    let page = app.get(&url(&format!("/practices/{p}"))).await.body;
    assert!(page.contains("Last: Removed “Weave”."));
    assert!(page.contains("Go back to before this"));
    let r = app.post(&url(&format!("/practices/{p}/undo")), &[]).await;
    let page = app.get(r.redirect()).await.body;
    assert!(page.contains("Undid: Removed “Weave”."));
    assert!(page.contains("used ×1"), "the drill is back in the station");

    // Back to before the move: the original order, the part keeping its id.
    let change = page
        .split("<li>")
        .find(|li| li.contains("Part moved."))
        .and_then(|li| li.split("name=\"change\" value=\"").nth(1))
        .and_then(|v| v.split('"').next())
        .expect("the move is in the history")
        .to_string();
    app.post(
        &url(&format!("/practices/{p}/restore")),
        &[("change", &change)],
    )
    .await;
    assert_eq!(titles(&mut app, p).await, ["Warm-up", "Skills", "Games"]);
    let page = app.get(&url(&format!("/practices/{p}"))).await.body;
    assert!(
        page.contains(&format!("id=\"seg-{a}\"")),
        "same part, same id"
    );

    // Another practice's change can't be restored here.
    let r = app
        .post(
            &url("/practices"),
            &[("team_id", "1"), ("date", "2026-11-24"), ("ice", "half")],
        )
        .await;
    let other = id_from(r.redirect(), "/practices/");
    let r = app
        .post(
            &url(&format!("/practices/{other}/restore")),
            &[("change", &change)],
        )
        .await;
    let page = app.get(r.redirect()).await.body;
    assert!(page.contains("in this practice") && page.contains("history"));
    assert!(titles(&mut app, other).await.is_empty());
}

#[tokio::test]
async fn the_library_cards_suggest_and_link_back_to_the_practice() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let p = practice(&mut app).await;
    // The practice's team is the first one made.
    app.post(
        &url("/teams/1/development"),
        &[("age", "u11"), ("targets", "hockey_canada")],
    )
    .await;
    let skills = drill(&mut app, "Skills", ("focus_technical", "100")).await;
    let battles = drill(&mut app, "Battles", ("focus_individual", "100")).await;
    let s = add_part(&mut app, p, "Stations", "").await;
    app.post(
        &url(&format!("/segments/{s}/stations/0")),
        &[("drill_id", &skills.to_string())],
    )
    .await;

    // All technical so far: battles bring it closer to U11's targets, more
    // skills don't.
    let page = app.get(&url(&format!("/practices/{p}"))).await.body;
    let card = |name: &str| {
        page.split("class=\"drill-chip drill-tile\"")
            .find(|c| c.contains(&format!("data-name=\"{}\"", name.to_lowercase())))
            .unwrap_or_else(|| panic!("{name}'s card"))
            .to_string()
    };
    let help = |c: &str| -> f32 {
        c.split("data-help=\"")
            .nth(1)
            .and_then(|v| v.split('"').next())
            .and_then(|v| v.parse().ok())
            .expect("a score")
    };
    assert!(help(&card("Battles")) > 0.5);
    assert!(help(&card("Skills")) <= 0.5);
    assert!(card("Skills").contains("used ×1"));
    assert!(
        card("Battles").contains("Tape to tape"),
        "key points for the preview"
    );

    // Editing from the card comes back to the practice.
    let edit = format!("/drills/{battles}?from={p}");
    assert!(card("Battles").contains(&edit.replace('&', "&amp;")));
    let form = app.get(&url(&edit)).await.body;
    assert!(form.contains(r#"name="from" value=""#) && form.contains("Back to the practice"));
    let r = app
        .post(
            &url(&format!("/drills/{battles}")),
            &[
                ("name", "Battles"),
                ("category", "passing"),
                ("ice", "quarter"),
                ("minutes", "8"),
                ("diagram", DIAGRAM),
                ("focus_individual", "100"),
                ("from", &p.to_string()),
            ],
        )
        .await;
    assert!(r.redirect().ends_with(&format!("/practices/{p}")));
}

#[tokio::test]
async fn each_person_turns_off_the_extras_they_dont_want() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let p = practice(&mut app).await;
    add_part(&mut app, p, "Warm-up", "").await;
    let page = app.get(&url(&format!("/practices/{p}"))).await.body;
    for marker in [
        "class=\"timeline\"",
        "data-sticky-summary",
        "Development focus",
        "class=\"fit-count",
    ] {
        assert!(page.contains(marker), "{marker} on by default");
    }

    // Only the timeline left on.
    let r = app
        .post(&url("/account/builder"), &[("show_timeline", "1")])
        .await;
    assert!(
        app.get(r.redirect())
            .await
            .body
            .contains("Builder settings saved.")
    );
    let page = app.get(&url(&format!("/practices/{p}"))).await.body;
    assert!(page.contains("class=\"timeline\""));
    for marker in [
        "data-sticky-summary",
        "Development focus",
        "class=\"fit-count",
    ] {
        assert!(!page.contains(marker), "{marker} off");
    }
    assert!(page.contains(r#"data-extras="timeline""#));

    // Someone else still has them all.
    app.create_user("coach", Role::User);
    let mut coach = app.client();
    coach.login("coach").await;
    let page = coach.get(&url("/account")).await.body;
    assert_eq!(page.matches("type=\"checkbox\" name=\"show_").count(), 6);
    assert_eq!(page.matches(" checked").count(), 6);
}
