//! Hockey practice planner core: everything that isn't the web server.
//!
//! * [`rink`]: rink geometry, ice sizes and how layouts split the ice.
//! * [`diagram`]: a drill's drawing (players, equipment, movement lines).
//! * [`drill`], [`practice`]: the drill library and practice plans.
//! * [`board`]: complete SVG diagrams; [`pdf`]: the printable plan.
//! * [`db`]: SQLite storage.
//!
//! The crate knows nothing about HTTP, users or sessions; `localapp-server`
//! adds those on top.

#[macro_use]
pub mod enum_macro;

pub mod board;
pub mod db;
pub mod diagram;
pub mod drill;
pub mod pdf;
pub mod practice;
pub mod rink;
pub mod samples;
pub mod svg;
