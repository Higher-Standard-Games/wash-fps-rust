//! Ground and air acceleration (wash-fps `simulation/Acceleration.ts`):
//! Q3 and Warsow friction and acceleration, on the XZ plane.

use super::movement_config::MovementConfig;
use super::vec3_helpers::vec3_dot_xz;
use shagger::shared::js::js_max;
use shagger::shared::server::simulation::types::Vec3;

/// `applyFriction`.
pub fn apply_friction(vel: &mut Vec3, friction: f64, stop_speed: f64, delta_time: f64) {
    let speed = (vel.x * vel.x + vel.z * vel.z).sqrt();
    if speed < 1.0 {
        vel.x = 0.0;
        vel.z = 0.0;
        return;
    }
    let control = if speed < stop_speed { stop_speed } else { speed };
    let drop = control * friction * delta_time;
    let mut new_speed = speed - drop;
    if new_speed < 0.0 {
        new_speed = 0.0;
    }
    let scale = new_speed / speed;
    vel.x *= scale;
    vel.z *= scale;
}

/// `groundAccelerate`.
pub fn ground_accelerate(vel: &mut Vec3, wish_dir: Vec3, wish_speed: f64, ground_accel: f64, delta_time: f64) {
    let current_speed = vec3_dot_xz(*vel, wish_dir);
    let add_speed = wish_speed - current_speed;
    if add_speed <= 0.0 {
        return;
    }
    let mut accel_speed = ground_accel * delta_time * wish_speed;
    if accel_speed > add_speed {
        accel_speed = add_speed;
    }
    vel.x += wish_dir.x * accel_speed;
    vel.z += wish_dir.z * accel_speed;
}

/// `calcAccelFalloff`: less air acceleration at high speed.
#[must_use]
pub fn calc_accel_falloff(cur_speed: f64, run_speed: f64, radical_speed: f64, top_speed: f64) -> f64 {
    if cur_speed <= run_speed {
        return 1.0;
    }
    if cur_speed < radical_speed {
        let ratio = (cur_speed - run_speed) / (radical_speed - run_speed);
        return 1.0 - ratio * 0.5;
    }
    let ratio = (cur_speed - radical_speed) / (top_speed - radical_speed);
    js_max(0.0, 0.5 * (1.0 - ratio * ratio * ratio))
}

/// `airAccelerateForward`: forward only and accelerating (Warsow).
pub fn air_accelerate_forward(
    vel: &mut Vec3,
    wish_dir: Vec3,
    wish_speed: f64,
    config: &MovementConfig,
    delta_time: f64,
) {
    let max_player_speed = config.max_speed;
    let topspeed = config.top_speed;
    let run_speed = config.run_speed;
    let radical_speed = 600.0;

    let airforwardaccel = 1.00001;
    let bunnyaccel = 0.1593;
    let turnaccel = 4.0;
    let backtosideratio = 0.8;

    let curvel_x = vel.x;
    let curvel_z = vel.z;
    let curspeed = (curvel_x * curvel_x + curvel_z * curvel_z).sqrt();

    let accel_falloff = calc_accel_falloff(curspeed, run_speed, radical_speed, topspeed);

    let target_wishspeed = if wish_speed > curspeed * 1.01 {
        let accelspeed = curspeed + airforwardaccel * max_player_speed * delta_time * accel_falloff;
        shagger::shared::js::js_min(wish_speed, accelspeed)
    } else {
        let f = js_max(0.0, (topspeed - curspeed) / (topspeed - max_player_speed));
        js_max(curspeed, max_player_speed) + bunnyaccel * f * accel_falloff * max_player_speed * delta_time
    };

    let wishvel_x = wish_dir.x * target_wishspeed;
    let wishvel_z = wish_dir.z * target_wishspeed;
    let mut accel_dir_x = wishvel_x - curvel_x;
    let mut accel_dir_z = wishvel_z - curvel_z;

    let accel_dir_len = (accel_dir_x * accel_dir_x + accel_dir_z * accel_dir_z).sqrt();
    if accel_dir_len < 0.001 {
        return;
    }
    accel_dir_x /= accel_dir_len;
    accel_dir_z /= accel_dir_len;
    let addspeed = accel_dir_len;

    let mut accelspeed = turnaccel * max_player_speed * delta_time;
    if accelspeed > addspeed {
        accelspeed = addspeed;
    }

    if backtosideratio < 1.0 && curspeed > 0.001 {
        let cur_dir_x = curvel_x / curspeed;
        let cur_dir_z = curvel_z / curspeed;
        let dot = accel_dir_x * cur_dir_x + accel_dir_z * cur_dir_z;
        if dot < 0.0 {
            accel_dir_x -= (1.0 - backtosideratio) * dot * cur_dir_x;
            accel_dir_z -= (1.0 - backtosideratio) * dot * cur_dir_z;
        }
    }

    vel.x += accelspeed * accel_dir_x;
    vel.z += accelspeed * accel_dir_z;
}

pub const STRAFE_BUNNY_ACCEL: f64 = 50.0;

/// `strafeBunnyAccelerate`: strafe only (CPMA and Q2 strafe jumping).
pub fn strafe_bunny_accelerate(
    vel: &mut Vec3,
    wish_dir: Vec3,
    wish_speed: f64,
    config: &MovementConfig,
    delta_time: f64,
) {
    let strafe_wishspeed = shagger::shared::js::js_min(wish_speed, config.max_air_speed);
    let cur_speed = (vel.x * vel.x + vel.z * vel.z).sqrt();
    let accel_falloff = calc_accel_falloff(cur_speed, config.run_speed, 600.0, config.top_speed);
    let strafe_bunny_accel = STRAFE_BUNNY_ACCEL * accel_falloff;
    let currentspeed = vel.x * wish_dir.x + vel.z * wish_dir.z;
    let addspeed = strafe_wishspeed - currentspeed;
    if addspeed > 0.0 && accel_falloff > 0.01 {
        let mut accelspeed = strafe_bunny_accel * delta_time * strafe_wishspeed;
        if accelspeed > addspeed {
            accelspeed = addspeed;
        }
        vel.x += accelspeed * wish_dir.x;
        vel.z += accelspeed * wish_dir.z;
    }
}

/// `normalAirAccelerate`: the default (Q3).
pub fn normal_air_accelerate(
    vel: &mut Vec3,
    wish_dir: Vec3,
    wish_speed: f64,
    config: &MovementConfig,
    delta_time: f64,
) {
    let cur_speed = (vel.x * vel.x + vel.z * vel.z).sqrt();
    let accel_falloff = calc_accel_falloff(cur_speed, config.run_speed, 600.0, config.top_speed);
    let currentspeed = vel.x * wish_dir.x + vel.z * wish_dir.z;
    let addspeed = wish_speed - currentspeed;
    if addspeed > 0.0 && accel_falloff > 0.01 {
        let mut accelspeed = config.air_acceleration * delta_time * wish_speed * accel_falloff;
        if accelspeed > addspeed {
            accelspeed = addspeed;
        }
        vel.x += accelspeed * wish_dir.x;
        vel.z += accelspeed * wish_dir.z;
    }
}
