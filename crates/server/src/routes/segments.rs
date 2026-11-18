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
        .route("/segments/{sid}/head", post(head))
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

/// Adding a part: its fields, and where it goes (`at`, a position; empty: at the end).
#[derive(Deserialize)]
struct AddForm {
    #[serde(flatten)]
    fields: SegmentForm,
    #[serde(default)]
    at: String,
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

/// Run a change to a practice's parts, recorded in its history (for undo),
/// and report it as a toast on the builder.
async fn change<T>(
    cx: &Ctx,
    practice_id: i64,
    segment_id: Option<i64>,
    f: impl FnOnce(&localapp_core::db::Database) -> anyhow::Result<T> + Send + 'static,
) -> Response
where
    T: std::fmt::Display + Send + 'static,
{
    let me = cx.member();
    let result = cx.db(move |db| db.tracked(practice_id, me, || f(db))).await;
    flash::report(
        &cx.st,
        &builder(cx, practice_id, segment_id),
        result.map(|m| m.to_string()),
    )
}

/// A new part's id, with the message for the coach.
struct Added(i64, &'static str);

impl std::fmt::Display for Added {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}

async fn add(pc: PracticeCtx, Form(f): Form<AddForm>) -> AppResult<Response> {
    let (cx, id, me) = (&pc.cx, pc.practice.id, pc.cx.member());
    let fields = f.fields.fields()?;
    let at = match f.at.trim() {
        "" => None,
        v => Some(form::number::<usize>(v, "Position")?),
    };
    let result = cx
        .db(move |db| {
            db.tracked(id, me, || {
                db.insert_segment(id, fields, at)
                    .map(|sid| Added(sid, "Part added."))
            })
        })
        .await;
    Ok(match result {
        Ok(Added(sid, msg)) => flash::info(&cx.st, &builder(cx, id, Some(sid)), msg),
        Err(e) => flash::error(&cx.st, &builder(cx, id, None), format!("{e:#}")),
    })
}

async fn update(sc: SegmentCtx, Form(f): Form<SegmentForm>) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let fields = f.fields()?;
    Ok(change(cx, pid, Some(sid), move |db| {
        db.update_segment(sid, fields).map(|dropped| match dropped {
            0 => "Part saved.".to_string(),
            n => format!("Part saved. {n} drill(s) didn't fit the new layout and were removed."),
        })
    })
    .await)
}

#[derive(Deserialize)]
struct HeadForm {
    #[serde(default)]
    title: String,
    minutes: String,
}

/// The title and minutes, edited in the part's header.
async fn head(sc: SegmentCtx, Form(f): Form<HeadForm>) -> AppResult<Response> {
    let (cx, seg) = (&sc.cx, sc.seg.clone());
    let fields = SegmentFields {
        title: f.title,
        minutes: form::number(&f.minutes, "Minutes")?,
        layout: seg.layout,
        rotation: seg.rotation,
        notes: seg.notes.clone(),
    };
    let what = match (
        fields.title.trim() == seg.title,
        fields.minutes == seg.minutes,
    ) {
        (true, false) => format!("{}: {} min.", seg.display_title(), fields.minutes),
        (false, true) => "Part renamed.".to_string(),
        _ => "Part saved.".to_string(),
    };
    Ok(change(cx, seg.practice_id, Some(seg.id), move |db| {
        db.update_segment(seg.id, fields).map(|_| what)
    })
    .await)
}

#[derive(Deserialize)]
struct MoveForm {
    /// "up" or "down"; or else `to`.
    #[serde(default)]
    dir: String,
    /// The position to move to (0: first).
    #[serde(default)]
    to: String,
}

async fn move_segment(sc: SegmentCtx, Form(f): Form<MoveForm>) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let (up, to) = match (f.dir.as_str(), f.to.trim()) {
        ("up", _) => (Some(true), None),
        ("down", _) => (Some(false), None),
        ("", to) if !to.is_empty() => (None, Some(form::number::<usize>(to, "Position")?)),
        (other, _) => {
            return Err(AppError::bad_request(format!(
                "Unknown direction “{other}”."
            )));
        }
    };
    Ok(change(cx, pid, Some(sid), move |db| {
        match (up, to) {
            (Some(up), _) => db.move_segment(sid, up),
            (None, to) => db.move_segment_to(sid, to.unwrap_or_default()),
        }
        .map(|()| "Part moved.")
    })
    .await)
}

async fn duplicate(sc: SegmentCtx) -> AppResult<Response> {
    let (cx, sid, pid, me) = (&sc.cx, sc.seg.id, sc.seg.practice_id, sc.cx.member());
    let result = cx
        .db(move |db| {
            db.tracked(pid, me, || {
                db.duplicate_segment(sid).map(|n| Added(n, "Part copied."))
            })
        })
        .await;
    Ok(match result {
        Ok(Added(new, msg)) => flash::info(&cx.st, &builder(cx, pid, Some(new)), msg),
        Err(e) => flash::error(&cx.st, &builder(cx, pid, Some(sid)), format!("{e:#}")),
    })
}

async fn delete(sc: SegmentCtx) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let title = sc.seg.display_title();
    Ok(change(cx, pid, None, move |db| {
        db.delete_segment(sid)
            .map(|()| format!("Deleted “{title}”."))
    })
    .await)
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
    let Some(drill_id) = form::opt_id(&f.drill_id) else {
        return Ok(flash::error(
            &cx.st,
            &builder(cx, pid, Some(sid)),
            "Choose a drill first.",
        ));
    };
    Ok(change(cx, pid, Some(sid), move |db| {
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
    .await)
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
    Ok(change(cx, pid, Some(sid), move |db| {
        db.set_station_note(sid, slot, &f.note)
            .map(|()| "Note saved.")
    })
    .await)
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
    Ok(change(cx, seg.practice_id, Some(sid), move |db| {
        db.set_goalie_warmup(sid, place).map(|()| msg)
    })
    .await)
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
    Ok(change(cx, pid, Some(sid), move |db| {
        db.set_station_flip(sid, slot, on).map(|()| {
            if on {
                "Drill flipped top to bottom."
            } else {
                "Drill shown as drawn."
            }
        })
    })
    .await)
}

async fn clear(sc: SegmentCtx, Path((_, slot)): Path<(i64, usize)>) -> AppResult<Response> {
    let (cx, sid, pid) = (&sc.cx, sc.seg.id, sc.seg.practice_id);
    let name = sc.seg.station(slot).map(|st| st.drill_id);
    Ok(change(cx, pid, Some(sid), move |db| {
        let name = match name {
            Some(d) => db.drill(d)?.map(|d| d.name),
            None => None,
        };
        db.clear_station(sid, slot).map(|()| match name {
            Some(n) => format!("Removed “{n}”."),
            None => "Drill removed from the station.".to_string(),
        })
    })
    .await)
}
