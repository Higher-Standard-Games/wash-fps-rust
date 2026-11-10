//! wash-fps `world/pickups/ItemSparkles.ts`: small white sparkles that rise
//! slowly from an item and fade out.
//!
//! Each is a tiny camera-facing quad under the item. Babylon places a
//! billboard under a parent by the parent's scaling and position only
//! (`BillboardUseParentOrientation` is off), so they bob with the item and
//! grow with its scale without turning with it. They stop with the item
//! when it is collected.

use std::io::Cursor;

use shagger::babylon::materials::standard_material::{StandardAlphaMode, StandardMaterialParams, TextureUse};
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelImage, ModelMaterial, ModelPrimitive, Skeleton};
use shagger::rendering::glam::{DMat3, DVec3, Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};

/// Sparkles per item, each rising and fading in turn, so about COUNT / LIFE
/// are born a second.
pub const COUNT: usize = 16;
/// Seconds a sparkle lives.
const LIFE_MIN: f64 = 2.0;
const LIFE_MAX: f64 = 3.2;
/// In the model's own units (the packaged items are ~1.9 across, drawn
/// ~18x): born just inside its shell, rising this fast, this large at their
/// brightest.
const SHELL_RADIUS: f64 = 0.8;
const RISE_MIN: f64 = 0.35;
const RISE_MAX: f64 = 0.6;
const SIZE_MIN: f64 = 0.04;
const SIZE_MAX: f64 = 0.08;
/// The dot texture's size (`sparkleMaterial`'s canvas).
const DOT: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Sparkle {
    position: DVec3,
    age: f64,
    life: f64,
    rise: f64,
    size: f64,
}

/// One item's sparkles (`ItemSparkles`).
#[derive(Debug, Clone)]
pub struct ItemSparkles {
    sparkles: Vec<Sparkle>,
    /// `Math.random`'s stand-in: a xorshift seeded by the item.
    seed: u64,
    last: Option<f64>,
}

impl ItemSparkles {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut out = Self {
            sparkles: Vec::with_capacity(COUNT),
            seed: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
            last: None,
        };
        for _ in 0..COUNT {
            let mut sparkle = Sparkle {
                position: DVec3::ZERO,
                age: 0.0,
                life: 1.0,
                rise: 0.0,
                size: 0.0,
            };
            out.respawn(&mut sparkle);
            // Spread the first births over a lifetime, so they do not all
            // rise together
            sparkle.age = out.random() * sparkle.life;
            out.sparkles.push(sparkle);
        }
        out
    }

    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A new sparkle at a random point just inside the item's shell.
    fn respawn(&mut self, sparkle: &mut Sparkle) {
        let u = self.random() * 2.0 - 1.0;
        let angle = self.random() * std::f64::consts::PI * 2.0;
        let ring = (1.0 - u * u).sqrt() * SHELL_RADIUS;
        sparkle.position = DVec3::new(angle.cos() * ring, u * SHELL_RADIUS, angle.sin() * ring);
        sparkle.age = 0.0;
        sparkle.life = LIFE_MIN + self.random() * (LIFE_MAX - LIFE_MIN);
        sparkle.rise = RISE_MIN + self.random() * (RISE_MAX - RISE_MIN);
        sparkle.size = SIZE_MIN + self.random() * (SIZE_MAX - SIZE_MIN);
    }

    /// Ages and lifts the sparkles to `now` seconds; a hidden item's rest.
    pub fn update(&mut self, now: f64, shown: bool) {
        // Clamped, so a stall or an item coming back into view does not
        // jump them
        let dt = self.last.map_or(0.0, |last| (now - last).min(0.1));
        self.last = Some(now);
        if !shown {
            return;
        }
        let mut sparkles = std::mem::take(&mut self.sparkles);
        for sparkle in &mut sparkles {
            sparkle.age += dt;
            if sparkle.age >= sparkle.life {
                self.respawn(sparkle);
            }
            sparkle.position.y += sparkle.rise * dt;
        }
        self.sparkles = sparkles;
    }

    /// Each sparkle's local position and scaling: grown in quickly, then
    /// shrunk away over the rest of the climb.
    fn placed(&self) -> impl Iterator<Item = (DVec3, f64)> + '_ {
        self.sparkles.iter().map(|s| {
            let t = s.age / s.life;
            let fade = if t < 0.15 { t / 0.15 } else { (1.0 - t) / 0.85 };
            (s.position, s.size * fade)
        })
    }

    /// The sparkles' draws under an item at `item_position` drawn at
    /// `item_scale`, facing the camera (`camera` turns camera axes to world).
    pub fn draws(
        &self,
        quad: ModelHandle,
        item_position: DVec3,
        item_scale: f64,
        camera: DMat3,
        draws: &mut Vec<ModelDraw>,
    ) {
        let facing = camera.as_mat3();
        for (local, scale) in self.placed() {
            if scale <= 0.0 {
                continue;
            }
            let at = (item_position + local * item_scale).as_vec3();
            let world = Mat4::from_translation(at)
                * Mat4::from_mat3(facing)
                * Mat4::from_scale(Vec3::splat((scale * item_scale) as f32));
            draws.push(ModelDraw::new(quad, 0, world));
        }
    }
}

/// The soft white dot (`sparkleMaterial`'s canvas radial gradient), grey
/// for its opacity read from RGB.
fn dot_pixels() -> Vec<u8> {
    let mut out = Vec::with_capacity((DOT * DOT * 4) as usize);
    let half = f64::from(DOT) / 2.0;
    for y in 0..DOT {
        for x in 0..DOT {
            let r = ((f64::from(x) + 0.5 - half).hypot(f64::from(y) + 0.5 - half) / half).min(1.0);
            let alpha = if r < 0.4 {
                1.0 - r / 0.4 * 0.4
            } else {
                0.6 * (1.0 - (r - 0.4) / 0.6)
            };
            let v = (alpha * 255.0).round() as u8;
            out.extend_from_slice(&[v, v, v, 255]);
        }
    }
    out
}

/// `sparkleMaterial`: unlit white, its dot as opacity, added onto what is
/// behind it, without depth writes, both sides.
#[must_use]
pub fn sparkle_material() -> StandardMaterialParams {
    StandardMaterialParams {
        disable_lighting: true,
        diffuse_color: Vec3::ZERO,
        specular_color: Vec3::ZERO,
        emissive_color: Vec3::ONE,
        alpha_blend: true,
        alpha_mode: StandardAlphaMode::Add,
        disable_depth_write: true,
        back_face_culling: false,
        texture: TextureUse {
            opacity_rgb: true,
            ..TextureUse::default()
        },
        ..StandardMaterialParams::default()
    }
}

/// The sparkle quad (`CreatePlane` of size 1) on the GPU.
pub fn upload_sparkle_quad(gpu: &Gpu, models: &mut ModelRenderer) -> ModelHandle {
    let mut png = Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut png,
        &dot_pixels(),
        DOT,
        DOT,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .expect("the dot encodes");
    let model = GltfModel {
        primitives: vec![ModelPrimitive {
            name: "item_sparkle".to_owned(),
            node: 0,
            node_scale: Vec3::ONE,
            node_matrix: Mat4::IDENTITY,
            positions: vec![[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]],
            normals: vec![[0.0, 0.0, -1.0]; 4],
            uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            material: Some(0),
            bounds_center: Vec3::ZERO,
            creation_order: 0,
            skin: None,
        }],
        materials: vec![ModelMaterial {
            name: "item_sparkle".to_owned(),
            base_color: [1.0; 4],
            base_color_image: Some(0),
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            alpha_mode: AlphaMode::Blend,
            alpha_cutoff: 0.5,
            double_sided: true,
        }],
        images: vec![ModelImage {
            name: "item_sparkle_dot.png".to_owned(),
            mime_type: Some("image/png".to_owned()),
            bytes: png.into_inner(),
        }],
        attachment_points: Default::default(),
        skeleton: Skeleton::default(),
    };
    models.upload(gpu, &model, |_| ModelMaterialMode::Standard(sparkle_material()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkles_rise_fade_and_respawn_inside_the_shell() {
        let mut s = ItemSparkles::new(7);
        assert_eq!(s.sparkles.len(), COUNT);
        for p in &s.sparkles {
            assert!(p.position.length() <= SHELL_RADIUS + 1e-9);
            assert!((LIFE_MIN..LIFE_MAX).contains(&p.life));
            assert!(p.age < p.life);
        }
        let before: Vec<f64> = s.sparkles.iter().map(|p| p.position.y).collect();
        s.update(0.0, true);
        s.update(0.05, true);
        for (p, y) in s.sparkles.iter().zip(before) {
            if p.age >= 0.05 {
                assert!((p.position.y - y - p.rise * 0.05).abs() < 1e-9);
            }
        }
        // A stall moves them at most a tenth of a second.
        let ages: Vec<f64> = s.sparkles.iter().map(|p| p.age).collect();
        s.update(10.0, true);
        for (p, a) in s.sparkles.iter().zip(ages) {
            assert!(p.age <= a + 0.1 + 1e-9);
        }
        // Hidden, they rest.
        let rest = s.sparkles.clone();
        s.update(10.05, false);
        assert_eq!(s.sparkles, rest);
    }

    #[test]
    fn the_dot_is_soft() {
        let dot = dot_pixels();
        let at = |x: u32, y: u32| dot[((y * DOT + x) * 4) as usize];
        assert!(at(16, 16) > 230);
        assert_eq!(at(0, 0), 0);
        assert!(at(16, 16) > at(22, 16) && at(22, 16) > at(28, 16));
    }
}
