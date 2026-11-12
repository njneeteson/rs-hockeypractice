//! Teams, the drill library, the practice builder and the PDF, end to end
//! through plain HTML forms (as a browser without JavaScript would use them).

mod common;

use axum::http::StatusCode;
use common::{TestApp, url};

/// Pull `href="…"`/`action="…"` targets that contain `needle`.
fn links(body: &str, attr: &str, needle: &str) -> Vec<String> {
    body.split(&format!(" {attr}=\""))
        .skip(1)
        .map(|s| s.split('"').next().unwrap().replace("&amp;", "&"))
        .filter(|s| s.contains(needle))
        .collect()
}

fn id_from(location: &str, prefix: &str) -> i64 {
    let rest = location.split(prefix).nth(1).expect(location);
    rest.split(['/', '#', '?'])
        .next()
        .unwrap()
        .parse()
        .expect(location)
}

const DIAGRAM: &str = r#"{"items":[
    {"t":"player","x":20,"y":20,"label":"X"},
    {"t":"cone","x":30,"y":30},
    {"t":"line","style":"pass","end":"arrow","pts":[[20,20],[40,30]]}
]}"#;

async fn new_drill(app: &mut TestApp, name: &str, ice: &str) -> i64 {
    let r = app
        .post(
            &url("/drills"),
            &[
                ("name", name),
                ("category", "passing"),
                ("ice", ice),
                ("minutes", "8"),
                ("description", "Pass and follow your pass."),
                ("points", "Tape to tape\nHeads up"),
                ("diagram", DIAGRAM),
                ("eq_pucks", "20"),
                ("eq_cones", "4"),
                ("eq_nets", ""),
            ],
        )
        .await;
    id_from(r.redirect(), "/drills/")
}

#[tokio::test]
async fn drills_are_created_edited_and_listed() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let id = new_drill(&mut app, "Pass and follow", "quarter").await;
    let page = app.get(&url(&format!("/drills/{id}"))).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Drill saved."));
    assert!(page.body.contains("data-diagram-editor"));
    assert!(
        page.body.contains("d-cone"),
        "server-rendered preview for no-JS"
    );

    let list = app.get(&url("/drills?ice=quarter&q=follow")).await.body;
    assert!(list.contains("Pass and follow"));
    assert!(list.contains("Pucks 20 · Cones 4"));
    assert!(
        !app.get(&url("/drills?ice=full"))
            .await
            .body
            .contains("Pass and follow")
    );

    // A bad diagram is rejected with the form kept, not lost.
    let r = app
        .post(
            &url(&format!("/drills/{id}")),
            &[
                ("name", ""),
                ("category", "passing"),
                ("ice", "quarter"),
                ("minutes", "8"),
                ("diagram", DIAGRAM),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = app
        .post(
            &url(&format!("/drills/{id}")),
            &[
                ("name", "Renamed"),
                ("category", "passing"),
                ("ice", "quarter"),
                ("minutes", "500"),
                ("diagram", DIAGRAM),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("Minutes must be between"));
    assert!(r.body.contains("Renamed"), "the submitted values are kept");

    let r = app
        .post(&url(&format!("/drills/{id}/duplicate")), &[])
        .await;
    let copy = id_from(r.redirect(), "/drills/");
    assert!(
        app.get(&url(&format!("/drills/{copy}")))
            .await
            .body
            .contains("Pass and follow (copy)")
    );
    let r = app.post(&url(&format!("/drills/{copy}/delete")), &[]).await;
    assert_eq!(r.redirect(), "/localapp/drills");
    assert_eq!(
        app.get(&url(&format!("/drills/{copy}"))).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn sample_drills_fill_an_empty_library() {
    let mut app = TestApp::new();
    app.with_admin().await;
    assert!(
        app.get(&url("/drills"))
            .await
            .body
            .contains("Add example drills")
    );
    let r = app.post(&url("/drills/samples"), &[]).await;
    let page = app.get(r.redirect()).await.body;
    assert!(page.contains("Added 7 example drills."));
    assert!(page.contains("1-on-1 corner battles"));
}

#[tokio::test]
async fn teams_are_managed() {
    let mut app = TestApp::new();
    app.with_admin().await;
    app.post(&url("/teams"), &[("name", "U11 Hawks"), ("notes", "")])
        .await;
    let dup = app.post(&url("/teams"), &[("name", "u11 hawks")]).await;
    assert!(
        app.get(dup.redirect())
            .await
            .body
            .contains("already a team")
    );
    let page = app.get(&url("/teams")).await.body;
    let action = links(&page, "action", "/teams/")
        .into_iter()
        .find(|a| !a.ends_with("/delete"))
        .unwrap();
    app.post(&action, &[("name", "U11 Eagles"), ("notes", "Tuesdays")])
        .await;
    let page = app.get(&url("/teams")).await.body;
    assert!(page.contains("U11 Eagles") && page.contains("Tuesdays"));
    let delete = links(&page, "action", "/delete").pop().unwrap();
    app.post(&delete, &[]).await;
    assert!(!app.get(&url("/teams")).await.body.contains("U11 Eagles"));
}

#[tokio::test]
async fn practices_are_built_and_printed() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let half = new_drill(&mut app, "Half-ice shooting", "half").await;
    let quarter = new_drill(&mut app, "Corner battles", "quarter").await;

    // No teams yet: the practice form creates one.
    assert!(
        app.get(&url("/practices/new"))
            .await
            .body
            .contains("new_team")
    );
    let r = app
        .post(
            &url("/practices"),
            &[
                ("new_team", "Riverside U13"),
                ("title", "Skills"),
                ("date", "2026-11-17"),
                ("start", "17:30"),
                ("ice", "full"),
                ("notes", ""),
            ],
        )
        .await;
    let pid = id_from(r.redirect(), "/practices/");
    let page = app.get(&url(&format!("/practices/{pid}"))).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("no parts yet"));
    assert!(!page.body.contains("plan.pdf"), "nothing to print yet");

    // Add a part split at the red line: a whole half, and a half in two lanes
    // (two quarters), as the form sends it.
    let r = app
        .post(
            &url(&format!("/practices/{pid}/segments")),
            &[
                ("title", "Stations"),
                ("minutes", "16"),
                ("whole", "ends"),
                ("left", "h1"),
                ("right", "h2"),
            ],
        )
        .await;
    let sid = id_from(r.redirect(), "#seg-");
    let page = app.get(&url(&format!("/practices/{pid}"))).await.body;
    assert!(page.contains("Part added."));
    assert_eq!(page.matches("class=\"drop-slot\"").count(), 3);

    // Place drills with the no-JS forms; a size mismatch is refused.
    let place = |slot: usize| url(&format!("/segments/{sid}/stations/{slot}"));
    let r = app
        .post(&place(1), &[("drill_id", &half.to_string())])
        .await;
    assert!(app.get(r.redirect()).await.body.contains("t fit in the "));
    // Smaller drills fit bigger stations: the whole half takes quarter drills.
    let slot0 = page
        .split("data-slot=\"0\"")
        .nth(1)
        .and_then(|s| s.split('>').next())
        .unwrap()
        .to_string();
    assert!(slot0.contains("data-accepts=\"half "), "{slot0}");
    assert!(slot0.contains(" half-2 "), "{slot0}");
    app.post(&place(0), &[("drill_id", &half.to_string())])
        .await;
    let r = app
        .post(&place(2), &[("drill_id", &quarter.to_string())])
        .await;
    assert!(
        app.get(r.redirect())
            .await
            .body
            .contains("Placed “Corner battles” in station 3.")
    );
    app.post(&format!("{}/note", place(2)), &[("note", "Coach Pat")])
        .await;

    // Flip the quarter drill top to bottom, and back.
    let before = app.get(&url(&format!("/practices/{pid}"))).await.body;
    assert!(before.contains("⇅ Flip top/bottom"));
    let r = app
        .post(&format!("{}/flip", place(2)), &[("flip", "1")])
        .await;
    let flipped = app.get(r.redirect()).await.body;
    assert!(flipped.contains("Drill flipped top to bottom."));
    assert!(flipped.contains("⇅ Flipped: undo"));
    let board = |page: &str| page.split("board-wrap").nth(1).unwrap_or("").to_string();
    assert_ne!(board(&before), board(&flipped), "the drawing changes");
    let r = app
        .post(&format!("{}/flip", place(2)), &[("flip", "0")])
        .await;
    let back = app.get(r.redirect()).await.body;
    assert!(back.contains("Drill shown as drawn."));
    let r = app
        .post(&format!("{}/flip", place(3)), &[("flip", "1")])
        .await;
    assert!(
        app.get(r.redirect())
            .await
            .body
            .contains("before flipping it")
    );

    // Rotation and strip notes.
    app.post(
        &url(&format!("/segments/{sid}")),
        &[
            ("title", "Stations"),
            ("minutes", "16"),
            ("layout", "ends-h1-h2"),
            ("rotation", "8"),
            ("notes", "Whistle to rotate"),
        ],
    )
    .await;
    let page = app.get(&url(&format!("/practices/{pid}"))).await.body;
    assert!(page.contains("Rotate every 8 min"));
    assert!(
        page.contains("board-legend"),
        "a legend for the drills on the ice"
    );
    assert!(page.contains("Left: whole half · Right: half in 2 lanes"));
    assert!(page.contains("Coach Pat"));
    assert!(page.contains("5:30–5:46 PM"));
    assert!(
        page.contains("Pucks 40"),
        "equipment adds up across stations"
    );

    // A drill in use can't be deleted or change size.
    let r = app
        .post(&url(&format!("/drills/{quarter}/delete")), &[])
        .await;
    assert!(
        app.get(r.redirect())
            .await
            .body
            .contains("used on 1 practice page")
    );

    // Second part, then reorder and copy.
    let r = app
        .post(
            &url(&format!("/practices/{pid}/segments")),
            &[
                ("title", "Scrimmage"),
                ("minutes", "10"),
                ("layout", "full"),
            ],
        )
        .await;
    let sid2 = id_from(r.redirect(), "#seg-");
    app.post(&url(&format!("/segments/{sid2}/move")), &[("dir", "up")])
        .await;
    let page = app.get(&url(&format!("/practices/{pid}"))).await.body;
    assert!(page.find("Scrimmage").unwrap() < page.find("Stations").unwrap());
    app.post(&url(&format!("/segments/{sid}/duplicate")), &[])
        .await;

    // The PDF: one page per part.
    let pdf = app.get(&url(&format!("/practices/{pid}/plan.pdf"))).await;
    assert_eq!(pdf.status, StatusCode::OK);
    assert!(pdf.body.starts_with("%PDF"));

    // Copy the practice to another date, then delete the original.
    let r = app
        .post(
            &url(&format!("/practices/{pid}/duplicate")),
            &[("date", "2026-11-24")],
        )
        .await;
    let copy = id_from(r.redirect(), "/practices/");
    assert_ne!(copy, pid);
    let page = app.get(&url(&format!("/practices/{copy}"))).await.body;
    assert_eq!(page.matches("class=\"card segment\"").count(), 3);
    let r = app
        .post(&url(&format!("/practices/{pid}/delete")), &[])
        .await;
    assert_eq!(r.redirect(), "/localapp/practices");
    assert_eq!(
        app.get(&url(&format!("/practices/{pid}"))).await.status,
        StatusCode::NOT_FOUND
    );

    // Switching the copy to half ice resets the full-ice layouts.
    let team = app
        .st
        .db(move |db| Ok(db.practice(copy)?.unwrap().team_id))
        .await
        .unwrap();
    let r = app
        .post(
            &url(&format!("/practices/{copy}")),
            &[
                ("team_id", &team.to_string()),
                ("date", "2026-11-24"),
                ("ice", "half"),
            ],
        )
        .await;
    assert!(app.get(r.redirect()).await.body.contains("were reset"));
}

#[tokio::test]
async fn new_pages_render_and_stay_under_the_base_path() {
    let mut app = TestApp::new();
    app.with_admin().await;
    app.post(&url("/drills/samples"), &[]).await;
    app.post(
        &url("/practices"),
        &[
            ("new_team", "Hawks"),
            ("date", "2026-12-01"),
            ("ice", "half"),
        ],
    )
    .await;
    let pid = app
        .st
        .db(|db| Ok(db.practices()?[0].practice.id))
        .await
        .unwrap();
    app.post(
        &url(&format!("/practices/{pid}/segments")),
        &[("minutes", "10"), ("left", "z4")],
    )
    .await;
    let page = app.get(&url(&format!("/practices/{pid}"))).await.body;
    assert_eq!(
        page.matches("class=\"drop-slot\"").count(),
        5,
        "four end-zone lanes and the neutral zone"
    );
    for path in [
        "/".to_string(),
        "/practices".into(),
        "/practices/new".into(),
        format!("/practices/{pid}"),
        format!("/practices/{pid}/edit"),
        "/drills".into(),
        "/drills/new?ice=zone".into(),
        "/drills/1".into(),
        "/teams".into(),
    ] {
        let r = app.get(&url(&path)).await;
        assert_eq!(r.status, StatusCode::OK, "{path}: {}", r.body);
        for attr in ["href", "action", "data-assign-url"] {
            for link in links(&r.body, attr, "") {
                assert!(
                    link.starts_with("/localapp")
                        || link.starts_with('#')
                        || link.starts_with("javascript:"),
                    "{path}: {attr} {link} escapes the base path"
                );
            }
        }
    }
    // Pages of other practices and missing things are 404s.
    assert_eq!(
        app.get(&url("/practices/999")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.get(&url("/practices/999/plan.pdf")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.post(&url("/segments/999/delete"), &[]).await.status,
        StatusCode::NOT_FOUND
    );
    // Other users share the same library and practices.
    app.create_user("coach", localapp_server::auth::store::Role::User);
    let mut coach = app.client();
    coach.login("coach").await;
    assert!(
        coach
            .get(&url(&format!("/practices/{pid}")))
            .await
            .body
            .contains("Hawks")
    );
}

#[tokio::test]
async fn lane_drills_and_rotated_equipment_are_saved() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let diagram = r#"{"items":[
        {"t":"net","x":11,"y":20,"rot":30},
        {"t":"barrier","x":50,"y":10,"rot":90},
        {"t":"dangler","x":60,"y":10,"rot":45},
        {"t":"player","x":20,"y":10,"label":"G"},
        {"t":"line","style":"padslide","end":"stop","pts":[[20,10],[30,12]]}
    ]}"#;
    let r = app
        .post(
            &url("/drills"),
            &[
                ("name", "Lane work"),
                ("category", "skating"),
                ("area", "half"),
                ("lanes", "3"),
                ("minutes", "8"),
                ("diagram", diagram),
            ],
        )
        .await;
    let id = id_from(r.redirect(), "/drills/");
    let d = app.st.db(move |db| db.drill(id)).await.unwrap().unwrap();
    assert_eq!(d.ice.key(), "half-3");
    assert_eq!(d.diagram.items.len(), 5);
    let page = app.get(&url(&format!("/drills/{id}"))).await.body;
    for class in ["d-net", "d-barrier", "d-dangler", "d-ring"] {
        assert!(page.contains(class), "{class}");
    }
    // The neutral zone has no lanes.
    let r = app
        .post(
            &url("/drills"),
            &[
                ("name", "Bad"),
                ("category", "skating"),
                ("area", "neutral"),
                ("lanes", "2"),
                ("minutes", "8"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn goalie_warmup_and_page_preview() {
    let mut app = TestApp::new();
    app.with_admin().await;
    let quarter = new_drill(&mut app, "Corner battles", "quarter").await;
    let r = app
        .post(
            &url("/practices"),
            &[
                ("new_team", "Riverside U13"),
                ("date", "2026-11-18"),
                ("ice", "half"),
            ],
        )
        .await;
    let pid = id_from(r.redirect(), "/practices/");
    let r = app
        .post(
            &url(&format!("/practices/{pid}/segments")),
            &[("title", "Stations"), ("minutes", "12"), ("left", "h1")],
        )
        .await;
    let sid = id_from(r.redirect(), "#seg-");
    // A quarter-ice drill fits the whole half.
    app.post(
        &url(&format!("/segments/{sid}/stations/0")),
        &[("drill_id", &quarter.to_string())],
    )
    .await;

    let page = app.get(&url(&format!("/practices/{pid}"))).await.body;
    assert!(page.contains("Goalie warm-up"));
    assert!(!page.contains("goalie-spot"), "none until placed");
    assert!(
        page.contains("id=\"builder-main\""),
        "updated in place with htmx"
    );

    // Pick a preset spot, turn it, drag it, then remove it.
    let goalies = url(&format!("/segments/{sid}/goalies"));
    let r = app.post(&goalies, &[("spot", "left-top")]).await;
    let page = app.get(r.redirect()).await.body;
    assert!(page.contains("Goalie warm-up placed."));
    assert!(page.contains("goalie-spot") && page.contains("Goalies: "));
    assert!(page.contains("Nets 2"), "the nets count as equipment");
    let r = app.post(&goalies, &[("turn", "45")]).await;
    assert!(app.get(r.redirect()).await.body.contains("turned"));
    let r = app.post(&goalies, &[("x", "60"), ("y", "80.2")]).await;
    let page = app.get(r.redirect()).await.body;
    assert!(page.contains("Goalie warm-up moved."));
    assert!(page.contains("Where you dragged it"));
    let seg = app.st.db(move |db| db.segment(sid)).await.unwrap().unwrap();
    let g = seg.goalies.unwrap();
    assert_eq!((g.x, g.y, g.rot), (60.0, 80.0, 90.0));
    // Off the half-ice sheet: refused.
    let r = app.post(&goalies, &[("x", "150"), ("y", "40")]).await;
    assert!(app.get(r.redirect()).await.body.contains("practice's ice"));
    let r = app.post(&goalies, &[("spot", "none")]).await;
    assert!(app.get(r.redirect()).await.body.contains("removed"));

    // The printed page as an image, linked from the builder.
    let page = app.get(&url(&format!("/practices/{pid}"))).await.body;
    let src = links(&page, "src", "/preview/");
    assert_eq!(src.len(), 1);
    let img = app.get(&src[0]).await;
    assert_eq!(img.status, StatusCode::OK);
    assert!(img.body.starts_with("<svg") && !img.body.contains("<text"));
    let missing = app
        .get(&url(&format!("/practices/{pid}/preview/999")))
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}
