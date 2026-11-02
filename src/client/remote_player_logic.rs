//! wash-fps `client/RemotePlayerLogic.ts`: the decisions the remote player
//! renderer makes from snapshot data. Ported so far: `deriveAnimation`
//! and its thresholds, `extractEntities` and `applyRoster`.

use std::collections::HashMap;

use shagger::network::client::snapshot_buffer::{EntitySnapshot, InterpolatedEntity};
use shagger::shared::net::game_state::GameState;

/// `CLIENT_KEY_PREFIX` (`ClientEntityKey.ts`): the entity keys of players.
pub const CLIENT_KEY_PREFIX: &str = "player_";

/// `isClientKey`.
#[must_use]
pub fn is_client_key(key: &str) -> bool {
    key.starts_with(CLIENT_KEY_PREFIX)
}

/// `extractEntities`: every player's body in a snapshot but the local
/// one's, as the interpolation buffer holds it. The wire carries no water
/// level or ladder flag, so those read dry and off.
#[must_use]
pub fn extract_entities<E>(state: &GameState<E>, local_entity_id: &str) -> HashMap<String, EntitySnapshot> {
    let mut entities = HashMap::new();
    for (entity_id, entity) in state.entities.iter() {
        if entity_id == local_entity_id || !is_client_key(entity_id) {
            continue;
        }
        let Some(transform) = entity.transform else {
            continue;
        };
        let velocity = entity.velocity;
        let movement = entity.movement_state;
        let view = movement.and_then(|m| m.view);
        entities.insert(
            entity_id.to_owned(),
            EntitySnapshot {
                x: transform.position.x,
                y: transform.position.y,
                z: transform.position.z,
                yaw: transform.rotation[1],
                pitch: Some(transform.rotation[0]),
                vx: velocity.map_or(0.0, |v| v.linear.x),
                vy: velocity.map_or(0.0, |v| v.linear.y),
                vz: velocity.map_or(0.0, |v| v.linear.z),
                grounded: velocity.is_none_or(|v| v.grounded),
                crouching: movement.is_some_and(|m| m.is_crouching),
                sliding: movement.is_some_and(|m| m.is_sliding),
                wall_jumping: movement.is_some_and(|m| m.is_wall_jumping),
                aiming: movement.map(|m| m.is_aiming),
                crouch_view_offset: view.map(|v| v.crouch_view_offset),
                strafe_input: view.map(|v| v.strafe_input),
                forward_input: view.map(|v| v.forward_input),
                slide_phase: view.map(|v| v.slide_phase),
                ledge_grab_phase: view.map(|v| v.ledge_grab_phase),
                powerup_mask: Some(entity.powerups.map_or(0, u32::from)),
                water_level: Some(0),
                on_ladder: Some(false),
            },
        );
    }
    entities
}

/// `applyRoster`: each player's team from the snapshot roster ("ffa" for
/// none). Returns the local player's team when the roster names them
/// (`Some(None)` for no team), `None` when it does not.
pub fn apply_roster<E>(
    teams: &mut HashMap<String, String>,
    state: &GameState<E>,
    local_entity_id: &str,
) -> Option<Option<String>> {
    let mut local = None;
    for player in state.players.values() {
        let team = if player.team.is_empty() { "ffa" } else { &player.team };
        teams.insert(player.entity_id.clone(), team.to_owned());
        if player.entity_id == local_entity_id {
            local = Some((!player.team.is_empty()).then(|| player.team.clone()));
        }
    }
    local
}

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
