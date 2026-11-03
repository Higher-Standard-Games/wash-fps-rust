//! Washoi, the Rust counterpart of wash-fps, on the SHAGGER engine.
//!
//! Module folders follow wash-fps (`simulation`, `client`, `world`, ...), so a
//! wash-fps path points at its Rust file. The plan is `docs/research/RUST.md`.

pub use shagger;

pub mod client;
pub mod console;
pub mod liquid;
pub mod server;
pub mod shared;
pub mod simulation;
pub mod world;

/// The client's version, as shown in the window title and sent nowhere yet.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the release build was made from (`WASHOI_BUILD` at build time, the
/// commit the release workflow builds), if anything set it.
pub const BUILD: Option<&str> = option_env!("WASHOI_BUILD");

/// The version with its build, as `--version` and crash reports give it.
#[must_use]
pub fn version_line() -> String {
    match BUILD {
        Some(build) => format!("{VERSION} ({build})"),
        None => VERSION.to_owned(),
    }
}

/// Where the account key, the cvars and the console history live (the
/// browser keeps them in local storage): `$XDG_CONFIG_HOME/washoi` or
/// `~/.config/washoi` on Linux, `~/Library/Application Support/Washoi` on
/// macOS, `%APPDATA%\Washoi` on Windows.
#[must_use]
pub fn config_dir() -> Option<std::path::PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
    };
    if cfg!(windows) {
        return env("APPDATA").map(|d| d.join("Washoi"));
    }
    if cfg!(target_os = "macos") {
        return env("HOME").map(|h| h.join("Library/Application Support/Washoi"));
    }
    let base = env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?;
    Some(base.join("washoi"))
}

/// Where downloaded game data lives (the browser's CacheStorage):
/// `$XDG_CACHE_HOME/washoi` or `~/.cache/washoi` on Linux,
/// `~/Library/Caches/Washoi` on macOS, `%LOCALAPPDATA%\Washoi` on Windows.
#[must_use]
pub fn cache_dir() -> Option<std::path::PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
    };
    if cfg!(windows) {
        return env("LOCALAPPDATA").map(|d| d.join("Washoi"));
    }
    if cfg!(target_os = "macos") {
        return env("HOME").map(|h| h.join("Library/Caches/Washoi"));
    }
    let base = env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))?;
    Some(base.join("washoi"))
}

/// The regions the game runs in (RUST.md 4, 20.1), the first the default.
pub const REGIONS: [&str; 2] = ["eu", "us"];

/// A region's lobby and published data.
#[must_use]
pub fn region_urls(region: &str) -> (String, String) {
    (
        format!("wss://{region}.washoi.net/lobby"),
        format!("https://{region}.washoi.net/data"),
    )
}
