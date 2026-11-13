//! wash-fps `client/RemotePlayerRenderer.ts`: remote players drawn as their
//! side's body, placed, styled and animated from their interpolated state.
//!
//! Ported so far: the bodies (Police by default and the variants each side
//! may choose, `cl_team_ally_model` and `cl_team_enemy_model`, with Vanguard
//! standing in while the packages lack them), the body's placement (an
//! anchor at the feet, turned by the yaw, the model fitted and scaled
//! [`MODEL_SCALE`]), the clip choice and its speed ratio (`updateModel`:
//! the relaxed and backwards runs, the run's leg and upper-body rates, the
//! aim bend, the blade stance and the blade swing), the visual style (team
//! tint and outline), the quad carrier's glow (`updatePowerupGlow`), and
//! the weapon in hand (`updateWeaponModel`, [`HandWeapons`], held on the
//! support palm). Teleport effects live in `teleport_effect`. Not yet: the
//! hitbox, the demo animation timeline, PVS and animation LOD.

use std::collections::HashMap;
use std::sync::Arc;

use shagger::animation::player_model_assets::{
    PLAYER_ANIMATION_ROOT, PLAYER_ANIMATIONS, VANGUARD_ANIMATIONS, VANGUARD_MODEL, VANGUARD_ROOT,
};
use shagger::animation::player_model_fit::{BladeStance, GunSupport};
use shagger::animation::player_model_manager::{
    BLADE_STANCE, PlayerModel, PlayerModelInstance, PlayerModelVisualStyle,
};
use shagger::babylon::meshes::vertex_data::babylon_local_matrix;
use shagger::core::cvar::{CvarOptions, CvarType, Cvars, with_cvars};
use shagger::loader::model::glb::GlbError;
use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::model::tags_loader::parse_tags;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
use shagger::rendering::glam::DVec3;
use shagger::rendering::glam::{Mat4, Quat, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer, Outline};
use shagger::weapon::weapon_barrels::{
    BarrelDef, barrel_material_mode, barrel_root_matrix, find_barrel_mount, world_weapon_material_mode,
};
use shagger::world::pickup::{flattened_pickup_matrices, pickup_scale};

use super::attached_blade::{BladeShape, hold_blade_grip, measure_blade_shape};
use super::attached_gun::{GunShape, hold_gun_grip, measure_gun_shape};
use super::player_model_selection::{PLAYER_MODELS, player_model_asset};
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
/// The backwards run's natural speed (`RUN_BACK_NATURAL_SPEED`).
pub const RUN_BACK_NATURAL_SPEED: f64 = 77.0;
/// Running players' legs and upper body, as shares of the speed-matched
/// rate (`RUN_LEGS_RATE`, `RUN_UPPER_RATE`).
pub const RUN_LEGS_RATE: f64 = 0.75;
pub const RUN_UPPER_RATE: f64 = 0.5;
/// Clips that keep their own arms: no aim bend, no blade stance
/// (`NON_AIMING_ANIMATIONS`).
pub const NON_AIMING_ANIMATIONS: [&str; 5] = ["swim", "swim_idle", "climb", "walljump", "slide"];
/// How long a gun stays raised after a shot (`noteWeaponFire`), seconds.
pub const WEAPON_READY_SECONDS: f64 = 0.6;
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
pub fn update_powerup_glow(draws: &mut [ModelDraw], powerup_mask: u32, elapsed: f64, scale_factor: f32) {
    if !mask_has_quad(powerup_mask) {
        return;
    }
    let pulse = get_quad_damage_pulse(elapsed);
    let intensity = (QUAD_EMISSIVE_MIN + QUAD_EMISSIVE_RANGE * pulse) as f32;
    let outline_size = QUAD_PLAYER_OUTLINE_MIN + QUAD_PLAYER_OUTLINE_RANGE * pulse;
    // Over the body's fit, as `applyPowerupGlow` divides it.
    let width = (outline_size / f64::from(MODEL_SCALE * 16.0).max(1.0)) as f32 / scale_factor;
    for draw in draws {
        draw.emissive = Some(QUAD_DAMAGE_COLOR * intensity);
        draw.outline = Some(Outline {
            color: QUAD_DAMAGE_COLOR,
            width,
        });
    }
}

/// The anchor: at the feet, turned by the yaw (`updateModel`).
#[must_use]
pub fn anchor_matrix(interp: &InterpolatedEntity) -> Mat4 {
    Mat4::from_translation(Vec3::new(interp.x as f32, feet_y(interp) as f32, interp.z as f32))
        * Mat4::from_quat(Quat::from_rotation_y(interp.yaw as f32))
}

/// The model's world matrix: the anchor, then the mesh lifted by its fit
/// and scaled with no rotation (`createInstance`, `updateModel`).
#[must_use]
pub fn body_matrix(interp: &InterpolatedEntity, model: &PlayerModel) -> Mat4 {
    anchor_matrix(interp) * model.root_matrix(MODEL_SCALE)
}

/// `movesBackward`.
#[must_use]
pub fn moves_backward(interp: &InterpolatedEntity) -> bool {
    interp.yaw.sin() * interp.vx + interp.yaw.cos() * interp.vz < 0.0
}

/// The running rates (`cl_player_run_legs`, `cl_player_run_upper`).
pub fn register_remote_player_cvars(cvars: &mut Cvars) {
    cvars.register(
        "cl_player_run_legs",
        RUN_LEGS_RATE,
        CvarOptions::describe("Running players' hips and legs, as a share of the rate that matches their speed")
            .range(0.05, 2.0)
            .kind(CvarType::Float),
    );
    cvars.register(
        "cl_player_run_upper",
        RUN_UPPER_RATE,
        CvarOptions::describe("Running players' spine, arms and head, as a share of the rate that matches their speed")
            .range(0.0, 2.0)
            .kind(CvarType::Float),
    );
}

/// The running rates, legs then upper body.
#[must_use]
pub fn run_rates() -> (f64, f64) {
    with_cvars(|c| {
        let number = |name: &str, d: f64| if c.exists(name) { c.get_number(name) } else { d };
        (
            number("cl_player_run_legs", RUN_LEGS_RATE),
            number("cl_player_run_upper", RUN_UPPER_RATE),
        )
    })
}

/// The locomotion clip's speed ratio, matching the feet to the movement
/// (`updateModel`), times the demo's time scale. A run plays its legs at
/// `run_legs` of that.
#[must_use]
pub fn animation_speed_ratio(anim: &str, interp: &InterpolatedEntity, time_scale: f64, run_legs: f64) -> f64 {
    let speed = (interp.vx * interp.vx + interp.vz * interp.vz).sqrt();
    let fwd_x = interp.yaw.sin();
    let fwd_z = interp.yaw.cos();
    let forward_dot = fwd_x * interp.vx + fwd_z * interp.vz;
    let right_dot = fwd_z * interp.vx - fwd_x * interp.vz;
    let running = matches!(anim, "run" | "run_relaxed" | "run_back");
    let ratio = match anim {
        "run" | "run_relaxed" | "run_back" | "walk" => {
            let natural = match anim {
                "walk" => WALK_NATURAL_SPEED,
                "run_back" => RUN_BACK_NATURAL_SPEED,
                _ => RUN_NATURAL_SPEED,
            };
            let mut r = speed / natural;
            // The backwards clip moves backwards itself; the others play reversed.
            if anim != "run_back" && moves_backward(interp) {
                r = -r;
            }
            r.clamp(-3.0, 3.0) * if running { run_legs } else { 1.0 }
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

/// One loaded body: its model, shared with the ragdolls that take its
/// players, and its uploads.
#[derive(Clone)]
pub struct PlayerBody {
    /// `modelKey`: the choice it was loaded for (`police` for the default
    /// body, Vanguard included).
    pub key: &'static str,
    pub model: Arc<PlayerModel>,
    pub handle: ModelHandle,
    /// The model uploaded to blend, for a ragdoll's fade.
    pub fade_handle: ModelHandle,
}

/// A player's body leaving the renderer (`detachModel`), for a ragdoll.
pub struct DetachedBody {
    pub body: PlayerBody,
    pub instance: PlayerModelInstance,
}

/// One remote player's pose and what the renderer remembers about them.
struct RemoteBody {
    body: usize,
    instance: PlayerModelInstance,
    /// `weaponReadyUntil`, on the pose clock.
    weapon_ready_until: f64,
    /// Whether the held blade stands in the stance (`setBladeStance`).
    blade_upright: bool,
}

/// Where a player's held weapon hangs: the weapon hand in the world and
/// what the body measured for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldFrame {
    /// The weapon hand (`weaponHand`, or the right hand without one).
    pub hand: Mat4,
    /// `modelScale`: the body's scale, fit included.
    pub model_scale: f32,
    pub gun_support: Option<GunSupport>,
    /// The blade's stance, when it stands in it now.
    pub blade_stance: Option<BladeStance>,
}

impl HeldFrame {
    /// A bare hand at `hand` on an unfitted body.
    #[must_use]
    pub fn bare(hand: Mat4) -> Self {
        Self {
            hand,
            model_scale: MODEL_SCALE,
            gun_support: None,
            blade_stance: None,
        }
    }
}

/// The bodies, uploaded once, and every remote player's pose.
pub struct RemotePlayerRenderer {
    /// The default body first, then the variants that loaded.
    pub bodies: Vec<PlayerBody>,
    /// The default body's model.
    pub model: Arc<PlayerModel>,
    instances: HashMap<String, RemoteBody>,
}

fn load_body(gpu: &Gpu, models: &mut ModelRenderer, key: &'static str, model: PlayerModel) -> PlayerBody {
    let handle = models.upload(gpu, &model.model, |_| ModelMaterialMode::Lit);
    let fade_handle = models.upload(gpu, &model.model, |_| ModelMaterialMode::Glass(1.0));
    PlayerBody {
        key,
        model: Arc::new(model),
        handle,
        fade_handle,
    }
}

/// Whether a packaged body loaded its shared clips (the standing idle at
/// least): without them the published packages predate it.
fn has_clips(model: &PlayerModel) -> bool {
    model.has_animation("idle")
}

impl RemotePlayerRenderer {
    /// Loads Police and its variants with the shared clips through `read`
    /// (package paths) and uploads them; Vanguard and its own clips when the
    /// packages do not have Police yet.
    pub fn load(
        gpu: &Gpu,
        models: &mut ModelRenderer,
        mut read: impl FnMut(&str) -> Option<Vec<u8>>,
    ) -> Result<Self, GlbError> {
        let (root, file) = player_model_asset("police");
        let police = PlayerModel::load(&mut read, root, file, PLAYER_ANIMATIONS, PLAYER_ANIMATION_ROOT)
            .ok()
            .filter(has_clips);
        let mut bodies = Vec::new();
        if let Some(police) = police {
            bodies.push(load_body(gpu, models, "police", police));
            for &choice in &PLAYER_MODELS[1..] {
                let (root, file) = player_model_asset(choice);
                match PlayerModel::load(&mut read, root, file, PLAYER_ANIMATIONS, PLAYER_ANIMATION_ROOT) {
                    Ok(model) if has_clips(&model) => bodies.push(load_body(gpu, models, choice, model)),
                    Ok(_) => eprintln!("[RemotePlayerRenderer] Player model variant failed: {choice}: no clips"),
                    Err(e) => eprintln!("[RemotePlayerRenderer] Player model variant failed: {choice}: {e}"),
                }
            }
        } else {
            eprintln!("[RemotePlayerRenderer] No packaged Police body; drawing Vanguard");
            let model = PlayerModel::load(read, VANGUARD_ROOT, VANGUARD_MODEL, VANGUARD_ANIMATIONS, VANGUARD_ROOT)?;
            bodies.push(load_body(gpu, models, "police", model));
        }
        Ok(Self {
            model: Arc::clone(&bodies[0].model),
            bodies,
            instances: HashMap::new(),
        })
    }

    /// The body a choice draws with: the default for Police, or for a
    /// variant that did not load.
    fn body_for(&self, choice: &str) -> usize {
        self.bodies.iter().position(|b| b.key == choice).unwrap_or(0)
    }

    /// The loaded body a choice draws with ([`Self::body_for`]).
    #[must_use]
    pub fn body_by_choice(&self, choice: &str) -> &PlayerBody {
        &self.bodies[self.body_for(choice)]
    }

    /// The body a player wears now.
    #[must_use]
    pub fn body_of(&self, id: &str) -> Option<&PlayerBody> {
        self.instances.get(id).map(|r| &self.bodies[r.body])
    }

    /// A player's body for `choice`: made with the idle playing
    /// (`createModelInstance`), and made again when the choice changed.
    fn ensure(&mut self, id: &str, choice: &str) -> (Arc<PlayerModel>, &mut RemoteBody) {
        let body = self.body_for(choice);
        if self.instances.get(id).is_some_and(|r| r.body != body) {
            self.instances.remove(id);
        }
        let model = Arc::clone(&self.bodies[body].model);
        let remote = self.instances.entry(id.to_owned()).or_insert_with(|| {
            let mut instance = model.create_instance();
            model.play_animation(&mut instance, "idle");
            RemoteBody {
                body,
                instance,
                weapon_ready_until: 0.0,
                blade_upright: false,
            }
        });
        (model, remote)
    }

    /// Moves one player on by `dt` seconds in a forced clip, as a pose
    /// held for a screenshot or a fresh corpse is, on `body` (the default
    /// body when `None`).
    pub fn update(
        &mut self,
        id: &str,
        interp: &InterpolatedEntity,
        dt: f64,
        time_scale: f64,
        clip: Option<&str>,
        body: Option<&str>,
    ) {
        let anim = clip.unwrap_or_else(|| derive_animation(interp));
        let (run_legs, _) = run_rates();
        let (model, remote) = self.ensure(id, body.unwrap_or("police"));
        let instance = &mut remote.instance;
        model.play_animation(instance, anim);
        instance.speed_ratio = animation_speed_ratio(anim, interp, time_scale, run_legs);
        model.advance(instance, dt);
    }

    /// `updateModel`: picks one live player's clips and poses them by `dt`
    /// seconds. `weapon` is what they hold, `pose_time` the clock their shots
    /// were noted on (the playback clock in a demo), `choice` their side's
    /// body.
    #[allow(clippy::too_many_arguments)]
    pub fn update_live(
        &mut self,
        id: &str,
        interp: &InterpolatedEntity,
        dt: f64,
        time_scale: f64,
        weapon: Option<&str>,
        pose_time: f64,
        choice: &str,
    ) {
        let (run_legs, run_upper) = run_rates();
        let (model, remote) = self.ensure(id, choice);
        let instance = &mut remote.instance;
        // Keep the next locomotion pose current while an attack owns the bones.
        let locomotion = derive_animation(interp);
        let blade = weapon == Some("blade");
        let weapon_ready = interp.aiming == Some(true) || (!blade && remote.weapon_ready_until > pose_time);
        let backward =
            matches!(locomotion, "run" | "walk") && moves_backward(interp) && model.has_animation("run_back");
        let relaxed = !backward && locomotion == "run" && !weapon_ready && model.has_animation("run_relaxed");
        let anim = if backward {
            "run_back"
        } else if relaxed {
            "run_relaxed"
        } else {
            locomotion
        };
        let aiming = !relaxed && !NON_AIMING_ANIMATIONS.contains(&locomotion);
        model.set_aim_pitch(
            instance,
            if aiming {
                interp.pitch.unwrap_or(0.0) as f32
            } else {
                0.0
            },
        );
        model.play_animation(instance, anim);
        // The blade is held in its own upper-body stance; swimming,
        // climbing and the like keep their arms.
        let stance = (blade && !NON_AIMING_ANIMATIONS.contains(&locomotion) && model.has_animation(BLADE_STANCE))
            .then_some(BLADE_STANCE);
        model.set_upper_body_stance(instance, stance, time_scale);
        // The blade stands up from the fist in the stance; a swing holds it
        // along the fingers.
        remote.blade_upright = stance.is_some() && !model.is_one_shot_playing(instance);
        if !model.is_full_body_one_shot_playing(instance) {
            let running = matches!(anim, "run" | "run_relaxed" | "run_back");
            instance.speed_ratio = animation_speed_ratio(anim, interp, time_scale, run_legs);
            let share = if running && run_legs > 0.0 {
                run_upper / run_legs
            } else {
                1.0
            };
            model.set_upper_body_share(instance, share);
        }
        model.advance(instance, dt);
    }

    /// `noteWeaponFire`: keeps the weapon raised briefly after each shot.
    pub fn note_weapon_fire(&mut self, id: &str, pose_time: f64) {
        if let Some(remote) = self.instances.get_mut(id) {
            remote.weapon_ready_until = pose_time + WEAPON_READY_SECONDS;
        }
    }

    /// `playAttackAnim`: a one-shot attack (the blade's swing) on a player.
    pub fn play_attack_anim(&mut self, id: &str, anim: &str, speed_ratio: f64) {
        if let Some(remote) = self.instances.get_mut(id) {
            let model = &self.bodies[remote.body].model;
            model.play_once(&mut remote.instance, anim, speed_ratio);
        }
    }

    /// Holds a player's clip at a frame (`goToFrame`), for comparing poses.
    pub fn go_to_frame(&mut self, id: &str, frame: f64) {
        if let Some(remote) = self.instances.get_mut(id) {
            self.bodies[remote.body].model.go_to_frame(&mut remote.instance, frame);
        }
    }

    /// Keeps only the players `keep` accepts, handing back the bodies of
    /// the rest (a death's ragdoll takes one over).
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) -> Vec<(String, DetachedBody)> {
        let gone: Vec<String> = self.instances.keys().filter(|id| !keep(id)).cloned().collect();
        gone.into_iter()
            .filter_map(|id| self.take(&id).map(|b| (id, b)))
            .collect()
    }

    /// `detachModel`: a player's body leaves the renderer (to a ragdoll).
    pub fn take(&mut self, id: &str) -> Option<DetachedBody> {
        let remote = self.instances.remove(id)?;
        let body = self.bodies[remote.body].clone();
        let mut instance = remote.instance;
        // The stance and a swing are the manager's; the ragdoll drives the bones now.
        body.model.set_upper_body_stance(&mut instance, None, 1.0);
        body.model.stop_one_shot(&mut instance, false);
        Some(DetachedBody { body, instance })
    }

    /// The default body's lit model and blending one.
    #[must_use]
    pub fn handles(&self) -> (ModelHandle, ModelHandle) {
        (self.bodies[0].handle, self.bodies[0].fade_handle)
    }

    /// Forgets a player who left.
    pub fn remove(&mut self, id: &str) {
        self.instances.remove(id);
    }

    /// A player's body in the world.
    #[must_use]
    pub fn world_of(&self, id: &str, interp: &InterpolatedEntity) -> Option<Mat4> {
        let remote = self.instances.get(id)?;
        Some(body_matrix(interp, &self.bodies[remote.body].model))
    }

    /// Where a player's weapon hangs (`mixamorig:RightHand` under the
    /// body, as `attachToBone` follows it, then the weapon hand's basis).
    #[must_use]
    pub fn hand_world(&self, id: &str, interp: &InterpolatedEntity) -> Option<HeldFrame> {
        let remote = self.instances.get(id)?;
        let model = &self.bodies[remote.body].model;
        held_frame(
            model,
            &remote.instance,
            body_matrix(interp, model),
            remote.blade_upright,
        )
    }

    /// `getBoneAnchor`: where a bone of a player's body is in the world
    /// (`mixamorig:` prefixed, as the glTF names it), and the body's yaw.
    #[must_use]
    pub fn bone_anchor(&self, id: &str, interp: &InterpolatedEntity, bone: &str) -> Option<(DVec3, f64)> {
        let remote = self.instances.get(id)?;
        let model = &self.bodies[remote.body].model;
        let skeleton = &model.model.skeleton;
        let joint = skeleton.find(&format!("mixamorig:{bone}"))?;
        let globals = skeleton.global_matrices(&remote.instance.locals);
        let world = body_matrix(interp, model) * *globals.get(joint)?;
        Some((world.w_axis.truncate().as_dvec3(), interp.yaw))
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
        let Some(remote) = self.instances.get(id) else {
            return;
        };
        let body = &self.bodies[remote.body];
        let world = body_matrix(interp, &body.model);
        body_draws(body, &remote.instance, world, style, joints, draws);
    }
}

/// Where the weapon hangs on a body posed by `instance` at `world`
/// ([`RemotePlayerRenderer::hand_world`]); `blade_upright` when the blade
/// stands in its stance.
#[must_use]
pub fn held_frame(
    model: &PlayerModel,
    instance: &PlayerModelInstance,
    world: Mat4,
    blade_upright: bool,
) -> Option<HeldFrame> {
    let skeleton = &model.model.skeleton;
    let hand = skeleton.find(RIGHT_HAND)?;
    let globals = skeleton.global_matrices(&instance.locals);
    let bone = world * *globals.get(hand)?;
    let basis = model.hand_basis;
    Some(HeldFrame {
        hand: basis.map_or(bone, |b| bone * b.matrix()),
        model_scale: MODEL_SCALE * model.scale_factor(),
        gun_support: basis.and(model.gun_support),
        blade_stance: basis.and(model.blade_stance).filter(|_| blade_upright),
    })
}

/// A body's draws posed by `instance` at `world`, in `style`, appending its
/// joint palettes to `joints` ([`RemotePlayerRenderer::draws`]).
pub fn body_draws(
    body: &PlayerBody,
    instance: &PlayerModelInstance,
    world: Mat4,
    style: &PlayerModelVisualStyle,
    joints: &mut Vec<Mat4>,
    draws: &mut Vec<ModelDraw>,
) {
    // `applyVisualStyle`: the outline is in world units, the mesh scaled by its fit.
    let outline_width = style.outline_width / body.model.scale_factor();
    let mut palettes: HashMap<usize, u32> = HashMap::new();
    for (index, primitive) in body.model.model.primitives.iter().enumerate() {
        let Some(skin) = &primitive.skin else { continue };
        let offset = *palettes.entry(skin.skin).or_insert_with(|| {
            let at = joints.len() as u32;
            joints.extend(body.model.joint_matrices(instance, skin.skin));
            at
        });
        draws.push(ModelDraw {
            tint: style.tint_color,
            skin: Some(offset),
            outline: style.outline_enabled.then_some(Outline {
                color: style.outline_color,
                width: outline_width,
            }),
            ..ModelDraw::new(body.handle, index, world)
        });
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
    /// The gun's muzzle and rear in its held frame (`measureGunShape`).
    gun_shape: GunShape,
    /// The blade's thin axis and grip (`measureBladeShape`).
    blade_shape: BladeShape,
    barrels: Vec<(ModelHandle, GltfModel, Mat4)>,
}

/// `updateWeaponModel`'s weapons: every gun's world model, loaded once
/// (wash-fps imports each on first use and clones it per player), and
/// drawn in a player's right hand.
#[derive(Default)]
pub struct HandWeapons {
    weapons: Vec<HandWeapon>,
}

/// The hand-attached leader's local matrix in the weapon hand's frame:
/// a gun at half the view model's scale over the body's, resting on the
/// support palm when the body has one (`holdGunGrip`); the blade
/// (`attachBladeToPlayer`) at the view model's scale over the body's, its
/// node tree under a grip moved so its handle sits in the hand, standing in
/// the fist in the stance (`holdBladeGrip`, `setBladeStance`).
#[must_use]
pub fn hand_weapon_local(
    def: &WeaponModel,
    frame: &HeldFrame,
    gun: Option<&GunShape>,
    blade: Option<&BladeShape>,
) -> Mat4 {
    let visual_scale = def.view.scale as f32;
    if def.id == "blade" {
        hold_blade_grip(
            &blade.copied().unwrap_or_default(),
            visual_scale,
            frame.model_scale,
            frame.blade_stance.as_ref(),
        )
    } else {
        hold_gun_grip(gun, visual_scale, frame.model_scale, frame.gun_support.as_ref())
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

impl HandWeapon {
    fn local(&self, frame: &HeldFrame) -> Mat4 {
        hand_weapon_local(self.def, frame, Some(&self.gun_shape), Some(&self.blade_shape))
    }
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
                gun_shape: measure_gun_shape(&model),
                blade_shape: measure_blade_shape(&model),
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

    /// `getWeaponMuzzleWorldPositionToRef`'s first two choices for `weapon`
    /// held in a hand at `hand` (world): the gun's `tag_flash` (kept under
    /// the leader, as `assembleGun` keeps it), and the leader's origin. The
    /// blade has no muzzle tag.
    #[must_use]
    pub fn muzzle(&self, weapon: &str, frame: &HeldFrame) -> Option<(Option<Vec3>, Vec3)> {
        let def = weapon_model(weapon)?;
        let w = self.weapons.iter().find(|w| w.def.id == def.id)?;
        let leader = frame.hand * w.local(frame);
        let tag = (def.id != "blade")
            .then(|| w.model.attachment_points.get("tag_flash"))
            .flatten()
            .map(|tag| leader.transform_point3(tag.position));
        Some((tag, leader.w_axis.truncate()))
    }

    /// The draws of `weapon` on the floor (`DroppedWeaponVisual.place`): the
    /// leader at `position`, turned by Babylon Euler `rotation` and scaled
    /// by the weapon's scale (1.5 when none), the other meshes flattened
    /// under it as on a pickup, the barrels on their mount.
    pub fn world_draws(&self, weapon: &str, position: [f64; 3], rotation: [f64; 3], draws: &mut Vec<ModelDraw>) {
        let Some(def) = weapon_model(weapon) else { return };
        let Some(w) = self.weapons.iter().find(|w| w.def.id == def.id) else {
            return;
        };
        let scale = f64::from(pickup_scale(Some(def.scale)));
        let leader = babylon_local_matrix(
            DVec3::from_array(position),
            DVec3::from_array(rotation),
            DVec3::splat(scale),
        )
        .as_mat4();
        for (primitive, world) in flattened_pickup_matrices(&w.model, leader).into_iter().enumerate() {
            draws.push(ModelDraw::new(w.handle, primitive, world));
        }
        for (handle, model, root) in &w.barrels {
            for (primitive, p) in model.primitives.iter().enumerate() {
                draws.push(ModelDraw::new(*handle, primitive, leader * *root * p.node_matrix));
            }
        }
    }

    /// The draws of `weapon` held in a hand at `hand` (world), with the
    /// quad's outline (`applyAttachedWeaponQuadOutline`) when `quad_pulse`
    /// is given: 0.7 to 1.25 world units, over each mesh's own scale.
    pub fn draws(&self, weapon: &str, frame: &HeldFrame, quad_pulse: Option<f64>, draws: &mut Vec<ModelDraw>) {
        let Some(def) = weapon_model(weapon) else { return };
        let Some(w) = self.weapons.iter().find(|w| w.def.id == def.id) else {
            return;
        };
        let leader = frame.hand * w.local(frame);
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
        assert_eq!(animation_speed_ratio("run", &run, 1.0, 1.0), 3.0);
        // The run's legs play at their share.
        assert_eq!(animation_speed_ratio("run_relaxed", &run, 1.0, 0.75), 2.25);
        let back = InterpolatedEntity { vz: -80.0, ..run };
        assert_eq!(animation_speed_ratio("walk", &back, 1.0, 0.75), -2.0);
        // The backwards run moves backwards itself.
        let slow_back = InterpolatedEntity { vz: -77.0, ..run };
        assert_eq!(animation_speed_ratio("run_back", &slow_back, 1.0, 1.0), 1.0);
        assert_eq!(animation_speed_ratio("run", &slow_back, 1.0, 1.0), -0.77);
        let up = InterpolatedEntity { vy: 160.0, ..run };
        assert_eq!(animation_speed_ratio("jump", &up, 0.5, 0.75), 0.5);
        assert_eq!(animation_speed_ratio("idle", &run, 1.0, 0.75), 1.0);
    }

    #[test]
    fn a_quad_carrier_glows() {
        let mut draws = vec![ModelDraw::new(ModelHandle::default(), 0, Mat4::IDENTITY)];
        update_powerup_glow(&mut draws, 0, 0.0, 1.0);
        assert_eq!(draws[0].emissive, None);
        update_powerup_glow(&mut draws, 1, 0.0, 1.0);
        // Half way through the pulse: 0.35 + 0.3, outline 40 / 576.
        let e = draws[0].emissive.unwrap();
        assert!((e.x - 0.65).abs() < 1e-6 && (e.y - 0.28 * 0.65).abs() < 1e-6);
        assert!((draws[0].outline.unwrap().width - 40.0 / 576.0).abs() < 1e-7);
    }

    #[test]
    fn a_gun_sits_in_the_hand_at_half_its_view_scale() {
        let plasmagun = weapon_model("plasmagun").unwrap();
        let frame = HeldFrame::bare(Mat4::IDENTITY);
        let local = hand_weapon_local(plasmagun, &frame, None, None);
        let (scale, _, translation) = local.to_scale_rotation_translation();
        let expected = plasmagun.view.scale as f32 / MODEL_SCALE * 0.5;
        assert!((scale.x - expected).abs() < 1e-6);
        assert_eq!(translation, Vec3::ZERO);
        // rotation.set(PI, 0, -PI / 2): the barrel (+X) turns along the
        // fingers (the bone's +Y).
        let barrel = local.transform_vector3(Vec3::X).normalize();
        assert!((barrel - Vec3::Y).length() < 1e-5, "{barrel}");
        // The blade's grip moves its handle into the hand.
        let blade = hand_weapon_local(weapon_model("blade").unwrap(), &frame, None, None);
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
