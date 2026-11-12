//! LocalApp core: everything that isn't the web server.
//!
//! Put the app's domain types and logic here, plus its storage ([`db`]). The
//! crate knows nothing about HTTP, users or sessions; `localapp-server` adds
//! those on top. Keeping it that way means the logic can be unit-tested on its
//! own and reused by another front end (a CLI, a desktop app).

#[macro_use]
pub mod enum_macro;

pub mod db;
