//! wash-fps `world/pickups/procedural/ProceduralPickupMaterials.ts`: the
//! `StandardMaterial`s procedural pickups are drawn with.
//!
//! Structural materials are dark, lit and shared; energy materials are
//! unlit and emissive, and pulse per pickup. Here a pickup's pulse is its
//! draw's emissive override rather than a private material clone
//! (`cloneEnergyMaterial`), and the ring's scroll is its draw's texture
//! matrix.

use shagger::babylon::materials::standard_material::{FresnelParameters, StandardMaterialParams, TextureUse};
use shagger::rendering::glam::Vec3;
use shagger::shared::js::js_round;

use super::types::Rgb;

fn to_color3(c: Rgb) -> Vec3 {
    Vec3::new(c.r as f32, c.g as f32, c.b as f32)
}

/// The scene's `ambientColor` (wash-fps sets it to white).
pub const SCENE_AMBIENT: Vec3 = Vec3::ONE;

/// `ENERGY_BOOST`.
pub const ENERGY_BOOST: f64 = 1.0;

/// Unlit and emissive only: `disableLighting`, black diffuse and specular.
fn unlit(emissive: Vec3) -> StandardMaterialParams {
    StandardMaterialParams {
        disable_lighting: true,
        diffuse_color: Vec3::ZERO,
        specular_color: Vec3::ZERO,
        emissive_color: emissive,
        ..StandardMaterialParams::default()
    }
}

/// `createStructuralMaterial`: dark, lit, with a faint emissive of the
/// item colour. Its white ambient against the white scene ambient saturates
/// the diffuse term, as it does in Babylon.
#[must_use]
pub fn create_structural_material(color: Rgb) -> StandardMaterialParams {
    StandardMaterialParams {
        diffuse_color: Vec3::new(0.05, 0.055, 0.07),
        specular_color: Vec3::new(0.15, 0.15, 0.18),
        specular_power: 32.0,
        emissive_color: to_color3(color) * 0.12,
        ambient_color: Vec3::ONE * SCENE_AMBIENT,
        ..StandardMaterialParams::default()
    }
}

/// `createArmorPlateMaterial`: a solid, saturated, lit plate with a low
/// emissive floor.
#[must_use]
pub fn create_armor_plate_material(color: Rgb) -> StandardMaterialParams {
    let base = to_color3(color);
    StandardMaterialParams {
        diffuse_color: base * 0.72,
        emissive_color: base * 0.08,
        specular_color: Vec3::new(0.22, 0.22, 0.25),
        specular_power: 24.0,
        ..StandardMaterialParams::default()
    }
}

/// `createEmblemFillMaterial`: the tier colour on the cross's up and down
/// faces.
#[must_use]
pub fn create_emblem_fill_material(color: Rgb) -> StandardMaterialParams {
    unlit(to_color3(color) * ENERGY_BOOST as f32)
}

/// `createEnergyMaterialTemplate`: self-lit emissive; below alpha 1 it
/// blends, two-sided, back faces first.
#[must_use]
pub fn create_energy_material_template(color: Rgb, alpha: f32, boost: f64) -> StandardMaterialParams {
    let mut mat = unlit(to_color3(color) * boost as f32);
    mat.alpha = alpha;
    if alpha < 1.0 {
        mat.alpha_blend = true;
        mat.back_face_culling = false;
        mat.separate_culling_pass = true;
    }
    mat
}

/// `createAmmoIconMaterial` once its icon has loaded: unlit, two-sided,
/// blended by the icon's alpha. Without the icon it stays invisible, so it
/// is not drawn at all.
#[must_use]
pub fn create_ammo_icon_material() -> StandardMaterialParams {
    StandardMaterialParams {
        back_face_culling: false,
        alpha_blend: true,
        texture: TextureUse {
            diffuse: true,
            alpha_from_diffuse: true,
            emissive: true,
            opacity_rgb: false,
        },
        ..unlit(Vec3::ONE)
    }
}

/// `RING_GRADIENT_BANDS`.
const RING_GRADIENT_BANDS: f64 = 3.0;
/// `RING_PEAK_ALPHA`.
const RING_PEAK_ALPHA: f32 = 0.72;
/// The gradient's width in texels.
pub const RING_GRADIENT_WIDTH: u32 = 128;

/// `createRingGradientMaterial`: white emissive plus a banded gradient that
/// is both the emissive and (by its RGB) the opacity texture.
#[must_use]
pub fn create_ring_gradient_material() -> StandardMaterialParams {
    StandardMaterialParams {
        alpha: RING_PEAK_ALPHA,
        alpha_blend: true,
        back_face_culling: false,
        separate_culling_pass: true,
        texture: TextureUse {
            emissive: true,
            opacity_rgb: true,
            ..TextureUse::default()
        },
        ..unlit(Vec3::ONE)
    }
}

/// The ring's gradient, `RING_GRADIENT_WIDTH` by 1 RGBA texels: comets with
/// an accent head fading to the base colour, then to black.
#[must_use]
pub fn ring_gradient_pixels(color: Rgb, accent: Rgb) -> Vec<u8> {
    let width = f64::from(RING_GRADIENT_WIDTH);
    let mut data = Vec::with_capacity(RING_GRADIENT_WIDTH as usize * 4);
    for i in 0..RING_GRADIENT_WIDTH {
        let phase = ((f64::from(i) / width) * RING_GRADIENT_BANDS) % 1.0;
        let tail = (1.0 - phase / 0.62).max(0.0);
        let k = tail * tail;
        let head = (1.0 - phase / 0.16).max(0.0);
        let r = color.r + (accent.r - color.r) * head;
        let g = color.g + (accent.g - color.g) * head;
        let b = color.b + (accent.b - color.b) * head;
        data.extend_from_slice(&[clamp255(r * k), clamp255(g * k), clamp255(b * k), 255]);
    }
    data
}

fn clamp255(v: f64) -> u8 {
    js_round(v.clamp(0.0, 1.0) * 255.0) as u8
}

/// `createFresnelShellMaterial`: nearly clear face-on, hot at the rim; its
/// emissive flows with the pickup's pulse.
#[must_use]
pub fn create_fresnel_shell_material(color: Rgb) -> StandardMaterialParams {
    StandardMaterialParams {
        emissive_fresnel: Some(FresnelParameters {
            left_color: Vec3::ONE,
            right_color: Vec3::splat(0.55),
            bias: 0.18,
            power: 2.6,
        }),
        opacity_fresnel: Some(FresnelParameters {
            left_color: Vec3::splat(0.9),
            right_color: Vec3::splat(0.12),
            bias: 0.0,
            power: 2.0,
        }),
        alpha: 0.0,
        alpha_blend: true,
        back_face_culling: true,
        disable_depth_write: true,
        ..unlit(to_color3(color) * ENERGY_BOOST as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::pickups::procedural::types::rgb;

    #[test]
    fn the_ring_gradient_runs_three_comets() {
        let px = ring_gradient_pixels(rgb(1.0, 0.5, 0.0), rgb(0.0, 0.5, 1.0));
        assert_eq!(px.len(), 128 * 4);
        // Each comet starts on its accent head: texel 0 is the accent.
        assert_eq!(&px[0..4], &[0, 128, 255, 255]);
        // ...and the tail has faded to black before the next band.
        let before_next = 42 * 4;
        assert_eq!(&px[before_next..before_next + 3], &[0, 0, 0]);
    }
}
