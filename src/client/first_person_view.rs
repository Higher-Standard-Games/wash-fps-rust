//! wash-fps `camera/FPSController.ts`, its view half: how the first-person
//! camera moves with the body on top of the mouse. Each frame the eye eases
//! up steps, dips into a slide, lifts through a ledge grab, and takes the
//! view effects (walk bob, landing, strafe lean, speed and aim FOV, idle
//! breathing). The result is an offset on the rendered camera only; the
//! aim sent to the server stays the mouse's.

use shagger::camera::first_person_motion::{ease_ledge_pitch, slide_pitch, smooth_step_up};
use shagger::camera::view_kick::ViewKick;
use shagger::rendering::glam::DVec3;
use shagger::viewport::{FirstPersonEffectStack, PlayerState, ViewCvars, WallJumpState, apply_fps};

use crate::simulation::movement_config::{LedgeGrabPhase, WallJumpPhase};

/// What the controller reads about the body this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct BodyFrame {
    /// The predicted feet, with the correction smoothing.
    pub feet: DVec3,
    /// `DEFAULT_VIEWHEIGHT + crouchViewOffset`.
    pub eye_lift: f64,
    pub velocity: DVec3,
    pub grounded: bool,
    pub sliding: bool,
    pub horizontal_speed: f64,
    pub slide_phase: f64,
    pub ledge_grab_phase: f64,
    pub ledge_grab: LedgeGrabPhase,
    pub wall_jump: WallJumpPhase,
}

/// The keys the effects read.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewKeys {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sprint: bool,
    pub aim: bool,
    pub fire: bool,
}

/// The rendered camera's difference from the mouse's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOffset {
    /// The eye, before the effects' offset.
    pub eye: DVec3,
    /// Radians: added to the mouse's pitch and yaw; roll is absolute.
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
    pub fov_multiplier: f64,
    /// The effects' offset in camera space (right, world up, forward).
    pub local_offset: DVec3,
}

impl ViewOffset {
    /// `FPSApplicator.apply` onto a camera looking along `forward` with
    /// `right` to its side: the eye in the world.
    #[must_use]
    pub fn eye_in_world(&self, right: DVec3, forward: DVec3) -> DVec3 {
        let delta = shagger::viewport::ViewportDelta {
            position_offset: self.local_offset,
            ..Default::default()
        };
        self.eye + apply_fps(&delta, right, forward).0
    }
}

/// The view state carried across frames.
#[derive(Debug, Default)]
pub struct FirstPersonView {
    effects: FirstPersonEffectStack,
    pub kick: ViewKick,
    smoothed_y: Option<f64>,
    ledge_grab_snap_frames: u32,
    ledge_pitch: f64,
    is_grounded: bool,
    landing_velocity_capture: f64,
}

impl FirstPersonView {
    /// Forgets the body: a respawn or a new match.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// One frame: `step_height` is `sv_stepheight`, `smooth_factor`
    /// `cl_smoothfactor`.
    pub fn update(
        &mut self,
        dt: f64,
        body: &BodyFrame,
        keys: &ViewKeys,
        cvars: &ViewCvars,
        step_height: f64,
        smooth_factor: f64,
    ) -> ViewOffset {
        // Stair smoothing; a ledge grab and a short grace after it follow
        // the body exactly so the pull's boost does not lag.
        let in_grab = body.ledge_grab != LedgeGrabPhase::None;
        if in_grab {
            self.ledge_grab_snap_frames = 15;
        } else {
            self.ledge_grab_snap_frames = self.ledge_grab_snap_frames.saturating_sub(1);
        }
        let snap = in_grab || self.ledge_grab_snap_frames > 0;
        let smoothed = match self.smoothed_y {
            Some(y) => smooth_step_up(y, body.feet.y, step_height, smooth_factor, snap),
            None => body.feet.y,
        };
        self.smoothed_y = Some(smoothed);
        let eye = DVec3::new(body.feet.x, smoothed + body.eye_lift, body.feet.z);

        let was_grounded = self.is_grounded;
        self.is_grounded = body.grounded;
        if !was_grounded && body.grounded && body.velocity.y < -50.0 {
            self.landing_velocity_capture = body.velocity.y.abs();
        }

        let slide = slide_pitch(body.slide_phase);
        self.ledge_pitch = ease_ledge_pitch(self.ledge_pitch, body.ledge_grab_phase, dt);
        self.kick.settle(dt);

        let axis = |plus: bool, minus: bool| f64::from(u8::from(plus)) - f64::from(u8::from(minus));
        let state = PlayerState {
            position: eye,
            velocity: body.velocity,
            horizontal_speed: body.horizontal_speed,
            is_grounded: body.grounded,
            was_grounded_last_frame: was_grounded,
            is_jumping: keys.jump && body.grounded,
            strafe_input: axis(keys.right, keys.left),
            forward_input: axis(keys.forward, keys.backward),
            landing_velocity: Some(self.landing_velocity_capture),
            wall_jump_state: match body.wall_jump {
                WallJumpPhase::None => WallJumpState::None,
                WallJumpPhase::Available => WallJumpState::Available,
                WallJumpPhase::Charging => WallJumpState::Charging,
            },
            is_sprinting: keys.sprint,
            is_aiming: keys.aim,
            is_firing: keys.fire,
            is_sliding: body.sliding,
        };
        let delta = self.effects.update(cvars, dt, &state);
        let r = delta.rotation_offset * (std::f64::consts::PI / 180.0);
        ViewOffset {
            eye,
            pitch: slide + self.ledge_pitch + self.kick.pitch + r.x,
            yaw: self.kick.yaw + r.y,
            roll: r.z,
            fov_multiplier: delta.fov_multiplier,
            local_offset: delta.position_offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(y: f64) -> BodyFrame {
        BodyFrame {
            feet: DVec3::new(0.0, y, 0.0),
            eye_lift: 26.0,
            grounded: true,
            ..BodyFrame::default()
        }
    }

    #[test]
    fn steps_ease_in_and_falls_follow() {
        let mut v = FirstPersonView::default();
        let c = ViewCvars::default();
        let k = ViewKeys::default();
        assert_eq!(v.update(0.016, &body(0.0), &k, &c, 18.0, 0.3).eye.y, 26.0);
        // Up a step: a third of the way this frame.
        let up = v.update(0.016, &body(10.0), &k, &c, 18.0, 0.3).eye.y;
        assert!((up - 29.0).abs() < 1e-9, "{up}");
        // Down: at once.
        assert_eq!(v.update(0.016, &body(-5.0), &k, &c, 18.0, 0.3).eye.y, 21.0);
    }

    #[test]
    fn sliding_dips_and_landing_drops_the_eye() {
        let mut v = FirstPersonView::default();
        let c = ViewCvars::default();
        let k = ViewKeys::default();
        let mut slide = body(0.0);
        slide.sliding = true;
        slide.slide_phase = 1.0;
        let o = v.update(0.016, &slide, &k, &c, 18.0, 0.3);
        assert!((o.pitch + 0.22).abs() < 1e-9, "{}", o.pitch);

        let mut v = FirstPersonView::default();
        let mut air = body(0.0);
        air.grounded = false;
        v.update(0.016, &air, &k, &c, 18.0, 0.3);
        let mut land = body(0.0);
        land.velocity.y = -600.0;
        let o = v.update(0.05, &land, &k, &c, 18.0, 0.3);
        assert!(o.local_offset.y < -3.0, "{}", o.local_offset.y);
    }

    #[test]
    fn running_bobs_and_strafing_rolls() {
        let mut v = FirstPersonView::default();
        let c = ViewCvars::default();
        let keys = ViewKeys {
            right: true,
            ..ViewKeys::default()
        };
        let mut run = body(0.0);
        run.horizontal_speed = 320.0;
        let mut heights = Vec::new();
        let mut o = v.update(0.016, &run, &keys, &c, 18.0, 0.3);
        for _ in 0..60 {
            o = v.update(0.016, &run, &keys, &c, 18.0, 0.3);
            heights.push(o.local_offset.y);
        }
        let span = heights.iter().copied().fold(f64::MIN, f64::max) - heights.iter().copied().fold(f64::MAX, f64::min);
        assert!(span > 1.5, "{span}");
        // Rolled into the strafe, plus the bob's own roll.
        assert!(o.roll < 0.0, "{}", o.roll);
    }
}
