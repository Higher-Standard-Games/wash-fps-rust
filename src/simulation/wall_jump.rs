//! Wall jump detection, charging and execution (wash-fps `simulation/WallJump.ts`).

use super::movement_config::{AdvancedMovementResult, LedgeGrabPhase, MovementConfig, WallJumpPhase};
use super::slide::Slide;
use super::vec3_helpers::{VEC3_ZERO, vec3_length_xz, vec3_mul_add};
use shagger::network::components::input_component::input_buttons;
use shagger::network::systems::movement::movement_profile::MovementInput;
use shagger::shared::collision::collision_world::{Aabb, CollisionWorld};
use shagger::shared::js::{js_cos, js_sin};
use shagger::shared::server::simulation::types::Vec3;
use std::f64::consts::{FRAC_1_SQRT_2, PI};

const DETECTION_GRACE_MS: f64 = 100.0;

/// A small box catches angled faces that point rays miss.
const WALL_TRACE_BOUNDS: Aabb = Aabb {
    min: Vec3::new(-4.0, -16.0, -4.0),
    max: Vec3::new(4.0, 16.0, 4.0),
};

/// One boost's cooldown, kept with its start so a replay can drop it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cooldown {
    pub start_ms: f64,
    pub until_ms: f64,
}

/// `WallJump`; a clone is `saveState`.
#[derive(Debug, Clone, PartialEq)]
pub struct WallJump {
    pub state: WallJumpPhase,
    pub normal: Vec3,
    /// Position when the grab started.
    pub position: Vec3,
    /// Velocity before charging.
    pub pre_velocity: Vec3,
    pub key_released: bool,
    pub charge_start_ms: f64,
    /// Up to 64 boosts, oldest first.
    pub cooldowns: Vec<Cooldown>,
    pub grace_until_ms: f64,
    pub ground_grace_until_ms: f64,
}

impl Default for WallJump {
    fn default() -> Self {
        Self {
            state: WallJumpPhase::None,
            normal: VEC3_ZERO,
            position: VEC3_ZERO,
            pre_velocity: VEC3_ZERO,
            key_released: true,
            charge_start_ms: 0.0,
            cooldowns: Vec::new(),
            grace_until_ms: 0.0,
            ground_grace_until_ms: 0.0,
        }
    }
}

/// Crouch and slide fields the charging result reports.
pub struct SlideView {
    pub is_sliding: bool,
    pub is_crouching: bool,
    pub crouch_view_offset: f64,
    pub slide_phase: f64,
}

impl WallJump {
    /// `isOnCooldown`: the newest cooldown that has started decides.
    #[must_use]
    pub fn is_on_cooldown(&self, now_ms: f64) -> bool {
        for c in self.cooldowns.iter().rev() {
            if now_ms >= c.start_ms {
                return now_ms < c.until_ms;
            }
        }
        false
    }

    /// `beginStep`: replaying a step drops any boost predicted at or after it.
    pub fn begin_step(&mut self, now_ms: f64) {
        while self.cooldowns.last().is_some_and(|c| c.start_ms >= now_ms) {
            self.cooldowns.pop();
        }
    }

    fn start_cooldown(&mut self, now_ms: f64, duration_ms: f64) {
        self.begin_step(now_ms);
        self.cooldowns.push(Cooldown {
            start_ms: now_ms,
            until_ms: now_ms + duration_ms,
        });
        if self.cooldowns.len() > 64 {
            self.cooldowns.remove(0);
        }
    }

    /// `reset`: keeps `key_released`, the normal, positions and cooldowns.
    pub fn reset(&mut self) {
        self.state = WallJumpPhase::None;
        self.charge_start_ms = 0.0;
        self.grace_until_ms = 0.0;
        self.ground_grace_until_ms = 0.0;
    }

    /// `update`: detection and the state machine; an instant jump changes `vel`.
    #[allow(clippy::too_many_lines)]
    pub fn update(
        &mut self,
        pos: Vec3,
        vel: &mut Vec3,
        input: &MovementInput,
        collision: Option<&mut dyn CollisionWorld>,
        now_ms: f64,
        config: &MovementConfig,
    ) {
        let Some(collision) = collision else { return };
        let is_jumping = input.is_pressed(input_buttons::JUMP);
        if !is_jumping {
            self.key_released = true;
        }
        if self.is_on_cooldown(now_ms) {
            self.state = WallJumpPhase::None;
            return;
        }

        let feet_y = pos.y - 24.0;
        let ground_check = collision.trace(
            pos,
            Vec3::new(pos.x, feet_y - config.wall_jump_min_height, pos.z),
            None,
            None,
        );
        let has_ground_clearance = !(ground_check.hit && ground_check.fraction < 1.0);
        if has_ground_clearance {
            self.ground_grace_until_ms = now_ms + DETECTION_GRACE_MS;
        } else if now_ms >= self.ground_grace_until_ms {
            self.state = WallJumpPhase::None;
            return;
        }

        let reach = config.wall_jump_reach;
        let input_len = vec3_length_xz(input.direction);
        let forward = if input_len > 0.1 {
            Vec3::new(input.direction.x / input_len, 0.0, input.direction.z / input_len)
        } else {
            let vel_len = vec3_length_xz(*vel);
            if vel_len > 1.0 {
                Vec3::new(vel.x / vel_len, 0.0, vel.z / vel_len)
            } else {
                let yaw = input.view_angles.yaw;
                Vec3::new(js_sin(yaw), 0.0, js_cos(yaw))
            }
        };

        let (cos, sin) = (js_cos(PI / 8.0), js_sin(PI / 8.0));
        let r = FRAC_1_SQRT_2;
        let (fx, fz) = (forward.x, forward.z);
        let directions = [
            forward,
            Vec3::new(fx * cos - fz * sin, 0.0, fx * sin + fz * cos),
            Vec3::new(fx * cos + fz * sin, 0.0, -fx * sin + fz * cos),
            Vec3::new(fx * r - fz * r, 0.0, fx * r + fz * r),
            Vec3::new(fx * r + fz * r, 0.0, -fx * r + fz * r),
            Vec3::new(-fz, 0.0, fx),
            Vec3::new(fz, 0.0, -fx),
            Vec3::new(-fx, 0.0, -fz),
        ];

        let mut wall_normal = None;
        for dir in directions {
            let end = vec3_mul_add(pos, dir, reach);
            let wall_check = collision.trace(pos, end, Some(WALL_TRACE_BOUNDS), None);
            let Some(normal) = wall_check.normal.filter(|_| wall_check.hit) else {
                continue;
            };
            if normal.y.abs() > 0.3 {
                continue;
            }
            let eye_height = 32.0;
            let eye_start = Vec3::new(pos.x, pos.y + eye_height - 10.0, pos.z);
            let eye_end = vec3_mul_add(eye_start, dir, reach);
            if !collision.trace(eye_start, eye_end, Some(WALL_TRACE_BOUNDS), None).hit {
                continue;
            }
            let vel_dot = vel.x * normal.x + vel.z * normal.z;
            if vel_dot > 5.0 {
                continue;
            }
            wall_normal = Some(normal);
            break;
        }

        if let Some(normal) = wall_normal {
            self.normal = normal;
            self.state = WallJumpPhase::Available;
            self.grace_until_ms = now_ms + DETECTION_GRACE_MS;
            if is_jumping && self.key_released {
                self.key_released = false;
                if config.wall_jump_charge_time > 0.0 {
                    self.start_charge(pos, *vel, normal, now_ms);
                } else {
                    self.execute_instant(vel, normal, config, now_ms);
                }
            }
        } else if now_ms >= self.grace_until_ms {
            self.state = WallJumpPhase::None;
        }
    }

    /// `handleCharging`: hang on the wall until jump is released or the
    /// charge time runs out, then boost.
    #[allow(clippy::too_many_arguments)]
    pub fn handle_charging(
        &mut self,
        input: Option<&MovementInput>,
        now_ms: f64,
        config: &MovementConfig,
        slide_state: SlideView,
        ledge_state: (LedgeGrabPhase, f64),
        actual_horiz_speed: f64,
        slide: &mut Slide,
    ) -> AdvancedMovementResult {
        let charge_elapsed_ms = now_ms - self.charge_start_ms;
        let is_jumping = input.is_some_and(|i| i.is_pressed(input_buttons::JUMP));
        let charge_time_ms = config.wall_jump_charge_time * 1000.0;

        if !is_jumping || charge_elapsed_ms >= charge_time_ms {
            self.execute_boost(config, now_ms);
            return AdvancedMovementResult {
                is_sliding: slide_state.is_sliding,
                is_crouching: slide_state.is_crouching,
                wall_jump_state: self.state,
                ledge_grab_state: ledge_state.0,
                actual_horiz_speed,
                crouch_view_offset: slide_state.crouch_view_offset,
                slide_phase: slide_state.slide_phase,
                ledge_grab_phase: ledge_state.1,
                ..AdvancedMovementResult::base(self.position, self.pre_velocity, false)
            };
        }

        if let Some(input) = input {
            slide.update_crouch(input, now_ms);
        }
        // The TypeScript read these before the crouch update.
        AdvancedMovementResult {
            is_sliding: false,
            is_crouching: slide_state.is_crouching,
            wall_jump_state: WallJumpPhase::Charging,
            ledge_grab_state: ledge_state.0,
            actual_horiz_speed: 0.0,
            crouch_view_offset: slide_state.crouch_view_offset,
            slide_phase: 0.0,
            ledge_grab_phase: ledge_state.1,
            ..AdvancedMovementResult::base(self.position, VEC3_ZERO, false)
        }
    }

    fn start_charge(&mut self, pos: Vec3, vel: Vec3, normal: Vec3, now_ms: f64) {
        self.state = WallJumpPhase::Charging;
        self.charge_start_ms = now_ms;
        self.normal = normal;
        self.position = pos;
        self.pre_velocity = vel;
    }

    /// `executeBoost`: reflect the stored velocity off the wall (R = V - 2(V.N)N).
    fn execute_boost(&mut self, config: &MovementConfig, now_ms: f64) {
        let normal = self.normal;
        let base = &mut self.pre_velocity;
        let bounce = config.wall_jump_bounce;
        let boost = config.wall_jump_boost;
        let dot = base.x * normal.x + base.z * normal.z;
        let reflected_x = base.x - 2.0 * dot * normal.x;
        let reflected_z = base.z - 2.0 * dot * normal.z;
        base.x = reflected_x * bounce;
        base.z = reflected_z * bounce;
        if vec3_length_xz(*base) < 100.0 {
            base.x += normal.x * 200.0 * boost;
            base.z += normal.z * 200.0 * boost;
        }
        base.y = config.wall_jump_height * boost;
        self.start_cooldown(now_ms, config.wall_jump_cooldown * 1000.0);
        self.state = WallJumpPhase::None;
        self.charge_start_ms = 0.0;
    }

    fn execute_instant(&mut self, vel: &mut Vec3, normal: Vec3, config: &MovementConfig, now_ms: f64) {
        let bounce = config.wall_jump_bounce;
        let dot = vel.x * normal.x + vel.z * normal.z;
        let reflected_x = vel.x - 2.0 * dot * normal.x;
        let reflected_z = vel.z - 2.0 * dot * normal.z;
        vel.x = reflected_x * bounce;
        vel.z = reflected_z * bounce;
        if vec3_length_xz(*vel) < 100.0 {
            vel.x += normal.x * 200.0;
            vel.z += normal.z * 200.0;
        }
        vel.y = config.wall_jump_height;
        self.start_cooldown(now_ms, config.wall_jump_cooldown * 1000.0);
        self.state = WallJumpPhase::None;
    }
}
