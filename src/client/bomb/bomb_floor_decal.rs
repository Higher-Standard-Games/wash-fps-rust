//! wash-fps `client/bomb/BombFloorDecal.ts`: the turning glow on the floor
//! under a bomb that is not on anyone's back.
//!
//! A disc of light the size of the arm/defuse radius, so it shows the ground
//! a player has to stand on to work on the bomb. Strongest at the centre, it
//! fades out to nothing at the radius, with soft spiral arms that turn. It
//! is a translucent wash of colour, never opaque, so the floor's own texture
//! shows through all the way out; on a bright floor, light added to it would
//! only turn white.
//!
//! Babylon gives the glow as an `opacityTexture`; here it is the diffuse
//! texture with its alpha used, under a black diffuse colour, which
//! `disableLighting` draws the same. The material's `zOffset` -2 has no
//! counterpart on a model draw: the lift keeps it off the floor.

use shagger::babylon::materials::standard_material::{StandardMaterialParams, TextureUse};
use shagger::babylon::meshes::vertex_data::{VertexData, babylon_local_matrix};
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelMaterial, Skeleton};
use shagger::rendering::glam::{DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};
use shagger::shared::js::{js_hypot, js_round};

use crate::world::pickups::blue_glow::png_image;
use crate::world::pickups::procedural::procedural_pickup_factory::primitive;

/// Texels across the generated glow.
const TEXTURE_SIZE: usize = 256;
/// Spiral arms in the glow, and how far each one winds from the centre to
/// the edge, in turns.
const ARMS: f64 = 3.0;
const ARM_WIND: f64 = 0.6;
/// How much the arms stand out from the even glow under them, 0-1.
const ARM_CONTRAST: f64 = 0.7;
/// The strongest the wash gets, at the centre, 0-1.
const PEAK_OPACITY: f64 = 0.7;
/// Lift off the floor, against z-fighting.
const LIFT: f64 = 1.5;
const TESSELLATION: usize = 64;

fn smooth(t: f64) -> f64 {
    let c = t.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}

/// `floorGlowAt`: the glow's strength at a point of the disc, 0-1: `r` from
/// 0 at the centre to 1 at the edge, `angle` in radians. A strong core, a
/// body that holds out to half the radius and then fades to nothing at the
/// edge, and spiral arms over it. The arms leave the core alone, where the
/// bomb sits.
#[must_use]
pub fn floor_glow_at(r: f64, angle: f64) -> f64 {
    if r >= 1.0 {
        return 0.0;
    }
    let fade = smooth((1.0 - r) / 0.55);
    let core = (-((r / 0.14).powi(2))).exp();
    let arm = 0.5 + 0.5 * (ARMS * angle - r * ARM_WIND * std::f64::consts::PI * 2.0 * ARMS).cos();
    let arms = 1.0 - ARM_CONTRAST * smooth(r / 0.2) * (1.0 - arm * arm);
    (fade * arms * 0.6 + core * 0.4).min(1.0)
}

/// `floorGlowPixels`: the glow as RGBA bytes, white, with the strength in
/// alpha. The material gives the colour.
#[must_use]
pub fn floor_glow_pixels(size: usize) -> Vec<u8> {
    let mut out = vec![0; size * size * 4];
    let half = size as f64 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f64 + 0.5 - half) / half;
            let dy = (y as f64 + 0.5 - half) / half;
            let i = (y * size + x) * 4;
            out[i] = 255;
            out[i + 1] = 255;
            out[i + 2] = 255;
            out[i + 3] = js_round(floor_glow_at(js_hypot(&[dx, dy]), dy.atan2(dx)) * 255.0) as u8;
        }
    }
    out
}

/// Babylon's `CreateDisc({ radius, tessellation })`: a fan in XY facing -Z,
/// its UVs `((cos + 1) / 2, (1 - sin) / 2)` around a centre at (0.5, 0.5),
/// the first rim vertex repeated to close it.
#[must_use]
pub fn create_disc(radius: f64, tessellation: usize) -> VertexData {
    let mut data = VertexData {
        positions: vec![0.0; 3],
        normals: vec![0.0, 0.0, -1.0],
        uvs: vec![0.5, 0.5],
        indices: Vec::new(),
    };
    let step = std::f64::consts::TAU / tessellation as f64;
    let mut a = 0.0_f64;
    for _ in 0..=tessellation {
        // The closing vertex repeats the first, as Babylon's does.
        let (x, y) = if data.vertex_count() == tessellation + 1 {
            (1.0, 0.0)
        } else {
            (a.cos(), a.sin())
        };
        data.positions
            .extend_from_slice(&[(radius * x) as f32, (radius * y) as f32, 0.0]);
        data.normals.extend_from_slice(&[0.0, 0.0, -1.0]);
        data.uvs
            .extend_from_slice(&[((x + 1.0) / 2.0) as f32, ((1.0 - y) / 2.0) as f32]);
        a += step;
    }
    let count = data.vertex_count() as u32;
    for i in 1..count - 1 {
        data.indices.extend_from_slice(&[i + 1, 0, i]);
    }
    data
}

/// `BombFloorDecal`: the disc, uploaded once, and its turn so far.
pub struct BombFloorDecal {
    model: ModelHandle,
    angle: f64,
    /// Where it shows this frame: its matrix, colour and alpha.
    shown: Option<(DVec3, Vec3, f32)>,
}

impl BombFloorDecal {
    pub fn new(gpu: &Gpu, models: &mut ModelRenderer, radius: f64) -> Self {
        let image = png_image(
            "bomb_floor_glow",
            &floor_glow_pixels(TEXTURE_SIZE),
            TEXTURE_SIZE as u32,
            TEXTURE_SIZE as u32,
        );
        let disc = create_disc(radius, TESSELLATION);
        let model = GltfModel {
            primitives: vec![primitive("bomb_floor", &disc, 0, 0)],
            materials: vec![ModelMaterial {
                name: "bomb_floor_mat".to_owned(),
                base_color: [1.0; 4],
                base_color_image: image.is_some().then_some(0),
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
            images: image.into_iter().collect(),
            attachment_points: Default::default(),
            skeleton: Skeleton::default(),
        };
        let params = StandardMaterialParams {
            diffuse_color: Vec3::ZERO,
            specular_color: Vec3::ZERO,
            emissive_color: Vec3::ONE,
            disable_lighting: true,
            alpha_blend: true,
            disable_depth_write: true,
            back_face_culling: false,
            texture: TextureUse {
                diffuse: true,
                alpha_from_diffuse: true,
                ..TextureUse::default()
            },
            ..StandardMaterialParams::default()
        };
        let model = models.upload(gpu, &model, |_| ModelMaterialMode::Standard(params));
        Self {
            model,
            angle: 0.0,
            shown: None,
        }
    }

    /// `show`: the glow at a floor point in a colour, at `strength` 0-1 of
    /// its peak, turning at `spin` radians a second. `dt` is seconds.
    pub fn show(&mut self, at: DVec3, color: Vec3, strength: f64, spin: f64, dt: f64) {
        self.angle = (self.angle + spin * dt) % (std::f64::consts::PI * 2.0);
        self.shown = Some((at + DVec3::new(0.0, LIFT, 0.0), color, (PEAK_OPACITY * strength) as f32));
    }

    pub fn hide(&mut self) {
        self.shown = None;
    }

    /// The glow's draw this frame, when it shows.
    pub fn draws(&self, draws: &mut Vec<ModelDraw>) {
        let Some((at, color, alpha)) = self.shown else { return };
        // The disc is built facing -Z; laid on the floor, then turned.
        let world = babylon_local_matrix(at, DVec3::new(std::f64::consts::FRAC_PI_2, self.angle, 0.0), DVec3::ONE);
        draws.push(ModelDraw {
            emissive: Some(color),
            alpha: Some(alpha),
            ..ModelDraw::new(self.model, 0, world.as_mat4())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_brightest_at_the_centre_and_fades_to_nothing_at_the_edge() {
        assert!(floor_glow_at(0.0, 0.0) > 0.9);
        assert!(floor_glow_at(0.5, 0.0) < floor_glow_at(0.1, 0.0));
        assert!(floor_glow_at(0.97, 0.0) < 0.02);
        assert_eq!(floor_glow_at(1.0, 0.0), 0.0);
    }

    #[test]
    fn its_arms_vary_with_angle_away_from_the_centre_but_not_at_it() {
        let around = |r: f64| {
            (0..24)
                .map(|i| floor_glow_at(r, f64::from(i) / 24.0 * std::f64::consts::TAU))
                .collect::<Vec<_>>()
        };
        let spread =
            |v: Vec<f64>| v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min);
        assert!(spread(around(0.5)) > 0.1);
        assert!(spread(around(0.02)) < 0.02);
    }

    #[test]
    fn the_textures_corners_are_transparent() {
        let pixels = floor_glow_pixels(16);
        assert_eq!(pixels[3], 0);
        assert_eq!(pixels[pixels.len() - 1], 0);
    }

    #[test]
    fn the_disc_is_a_closed_fan() {
        let disc = create_disc(100.0, 64);
        // Centre, 64 rim vertices and the first repeated.
        assert_eq!(disc.vertex_count(), 66);
        assert_eq!(disc.indices.len(), 64 * 3);
        assert_eq!(&disc.positions[3..6], &disc.positions[disc.positions.len() - 3..]);
        assert_eq!(&disc.uvs[2..4], &[1.0, 0.5]);
    }
}
