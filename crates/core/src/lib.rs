//! Hockey practice planner core: everything that isn't the web server.
//!
//! * [`rink`]: rink geometry, ice sizes and how layouts split the ice.
//! * [`diagram`]: a drill's drawing (players, equipment, movement lines).
//! * [`drill`], [`practice`]: the drill library and practice plans.
//! * [`board`]: complete SVG diagrams; [`pdf`]: the printable plan.
//! * [`owner`]: team members and drill libraries.
//! * [`db`]: SQLite storage.
//!
//! The crate knows nothing about HTTP, accounts or sessions; `localapp-server`
//! adds those on top. It stores who owns what by the server's user id
//! ([`owner::MemberId`]), but never authenticates anyone.

#[macro_use]
pub mod enum_macro;

pub mod board;
pub mod db;
pub mod development;
pub mod diagram;
pub mod drill;
pub mod owner;
pub mod pdf;
pub mod practice;
pub mod rink;
pub mod samples;
pub mod svg;
