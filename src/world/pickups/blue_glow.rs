//! wash-fps `world/pickups/BlueGlow.ts`: the megahealth's blue core glows.
//!
//! Babylon gives each material an emissive texture holding only the
//! strongly blue texels of its base texture, three times brighter than the
//! lit surface, breathing slowly; a material that already has an emissive
//! texture only breathes. PBR adds emissive light over the lit colour, so
//! here the model draws a second time as added light, with the masks (or
//! the emissive textures) as its images and the breath as its tint.

use std::io::Cursor;

use shagger::loader::model::gltf_model::{GltfModel, ModelImage, ModelMaterial};
use shagger::loader::texture::decode_image;
use shagger::rendering::glam::Vec3;

/// How much brighter than the lit surface the blue parts glow; above 1,
/// bloom picks them up.
pub const BLUE_GLOW_INTENSITY: f64 = 3.0;
/// Blue leading red and green by this much (0-255) starts to glow, and by
/// the upper value glows fully.
const BLUE_LEAD_START: f64 = 30.0;
const BLUE_LEAD_FULL: f64 = 90.0;

/// The glow breathes this many times a second, between these shares of its
/// full strength.
const PULSE_RATE: f64 = 0.5;
const PULSE_LOW: f64 = 0.7;
const PULSE_HIGH: f64 = 1.15;

/// How strongly a texel glows, 0-1, from how far its blue leads its red and
/// green. Grey, white and black panels lead by nothing and stay dark.
#[must_use]
pub fn blue_glow_weight(r: f64, g: f64, b: f64) -> f64 {
    let lead = b - r.max(g);
    let t = ((lead - BLUE_LEAD_START) / (BLUE_LEAD_FULL - BLUE_LEAD_START)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Keep each texel's colour where it glows and black elsewhere, as RGBA
/// bytes (a `Uint8Array` store truncates).
#[must_use]
pub fn blue_glow_mask(pixels: &[u8]) -> Vec<u8> {
    let mut out = vec![0; pixels.len()];
    for (texel, mask) in pixels.as_chunks::<4>().0.iter().zip(out.as_chunks_mut::<4>().0) {
        let [r, g, b] = [texel[0], texel[1], texel[2]].map(f64::from);
        let w = blue_glow_weight(r, g, b);
        mask[0] = (r * w) as u8;
        mask[1] = (g * w) as u8;
        mask[2] = (b * w) as u8;
        mask[3] = 255;
    }
    out
}

/// RGBA bytes read from a texture as a new PNG image (`textureFromPixels`):
/// the bytes are in the order the source holds them, so the image reads
/// with its coordinates.
#[must_use]
pub fn png_image(name: &str, bytes: &[u8], width: u32, height: u32) -> Option<ModelImage> {
    let mut png = Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut png,
        bytes,
        width,
        height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(ModelImage {
        name: name.to_owned(),
        mime_type: Some("image/png".to_owned()),
        bytes: png.into_inner(),
    })
}

/// The glow's emissive intensity at `seconds` (`scenePulse`), every core
/// breathing together.
#[must_use]
pub fn blue_glow_intensity(seconds: f64) -> f64 {
    let strength =
        PULSE_LOW + (PULSE_HIGH - PULSE_LOW) * (0.5 + 0.5 * (seconds * std::f64::consts::PI * 2.0 * PULSE_RATE).sin());
    BLUE_GLOW_INTENSITY * strength
}

/// The added-light copy of a model (`glowBlueParts`), and each primitive's
/// emissive colour (its tint at intensity 1): None when nothing glows.
#[must_use]
pub fn blue_glow_layer(model: &GltfModel) -> Option<(GltfModel, Vec<Option<Vec3>>)> {
    let mut images = Vec::new();
    // Each source material's glow: its image in the copy and its colour.
    let mut glows: Vec<Option<(usize, Vec3)>> = Vec::new();
    let mut masks: Vec<(usize, usize)> = Vec::new();
    for material in &model.materials {
        let glow = if let Some(emissive) = material.emissive_image {
            // Already emissive: it only breathes.
            let image = model.images.get(emissive)?.clone();
            images.push(image);
            Some((images.len() - 1, Vec3::from(material.emissive)))
        } else if let Some(base) = material.base_color_image {
            // One mask per base texture: every copy shares it.
            if let Some(&(_, i)) = masks.iter().find(|(b, _)| *b == base) {
                Some((i, Vec3::ONE))
            } else {
                let image = model.images.get(base)?;
                let decoded = decode_image(&image.name, &image.bytes)?;
                let mask = blue_glow_mask(&decoded.pixels);
                images.push(png_image(
                    &format!("{}_blue_glow", image.name),
                    &mask,
                    decoded.width,
                    decoded.height,
                )?);
                masks.push((base, images.len() - 1));
                Some((images.len() - 1, Vec3::ONE))
            }
        } else {
            None
        };
        glows.push(glow);
    }
    if glows.iter().all(Option::is_none) {
        return None;
    }
    let materials = glows
        .iter()
        .zip(&model.materials)
        .map(|(glow, m)| ModelMaterial {
            name: format!("{}_glow", m.name),
            base_color: [1.0; 4],
            base_color_image: glow.map(|(i, _)| i),
            metallic_roughness_image: None,
            normal_image: None,
            // The layer is the glow: its own emissive would add a flat
            // white over the whole model.
            emissive: [0.0; 3],
            emissive_image: None,
            ..m.clone()
        })
        .collect();
    let tints = model
        .primitives
        .iter()
        .map(|p| p.material.and_then(|m| glows.get(m).copied().flatten()).map(|(_, c)| c))
        .collect();
    Some((
        GltfModel {
            primitives: model.primitives.clone(),
            materials,
            images,
            attachment_points: Default::default(),
            skeleton: model.skeleton.clone(),
        },
        tints,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_strong_blue_glows() {
        assert_eq!(blue_glow_weight(200.0, 200.0, 200.0), 0.0);
        assert_eq!(blue_glow_weight(0.0, 0.0, 255.0), 1.0);
        assert_eq!(blue_glow_weight(0.0, 0.0, 60.0), 0.5);
        assert_eq!(
            blue_glow_mask(&[10, 20, 255, 7, 90, 90, 90, 255, 0, 0, 60, 0]),
            vec![10, 20, 255, 255, 0, 0, 0, 255, 0, 0, 30, 255]
        );
    }

    #[test]
    fn the_glow_breathes_between_its_shares() {
        assert!((blue_glow_intensity(0.0) - 3.0 * 0.925).abs() < 1e-12);
        assert!((blue_glow_intensity(0.5) - 3.0 * 1.15).abs() < 1e-12);
        assert!((blue_glow_intensity(1.5) - 3.0 * 0.7).abs() < 1e-12);
    }
}
