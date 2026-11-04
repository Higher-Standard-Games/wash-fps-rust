//! wash-fps `camera/FPSController.ts`, its view half: how the first-person
//! camera moves with the body on top of the mouse. Each frame the eye eases
//! up steps, dips into a slide, lifts through a ledge grab, kicks with our
//! shots, shakes with explosions and hits, and takes the view effects (walk
//! bob, landing, strafe lean, speed and aim FOV, idle breathing). The result
//! is an offset on the rendered camera. The aim sent to the server takes
//! the slide's dip and the ledge grab's lift with it, as the browser's does
//! (it sends the camera's pitch less the view kick), so the crosshair stays
//! where the shots go; the kick, shake and effects stay off it.

use shagger::camera::camera_shake::CameraShake;
use shagger::camera::first_person_motion::{ease_ledge_pitch, slide_pitch, smooth_step_up};
use shagger::camera::view_kick::ViewKick;
use shagger::rendering::glam::DVec3;
use shagger::shared::spread_utils::mulberry32;
use shagger::viewport::{FirstPersonEffectStack, PlayerState, ViewCvars, WallJumpState, apply_fps};

use shagger::weapon::explosion::explosion_batch_renderer::ExplosionLook;
use shagger::weapon::explosion_effect::explosion_shake;

use crate::shared::entities::definitions::weapons::weapons_table::weapon_view_kick;
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
    /// `state.horizontalSpeed`, which the weapon bob reads.
    pub horizontal_speed: f64,
    /// `state.bobCycle`: the walk bob's cycle, which the weapon bob follows.
    pub bob_cycle: f64,
    /// `state.landingVelocity`: the last landing's fall speed.
    pub landing_velocity: f64,
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
pub struct FirstPersonView {
    effects: FirstPersonEffectStack,
    pub kick: ViewKick,
    pub shake: CameraShake,
    /// `Math.random` for the shake.
    random: Box<dyn FnMut() -> f64>,
    smoothed_y: Option<f64>,
    ledge_grab_snap_frames: u32,
    ledge_pitch: f64,
    /// The slide's dip and the ledge grab's lift as last drawn: the part of
    /// the view's pitch the aim carries.
    pub aim_pitch: f64,
    is_grounded: bool,
    landing_velocity_capture: f64,
}

impl Default for FirstPersonView {
    fn default() -> Self {
        Self {
            effects: FirstPersonEffectStack::default(),
            kick: ViewKick::default(),
            shake: CameraShake::default(),
            random: Box::new(mulberry32(0x5eed)),
            smoothed_y: None,
            ledge_grab_snap_frames: 0,
            ledge_pitch: 0.0,
            aim_pitch: 0.0,
            is_grounded: false,
            landing_velocity_capture: 0.0,
        }
    }
}

impl std::fmt::Debug for FirstPersonView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirstPersonView")
            .field("kick", &self.kick)
            .field("shake", &self.shake)
            .field("smoothed_y", &self.smoothed_y)
            .finish_non_exhaustive()
    }
}

impl FirstPersonView {
    /// Forgets the body: a respawn or a new match.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `shakeCamera(intensity, "explosion")`: an explosion went off at
    /// `at`, near `player` (the predicted body), with `cg_screenshake` on.
    pub fn explosion_at(&mut self, look: &ExplosionLook, at: DVec3, player: DVec3, screenshake: bool) {
        if !screenshake {
            return;
        }
        if let Some(intensity) = explosion_shake(look, at.distance(player)) {
            self.shake.shake(intensity);
        }
    }

    /// `kickLocalView`: our own shot with `weapon_id` kicks the view,
    /// blended from hip to aimed by `ads_progress`, with a random sideways
    /// part.
    pub fn kick_for_shot(&mut self, weapon_id: &str, ads_progress: f64) {
        let Some(kick) = weapon_view_kick(weapon_id) else {
            return;
        };
        let ads = ads_progress.clamp(0.0, 1.0);
        let pitch = kick.pitch + (kick.ads_pitch - kick.pitch) * ads;
        let yaw = kick.yaw + (kick.ads_yaw - kick.yaw) * ads;
        let side = (self.random)() * 2.0 - 1.0;
        self.kick.kick(pitch, side * yaw, kick.recover_seconds);
    }

    /// `shakeCamera(intensity, "hit")`: the pain shake for `damage` from
    /// `weapon_id`, with `cg_hitshake` on (`GameEvents`).
    pub fn hit(&mut self, damage: f64, weapon_id: &str, hitshake: bool) {
        if !hitshake {
            return;
        }
        let pain = (damage / 200.0).clamp(0.025, 0.3);
        let scale = match weapon_id {
            "plasmagun" | "plasma_bolt" => 0.5,
            "voltaic" | "lasergun" => 0.7,
            _ => 1.0,
        };
        self.shake.shake(pain * scale);
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
        self.aim_pitch = slide + self.ledge_pitch;
        self.kick.settle(dt);
        // This frame shows the shake's last offsets, then it moves on
        // (`update`'s order).
        let (shake_pitch, shake_yaw) = (self.shake.pitch, self.shake.yaw);
        self.shake.update(dt, &mut self.random);

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
            pitch: slide + self.ledge_pitch + shake_pitch + self.kick.pitch + r.x,
            yaw: shake_yaw + self.kick.yaw + r.y,
            roll: r.z,
            fov_multiplier: delta.fov_multiplier,
            local_offset: delta.position_offset,
            horizontal_speed: body.horizontal_speed,
            bob_cycle: self.effects.walk_bob.shared_cycle,
            landing_velocity: self.landing_velocity_capture,
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

    #[test]
    fn the_slide_dip_aims_too_and_the_kick_does_not() {
        let mut v = FirstPersonView::default();
        let c = ViewCvars::default();
        let k = ViewKeys::default();
        let mut slide = body(0.0);
        slide.sliding = true;
        slide.slide_phase = 1.0;
        v.kick_for_shot("machinegun", 0.0);
        let o = v.update(0.016, &slide, &k, &c, 18.0, 0.3);
        assert!((v.aim_pitch + 0.22).abs() < 1e-9, "{}", v.aim_pitch);
        // The kick is drawn (up is negative) but left out of the aim.
        assert!(o.pitch < -0.22 - 1e-3, "{}", o.pitch);
        // Other guns do not kick.
        let mut still = FirstPersonView::default();
        still.kick_for_shot("rlauncher", 0.0);
        assert_eq!(still.kick.pitch, 0.0);
    }

    #[test]
    fn a_close_rocket_shakes_and_a_hit_only_with_hitshake() {
        let c = ViewCvars::default();
        let k = ViewKeys::default();
        let mut v = FirstPersonView::default();
        let rocket = &shagger::weapon::explosion_effect::ROCKET_EXPLOSION;
        v.explosion_at(rocket, DVec3::new(50.0, 0.0, 0.0), DVec3::ZERO, false);
        assert_eq!(v.shake.intensity, 0.0);
        v.explosion_at(rocket, DVec3::new(50.0, 0.0, 0.0), DVec3::ZERO, true);
        assert!(v.shake.intensity > 0.5);
        // The first frame shows the shake's previous (still) offsets.
        let first = v.update(0.016, &body(0.0), &k, &c, 18.0, 0.3);
        let second = v.update(0.016, &body(0.0), &k, &c, 18.0, 0.3);
        assert!(
            first.yaw.abs() < 1e-12 && second.yaw != 0.0,
            "{} {}",
            first.yaw,
            second.yaw
        );
        assert_eq!(v.aim_pitch, 0.0, "the shake stays off the aim");

        let mut h = FirstPersonView::default();
        h.hit(100.0, "rlauncher", false);
        assert_eq!(h.shake.intensity, 0.0);
        h.hit(100.0, "plasmagun", true);
        // Capped at 0.3, halved for plasma.
        assert!((h.shake.intensity - 0.15).abs() < 1e-12, "{}", h.shake.intensity);
    }

    #[test]
    fn the_weapon_bob_follows_the_view_bob_and_the_landing() {
        let mut v = FirstPersonView::default();
        let c = ViewCvars::default();
        let k = ViewKeys::default();
        let mut run = body(0.0);
        run.horizontal_speed = 320.0;
        let o = v.update(0.016, &run, &k, &c, 18.0, 0.3);
        // 320 u/s at cg_bobfrequency 0.03 for 16 ms.
        assert!((o.bob_cycle - 320.0 * 0.016 * 0.03).abs() < 1e-12, "{}", o.bob_cycle);
        assert_eq!(o.horizontal_speed, 320.0);
        let mut air = body(0.0);
        air.grounded = false;
        v.update(0.016, &air, &k, &c, 18.0, 0.3);
        let mut land = body(0.0);
        land.velocity.y = -450.0;
        assert_eq!(v.update(0.016, &land, &k, &c, 18.0, 0.3).landing_velocity, 450.0);
    }
}
