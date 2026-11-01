//! Ladders (wash-fps `simulation/Ladder.ts`), on full 3D velocity.

use super::swim::{Intent, local_axes};
use shagger::shared::js::{js_max, js_min};
use shagger::shared::server::simulation::types::Vec3;

/// `LadderMovementProfile`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LadderMovementProfile {
    pub climb_speed: f64,
    pub acceleration: f64,
    pub friction: f64,
    pub lateral_scale: f64,
    pub detach_up: f64,
    pub detach_cooldown_ms: f64,
}

/// `DEFAULT_LADDER_MOVEMENT`.
pub const DEFAULT_LADDER_MOVEMENT: LadderMovementProfile = LadderMovementProfile {
    climb_speed: 168.0,
    acceleration: 30.0,
    friction: 20.0,
    lateral_scale: 0.4,
    detach_up: 270.0,
    detach_cooldown_ms: 400.0,
};

/// `applyLadderFriction`.
pub fn apply_ladder_friction(vel: &mut Vec3, profile: &LadderMovementProfile, delta_time: f64) {
    let speed = (vel.x * vel.x + vel.y * vel.y + vel.z * vel.z).sqrt();
    if speed < 1.0 {
        *vel = Vec3::new(0.0, 0.0, 0.0);
        return;
    }
    let drop = speed * profile.friction * delta_time;
    let scale = js_max(0.0, speed - drop) / speed;
    vel.x *= scale;
    vel.y *= scale;
    vel.z *= scale;
}

/// `ladderIntent`: as the swim intent, with sideways input flattened and
/// scaled down.
#[must_use]
pub fn ladder_intent(
    world_wish: Vec3,
    pitch: f64,
    yaw: f64,
    vertical_intent: f64,
    profile: &LadderMovementProfile,
) -> Intent {
    let (forward_move, side_move, forward, right) = local_axes(world_wish, pitch, yaw);
    let wish = Vec3::new(
        (forward.x * forward_move + right.x * side_move) * profile.lateral_scale,
        forward.y * forward_move + vertical_intent,
        (forward.z * forward_move + right.z * side_move) * profile.lateral_scale,
    );
    let length = (wish.x * wish.x + wish.y * wish.y + wish.z * wish.z).sqrt();
    if length < 0.001 {
        return Intent { dir: None, speed: 0.0 };
    }
    Intent {
        dir: Some(Vec3::new(wish.x / length, wish.y / length, wish.z / length)),
        speed: js_min(length, 1.0) * profile.climb_speed,
    }
}

/// `ladderAccelerate`.
pub fn ladder_accelerate(vel: &mut Vec3, wish_dir: Vec3, wish_speed: f64, acceleration: f64, delta_time: f64) {
    super::swim::water_accelerate(vel, wish_dir, wish_speed, acceleration, delta_time);
}

/// `ladderClimbDelta`.
#[must_use]
pub fn ladder_climb_delta(previous_y: f64, current_y: f64) -> f64 {
    current_y - previous_y
}
