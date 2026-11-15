//! Request extractors that enforce who may see and change what.
//!
//! * [`Ctx`]: a signed-in user (every page behind sign-in).
//! * [`AdminCtx`]: an admin (403 otherwise). Admins manage accounts; they
//!   get no extra access to teams' data.
//! * [`TeamCtx`], [`TeamOwnerCtx`]: a team (path `{id}`) the user belongs to,
//!   or owns.
//! * [`PracticeCtx`]: a practice (path `{id}`) of one of the user's teams.
//! * [`SegmentCtx`]: a part (path `{sid}`) of such a practice.
//! * [`DrillCtx`]: a drill (path `{id}`) in the user's own library or one of
//!   their teams'.
//!
//! A handler states its requirement by the extractor it takes, so the check
//! can't be forgotten: every route with an id in its path takes one of these.
//! Something that exists but isn't yours is "404 Not Found", exactly like
//! something that doesn't exist, so ids can't be probed.

use crate::auth::CurrentUser;
use crate::auth::store::User;
use crate::error::{AppError, AppResult};
use crate::flash::{Flash, FlashSlot};
use crate::state::Shared;
use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use localapp_core::db::Database;
use localapp_core::drill::Drill;
use localapp_core::owner::{MemberId, TeamRole};
use localapp_core::practice::{Practice, Segment, Team};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone)]
pub struct Ctx {
    pub st: Shared,
    pub user: Arc<User>,
    /// Hash of this request's session token.
    pub session: Arc<str>,
    flash: FlashSlot,
}

impl Ctx {
    /// URL under the base path.
    pub fn url(&self, path: &str) -> String {
        self.st.url(path)
    }

    /// Run blocking work against the app database (see [`crate::AppState::db`]).
    pub async fn db<T, F>(&self, f: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> anyhow::Result<T> + Send + 'static,
    {
        self.st.db(f).await
    }

    /// The signed-in user as the core knows them (owner of drills, team member).
    pub fn member(&self) -> MemberId {
        MemberId(self.user.id)
    }

    /// The pending flash message (marks it as shown). Call once, from the layout.
    pub fn take_flash(&self) -> Option<Flash> {
        self.flash.take()
    }
}

impl FromRequestParts<Shared> for Ctx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> AppResult<Self> {
        let cu = parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .ok_or(AppError::Forbidden("Please sign in."))?;
        let flash = parts
            .extensions
            .get::<FlashSlot>()
            .cloned()
            .unwrap_or_default();
        Ok(Self {
            st: st.clone(),
            user: cu.user,
            session: cu.session,
            flash,
        })
    }
}

#[derive(Clone)]
pub struct AdminCtx(pub Ctx);

impl Deref for AdminCtx {
    type Target = Ctx;
    fn deref(&self) -> &Ctx {
        &self.0
    }
}

impl FromRequestParts<Shared> for AdminCtx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> AppResult<Self> {
        let cx = Ctx::from_request_parts(parts, st).await?;
        if !cx.user.is_admin() {
            return Err(AppError::Forbidden("Only admins can manage users."));
        }
        Ok(Self(cx))
    }
}

/// A numeric path parameter by name; anything else is "not found".
async fn path_id(parts: &mut Parts, st: &Shared, name: &str) -> AppResult<i64> {
    let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, st)
        .await
        .map_err(|_| AppError::NotFound)?;
    params
        .get(name)
        .and_then(|v| v.parse().ok())
        .ok_or(AppError::NotFound)
}

/// Implements `Deref<Target = Ctx>` for an extractor with a `cx` field.
macro_rules! deref_ctx {
    ($t:ty) => {
        impl Deref for $t {
            type Target = Ctx;
            fn deref(&self) -> &Ctx {
                &self.cx
            }
        }
    };
}

/// A team the signed-in user belongs to (path `{id}`).
#[derive(Clone)]
pub struct TeamCtx {
    pub cx: Ctx,
    pub team: Team,
    pub role: TeamRole,
}
deref_ctx!(TeamCtx);

impl FromRequestParts<Shared> for TeamCtx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> AppResult<Self> {
        let cx = Ctx::from_request_parts(parts, st).await?;
        let id = path_id(parts, st, "id").await?;
        let me = cx.member();
        let found = cx
            .db(move |db| Ok(db.member_role(id, me)?.zip(db.team(id)?)))
            .await?;
        let (role, team) = found.ok_or(AppError::NotFound)?;
        Ok(Self { cx, team, role })
    }
}

/// A team the signed-in user owns (path `{id}`); members who aren't owners
/// get "403 Not allowed".
#[derive(Clone)]
pub struct TeamOwnerCtx(pub TeamCtx);

impl Deref for TeamOwnerCtx {
    type Target = TeamCtx;
    fn deref(&self) -> &TeamCtx {
        &self.0
    }
}

impl FromRequestParts<Shared> for TeamOwnerCtx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> AppResult<Self> {
        let t = TeamCtx::from_request_parts(parts, st).await?;
        if t.role != TeamRole::Owner {
            return Err(AppError::Forbidden("Only the team's owners can do that."));
        }
        Ok(Self(t))
    }
}

/// A practice of one of the signed-in user's teams (path `{id}`).
#[derive(Clone)]
pub struct PracticeCtx {
    pub cx: Ctx,
    pub practice: Practice,
}
deref_ctx!(PracticeCtx);

impl FromRequestParts<Shared> for PracticeCtx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> AppResult<Self> {
        let cx = Ctx::from_request_parts(parts, st).await?;
        let id = path_id(parts, st, "id").await?;
        let me = cx.member();
        let practice = cx
            .db(move |db| {
                let Some(p) = db.practice(id)? else {
                    return Ok(None);
                };
                Ok(db.member_role(p.team_id, me)?.map(|_| p))
            })
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(Self { cx, practice })
    }
}

/// A part of a practice of one of the signed-in user's teams (path `{sid}`).
#[derive(Clone)]
pub struct SegmentCtx {
    pub cx: Ctx,
    pub seg: Segment,
    pub team: i64,
}
deref_ctx!(SegmentCtx);

impl FromRequestParts<Shared> for SegmentCtx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> AppResult<Self> {
        let cx = Ctx::from_request_parts(parts, st).await?;
        let sid = path_id(parts, st, "sid").await?;
        let me = cx.member();
        let (seg, team) = cx
            .db(move |db| {
                let (Some(seg), Some(team)) = (db.segment(sid)?, db.segment_team(sid)?) else {
                    return Ok(None);
                };
                Ok(db.member_role(team, me)?.map(|_| (seg, team)))
            })
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(Self { cx, seg, team })
    }
}

/// A drill in the signed-in user's own library or one of their teams'
/// (path `{id}`). Whoever may see a drill may also change it: a personal
/// library is its owner's alone, and a team's is shared by its members.
#[derive(Clone)]
pub struct DrillCtx {
    pub cx: Ctx,
    pub drill: Drill,
}
deref_ctx!(DrillCtx);

impl FromRequestParts<Shared> for DrillCtx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, st: &Shared) -> AppResult<Self> {
        let cx = Ctx::from_request_parts(parts, st).await?;
        let id = path_id(parts, st, "id").await?;
        let me = cx.member();
        let drill = cx
            .db(move |db| {
                let Some(d) = db.drill(id)? else {
                    return Ok(None);
                };
                Ok(db.can_use_library(me, d.library)?.then_some(d))
            })
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(Self { cx, drill })
    }
}
