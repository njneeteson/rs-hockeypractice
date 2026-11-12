//! Changes to a practice's parts (segments) and the drills in their stations.
//! Every action redirects back to the practice builder, at the changed part.

use crate::error::{AppError, AppResult};
use crate::extract::Ctx;
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

/// The practice a segment belongs to (404 if the segment doesn't exist).
async fn practice_of(cx: &Ctx, sid: i64) -> AppResult<i64> {
    cx.db(move |db| db.segment(sid))
        .await?
        .map(|s| s.practice_id)
        .ok_or(AppError::NotFound)
}

async fn add(cx: Ctx, Path(id): Path<i64>, Form(f): Form<SegmentForm>) -> AppResult<Response> {
    let fields = f.fields()?;
    cx.db(move |db| db.practice(id))
        .await?
        .ok_or(AppError::NotFound)?;
    match cx.db(move |db| db.add_segment(id, fields)).await {
        Ok(sid) => Ok(flash::info(
            &cx.st,
            &builder(&cx, id, Some(sid)),
            "Part added.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &builder(&cx, id, None),
            format!("{e:#}"),
        )),
    }
}

async fn update(cx: Ctx, Path(sid): Path<i64>, Form(f): Form<SegmentForm>) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
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
    Ok(flash::report(&cx.st, &builder(&cx, pid, Some(sid)), result))
}

#[derive(Deserialize)]
struct MoveForm {
    dir: String,
}

async fn move_segment(
    cx: Ctx,
    Path(sid): Path<i64>,
    Form(f): Form<MoveForm>,
) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
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
    Ok(flash::report(&cx.st, &builder(&cx, pid, Some(sid)), result))
}

async fn duplicate(cx: Ctx, Path(sid): Path<i64>) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
    match cx.db(move |db| db.duplicate_segment(sid)).await {
        Ok(new) => Ok(flash::info(
            &cx.st,
            &builder(&cx, pid, Some(new)),
            "Part copied.",
        )),
        Err(e) => Ok(flash::error(
            &cx.st,
            &builder(&cx, pid, Some(sid)),
            format!("{e:#}"),
        )),
    }
}

async fn delete(cx: Ctx, Path(sid): Path<i64>) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
    let result = cx
        .db(move |db| db.delete_segment(sid).map(|()| "Part deleted."))
        .await;
    Ok(flash::report(&cx.st, &builder(&cx, pid, None), result))
}

#[derive(Deserialize)]
struct PlaceForm {
    drill_id: String,
    #[serde(default)]
    note: String,
}

async fn place(
    cx: Ctx,
    Path((sid, slot)): Path<(i64, usize)>,
    Form(f): Form<PlaceForm>,
) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
    let to = builder(&cx, pid, Some(sid));
    let Some(drill_id) = form::opt_id(&f.drill_id) else {
        return Ok(flash::error(&cx.st, &to, "Choose a drill first."));
    };
    let result = cx
        .db(move |db| {
            db.set_station(sid, slot, drill_id, &f.note)?;
            let name = db.drill(drill_id)?.map(|d| d.name).unwrap_or_default();
            Ok(format!("Placed “{name}” in station {}.", slot + 1))
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
    cx: Ctx,
    Path((sid, slot)): Path<(i64, usize)>,
    Form(f): Form<NoteForm>,
) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
    let result = cx
        .db(move |db| {
            db.set_station_note(sid, slot, &f.note)
                .map(|()| "Note saved.")
        })
        .await;
    Ok(flash::report(&cx.st, &builder(&cx, pid, Some(sid)), result))
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
}

/// Place, move, turn or remove the part's goalie warm-up.
async fn goalies(cx: Ctx, Path(sid): Path<i64>, Form(f): Form<GoalieForm>) -> AppResult<Response> {
    let seg = cx
        .db(move |db| db.segment(sid))
        .await?
        .ok_or(AppError::NotFound)?;
    let to = builder(&cx, seg.practice_id, Some(sid));
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
        // Dragged: a new spot, facing the same way (into the ice for a new one).
        let rot = current.map_or(if y < 42.5 { 90.0 } else { -90.0 }, |g| g.rot);
        (Some(GoalieWarmup { x, y, rot }), "Goalie warm-up moved.")
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
    cx: Ctx,
    Path((sid, slot)): Path<(i64, usize)>,
    Form(f): Form<FlipForm>,
) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
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
    Ok(flash::report(&cx.st, &builder(&cx, pid, Some(sid)), result))
}

async fn clear(cx: Ctx, Path((sid, slot)): Path<(i64, usize)>) -> AppResult<Response> {
    let pid = practice_of(&cx, sid).await?;
    let result = cx
        .db(move |db| {
            db.clear_station(sid, slot)
                .map(|()| "Drill removed from the station.")
        })
        .await;
    Ok(flash::report(&cx.st, &builder(&cx, pid, Some(sid)), result))
}
