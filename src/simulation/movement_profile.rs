//! The Q3 and Warsow movement step (wash-fps `simulation/MovementProfile.ts`):
//! ground and air acceleration, swimming, ladders, wall jump, ledge grab,
//! crouchslide, and the map's teleporters and jumppads.
//!
//! Every timer runs on the sim clock (`simStep * dt`), so a reconciliation
//! replay recomputes the same values. The struct's fields are the state a
//! step carries to the next; a clone is `saveState`.

use super::acceleration::{
    air_accelerate_forward, apply_friction, ground_accelerate, normal_air_accelerate, strafe_bunny_accelerate,
};
use super::ladder::{
    DEFAULT_LADDER_MOVEMENT, LadderMovementProfile, apply_ladder_friction, ladder_accelerate, ladder_intent,
};
use super::ledge_grab::LedgeGrab;
use super::movement_config::{AdvancedMovementResult, LedgeGrabPhase, MovementConfig, WallJumpPhase};
use super::slide::Slide;
use super::swim::{
    DEFAULT_LIQUID_MOVEMENT, LiquidMovementProfile, apply_water_friction, check_water_jump, idle_sink_velocity,
    swim_intent, wade_speed_scale, water_accelerate,
};
use super::vec3_helpers::{VEC3_ZERO, vec3_length, vec3_length_xz};
use super::wall_jump::{SlideView, WallJump};
use crate::liquid::liquid_types::{LiquidSample, WaterLevel, is_swimming};
use crate::liquid::liquid_volume_sampler::{LiquidSampleBounds, sample_liquid};
use shagger::network::components::input_component::input_buttons;
use shagger::network::systems::movement::movement_profile::{
    MovementDefaults, MovementInput, MovementProfile as MovementProfileTrait,
};
use shagger::physics::player_physics_constants::{CROUCH_MINS_Z, CROUCH_VIEWHEIGHT, DEFAULT_VIEWHEIGHT, PLAYER_MINS_Z};
use shagger::shared::bsp::trigger_table::{TriggerType, TriggerVolume};
use shagger::shared::collision::collision_world::{Aabb, CollisionWorld, MASK_LIQUID, MASK_SOLID};
use shagger::shared::js::{js_cos, js_sin};
use shagger::shared::server::simulation::types::Vec3;

/// `DEFAULT_FPS_MOVEMENT_CONFIG`: the one tuning the server and client use.
pub const DEFAULT_FPS_MOVEMENT_CONFIG: MovementConfig = MovementConfig {
    max_speed: 280.0,
    run_speed: 400.0,
    ground_acceleration: 10.0,
    air_acceleration: 1.0,
    max_air_speed: 30.0,
    friction: 6.0,
    stop_speed: 100.0,
    gravity: 1000.0,
    falling_gravity_multiplier: 1.25,
    jump_velocity: 320.0,
    crouch_multiplier: 0.5,
    step_height: 18.0,
    top_speed: 1000.0,
    air_control: 150.0,
    air_control_power: 50.0,
    wall_jump_enabled: true,
    wall_jump_height: 280.0,
    wall_jump_bounce: 1.2,
    wall_jump_reach: 20.0,
    wall_jump_cooldown: 1.0,
    wall_jump_min_height: 32.0,
    wall_jump_charge_time: 0.2,
    wall_jump_boost: 1.5,
    ledge_grab_enabled: true,
    ledge_reach: 32.0,
    ledge_height: 64.0,
    ledge_min_fall: -50.0,
    ledge_timeout: 2.0,
    ledge_pull_time: 0.5,
    slide_enabled: true,
    slide_duration: 800.0,
    slide_speed_multiplier: 1.4,
    slide_decay: 0.5,
    slide_jump_boost: 1.3,
    slide_min_speed: 375.0,
    slide_friction: 0.1,
    slide_cooldown: 1500.0,
    teleport_speed_mode: 1.0,
    teleport_exit_speed: 400.0,
};

/// Q3's asymmetric box: the origin at the waist, 24 down to the feet.
pub const PLAYER_BOUNDS: Aabb = Aabb {
    min: Vec3::new(-16.0, -24.0, -16.0),
    max: Vec3::new(16.0, 32.0, 16.0),
};
pub const PLAYER_BOUNDS_CROUCHED: Aabb = Aabb {
    min: Vec3::new(-16.0, -24.0, -16.0),
    max: Vec3::new(16.0, 8.0, 16.0),
};
/// A moving swimmer lies flat: the same radius, less height.
pub const PLAYER_BOUNDS_SWIMMING: Aabb = Aabb {
    min: Vec3::new(-16.0, -14.0, -16.0),
    max: Vec3::new(16.0, 14.0, 16.0),
};

/// Feet (24) plus 1 of clearance: teleporter destinations sit on the floor.
const TELEPORT_EXIT_LIFT: f64 = 25.0;

/// Reborrow the collision world for one call (`as_deref_mut` cannot shorten
/// the trait object's lifetime).
fn re<'a>(collision: &'a mut Option<&mut dyn CollisionWorld>) -> Option<&'a mut dyn CollisionWorld> {
    match collision {
        Some(c) => Some(&mut **c),
        None => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct JumppadVolume {
    id: usize,
    min: Vec3,
    max: Vec3,
    dest: Vec3,
}

/// `MovementProfileState`: what a profile carries from one step to the next
/// besides position and velocity; see [`MovementProfile::save_state`].
#[derive(Debug, Clone, PartialEq)]
pub struct MovementProfileState {
    pub is_grounded: bool,
    pub was_grounded_last_frame: bool,
    pub jump_consumed: bool,
    pub water_level: WaterLevel,
    pub water_jump_until_ms: f64,
    pub prone_in_water: bool,
    pub in_ladder_volume: bool,
    pub climbing: bool,
    pub ladder_detach_until_ms: f64,
    pub sim_now_ms: f64,
    pub actual_horiz_speed: f64,
    pub smoothed_y: f64,
    pub wall_jump: WallJump,
    pub slide: Slide,
    pub ledge_grab: LedgeGrab,
}

/// `MovementProfile`.
#[derive(Debug, Clone, PartialEq)]
pub struct MovementProfile {
    pub config: MovementConfig,
    pub is_grounded: bool,
    pub was_grounded_last_frame: bool,
    pub jump_consumed: bool,
    /// Water level entering the step; re-derived from position every step.
    pub water_level: WaterLevel,
    /// Sim-clock end of an assisted climb out of water, or 0.
    pub water_jump_until_ms: f64,
    pub prone_in_water: bool,
    pub liquid: LiquidMovementProfile,
    /// The hull overlaps a ladder and the detach cooldown is over.
    pub in_ladder_volume: bool,
    /// Climbing is this step's movement mode.
    pub climbing: bool,
    pub ladder_detach_until_ms: f64,
    pub ladder: LadderMovementProfile,
    pub sim_now_ms: f64,
    pub actual_horiz_speed: f64,
    pub smoothed_y: f64,
    pub wall_jump: WallJump,
    pub slide: Slide,
    pub ledge_grab: LedgeGrab,
    /// External speed bonus (+25 with the blade), on walk and run speed.
    pub speed_bonus: f64,
    jumppads: Vec<JumppadVolume>,
    teleporters: Vec<TriggerVolume>,
}

impl Default for MovementProfile {
    fn default() -> Self {
        Self::new(DEFAULT_FPS_MOVEMENT_CONFIG)
    }
}

impl MovementProfile {
    #[must_use]
    pub fn new(config: MovementConfig) -> Self {
        Self {
            config,
            is_grounded: false,
            was_grounded_last_frame: false,
            jump_consumed: false,
            water_level: 0,
            water_jump_until_ms: 0.0,
            prone_in_water: false,
            liquid: DEFAULT_LIQUID_MOVEMENT,
            in_ladder_volume: false,
            climbing: false,
            ladder_detach_until_ms: 0.0,
            ladder: DEFAULT_LADDER_MOVEMENT,
            sim_now_ms: 0.0,
            actual_horiz_speed: 0.0,
            smoothed_y: 0.0,
            wall_jump: WallJump::default(),
            slide: Slide::default(),
            ledge_grab: LedgeGrab::default(),
            speed_bonus: 0.0,
            jumppads: Vec::new(),
            teleporters: Vec::new(),
        }
    }

    /// `setTriggerVolumes`: teleporters, and jumppads whose target is above
    /// the pad's centre (the rest are dropped).
    pub fn set_trigger_volumes(&mut self, volumes: &[TriggerVolume]) {
        self.jumppads.clear();
        self.teleporters.clear();
        for v in volumes {
            if v.kind == TriggerType::Teleport {
                self.teleporters.push(*v);
                continue;
            }
            let center_y = (v.min.y + v.max.y) / 2.0;
            if v.dest.y - center_y <= 0.0 {
                continue;
            }
            self.jumppads.push(JumppadVolume {
                id: v.id,
                min: v.min,
                max: v.max,
                dest: v.dest,
            });
        }
    }

    /// `getDeterminismFingerprint`: every config field as sorted `key=value`
    /// pairs, numbers printed as JavaScript prints them.
    #[must_use]
    pub fn determinism_fingerprint(&self) -> String {
        let c = &self.config;
        let n = |v: f64| shagger::shared::js::js_number_to_string(v);
        let b = |v: bool| v.to_string();
        let mut fields = [
            ("maxSpeed", n(c.max_speed)),
            ("runSpeed", n(c.run_speed)),
            ("groundAcceleration", n(c.ground_acceleration)),
            ("airAcceleration", n(c.air_acceleration)),
            ("maxAirSpeed", n(c.max_air_speed)),
            ("friction", n(c.friction)),
            ("stopSpeed", n(c.stop_speed)),
            ("gravity", n(c.gravity)),
            ("fallingGravityMultiplier", n(c.falling_gravity_multiplier)),
            ("jumpVelocity", n(c.jump_velocity)),
            ("crouchMultiplier", n(c.crouch_multiplier)),
            ("stepHeight", n(c.step_height)),
            ("topSpeed", n(c.top_speed)),
            ("airControl", n(c.air_control)),
            ("airControlPower", n(c.air_control_power)),
            ("wallJumpEnabled", b(c.wall_jump_enabled)),
            ("wallJumpHeight", n(c.wall_jump_height)),
            ("wallJumpBounce", n(c.wall_jump_bounce)),
            ("wallJumpReach", n(c.wall_jump_reach)),
            ("wallJumpCooldown", n(c.wall_jump_cooldown)),
            ("wallJumpMinHeight", n(c.wall_jump_min_height)),
            ("wallJumpChargeTime", n(c.wall_jump_charge_time)),
            ("wallJumpBoost", n(c.wall_jump_boost)),
            ("ledgeGrabEnabled", b(c.ledge_grab_enabled)),
            ("ledgeReach", n(c.ledge_reach)),
            ("ledgeHeight", n(c.ledge_height)),
            ("ledgeMinFall", n(c.ledge_min_fall)),
            ("ledgeTimeout", n(c.ledge_timeout)),
            ("ledgePullTime", n(c.ledge_pull_time)),
            ("slideEnabled", b(c.slide_enabled)),
            ("slideDuration", n(c.slide_duration)),
            ("slideSpeedMultiplier", n(c.slide_speed_multiplier)),
            ("slideDecay", n(c.slide_decay)),
            ("slideJumpBoost", n(c.slide_jump_boost)),
            ("slideMinSpeed", n(c.slide_min_speed)),
            ("slideFriction", n(c.slide_friction)),
            ("slideCooldown", n(c.slide_cooldown)),
            ("teleportSpeedMode", n(c.teleport_speed_mode)),
            ("teleportExitSpeed", n(c.teleport_exit_speed)),
        ];
        // localeCompare on these ASCII camelCase keys orders case-insensitively.
        fields.sort_by_key(|a| a.0.to_lowercase());
        fields
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// `setGrounded`.
    /// `saveState`: everything this profile carries from one step to the
    /// next. Tuning, map volumes and the speed bonus are inputs, not state.
    /// A reconciliation replay starts from the state the acknowledged
    /// command left.
    #[must_use]
    pub fn save_state(&self) -> MovementProfileState {
        MovementProfileState {
            is_grounded: self.is_grounded,
            was_grounded_last_frame: self.was_grounded_last_frame,
            jump_consumed: self.jump_consumed,
            water_level: self.water_level,
            water_jump_until_ms: self.water_jump_until_ms,
            prone_in_water: self.prone_in_water,
            in_ladder_volume: self.in_ladder_volume,
            climbing: self.climbing,
            ladder_detach_until_ms: self.ladder_detach_until_ms,
            sim_now_ms: self.sim_now_ms,
            actual_horiz_speed: self.actual_horiz_speed,
            smoothed_y: self.smoothed_y,
            wall_jump: self.wall_jump.clone(),
            slide: self.slide,
            ledge_grab: self.ledge_grab,
        }
    }

    /// `loadState`: resume from a state taken by [`Self::save_state`].
    pub fn load_state(&mut self, state: &MovementProfileState) {
        self.is_grounded = state.is_grounded;
        self.was_grounded_last_frame = state.was_grounded_last_frame;
        self.jump_consumed = state.jump_consumed;
        self.water_level = state.water_level;
        self.water_jump_until_ms = state.water_jump_until_ms;
        self.prone_in_water = state.prone_in_water;
        self.in_ladder_volume = state.in_ladder_volume;
        self.climbing = state.climbing;
        self.ladder_detach_until_ms = state.ladder_detach_until_ms;
        self.sim_now_ms = state.sim_now_ms;
        self.actual_horiz_speed = state.actual_horiz_speed;
        self.smoothed_y = state.smoothed_y;
        self.wall_jump = state.wall_jump.clone();
        self.slide = state.slide;
        self.ledge_grab = state.ledge_grab;
    }

    pub fn set_grounded(&mut self, grounded: bool) {
        self.is_grounded = grounded;
    }

    /// `resetPhysicsState`: keeps the slide and an active ledge grab.
    pub fn reset_physics_state(&mut self, grounded: bool) {
        self.is_grounded = grounded;
        self.was_grounded_last_frame = grounded;
        if grounded {
            self.jump_consumed = false;
        }
        self.wall_jump.reset();
    }

    fn current_bounds(&self) -> Aabb {
        if self.prone_in_water {
            PLAYER_BOUNDS_SWIMMING
        } else if self.slide.is_crouching || self.slide.is_sliding {
            PLAYER_BOUNDS_CROUCHED
        } else {
            PLAYER_BOUNDS
        }
    }

    fn sample_at(&self, collision: Option<&mut dyn CollisionWorld>, origin: Vec3) -> LiquidSample {
        let crouched = self.slide.is_crouching || self.slide.is_sliding;
        sample_liquid(
            collision,
            origin,
            LiquidSampleBounds {
                mins_y: if crouched { CROUCH_MINS_Z } else { PLAYER_MINS_Z },
                view_height: if crouched {
                    CROUCH_VIEWHEIGHT
                } else {
                    DEFAULT_VIEWHEIGHT
                },
            },
        )
    }

    fn sample_ladder_at(&self, collision: Option<&mut dyn CollisionWorld>, origin: Vec3) -> bool {
        let Some(collision) = collision else { return false };
        if is_swimming(self.water_level) || self.sim_now_ms < self.ladder_detach_until_ms {
            return false;
        }
        let b = self.current_bounds();
        collision.is_in_ladder(
            Vec3::new(origin.x + b.min.x, origin.y + b.min.y, origin.z + b.min.z),
            Vec3::new(origin.x + b.max.x, origin.y + b.max.y, origin.z + b.max.z),
        )
    }

    /// `simulate`: one step, with the liquid and ladder sampled before (to
    /// choose the branch) and after (for the result and the next step).
    pub fn simulate(
        &mut self,
        position: Vec3,
        velocity: Vec3,
        input: Option<&MovementInput>,
        delta_time: f64,
        mut collision: Option<&mut dyn CollisionWorld>,
    ) -> AdvancedMovementResult {
        self.water_level = self.sample_at(re(&mut collision), position).level;
        self.in_ladder_volume = self.sample_ladder_at(re(&mut collision), position);

        let mut result = self.simulate_core(position, velocity, input, delta_time, re(&mut collision));

        let liquid = self.sample_at(re(&mut collision), result.position);
        self.water_level = liquid.level;
        result.water_level = Some(liquid.level);
        result.liquid_contents = Some(liquid.raw_contents);
        result.liquid_kind = Some(liquid.kind);

        self.in_ladder_volume = self.sample_ladder_at(re(&mut collision), result.position);
        result.on_ladder = Some(self.climbing);
        result.ladder_climb_delta = Some(if self.climbing {
            result.position.y - position.y
        } else {
            0.0
        });
        result
    }

    #[allow(clippy::too_many_lines)]
    fn simulate_core(
        &mut self,
        position: Vec3,
        velocity: Vec3,
        input: Option<&MovementInput>,
        delta_time: f64,
        mut collision: Option<&mut dyn CollisionWorld>,
    ) -> AdvancedMovementResult {
        let mut new_pos = position;
        let mut new_vel = velocity;
        let dt_ms = delta_time * 1000.0;

        if let Some(step) = input.and_then(|i| i.sim_step).filter(|&s| s >= 0.0) {
            self.sim_now_ms = step * dt_ms;
        } else if let Some(sequence) = input.and_then(|i| i.sequence).filter(|&s| s > 0.0) {
            self.sim_now_ms = sequence * dt_ms;
        } else {
            self.sim_now_ms += dt_ms;
        }
        let now_ms = self.sim_now_ms;
        self.wall_jump.begin_step(now_ms);

        let just_landed = !self.was_grounded_last_frame && self.is_grounded;

        if is_swimming(self.water_level) || self.in_ladder_volume {
            if self.wall_jump.state != WallJumpPhase::None {
                self.wall_jump.reset();
            }
            if self.ledge_grab.state != LedgeGrabPhase::None {
                self.ledge_grab.reset();
            }
            if self.slide.is_sliding {
                self.slide.exit(now_ms, Some(&self.config));
            }
        }

        if self.wall_jump.state == WallJumpPhase::Charging {
            let view = SlideView {
                is_sliding: self.slide.is_sliding,
                is_crouching: self.slide.is_crouching,
                crouch_view_offset: self.slide.crouch_view_offset,
                slide_phase: self.slide.slide_phase,
            };
            let ledge = (self.ledge_grab.state, self.ledge_grab.phase);
            return self.wall_jump.handle_charging(
                input,
                now_ms,
                &self.config,
                view,
                ledge,
                self.actual_horiz_speed,
                &mut self.slide,
            );
        }

        if self.config.ledge_grab_enabled
            && !is_swimming(self.water_level)
            && !self.in_ladder_volume
            && let Some(input) = input
        {
            let grounded = self.is_grounded;
            if self.ledge_grab.update(
                &mut new_pos,
                &mut new_vel,
                input,
                re(&mut collision),
                now_ms,
                &self.config,
                grounded,
            ) {
                return AdvancedMovementResult {
                    is_crouching: self.slide.is_crouching,
                    ledge_grab_state: self.ledge_grab.state,
                    crouch_view_offset: self.slide.crouch_view_offset,
                    ledge_grab_phase: self.ledge_grab.phase,
                    ..AdvancedMovementResult::base(new_pos, new_vel, false)
                };
            }
        }

        let swimming = is_swimming(self.water_level);
        let water_jumping = self.water_jump_until_ms > now_ms;
        if !swimming {
            self.prone_in_water = false;
        }

        let climb_intent = match input {
            Some(input) if self.in_ladder_volume => Some(ladder_intent(
                input.direction,
                input.view_angles.pitch,
                input.view_angles.yaw,
                if input.is_pressed(input_buttons::CROUCH) {
                    -1.0
                } else {
                    0.0
                },
                &self.ladder,
            )),
            _ => None,
        };
        let intent_up = climb_intent.and_then(|i| i.dir).map_or(0.0, |d| d.y);
        self.climbing = self.in_ladder_volume && (!self.is_grounded || intent_up > 0.1);

        if swimming && !water_jumping || self.climbing {
            // Held up by the water, or by the rungs.
        } else if !self.is_grounded {
            let grav_scale = if new_vel.y < 0.0 {
                self.config.falling_gravity_multiplier
            } else {
                1.0
            };
            new_vel.y -= self.config.gravity * grav_scale * delta_time;
        } else if new_vel.y < 0.0 {
            new_vel.y = 0.0;
        }

        if let Some(input) = input {
            let is_aiming = input.is_pressed(input_buttons::ALT_FIRE);
            let is_firing = input.is_pressed(input_buttons::FIRE);
            let is_crouching = input.is_pressed(input_buttons::CROUCH);
            let is_jumping = input.is_pressed(input_buttons::JUMP);
            let can_run = !is_aiming && !is_firing && !is_crouching;
            let is_sprinting = can_run
                && input.is_pressed(input_buttons::SPRINT)
                && input.is_pressed(input_buttons::FORWARD)
                && !input.is_pressed(input_buttons::BACK);
            let forward_only = input.is_pressed(input_buttons::FORWARD)
                && !input.is_pressed(input_buttons::BACK)
                && !input.is_pressed(input_buttons::LEFT)
                && !input.is_pressed(input_buttons::RIGHT);
            let mut wishspeed = if is_sprinting && forward_only {
                self.config.run_speed + self.speed_bonus
            } else {
                self.config.max_speed + self.speed_bonus
            };

            if is_crouching && !self.slide.is_sliding {
                let horiz_speed = vec3_length_xz(new_vel);
                let cooldown_passed = !self.slide.is_on_cooldown(now_ms);
                if horiz_speed < self.config.slide_min_speed || !cooldown_passed {
                    wishspeed *= self.config.crouch_multiplier;
                }
            }

            if !is_jumping {
                self.jump_consumed = false;
            }
            let can_jump = !self.jump_consumed || just_landed;

            if is_jumping && can_jump && self.climbing {
                new_vel.y = self.ladder.detach_up;
                self.climbing = false;
                self.in_ladder_volume = false;
                self.ladder_detach_until_ms = now_ms + self.ladder.detach_cooldown_ms;
                self.jump_consumed = true;
                self.is_grounded = false;
            }

            if is_jumping && self.is_grounded && can_jump && !swimming && !self.climbing {
                let mut can_jump_on_slope = true;
                if let Some(collision) = re(&mut collision) {
                    let t = collision.trace(
                        new_pos,
                        Vec3::new(new_pos.x, new_pos.y - 2.0, new_pos.z),
                        Some(self.current_bounds()),
                        None,
                    );
                    if t.hit && t.normal.is_some_and(|n| n.y < 0.85) {
                        can_jump_on_slope = false;
                    }
                }
                if can_jump_on_slope {
                    new_vel.y = self.config.jump_velocity;
                    self.jump_consumed = true;
                    self.is_grounded = false;
                    self.wall_jump.key_released = false;
                    if self.slide.is_sliding {
                        self.slide.exit(now_ms, Some(&self.config));
                    }
                }
            }

            let wish_dir = input.direction;
            let wish_len = vec3_length_xz(wish_dir);
            let has_input = wish_len > 0.001;
            let normalized_wish = if has_input {
                Vec3::new(wish_dir.x / wish_len, 0.0, wish_dir.z / wish_len)
            } else {
                VEC3_ZERO
            };

            if water_jumping {
                if self.is_grounded || new_vel.y < 0.0 {
                    self.water_jump_until_ms = 0.0;
                }
            } else if self.climbing {
                apply_ladder_friction(&mut new_vel, &self.ladder, delta_time);
                if let Some(dir) = climb_intent.and_then(|i| i.dir) {
                    let speed = climb_intent.map_or(0.0, |i| i.speed);
                    ladder_accelerate(&mut new_vel, dir, speed, self.ladder.acceleration, delta_time);
                }
            } else if swimming {
                self.update_prone_state(new_pos, has_input || is_jumping != is_crouching, re(&mut collision));
                self.swim_move(new_pos, &mut new_vel, input, wish_dir, delta_time, re(&mut collision));
            } else if self.is_grounded {
                let friction = if self.slide.is_sliding {
                    self.config.slide_friction
                } else {
                    self.config.friction
                };
                apply_friction(&mut new_vel, friction, self.config.stop_speed, delta_time);
                apply_water_friction(
                    &mut new_vel,
                    self.water_level,
                    &self.liquid,
                    self.config.stop_speed,
                    delta_time,
                );
                if has_input {
                    let waded = wishspeed * wade_speed_scale(self.water_level, &self.liquid);
                    ground_accelerate(
                        &mut new_vel,
                        normalized_wish,
                        waded,
                        self.config.ground_acceleration,
                        delta_time,
                    );
                }
            } else {
                let holding_forward = input.is_pressed(input_buttons::FORWARD) || input.is_pressed(input_buttons::BACK);
                let holding_strafe = input.is_pressed(input_buttons::LEFT) || input.is_pressed(input_buttons::RIGHT);
                let velocity_dot_wish = new_vel.x * normalized_wish.x + new_vel.z * normalized_wish.z;
                let is_accelerating = velocity_dot_wish > 0.0;
                if has_input {
                    if holding_forward && !holding_strafe && is_accelerating {
                        air_accelerate_forward(&mut new_vel, normalized_wish, wishspeed, &self.config, delta_time);
                    } else if holding_strafe && !holding_forward {
                        strafe_bunny_accelerate(&mut new_vel, normalized_wish, wishspeed, &self.config, delta_time);
                    } else {
                        normal_air_accelerate(&mut new_vel, normalized_wish, wishspeed, &self.config, delta_time);
                    }
                }
                if self.config.wall_jump_enabled
                    && !self.slide.is_sliding
                    && self.ledge_grab.state == LedgeGrabPhase::None
                    && !self.ledge_grab.is_on_cooldown(now_ms)
                {
                    self.wall_jump
                        .update(new_pos, &mut new_vel, input, re(&mut collision), now_ms, &self.config);
                }
            }

            if self.config.slide_enabled && !swimming && !self.climbing {
                let grounded = self.is_grounded;
                self.slide
                    .update(&mut new_vel, input, grounded, now_ms, delta_time, &self.config);
            }
            self.slide.update_crouch(input, now_ms);
        }

        if let Some(collision) = re(&mut collision) {
            if self.is_grounded {
                let ground = collision.trace(
                    new_pos,
                    Vec3::new(new_pos.x, new_pos.y - 2.0, new_pos.z),
                    Some(self.current_bounds()),
                    None,
                );
                if let Some(n) = ground.normal.filter(|_| ground.hit) {
                    if n.y > 0.7 && n.y < 0.99 {
                        let dot = new_vel.x * n.x + new_vel.y * n.y + new_vel.z * n.z;
                        const OVERCLIP: f64 = 1.001;
                        new_vel.x -= n.x * dot * OVERCLIP;
                        new_vel.y -= n.y * dot * OVERCLIP;
                        new_vel.z -= n.z * dot * OVERCLIP;
                    } else if n.y > 0.0 && n.y <= 0.7 {
                        self.is_grounded = false;
                        let grav_slope = self.config.gravity * delta_time;
                        let grav_dot = -grav_slope * n.y;
                        new_vel.x -= n.x * grav_dot;
                        new_vel.y -= grav_slope - n.y * grav_dot;
                        new_vel.z -= n.z * grav_dot;
                    }
                }
            }
            let result = collision.step_slide_move(new_pos, new_vel, delta_time, self.current_bounds());
            new_pos = result.end_pos;
            new_vel = result.velocity;
            self.was_grounded_last_frame = self.is_grounded;
            self.is_grounded = result.grounded;
        } else {
            new_pos.x += new_vel.x * delta_time;
            new_pos.y += new_vel.y * delta_time;
            new_pos.z += new_vel.z * delta_time;
            self.was_grounded_last_frame = self.is_grounded;
            if new_pos.y < 0.0 {
                new_pos.y = 0.0;
                new_vel.y = 0.0;
                self.is_grounded = true;
            } else {
                self.is_grounded = false;
            }
        }

        let mut jumppad_launch = false;
        let mut teleported = false;
        let mut teleport_yaw = None;
        let mut fired_trigger_id = None;
        let (pre_trigger_x, pre_trigger_z) = (new_pos.x, new_pos.z);
        let bounds = self.current_bounds();
        let overlaps = |p: Vec3, v_min: Vec3, v_max: Vec3| {
            p.x + bounds.min.x <= v_max.x
                && p.x + bounds.max.x >= v_min.x
                && p.y + bounds.min.y <= v_max.y
                && p.y + bounds.max.y >= v_min.y
                && p.z + bounds.min.z <= v_max.z
                && p.z + bounds.max.z >= v_min.z
        };

        let mut teleport_from = None;
        for tele in &self.teleporters {
            if !overlaps(new_pos, tele.min, tele.max) {
                continue;
            }
            teleport_from = Some(new_pos);
            new_pos = Vec3::new(tele.dest.x, tele.dest.y + TELEPORT_EXIT_LIFT, tele.dest.z);
            let yaw = tele.dest_yaw.unwrap_or(0.0);
            let speed = if self.config.teleport_speed_mode == 0.0 {
                self.config.teleport_exit_speed
            } else {
                vec3_length(new_vel)
            };
            new_vel = Vec3::new(js_sin(yaw) * speed, 0.0, js_cos(yaw) * speed);
            self.is_grounded = false;
            teleported = true;
            teleport_yaw = Some(yaw);
            fired_trigger_id = Some(tele.id);
            break;
        }

        if !teleported {
            for pad in &self.jumppads {
                if !overlaps(new_pos, pad.min, pad.max) {
                    continue;
                }
                let height = pad.dest.y - new_pos.y;
                if height <= 0.0 {
                    break;
                }
                let time = (height / (0.5 * self.config.gravity)).sqrt();
                new_vel.x = (pad.dest.x - new_pos.x) / time;
                new_vel.y = self.config.gravity * time;
                new_vel.z = (pad.dest.z - new_pos.z) / time;
                self.is_grounded = false;
                jumppad_launch = true;
                fired_trigger_id = Some(pad.id);
                break;
            }
        }

        let dx = pre_trigger_x - position.x;
        let dz = pre_trigger_z - position.z;
        self.actual_horiz_speed = (dx * dx + dz * dz).sqrt() / delta_time;

        let height_diff = (new_pos.y - self.smoothed_y).abs();
        if height_diff > self.config.step_height * 2.0 {
            self.smoothed_y = new_pos.y;
        } else {
            self.smoothed_y += (new_pos.y - self.smoothed_y) * 0.15;
        }

        AdvancedMovementResult {
            is_sliding: self.slide.is_sliding,
            is_crouching: self.slide.is_crouching,
            wall_jump_state: self.wall_jump.state,
            ledge_grab_state: self.ledge_grab.state,
            actual_horiz_speed: self.actual_horiz_speed,
            crouch_view_offset: self.slide.crouch_view_offset,
            slide_phase: self.slide.slide_phase,
            ledge_grab_phase: self.ledge_grab.phase,
            jumppad_launch: Some(jumppad_launch),
            teleported: Some(teleported),
            teleport_yaw,
            teleport_from,
            trigger_id: fired_trigger_id,
            ..AdvancedMovementResult::base(new_pos, new_vel, self.is_grounded)
        }
    }

    /// `swimMove`: a way out first, then drag, then the player's intent.
    fn swim_move(
        &mut self,
        new_pos: Vec3,
        new_vel: &mut Vec3,
        input: &MovementInput,
        wish_dir: Vec3,
        delta_time: f64,
        collision: Option<&mut dyn CollisionWorld>,
    ) {
        if let Some(collision) = collision {
            let level = self.water_level;
            let world = std::cell::RefCell::new(collision);
            let jump = check_water_jump(
                new_pos,
                input.view_angles.yaw,
                level,
                &mut |p| world.borrow_mut().point_contents(p) & MASK_SOLID != 0,
                &mut |p| world.borrow_mut().point_contents(p) & MASK_LIQUID != 0,
            );
            if jump {
                let yaw = input.view_angles.yaw;
                new_vel.x = js_sin(yaw) * self.liquid.water_jump_forward;
                new_vel.z = js_cos(yaw) * self.liquid.water_jump_forward;
                new_vel.y = self.liquid.water_jump_up;
                self.water_jump_until_ms = self.sim_now_ms + self.liquid.water_jump_duration_ms;
                self.is_grounded = false;
                return;
            }
        }

        apply_water_friction(
            new_vel,
            self.water_level,
            &self.liquid,
            self.config.stop_speed,
            delta_time,
        );
        let up = if input.is_pressed(input_buttons::JUMP) {
            1.0
        } else {
            0.0
        };
        let down = if input.is_pressed(input_buttons::CROUCH) {
            1.0
        } else {
            0.0
        };
        let intent = swim_intent(
            wish_dir,
            input.view_angles.pitch,
            input.view_angles.yaw,
            up - down,
            self.config.max_speed + self.speed_bonus,
            &self.liquid,
        );
        match intent.dir {
            None => {
                let sink = idle_sink_velocity(&self.liquid);
                water_accelerate(
                    new_vel,
                    Vec3::new(0.0, -1.0, 0.0),
                    -sink.y,
                    self.liquid.water_acceleration,
                    delta_time,
                );
            }
            Some(dir) => water_accelerate(new_vel, dir, intent.speed, self.liquid.water_acceleration, delta_time),
        }
    }

    /// `updateProneState`: going prone always works; standing back up needs
    /// room for the upright hull.
    fn update_prone_state(&mut self, position: Vec3, moving: bool, collision: Option<&mut dyn CollisionWorld>) {
        if !is_swimming(self.water_level) {
            self.prone_in_water = false;
            return;
        }
        if moving {
            self.prone_in_water = true;
            return;
        }
        if !self.prone_in_water {
            return;
        }
        let Some(collision) = collision else {
            self.prone_in_water = false;
            return;
        };
        let upright = if self.slide.is_crouching {
            PLAYER_BOUNDS_CROUCHED
        } else {
            PLAYER_BOUNDS
        };
        self.prone_in_water = collision.trace(position, position, Some(upright), None).hit;
    }
}

impl MovementProfileTrait for MovementProfile {
    type Result = AdvancedMovementResult;

    fn simulate(
        &mut self,
        position: Vec3,
        velocity: Vec3,
        input: Option<&MovementInput>,
        delta_time: f64,
        collision: Option<&mut dyn CollisionWorld>,
    ) -> AdvancedMovementResult {
        Self::simulate(self, position, velocity, input, delta_time, collision)
    }

    fn defaults(&self) -> MovementDefaults {
        MovementDefaults {
            max_speed: self.config.max_speed,
            acceleration: self.config.ground_acceleration,
            friction: self.config.friction,
            gravity: self.config.gravity,
            jump_velocity: self.config.jump_velocity,
        }
    }

    fn name(&self) -> &'static str {
        "AdvancedFPSMovement"
    }
}
