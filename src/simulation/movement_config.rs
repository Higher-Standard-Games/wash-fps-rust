//! Movement tuning and the extended result (wash-fps `simulation/MovementConfig.ts`).

use crate::liquid::liquid_types::{LiquidKind, WaterLevel};
use shagger::shared::server::simulation::types::Vec3;

/// `MovementConfig`. Times are as the TypeScript has them: wall jump and
/// ledge timings in seconds, slide timings in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementConfig {
    pub max_speed: f64,
    pub run_speed: f64,
    pub ground_acceleration: f64,
    pub air_acceleration: f64,
    pub max_air_speed: f64,
    pub friction: f64,
    pub stop_speed: f64,
    pub gravity: f64,
    pub falling_gravity_multiplier: f64,
    pub jump_velocity: f64,
    pub crouch_multiplier: f64,
    pub step_height: f64,
    pub top_speed: f64,
    pub air_control: f64,
    pub air_control_power: f64,
    pub wall_jump_enabled: bool,
    pub wall_jump_height: f64,
    pub wall_jump_bounce: f64,
    pub wall_jump_reach: f64,
    pub wall_jump_cooldown: f64,
    pub wall_jump_min_height: f64,
    pub wall_jump_charge_time: f64,
    pub wall_jump_boost: f64,
    pub ledge_grab_enabled: bool,
    pub ledge_reach: f64,
    pub ledge_height: f64,
    pub ledge_min_fall: f64,
    pub ledge_timeout: f64,
    pub ledge_pull_time: f64,
    pub slide_enabled: bool,
    pub slide_duration: f64,
    pub slide_speed_multiplier: f64,
    pub slide_decay: f64,
    pub slide_jump_boost: f64,
    pub slide_min_speed: f64,
    pub slide_friction: f64,
    pub slide_cooldown: f64,
    /// 0: a fixed exit speed (Q3); 1: the full 3D speed along the exit yaw.
    pub teleport_speed_mode: f64,
    pub teleport_exit_speed: f64,
}

/// Wall jump state as the result reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WallJumpPhase {
    #[default]
    None,
    Available,
    Charging,
}

impl WallJumpPhase {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Available => "available",
            Self::Charging => "charging",
        }
    }
}

/// Ledge grab state as the result reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LedgeGrabPhase {
    #[default]
    None,
    Grabbing,
    Pulling,
}

impl LedgeGrabPhase {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Grabbing => "grabbing",
            Self::Pulling => "pulling",
        }
    }
}

/// `AdvancedMovementResult`. The optional fields are `None` where the
/// TypeScript leaves them undefined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdvancedMovementResult {
    pub position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
    pub is_sliding: bool,
    pub is_crouching: bool,
    pub wall_jump_state: WallJumpPhase,
    pub ledge_grab_state: LedgeGrabPhase,
    pub actual_horiz_speed: f64,
    pub crouch_view_offset: f64,
    pub slide_phase: f64,
    pub ledge_grab_phase: f64,
    pub jumppad_launch: Option<bool>,
    pub teleported: Option<bool>,
    pub teleport_yaw: Option<f64>,
    pub teleport_from: Option<Vec3>,
    pub trigger_id: Option<usize>,
    pub water_level: Option<WaterLevel>,
    pub liquid_contents: Option<u32>,
    pub liquid_kind: Option<Option<LiquidKind>>,
    pub on_ladder: Option<bool>,
    pub ladder_climb_delta: Option<f64>,
}

impl AdvancedMovementResult {
    /// A result with the step-end fields left unset.
    #[must_use]
    pub fn base(position: Vec3, velocity: Vec3, grounded: bool) -> Self {
        Self {
            position,
            velocity,
            grounded,
            is_sliding: false,
            is_crouching: false,
            wall_jump_state: WallJumpPhase::None,
            ledge_grab_state: LedgeGrabPhase::None,
            actual_horiz_speed: 0.0,
            crouch_view_offset: 0.0,
            slide_phase: 0.0,
            ledge_grab_phase: 0.0,
            jumppad_launch: None,
            teleported: None,
            teleport_yaw: None,
            teleport_from: None,
            trigger_id: None,
            water_level: None,
            liquid_contents: None,
            liquid_kind: None,
            on_ladder: None,
            ladder_climb_delta: None,
        }
    }
}
