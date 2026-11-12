//! Admin pages. Every handler here takes `AdminCtx`, so non-admins get 403.

mod users;

use crate::state::Shared;
use axum::Router;

pub fn routes() -> Router<Shared> {
    Router::new().merge(users::routes())
}
