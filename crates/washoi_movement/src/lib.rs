//! Washoi's movement simulation, headless: the movement profile and its parts
//! (slide, wall jump, ledge grab, swim, ladder, knockback) and liquid sampling,
//! the same code the server runs (wash-fps `simulation` and `liquid`). It
//! depends only on `shagger_shared`, so a bot or a server can predict Washoi
//! movement without the engine's GPU, window, or audio. The `washoi` client
//! re-exports it at its old paths.

pub mod liquid;
pub mod simulation;
