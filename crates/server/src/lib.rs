//! LocalApp server: the app as a web interface for a home network.
//!
//! * [`auth`]: local accounts, sessions, sign-in throttling, cross-site protection.
//! * [`extract`]: who may see and change what, checked on every request.
//! * [`routes`]: the pages; [`views`]: shared HTML building blocks.
//!
//! App logic and storage come from `localapp-core`.

pub mod assets;
pub mod auth;
pub mod cli;
pub mod config;
pub mod error;
pub mod extract;
pub mod flash;
pub mod form;
pub mod htmx;
pub mod routes;
pub mod state;
pub mod views;

pub use config::Config;
pub use routes::router;
pub use state::{AppState, Shared};
