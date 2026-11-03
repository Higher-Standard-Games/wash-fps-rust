//! Port of wash-fps `liquid/client/UnderwaterLook.ts`: what being under
//! water looks like, the pure half. Colours, densities and the transition
//! curve; [`super::underwater_view::UnderwaterView`] applies them.

use shagger::shared::js::js_sin;

use crate::liquid::liquid_types::LiquidKind;

/// `LiquidLook`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidLook {
    /// Fog colour, linear 0-1 RGB.
    pub color: [f64; 3],
    /// Exponential fog density at full submersion.
    pub density: f64,
    /// How far the screen tints toward `color`, 0-1.
    pub tint_strength: f64,
}

/// `lookFor`.
#[must_use]
pub fn look_for(kind: LiquidKind) -> LiquidLook {
    match kind {
        // Deep blue-green: dark enough to read as under water, open enough to fight in.
        LiquidKind::Water => LiquidLook {
            color: [0.04, 0.2, 0.31],
            density: 0.0052,
            tint_strength: 0.8,
        },
        LiquidKind::Slime => LiquidLook {
            color: [0.13, 0.42, 0.06],
            density: 0.009,
            tint_strength: 0.88,
        },
        LiquidKind::Lava => LiquidLook {
            color: [0.75, 0.18, 0.02],
            density: 0.012,
            tint_strength: 0.9,
        },
    }
}

/// `SUBMERSION_RATE`: about 150 ms each way.
pub const SUBMERSION_RATE: f64 = 6.7;

/// `approachSubmersion`: toward the target, snapping the last sliver.
#[must_use]
pub fn approach_submersion(current: f64, target: f64, delta_time: f64) -> f64 {
    let t = (SUBMERSION_RATE * delta_time).min(1.0);
    let next = current + (target - current) * t;
    if (target - next).abs() < 0.002 {
        return target;
    }
    next
}

/// `WARP_AMPLITUDE_DEGREES`.
pub const WARP_AMPLITUDE_DEGREES: f64 = 1.0;
/// `WARP_FREQUENCY_HZ`.
pub const WARP_FREQUENCY_HZ: f64 = 0.4;

/// `fovWarpDegrees`: Quake's underwater view warp, scaled by the comfort
/// setting (`r_underwater_distortion`).
#[must_use]
pub fn fov_warp_degrees(time_seconds: f64, submersion: f64, comfort_scale: f64) -> f64 {
    if submersion <= 0.0 || comfort_scale <= 0.0 {
        return 0.0;
    }
    let phase = time_seconds * WARP_FREQUENCY_HZ * std::f64::consts::PI * 2.0;
    js_sin(phase) * WARP_AMPLITUDE_DEGREES * submersion * comfort_scale
}

/// `fogDensityFor`: nothing at all when dry.
#[must_use]
pub fn fog_density_for(look: &LiquidLook, submersion: f64) -> f64 {
    look.density * submersion
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks() {
        let (water, slime, lava) = (
            look_for(LiquidKind::Water),
            look_for(LiquidKind::Slime),
            look_for(LiquidKind::Lava),
        );
        assert!(water.color[2] > water.color[0]);
        assert!(slime.color[1] > slime.color[0] && slime.color[1] > slime.color[2]);
        assert!(lava.color[0] > lava.color[1]);
        assert!(slime.density > water.density && lava.density > slime.density);
        assert!(water.density < 0.01);
        assert_eq!(fog_density_for(&water, 0.0), 0.0);
        assert!((fog_density_for(&water, 0.5) - water.density / 2.0).abs() < 1e-6);
    }

    #[test]
    fn submersion_eases_and_lands() {
        let next = approach_submersion(0.0, 1.0, 1.0 / 60.0);
        assert!(next > 0.0 && next < 1.0);
        let mut value = 0.0;
        for _ in 0..9 {
            value = approach_submersion(value, 1.0, 1.0 / 60.0);
        }
        assert!(value > 0.6);
        let mut value = 1.0;
        for _ in 0..200 {
            value = approach_submersion(value, 0.0, 1.0 / 60.0);
        }
        assert_eq!(value, 0.0);
        assert_eq!(approach_submersion(0.0, 1.0, 10.0), 1.0);
    }

    #[test]
    fn warp() {
        assert_eq!(fov_warp_degrees(1.234, 0.0, 1.0), 0.0);
        assert_eq!(fov_warp_degrees(1.234, 1.0, 0.0), 0.0);
        let sum: f64 = (0..400)
            .map(|i| fov_warp_degrees(f64::from(i) / 400.0 * 5.0, 1.0, 1.0))
            .sum();
        assert!((sum / 400.0).abs() < 0.05);
        for i in 0..100 {
            assert!(fov_warp_degrees(f64::from(i) * 0.1, 1.0, 1.0).abs() <= WARP_AMPLITUDE_DEGREES);
        }
    }
}
