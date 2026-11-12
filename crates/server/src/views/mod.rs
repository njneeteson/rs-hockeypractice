//! HTML building blocks (maud). Pages live in `routes`; this module holds
//! what they share.

pub mod components;
pub mod forms;
pub mod layout;
pub mod nav;

pub use layout::{Page, bare_page, page};
pub use nav::Nav;
