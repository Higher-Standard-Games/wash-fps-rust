//! Port of wash-fps `liquid/client/UnderwaterView.ts`: underwater
//! presentation, driven by the camera rather than by the player, so the
//! spectator, the death chase and the free camera cross the surface on
//! their own.
//!
//! The TypeScript writes Babylon's scene fog and suspends world fog; here
//! [`UnderwaterView::update`] returns the scene fog to draw instead, and
//! the map renderer hands it to every material.

use shagger::rendering::camera::Camera;
use shagger::rendering::glam::DVec3;
use shagger::shared::collision::collision_world::MASK_LIQUID;

use super::underwater_look::{approach_submersion, fog_density_for, fov_warp_degrees, look_for};
use crate::liquid::liquid_types::LiquidKind;
use crate::liquid::liquid_volume_sampler::liquid_kind_at;

/// The scene fog while under a surface: Babylon's `FOGMODE_EXP2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnderwaterFog {
    pub color: [f64; 3],
    pub density: f64,
}

/// `UnderwaterView`.
#[derive(Debug, Clone)]
pub struct UnderwaterView {
    submersion: f64,
    kind: LiquidKind,
    elapsed: f64,
    /// The scene fog's colour as found, while we hold it.
    saved: Option<[f64; 3]>,
    /// The camera's field of view as authored, and the one we last wrote.
    base_fov: Option<f32>,
    written_fov: Option<f32>,
}

impl Default for UnderwaterView {
    fn default() -> Self {
        Self {
            submersion: 0.0,
            kind: LiquidKind::Water,
            elapsed: 0.0,
            saved: None,
            base_fov: None,
            written_fov: None,
        }
    }
}

impl UnderwaterView {
    /// `update`: sample the camera's eye, ease the mix, warp the view.
    /// `scene_color` is the scene fog's colour now (world fog's, or the
    /// default), `comfort` is `r_underwater_distortion`. Returns the fog to
    /// draw, none when dry.
    pub fn update(
        &mut self,
        camera: &mut Camera,
        dt: f64,
        contents: Option<&dyn Fn(DVec3) -> u32>,
        scene_color: [f64; 3],
        comfort: f64,
    ) -> Option<UnderwaterFog> {
        self.elapsed += dt;
        let eye = camera.position.as_dvec3();
        let liquid = liquid_kind_at(contents.map_or(0, |c| c(eye)) & MASK_LIQUID);
        // Hold the last kind while fading out.
        if let Some(kind) = liquid {
            self.kind = kind;
        }
        self.submersion = approach_submersion(self.submersion, if liquid.is_some() { 1.0 } else { 0.0 }, dt);
        if self.submersion <= 0.0 {
            self.saved = None;
            self.restore_fov(camera);
            return None;
        }
        let look = look_for(self.kind);
        // Blend toward the liquid's colour from the scene's own, so a map's
        // fog keeps some of its character under water.
        let base = *self.saved.get_or_insert(scene_color);
        let t = look.tint_strength * self.submersion;
        let color = [0, 1, 2].map(|i| base[i] + (look.color[i] - base[i]) * t);
        self.apply_warp(camera, comfort);
        Some(UnderwaterFog {
            color,
            density: fog_density_for(&look, self.submersion),
        })
    }

    fn apply_warp(&mut self, camera: &mut Camera, comfort: f64) {
        // A field of view that is not the one we wrote was set since (zoom,
        // the slider), and is the new base.
        let base = match self.base_fov {
            Some(base) if Some(camera.fov_y) == self.written_fov => base,
            _ => camera.fov_y,
        };
        self.base_fov = Some(base);
        let warp = fov_warp_degrees(self.elapsed, self.submersion, comfort);
        camera.fov_y = base + (warp * std::f64::consts::PI / 180.0) as f32;
        self.written_fov = Some(camera.fov_y);
    }

    fn restore_fov(&mut self, camera: &mut Camera) {
        if let Some(base) = self.base_fov.take() {
            // Only undo our own write.
            if Some(camera.fov_y) == self.written_fov {
                camera.fov_y = base;
            }
            self.written_fov = None;
        }
    }

    /// `submersionAmount`: 0 dry, 1 fully under.
    #[must_use]
    pub fn submersion_amount(&self) -> f64 {
        self.submersion
    }

    /// `liquidKind`.
    #[must_use]
    pub fn liquid_kind(&self) -> LiquidKind {
        self.kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::shared::collision::collision_world::contents;

    #[test]
    fn fogs_under_the_surface_and_gives_the_view_back() {
        let mut view = UnderwaterView::default();
        let mut camera = Camera::default();
        let fov = camera.fov_y;
        let wet = |p: DVec3| if p.y < 0.0 { contents::WATER } else { 0 };
        camera.position.y = -10.0;
        let fog = view.update(&mut camera, 0.3, Some(&wet), [0.2, 0.2, 0.3], 1.0).unwrap();
        assert_eq!(view.submersion_amount(), 1.0);
        assert_eq!(fog.density, look_for(LiquidKind::Water).density);
        assert!(fog.color[2] > fog.color[0]);
        assert_ne!(camera.fov_y, fov);
        // A zoom while under water is the new base.
        camera.fov_y = 0.5;
        view.update(&mut camera, 0.3, Some(&wet), [0.2, 0.2, 0.3], 0.0);
        assert_eq!(camera.fov_y, 0.5);
        // Out: the fog fades over a few frames, then the view comes back.
        camera.position.y = 10.0;
        let mut frames = 0;
        while view
            .update(&mut camera, 1.0 / 60.0, Some(&wet), [0.2, 0.2, 0.3], 1.0)
            .is_some()
        {
            frames += 1;
        }
        assert!(frames > 3 && frames < 60);
        assert_eq!(camera.fov_y, 0.5);
        // Lava keeps its kind while fading.
        let lava = |p: DVec3| if p.y < 0.0 { contents::LAVA } else { 0 };
        camera.position.y = -10.0;
        view.update(&mut camera, 0.05, Some(&lava), [0.2, 0.2, 0.3], 1.0);
        camera.position.y = 10.0;
        view.update(&mut camera, 0.01, Some(&lava), [0.2, 0.2, 0.3], 1.0);
        assert_eq!(view.liquid_kind(), LiquidKind::Lava);
    }
}
