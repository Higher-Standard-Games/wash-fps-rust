//! Liquid state the simulation reads (wash-fps `liquid`). Sampling lives in
//! the headless `washoi_movement` crate; the client's rendering and audio here.

pub mod client;
pub use washoi_movement::liquid::{liquid_types, liquid_volume_sampler};
