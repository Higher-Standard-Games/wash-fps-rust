//! Port of wash-fps `client/CartoonHitEffect.ts`: Warsow-style cartoon hit
//! sprites (POW! and the like) over a victim the player on screen hit hard.
//!
//! The TypeScript gives each sprite its own plane and material over one of
//! two textures; here both images share an atlas and one batch of
//! camera-facing quads. Rendering group 1 (drawn over the scene after the
//! depth is cleared) becomes quads drawn last with no depth test. The
//! artwork has no mipmaps here.

use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::texture::decode_image;
use shagger::rendering::effect_quad_batch::{EffectFrame, EffectInstance, EffectQuadBatch};
use shagger::rendering::glam::DVec3;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;
use shagger::rendering::world_renderer::Rgba8Image;

/// `MIN_DAMAGE`.
pub const MIN_DAMAGE: u16 = 75;
/// `EFFECT_DURATION`, seconds.
pub const EFFECT_DURATION: f64 = 0.8;
pub const START_SCALE: f64 = 0.8;
/// `END_SCALE`: the plane's size in world units at scale 1.
pub const END_SCALE: f64 = 144.0;
pub const FLOAT_UP: f64 = 40.0;
/// `ROTATION_RANGE`: up to 45 degrees either way.
pub const ROTATION_RANGE: f64 = std::f64::consts::PI * 0.25;
pub const MAX_EFFECTS: usize = 6;
pub const Y_OFFSET: f64 = 30.0;
/// `HIT_SPRITES`.
pub const HIT_SPRITES: [&str; 2] = ["effects/cartoonhit1.png", "effects/cartoonhit2.png"];

/// One live sprite (`CartoonHitInstance`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CartoonHit {
    pub sprite: usize,
    pub base: DVec3,
    pub rotation: f64,
    pub elapsed: f64,
}

impl CartoonHit {
    fn t(&self) -> f64 {
        (self.elapsed / EFFECT_DURATION).min(1.0)
    }

    /// Grows from 80 % to 110 % (an overshoot for the pop).
    #[must_use]
    pub fn scale(&self) -> f64 {
        (START_SCALE + (1.1 - START_SCALE) * self.t()) * END_SCALE
    }

    /// Floats up from where it started.
    #[must_use]
    pub fn position(&self) -> DVec3 {
        self.base + DVec3::new(0.0, FLOAT_UP * self.t(), 0.0)
    }

    /// Fades from 0.9 to nothing.
    #[must_use]
    pub fn visibility(&self) -> f64 {
        0.9 * (1.0 - self.t())
    }
}

/// `CartoonHitEffect`: the live sprites, oldest first.
#[derive(Debug, Clone)]
pub struct CartoonHitEffect {
    effects: Vec<CartoonHit>,
    /// How many sprites were loaded.
    sprites: usize,
    /// `Math.random`'s stand-in (xorshift64*).
    seed: u64,
}

impl CartoonHitEffect {
    /// Holds sprites of `sprites` images (none loaded spawns nothing).
    #[must_use]
    pub fn new(sprites: usize) -> Self {
        Self {
            effects: Vec::new(),
            sprites,
            seed: 0x2545_f491_4f6c_dd1d,
        }
    }

    fn random(&mut self) -> f64 {
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `spawn`: only hits of `MIN_DAMAGE` or more; past `MAX_EFFECTS` the
    /// oldest goes.
    pub fn spawn(&mut self, at: DVec3, damage: u16) {
        if damage < MIN_DAMAGE || self.sprites == 0 {
            return;
        }
        let sprite = ((self.random() * self.sprites as f64) as usize).min(self.sprites - 1);
        let rotation = (self.random() - 0.5) * ROTATION_RANGE;
        if self.effects.len() >= MAX_EFFECTS {
            self.effects.remove(0);
        }
        self.effects.push(CartoonHit {
            sprite,
            base: DVec3::new(at.x, at.y + Y_OFFSET, at.z),
            rotation,
            elapsed: 0.0,
        });
    }

    /// `update`: animate, and drop the finished.
    pub fn update(&mut self, dt: f64) {
        for effect in &mut self.effects {
            effect.elapsed += dt;
        }
        self.effects.retain(|e| e.t() < 1.0);
    }

    #[must_use]
    pub fn effects(&self) -> &[CartoonHit] {
        &self.effects
    }
}

/// Both sprites side by side, and each one's uv rectangle.
#[must_use]
pub fn sprite_atlas(images: &[Rgba8Image]) -> (Rgba8Image, Vec<[f32; 4]>) {
    let width: u32 = images.iter().map(|i| i.width).sum::<u32>().max(1);
    let height: u32 = images.iter().map(|i| i.height).max().unwrap_or(1).max(1);
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let mut rects = Vec::with_capacity(images.len());
    let mut x0 = 0u32;
    for image in images {
        for y in 0..image.height {
            let src = (y * image.width * 4) as usize;
            let dst = ((y * width + x0) * 4) as usize;
            pixels[dst..dst + (image.width * 4) as usize]
                .copy_from_slice(&image.pixels[src..src + (image.width * 4) as usize]);
        }
        // Half a texel in, so the bilinear filter stays inside the sprite.
        rects.push([
            (x0 as f32 + 0.5) / width as f32,
            0.5 / height as f32,
            ((x0 + image.width) as f32 - 0.5) / width as f32,
            (image.height as f32 - 0.5) / height as f32,
        ]);
        x0 += image.width;
    }
    (Rgba8Image { width, height, pixels }, rects)
}

/// The sprites' shader after the shared quad half.
const CARTOON_HIT_WGSL: &str = include_str!("cartoon_hit_effect.wgsl");

/// The sprites' quads.
pub struct CartoonHitRenderer {
    batch: EffectQuadBatch,
    rects: Vec<[f32; 4]>,
    instances: Vec<EffectInstance>,
}

impl CartoonHitRenderer {
    /// `loadTextures`: none when a sprite is missing from the packages.
    pub fn load(gpu: &Gpu, color_format: wgpu::TextureFormat, pk3: &mut Pk3Loader) -> Option<Self> {
        let mut images = Vec::with_capacity(HIT_SPRITES.len());
        for path in HIT_SPRITES {
            let Some(decoded) = pk3.get_file(path).and_then(|bytes| decode_image(path, &bytes)) else {
                println!("cartoon hits: missing {path}");
                return None;
            };
            images.push(Rgba8Image {
                width: decoded.width,
                height: decoded.height,
                pixels: decoded.pixels,
            });
        }
        let (atlas, rects) = sprite_atlas(&images);
        Some(Self {
            batch: EffectQuadBatch::textured(
                gpu,
                color_format,
                "cartoon-hits",
                CARTOON_HIT_WGSL,
                MAX_EFFECTS,
                &atlas,
                false,
            ),
            rects,
            instances: Vec::with_capacity(MAX_EFFECTS),
        })
    }

    /// How many sprites the atlas holds.
    #[must_use]
    pub fn sprite_count(&self) -> usize {
        self.rects.len()
    }

    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame, effects: &CartoonHitEffect) {
        self.instances.clear();
        for effect in effects.effects() {
            let Some(rect) = self.rects.get(effect.sprite) else {
                continue;
            };
            let p = effect.position();
            let mut instance = [0.0f32; 20];
            instance[..4].copy_from_slice(&[p.x as f32, p.y as f32, p.z as f32, effect.scale() as f32]);
            // Babylon's billboard replaces the node's rotation, so the random
            // roll never shows (TeleportEffect spins a child for that reason).
            instance[4..8].copy_from_slice(&[0.0, effect.visibility() as f32, 0.0, 0.0]);
            instance[8..12].copy_from_slice(rect);
            self.instances.push(instance);
        }
        self.batch.prepare(gpu, frame, &self.instances, true);
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.batch.draw(pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_hits_draw_nothing() {
        let mut hits = CartoonHitEffect::new(2);
        hits.spawn(DVec3::ZERO, 74);
        assert!(hits.effects().is_empty());
        hits.spawn(DVec3::ZERO, 75);
        assert_eq!(hits.effects().len(), 1);
        assert!(CartoonHitEffect::new(0).effects().is_empty());
    }

    #[test]
    fn a_hit_pops_floats_and_fades() {
        let mut hits = CartoonHitEffect::new(2);
        hits.spawn(DVec3::new(10.0, 20.0, 30.0), 100);
        let hit = hits.effects()[0];
        assert!(hit.sprite < 2);
        assert!(hit.rotation.abs() <= ROTATION_RANGE / 2.0);
        assert_eq!(hit.position(), DVec3::new(10.0, 50.0, 30.0));
        assert!((hit.scale() - 0.8 * 144.0).abs() < 1e-9);
        assert!((hit.visibility() - 0.9).abs() < 1e-12);
        hits.update(0.4);
        let hit = hits.effects()[0];
        assert!((hit.scale() - 0.95 * 144.0).abs() < 1e-9);
        assert!((hit.position().y - 70.0).abs() < 1e-9);
        assert!((hit.visibility() - 0.45).abs() < 1e-12);
        hits.update(0.4);
        assert!(hits.effects().is_empty());
    }

    #[test]
    fn the_oldest_makes_way() {
        let mut hits = CartoonHitEffect::new(2);
        for i in 0..8 {
            hits.spawn(DVec3::new(f64::from(i), 0.0, 0.0), 90);
        }
        assert_eq!(hits.effects().len(), MAX_EFFECTS);
        assert_eq!(hits.effects()[0].base.x, 2.0);
    }

    #[test]
    fn sprites_share_an_atlas() {
        let a = Rgba8Image {
            width: 2,
            height: 1,
            pixels: vec![255; 8],
        };
        let b = Rgba8Image {
            width: 1,
            height: 2,
            pixels: vec![7; 8],
        };
        let (atlas, rects) = sprite_atlas(&[a, b]);
        assert_eq!((atlas.width, atlas.height), (3, 2));
        assert_eq!(&atlas.pixels[8..12], &[7; 4]);
        assert_eq!(&atlas.pixels[12..16], &[0; 4]);
        assert_eq!(rects.len(), 2);
    }
}
