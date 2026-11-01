//! Three-point water sampling (wash-fps `liquid/LiquidVolumeSampler.ts`).

use super::liquid_types::{DRY_SAMPLE, LiquidKind, LiquidSample, WaterLevel};
use shagger::shared::collision::collision_world::{CollisionWorld, MASK_LIQUID, contents};
use shagger::shared::server::simulation::types::Vec3;

/// `LiquidSampleBounds`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidSampleBounds {
    /// Hull floor relative to the origin: -24 standing, -4 crouched.
    pub mins_y: f64,
    /// Eye height relative to the origin: 34 standing, 12 crouched.
    pub view_height: f64,
}

/// `liquidKindAt`: the most hostile liquid wins.
#[must_use]
pub fn liquid_kind_at(c: u32) -> Option<LiquidKind> {
    if c & contents::LAVA != 0 {
        Some(LiquidKind::Lava)
    } else if c & contents::SLIME != 0 {
        Some(LiquidKind::Slime)
    } else if c & contents::WATER != 0 {
        Some(LiquidKind::Water)
    } else {
        None
    }
}

/// `sampleLiquid`: foot (feet + 1), waist (midway from the feet to the eye)
/// and eye probes, stopping at the first dry one.
pub fn sample_liquid(
    collision: Option<&mut dyn CollisionWorld>,
    origin: Vec3,
    bounds: LiquidSampleBounds,
) -> LiquidSample {
    let Some(collision) = collision else { return DRY_SAMPLE };
    let feet_y = origin.y + bounds.mins_y;
    let eye_y = origin.y + bounds.view_height;

    let foot_contents = collision.point_contents(Vec3::new(origin.x, feet_y + 1.0, origin.z)) & MASK_LIQUID;
    if foot_contents == 0 {
        return DRY_SAMPLE;
    }
    let mid_y = feet_y + (eye_y - feet_y) * 0.5;
    let waist_contents = collision.point_contents(Vec3::new(origin.x, mid_y, origin.z)) & MASK_LIQUID;

    let mut level: WaterLevel = 1;
    let mut eye_contents = 0;
    if waist_contents != 0 {
        level = 2;
        eye_contents = collision.point_contents(Vec3::new(origin.x, eye_y, origin.z)) & MASK_LIQUID;
        if eye_contents != 0 {
            level = 3;
        }
    }
    LiquidSample {
        level,
        kind: liquid_kind_at(foot_contents),
        raw_contents: foot_contents,
        foot_contents,
        waist_contents,
        eye_contents,
    }
}
