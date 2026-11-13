//! wash-fps `client/PostGameShowcase.ts`: the winner's player model during
//! the post-game screen, dancing in front of where the match ended, seen
//! through a camera of its own that frames it in the left third beside the
//! result board.
//!
//! A copy of the winner's body (in their colour as this viewer sees it,
//! without the outline) stands at a clear spot traced from the frozen
//! camera, facing it, with the weapon they last held in the right hand. The
//! diorama camera sits in front of the model, where the player stood, so
//! the shot looks the same every match whatever the map around it. A light
//! of the model's own, from the camera's side and above, keeps it out of
//! the dark; the map keeps its lighting. The view model is not drawn while
//! the diorama camera is up (the browser's `activeCameras` swap drops it).

use shagger::animation::player_model_manager::{DANCE_ANIMS, PlayerModelInstance, PlayerModelVisualStyle};
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
use shagger::rendering::camera::Camera;
use shagger::rendering::glam::{DVec3, Mat4, Vec3};
use shagger::rendering::model_renderer::{KeyLight, ModelDraw};

use crate::client::death_camera::look_angles;
use crate::client::remote_player_renderer::{HandWeapons, PlayerBody, body_draws, body_matrix, held_frame};

/// Distance in front of the camera to place the model (200 frames the
/// whole body with headroom).
const SHOWCASE_DISTANCE: f64 = 200.0;
/// The screen point the placement ray goes through, as fractions of the
/// view (`SHOWCASE_SCREEN_X`, `SHOWCASE_SCREEN_Y`; y down).
const SHOWCASE_SCREEN_X: f64 = 0.5;
const SHOWCASE_SCREEN_Y: f64 = 0.55;
/// Never closer than this (a match ended nose to a wall).
const SHOWCASE_MIN_DISTANCE: f64 = 60.0;
/// How far in front of a blocking wall the model stands.
const SHOWCASE_WALL_PADDING: f64 = 24.0;

/// The diorama camera's distance in front of the model (at most the clear
/// space the placement found).
const DIORAMA_DISTANCE: f64 = 175.0;
/// Its height over the model's origin.
const DIORAMA_EYE_HEIGHT: f64 = 40.0;
/// The height it looks at (the torso).
const DIORAMA_LOOK_HEIGHT: f64 = 36.0;
/// Its vertical field of view.
const DIORAMA_FOV: f64 = 0.8;
/// Where across the screen the model stands: the left third.
const DIORAMA_FRAME_X: f64 = 0.3;

/// The light the model gets from in front and above.
const SHOWCASE_LIGHT_INTENSITY: f32 = 1.15;

/// The winner's model and the diorama camera, from the post-game until the
/// next warmup (`stopPostGameShowcase` is dropping it).
pub struct PostGameShowcase {
    body: PlayerBody,
    instance: PlayerModelInstance,
    world: Mat4,
    style: PlayerModelVisualStyle,
    weapon: Option<String>,
    eye: DVec3,
    look: DVec3,
    light: KeyLight,
}

/// What the showcase starts from: the winner's body and style, the weapon
/// they last held, and the frozen camera with the view's aspect.
pub struct ShowcaseStart<'a> {
    pub body: PlayerBody,
    pub style: PlayerModelVisualStyle,
    pub weapon: Option<String>,
    pub camera: &'a Camera,
    pub aspect: f64,
    /// A number in `[0, 1)` that picks the dance.
    pub pick: f64,
}

impl PostGameShowcase {
    /// `startPostGameShowcase`: places the model, frames it and starts its
    /// dance. `trace` is a ray through the map's brushes and where it hits.
    pub fn start(start: ShowcaseStart<'_>, mut trace: impl FnMut(DVec3, DVec3) -> Option<DVec3>) -> Self {
        let ShowcaseStart {
            body,
            style,
            weapon,
            camera,
            aspect,
            pick,
        } = start;
        // The picking ray through the fixed screen anchor.
        let half_height = (f64::from(camera.fov_y) * 0.5).tan();
        let ndc_x = SHOWCASE_SCREEN_X * 2.0 - 1.0;
        let ndc_y = 1.0 - SHOWCASE_SCREEN_Y * 2.0;
        let dir = (camera.forward().as_dvec3()
            + camera.right().as_dvec3() * (ndc_x * half_height * aspect)
            + camera.up().as_dvec3() * (ndc_y * half_height))
            .normalize_or(DVec3::Z);
        let origin = camera.position.as_dvec3();
        // Pulled in front of a wall the ray meets.
        let mut distance = SHOWCASE_DISTANCE;
        if let Some(hit) = trace(origin, origin + dir * SHOWCASE_DISTANCE) {
            let hit_distance = origin.distance(hit);
            distance = distance.min(SHOWCASE_MIN_DISTANCE.max(hit_distance - SHOWCASE_WALL_PADDING));
        }
        let at = origin + dir * distance;
        // Facing the camera.
        let face_yaw = (origin.x - at.x).atan2(origin.z - at.z);
        let interp = InterpolatedEntity {
            x: at.x,
            y: at.y,
            z: at.z,
            yaw: face_yaw,
            ..InterpolatedEntity::default()
        };
        let world = body_matrix(&interp, &body.model);
        let mut instance = body.model.create_instance();
        body.model.play_animation(&mut instance, "idle");
        // A dance from those shipped, from a shuffled order; none keeps the idle.
        let first = ((pick * DANCE_ANIMS.len() as f64) as usize).min(DANCE_ANIMS.len() - 1);
        for i in 0..DANCE_ANIMS.len() {
            if body
                .model
                .play_animation(&mut instance, DANCE_ANIMS[(first + i) % DANCE_ANIMS.len()])
            {
                break;
            }
        }
        // The diorama camera, in front of the model where the player stood.
        let (face_x, face_z) = (origin.x - at.x, origin.z - at.z);
        let face_len = match face_x.hypot(face_z) {
            l if l > 0.0 => l,
            _ => 1.0,
        };
        let (face_x, face_z) = (face_x / face_len, face_z / face_len);
        let cam_dist = DIORAMA_DISTANCE.min(distance);
        let eye = DVec3::new(
            at.x + face_x * cam_dist,
            at.y + DIORAMA_EYE_HEIGHT,
            at.z + face_z * cam_dist,
        );
        let torso = DVec3::new(at.x, at.y + DIORAMA_LOOK_HEIGHT, at.z);
        // Aimed past the model to its right, so it stands at
        // DIORAMA_FRAME_X across the screen.
        let lateral = (1.0 - 2.0 * DIORAMA_FRAME_X) * cam_dist * (DIORAMA_FOV / 2.0).tan() * aspect;
        let (yaw, _) = look_angles(eye, torso);
        let right = DVec3::new(yaw.cos(), 0.0, -yaw.sin());
        let look = torso + right * lateral;
        Self {
            body,
            instance,
            world,
            style: PlayerModelVisualStyle {
                fullbright_enabled: false,
                fullbright_intensity: 0.0,
                outline_enabled: false,
                ..style
            },
            weapon,
            eye,
            look,
            light: KeyLight {
                direction: Vec3::new(face_x as f32, 1.2, face_z as f32),
                intensity: SHOWCASE_LIGHT_INTENSITY,
            },
        }
    }

    /// Moves the dance on by `dt` seconds.
    pub fn advance(&mut self, dt: f64) {
        self.body.model.advance(&mut self.instance, dt);
    }

    /// Puts `camera` where the diorama camera is.
    pub fn frame(&self, camera: &mut Camera) {
        let (yaw, pitch) = look_angles(self.eye, self.look);
        camera.position = self.eye.as_vec3();
        camera.yaw = yaw as f32;
        camera.pitch = pitch as f32;
        camera.roll = 0.0;
        camera.fov_y = DIORAMA_FOV as f32;
    }

    /// The model's draws and its weapon's, lit by the showcase's light.
    pub fn draws(&self, hand_weapons: &HandWeapons, joints: &mut Vec<Mat4>, draws: &mut Vec<ModelDraw>) {
        let first = draws.len();
        body_draws(&self.body, &self.instance, self.world, &self.style, joints, draws);
        if let (Some(weapon), Some(hand)) = (
            &self.weapon,
            held_frame(&self.body.model, &self.instance, self.world, false),
        ) {
            hand_weapons.draws(weapon, &hand, None, draws);
        }
        for draw in &mut draws[first..] {
            draw.key_light = Some(self.light);
            // Skinned bounds are the bind pose's; a dance leaves them.
            draw.cull = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diorama_frames_the_model_left_of_centre() {
        // A camera at the origin looking along +Z, with nothing in the way.
        let camera = Camera::default();
        let aspect = 16.0 / 9.0;
        let half_height = (f64::from(camera.fov_y) * 0.5).tan();
        let dir = (camera.forward().as_dvec3() + camera.up().as_dvec3() * (-0.1 * half_height)).normalize();
        let at = camera.position.as_dvec3() + dir * SHOWCASE_DISTANCE;
        // Where the model ends up on the diorama camera's screen.
        let (face_x, face_z) = (-at.x, -at.z);
        let len = face_x.hypot(face_z);
        let eye = DVec3::new(
            at.x + face_x / len * DIORAMA_DISTANCE,
            at.y + DIORAMA_EYE_HEIGHT,
            at.z + face_z / len * DIORAMA_DISTANCE,
        );
        let torso = DVec3::new(at.x, at.y + DIORAMA_LOOK_HEIGHT, at.z);
        let lateral = (1.0 - 2.0 * DIORAMA_FRAME_X) * DIORAMA_DISTANCE * (DIORAMA_FOV / 2.0).tan() * aspect;
        let (yaw, _) = look_angles(eye, torso);
        let right = DVec3::new(yaw.cos(), 0.0, -yaw.sin());
        let look = torso + right * lateral;
        let (yaw, pitch) = look_angles(eye, look);
        let diorama = Camera {
            position: eye.as_vec3(),
            yaw: yaw as f32,
            pitch: pitch as f32,
            fov_y: DIORAMA_FOV as f32,
            ..Camera::default()
        };
        let to_model = (torso - eye).normalize();
        let forward = diorama.forward().as_dvec3();
        let x = to_model.dot(diorama.right().as_dvec3()) / to_model.dot(forward);
        let screen_x = 0.5 + 0.5 * x / ((DIORAMA_FOV / 2.0).tan() * aspect);
        assert!((screen_x - DIORAMA_FRAME_X).abs() < 0.02, "{screen_x}");
    }
}
