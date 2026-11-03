//! Port of wash-fps `liquid/client/SplashDropletRenderer.ts`: the crowns'
//! fine spray as one batch of soft, velocity-aligned camera-facing quads,
//! drawn through shagger's effect quad batch (the TypeScript builds its own
//! pooled mesh; the shader is the same).

use shagger::rendering::effect_quad_batch::{EffectFrame, EffectInstance, EffectQuadBatch};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;

use super::splash_droplets::{DropletSprite, MAX_DROPLETS};

/// The spray's shader after the shared quad half.
const SPRAY_WGSL: &str = include_str!("splash_droplet_renderer.wgsl");

/// `SplashDropletRenderer`.
pub struct SplashDropletRenderer {
    batch: EffectQuadBatch,
    instances: Vec<EffectInstance>,
}

impl SplashDropletRenderer {
    /// `start`.
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        Self {
            // The batch puts the shared quad half (`EFFECT_QUAD_WGSL`) first.
            batch: EffectQuadBatch::new(gpu, color_format, "liquid-splash-spray", SPRAY_WGSL, MAX_DROPLETS),
            instances: Vec::with_capacity(MAX_DROPLETS),
        }
    }

    /// `update`: this frame's droplets.
    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame, sprites: &[DropletSprite]) {
        self.instances.clear();
        for s in sprites.iter().take(MAX_DROPLETS) {
            let f = |x: f64| x as f32;
            let mut instance = [0.0f32; 20];
            instance[..4].copy_from_slice(&[f(s.position[0]), f(s.position[1]), f(s.position[2]), f(s.size)]);
            instance[4..8].copy_from_slice(&[f(s.velocity[0]), f(s.velocity[1]), f(s.velocity[2]), f(s.opacity)]);
            self.instances.push(instance);
        }
        self.batch.prepare(gpu, frame, &self.instances, true);
    }

    /// `drawnCount`.
    #[must_use]
    pub fn drawn_count(&self) -> usize {
        self.instances.len()
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.batch.draw(pass);
    }
}
