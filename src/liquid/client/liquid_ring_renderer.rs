//! Port of the ring discs in wash-fps `liquid/client/LiquidInteractionSystem.ts`
//! (`createRing`, `ringMask`): each ring still alive is an unlit disc on the
//! surface, its colour and opacity the soft ring mask, drawn as one batch
//! of flat quads through shagger's effect quad batch. The TypeScript bakes
//! the mask into a 64-pixel texture and draws 32-sided discs; here the
//! shader evaluates the same mask, which is zero past the disc's edge.

use shagger::rendering::effect_quad_batch::{EffectFrame, EffectInstance, EffectQuadBatch};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;

use super::liquid_interaction_system::{RING_POOL_SIZE, Ring, ring_color};

const RING_WGSL: &str = include_str!("liquid_ring_renderer.wgsl");

/// The rings' batch.
pub struct LiquidRingRenderer {
    batch: EffectQuadBatch,
    instances: Vec<EffectInstance>,
}

/// One ring's instance: position and radius, colour and alpha.
#[must_use]
pub fn ring_instance(ring: &Ring) -> EffectInstance {
    let f = |x: f64| x as f32;
    let [r, g, b] = ring_color(ring.kind);
    let mut instance = [0.0f32; 20];
    instance[..4].copy_from_slice(&[
        f(ring.position.x),
        f(ring.position.y),
        f(ring.position.z),
        f(ring.scale),
    ]);
    instance[4..8].copy_from_slice(&[r, g, b, f(ring.alpha.clamp(0.0, 1.0))]);
    instance
}

impl LiquidRingRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Self {
        Self {
            batch: EffectQuadBatch::new(gpu, color_format, "liquid-ring", RING_WGSL, RING_POOL_SIZE),
            instances: Vec::with_capacity(RING_POOL_SIZE),
        }
    }

    /// This frame's live rings.
    pub fn prepare<'a>(&mut self, gpu: &Gpu, frame: &EffectFrame, rings: impl Iterator<Item = &'a Ring>) {
        self.instances.clear();
        self.instances.extend(rings.take(RING_POOL_SIZE).map(ring_instance));
        self.batch.prepare(gpu, frame, &self.instances, true);
    }

    /// How many rings this frame draws.
    #[must_use]
    pub fn drawn_count(&self) -> usize {
        self.instances.len()
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.batch.draw(pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liquid::liquid_types::LiquidKind;
    use shagger::rendering::glam::DVec3;

    #[test]
    fn a_ring_carries_its_colour_and_size() {
        let ring = Ring {
            position: DVec3::new(1.0, 2.0, 3.0),
            kind: LiquidKind::Lava,
            alpha: 0.4,
            scale: 20.0,
            age: 0.1,
            max_radius: 48.0,
        };
        let i = ring_instance(&ring);
        assert_eq!(&i[..8], &[1.0, 2.0, 3.0, 20.0, 1.0, 0.75, 0.35, 0.4]);
    }
}
