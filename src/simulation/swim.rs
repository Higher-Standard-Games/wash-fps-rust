//! Swimming and wading (wash-fps `simulation/Swim.ts`), on full 3D velocity.

use crate::liquid::liquid_types::WaterLevel;
use shagger::shared::js::{js_cos, js_max, js_min, js_sin};
use shagger::shared::server::simulation::types::Vec3;

/// `LiquidMovementProfile`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidMovementProfile {
    pub swim_scale: f64,
    pub water_acceleration: f64,
    pub water_friction: f64,
    /// Lowest water level at which friction applies: 1 or 2.
    pub friction_starts_at_level: WaterLevel,
    pub idle_sink_speed: f64,
    pub water_jump_forward: f64,
    pub water_jump_up: f64,
    pub water_jump_duration_ms: f64,
}

/// `DEFAULT_LIQUID_MOVEMENT`.
pub const DEFAULT_LIQUID_MOVEMENT: LiquidMovementProfile = LiquidMovementProfile {
    swim_scale: 0.5,
    water_acceleration: 4.0,
    water_friction: 1.0,
    friction_starts_at_level: 1,
    idle_sink_speed: 60.0,
    water_jump_forward: 200.0,
    water_jump_up: 350.0,
    water_jump_duration_ms: 2000.0,
};

/// `applyWaterFriction`: drag on all three axes, scaled by depth.
pub fn apply_water_friction(
    vel: &mut Vec3,
    level: WaterLevel,
    profile: &LiquidMovementProfile,
    stop_speed: f64,
    delta_time: f64,
) {
    if level < profile.friction_starts_at_level {
        return;
    }
    let speed = (vel.x * vel.x + vel.y * vel.y + vel.z * vel.z).sqrt();
    if speed < 1.0 {
        *vel = Vec3::new(0.0, 0.0, 0.0);
        return;
    }
    let control = if speed < stop_speed { stop_speed } else { speed };
    let drop = control * profile.water_friction * f64::from(level) * delta_time;
    let new_speed = js_max(0.0, speed - drop);
    let scale = new_speed / speed;
    vel.x *= scale;
    vel.y *= scale;
    vel.z *= scale;
}

/// `waterAccelerate`.
pub fn water_accelerate(vel: &mut Vec3, wish_dir: Vec3, wish_speed: f64, acceleration: f64, delta_time: f64) {
    let current_speed = vel.x * wish_dir.x + vel.y * wish_dir.y + vel.z * wish_dir.z;
    let add_speed = wish_speed - current_speed;
    if add_speed <= 0.0 {
        return;
    }
    let mut accel_speed = acceleration * delta_time * wish_speed;
    if accel_speed > add_speed {
        accel_speed = add_speed;
    }
    vel.x += wish_dir.x * accel_speed;
    vel.y += wish_dir.y * accel_speed;
    vel.z += wish_dir.z * accel_speed;
}

/// `wadeSpeedScale`.
#[must_use]
pub fn wade_speed_scale(level: WaterLevel, profile: &LiquidMovementProfile) -> f64 {
    if level == 0 {
        return 1.0;
    }
    1.0 - (1.0 - profile.swim_scale) * (f64::from(level) / 3.0)
}

/// `SwimIntent`, also used for ladders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Intent {
    /// Unit direction, or none when the player asks for nothing.
    pub dir: Option<Vec3>,
    /// Desired speed along `dir`, already capped.
    pub speed: f64,
}

/// The wish split into forward and side scalars by the command's own yaw,
/// and the pitched forward and flat right axes. Shared by swim and ladder.
pub(crate) fn local_axes(world_wish: Vec3, pitch: f64, yaw: f64) -> (f64, f64, Vec3, Vec3) {
    let sin_yaw = js_sin(yaw);
    let cos_yaw = js_cos(yaw);
    let forward_move = world_wish.x * sin_yaw + world_wish.z * cos_yaw;
    let side_move = world_wish.x * cos_yaw - world_wish.z * sin_yaw;
    let cos_pitch = js_cos(pitch);
    let forward = Vec3::new(sin_yaw * cos_pitch, -js_sin(pitch), cos_yaw * cos_pitch);
    let right = Vec3::new(cos_yaw, 0.0, -sin_yaw);
    (forward_move, side_move, forward, right)
}

/// `swimIntent`: forward is the pitched view; jump and crouch add a vertical
/// intent that does not depend on the view.
#[must_use]
pub fn swim_intent(
    world_wish: Vec3,
    pitch: f64,
    yaw: f64,
    vertical_intent: f64,
    max_speed: f64,
    profile: &LiquidMovementProfile,
) -> Intent {
    let (forward_move, side_move, forward, right) = local_axes(world_wish, pitch, yaw);
    let wish = Vec3::new(
        forward.x * forward_move + right.x * side_move,
        forward.y * forward_move + vertical_intent,
        forward.z * forward_move + right.z * side_move,
    );
    let length = (wish.x * wish.x + wish.y * wish.y + wish.z * wish.z).sqrt();
    if length < 0.001 {
        return Intent { dir: None, speed: 0.0 };
    }
    Intent {
        dir: Some(Vec3::new(wish.x / length, wish.y / length, wish.z / length)),
        speed: js_min(length, 1.0) * max_speed * profile.swim_scale,
    }
}

/// `idleSinkVelocity`.
#[must_use]
pub fn idle_sink_velocity(profile: &LiquidMovementProfile) -> Vec3 {
    Vec3::new(0.0, -profile.idle_sink_speed, 0.0)
}

/// `checkWaterJump`: at level 2, solid ahead at chest height and dry air
/// above it.
pub fn check_water_jump(
    origin: Vec3,
    yaw: f64,
    level: WaterLevel,
    is_solid_at: &mut dyn FnMut(Vec3) -> bool,
    is_liquid_at: &mut dyn FnMut(Vec3) -> bool,
) -> bool {
    if level != 2 {
        return false;
    }
    let (fx, fz) = (js_sin(yaw), js_cos(yaw));
    let ahead = Vec3::new(origin.x + fx * 30.0, origin.y + 4.0, origin.z + fz * 30.0);
    if !is_solid_at(ahead) {
        return false;
    }
    let above = Vec3::new(ahead.x, ahead.y + 16.0, ahead.z);
    !is_solid_at(above) && !is_liquid_at(above)
}
