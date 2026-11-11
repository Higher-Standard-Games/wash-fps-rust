//! Ground trails behind a sliding player (wash-fps `client/SlideDust.ts`):
//! pairs of small unlit grey cubes kicked out to both sides every stretch
//! of travel, which hop, tumble and fade. At most 128 at once, and only
//! within 1200 units of the camera.
//!
//! The browser draws one thin-instanced box whose faces carry six shades as
//! vertex colours; the model renderer has no vertex colours, so each face
//! is its own primitive with its shade as the unlit diffuse. `Math.random`
//! is a xorshift here: the dust is cosmetic and never reaches the
//! simulation.

use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::meshes::mesh_builder::create_box;
use shagger::babylon::meshes::vertex_data::{VertexData, babylon_local_matrix};
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelMaterial, Skeleton};
use shagger::physics::player_physics_constants::CROUCH_MINS_Z;
use shagger::rendering::glam::{DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};
use shagger::shared::js::js_hypot;

use crate::world::pickups::procedural::procedural_pickup_factory::primitive;

const CAPACITY: usize = 128;
const RANGE_SQUARED: f64 = 1200.0 * 1200.0;
const LIFETIME_SCALE: f64 = 1.2;
const GRAVITY: f64 = 45.0 / (LIFETIME_SCALE * LIFETIME_SCALE);
/// `faceColors`' shades, by face normal: +z, -z, +x, -x, +y, -y.
const FACE_SHADES: [(DVec3, f32); 6] = [
    (DVec3::Z, 0.8),
    (DVec3::NEG_Z, 0.6),
    (DVec3::X, 0.7),
    (DVec3::NEG_X, 0.5),
    (DVec3::Y, 0.85),
    (DVec3::NEG_Y, 0.55),
];

#[derive(Debug, Clone, Copy)]
struct Emitter {
    at: DVec3,
    distance: f64,
    phase: f64,
    seen: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Cube {
    active: bool,
    at: DVec3,
    v: DVec3,
    age: f64,
    life: f64,
    floor: f64,
    peak: f64,
    size: f64,
    rx: f64,
    ry: f64,
    rz: f64,
    spin: f64,
}

/// `SlideDust`'s pool.
#[derive(Debug)]
pub struct SlideDust {
    cubes: Vec<Cube>,
    emitters: Vec<(String, Emitter)>,
    elapsed: f64,
    cursor: usize,
    seed: u64,
    /// Where the camera was last frame, for the range check.
    pub eye: Option<DVec3>,
}

impl Default for SlideDust {
    fn default() -> Self {
        Self {
            cubes: vec![Cube::default(); CAPACITY],
            emitters: Vec::new(),
            elapsed: 0.0,
            cursor: 0,
            seed: 0x2f6b_9e3a_17c4_d851,
            eye: None,
        }
    }
}

impl SlideDust {
    fn random(&mut self) -> f64 {
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn forget(&mut self, id: &str) {
        self.emitters.retain(|(e, _)| e != id);
    }

    /// `emit`: `id` is at `at` (feet), moving `vx`, `vz`; `sliding` on
    /// the ground, out of water and off ladders.
    pub fn emit(&mut self, id: &str, at: DVec3, vx: f64, vz: f64, sliding: bool) {
        if !sliding {
            self.forget(id);
            return;
        }
        if !(at.x + at.y + at.z + vx + vz).is_finite() {
            return;
        }
        if let Some(eye) = self.eye
            && at.distance_squared(eye) > RANGE_SQUARED
        {
            self.forget(id);
            return;
        }
        let speed = js_hypot(&[vx, vz]);
        if speed < 80.0 {
            self.forget(id);
            return;
        }
        self.trace(id, at, vx / speed, vz / speed, (speed / 16.0).max(18.0));
    }

    fn trace(&mut self, id: &str, at: DVec3, dx: f64, dz: f64, spacing: f64) {
        let elapsed = self.elapsed;
        let Some(i) = self.emitters.iter().position(|(e, _)| e == id) else {
            self.emitters.push((
                id.to_owned(),
                Emitter {
                    at,
                    distance: 0.0,
                    phase: 0.0,
                    seen: elapsed,
                },
            ));
            return;
        };
        let mut last = self.emitters[i].1;
        let travel = js_hypot(&[at.x - last.at.x, at.z - last.at.z]);
        if travel > 160.0 || (at.y - last.at.y).abs() > 32.0 {
            last.distance = 0.0;
        } else {
            last.distance += travel;
            let mut pairs = 0;
            while last.distance >= spacing && pairs < 2 {
                pairs += 1;
                last.distance -= spacing;
                let t = if travel > 0.0 {
                    (1.0 - last.distance / travel).max(0.0)
                } else {
                    1.0
                };
                let x = last.at.x + (at.x - last.at.x) * t - dx * 4.0;
                let y = last.at.y + (at.y - last.at.y) * t + CROUCH_MINS_Z;
                let z = last.at.z + (at.z - last.at.z) * t - dz * 4.0;
                let spread = 2.0 + last.phase.sin() * 0.75;
                self.spawn(x + dz * spread, y, z - dx * spread, dz, -dx);
                self.spawn(x - dz * spread, y, z + dx * spread, -dz, dx);
                last.phase += 0.7;
            }
            last.distance = last.distance.min(spacing - 0.0001);
        }
        last.at = at;
        last.seen = elapsed;
        self.emitters[i].1 = last;
    }

    fn spawn(&mut self, x: f64, floor: f64, z: f64, side_x: f64, side_z: f64) {
        let mut slot = None;
        for _ in 0..CAPACITY {
            let i = self.cursor % CAPACITY;
            self.cursor += 1;
            if !self.cubes[i].active {
                slot = Some(i);
                break;
            }
        }
        let Some(i) = slot else { return };
        let vx = side_x * (10.0 + self.random() * 6.0);
        let vz = side_z * (10.0 + self.random() * 6.0);
        let vy = (12.0 + self.random() * 8.0) / LIFETIME_SCALE;
        let life = (0.8 + self.random() * 0.2) * LIFETIME_SCALE;
        let size = 3.0 + self.random() * 2.0;
        let (rx, ry, rz) = (self.random() * 3.0, self.random() * 3.0, self.random() * 3.0);
        let spin = (self.random() - 0.5) * 2.0;
        self.cubes[i] = Cube {
            active: true,
            at: DVec3::new(x, floor + 4.0, z),
            v: DVec3::new(vx, vy, vz),
            age: 0.0,
            life,
            floor,
            peak: vy / GRAVITY,
            size,
            rx,
            ry,
            rz,
            spin,
        };
    }

    /// `step`: the effect clock moves `seconds`.
    pub fn step(&mut self, seconds: f64) {
        let dt = if seconds.is_finite() {
            seconds.clamp(0.0, 0.1)
        } else {
            0.0
        };
        if dt == 0.0 {
            return;
        }
        self.elapsed += dt;
        let elapsed = self.elapsed;
        self.emitters.retain(|(_, e)| elapsed - e.seen <= 0.25);
        for cube in self.cubes.iter_mut().filter(|c| c.active) {
            cube.age += dt;
            if cube.age >= cube.life || cube.at.y <= cube.floor {
                cube.active = false;
                continue;
            }
            cube.at.x += cube.v.x * dt;
            cube.at.z += cube.v.z * dt;
            cube.at.y += cube.v.y * dt - GRAVITY * dt * dt * 0.5;
            cube.v.y -= GRAVITY * dt;
            cube.rx += cube.spin * dt;
            cube.ry += cube.spin * dt * 0.7;
        }
    }

    /// `reset`: a seek or a new match.
    pub fn reset(&mut self) {
        for cube in &mut self.cubes {
            cube.active = false;
        }
        self.emitters.clear();
    }

    /// The live cubes: world matrix and alpha (`upload`).
    fn instances(&self) -> impl Iterator<Item = (shagger::rendering::glam::Mat4, f32)> + '_ {
        self.cubes.iter().filter(|c| c.active).map(|cube| {
            let falling = ((cube.age - cube.peak) / (cube.life - cube.peak)).max(0.0);
            let f = falling.min(1.0);
            let fade = 1.0 - f * f * (3.0 - 2.0 * f);
            let landing = if cube.v.y < 0.0 {
                ((cube.at.y - cube.floor) / 5.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let alpha = 0.6 * (cube.age / 0.06).min(1.0) * fade * landing;
            let world = babylon_local_matrix(
                cube.at,
                DVec3::new(cube.rx, cube.ry, cube.rz),
                DVec3::splat(cube.size * (1.0 - falling * 0.6)),
            );
            (world.as_mat4(), alpha as f32)
        })
    }
}

/// The dust's box, a primitive a face.
pub struct SlideDustRenderer {
    model: ModelHandle,
}

impl SlideDustRenderer {
    pub fn new(gpu: &Gpu, models: &mut ModelRenderer) -> Self {
        let cube = create_box(1.0, 1.0, 1.0);
        let mut faces = Vec::new();
        for (face, quad) in cube.indices.chunks(6).enumerate() {
            let first = quad[0] as usize;
            let normal = DVec3::new(
                f64::from(cube.normals[first * 3]),
                f64::from(cube.normals[first * 3 + 1]),
                f64::from(cube.normals[first * 3 + 2]),
            );
            let shade = FACE_SHADES
                .iter()
                .find(|(n, _)| n.dot(normal) > 0.5)
                .map_or(0.7, |(_, s)| *s);
            let data = VertexData {
                positions: cube.positions.clone(),
                normals: cube.normals.clone(),
                uvs: cube.uvs.clone(),
                indices: quad.to_vec(),
            };
            faces.push((primitive(&format!("slide-dust-{face}"), &data, face, face), shade));
        }
        let model = GltfModel {
            primitives: faces.iter().map(|(p, _)| p.clone()).collect(),
            materials: (0..faces.len())
                .map(|i| ModelMaterial {
                    name: format!("slide-dust-{i}"),
                    base_color: [1.0; 4],
                    base_color_image: None,
                    metallic: 0.0,
                    roughness: 1.0,
                    metallic_roughness_image: None,
                    normal_image: None,
                    emissive: [0.0; 3],
                    emissive_image: None,
                    alpha_mode: AlphaMode::Blend,
                    alpha_cutoff: 0.5,
                    double_sided: false,
                })
                .collect(),
            images: Vec::new(),
            attachment_points: Default::default(),
            skeleton: Skeleton::default(),
        };
        let shades: Vec<f32> = faces.iter().map(|(_, s)| *s).collect();
        let model = models.upload(gpu, &model, |material| {
            let face = material.name.rsplit('-').next().and_then(|i| i.parse::<usize>().ok());
            let shade = face.and_then(|i| shades.get(i)).copied().unwrap_or(0.7);
            // Unlit: the emissive grey times the face's vertex shade.
            ModelMaterialMode::Standard(StandardMaterialParams {
                emissive_color: Vec3::splat(0.7 * shade),
                specular_color: Vec3::ZERO,
                disable_lighting: true,
                alpha_blend: true,
                alpha: 0.999,
                ..StandardMaterialParams::default()
            })
        });
        Self { model }
    }

    pub fn draws(&self, dust: &SlideDust, draws: &mut Vec<ModelDraw>) {
        for (world, alpha) in dust.instances() {
            for face in 0..6 {
                draws.push(ModelDraw {
                    alpha: Some(alpha),
                    ..ModelDraw::new(self.model, face, world)
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(dust: &mut SlideDust, frames: usize) {
        for i in 0..frames {
            dust.emit("p", DVec3::new(i as f64 * 5.0, 0.0, 0.0), 300.0, 0.0, true);
            dust.step(1.0 / 60.0);
        }
    }

    #[test]
    fn a_slide_leaves_pairs_of_cubes_that_settle() {
        let mut dust = SlideDust::default();
        slide(&mut dust, 30);
        let live = dust.cubes.iter().filter(|c| c.active).count();
        // 145 units at one pair each 18.75: seven pairs.
        assert_eq!(live, 14, "{live}");
        assert!(dust.cubes.iter().filter(|c| c.active).all(|c| c.at.y >= c.floor));
        for _ in 0..120 {
            dust.step(1.0 / 60.0);
        }
        assert_eq!(dust.instances().count(), 0);
    }

    #[test]
    fn nothing_when_slow_far_or_standing() {
        let mut dust = SlideDust::default();
        for i in 0..30 {
            dust.emit("p", DVec3::new(i as f64, 0.0, 0.0), 60.0, 0.0, true);
            dust.step(1.0 / 60.0);
        }
        dust.eye = Some(DVec3::new(5000.0, 0.0, 0.0));
        slide(&mut dust, 30);
        dust.eye = None;
        for i in 0..30 {
            dust.emit("p", DVec3::new(i as f64 * 5.0, 0.0, 0.0), 300.0, 0.0, false);
        }
        assert_eq!(dust.instances().count(), 0);
    }
}
