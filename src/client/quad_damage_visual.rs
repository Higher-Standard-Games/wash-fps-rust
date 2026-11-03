//! Port of wash-fps `client/QuadDamageVisual.ts`: Quad Damage's colour and
//! the one breathing pulse the carrier's body and weapons share.

use shagger::rendering::glam::Vec3;
use shagger::shared::js::js_sin;

const QUAD_PULSE_HZ: f64 = 1.5;
/// `QUAD_DAMAGE_COLOR`.
pub const QUAD_DAMAGE_COLOR: Vec3 = Vec3::new(1.0, 0.28, 0.025);
pub const QUAD_EMISSIVE_MIN: f64 = 0.35;
pub const QUAD_EMISSIVE_RANGE: f64 = 0.6;
/// Player outline sizes, on `cl_player_outline_size`'s 0 to 100 scale.
pub const QUAD_PLAYER_OUTLINE_MIN: f64 = 28.0;
pub const QUAD_PLAYER_OUTLINE_RANGE: f64 = 24.0;
/// A third-person weapon's world-space outline.
pub const QUAD_WORLD_WEAPON_OUTLINE_MIN: f64 = 0.7;
pub const QUAD_WORLD_WEAPON_OUTLINE_RANGE: f64 = 0.55;
/// A first-person weapon's outline before its root scale.
pub const QUAD_VIEW_WEAPON_OUTLINE_MIN: f64 = 0.24;
pub const QUAD_VIEW_WEAPON_OUTLINE_RANGE: f64 = 0.24;

/// `getQuadDamagePulse`: 0 to 1 and back, 1.5 times a second.
#[must_use]
pub fn get_quad_damage_pulse(elapsed_seconds: f64) -> f64 {
    0.5 + 0.5 * js_sin(elapsed_seconds * QUAD_PULSE_HZ * std::f64::consts::PI * 2.0)
}

/// `POWERUP_KINDS`' first bit.
pub const QUAD_MASK_BIT: u32 = 1;

/// `maskHasPowerup(mask, "quad")`.
#[must_use]
pub fn mask_has_quad(mask: u32) -> bool {
    mask & QUAD_MASK_BIT != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pulse_breathes() {
        assert_eq!(get_quad_damage_pulse(0.0), 0.5);
        assert!((get_quad_damage_pulse(1.0 / 6.0) - 1.0).abs() < 1e-12);
        assert!(get_quad_damage_pulse(0.5).abs() < 1e-12);
        assert!(mask_has_quad(1) && !mask_has_quad(0) && !mask_has_quad(2));
    }
}
