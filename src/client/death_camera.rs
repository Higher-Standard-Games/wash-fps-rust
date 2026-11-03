//! Port of wash-fps `client/DeathCamera.ts`: after the local player dies,
//! the view eases to a chase position behind their ragdoll's hips or their
//! gib burst's centre, with guard rails so a ragdoll gone wrong cannot drag
//! it under the map.
//!
//! The TypeScript reads the target through a getter each frame; here the
//! caller passes the target's position to [`DeathCamera::update`] (none once
//! it is gone). Its wall clamp picks the map's meshes with a ray; here a
//! trace through the map's brushes with the shot mask stands in, which
//! the caller supplies as a closure.

use shagger::rendering::glam::DVec3;

/// Distance the camera sits behind the corpse or burst.
pub const BACK_DISTANCE: f64 = 150.0;
/// Height the camera floats above the target.
pub const HEIGHT_ABOVE: f64 = 110.0;
/// How far above the target to aim.
pub const LOOK_HEIGHT: f64 = 25.0;
pub const POS_SMOOTH: f64 = 3.2;
pub const LOOK_SMOOTH: f64 = 5.0;
/// A target this far below the death origin has escaped.
pub const MAX_TARGET_DROP_BELOW_DEATH: f64 = 72.0;
/// A target this far from the death origin has escaped.
pub const MAX_TARGET_DISTANCE_FROM_DEATH: f64 = 900.0;
pub const MIN_CAMERA_HEIGHT_ABOVE_DEATH: f64 = 18.0;
pub const MIN_LOOK_HEIGHT_ABOVE_DEATH: f64 = 8.0;
/// Pull the camera this far in front of the map it hits.
pub const CAMERA_WALL_PADDING: f64 = 18.0;
/// At least this far from the target after a wall clamp.
pub const MIN_CAMERA_TARGET_DISTANCE: f64 = 36.0;

/// A ray against the map: from, to, and where it hits.
pub type MapTrace<'a> = &'a mut dyn FnMut(DVec3, DVec3) -> Option<DVec3>;

/// `DeathCamera`.
#[derive(Debug, Clone, Default)]
pub struct DeathCamera {
    active: bool,
    yaw: f64,
    death_pos: DVec3,
    cam_pos: DVec3,
    look_pos: DVec3,
    last_target: DVec3,
}

impl DeathCamera {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// `start`: from behind `death_pos` as the player faced (`yaw`).
    pub fn start(&mut self, death_pos: DVec3, yaw: f64) {
        self.active = true;
        self.yaw = yaw;
        self.death_pos = death_pos;
        self.last_target = death_pos;
        self.cam_pos = death_pos + self.back_vector() * BACK_DISTANCE;
        self.cam_pos.y = (self.cam_pos.y + HEIGHT_ABOVE).max(self.min_camera_y());
        self.look_pos = death_pos;
        self.look_pos.y = (self.look_pos.y + LOOK_HEIGHT).max(self.min_look_y());
    }

    /// `stop`.
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// `update`: eases toward `target` (the follow position, if any) and
    /// returns where the camera is and what it looks at, or none when
    /// inactive.
    pub fn update(&mut self, dt: f64, target: Option<DVec3>, trace: Option<MapTrace<'_>>) -> Option<(DVec3, DVec3)> {
        if !self.active {
            return None;
        }
        let target = self.sanitize_target(target);
        self.last_target = target;
        let desired_look = DVec3::new(target.x, (target.y + LOOK_HEIGHT).max(self.min_look_y()), target.z);
        let mut desired = target + self.back_vector() * BACK_DISTANCE;
        desired.y = (desired.y + HEIGHT_ABOVE).max(self.min_camera_y());
        let mut safe = clamp_camera_to_world(trace, desired_look, desired);
        safe.y = safe.y.max(self.min_camera_y());
        let kp = 1.0 - (-POS_SMOOTH * dt).exp();
        let kl = 1.0 - (-LOOK_SMOOTH * dt).exp();
        // `Vector3.LerpToRef`: start + (end - start) * amount.
        self.cam_pos += (safe - self.cam_pos) * kp;
        self.look_pos += (desired_look - self.look_pos) * kl;
        Some((self.cam_pos, self.look_pos))
    }

    fn sanitize_target(&self, target: Option<DVec3>) -> DVec3 {
        let Some(target) = target.filter(|t| t.is_finite()) else {
            return self.last_target;
        };
        if target.y < self.death_pos.y - MAX_TARGET_DROP_BELOW_DEATH
            || target.distance_squared(self.death_pos) > MAX_TARGET_DISTANCE_FROM_DEATH * MAX_TARGET_DISTANCE_FROM_DEATH
        {
            return self.last_target;
        }
        target
    }

    fn min_camera_y(&self) -> f64 {
        self.death_pos.y + MIN_CAMERA_HEIGHT_ABOVE_DEATH
    }

    fn min_look_y(&self) -> f64 {
        self.death_pos.y + MIN_LOOK_HEIGHT_ABOVE_DEATH
    }

    /// Behind the player for their death yaw.
    fn back_vector(&self) -> DVec3 {
        DVec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }
}

/// `clampCameraToWorld`: in front of the first wall between the target
/// and the camera, never closer than [`MIN_CAMERA_TARGET_DISTANCE`].
fn clamp_camera_to_world(trace: Option<MapTrace<'_>>, look: DVec3, desired: DVec3) -> DVec3 {
    let Some(trace) = trace else { return desired };
    let offset = desired - look;
    let distance = offset.length();
    if distance <= MIN_CAMERA_TARGET_DISTANCE {
        return desired;
    }
    let dir = offset * (1.0 / distance);
    let Some(hit) = trace(look, look + dir * distance) else {
        return desired;
    };
    let clamped = (hit.distance(look) - CAMERA_WALL_PADDING).max(MIN_CAMERA_TARGET_DISTANCE);
    if clamped >= distance {
        return desired;
    }
    look + dir * clamped
}

/// The yaw and pitch (positive looks down) that look from `from` at `to`,
/// Babylon's `setTarget` for a free camera.
#[must_use]
pub fn look_angles(from: DVec3, to: DVec3) -> (f64, f64) {
    let d = to - from;
    let yaw = d.x.atan2(d.z);
    let pitch = (-d.y).atan2((d.x * d.x + d.z * d.z).sqrt());
    (yaw, pitch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_behind_and_above() {
        let mut camera = DeathCamera::default();
        assert!(camera.update(0.016, None, None).is_none());
        camera.start(DVec3::new(0.0, 100.0, 0.0), 0.0);
        let (cam, look) = camera.update(0.0, None, None).unwrap();
        assert!((cam - DVec3::new(0.0, 210.0, -150.0)).length() < 1e-9);
        assert!((look - DVec3::new(0.0, 125.0, 0.0)).length() < 1e-9);
        camera.stop();
        assert!(!camera.is_active());
    }

    #[test]
    fn eases_after_the_target_and_ignores_escapes() {
        let mut camera = DeathCamera::default();
        camera.start(DVec3::ZERO, 0.0);
        let (cam, _) = camera.update(1.0, Some(DVec3::new(100.0, 0.0, 0.0)), None).unwrap();
        let k = 1.0 - (-3.2f64).exp();
        assert!((cam.x - 100.0 * k).abs() < 1e-9);
        // Fallen through the floor: the last good target holds.
        let (cam2, _) = camera.update(1.0, Some(DVec3::new(0.0, -500.0, 0.0)), None).unwrap();
        assert!(cam2.x > cam.x);
        assert!(cam2.y >= MIN_CAMERA_HEIGHT_ABOVE_DEATH);
    }

    #[test]
    fn walls_pull_the_camera_in() {
        let mut camera = DeathCamera::default();
        camera.start(DVec3::ZERO, 0.0);
        let mut wall = |from: DVec3, to: DVec3| Some(from + (to - from).normalize() * 100.0);
        for _ in 0..200 {
            camera.update(0.1, Some(DVec3::ZERO), Some(&mut wall));
        }
        let (cam, look) = camera.update(0.1, Some(DVec3::ZERO), Some(&mut wall)).unwrap();
        assert!((cam.distance(look) - 82.0).abs() < 0.5, "{}", cam.distance(look));
    }

    #[test]
    fn looks_where_it_aims() {
        let (yaw, pitch) = look_angles(DVec3::ZERO, DVec3::new(0.0, -1.0, 1.0));
        assert!(yaw.abs() < 1e-12);
        assert!((pitch - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }
}
