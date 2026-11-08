//! Port of wash-fps `client/RagdollSystem.ts`: a body that dies without
//! gibbing falls as a ragdoll, one box per major bone, launched along the
//! killing push; once it lies still (or after [`RAGDOLL_LIFETIME`] at the
//! latest) it fades out and bursts into gibs in its colour.
//!
//! Havok becomes shagger's rapier [`PhysicsSystem`] and Babylon's `Ragdoll`
//! its port in `shagger::babylon::physics::v2::ragdoll`. The ragdoll owns
//! the dead player's pose (`PlayerModelInstance`) and the body matrix it
//! died with; the bones move under it. `BatchedPoseSync` (a speed-up of the
//! same sync) has no counterpart; the follow handle is the ragdoll's serial
//! ([`RagdollSystem::position_of`]). The speed trail is Babylon's
//! `TrailMesh` (shagger's port) behind the hips box, one uploaded mesh per
//! ragdoll slot, its vertices streamed each frame.

use shagger::animation::player_model_manager::{PlayerModel, PlayerModelInstance, PlayerModelVisualStyle};
use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::meshes::trail_mesh::TrailMesh;
use shagger::babylon::physics::v2::ragdoll::{PhysicsConstraintType, Ragdoll, RagdollBoneProperties};
use shagger::physics::physics_system::{PhysicsSystem, from_rapier, quat_from_rapier, to_rapier};
use shagger::rendering::glam::{DMat4, DVec3, Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelRenderer, Outline};
use shagger::weapon::projectile_manager::{update_standard_mesh, upload_standard_mesh};

/// Longest a ragdoll simulates before gib-exploding, resting or not
/// (`RAGDOLL_LIFETIME`, seconds).
pub const RAGDOLL_LIFETIME: f64 = 5.0;
/// Seconds a ragdoll takes to fade before it bursts (`RAGDOLL_FADE`).
pub const RAGDOLL_FADE: f64 = 1.0;
/// A ragdoll is not judged at rest before it has flown this long.
pub const REST_MIN_AGE: f64 = 0.5;
/// At rest: the hips stay within this distance for [`REST_SECONDS`].
pub const REST_RADIUS: f64 = 12.0;
pub const REST_SECONDS: f64 = 0.4;
/// `MAX_RAGDOLLS`: the oldest goes (without gibs) to make room.
pub const MAX_RAGDOLLS: usize = 4;
/// `LAUNCH_SPEED` along the server's knockback direction.
pub const LAUNCH_SPEED: f64 = 670.0;
/// `RAGDOLL_GRAVITY_FACTOR`: slightly floatier arcs.
pub const RAGDOLL_GRAVITY_FACTOR: f32 = 0.8;
/// `TRAIL_LIFETIME`: seconds before the speed trail is gone.
pub const TRAIL_LIFETIME: f64 = 1.0;
/// `TRAIL_DIAMETER`: the ribbon's width.
pub const TRAIL_DIAMETER: f64 = 8.0;
/// `TRAIL_LENGTH`: its segments.
pub const TRAIL_LENGTH: usize = 30;
/// `MODEL_SCALE`: the boxes are sized in world units.
pub const MODEL_SCALE: f32 = 36.0;

/// `createBoneConfig`: the Mixamo Vanguard's major bones as boxes.
#[must_use]
pub fn create_bone_config() -> Vec<RagdollBoneProperties> {
    use PhysicsConstraintType::{BallAndSocket, Hinge};
    let s = MODEL_SCALE;
    let right = Vec3::X;
    // Babylon's `Vector3.Forward()` is +Z in its left-handed space.
    let forward = Vec3::Z;
    let bone = |name: &str, w: f32, h: f32, d: f32, joint, axis, min: f32, max: f32, mass: Option<f32>| {
        RagdollBoneProperties {
            joint,
            rotation_axis: Some(axis),
            min: Some(min),
            max: Some(max),
            mass,
            restitution: Some(0.0),
            ..RagdollBoneProperties::new(&format!("mixamorig:{name}"), w * s, h * s, d * s)
        }
    };
    vec![
        RagdollBoneProperties {
            mass: Some(2.0),
            restitution: Some(0.0),
            ..RagdollBoneProperties::new("mixamorig:Hips", 0.3 * s, 0.15 * s, 0.2 * s)
        },
        // The spine names no mass, so Babylon's default of 10 holds.
        bone("Spine", 0.25, 0.12, 0.18, Some(BallAndSocket), right, -0.1, 0.1, None),
        bone(
            "Spine2",
            0.32,
            0.16,
            0.22,
            Some(BallAndSocket),
            right,
            -0.1,
            0.1,
            Some(1.0),
        ),
        bone(
            "Neck",
            0.08,
            0.05,
            0.08,
            Some(BallAndSocket),
            right,
            -0.15,
            0.15,
            Some(1.0),
        ),
        bone(
            "Head",
            0.18,
            0.22,
            0.18,
            Some(BallAndSocket),
            right,
            -0.2,
            0.2,
            Some(1.0),
        ),
        bone(
            "LeftArm",
            0.08,
            0.22,
            0.08,
            Some(BallAndSocket),
            forward,
            -0.6,
            0.6,
            Some(1.0),
        ),
        bone(
            "LeftForeArm",
            0.06,
            0.22,
            0.06,
            Some(Hinge),
            right,
            -1.5,
            0.0,
            Some(1.0),
        ),
        bone(
            "RightArm",
            0.08,
            0.22,
            0.08,
            Some(BallAndSocket),
            forward,
            -0.6,
            0.6,
            Some(1.0),
        ),
        bone(
            "RightForeArm",
            0.06,
            0.22,
            0.06,
            Some(Hinge),
            right,
            -1.5,
            0.0,
            Some(1.0),
        ),
        bone(
            "LeftUpLeg",
            0.12,
            0.4,
            0.12,
            Some(BallAndSocket),
            right,
            -0.5,
            0.3,
            Some(1.0),
        ),
        bone("LeftLeg", 0.1, 0.38, 0.1, Some(Hinge), right, 0.0, 1.2, Some(1.0)),
        bone(
            "LeftFoot",
            0.1,
            0.06,
            0.2,
            Some(BallAndSocket),
            right,
            -0.2,
            0.2,
            Some(1.0),
        ),
        bone(
            "RightUpLeg",
            0.12,
            0.4,
            0.12,
            Some(BallAndSocket),
            right,
            -0.5,
            0.3,
            Some(1.0),
        ),
        bone("RightLeg", 0.1, 0.38, 0.1, Some(Hinge), right, 0.0, 1.2, Some(1.0)),
        bone(
            "RightFoot",
            0.1,
            0.06,
            0.2,
            Some(BallAndSocket),
            right,
            -0.2,
            0.2,
            Some(1.0),
        ),
    ]
}

/// The launch velocity: the full knockback direction at [`LAUNCH_SPEED`],
/// straight up when it has none.
#[must_use]
pub fn launch_velocity(dir: DVec3) -> DVec3 {
    let len = (dir.x * dir.x + dir.y * dir.y + dir.z * dir.z).sqrt();
    if len > 0.001 {
        DVec3::new(
            dir.x / len * LAUNCH_SPEED,
            dir.y / len * LAUNCH_SPEED,
            dir.z / len * LAUNCH_SPEED,
        )
    } else {
        DVec3::new(0.0, LAUNCH_SPEED, 0.0)
    }
}

/// `ActiveRagdoll`.
pub struct ActiveRagdoll {
    /// Its number, for the follow handle.
    pub serial: u64,
    pub instance: PlayerModelInstance,
    /// The body matrix it died with (anchor, yaw, scale).
    pub world: Mat4,
    ragdoll: Ragdoll,
    pub age: f64,
    /// The colour the body was drawn with; its gibs take it.
    pub color: Option<Vec3>,
    /// The outline it was drawn with: the body keeps its meshes, and with
    /// them `renderOutline`.
    pub outline: Option<Outline>,
    rest_anchor: Option<DVec3>,
    rest_seconds: f64,
    pub fade_start: Option<f64>,
    /// The speed trail behind the hips, for its first second.
    pub trail: Option<TrailMesh>,
}

impl ActiveRagdoll {
    /// The trail's alpha: 0.6 fading to nothing over its life.
    #[must_use]
    pub fn trail_alpha(&self) -> f64 {
        0.6 * (1.0 - self.age / TRAIL_LIFETIME)
    }

    /// How visible it is: 1 until the fade, then down to 0.
    #[must_use]
    pub fn fade(&self) -> f64 {
        self.fade_start
            .map_or(1.0, |start| (1.0 - (self.age - start) / RAGDOLL_FADE).max(0.0))
    }
}

/// `RagdollSystem`.
pub struct RagdollSystem {
    pub physics: PhysicsSystem,
    ragdolls: Vec<ActiveRagdoll>,
    /// Bursts waiting for the gib system (`onGibExplode`): where, and the
    /// colour.
    gibs: Vec<(DVec3, Option<Vec3>)>,
    next_serial: u64,
}

impl RagdollSystem {
    /// A world falling at `sv_gravity`, with no map yet.
    #[must_use]
    pub fn new(sv_gravity: f32) -> Self {
        Self {
            physics: PhysicsSystem::new(sv_gravity),
            ragdolls: Vec::new(),
            gibs: Vec::new(),
            next_serial: 0,
        }
    }

    #[must_use]
    pub fn ragdolls(&self) -> &[ActiveRagdoll] {
        &self.ragdolls
    }

    /// `spawn`: hands `instance`, posed under `world`, to physics and
    /// launches it along `dir`, drawn on in `style` (its tint and outline;
    /// none for an untinted body). Returns its serial for the follow handle
    /// ([`Self::position_of`]); none when the skeleton lacks a bone.
    pub fn spawn(
        &mut self,
        model: &PlayerModel,
        mut instance: PlayerModelInstance,
        world: Mat4,
        dir: DVec3,
        style: Option<&PlayerModelVisualStyle>,
    ) -> Option<u64> {
        let color = style.and_then(|s| s.tint_color);
        let outline = style.filter(|s| s.outline_enabled).map(|s| Outline {
            color: s.outline_color,
            width: s.outline_width,
        });
        while self.ragdolls.len() >= MAX_RAGDOLLS {
            let mut oldest = self.ragdolls.remove(0);
            oldest.ragdoll.dispose(&mut self.physics);
        }
        // The clips stop: physics owns the bones now.
        instance.clip = None;
        let config = create_bone_config();
        let skeleton = &model.model.skeleton;
        let Some(mut ragdoll) = Ragdoll::new(&mut self.physics, skeleton, &instance.locals, world, &config) else {
            eprintln!("[RagdollSystem] Missing bones; no ragdoll");
            return None;
        };
        ragdoll.ragdoll(&mut self.physics);
        let v = launch_velocity(dir);
        let velocity = to_rapier(Vec3::new(v.x as f32, v.y as f32, v.z as f32));
        for &handle in ragdoll.bodies() {
            if let Some(body) = self.physics.world.bodies.get_mut(handle) {
                body.set_linvel(velocity, true);
                body.set_gravity_scale(RAGDOLL_GRAVITY_FACTOR, true);
            }
        }
        let trail = hips_pose(&self.physics, &ragdoll).map(|pose| TrailMesh::new(pose, TRAIL_DIAMETER, TRAIL_LENGTH));
        self.next_serial += 1;
        self.ragdolls.push(ActiveRagdoll {
            serial: self.next_serial,
            instance,
            world,
            ragdoll,
            age: 0.0,
            color,
            outline,
            rest_anchor: None,
            rest_seconds: 0.0,
            fade_start: None,
            trail,
        });
        Some(self.next_serial)
    }

    /// The handle's `getPosition`: a ragdoll's hips, none once it is gone.
    #[must_use]
    pub fn position_of(&self, serial: u64) -> Option<DVec3> {
        let r = self.ragdolls.iter().find(|r| r.serial == serial)?;
        r.ragdoll.root_position(&self.physics).map(|p| p.as_dvec3())
    }

    /// The hips' world position.
    #[must_use]
    pub fn position(&self, index: usize) -> Option<DVec3> {
        let r = self.ragdolls.get(index)?;
        r.ragdoll.root_position(&self.physics).map(|p| p.as_dvec3())
    }

    /// `update`: physics steps, the bones follow, and each ragdoll ages,
    /// fades once still, and bursts when faded.
    pub fn update(&mut self, model: &PlayerModel, dt: f64) {
        let dt = dt.max(0.0);
        if !self.ragdolls.is_empty() {
            self.physics.step(dt);
        }
        let skeleton = &model.model.skeleton;
        let mut i = self.ragdolls.len();
        while i > 0 {
            i -= 1;
            let physics = &self.physics;
            let active = &mut self.ragdolls[i];
            active
                .ragdoll
                .sync(physics, skeleton, &mut active.instance.locals, active.world);
            active.age += dt;
            // The trail follows the hips each frame, then goes.
            if active.age >= TRAIL_LIFETIME {
                active.trail = None;
            } else if let (Some(trail), Some(pose)) = (&mut active.trail, hips_pose(physics, &active.ragdoll)) {
                trail.update(pose);
            }
            let hips = active.ragdoll.root_position(physics).map(|p| p.as_dvec3());
            if active.fade_start.is_none() {
                if came_to_rest(active, hips, dt) {
                    active.fade_start = Some(active.age);
                } else if active.age >= RAGDOLL_LIFETIME - RAGDOLL_FADE {
                    active.fade_start = Some(RAGDOLL_LIFETIME - RAGDOLL_FADE);
                }
            }
            if active.fade_start.is_some() && active.fade() <= 0.0 {
                let mut gone = self.ragdolls.remove(i);
                let at = hips.unwrap_or_else(|| gone.world.w_axis.truncate().as_dvec3());
                self.gibs.push((at, gone.color));
                gone.ragdoll.dispose(&mut self.physics);
            }
        }
    }

    /// The bursts since the last call (`onGibExplode`).
    pub fn take_gibs(&mut self) -> Vec<(DVec3, Option<Vec3>)> {
        std::mem::take(&mut self.gibs)
    }

    /// `disposeAll`: every ragdoll goes, without gibs.
    pub fn dispose_all(&mut self) {
        for mut r in self.ragdolls.drain(..) {
            r.ragdoll.dispose(&mut self.physics);
        }
    }

    /// Each ragdoll's draws, its joint palettes appended to `joints`: lit
    /// while whole, through `fade_handle` (the model uploaded to blend) at
    /// its fade once fading.
    pub fn draws(
        &self,
        model: &PlayerModel,
        handle: ModelHandle,
        fade_handle: ModelHandle,
        joints: &mut Vec<Mat4>,
        draws: &mut Vec<ModelDraw>,
    ) {
        for active in &self.ragdolls {
            let fading = active.fade_start.is_some();
            let mut palettes: Vec<(usize, u32)> = Vec::new();
            for (index, primitive) in model.model.primitives.iter().enumerate() {
                let Some(skin) = &primitive.skin else { continue };
                let offset = if let Some(&(_, at)) = palettes.iter().find(|(s, _)| *s == skin.skin) {
                    at
                } else {
                    let at = joints.len() as u32;
                    joints.extend(model.joint_matrices(&active.instance, skin.skin));
                    palettes.push((skin.skin, at));
                    at
                };
                draws.push(ModelDraw {
                    tint: active.color,
                    skin: Some(offset),
                    // A blended (fading) draw has no outline pass.
                    outline: active.outline,
                    alpha: fading.then_some(active.fade() as f32),
                    ..ModelDraw::new(if fading { fade_handle } else { handle }, index, active.world)
                });
            }
        }
    }
}

/// The hips box's world matrix (the trail's generator).
fn hips_pose(physics: &PhysicsSystem, ragdoll: &Ragdoll) -> Option<DMat4> {
    let body = physics.world.bodies.get(ragdoll.get_aggregate(0)?)?;
    Some(DMat4::from_rotation_translation(
        quat_from_rapier(*body.rotation()).as_dquat(),
        from_rapier(body.translation()).as_dvec3(),
    ))
}

/// The ragdolls' speed trails: a mesh per slot, uploaded on first use.
#[derive(Default)]
pub struct RagdollTrails {
    slots: Vec<ModelHandle>,
}

impl RagdollTrails {
    /// Streams each live trail into a slot and draws it in its body's
    /// colour, clamped (team colours run over 1), white without one.
    pub fn draws(
        &mut self,
        gpu: &Gpu,
        models: &mut ModelRenderer,
        ragdolls: &RagdollSystem,
        draws: &mut Vec<ModelDraw>,
    ) {
        let mut slot = 0;
        for active in ragdolls.ragdolls() {
            let Some(trail) = &active.trail else { continue };
            if slot == self.slots.len() {
                // `ragdoll_trail_mat`: unlit, both sides, alpha 0.6.
                let material = StandardMaterialParams {
                    emissive_color: Vec3::ONE,
                    diffuse_color: Vec3::ONE,
                    disable_lighting: true,
                    back_face_culling: false,
                    alpha: 0.6,
                    ..StandardMaterialParams::default()
                };
                self.slots.push(upload_standard_mesh(
                    gpu,
                    models,
                    &[("ragdoll_trail", &trail.data)],
                    material,
                ));
            } else {
                update_standard_mesh(gpu, models, self.slots[slot], 0, &trail.data);
            }
            let color = active.color.map_or(Vec3::ONE, |c| c.min(Vec3::ONE));
            draws.push(ModelDraw {
                tint: Some(color),
                emissive: Some(color),
                alpha: Some(active.trail_alpha() as f32),
                ..ModelDraw::new(self.slots[slot], 0, Mat4::IDENTITY)
            });
            slot += 1;
        }
    }
}

/// `cameToRest`: whether the hips have stayed within [`REST_RADIUS`] for
/// [`REST_SECONDS`], advancing that check by `dt`.
fn came_to_rest(active: &mut ActiveRagdoll, hips: Option<DVec3>, dt: f64) -> bool {
    if active.age < REST_MIN_AGE {
        return false;
    }
    let Some(hips) = hips else { return false };
    match active.rest_anchor {
        Some(anchor) if hips.distance_squared(anchor) <= REST_RADIUS * REST_RADIUS => {
            active.rest_seconds += dt.max(0.0);
        }
        _ => {
            active.rest_anchor = Some(hips);
            active.rest_seconds = 0.0;
        }
    }
    active.rest_seconds >= REST_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_follows_the_push() {
        let v = launch_velocity(DVec3::new(3.0, 4.0, 0.0));
        assert!((v - DVec3::new(402.0, 536.0, 0.0)).length() < 1e-9);
        assert_eq!(launch_velocity(DVec3::ZERO), DVec3::new(0.0, 670.0, 0.0));
    }

    #[test]
    fn the_config_covers_fifteen_bones() {
        let config = create_bone_config();
        assert_eq!(config.len(), 15);
        assert_eq!(config[0].bone, "mixamorig:Hips");
        assert_eq!(config[0].mass, Some(2.0));
        assert_eq!(config[1].mass, None);
        assert_eq!(config[10].joint, Some(PhysicsConstraintType::Hinge));
        assert!((config[9].height.unwrap() - 14.4).abs() < 1e-4);
    }
}
