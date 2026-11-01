//! Washoi, the Rust counterpart of wash-fps, on the SHAGGER engine.
//!
//! Module folders follow wash-fps (`simulation`, `client`, `world`, ...), so a
//! wash-fps path points at its Rust file. The plan is `docs/research/RUST.md`.

pub use shagger;

pub mod liquid;
pub mod shared;
pub mod simulation;
pub mod world;

/// The client's version, as shown in the window title and sent nowhere yet.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
