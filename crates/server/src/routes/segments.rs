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
use localapp_core::practice::SegmentFields;
use localapp_core::rink::Layout;
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
        .route("/segments/{sid}/stations/{slot}/clear", post(clear))
}

#[derive(Deserialize)]
struct SegmentForm {
    #[serde(default)]
    title: String,
    minutes: String,
    layout: String,
    #[serde(default)]
    rotation: String,
    #[serde(default)]
    notes: String,
}

impl SegmentForm {
    fn fields(&self) -> AppResult<SegmentFields> {
        let rotation = match self.rotation.trim() {
            "" => None,
            r => Some(form::number(r, "Rotation")?),
        };
        Ok(SegmentFields {
            title: self.title.clone(),
            minutes: form::number(&self.minutes, "Minutes")?,
            layout: form::key::<Layout>(&self.layout, "layout")?,
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
