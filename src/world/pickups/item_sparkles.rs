//! wash-fps `world/pickups/ItemSparkles.ts`: small white sparkles that rise
//! slowly from an item and fade out.
//!
//! Each sparkle lives in its item's own space, so it turns and bobs with
//! it, and draws as a camera-facing dot (Babylon draws every sparkle of a
//! scene as thin instances of one dot). Only the nearest
//! [`MAX_SPARKLING_ITEMS`] drawn items within [`SPARKLE_RANGE`] of the
//! camera sparkle ([`choose_sparkling`]); the others rest.

use std::io::Cursor;

use shagger::babylon::materials::standard_material::{StandardAlphaMode, StandardMaterialParams, TextureUse};
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelImage, ModelMaterial, ModelPrimitive, Skeleton};
use shagger::rendering::glam::{DMat3, DVec3, Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};

/// Seconds a sparkle lives.
const LIFE_MIN: f64 = 2.0;
const LIFE_MAX: f64 = 3.2;
/// In world units: rising this fast, this large at their brightest.
const RISE_MIN: f64 = 6.3;
const RISE_MAX: f64 = 10.8;
const SIZE_MIN: f64 = 0.72;
const SIZE_MAX: f64 = 1.44;
/// Born just inside the item: this share of its bounding radius.
const SHELL_SHARE: f64 = 0.85;

/// Only items this close to the camera sparkle, and only the nearest few
/// of them.
pub const SPARKLE_RANGE: f64 = 900.0;
pub const MAX_SPARKLING_ITEMS: usize = 8;
/// The most sparkles one item has; more are not made.
pub const MAX_SPARKLES_PER_ITEM: usize = 16;
/// Seconds between choosing which items sparkle.
pub const CHOOSE_EVERY: f64 = 0.25;
/// The dot texture's size (`createDot`'s canvas).
const DOT: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Sparkle {
    /// Where it is in the item's own space.
    local: DVec3,
    age: f64,
    life: f64,
    rise: f64,
    size: f64,
}

/// One item's sparkles (`ItemSparkles`).
#[derive(Debug, Clone)]
pub struct ItemSparkles {
    sparkles: Vec<Sparkle>,
    shell: f64,
    /// `Math.random`'s stand-in: a xorshift seeded by the item.
    seed: u64,
    /// Among the nearest, so it sparkles (`active`).
    pub active: bool,
}

impl ItemSparkles {
    /// `count` sparkles (at most [`MAX_SPARKLES_PER_ITEM`]) for an item whose
    /// bounding sphere, in its own units, has this radius.
    #[must_use]
    pub fn new(seed: u64, count: usize, radius: f64) -> Self {
        let mut out = Self {
            sparkles: Vec::with_capacity(count.min(MAX_SPARKLES_PER_ITEM)),
            shell: radius * SHELL_SHARE,
            seed: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
            active: false,
        };
        for _ in 0..count.min(MAX_SPARKLES_PER_ITEM) {
            let mut sparkle = Sparkle {
                local: DVec3::ZERO,
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

    #[must_use]
    pub fn len(&self) -> usize {
        self.sparkles.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sparkles.is_empty()
    }

    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A new sparkle at a random point just inside the item.
    fn respawn(&mut self, sparkle: &mut Sparkle) {
        let u = self.random() * 2.0 - 1.0;
        let angle = self.random() * std::f64::consts::PI * 2.0;
        let ring = (1.0 - u * u).sqrt() * self.shell;
        sparkle.local = DVec3::new(angle.cos() * ring, u * self.shell, angle.sin() * ring);
        sparkle.age = 0.0;
        sparkle.life = LIFE_MIN + self.random() * (LIFE_MAX - LIFE_MIN);
        sparkle.rise = RISE_MIN + self.random() * (RISE_MAX - RISE_MIN);
        sparkle.size = SIZE_MIN + self.random() * (SIZE_MAX - SIZE_MIN);
    }

    /// Ages and lifts the sparkles by `dt` seconds under an item drawn at
    /// `scale`: rise speeds are in world units, the sparkles in the item's.
    pub fn advance(&mut self, dt: f64, scale: f64) {
        let scale = if scale == 0.0 { 1.0 } else { scale };
        let mut sparkles = std::mem::take(&mut self.sparkles);
        for sparkle in &mut sparkles {
            sparkle.age += dt;
            if sparkle.age >= sparkle.life {
                self.respawn(sparkle);
            }
            sparkle.local.y += (sparkle.rise * dt) / scale;
        }
        self.sparkles = sparkles;
    }

    /// The sparkles' draws under an item whose world matrix is `item`,
    /// facing the camera (`camera` turns camera axes to world): grown in
    /// quickly, then shrunk away over the rest of the climb.
    pub fn draws(&self, quad: ModelHandle, item: Mat4, camera: DMat3, draws: &mut Vec<ModelDraw>) {
        let facing = camera.as_mat3();
        for s in &self.sparkles {
            let t = s.age / s.life;
            let size = s.size * if t < 0.15 { t / 0.15 } else { (1.0 - t) / 0.85 };
            if size <= 0.0 {
                continue;
            }
            let at = item.transform_point3(s.local.as_vec3());
            let world =
                Mat4::from_translation(at) * Mat4::from_mat3(facing) * Mat4::from_scale(Vec3::splat(size as f32));
            draws.push(ModelDraw::new(quad, 0, world));
        }
    }
}

/// `SparkleField.choose`: of the drawn items' distances to the camera (by
/// their place in the list, None when not drawn), the nearest
/// [`MAX_SPARKLING_ITEMS`] within [`SPARKLE_RANGE`].
#[must_use]
pub fn choose_sparkling(distances: &[Option<f64>]) -> Vec<usize> {
    let mut near: Vec<(usize, f64)> = distances
        .iter()
        .enumerate()
        .filter_map(|(i, d)| d.filter(|d| *d <= SPARKLE_RANGE).map(|d| (i, d)))
        .collect();
    // `Array.sort` is stable, as is this.
    near.sort_by(|a, b| a.1.total_cmp(&b.1));
    near.into_iter().take(MAX_SPARKLING_ITEMS).map(|(i, _)| i).collect()
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
    fn sparkles_rise_in_world_units_inside_the_shell() {
        let mut s = ItemSparkles::new(7, 20, 1.0);
        assert_eq!(s.len(), MAX_SPARKLES_PER_ITEM);
        for p in &s.sparkles {
            assert!(p.local.length() <= SHELL_SHARE + 1e-9);
            assert!((LIFE_MIN..LIFE_MAX).contains(&p.life));
            assert!(p.age < p.life);
        }
        let before: Vec<f64> = s.sparkles.iter().map(|p| p.local.y).collect();
        s.advance(0.05, 18.0);
        for (p, y) in s.sparkles.iter().zip(before) {
            if p.age >= 0.05 {
                assert!((p.local.y - y - p.rise * 0.05 / 18.0).abs() < 1e-9);
            }
        }
        assert_eq!(ItemSparkles::new(1, 5, 1.0).len(), 5);
    }

    #[test]
    fn only_the_nearest_eight_in_range_sparkle() {
        let mut distances: Vec<Option<f64>> = (0..12).map(|i| Some(f64::from(i) * 50.0)).collect();
        distances[0] = None;
        distances[3] = Some(950.0);
        assert_eq!(choose_sparkling(&distances), vec![1, 2, 4, 5, 6, 7, 8, 9]);
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
