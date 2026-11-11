//! Port of wash-fps `client/RagdollSystem.ts`: a body that dies without
//! gibbing falls as a ragdoll, one box per major bone, launched along the
//! killing push; once it lies still (or after [`RAGDOLL_LIFETIME`] at the
//! latest) it fades out and bursts into gibs in its colour. A death with a
//! clip (`chooseDeathClip`) is thrown first: the body turns to the shot,
//! one sphere flies the launch arc with the clip posed on the body, until
//! the sphere touches the map or the clip ends, and the ragdoll takes over
//! at the sphere's velocity.
//!
//! Havok becomes shagger's rapier [`PhysicsSystem`] and Babylon's `Ragdoll`
//! its port in `shagger::babylon::physics::v2::ragdoll`. The ragdoll owns
//! the dead player's pose (`PlayerModelInstance`) and the body matrix it
//! died with; the bones move under it. `BatchedPoseSync` (a speed-up of the
//! same sync) has no counterpart; the follow handle is the ragdoll's serial
//! ([`RagdollSystem::position_of`]). The speed trail is Babylon's
//! `TrailMesh` (shagger's port) behind the hips box, one uploaded mesh per
//! ragdoll slot, its vertices streamed each frame.

use shagger::animation::player_model_manager::PlayerModelInstance;
use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::meshes::trail_mesh::TrailMesh;
use shagger::babylon::physics::v2::ragdoll::{PhysicsConstraintType, Ragdoll, RagdollBoneProperties};
use shagger::loader::model::gltf_animation::GLTF_TARGET_FPS;
use shagger::physics::physics_system::rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, LockedAxes, RigidBodyBuilder, RigidBodyHandle,
};
use shagger::physics::physics_system::{PhysicsSystem, debris_groups, from_rapier, quat_from_rapier, to_rapier};
use shagger::rendering::glam::{DMat4, DQuat, DVec3, Mat4, Quat, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelRenderer};
use shagger::weapon::projectile_manager::{update_standard_mesh, upload_standard_mesh};

use super::ragdoll::death_clip::{DeathFacts, choose_death_clip, death_clip_yaw};
use super::remote_player_renderer::PlayerBody;

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
/// The sphere a thrown body flies as, centred on the hips (`THROW_RADIUS`).
pub const THROW_RADIUS: f32 = 34.0;
/// Contacts this early do not end the throw (`THROW_CONTACT_GRACE`).
pub const THROW_CONTACT_GRACE: f64 = 0.1;
/// Hips height above the anchor when the body has none to measure
/// (`THROW_HIPS_HEIGHT`).
pub const THROW_HIPS_HEIGHT: f64 = 34.0;

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

/// `DeathThrow`: a body playing its death clip while one sphere flies the
/// launch arc.
struct DeathThrow {
    body: RigidBodyHandle,
    collider: ColliderHandle,
    hips_height: f64,
    /// The yaw the body turned to (`deathClipYaw`).
    yaw: f64,
    clip: usize,
    from_frame: f64,
    clip_length: f64,
    clip_time: f64,
    /// Set by the sphere's contacts once past the grace time.
    touched: bool,
    /// The normal of the surface it touched.
    surface: Option<Vec3>,
}

/// `ActiveRagdoll`.
pub struct ActiveRagdoll {
    /// Its number, for the follow handle.
    pub serial: u64,
    /// The body it wears, for its clips and its draws.
    pub body: PlayerBody,
    pub instance: PlayerModelInstance,
    /// The body matrix (anchor, yaw, fit and scale): the one it died with,
    /// or the throw's last.
    pub world: Mat4,
    /// None while the body is still thrown.
    ragdoll: Option<Ragdoll>,
    throw: Option<DeathThrow>,
    /// The trail rides a node at the hips that never turns, after a throw.
    trail_follower: bool,
    pub age: f64,
    /// The colour the body was drawn with; its gibs take it.
    pub color: Option<Vec3>,
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
    /// launches it along `dir`; with `facts` from its death event it may be
    /// thrown playing a death clip first. Returns its serial for the follow
    /// handle ([`Self::position_of`]); none when the skeleton lacks a bone.
    pub fn spawn(
        &mut self,
        body: PlayerBody,
        mut instance: PlayerModelInstance,
        world: Mat4,
        dir: DVec3,
        color: Option<Vec3>,
        facts: Option<&DeathFacts<'_>>,
    ) -> Option<u64> {
        while self.ragdolls.len() >= MAX_RAGDOLLS {
            let mut oldest = self.ragdolls.remove(0);
            oldest.release(&mut self.physics);
        }
        let model = &body.model;
        // The clips stop: physics, or the throw, owns the bones now.
        model.release(&mut instance);
        let skeleton = &model.model.skeleton;
        if create_bone_config()
            .iter()
            .any(|cfg| skeleton.find(&cfg.bone).is_none())
        {
            eprintln!("[RagdollSystem] Missing bones; no ragdoll");
            return None;
        }
        // The clip is chosen against the way the body faced when it died.
        let anchor = world * model.root_matrix(MODEL_SCALE).inverse();
        let facing = anchor.transform_vector3(Vec3::Z);
        let yaw = f64::from(facing.x).atan2(f64::from(facing.z));
        let clip = facts
            .and_then(|f| choose_death_clip(f, yaw))
            .and_then(|c| model.anim_index(c.name()).map(|i| (c, i)));
        let v = launch_velocity(dir);
        let mut active = ActiveRagdoll {
            serial: self.next_serial + 1,
            body: body.clone(),
            instance,
            world,
            ragdoll: None,
            throw: None,
            trail_follower: false,
            age: 0.0,
            color,
            rest_anchor: None,
            rest_seconds: 0.0,
            fade_start: None,
            trail: None,
        };
        if let Some((clip, index)) = clip {
            let turn = death_clip_yaw(clip, dir.x, dir.z);
            active.throw = Some(self.start_throw(&mut active, anchor, index, turn, v));
        } else if !self.activate_ragdoll(&mut active, v.as_vec3()) {
            return None;
        }
        active.trail = self
            .trail_pose(&active)
            .map(|pose| TrailMesh::new(pose, TRAIL_DIAMETER, TRAIL_LENGTH));
        self.next_serial += 1;
        self.ragdolls.push(active);
        Some(self.next_serial)
    }

    /// `startThrow`: turns the body to the shot and flies it as one sphere,
    /// with the death clip posed frame by frame from `update`.
    fn start_throw(
        &mut self,
        active: &mut ActiveRagdoll,
        anchor: Mat4,
        clip: usize,
        yaw: f64,
        velocity: DVec3,
    ) -> DeathThrow {
        let model = &active.body.model;
        // Measured before the turn.
        let anchor_pos = anchor.w_axis.truncate().as_dvec3();
        let skeleton = &model.model.skeleton;
        let measured = active.instance.hips_bind.map_or(f64::NAN, |(hips, _)| {
            let globals = skeleton.global_matrices(&active.instance.locals);
            f64::from((active.world * globals[hips]).w_axis.y) - anchor_pos.y
        });
        let hips_height = if measured > 5.0 && measured < 80.0 {
            measured
        } else {
            THROW_HIPS_HEIGHT
        };
        let anim = &model.anims[clip];
        let from_frame = anim.from;
        model.pose_clip(&mut active.instance, clip, from_frame);
        let at = anchor_pos + DVec3::new(0.0, hips_height, 0.0);
        let (body, collider) = self.physics.world.insert(
            // The sphere only carries the body's position: it never tumbles.
            RigidBodyBuilder::dynamic()
                .translation(to_rapier(at.as_vec3()))
                .linvel(to_rapier(velocity.as_vec3()))
                .gravity_scale(RAGDOLL_GRAVITY_FACTOR)
                .locked_axes(LockedAxes::ROTATION_LOCKED)
                .ccd_enabled(true),
            ColliderBuilder::ball(THROW_RADIUS)
                .mass(1.0)
                .restitution(0.0)
                .collision_groups(debris_groups()),
        );
        let thrown = DeathThrow {
            body,
            collider,
            hips_height,
            yaw,
            clip,
            from_frame,
            clip_length: (anim.to - anim.from) / GLTF_TARGET_FPS,
            clip_time: 0.0,
            touched: false,
            surface: None,
        };
        active.world = throw_world(&active.body, &thrown, at);
        thrown
    }

    /// `activateRagdoll`: the per-bone ragdoll from the body's current pose,
    /// every bone launched at `velocity`. False when a bone is missing.
    fn activate_ragdoll(&mut self, active: &mut ActiveRagdoll, velocity: Vec3) -> bool {
        let config = create_bone_config();
        let skeleton = &active.body.model.model.skeleton;
        let Some(mut ragdoll) = Ragdoll::new(
            &mut self.physics,
            skeleton,
            &active.instance.locals,
            active.world,
            &config,
        ) else {
            eprintln!("[RagdollSystem] Missing bones; no ragdoll");
            return false;
        };
        ragdoll.ragdoll(&mut self.physics);
        let velocity = to_rapier(velocity);
        for &handle in ragdoll.bodies() {
            if let Some(body) = self.physics.world.bodies.get_mut(handle) {
                body.set_linvel(velocity, true);
                body.set_gravity_scale(RAGDOLL_GRAVITY_FACTOR, true);
            }
        }
        active.ragdoll = Some(ragdoll);
        true
    }

    /// `advanceThrow`: the clip moves on and the body rides its sphere; a
    /// touch or the clip's end hands it to the ragdoll.
    fn advance_throw(&mut self, index: usize, dt: f64) {
        let active = &mut self.ragdolls[index];
        let Some(thrown) = active.throw.as_mut() else { return };
        // The pose drawn last frame is the one the ragdoll starts from.
        if thrown.touched || thrown.clip_time >= thrown.clip_length {
            self.end_throw(index);
            return;
        }
        thrown.clip_time = (thrown.clip_time + dt.max(0.0)).min(thrown.clip_length);
        let frame = thrown.from_frame + thrown.clip_time * GLTF_TARGET_FPS;
        active.body.model.pose_clip(&mut active.instance, thrown.clip, frame);
        if let Some(body) = self.physics.world.bodies.get(thrown.body) {
            let at = from_rapier(body.translation()).as_dvec3();
            active.world = throw_world(&active.body, thrown, at);
        }
    }

    /// `endThrow`: the body goes limp, moving as its sphere was, less what
    /// carried it into the surface it landed on.
    fn end_throw(&mut self, index: usize) {
        let Some(thrown) = self.ragdolls[index].throw.take() else {
            return;
        };
        let mut velocity = self
            .physics
            .world
            .bodies
            .get(thrown.body)
            .map_or(Vec3::ZERO, |b| from_rapier(b.linvel()));
        if let Some(normal) = thrown.surface.filter(|n| n.length_squared() > 1e-6) {
            let normal = normal.normalize();
            velocity -= normal * velocity.dot(normal);
        }
        self.physics.world.remove_body(thrown.body);
        let mut active = self.ragdolls.remove(index);
        active.trail_follower = active.trail.is_some();
        if self.activate_ragdoll(&mut active, velocity) {
            self.ragdolls.insert(index, active);
        } else {
            // A body that cannot go limp bursts where it is.
            let at = active.world.w_axis.truncate().as_dvec3();
            self.gibs.push((at, active.color));
        }
    }

    /// The trail's generator: the hips box, or a node at the hips (the
    /// sphere's while thrown) that never turns.
    fn trail_pose(&self, active: &ActiveRagdoll) -> Option<DMat4> {
        if active.throw.is_some() || active.trail_follower {
            return self
                .body_position(active)
                .map(|p| DMat4::from_rotation_translation(DQuat::IDENTITY, p));
        }
        hips_pose(&self.physics, active.ragdoll.as_ref()?)
    }

    /// `bodyPosition`: the hips, or the sphere while thrown.
    fn body_position(&self, active: &ActiveRagdoll) -> Option<DVec3> {
        if let Some(thrown) = &active.throw {
            return self
                .physics
                .world
                .bodies
                .get(thrown.body)
                .map(|b| from_rapier(b.translation()).as_dvec3());
        }
        active
            .ragdoll
            .as_ref()?
            .root_position(&self.physics)
            .map(|p| p.as_dvec3())
    }

    /// The handle's `getPosition`: a ragdoll's hips, none once it is gone.
    #[must_use]
    pub fn position_of(&self, serial: u64) -> Option<DVec3> {
        let r = self.ragdolls.iter().find(|r| r.serial == serial)?;
        self.body_position(r)
    }

    /// The hips' world position (the sphere's while thrown).
    #[must_use]
    pub fn position(&self, index: usize) -> Option<DVec3> {
        self.body_position(self.ragdolls.get(index)?)
    }

    /// `update`: physics steps, thrown bodies ride their spheres and the
    /// bones follow, and each ragdoll ages, fades once still, and bursts
    /// when faded.
    pub fn update(&mut self, dt: f64) {
        let dt = dt.max(0.0);
        if !self.ragdolls.is_empty() {
            self.physics.step(dt);
        }
        // The spheres' contacts, once past the grace time.
        for active in &mut self.ragdolls {
            let Some(thrown) = active.throw.as_mut() else { continue };
            if thrown.clip_time < THROW_CONTACT_GRACE {
                continue;
            }
            for pair in self.physics.world.contact_pairs_with(thrown.collider) {
                if !pair.has_any_active_contact() {
                    continue;
                }
                thrown.touched = true;
                if let Some(m) = pair.manifolds().first() {
                    thrown.surface = Some(from_rapier(m.data.normal));
                }
            }
        }
        let mut i = self.ragdolls.len();
        while i > 0 {
            i -= 1;
            self.ragdolls[i].age += dt;
            if self.ragdolls[i].throw.is_some() {
                let before = self.ragdolls.len();
                self.advance_throw(i, dt);
                if self.ragdolls.len() < before {
                    continue;
                }
            }
            let pose = self.trail_pose(&self.ragdolls[i]);
            let hips = self.body_position(&self.ragdolls[i]);
            let physics = &self.physics;
            let active = &mut self.ragdolls[i];
            if let Some(ragdoll) = &active.ragdoll {
                let skeleton = &active.body.model.model.skeleton;
                ragdoll.sync(physics, skeleton, &mut active.instance.locals, active.world);
            }
            // The trail follows the hips each frame, then goes.
            if active.age >= TRAIL_LIFETIME {
                active.trail = None;
                active.trail_follower = false;
            } else if let (Some(trail), Some(pose)) = (&mut active.trail, pose) {
                trail.update(pose);
            }
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
                gone.release(&mut self.physics);
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
            r.release(&mut self.physics);
        }
    }

    /// Each ragdoll's draws, its joint palettes appended to `joints`: lit
    /// while whole, through its body's model uploaded to blend at its fade
    /// once fading.
    pub fn draws(&self, joints: &mut Vec<Mat4>, draws: &mut Vec<ModelDraw>) {
        for active in &self.ragdolls {
            let model = &active.body.model;
            let (handle, fade_handle) = (active.body.handle, active.body.fade_handle);
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
                    alpha: fading.then_some(active.fade() as f32),
                    // The bones fly free of `world` once limp.
                    cull: false,
                    ..ModelDraw::new(if fading { fade_handle } else { handle }, index, active.world)
                });
            }
        }
    }
}

impl ActiveRagdoll {
    /// `releaseBody`: the throw's sphere and the ragdoll's bodies go.
    fn release(&mut self, physics: &mut PhysicsSystem) {
        if let Some(thrown) = self.throw.take() {
            physics.world.remove_body(thrown.body);
        }
        if let Some(ragdoll) = &mut self.ragdoll {
            ragdoll.dispose(physics);
        }
    }
}

/// A thrown body's matrix: the anchor `hips_height` below the sphere at
/// `at`, turned to the throw's yaw, then the mesh's fit and scale.
fn throw_world(body: &PlayerBody, thrown: &DeathThrow, at: DVec3) -> Mat4 {
    let anchor = (at - DVec3::new(0.0, thrown.hips_height, 0.0)).as_vec3();
    Mat4::from_translation(anchor)
        * Mat4::from_quat(Quat::from_rotation_y(thrown.yaw as f32))
        * body.model.root_matrix(MODEL_SCALE)
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
    // A thrown body is still flying.
    if active.age < REST_MIN_AGE || active.ragdoll.is_none() {
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
