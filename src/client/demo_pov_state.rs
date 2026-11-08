//! wash-fps `client/DemoPovState.ts`: the first-person view of a watched
//! player, in demo playback or live spectating.
//!
//! Snapshots and demos carry the body's movement, not the player's camera
//! phase. This rebuilds it by running the local camera's own view effects
//! over the sampled entity on the clock it is handed (the demo's playhead,
//! so a pause holds still and a seek starts over), and `watched_eye_pose`
//! puts the camera at their eyes with it.

use shagger::camera::first_person_motion::{ease_ledge_pitch, slide_pitch, smooth_step_up};
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
use shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT;
use shagger::rendering::glam::DVec3;
use shagger::viewport::{FirstPersonEffectStack, PlayerState, ViewCvars, ViewportDelta, WallJumpState};

/// The eye drop of a crouch or slide when the sample has no offset.
const FALLBACK_CROUCH_VIEW_OFFSET: f64 = -24.0;
/// Frames a ledge grab keeps the eye following exactly after it ends.
const GRAB_SNAP_FRAMES: u32 = 15;

/// `DemoPovPresentation`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemoPovPresentation {
    pub eye_height: f64,
    /// Their screen's view effects, on this clock.
    pub effects: ViewportDelta,
    /// Their eye on its way up a step: the origin's height, eased.
    pub smoothed_y: f64,
    /// Their slide dip and ledge lift, radians, added to their pitch.
    pub pitch_offset: f64,
    pub bob_cycle: f64,
    pub delta_seconds: f64,
    pub horizontal_speed: f64,
    pub velocity_y: f64,
    pub grounded: bool,
    pub sliding: bool,
    pub landing_velocity: f64,
}

/// `DemoPovState`.
#[derive(Debug, Default)]
pub struct DemoPovState {
    effects: FirstPersonEffectStack,
    last_entity_id: Option<String>,
    started: bool,
    was_grounded: bool,
    last_velocity_y: f64,
    ledge_pitch: f64,
    smoothed_y: Option<f64>,
    grab_snap_frames: u32,
    aiming: bool,
}

/// The crouch's eye drop: the sample's, else the fallback while crouched
/// or sliding.
fn crouch_offset(sample: &InterpolatedEntity) -> f64 {
    sample
        .crouch_view_offset
        .unwrap_or(if sample.crouching || sample.sliding {
            FALLBACK_CROUCH_VIEW_OFFSET
        } else {
            0.0
        })
}

impl DemoPovState {
    /// `setAiming`: the view zooms as theirs did.
    pub fn set_aiming(&mut self, aiming: bool) {
        self.aiming = aiming;
    }

    /// `update`: `delta_seconds` on the clock (0 while paused);
    /// `discontinuous` after a seek. Another entity also starts over.
    /// `step_height` is `sv_stepheight`, `smooth_factor` `cl_smoothfactor`.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        entity_id: &str,
        sample: &InterpolatedEntity,
        delta_seconds: f64,
        discontinuous: bool,
        cvars: &ViewCvars,
        step_height: f64,
        smooth_factor: f64,
    ) -> DemoPovPresentation {
        let discontinuous = discontinuous || !self.started || self.last_entity_id.as_deref() != Some(entity_id);
        if discontinuous {
            self.effects.reset();
            self.ledge_pitch = 0.0;
            self.smoothed_y = Some(sample.y);
            self.grab_snap_frames = 0;
            self.was_grounded = sample.grounded;
            self.last_velocity_y = sample.vy;
        }
        let dt = if discontinuous { 0.0 } else { delta_seconds.max(0.0) };

        let horizontal_speed = sample.vx.hypot(sample.vz);
        let landing_velocity = if !self.was_grounded && sample.grounded {
            self.last_velocity_y.abs()
        } else {
            0.0
        };
        let state = PlayerState {
            position: DVec3::new(sample.x, sample.y, sample.z),
            velocity: DVec3::new(sample.vx, sample.vy, sample.vz),
            horizontal_speed,
            is_grounded: sample.grounded,
            was_grounded_last_frame: self.was_grounded,
            is_jumping: false,
            strafe_input: f64::from(sample.strafe_input.unwrap_or(0)),
            forward_input: f64::from(sample.forward_input.unwrap_or(0)),
            landing_velocity: Some(landing_velocity),
            wall_jump_state: if sample.wall_jumping {
                WallJumpState::Charging
            } else {
                WallJumpState::None
            },
            is_sprinting: false,
            is_aiming: self.aiming,
            is_firing: false,
            is_sliding: sample.sliding,
        };
        let effects = self.effects.update(cvars, dt, &state);

        // The eye's own motion, on the same clock: still while paused.
        let grabbing = sample.ledge_grab_phase.unwrap_or(0.0) > 0.01;
        if dt > 0.0 || self.smoothed_y.is_none() {
            if grabbing {
                self.grab_snap_frames = GRAB_SNAP_FRAMES;
            } else {
                self.grab_snap_frames = self.grab_snap_frames.saturating_sub(1);
            }
            let from = self.smoothed_y.unwrap_or(sample.y);
            self.smoothed_y = Some(smooth_step_up(
                from,
                sample.y,
                step_height,
                smooth_factor,
                grabbing || self.grab_snap_frames > 0,
            ));
            self.ledge_pitch = ease_ledge_pitch(self.ledge_pitch, sample.ledge_grab_phase.unwrap_or(0.0), dt);
        }

        self.last_entity_id = Some(entity_id.to_owned());
        self.started = true;
        // A held frame keeps its landing edge for the frame that moves.
        if dt > 0.0 || discontinuous {
            self.was_grounded = sample.grounded;
            self.last_velocity_y = sample.vy;
        }

        DemoPovPresentation {
            eye_height: DEFAULT_VIEWHEIGHT + crouch_offset(sample),
            effects,
            smoothed_y: self.smoothed_y.unwrap_or(sample.y),
            pitch_offset: slide_pitch(sample.slide_phase.unwrap_or(0.0)) + self.ledge_pitch,
            bob_cycle: self.effects.walk_bob.shared_cycle,
            delta_seconds: dt,
            horizontal_speed,
            velocity_y: sample.vy,
            grounded: sample.grounded,
            sliding: sample.sliding,
            landing_velocity,
        }
    }

    /// `reset`.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// `FirstPersonPose`: where the camera goes and how it looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyePose {
    pub position: DVec3,
    pub yaw: f64,
    pub pitch: f64,
    pub effects: Option<ViewportDelta>,
}

/// `watchedEyePose`: the sample they are drawn with, at their eye height,
/// with their screen's effects.
#[must_use]
pub fn watched_eye_pose(sample: &InterpolatedEntity, pov: Option<&DemoPovPresentation>) -> EyePose {
    let eye_height = pov.map_or(DEFAULT_VIEWHEIGHT + crouch_offset(sample), |p| p.eye_height);
    EyePose {
        position: DVec3::new(sample.x, pov.map_or(sample.y, |p| p.smoothed_y) + eye_height, sample.z),
        yaw: sample.yaw,
        pitch: sample.pitch.unwrap_or(0.0) + pov.map_or(0.0, |p| p.pitch_offset),
        effects: pov.map(|p| p.effects),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner(x: f64) -> InterpolatedEntity {
        InterpolatedEntity {
            x,
            vx: 320.0,
            grounded: true,
            ..InterpolatedEntity::default()
        }
    }

    #[test]
    fn their_stride_bobs_on_the_clock_and_holds_when_paused() {
        let mut pov = DemoPovState::default();
        let c = ViewCvars::default();
        let first = pov.update("p1", &runner(0.0), 0.016, false, &c, 18.0, 0.3);
        // The first frame starts over: no time passes.
        assert_eq!(first.delta_seconds, 0.0);
        assert_eq!(first.bob_cycle, 0.0);
        let moving = pov.update("p1", &runner(5.0), 0.016, false, &c, 18.0, 0.3);
        assert!(moving.bob_cycle > 0.0);
        assert_eq!(moving.horizontal_speed, 320.0);
        let paused = pov.update("p1", &runner(5.0), 0.0, false, &c, 18.0, 0.3);
        assert_eq!(paused.bob_cycle, moving.bob_cycle);
        // Someone else: over again.
        assert_eq!(
            pov.update("p2", &runner(5.0), 0.016, false, &c, 18.0, 0.3).bob_cycle,
            0.0
        );
    }

    #[test]
    fn a_landing_is_felt_once() {
        let mut pov = DemoPovState::default();
        let c = ViewCvars::default();
        let air = InterpolatedEntity {
            vy: -450.0,
            grounded: false,
            ..InterpolatedEntity::default()
        };
        pov.update("p", &air, 0.016, false, &c, 18.0, 0.3);
        pov.update("p", &air, 0.016, false, &c, 18.0, 0.3);
        let land = InterpolatedEntity {
            grounded: true,
            ..InterpolatedEntity::default()
        };
        assert_eq!(
            pov.update("p", &land, 0.016, false, &c, 18.0, 0.3).landing_velocity,
            450.0
        );
        assert_eq!(
            pov.update("p", &land, 0.016, false, &c, 18.0, 0.3).landing_velocity,
            0.0
        );
    }

    #[test]
    fn the_eye_sits_at_their_view_height_and_crouches() {
        let mut s = InterpolatedEntity {
            y: 100.0,
            pitch: Some(0.2),
            ..InterpolatedEntity::default()
        };
        let pose = watched_eye_pose(&s, None);
        assert_eq!(pose.position.y, 100.0 + DEFAULT_VIEWHEIGHT);
        assert_eq!(pose.pitch, 0.2);
        s.crouching = true;
        assert_eq!(watched_eye_pose(&s, None).position.y, 100.0 + DEFAULT_VIEWHEIGHT - 24.0);
    }
}
