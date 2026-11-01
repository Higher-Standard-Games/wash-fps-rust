//! wash-fps `client/RemotePlayerLogic.ts`: the decisions the remote player
//! renderer makes from snapshot data. Only `deriveAnimation` and its
//! thresholds are ported so far.

use shagger::network::client::snapshot_buffer::InterpolatedEntity;

pub const IDLE_SPEED_THRESHOLD: f64 = 50.0;

/// Below this 3-D speed a swimmer is treading rather than swimming.
pub const SWIM_IDLE_SPEED_THRESHOLD: f64 = 70.0;
pub const RUN_SPEED_THRESHOLD: f64 = 300.0;

/// The clip a remote player plays, from velocity, grounded state and yaw
/// (`deriveAnimation`).
#[must_use]
pub fn derive_animation(interp: &InterpolatedEntity) -> &'static str {
    // Walljump: on a wall, before the grounded and airborne rules.
    if interp.wall_jumping {
        return "walljump";
    }

    // Swimming, before the airborne rule: a swimmer is ungrounded.
    if interp.water_level.unwrap_or(0) >= 2 {
        let swim_speed = (interp.vx * interp.vx + interp.vy * interp.vy + interp.vz * interp.vz).sqrt();
        return if swim_speed < SWIM_IDLE_SPEED_THRESHOLD {
            "swim_idle"
        } else {
            "swim"
        };
    }

    // Climbing, before the airborne rule for the same reason.
    if interp.on_ladder.unwrap_or(false) {
        return "climb";
    }

    // Airborne: jump for the whole time in the air (Warsow-style).
    if !interp.grounded {
        return "jump";
    }

    if interp.sliding {
        return "slide";
    }

    let speed = (interp.vx * interp.vx + interp.vz * interp.vz).sqrt();
    let fwd_x = interp.yaw.sin();
    let fwd_z = interp.yaw.cos();
    let right_x = fwd_z;
    let right_z = -fwd_x;
    let forward_dot = fwd_x * interp.vx + fwd_z * interp.vz;
    let right_dot = right_x * interp.vx + right_z * interp.vz;

    if interp.crouching {
        if speed < IDLE_SPEED_THRESHOLD {
            return "crouch_walk"; // Frozen first frame = crouch idle
        }
        if right_dot.abs() > forward_dot.abs() * 1.5 {
            return if right_dot > 0.0 {
                "crouch_strafe_l"
            } else {
                "crouch_strafe_r"
            };
        }
        return "crouch_walk"; // Backward handled by negative speedRatio
    }

    if speed < IDLE_SPEED_THRESHOLD {
        return "idle";
    }

    if right_dot.abs() > forward_dot.abs() * 1.5 {
        return if right_dot > 0.0 { "strafe_l" } else { "strafe_r" };
    }

    if speed > RUN_SPEED_THRESHOLD { "run" } else { "walk" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moving(vx: f64, vz: f64) -> InterpolatedEntity {
        InterpolatedEntity {
            vx,
            vz,
            grounded: true,
            ..Default::default()
        }
    }

    #[test]
    fn picks_locomotion_from_velocity_and_yaw() {
        // Yaw 0 faces +Z; right is +X... and `right_dot > 0` plays strafe_l,
        // as wash-fps names it.
        assert_eq!(derive_animation(&moving(0.0, 0.0)), "idle");
        assert_eq!(derive_animation(&moving(0.0, 320.0)), "run");
        assert_eq!(derive_animation(&moving(0.0, -200.0)), "walk");
        assert_eq!(derive_animation(&moving(320.0, 0.0)), "strafe_l");
        assert_eq!(derive_animation(&moving(-320.0, 0.0)), "strafe_r");
        let mut air = moving(0.0, 320.0);
        air.grounded = false;
        assert_eq!(derive_animation(&air), "jump");
        air.water_level = Some(2);
        assert_eq!(derive_animation(&air), "swim");
        let mut crouch = moving(10.0, 0.0);
        crouch.crouching = true;
        assert_eq!(derive_animation(&crouch), "crouch_walk");
    }
}
