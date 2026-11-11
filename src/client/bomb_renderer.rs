//! wash-fps `client/BombRenderer.ts`: the Search & Destroy bomb in the
//! world.
//!
//! One bundle of dynamite, three lives: slung on the carrier's back, lying
//! on the ground (bobbing and turning like a pickup), or planted at a site
//! with a light that blinks with the server's beep interval. A ring on the
//! floor marks the arm and defuse radius wherever the bomb sits. Everything
//! comes from the server's objective snapshot.
//!
//! Babylon builds the meshes under a transform node; here they are one
//! model of four primitives (sticks, strap, light, ring), the first three
//! drawn under the bomb's matrix and the ring under its own.

use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::meshes::mesh_builder::{create_box, create_cylinder, create_torus};
use shagger::babylon::meshes::vertex_data::{VertexData, babylon_local_matrix};
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelMaterial, Skeleton};
use shagger::rendering::glam::{DMat4, DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};
use shagger::shared::json::Value;

use crate::world::pickups::procedural::procedural_pickup_factory::primitive;

/// Dynamite stick radius and height, in world units.
const STICK_RADIUS: f64 = 2.6;
const STICK_HEIGHT: f64 = 13.0;
/// Sticks in the bundle, in a ring.
const STICK_COUNT: usize = 5;
const BUNDLE_RADIUS: f64 = 3.4;

/// Where on the carrier's back it rides, from the spine bone, in world
/// units (y down, z behind).
const BACK_OFFSET: DVec3 = DVec3::new(0.0, -6.0, 10.0);
const CARRY_SCALE: f64 = 0.75;
/// Shoulder height above the carrier's origin, when there is no bone.
pub const CARRY_FALLBACK_HEIGHT: f64 = 28.0;

/// Lift off the floor when dropped.
const GROUND_LIFT: f64 = 6.0;

/// Idle bob and spin while loose on the ground.
const LOOSE_SPIN_RATE: f64 = 1.4;
const LOOSE_BOB_AMPLITUDE: f64 = 2.5;
const LOOSE_BOB_RATE: f64 = 2.2;

/// The ring: `DEFAULT_BOMB_RULES.armDefuseRadius`.
const ZONE_RADIUS: f64 = 100.0;
const ZONE_THICKNESS: f64 = 3.2;
const ZONE_LIFT: f64 = 2.0;
const ZONE_TESSELLATION: u32 = 64;
/// Pulses per second while armed.
const ZONE_PULSE_HZ: f64 = 1.6;

/// The model's primitives, in order.
const STICKS: usize = 0;
const STRAP: usize = 1;
const LIGHT: usize = 2;
const ZONE: usize = 3;

/// The bomb's parts this frame (`update`): the bundle's matrix when it is
/// drawn, its timer light's emissive colour, and the ring (where, its
/// emissive colour and alpha) when it is on the floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BombPose {
    pub root: Option<DMat4>,
    pub light: Vec3,
    pub zone: Option<(DVec3, Vec3, f32)>,
}

/// The bomb's state as `BombObjectiveSnapshot` sends it.
#[derive(Debug, Clone, PartialEq)]
pub struct BombSnapshot {
    pub phase: String,
    pub position: DVec3,
    pub carrier: Option<String>,
    pub beep_interval: Option<f64>,
}

impl BombSnapshot {
    /// The objective when it is the bomb (`MatchStateManager.bomb`).
    #[must_use]
    pub fn from_objective(bomb: &Value) -> Option<Self> {
        let number = |k: &str| bomb.get(k).and_then(Value::as_f64);
        Some(Self {
            phase: bomb.get("phase")?.as_str()?.to_owned(),
            position: DVec3::new(number("x")?, number("y")?, number("z")?),
            carrier: bomb.get("carrier").and_then(Value::as_str).map(str::to_owned),
            beep_interval: number("beepInterval"),
        })
    }
}

/// The ring's emissive colour and alpha for a phase, `elapsed` seconds in.
#[must_use]
pub fn zone_look(phase: &str, elapsed: f64) -> (Vec3, f32) {
    match phase {
        "armed" => {
            let pulse = 0.5 + 0.5 * (elapsed * ZONE_PULSE_HZ * std::f64::consts::TAU).sin();
            (
                Vec3::new(0.85, (0.12 + pulse * 0.1) as f32, 0.1),
                (0.45 + pulse * 0.35) as f32,
            )
        }
        "planted" => (Vec3::new(0.95, 0.45, 0.08), 0.6),
        _ => (Vec3::new(0.98, 0.78, 0.15), 0.5),
    }
}

/// `updateLight`: a short flash at the top of each beep interval while
/// armed, a dim steady red while planted, dark otherwise.
#[must_use]
pub fn light_look(phase: &str, beep_interval: Option<f64>, elapsed: f64) -> Vec3 {
    match beep_interval {
        Some(interval) if phase == "armed" && interval > 0.0 => {
            if (elapsed % interval) / interval < 0.25 {
                Vec3::new(1.0, 0.15, 0.1)
            } else {
                Vec3::new(0.2, 0.02, 0.02)
            }
        }
        _ if phase == "planted" => Vec3::new(0.25, 0.02, 0.02),
        _ => Vec3::ZERO,
    }
}

/// `update`'s pose for a bomb `elapsed` seconds into its life. `anchor` is
/// the carrier's spine (or shoulder height) and yaw, when they are drawn.
#[must_use]
pub fn bomb_pose(bomb: &BombSnapshot, elapsed: f64, anchor: Option<(DVec3, f64)>) -> Option<BombPose> {
    if bomb.phase == "defused" || bomb.phase == "detonated" {
        return None;
    }
    let carried = bomb.phase == "carried";
    let root = if carried && bomb.carrier.is_some() {
        // `carry`: behind them by their own facing; nobody to hang it on
        // (out of view) hides it.
        anchor.map(|(at, yaw)| {
            let back = BACK_OFFSET.z;
            let position = DVec3::new(at.x - yaw.sin() * back, at.y + BACK_OFFSET.y, at.z - yaw.cos() * back);
            babylon_local_matrix(
                position,
                DVec3::new(std::f64::consts::FRAC_PI_2, yaw, 0.0),
                DVec3::splat(CARRY_SCALE),
            )
        })
    } else {
        // `place`: still and upright when planted, turning and bobbing loose.
        let planted = bomb.phase == "planted" || bomb.phase == "armed";
        let bob = if planted {
            0.0
        } else {
            (elapsed * LOOSE_BOB_RATE).sin() * LOOSE_BOB_AMPLITUDE
        };
        let spin = if planted { 0.0 } else { elapsed * LOOSE_SPIN_RATE };
        Some(babylon_local_matrix(
            bomb.position + DVec3::new(0.0, GROUND_LIFT + bob, 0.0),
            DVec3::new(0.0, spin, 0.0),
            DVec3::ONE,
        ))
    };
    let zone = (!carried).then(|| {
        let (color, alpha) = zone_look(&bomb.phase, elapsed);
        (bomb.position + DVec3::new(0.0, ZONE_LIFT, 0.0), color, alpha)
    });
    Some(BombPose {
        root,
        light: light_look(&bomb.phase, bomb.beep_interval, elapsed),
        zone,
    })
}

/// The bundle, strap, light and ring as model primitives, and each one's
/// material.
#[must_use]
pub fn bomb_model() -> (GltfModel, [StandardMaterialParams; 4]) {
    let mut sticks = VertexData::default();
    for i in 0..STICK_COUNT {
        let angle = (i as f64 / STICK_COUNT as f64) * std::f64::consts::TAU;
        let mut stick = create_cylinder(STICK_HEIGHT, STICK_RADIUS * 2.0, STICK_RADIUS * 2.0, 10);
        stick.bake(DMat4::from_translation(DVec3::new(
            angle.cos() * BUNDLE_RADIUS,
            0.0,
            angle.sin() * BUNDLE_RADIUS,
        )));
        sticks.merge(&stick);
    }
    let strap_diameter = (BUNDLE_RADIUS + STICK_RADIUS) * 2.15;
    let strap = create_cylinder(3.0, strap_diameter, strap_diameter, 14);
    let mut light = create_box(3.2, 3.2, 3.2);
    light.bake(DMat4::from_translation(DVec3::new(0.0, STICK_HEIGHT * 0.5, 0.0)));
    let zone = create_torus(ZONE_RADIUS * 2.0, ZONE_THICKNESS, ZONE_TESSELLATION);
    let parts = [
        ("bomb_sticks", sticks),
        ("bomb_strap", strap),
        ("bomb_light", light),
        ("bomb_zone", zone),
    ];
    let primitives = parts
        .iter()
        .enumerate()
        .map(|(i, (name, data))| primitive(name, data, i, i))
        .collect();
    let materials = (0..parts.len())
        .map(|i| ModelMaterial {
            name: format!("{i}"),
            base_color: [1.0; 4],
            base_color_image: None,
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            alpha_mode: if i == ZONE { AlphaMode::Blend } else { AlphaMode::Opaque },
            alpha_cutoff: 0.5,
            double_sided: false,
        })
        .collect();
    let body = StandardMaterialParams {
        diffuse_color: Vec3::new(0.42, 0.13, 0.09),
        specular_color: Vec3::splat(0.1),
        ..StandardMaterialParams::default()
    };
    let band = StandardMaterialParams {
        diffuse_color: Vec3::new(0.15, 0.13, 0.11),
        specular_color: Vec3::ZERO,
        ..StandardMaterialParams::default()
    };
    let light = StandardMaterialParams {
        diffuse_color: Vec3::new(0.5, 0.05, 0.05),
        specular_color: Vec3::ZERO,
        ..StandardMaterialParams::default()
    };
    let zone = StandardMaterialParams {
        diffuse_color: Vec3::ZERO,
        specular_color: Vec3::ZERO,
        alpha: 0.55,
        alpha_blend: true,
        disable_lighting: true,
        ..StandardMaterialParams::default()
    };
    let model = GltfModel {
        primitives,
        materials,
        images: Vec::new(),
        attachment_points: Default::default(),
        skeleton: Skeleton::default(),
    };
    (model, [body, band, light, zone])
}

/// `BombRenderer`: the model, uploaded once, and the bomb's clock.
pub struct BombRenderer {
    model: ModelHandle,
    /// Seconds for the loose bomb's bob and the light's blink.
    elapsed: f64,
    pose: Option<BombPose>,
}

impl BombRenderer {
    pub fn new(gpu: &Gpu, models: &mut ModelRenderer) -> Self {
        let (model, modes) = bomb_model();
        let model = models.upload(gpu, &model, |m| {
            ModelMaterialMode::Standard(modes[m.name.parse::<usize>().unwrap_or(0).min(3)])
        });
        Self {
            model,
            elapsed: 0.0,
            pose: None,
        }
    }

    /// `update`: the bomb this frame from the objective, `anchor` finding a
    /// drawn carrier's spine and yaw.
    pub fn update(&mut self, dt: f64, bomb: Option<&Value>, anchor: impl FnOnce(&str) -> Option<(DVec3, f64)>) {
        let Some(bomb) = bomb.and_then(BombSnapshot::from_objective) else {
            self.pose = None;
            return;
        };
        if bomb.phase == "defused" || bomb.phase == "detonated" {
            self.pose = None;
            return;
        }
        self.elapsed += dt;
        let anchor = match (&bomb.carrier, bomb.phase.as_str()) {
            (Some(carrier), "carried") => anchor(carrier),
            _ => None,
        };
        self.pose = bomb_pose(&bomb, self.elapsed, anchor);
    }

    /// The bomb's draws this frame.
    pub fn draws(&self, draws: &mut Vec<ModelDraw>) {
        let Some(pose) = self.pose else { return };
        if let Some(root) = pose.root {
            let world = root.as_mat4();
            draws.push(ModelDraw::new(self.model, STICKS, world));
            draws.push(ModelDraw::new(self.model, STRAP, world));
            draws.push(ModelDraw {
                emissive: Some(pose.light),
                ..ModelDraw::new(self.model, LIGHT, world)
            });
        }
        if let Some((at, color, alpha)) = pose.zone {
            draws.push(ModelDraw {
                emissive: Some(color),
                alpha: Some(alpha),
                ..ModelDraw::new(self.model, ZONE, DMat4::from_translation(at).as_mat4())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bomb(phase: &str) -> BombSnapshot {
        BombSnapshot {
            phase: phase.into(),
            position: DVec3::new(100.0, 20.0, -50.0),
            carrier: (phase == "carried").then(|| "p1".into()),
            beep_interval: (phase == "armed").then_some(1.0),
        }
    }

    #[test]
    fn a_loose_bomb_bobs_and_turns_over_its_ring() {
        let pose = bomb_pose(&bomb("dropped"), 0.0, None).unwrap();
        let at = pose.root.unwrap().w_axis.truncate();
        assert_eq!(at, DVec3::new(100.0, 26.0, -50.0));
        let (zone, color, alpha) = pose.zone.unwrap();
        assert_eq!(zone, DVec3::new(100.0, 22.0, -50.0));
        assert_eq!((color, alpha), (Vec3::new(0.98, 0.78, 0.15), 0.5));
        assert_eq!(pose.light, Vec3::ZERO);
    }

    #[test]
    fn a_carried_bomb_rides_behind_its_carrier_or_hides() {
        let carried = bomb("carried");
        let pose = bomb_pose(&carried, 1.0, Some((DVec3::new(0.0, 40.0, 0.0), 0.0))).unwrap();
        let root = pose.root.unwrap();
        // Behind: yaw 0 faces +Z, so the bomb is 10 units toward -Z, 6 down.
        assert!((root.w_axis.truncate() - DVec3::new(0.0, 34.0, -10.0)).length() < 1e-9);
        assert!((root.x_axis.truncate().length() - 0.75).abs() < 1e-9);
        assert!(pose.zone.is_none());
        assert!(bomb_pose(&carried, 1.0, None).unwrap().root.is_none());
    }

    #[test]
    fn the_armed_light_blinks_with_the_beep() {
        assert_eq!(light_look("armed", Some(1.0), 0.1), Vec3::new(1.0, 0.15, 0.1));
        assert_eq!(light_look("armed", Some(1.0), 0.5), Vec3::new(0.2, 0.02, 0.02));
        assert_eq!(light_look("planted", None, 0.5), Vec3::new(0.25, 0.02, 0.02));
        assert!(bomb_pose(&bomb("defused"), 0.0, None).is_none());
    }

    #[test]
    fn the_objective_reads_as_a_snapshot() {
        let v = shagger::shared::json::parse(
            r#"{"kind":"bomb","phase":"armed","x":1,"y":2,"z":3,"carrier":null,"beepInterval":0.5}"#,
        )
        .unwrap();
        let b = BombSnapshot::from_objective(&v).unwrap();
        assert_eq!(b.phase, "armed");
        assert_eq!(b.carrier, None);
        assert_eq!(b.beep_interval, Some(0.5));
    }
}
