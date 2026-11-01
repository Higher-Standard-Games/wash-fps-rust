//! Ledge detection, grab pause and pull-up (wash-fps `simulation/LedgeGrab.ts`).

use super::movement_config::{LedgeGrabPhase, MovementConfig};
use super::vec3_helpers::{VEC3_ZERO, vec3_add, vec3_normalize, vec3_scale};
use shagger::network::components::input_component::input_buttons;
use shagger::network::systems::movement::movement_profile::MovementInput;
use shagger::physics::player_physics_constants::{PLAYER_MAXS_Z, PLAYER_MINS_Z, PLAYER_RADIUS};
use shagger::shared::collision::collision_world::{Aabb, CollisionWorld};
use shagger::shared::js::{js_cos, js_hypot, js_max, js_min, js_sin};
use shagger::shared::server::simulation::types::Vec3;

const PLAYER_BOUNDS: Aabb = Aabb {
    min: Vec3::new(-PLAYER_RADIUS, PLAYER_MINS_Z, -PLAYER_RADIUS),
    max: Vec3::new(PLAYER_RADIUS, PLAYER_MAXS_Z, PLAYER_RADIUS),
};
const CLEARANCE: f64 = 1.0;
const RETRY_DELAY_MS: f64 = 300.0;

/// `LedgeGrab`; a copy is `saveState`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LedgeGrab {
    pub state: LedgeGrabPhase,
    /// Camera phase, 0 to 1.
    pub phase: f64,
    pub state_start_ms: f64,
    pub cooldown_until_ms: f64,
    pub target_pos: Vec3,
    pub start_pos: Vec3,
}

fn hit(collision: &mut dyn CollisionWorld, start: Vec3, end: Vec3) -> bool {
    collision.trace(start, end, Some(PLAYER_BOUNDS), None).hit
}

impl LedgeGrab {
    /// `reset`: keeps the target and start positions.
    pub fn reset(&mut self) {
        self.state = LedgeGrabPhase::None;
        self.phase = 0.0;
        self.state_start_ms = 0.0;
        self.cooldown_until_ms = 0.0;
    }

    #[must_use]
    pub fn is_on_cooldown(&self, now_ms: f64) -> bool {
        now_ms < self.cooldown_until_ms
    }

    /// `update`: true while the grab owns the step. Moves `pos` and `vel`.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        pos: &mut Vec3,
        vel: &mut Vec3,
        input: &MovementInput,
        collision: Option<&mut dyn CollisionWorld>,
        now_ms: f64,
        config: &MovementConfig,
        is_grounded: bool,
    ) -> bool {
        if !config.ledge_grab_enabled {
            return false;
        }

        if self.state == LedgeGrabPhase::Grabbing {
            let grab_elapsed_sec = (now_ms - self.state_start_ms) / 1000.0;
            let Some(collision) = collision else {
                return self.abort(vel, now_ms);
            };
            if hit(collision, *pos, self.start_pos) {
                return self.abort(vel, now_ms);
            }
            *pos = self.start_pos;
            *vel = VEC3_ZERO;
            let grab_pause_time = 0.12;
            self.phase = js_min(0.3, (grab_elapsed_sec / grab_pause_time) * 0.3);
            let min_grab_time = 0.05;
            if grab_elapsed_sec > min_grab_time && input.is_pressed(input_buttons::CROUCH) {
                return self.abort(vel, now_ms);
            }
            if grab_elapsed_sec >= grab_pause_time {
                self.state = LedgeGrabPhase::Pulling;
                self.state_start_ms = now_ms;
            }
            return true;
        }

        if self.state == LedgeGrabPhase::Pulling {
            let pull_elapsed_sec = (now_ms - self.state_start_ms) / 1000.0;
            let progress = js_min(1.0, pull_elapsed_sec / config.ledge_pull_time);
            let t = if progress < 0.5 {
                2.0 * progress * progress
            } else {
                let a = -2.0 * progress + 2.0;
                1.0 - (a * a) / 2.0
            };
            self.phase = 0.3 + t * 0.7;

            let (start, target) = (self.start_pos, self.target_pos);
            let rise = target.y - start.y;
            let across = js_hypot(&[target.x - start.x, target.z - start.z]);
            let travel = t * (rise + across);
            let next = if travel <= rise {
                Vec3::new(start.x, start.y + travel, start.z)
            } else {
                let k = js_min(1.0, (travel - rise) / js_max(across, 1e-9));
                Vec3::new(
                    start.x + (target.x - start.x) * k,
                    target.y,
                    start.z + (target.z - start.z) * k,
                )
            };
            let Some(collision) = collision else {
                return self.abort(vel, now_ms);
            };
            if pos.y < target.y && travel > rise {
                let raised = Vec3::new(pos.x, target.y, pos.z);
                if hit(collision, *pos, raised) {
                    return self.abort(vel, now_ms);
                }
                pos.y = raised.y;
            }
            if hit(collision, *pos, next) {
                return self.abort(vel, now_ms);
            }
            *pos = next;
            *vel = VEC3_ZERO;
            if progress >= 1.0 {
                self.state = LedgeGrabPhase::None;
                self.phase = 0.0;
                self.cooldown_until_ms = now_ms + RETRY_DELAY_MS;
                let forward = vec3_normalize(input.direction);
                vel.x = forward.x * 50.0;
                vel.y = 0.0;
                vel.z = forward.z * 50.0;
            }
            return true;
        }

        if self.is_on_cooldown(now_ms) {
            return false;
        }
        let Some(collision) = collision else { return false };
        if is_grounded || vel.y > 200.0 {
            return false;
        }

        let reach = config.ledge_reach;
        let climb_height = config.ledge_height;
        let yaw = input.view_angles.yaw;
        let fwd = Vec3::new(js_sin(yaw), 0.0, js_cos(yaw));
        let is_pressing_forward = input.is_pressed(input_buttons::FORWARD);
        let vel_forward_dot = vel.x * fwd.x + vel.z * fwd.z;
        if !(is_pressing_forward || vel_forward_dot > 50.0) {
            return false;
        }

        let eye_y = pos.y + 24.0;
        let (cos30, sin30, cos60, sin60) = (0.866, 0.5, 0.5, 0.866);
        let probes = [
            fwd,
            Vec3::new(fwd.x * cos30 - fwd.z * sin30, 0.0, fwd.x * sin30 + fwd.z * cos30),
            Vec3::new(fwd.x * cos30 + fwd.z * sin30, 0.0, -fwd.x * sin30 + fwd.z * cos30),
            Vec3::new(fwd.x * cos60 - fwd.z * sin60, 0.0, fwd.x * sin60 + fwd.z * cos60),
            Vec3::new(fwd.x * cos60 + fwd.z * sin60, 0.0, -fwd.x * sin60 + fwd.z * cos60),
        ];

        let mut target_pos = None;
        let mut best_fraction = 1.0;
        for probe in probes {
            let waist_start = Vec3::new(pos.x, eye_y - 16.0, pos.z);
            let waist_end = vec3_add(waist_start, vec3_scale(probe, reach));
            let wall = collision.trace(waist_start, waist_end, None, None);
            let Some(normal) = wall.normal else { continue };
            if !wall.hit || wall.fraction >= best_fraction {
                continue;
            }
            let head_start = Vec3::new(pos.x, eye_y + 8.0, pos.z);
            let head = collision.trace(head_start, vec3_add(head_start, vec3_scale(probe, reach)), None, None);
            if head.hit && head.fraction < 0.9 {
                continue;
            }
            let contact = wall.hit_point.unwrap_or(Vec3::new(
                waist_start.x + (waist_end.x - waist_start.x) * wall.fraction,
                waist_start.y,
                waist_start.z + (waist_end.z - waist_start.z) * wall.fraction,
            ));
            if let Some(candidate) = Self::find_target(*pos, contact, normal, collision, eye_y, climb_height) {
                target_pos = Some(candidate);
                best_fraction = wall.fraction;
            }
        }
        let Some(target) = target_pos else { return false };

        self.state = LedgeGrabPhase::Grabbing;
        self.state_start_ms = now_ms;
        self.start_pos = *pos;
        self.target_pos = target;
        *vel = VEC3_ZERO;
        true
    }

    /// `findTarget`: a supported full-hull destination with a clear lift and
    /// a clear way across.
    fn find_target(
        pos: Vec3,
        contact: Vec3,
        wall_normal: Vec3,
        collision: &mut dyn CollisionWorld,
        eye_y: f64,
        climb_height: f64,
    ) -> Option<Vec3> {
        let length = js_hypot(&[wall_normal.x, wall_normal.z]);
        if length < 1e-6 {
            return None;
        }
        let (nx, nz) = (wall_normal.x / length, wall_normal.z / length);
        let inset = PLAYER_RADIUS * (nx.abs() + nz.abs()) + CLEARANCE;
        let above = Vec3::new(contact.x - nx * inset, eye_y + climb_height, contact.z - nz * inset);
        let surface = collision.trace(
            above,
            Vec3 {
                y: eye_y - 48.0,
                ..above
            },
            None,
            None,
        );
        if !surface.hit || surface.normal.is_none_or(|n| n.y < 0.7) {
            return None;
        }
        let surface_y = above.y + (eye_y - 48.0 - above.y) * surface.fraction;
        if surface_y - eye_y < -16.0 || surface_y - eye_y > climb_height {
            return None;
        }

        let support_start = Vec3 {
            y: above.y - PLAYER_MINS_Z + CLEARANCE,
            ..above
        };
        let support_end = Vec3 {
            y: surface_y - PLAYER_MINS_Z - 2.0,
            ..above
        };
        let support = collision.trace(support_start, support_end, Some(PLAYER_BOUNDS), None);
        if !support.hit || support.normal.is_none_or(|n| n.y < 0.7) {
            return None;
        }
        let target = Vec3::new(
            above.x,
            support_start.y + (support_end.y - support_start.y) * support.fraction + CLEARANCE,
            above.z,
        );
        let standing_surface_y = target.y + PLAYER_MINS_Z - CLEARANCE;
        if standing_surface_y - eye_y < -16.0 || standing_surface_y - eye_y > climb_height {
            return None;
        }
        if hit(collision, target, target) {
            return None;
        }
        let raised = Vec3::new(pos.x, target.y, pos.z);
        if hit(collision, pos, raised) || hit(collision, raised, target) {
            return None;
        }
        Some(target)
    }

    fn abort(&mut self, vel: &mut Vec3, now_ms: f64) -> bool {
        self.state = LedgeGrabPhase::None;
        self.phase = 0.0;
        self.cooldown_until_ms = now_ms + RETRY_DELAY_MS;
        vel.y = js_min(vel.y, -100.0);
        false
    }
}
