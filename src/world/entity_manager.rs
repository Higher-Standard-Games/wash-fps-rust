//! wash-fps `world/EntityManager.ts`: the parts that place map pickups.

/// `mapWeaponYaw`: a map weapon's resting yaw, from its pickup id, so a rack
/// of them does not read as identical props.
#[must_use]
pub fn map_weapon_yaw(pickup_id: u32) -> f64 {
    (f64::from(pickup_id) * 2.399963) % (std::f64::consts::PI * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_wraps_at_a_full_turn() {
        assert_eq!(map_weapon_yaw(0), 0.0);
        assert_eq!(map_weapon_yaw(2), 4.799926);
        // 3 * 2.399963 = 7.199889, past 2π.
        assert!((map_weapon_yaw(3) - (7.199889 - std::f64::consts::TAU)).abs() < 1e-12);
    }
}
