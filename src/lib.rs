//! Washoi, the Rust counterpart of wash-fps, on the SHAGGER engine.
//!
//! Module folders follow wash-fps (`simulation`, `client`, `world`, ...), so a
//! wash-fps path points at its Rust file. The plan is `docs/research/RUST.md`.

pub use shagger;

pub mod client;
pub mod console;
pub mod liquid;
pub mod shared;
pub mod simulation;
pub mod world;

/// The client's version, as shown in the window title and sent nowhere yet.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `~/.config/washoi` (or `$XDG_CONFIG_HOME/washoi`): the account key, the
/// cvars and the console history, which the browser keeps in local storage.
#[must_use]
pub fn config_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("washoi"))
}
