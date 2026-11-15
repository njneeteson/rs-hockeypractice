//! Changes to a practice's parts (segments) and the drills in their stations.
//! Every action redirects back to the practice builder, at the changed part.

use crate::error::{AppError, AppResult};
use crate::extract::{Ctx, PracticeCtx, SegmentCtx};
use crate::flash;
use crate::form;
use crate::state::Shared;
use axum::extract::Path;
use axum::response::Response;
use axum::routing::post;
use axum::{Form, Router};
use localapp_core::practice::{GoalieWarmup, SegmentFields};
use localapp_core::rink::{End, Layout, MAX_LANES};
use serde::Deserialize;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/practices/{id}/segments", post(add))
        .route("/segments/{sid}", post(update))
        .route("/segments/{sid}/move", post(move_segment))
        .route("/segments/{sid}/duplicate", post(duplicate))
        .route("/segments/{sid}/delete", post(delete))
        .route("/segments/{sid}/stations/{slot}", post(place))
        .route("/segments/{sid}/stations/{slot}/note", post(note))
        .route("/segments/{sid}/goalies", post(goalies))
        .route("/segments/{sid}/stations/{slot}/flip", post(flip))
        .route("/segments/{sid}/stations/{slot}/clear", post(clear))
}

#[derive(Deserialize)]
struct SegmentForm {
    #[serde(default)]
    title: String,
    minutes: String,
    /// A layout key; or else `whole`, `left` and `right` (see `layout_fields`).
    #[serde(default)]
    layout: String,
    #[serde(default)]
    whole: String,
    #[serde(default)]
    left: String,
    #[serde(default)]
    right: String,
    #[serde(default)]
    rotation: String,
    #[serde(default)]
    notes: String,
}

impl SegmentForm {
    fn layout(&self) -> AppResult<Layout> {
        let bad = |what: &str, v: &str| AppError::bad_request(format!("Unknown {what} “{v}”."));
        if !self.layout.trim().is_empty() {
            return Layout::from_key(&self.layout).ok_or_else(|| bad("layout", &self.layout));
        }
        // The end fields only count when the ice is split (the form always
        // sends them, but hides them otherwise).
        let end = |v: &str| End::from_key(v).ok_or_else(|| bad("ice split", v));
        Ok(match self.whole.trim() {
            "" => Layout::Half(end(&self.left)?),
            "ends" => Layout::Ends(end(&self.left)?, end(&self.right)?),
            n => match n.parse::<u8>() {
                Ok(n) if (1..=MAX_LANES).contains(&n) => Layout::Full(n),
                _ => return Err(bad("layout", n)),
            },
        })
    }

    fn fields(&self) -> AppResult<SegmentFields> {
        let rotation = match self.rotation.trim() {
            "" => None,
            r => Some(form::number(r, "Rotation")?),
        };
        Ok(SegmentFields {
            title: self.title.clone(),
            minutes: form::number(&self.minutes, "Minutes")?,
            layout: self.layout()?,
            rotation,
            notes: self.notes.clone(),
        })
    }
}

/// The builder page, scrolled to a part.
fn builder(cx: &Ctx, practice_id: i64, segment_id: Option<i64>) -> String {
    match segment_id {
        Some(s) => cx.url(&format!("/practices/{practice_id}#seg-{s}")),
        None => cx.url(&format!("/practices/{practice_id}")),
    }
}

async fn add(pc: PracticeCtx, Form(f): Form<SegmentForm>) -> AppResult<Response> {
    let (cx, id) = (&pc.cx, pc.practice.id);
    let fields = f.fields()?;
    match cx.db(move |db| db.add_segment(id, fields)).await {
        Ok(sid) => Ok(flash::info(
            &cx.st,
            &builder(cx, id, Some(sid)),
            "Part added.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &builder(cx, id, None),
            format!("{e:#}"),
        )),
    }
}

async fn update(sc: SegmentCtx, Form(f): Form<SegmentForm>) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let fields = f.fields()?;
    let result =
        cx.db(move |db| db.update_segment(sid, fields))
            .await
            .map(|dropped| match dropped {
                0 => "Part saved.".to_string(),
                n => {
                    format!("Part saved. {n} drill(s) didn't fit the new layout and were removed.")
                }
            });
    Ok(flash::report(&cx.st, &builder(cx, pid, Some(sid)), result))
}

#[derive(Deserialize)]
struct MoveForm {
    dir: String,
}

async fn move_segment(sc: SegmentCtx, Form(f): Form<MoveForm>) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let up = match f.dir.as_str() {
        "up" => true,
        "down" => false,
        other => {
            return Err(AppError::bad_request(format!(
                "Unknown direction “{other}”."
            )));
        }
    };
    let result = cx
        .db(move |db| db.move_segment(sid, up).map(|()| "Part moved."))
        .await;
    Ok(flash::report(&cx.st, &builder(cx, pid, Some(sid)), result))
}

async fn duplicate(sc: SegmentCtx) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    match cx.db(move |db| db.duplicate_segment(sid)).await {
        Ok(new) => Ok(flash::info(
            &cx.st,
            &builder(cx, pid, Some(new)),
            "Part copied.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &builder(cx, pid, Some(sid)),
            format!("{e:#}"),
        )),
    }
}

async fn delete(sc: SegmentCtx) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let result = cx
        .db(move |db| db.delete_segment(sid).map(|()| "Part deleted."))
        .await;
    Ok(flash::report(&cx.st, &builder(cx, pid, None), result))
}

#[derive(Deserialize)]
struct PlaceForm {
    drill_id: String,
    #[serde(default)]
    note: String,
}

async fn place(
    sc: SegmentCtx,
    Path((_, slot)): Path<(i64, usize)>,
    Form(f): Form<PlaceForm>,
) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let me = cx.member();
    let to = builder(cx, pid, Some(sid));
    let Some(drill_id) = form::opt_id(&f.drill_id) else {
        return Ok(flash::error(&cx.st, &to, "Choose a drill first."));
    };
    let result = cx
        .db(move |db| {
            // Any drill the user may use: theirs, or one of their teams'.
            let usable = match db.drill(drill_id)? {
                Some(d) => db.can_use_library(me, d.library)?,
                None => false,
            };
            anyhow::ensure!(usable, "That drill doesn't exist.");
            let placed = db.place_drill(sid, slot, drill_id, &f.note)?;
            let name = db.drill(placed)?.map(|d| d.name).unwrap_or_default();
            // Name the zone ("the right half, top lane"): station numbers
            // count only the zones with drills, so they shift as drills come and go.
            let slots = db
                .segment(sid)?
                .map(|s| s.layout.slots())
                .unwrap_or_default();
            let zone = match slots.get(slot) {
                Some(s) if slots.len() > 1 => s.name.to_lowercase(),
                _ => String::new(),
            };
            Ok(if zone.is_empty() {
                format!("Placed “{name}”.")
            } else {
                format!("Placed “{name}” in the {zone}.")
            })
        })
        .await;
    Ok(flash::report(&cx.st, &to, result))
}

#[derive(Deserialize)]
struct NoteForm {
    #[serde(default)]
    note: String,
}

async fn note(
    sc: SegmentCtx,
    Path((_, slot)): Path<(i64, usize)>,
    Form(f): Form<NoteForm>,
) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let result = cx
        .db(move |db| {
            db.set_station_note(sid, slot, &f.note)
                .map(|()| "Note saved.")
        })
        .await;
    Ok(flash::report(&cx.st, &builder(cx, pid, Some(sid)), result))
}

#[derive(Deserialize)]
struct GoalieForm {
    /// A preset key, "none", or "custom" (keep where it is).
    #[serde(default)]
    spot: String,
    /// Where it was dragged to (rink feet); both or neither.
    #[serde(default)]
    x: String,
    #[serde(default)]
    y: String,
    /// Degrees to turn it by.
    #[serde(default)]
    turn: String,
    /// The angle it faces after being dragged (degrees); with `x` and `y`.
    #[serde(default)]
    rot: String,
}

/// Place, move, turn or remove the part's goalie warm-up.
async fn goalies(sc: SegmentCtx, Form(f): Form<GoalieForm>) -> AppResult<Response> {
    let (cx, seg) = (&sc.cx, &sc.seg);
    let sid = seg.id;
    let to = builder(cx, seg.practice_id, Some(sid));
    let ice = seg.layout.ice();
    let num = |s: &str| -> AppResult<Option<f32>> {
        if s.trim().is_empty() {
            return Ok(None);
        }
        s.trim()
            .parse::<f32>()
            .map(Some)
            .map_err(|_| AppError::BadRequest("Not a number.".into()))
    };
    let current = seg.goalies;
    let (place, msg) = if let Some(t) = num(&f.turn)? {
        let Some(g) = current else {
            return Ok(flash::error(&cx.st, &to, "Place the goalie warm-up first."));
        };
        let rot = g.rot + t;
        (Some(GoalieWarmup { rot, ..g }), "Goalie warm-up turned.")
    } else if let (Some(x), Some(y)) = (num(&f.x)?, num(&f.y)?) {
        // Dragged: a new spot, at the angle it was turned to while dragging,
        // else facing the same way (into the ice for a new one).
        let same = current.map_or(if y < 42.5 { 90.0 } else { -90.0 }, |g| g.rot);
        let rot = num(&f.rot)?.unwrap_or(same);
        let msg = match current {
            Some(g) if (g.x, g.y) == (x, y) => "Goalie warm-up turned.",
            _ => "Goalie warm-up moved.",
        };
        (Some(GoalieWarmup { x, y, rot }), msg)
    } else {
        match f.spot.as_str() {
            "" | "none" => (None, "Goalie warm-up removed."),
            "custom" => (current, "Goalie warm-up kept where it is."),
            key => {
                let g = GoalieWarmup::presets(ice)
                    .into_iter()
                    .find(|(k, _, _)| *k == key)
                    .map(|(_, _, g)| g)
                    .ok_or_else(|| AppError::BadRequest("Unknown spot.".into()))?;
                (Some(g), "Goalie warm-up placed.")
            }
        }
    };
    let result = cx
        .db(move |db| db.set_goalie_warmup(sid, place).map(|()| msg))
        .await;
    Ok(flash::report(&cx.st, &to, result))
}

#[derive(Deserialize)]
struct FlipForm {
    /// "1" to flip, anything else to draw the drill as it was made.
    #[serde(default)]
    flip: String,
}

async fn flip(
    sc: SegmentCtx,
    Path((_, slot)): Path<(i64, usize)>,
    Form(f): Form<FlipForm>,
) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let on = f.flip == "1";
    let result = cx
        .db(move |db| {
            db.set_station_flip(sid, slot, on).map(|()| {
                if on {
                    "Drill flipped top to bottom."
                } else {
                    "Drill shown as drawn."
                }
            })
        })
        .await;
    Ok(flash::report(&cx.st, &builder(cx, pid, Some(sid)), result))
}

async fn clear(sc: SegmentCtx, Path((_, slot)): Path<(i64, usize)>) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let result = cx
        .db(move |db| {
            db.clear_station(sid, slot)
                .map(|()| "Drill removed from the station.")
        })
        .await;
    Ok(flash::report(&cx.st, &builder(cx, pid, Some(sid)), result))
}
