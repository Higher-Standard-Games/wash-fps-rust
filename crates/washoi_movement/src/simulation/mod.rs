//! The shared movement simulation (wash-fps `simulation`): the same code the
//! server runs, so prediction matches it.

pub mod acceleration;
pub mod knockback;
pub mod ladder;
pub mod ledge_grab;
pub mod movement_config;
pub mod movement_profile;
pub mod slide;
pub mod swim;
pub mod vec3_helpers;
pub mod wall_jump;
