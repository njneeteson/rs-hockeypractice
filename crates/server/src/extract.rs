//! Request extractors that enforce who may see and change what.
//!
//! * [`Ctx`]: a signed-in user (every page behind sign-in).
//! * [`AdminCtx`]: an admin (403 otherwise).
//!
//! A handler states its requirement by the extractor it takes, so the check
//! can't be forgotten. Add more extractors the same way when the app needs them.

use crate::auth::CurrentUser;
use crate::auth::store::User;
use crate::error::{AppError, AppResult};
use crate::flash::{Flash, FlashSlot};
use crate::state::Shared;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use localapp_core::db::Database;
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
