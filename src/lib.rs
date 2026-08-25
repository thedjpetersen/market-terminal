//! Market Terminal's reusable application and feature library.
//!
//! The native terminal host is intentionally kept at the edge: features and
//! application state can be constructed and tested without starting a TUI.

pub mod app;
pub mod bootstrap;
pub mod features;
pub mod runtime;

mod infrastructure;
mod ui;

pub use app::App;
