//! wash-fps `world/pickups/HealthTint.ts`: the health bubbles' tier colours
//! and the ultra health's purple core.
//!
//! Babylon gives each tier a copy of the bubble's material whose albedo is
//! recoloured to the tier's hue, with an emissive copy that lights its blue
//! trim; the cross, the model's innermost part, is fullbright. The ultra
//! health is the megahealth with its cyan-to-blue texels turned purple and
//! glowing. Here the copies are new images on a copy of the model: the
//! albedo as its base colour, the glow as its emissive image, which the
//! added-light layer ([`super::blue_glow::blue_glow_layer`]) draws.

use std::collections::HashMap;

use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::texture::decode_image;
use shagger::shared::js::{js_hypot, js_round};

use super::blue_glow::{blue_glow_weight, png_image};

/// A tier's colour: the hue (degrees) every texel turns to, and how much of
/// its lightness it keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HealthTint {
    pub hue: Option<f64>,
    pub lightness: f64,
}

/// The health bubble's colour by tier; the +5 keeps the model's own blue.
#[must_use]
pub fn health_tint(tier: &str) -> Option<HealthTint> {
    match tier {
        "health_large" => Some(HealthTint {
            hue: Some(18.0),
            lightness: 0.85,
        }),
        "health_medium" => Some(HealthTint {
            hue: Some(40.0),
            lightness: 1.0,
        }),
        "health_small" => Some(HealthTint {
            hue: None,
            lightness: 1.0,
        }),
        _ => None,
    }
}

/// How brightly the bubble glows: the cross and its strongly blue trim
/// fully, the rest at this share.
pub const GLOW_INTENSITY: f64 = 1.6;
const GLOW_BASE: f64 = 0.25;

/// The hue of an RGB colour in 0-1, as `tintTexel` and `coreHueWeight` take
/// it.
fn hue_of(r: f64, g: f64, b: f64, max: f64, d: f64) -> f64 {
    if max == r {
        60.0 * (((g - b) / d + 6.0) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    }
}

/// Recolour one texel: its hue becomes the tint's, its saturation stays and
/// its lightness is scaled.
#[must_use]
pub fn tint_texel(r: f64, g: f64, b: f64, tint: HealthTint) -> [f64; 3] {
    if tint.hue.is_none() && tint.lightness == 1.0 {
        return [r, g, b];
    }
    let (rr, gg, bb) = (r / 255.0, g / 255.0, b / 255.0);
    let max = rr.max(gg).max(bb);
    let min = rr.min(gg).min(bb);
    let l = ((max + min) / 2.0) * tint.lightness;
    let d = max - min;
    let s = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (max + min - 1.0).abs())
    };
    let h = tint
        .hue
        .unwrap_or_else(|| if d == 0.0 { 0.0 } else { hue_of(rr, gg, bb, max, d) });
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    [
        js_round((r1 + m) * 255.0),
        js_round((g1 + m) * 255.0),
        js_round((b1 + m) * 255.0),
    ]
}

/// A `Uint8Array` store: truncated, and wrapped past 255.
fn store(v: f64) -> u8 {
    shagger::shared::js::to_uint8(v)
}

/// The tinted texture and its glow, as RGBA bytes. Texels the fullbright
/// mask covers (0-255 per texel) lose their lit colour to the glow, so they
/// show their full colour whatever light falls on them.
#[must_use]
pub fn tint_pixels(pixels: &[u8], tint: HealthTint, fullbright: Option<&[u8]>) -> (Vec<u8>, Vec<u8>) {
    let mut color = vec![0; pixels.len()];
    let mut glow = vec![0; pixels.len()];
    for i in (0..pixels.len() - pixels.len() % 4).step_by(4) {
        let [r0, g0, b0] = [pixels[i], pixels[i + 1], pixels[i + 2]].map(f64::from);
        let [r, g, b] = tint_texel(r0, g0, b0, tint);
        let full = fullbright.map_or(0.0, |mask| f64::from(mask[i >> 2]) / 255.0);
        color[i] = store(r * (1.0 - full));
        color[i + 1] = store(g * (1.0 - full));
        color[i + 2] = store(b * (1.0 - full));
        color[i + 3] = pixels[i + 3];
        // The glow follows the original blue, so the blue trim shines
        // whatever colour it turns
        let blue = GLOW_BASE + (1.0 - GLOW_BASE) * blue_glow_weight(r0, g0, b0);
        let share = blue.max(full);
        glow[i] = store(r * share);
        glow[i + 1] = store(g * share);
        glow[i + 2] = store(b * share);
        glow[i + 3] = 255;
    }
    (color, glow)
}

/// A piece of a model: its triangles, and the box around them.
type Part = (Vec<usize>, [f64; 3], [f64; 3]);

/// The triangles (their first index's place) of a model's innermost part:
/// of its separate pieces (joined where they share a corner), the smallest
/// one whose bounds hold the model's centre. For the health bubble that is
/// the cross.
#[must_use]
pub fn innermost_part(positions: &[[f32; 3]], indices: &[u32]) -> Vec<usize> {
    let count = positions.len();
    let mut parent: Vec<usize> = (0..count).collect();
    fn find(parent: &mut [usize], mut v: usize) -> usize {
        while parent[v] != v {
            parent[v] = parent[parent[v]];
            v = parent[v];
        }
        v
    }
    fn join(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        parent[ra] = rb;
    }
    // Seams split a corner into several vertices; the same position joins
    // them again
    let mut by_place: HashMap<[i64; 3], usize> = HashMap::new();
    let mut center_min = [f64::INFINITY; 3];
    let mut center_max = [f64::NEG_INFINITY; 3];
    for (v, p) in positions.iter().enumerate() {
        let p = p.map(f64::from);
        let place = p.map(|x| js_round(x * 1e4) as i64);
        match by_place.get(&place) {
            None => {
                by_place.insert(place, v);
            }
            Some(&same) => join(&mut parent, v, same),
        }
        for a in 0..3 {
            center_min[a] = center_min[a].min(p[a]);
            center_max[a] = center_max[a].max(p[a]);
        }
    }
    let triangles = indices.len() / 3;
    for t in 0..triangles {
        let [a, b, c] = [0, 1, 2].map(|k| indices[t * 3 + k] as usize);
        join(&mut parent, a, b);
        join(&mut parent, a, c);
    }
    let center: [f64; 3] = std::array::from_fn(|a| (center_min[a] + center_max[a]) / 2.0);
    // In first-seen order, as a `Map` iterates.
    let mut order: Vec<usize> = Vec::new();
    let mut parts: HashMap<usize, Part> = HashMap::new();
    for t in 0..triangles {
        let root = find(&mut parent, indices[t * 3] as usize);
        let part = parts.entry(root).or_insert_with(|| {
            order.push(root);
            (Vec::new(), [f64::INFINITY; 3], [f64::NEG_INFINITY; 3])
        });
        part.0.push(t * 3);
        for k in 0..3 {
            let p = positions[indices[t * 3 + k] as usize].map(f64::from);
            for (a, x) in p.into_iter().enumerate() {
                part.1[a] = part.1[a].min(x);
                part.2[a] = part.2[a].max(x);
            }
        }
    }
    let mut inner: Option<(&Vec<usize>, f64)> = None;
    for root in &order {
        let (triangles, min, max) = &parts[root];
        if !(0..3).all(|a| min[a] <= center[a] && center[a] <= max[a]) {
            continue;
        }
        let size = js_hypot(&[max[0] - min[0], max[1] - min[1], max[2] - min[2]]);
        if inner.is_none_or(|(_, s)| size < s) {
            inner = Some((triangles, size));
        }
    }
    inner.map(|(t, _)| t.clone()).unwrap_or_default()
}

/// How far past a triangle's edge, in texels, the mask reaches, so
/// filtering never shows its lit border.
const MASK_BLEED: i64 = 2;

/// Cover the given triangles' texture area with 255, one byte per texel,
/// row v × height.
#[must_use]
pub fn triangle_mask(uvs: &[[f32; 2]], indices: &[u32], triangles: &[usize], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (i64::from(width), i64::from(height));
    let mut mask = vec![0u8; (width * height) as usize];
    for &t in triangles {
        let [a, b, c] = [0, 1, 2].map(|k| {
            let uv = uvs[indices[t + k] as usize];
            [f64::from(uv[0]) * w as f64, f64::from(uv[1]) * h as f64]
        });
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area == 0.0 {
            continue;
        }
        let x0 = 0.max(a[0].min(b[0]).min(c[0]).floor() as i64);
        let x1 = (w - 1).min(a[0].max(b[0]).max(c[0]).ceil() as i64);
        let y0 = 0.max(a[1].min(b[1]).min(c[1]).floor() as i64);
        let y1 = (h - 1).min(a[1].max(b[1]).max(c[1]).ceil() as i64);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                if w0 >= 0.0 && w1 >= 0.0 && w0 + w1 <= 1.0 {
                    mask[(y * w + x) as usize] = 255;
                }
            }
        }
    }
    // Spread the cover a few texels, first across and then down
    for pass in 0..2 {
        let from = mask.clone();
        for y in 0..h {
            for x in 0..w {
                if from[(y * w + x) as usize] != 0 {
                    continue;
                }
                for d in -MASK_BLEED..=MASK_BLEED {
                    let (nx, ny) = if pass == 0 { (x + d, y) } else { (x, y + d) };
                    if nx >= 0 && nx < w && ny >= 0 && ny < h && from[(ny * w + nx) as usize] != 0 {
                        mask[(y * w + x) as usize] = 255;
                        break;
                    }
                }
            }
        }
    }
    mask
}

/// A copy of a health bubble in its tier's colour (`tintHealthBubble`):
/// each textured material's albedo recoloured, with the cross of the first
/// primitive drawing it fullbright, and its glow as the emissive image at
/// emissive colour white. None for a tier without a tint or a model whose
/// texture cannot be read.
#[must_use]
pub fn tint_health_bubble(model: &GltfModel, tier: &str) -> Option<GltfModel> {
    let tint = health_tint(tier)?;
    let mut out = model.clone();
    let mut changed = false;
    for (index, material) in model.materials.iter().enumerate() {
        let Some(base) = material.base_color_image else {
            continue;
        };
        let Some(image) = model.images.get(base) else { continue };
        let Some(read) = decode_image(&image.name, &image.bytes) else {
            continue;
        };
        // `crossMask` of the first mesh drawing this material.
        let mask = model
            .primitives
            .iter()
            .find(|p| p.material == Some(index))
            .and_then(|p| {
                if p.uvs.len() != p.positions.len() {
                    return None;
                }
                let triangles = innermost_part(&p.positions, &p.indices);
                (!triangles.is_empty()).then(|| triangle_mask(&p.uvs, &p.indices, &triangles, read.width, read.height))
            });
        let (color, glow) = tint_pixels(&read.pixels, tint, mask.as_deref());
        let Some(albedo) = png_image(&format!("{}_{tier}", image.name), &color, read.width, read.height) else {
            continue;
        };
        let Some(emissive) = png_image(&format!("{}_{tier}_glow", image.name), &glow, read.width, read.height) else {
            continue;
        };
        out.images.push(albedo);
        out.images.push(emissive);
        let m = &mut out.materials[index];
        m.name = format!("{}_{tier}", material.name);
        m.base_color_image = Some(out.images.len() - 2);
        m.emissive_image = Some(out.images.len() - 1);
        m.emissive = [1.0; 3];
        changed = true;
    }
    changed.then_some(out)
}

/// The cyan-to-blue hues the megahealth's core is painted in, in degrees,
/// and the fade past them.
const CORE_HUE_CENTRE: f64 = 205.0;
const CORE_HUE_REACH: f64 = 45.0;
const CORE_HUE_FADE: f64 = 20.0;

fn smooth(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a texel belongs to a cyan-to-blue core, 0-1: its hue near
/// the core's and its colour saturated enough to read as one. Greys, whites
/// and other colours score nothing.
#[must_use]
pub fn core_hue_weight(r: f64, g: f64, b: f64) -> f64 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if max == 0.0 || d == 0.0 {
        return 0.0;
    }
    let off = (hue_of(r, g, b, max, d) - CORE_HUE_CENTRE).abs();
    (1.0 - smooth(CORE_HUE_REACH, CORE_HUE_REACH + CORE_HUE_FADE, off)) * smooth(0.2, 0.4, d / max)
}

/// Turn a texture's cyan-to-blue texels to `hue`, as RGBA bytes, with its
/// glow: the recoloured texels as strongly as the megahealth's blue glows,
/// black elsewhere. Every other texel keeps its colour.
#[must_use]
pub fn recolor_blue_pixels(pixels: &[u8], hue: f64) -> (Vec<u8>, Vec<u8>) {
    let mut color = vec![0; pixels.len()];
    let mut glow = vec![0; pixels.len()];
    let tint = HealthTint {
        hue: Some(hue),
        lightness: 1.0,
    };
    for i in (0..pixels.len() - pixels.len() % 4).step_by(4) {
        let [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]].map(f64::from);
        let w = core_hue_weight(r, g, b);
        let [tr, tg, tb] = if w > 0.0 { tint_texel(r, g, b, tint) } else { [r, g, b] };
        color[i] = store(r + (tr - r) * w);
        color[i + 1] = store(g + (tg - g) * w);
        color[i + 2] = store(b + (tb - b) * w);
        color[i + 3] = pixels[i + 3];
        let shine = w * blue_glow_weight(r, g, b).max(0.5);
        glow[i] = store(tr * shine);
        glow[i + 1] = store(tg * shine);
        glow[i + 2] = store(tb * shine);
        glow[i + 3] = 255;
    }
    (color, glow)
}

/// A copy of a model with its strongly blue parts in another colour,
/// glowing in it (`recolorBlueParts`): the ultra health is the megahealth
/// with a purple core. The glow is the emissive image, which
/// [`super::blue_glow::blue_glow_layer`] then makes breathe.
#[must_use]
pub fn recolor_blue_parts(model: &GltfModel, hue: f64) -> Option<GltfModel> {
    let mut out = model.clone();
    let mut changed = false;
    for (index, material) in model.materials.iter().enumerate() {
        let Some(base) = material.base_color_image else {
            continue;
        };
        let Some(image) = model.images.get(base) else { continue };
        let Some(read) = decode_image(&image.name, &image.bytes) else {
            continue;
        };
        let (color, glow) = recolor_blue_pixels(&read.pixels, hue);
        let name = format!("{}_hue{hue}", image.name);
        let (Some(albedo), Some(emissive)) = (
            png_image(&name, &color, read.width, read.height),
            png_image(&format!("{name}_glow"), &glow, read.width, read.height),
        ) else {
            continue;
        };
        out.images.push(albedo);
        out.images.push(emissive);
        let m = &mut out.materials[index];
        m.name = format!("{}_hue{hue}", material.name);
        m.base_color_image = Some(out.images.len() - 2);
        m.emissive_image = Some(out.images.len() - 1);
        m.emissive = [1.0; 3];
        changed = true;
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_turn_their_hue_and_the_small_keeps_its_blue() {
        let small = health_tint("health_small").unwrap();
        assert_eq!(tint_texel(10.0, 20.0, 250.0, small), [10.0, 20.0, 250.0]);
        let medium = health_tint("health_medium").unwrap();
        // Pure blue at hue 40: full red, two thirds green.
        assert_eq!(tint_texel(0.0, 0.0, 255.0, medium), [255.0, 170.0, 0.0]);
        // Grey has no saturation and stays grey, darker for the large.
        let large = health_tint("health_large").unwrap();
        assert_eq!(tint_texel(200.0, 200.0, 200.0, large), [170.0, 170.0, 170.0]);
        assert!(health_tint("health_mega").is_none());
    }

    #[test]
    fn the_fullbright_cross_loses_its_lit_colour_to_the_glow() {
        let medium = health_tint("health_medium").unwrap();
        let (color, glow) = tint_pixels(&[0, 0, 255, 9, 100, 100, 100, 255], medium, Some(&[255, 0]));
        assert_eq!(&color[..4], &[0, 0, 0, 9]);
        assert_eq!(&glow[..4], &[255, 170, 0, 255]);
        // A grey texel keeps its colour and glows at the base share.
        assert_eq!(&color[4..], &[100, 100, 100, 255]);
        assert_eq!(&glow[4..], &[25, 25, 25, 255]);
    }

    /// Two nested quads at the centre: a large one (two triangles) and a
    /// small one; the small one is innermost.
    #[test]
    fn the_innermost_part_is_the_smallest_holding_the_centre() {
        let quad = |s: f32, z: f32| [[-s, -s, z], [s, -s, z], [s, s, z], [-s, s, z]];
        let mut positions = quad(1.0, 0.0).to_vec();
        positions.extend(quad(0.3, 0.1));
        // A piece off to the side, which does not hold the centre.
        positions.extend([[0.8, 0.8, 0.2], [0.9, 0.8, 0.2], [0.9, 0.9, 0.2]]);
        let indices = [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10];
        assert_eq!(innermost_part(&positions, &indices), vec![6, 9]);
        let uvs: Vec<[f32; 2]> = positions
            .iter()
            .map(|p| [(p[0] + 1.0) / 2.0, (p[1] + 1.0) / 2.0])
            .collect();
        let mask = triangle_mask(&uvs, &indices, &[6, 9], 20, 20);
        assert_eq!(mask[10 * 20 + 10], 255);
        assert_eq!(mask[0], 0);
        // The cover bleeds two texels past the edge, and no further.
        assert_eq!(mask[10 * 20 + 5], 255);
        assert_eq!(mask[10 * 20 + 4], 0);
    }

    #[test]
    fn only_the_cyan_to_blue_core_turns_purple() {
        assert_eq!(core_hue_weight(200.0, 200.0, 200.0), 0.0);
        assert_eq!(core_hue_weight(255.0, 0.0, 0.0), 0.0);
        assert_eq!(core_hue_weight(0.0, 140.0, 255.0), 1.0);
        let (color, glow) = recolor_blue_pixels(&[0, 140, 255, 255, 255, 0, 0, 255], 285.0);
        assert_eq!(&color[..4], &[191, 0, 255, 255]);
        assert_eq!(&glow[..4], &[191, 0, 255, 255]);
        assert_eq!(&color[4..], &[255, 0, 0, 255]);
        assert_eq!(&glow[4..], &[0, 0, 0, 255]);
    }
}
