//! wash-fps `client/RemotePlayerRenderer.ts`: remote players drawn as the
//! Vanguard model, placed, styled and animated from their interpolated
//! state.
//!
//! Ported so far: the body's placement (an anchor at the feet, turned by
//! the yaw, the model scaled [`MODEL_SCALE`]), the clip choice and its
//! speed ratio (`updateModel`), and the visual style (team tint and
//! outline), the quad carrier's glow (`updatePowerupGlow`), and the weapon
//! in hand (`updateWeaponModel`, [`HandWeapons`]). Teleport effects live in
//! `teleport_effect`. Not yet: the hitbox, one-shot clips, PVS and
//! animation LOD.

use std::collections::HashMap;

use shagger::animation::player_model_assets::{VANGUARD_ANIMATIONS, VANGUARD_MODEL, VANGUARD_ROOT};
use shagger::animation::player_model_manager::{PlayerModel, PlayerModelInstance, PlayerModelVisualStyle};
use shagger::loader::model::glb::GlbError;
use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::model::tags_loader::parse_tags;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
use shagger::rendering::glam::{EulerRot, Mat4, Quat, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer, Outline};
use shagger::weapon::weapon_barrels::{
    BarrelDef, barrel_material_mode, barrel_root_matrix, find_barrel_mount, world_weapon_material_mode,
};
use shagger::world::pickup::flattened_pickup_matrices;

use super::quad_damage_visual::{
    QUAD_DAMAGE_COLOR, QUAD_EMISSIVE_MIN, QUAD_EMISSIVE_RANGE, QUAD_PLAYER_OUTLINE_MIN, QUAD_PLAYER_OUTLINE_RANGE,
    QUAD_WORLD_WEAPON_OUTLINE_MIN, QUAD_WORLD_WEAPON_OUTLINE_RANGE, get_quad_damage_pulse, mask_has_quad,
};
use super::remote_player_logic::{IDLE_SPEED_THRESHOLD, derive_animation};
use crate::shared::entities::definitions::weapons::weapons_table::{WEAPON_MODELS, WeaponModel, weapon_model};
use crate::world::fps_pickup::{directory, package_path};

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

/// `updatePowerupGlow`: a quad carrier's body draws glow in the quad colour,
/// lifted from no emissive of their own, and wear its outline, both
/// pulsing at `elapsed` seconds. Anything else is left in its style.
pub fn update_powerup_glow(draws: &mut [ModelDraw], powerup_mask: u32, elapsed: f64) {
    if !mask_has_quad(powerup_mask) {
        return;
    }
    let pulse = get_quad_damage_pulse(elapsed);
    let intensity = (QUAD_EMISSIVE_MIN + QUAD_EMISSIVE_RANGE * pulse) as f32;
    let outline_size = QUAD_PLAYER_OUTLINE_MIN + QUAD_PLAYER_OUTLINE_RANGE * pulse;
    let width = (outline_size / f64::from(MODEL_SCALE * 16.0).max(1.0)) as f32;
    for draw in draws {
        draw.emissive = Some(QUAD_DAMAGE_COLOR * intensity);
        draw.outline = Some(Outline {
            color: QUAD_DAMAGE_COLOR,
            width,
        });
    }
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
    /// The model uploaded to blend, for a ragdoll's fade.
    fade_handle: ModelHandle,
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
        let fade_handle = models.upload(gpu, &model.model, |_| ModelMaterialMode::Glass(1.0));
        Ok(Self {
            model,
            handle,
            fade_handle,
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

    /// Keeps only the players `keep` accepts, handing back the poses of
    /// the rest (a death's ragdoll takes one over).
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) -> Vec<(String, PlayerModelInstance)> {
        let gone: Vec<String> = self.instances.keys().filter(|id| !keep(id)).cloned().collect();
        gone.into_iter()
            .filter_map(|id| self.instances.remove(&id).map(|i| (id, i)))
            .collect()
    }

    /// `detachModel`: a player's pose leaves the renderer (to a ragdoll).
    pub fn take(&mut self, id: &str) -> Option<PlayerModelInstance> {
        self.instances.remove(id)
    }

    /// The lit model and the blending one.
    #[must_use]
    pub fn handles(&self) -> (ModelHandle, ModelHandle) {
        (self.handle, self.fade_handle)
    }

    /// Forgets a player who left.
    pub fn remove(&mut self, id: &str) {
        self.instances.remove(id);
    }

    /// A player's right hand in the world (`mixamorig:RightHand` under the
    /// body), as `attachToBone` follows it.
    #[must_use]
    pub fn hand_world(&self, id: &str, interp: &InterpolatedEntity) -> Option<Mat4> {
        let instance = self.instances.get(id)?;
        let skeleton = &self.model.model.skeleton;
        let hand = skeleton.find(RIGHT_HAND)?;
        let globals = skeleton.global_matrices(&instance.locals);
        Some(body_matrix(interp) * *globals.get(hand)?)
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

/// The bone a gun is held by.
pub const RIGHT_HAND: &str = "mixamorig:RightHand";

/// One weapon loaded for players' hands, with its barrels hung on its
/// mount (relative to the leader).
struct HandWeapon {
    def: &'static WeaponModel,
    handle: ModelHandle,
    model: GltfModel,
    barrels: Vec<(ModelHandle, GltfModel, Mat4)>,
}

/// `updateWeaponModel`'s weapons: every gun's world model, loaded once
/// (wash-fps imports each on first use and clones it per player), and
/// drawn in a player's right hand.
#[derive(Default)]
pub struct HandWeapons {
    weapons: Vec<HandWeapon>,
}

/// The hand-attached leader's local matrix (`attachToBone`, then position
/// zero, `rotation.set(PI, 0, -PI / 2)` and the third-person scale): the
/// view model's scale over the body's 36, halved. The blade
/// (`attachBladeToPlayer`) keeps its whole node tree under a grip turned
/// the other way and moved so its handle sits in the hand.
#[must_use]
pub fn hand_weapon_local(def: &WeaponModel) -> Mat4 {
    let visual_scale = def.view.scale as f32;
    if def.id == "blade" {
        // The right-handed branch: shagger's glTF models are not mirrored.
        Mat4::from_quat(Quat::from_euler(
            EulerRot::YXZ,
            0.0,
            std::f32::consts::PI,
            -std::f32::consts::FRAC_PI_2,
        )) * Mat4::from_scale(Vec3::splat(visual_scale / MODEL_SCALE))
            * Mat4::from_translation(Vec3::new(0.45, 0.0, 0.0))
    } else {
        Mat4::from_quat(Quat::from_euler(
            EulerRot::YXZ,
            0.0,
            std::f32::consts::PI,
            -std::f32::consts::FRAC_PI_2,
        )) * Mat4::from_scale(Vec3::splat(visual_scale / MODEL_SCALE * 0.5))
    }
}

/// The largest axis scale of a world matrix (`absoluteScaling`).
fn absolute_scale(m: Mat4) -> f32 {
    m.x_axis
        .truncate()
        .length()
        .max(m.y_axis.truncate().length())
        .max(m.z_axis.truncate().length())
        .max(0.001)
}

impl HandWeapons {
    /// Loads every weapon's world model and barrels from the packages; a
    /// model that fails is left out (its holders' hands stay empty).
    pub fn load(gpu: &Gpu, models: &mut ModelRenderer, pk3: &mut Pk3Loader) -> Self {
        let mut weapons = Vec::new();
        for def in WEAPON_MODELS {
            let path = package_path(def.model_path);
            let Some(Ok(model)) = pk3.get_file(&path).map(|bytes| GltfModel::from_glb(&bytes)) else {
                eprintln!("[RemoteWeapon] Failed to load {}: {path}", def.id);
                continue;
            };
            let handle = models.upload(gpu, &model, world_weapon_material_mode);
            let mut barrels = Vec::new();
            if let Some(first) = def.barrels.first() {
                let tags = pk3
                    .get_file(&format!("{}tags.txt", directory(&package_path(first.model_path))))
                    .map(|bytes| parse_tags(&String::from_utf8_lossy(&bytes)))
                    .unwrap_or_default();
                let mount = find_barrel_mount(&tags, &model.attachment_points);
                for barrel in def.barrels {
                    let Some(Ok(barrel_model)) = pk3
                        .get_file(&package_path(barrel.model_path))
                        .map(|bytes| GltfModel::from_glb(&bytes))
                    else {
                        continue;
                    };
                    let barrel_handle = models.upload(gpu, &barrel_model, barrel_material_mode);
                    let root = barrel_root_matrix(
                        &mount,
                        &BarrelDef {
                            model_path: barrel.model_path.to_owned(),
                            offset: barrel.offset.map(Vec3::from),
                        },
                    );
                    barrels.push((barrel_handle, barrel_model, root));
                }
            }
            weapons.push(HandWeapon {
                def,
                handle,
                model,
                barrels,
            });
        }
        Self { weapons }
    }

    /// How many weapons loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.weapons.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.weapons.is_empty()
    }

    /// The draws of `weapon` held in a hand at `hand` (world), with the
    /// quad's outline (`applyAttachedWeaponQuadOutline`) when `quad_pulse`
    /// is given: 0.7 to 1.25 world units, over each mesh's own scale.
    pub fn draws(&self, weapon: &str, hand: Mat4, quad_pulse: Option<f64>, draws: &mut Vec<ModelDraw>) {
        let Some(def) = weapon_model(weapon) else { return };
        let Some(w) = self.weapons.iter().find(|w| w.def.id == def.id) else {
            return;
        };
        let leader = hand * hand_weapon_local(def);
        let outline = |world: Mat4| {
            quad_pulse.map(|pulse| Outline {
                color: QUAD_DAMAGE_COLOR,
                width: (QUAD_WORLD_WEAPON_OUTLINE_MIN + QUAD_WORLD_WEAPON_OUTLINE_RANGE * pulse) as f32
                    / absolute_scale(world),
            })
        };
        let matrices: Vec<Mat4> = if def.id == "blade" {
            // The blade keeps its node tree.
            w.model.primitives.iter().map(|p| leader * p.node_matrix).collect()
        } else {
            flattened_pickup_matrices(&w.model, leader)
        };
        for (primitive, world) in matrices.into_iter().enumerate() {
            draws.push(ModelDraw {
                outline: outline(world),
                ..ModelDraw::new(w.handle, primitive, world)
            });
        }
        for (handle, model, root) in &w.barrels {
            for (primitive, p) in model.primitives.iter().enumerate() {
                let world = leader * *root * p.node_matrix;
                draws.push(ModelDraw {
                    outline: outline(world),
                    ..ModelDraw::new(*handle, primitive, world)
                });
            }
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
    fn a_quad_carrier_glows() {
        let mut draws = vec![ModelDraw::new(ModelHandle::default(), 0, Mat4::IDENTITY)];
        update_powerup_glow(&mut draws, 0, 0.0);
        assert_eq!(draws[0].emissive, None);
        update_powerup_glow(&mut draws, 1, 0.0);
        // Half way through the pulse: 0.35 + 0.3, outline 40 / 576.
        let e = draws[0].emissive.unwrap();
        assert!((e.x - 0.65).abs() < 1e-6 && (e.y - 0.28 * 0.65).abs() < 1e-6);
        assert!((draws[0].outline.unwrap().width - 40.0 / 576.0).abs() < 1e-7);
    }

    #[test]
    fn a_gun_sits_in_the_hand_at_half_its_view_scale() {
        let plasmagun = weapon_model("plasmagun").unwrap();
        let local = hand_weapon_local(plasmagun);
        let (scale, _, translation) = local.to_scale_rotation_translation();
        let expected = plasmagun.view.scale as f32 / MODEL_SCALE * 0.5;
        assert!((scale.x - expected).abs() < 1e-6);
        assert_eq!(translation, Vec3::ZERO);
        // rotation.set(PI, 0, -PI / 2): the barrel (+X) turns along the
        // fingers (the bone's +Y).
        let barrel = local.transform_vector3(Vec3::X).normalize();
        assert!((barrel - Vec3::Y).length() < 1e-5, "{barrel}");
        // The blade's grip moves its handle into the hand.
        let blade = hand_weapon_local(weapon_model("blade").unwrap());
        assert!(blade.transform_point3(Vec3::ZERO).length() > 0.0);
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
