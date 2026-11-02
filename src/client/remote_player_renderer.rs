//! wash-fps `client/RemotePlayerRenderer.ts`: remote players drawn as the
//! Vanguard model, placed, styled and animated from their interpolated
//! state.
//!
//! Ported so far: the body's placement (an anchor at the feet, turned by
//! the yaw, the model scaled [`MODEL_SCALE`]), the clip choice and its
//! speed ratio (`updateModel`), and the visual style (team tint and
//! outline). Not yet: the hitbox, the weapon in hand, one-shot clips,
//! powerup glow, teleport effects, PVS and animation LOD.

use std::collections::HashMap;

use shagger::animation::player_model_assets::{VANGUARD_ANIMATIONS, VANGUARD_MODEL, VANGUARD_ROOT};
use shagger::animation::player_model_manager::{PlayerModel, PlayerModelInstance, PlayerModelVisualStyle};
use shagger::loader::model::glb::GlbError;
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
use shagger::rendering::glam::{Mat4, Quat, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer, Outline};

use super::remote_player_logic::{IDLE_SPEED_THRESHOLD, derive_animation};

/// Mixamo metres to Quake units.
pub const MODEL_SCALE: f32 = 36.0;
/// Feet against the floor; the anchor sits at origin + `PLAYER_MINS_Z`.
pub const MODEL_Y_OFFSET: f64 = 0.0;
pub const PLAYER_MINS_Z: f64 = -24.0;

// Mixamo clips' natural speeds (Quake units a second at 1x).
pub const WALK_NATURAL_SPEED: f64 = 40.0;
pub const RUN_NATURAL_SPEED: f64 = 100.0;
pub const STRAFE_NATURAL_SPEED: f64 = 29.0;
/// Climb rate the ladder clip reads naturally at 1x.
pub const CLIMB_NATURAL_SPEED: f64 = 120.0;
/// `MovementProfile.jumpVelocity`, for the jump clip's scrub.
pub const JUMP_VELOCITY: f64 = 320.0;

/// Where the anchor stands: the feet, dropped further while crouching or
/// sliding (`updateModel`).
#[must_use]
pub fn feet_y(interp: &InterpolatedEntity) -> f64 {
    let mut feet = interp.y + PLAYER_MINS_Z + MODEL_Y_OFFSET;
    if interp.sliding {
        feet -= 20.0;
    } else if interp.crouching {
        feet -= 10.0;
    }
    feet
}

/// The model's world matrix: anchor at the feet turned by the yaw, the
/// mesh scaled with no rotation (`createInstance`, `updateModel`).
#[must_use]
pub fn body_matrix(interp: &InterpolatedEntity) -> Mat4 {
    Mat4::from_translation(Vec3::new(interp.x as f32, feet_y(interp) as f32, interp.z as f32))
        * Mat4::from_quat(Quat::from_rotation_y(interp.yaw as f32))
        * Mat4::from_scale(Vec3::splat(MODEL_SCALE))
}

/// The locomotion clip's speed ratio, matching the feet to the movement
/// (`updateModel`), times the demo's time scale.
#[must_use]
pub fn animation_speed_ratio(anim: &str, interp: &InterpolatedEntity, time_scale: f64) -> f64 {
    let speed = (interp.vx * interp.vx + interp.vz * interp.vz).sqrt();
    let fwd_x = interp.yaw.sin();
    let fwd_z = interp.yaw.cos();
    let forward_dot = fwd_x * interp.vx + fwd_z * interp.vz;
    let right_dot = fwd_z * interp.vx - fwd_x * interp.vz;
    let ratio = match anim {
        "run" | "walk" => {
            let natural = if anim == "run" {
                RUN_NATURAL_SPEED
            } else {
                WALK_NATURAL_SPEED
            };
            let mut r = speed / natural;
            if forward_dot < 0.0 {
                r = -r;
            }
            r.clamp(-3.0, 3.0)
        }
        "strafe_l" | "strafe_r" | "crouch_strafe_l" | "crouch_strafe_r" => {
            (right_dot.abs() / STRAFE_NATURAL_SPEED).clamp(0.3, 3.0)
        }
        "jump" => {
            // Ascending plays forward to the tucked pose, descending
            // reverses back; frozen at the peak.
            if interp.vy > 10.0 {
                (interp.vy / JUMP_VELOCITY * 2.0).max(0.3)
            } else if interp.vy < -10.0 {
                (interp.vy / JUMP_VELOCITY * 1.5).min(-0.2)
            } else {
                0.0
            }
        }
        "climb" => (interp.vy / CLIMB_NATURAL_SPEED).clamp(-2.0, 2.0),
        "slide" => (speed / 400.0).clamp(0.3, 2.0),
        "crouch_walk" => {
            if speed < IDLE_SPEED_THRESHOLD {
                0.0
            } else {
                let mut r = speed / WALK_NATURAL_SPEED;
                if forward_dot < 0.0 {
                    r = -r;
                }
                r.clamp(-3.0, 3.0)
            }
        }
        _ => 1.0,
    };
    ratio * time_scale
}

/// The player model, uploaded once, and every remote player's pose.
pub struct RemotePlayerRenderer {
    pub model: PlayerModel,
    handle: ModelHandle,
    instances: HashMap<String, PlayerModelInstance>,
}

impl RemotePlayerRenderer {
    /// Loads Vanguard and its clips through `read` (package paths) and
    /// uploads the mesh.
    pub fn load(
        gpu: &Gpu,
        models: &mut ModelRenderer,
        read: impl FnMut(&str) -> Option<Vec<u8>>,
    ) -> Result<Self, GlbError> {
        let model = PlayerModel::load(read, VANGUARD_ROOT, VANGUARD_MODEL, VANGUARD_ANIMATIONS)?;
        let handle = models.upload(gpu, &model.model, |_| ModelMaterialMode::Lit);
        Ok(Self {
            model,
            handle,
            instances: HashMap::new(),
        })
    }

    /// Moves one player on by `dt` seconds: picks its clip and speed and
    /// poses it (`updateModel`). `clip` forces a clip instead of the derived
    /// one, as a one-shot would.
    pub fn update(&mut self, id: &str, interp: &InterpolatedEntity, dt: f64, time_scale: f64, clip: Option<&str>) {
        let instance = self
            .instances
            .entry(id.to_owned())
            .or_insert_with(|| self.model.create_instance());
        let anim = clip.unwrap_or_else(|| derive_animation(interp));
        self.model.play_animation(instance, anim);
        instance.speed_ratio = animation_speed_ratio(anim, interp, time_scale);
        self.model.advance(instance, dt);
    }

    /// Holds a player's clip at a frame (`goToFrame`), for comparing poses.
    pub fn go_to_frame(&mut self, id: &str, frame: f64) {
        if let Some(instance) = self.instances.get_mut(id) {
            self.model.go_to_frame(instance, frame);
        }
    }

    /// Keeps only the players `keep` accepts.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.instances.retain(|id, _| keep(id));
    }

    /// Forgets a player who left.
    pub fn remove(&mut self, id: &str) {
        self.instances.remove(id);
    }

    /// One player's draws, appending its joint palettes to `joints`.
    pub fn draws(
        &self,
        id: &str,
        interp: &InterpolatedEntity,
        style: &PlayerModelVisualStyle,
        joints: &mut Vec<Mat4>,
        draws: &mut Vec<ModelDraw>,
    ) {
        let Some(instance) = self.instances.get(id) else {
            return;
        };
        let world = body_matrix(interp);
        let mut palettes: HashMap<usize, u32> = HashMap::new();
        for (index, primitive) in self.model.model.primitives.iter().enumerate() {
            let Some(skin) = &primitive.skin else { continue };
            let offset = *palettes.entry(skin.skin).or_insert_with(|| {
                let at = joints.len() as u32;
                joints.extend(self.model.joint_matrices(instance, skin.skin));
                at
            });
            draws.push(ModelDraw {
                tint: style.tint_color,
                skin: Some(offset),
                outline: style.outline_enabled.then_some(Outline {
                    color: style.outline_color,
                    width: style.outline_width,
                }),
                ..ModelDraw::new(self.handle, index, world)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_ratio_follows_the_feet() {
        let run = InterpolatedEntity {
            vz: 300.0,
            grounded: true,
            ..Default::default()
        };
        assert_eq!(animation_speed_ratio("run", &run, 1.0), 3.0);
        let back = InterpolatedEntity { vz: -80.0, ..run };
        assert_eq!(animation_speed_ratio("walk", &back, 1.0), -2.0);
        let up = InterpolatedEntity { vy: 160.0, ..run };
        assert_eq!(animation_speed_ratio("jump", &up, 0.5), 0.5);
        assert_eq!(animation_speed_ratio("idle", &run, 1.0), 1.0);
    }

    #[test]
    fn crouching_lowers_the_anchor() {
        let mut p = InterpolatedEntity {
            y: 100.0,
            ..Default::default()
        };
        assert_eq!(feet_y(&p), 76.0);
        p.crouching = true;
        assert_eq!(feet_y(&p), 66.0);
        p.sliding = true;
        assert_eq!(feet_y(&p), 56.0);
    }
}
