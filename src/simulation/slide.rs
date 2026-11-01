//! Crouchslide and crouch view state (wash-fps `simulation/Slide.ts`).
//!
//! Every timer is an absolute sim-clock time, never accumulated dt, so a
//! reconciliation replay recomputes the same values.

use super::movement_config::MovementConfig;
use super::vec3_helpers::vec3_length_xz;
use shagger::network::components::input_component::input_buttons;
use shagger::network::systems::movement::movement_profile::MovementInput;
use shagger::shared::js::{js_max, js_min};
use shagger::shared::server::simulation::types::Vec3;

const SLIDE_FADE_MS: f64 = 400.0;
const SLIDE_EXIT_FADE_MS: f64 = 250.0;
const CROUCH_HEIGHT_OFFSET: f64 = -24.0;
const CROUCH_DOWN_MS: f64 = 140.0;
const CROUCH_UP_MS: f64 = 260.0;

fn smoothstep01(t: f64) -> f64 {
    let c = js_max(0.0, js_min(1.0, t));
    c * c * (3.0 - 2.0 * c)
}

/// `Slide`; a copy is `saveState`, assigning one back is `loadState`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Slide {
    pub is_sliding: bool,
    pub is_crouching: bool,
    pub crouch_view_offset: f64,
    /// Camera tilt intensity, 0 to 1.
    pub slide_phase: f64,
    pub slide_start_ms: f64,
    pub cooldown_until_ms: f64,
    pub slide_end_ms: f64,
    pub slide_end_phase: f64,
    pub crouch_anchor_ms: f64,
    pub crouch_from_offset: f64,
    pub crouch_target_offset: f64,
}

impl Slide {
    #[must_use]
    pub fn is_on_cooldown(&self, now_ms: f64) -> bool {
        now_ms < self.cooldown_until_ms
    }

    /// `reset`: leaves the crouch view offset where it is.
    pub fn reset(&mut self) {
        *self = Self {
            crouch_view_offset: self.crouch_view_offset,
            ..Self::default()
        };
    }

    /// `update`: the slide state machine; boosts and decays `vel`.
    pub fn update(
        &mut self,
        vel: &mut Vec3,
        input: &MovementInput,
        is_grounded: bool,
        now_ms: f64,
        delta_time: f64,
        config: &MovementConfig,
    ) {
        let is_crouching = input.is_pressed(input_buttons::CROUCH);
        let is_jumping = input.is_pressed(input_buttons::JUMP);
        let horiz_speed = vec3_length_xz(*vel);

        if !self.is_sliding {
            let can_slide =
                is_grounded && is_crouching && horiz_speed >= config.slide_min_speed && !self.is_on_cooldown(now_ms);
            if can_slide {
                self.is_sliding = true;
                self.slide_start_ms = now_ms;
                self.slide_phase = 0.0;
                let boost = config.slide_speed_multiplier;
                vel.x *= boost;
                vel.z *= boost;
            } else if self.slide_end_phase > 0.0 {
                let t = (now_ms - self.slide_end_ms) / SLIDE_EXIT_FADE_MS;
                self.slide_phase = if t >= 1.0 {
                    0.0
                } else {
                    self.slide_end_phase * (1.0 - smoothstep01(t))
                };
                if self.slide_phase == 0.0 {
                    self.slide_end_phase = 0.0;
                }
            } else {
                self.slide_phase = 0.0;
            }
        } else {
            let slide_elapsed = now_ms - self.slide_start_ms;
            let slide_duration = config.slide_duration;
            let time_remaining = slide_duration - slide_elapsed;
            if slide_elapsed < SLIDE_FADE_MS {
                let t = js_max(0.0, slide_elapsed) / SLIDE_FADE_MS;
                self.slide_phase = t * t * (3.0 - 2.0 * t);
            } else if time_remaining < SLIDE_FADE_MS {
                let t = js_max(0.0, time_remaining) / SLIDE_FADE_MS;
                self.slide_phase = t * t * (3.0 - 2.0 * t);
            } else {
                self.slide_phase = 1.0;
            }

            let slide_expired = slide_elapsed >= slide_duration;
            let too_slow = horiz_speed < 50.0;
            let released_crouch = !is_crouching;
            let jump_cancel = is_jumping && slide_elapsed > 100.0;

            if slide_expired || too_slow || released_crouch || jump_cancel {
                self.exit(now_ms, Some(config));
                if jump_cancel {
                    vel.y = config.jump_velocity;
                    let boost = config.slide_jump_boost;
                    vel.x *= boost;
                    vel.z *= boost;
                }
            } else {
                let decay = config.slide_decay * delta_time;
                let current_speed = vec3_length_xz(*vel);
                let new_speed = js_max(50.0, current_speed - decay);
                let scale = new_speed / current_speed;
                vel.x *= scale;
                vel.z *= scale;
            }
        }
    }

    /// `exit`: end the slide, start the cooldown when given a config.
    pub fn exit(&mut self, now_ms: f64, config: Option<&MovementConfig>) {
        self.is_sliding = false;
        if let Some(config) = config {
            self.cooldown_until_ms = now_ms + config.slide_cooldown;
        }
        self.slide_end_ms = now_ms;
        self.slide_end_phase = self.slide_phase;
    }

    /// `updateCrouch`: crouch state, and the view offset as a function of the
    /// sim clock (snappy down, slower up).
    pub fn update_crouch(&mut self, input: &MovementInput, now_ms: f64) {
        self.is_crouching = input.is_pressed(input_buttons::CROUCH) || self.is_sliding;
        let target_offset = if self.is_crouching { CROUCH_HEIGHT_OFFSET } else { 0.0 };
        if target_offset != self.crouch_target_offset {
            self.crouch_from_offset = self.crouch_view_offset;
            self.crouch_target_offset = target_offset;
            self.crouch_anchor_ms = now_ms;
        }
        let duration_ms = if self.crouch_target_offset == 0.0 {
            CROUCH_UP_MS
        } else {
            CROUCH_DOWN_MS
        };
        let t = smoothstep01((now_ms - self.crouch_anchor_ms) / duration_ms);
        self.crouch_view_offset = self.crouch_from_offset + (self.crouch_target_offset - self.crouch_from_offset) * t;
    }
}
