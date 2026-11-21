//! wash-fps `client/bomb/EmpBombModel.ts` and `client/bomb/EmpCorePlugin.ts`:
//! the Search & Destroy bomb's model and its purple glow.
//!
//! Loads `models/bomb_emp.glb` from the model pack, makes its purple parts
//! glow (an emissive mask built from the base texture's own purple texels)
//! and adds the moving lightning core. The caller drives both glows each
//! frame through [`EmpBombModel::draws`].
//!
//! Babylon adds the mask as the material's emissive texture, scaled by
//! `emissiveIntensity`; here, as with the megahealth's `BlueGlow`, the model
//! draws a second time as added light with the mask as its image and the
//! intensity as its tint. The core is shagger's pole ripple
//! ([`PoleRipple`]), a port of `EmpCorePlugin`'s fragment code, on the lit
//! draw.

use shagger::loader::model::gltf_model::{GltfModel, ModelMaterial};
use shagger::loader::texture::decode_image;
use shagger::rendering::glam::{Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer, PoleRipple};

use crate::world::fps_pickup::package_path;
use crate::world::pickups::blue_glow::png_image;

pub const EMP_BOMB_MODEL: &str = "/data/models/bomb_emp.glb";

/// Purple leading green by this much (0-255), in both red and blue, starts
/// to glow; fully by the upper value.
const PURPLE_LEAD_START: f64 = 25.0;
const PURPLE_LEAD_FULL: f64 = 90.0;

/// `EMP_CORE_RADIUS`: the disc's radius about the pole axis, in model units
/// (the model spans about ±0.46).
pub const EMP_CORE_RADIUS: f32 = 0.24;
/// `EMP_CORE_POLE_Z`: fragments nearer the equator than this, in model
/// units, are not on either disc.
pub const EMP_CORE_POLE_Z: f32 = 0.3;
/// Rings across the disc and how fast they run outward, in radians per model
/// unit and per second.
const RING_FREQUENCY: f32 = 90.0;
const RING_SPEED: f32 = 7.0;
/// How far the lightning wobbles, in texture coordinates (about four texels
/// of the 1024 atlas).
const WARP: f32 = 0.004;
/// The core's colour, multiplied into the texture it samples.
const CORE_TINT: Vec3 = Vec3::new(1.25, 0.55, 1.5);

/// How strongly a texel glows, 0-1: purple is red and blue both well over
/// green. Grey, white and black panels lead by nothing and stay dark.
#[must_use]
pub fn purple_glow_weight(r: f64, g: f64, b: f64) -> f64 {
    let lead = r.min(b) - g;
    let t = ((lead - PURPLE_LEAD_START) / (PURPLE_LEAD_FULL - PURPLE_LEAD_START)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Keep each texel's colour where it glows and black elsewhere, as RGBA
/// bytes (a `Uint8Array` store truncates).
#[must_use]
pub fn purple_glow_mask(pixels: &[u8]) -> Vec<u8> {
    let mut out = vec![0; pixels.len()];
    for (texel, mask) in pixels.as_chunks::<4>().0.iter().zip(out.as_chunks_mut::<4>().0) {
        let [r, g, b] = [texel[0], texel[1], texel[2]].map(f64::from);
        let w = purple_glow_weight(r, g, b);
        mask[0] = (r * w) as u8;
        mask[1] = (g * w) as u8;
        mask[2] = (b * w) as u8;
        mask[3] = 255;
    }
    out
}

/// The added-light copy of the model (`buildMask` on each material): each
/// material's base texture swapped for its purple mask. None when no
/// material has a base texture to mask.
#[must_use]
pub fn purple_glow_layer(model: &GltfModel) -> Option<GltfModel> {
    let mut images = Vec::new();
    // One mask per base texture: every material that shares it shares it.
    let mut masks: Vec<(usize, usize)> = Vec::new();
    let mut materials = Vec::new();
    for material in &model.materials {
        let mask = match material.base_color_image {
            Some(base) => match masks.iter().find(|(b, _)| *b == base) {
                Some(&(_, i)) => Some(i),
                None => {
                    let image = model.images.get(base)?;
                    let decoded = decode_image(&image.name, &image.bytes)?;
                    let mask = purple_glow_mask(&decoded.pixels);
                    images.push(png_image(
                        &format!("{}_purple_glow", image.name),
                        &mask,
                        decoded.width,
                        decoded.height,
                    )?);
                    masks.push((base, images.len() - 1));
                    Some(images.len() - 1)
                }
            },
            None => None,
        };
        materials.push(ModelMaterial {
            name: format!("{}_purple_glow", material.name),
            base_color: [1.0; 4],
            base_color_image: mask,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            ..material.clone()
        });
    }
    if masks.is_empty() {
        return None;
    }
    Some(GltfModel {
        primitives: model.primitives.clone(),
        materials,
        images,
        attachment_points: Default::default(),
        skeleton: model.skeleton.clone(),
    })
}

/// The glTF loader's `__root__`: a half turn about Y and scaling (1, 1, -1),
/// which together mirror X.
fn gltf_root() -> Mat4 {
    Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0))
}

/// `EmpBombModel`: the bomb, uploaded, and its glow layer.
pub struct EmpBombModel {
    handle: ModelHandle,
    /// The purple mask drawn as added light, when the texture had one.
    glow: Option<ModelHandle>,
    /// Each primitive's node matrix.
    nodes: Vec<Mat4>,
    /// Whether each primitive's material has a mask.
    masked: Vec<bool>,
}

impl EmpBombModel {
    /// `load`: the bomb from the packages, `file` reading a package path.
    /// Err says why it could not be loaded; the caller draws no bomb then.
    ///
    /// # Errors
    /// The model is in no package, or does not parse.
    pub fn load(
        gpu: &Gpu,
        models: &mut ModelRenderer,
        file: impl FnOnce(&str) -> Option<Vec<u8>>,
    ) -> Result<Self, String> {
        let path = package_path(EMP_BOMB_MODEL);
        let bytes = file(&path).ok_or_else(|| format!("{path}: in no package"))?;
        let model = GltfModel::from_glb(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let handle = models.upload(gpu, &model, |_| ModelMaterialMode::Lit);
        let layer = purple_glow_layer(&model);
        if layer.is_none() {
            eprintln!("[EmpBombModel] No glow mask: {path}");
        }
        let masked = model
            .primitives
            .iter()
            .map(|p| {
                layer
                    .as_ref()
                    .and_then(|l| p.material.and_then(|m| l.materials.get(m)))
                    .is_some_and(|m| m.base_color_image.is_some())
            })
            .collect();
        let glow = layer.map(|layer| models.upload(gpu, &layer, |_| ModelMaterialMode::AddedLight));
        Ok(Self {
            handle,
            glow,
            nodes: model.primitives.iter().map(|p| p.node_matrix).collect(),
            masked,
        })
    }

    /// The bomb's draws at `root` (the model's root, its scale included)
    /// with this frame's glow (`setGlow`): `shell` for the purple parts all
    /// over the bomb, `core` for the lightning disc, both multiples of the
    /// lit surface (above 1, bloom picks them up). `time` drives the core's
    /// ripple.
    pub fn draws(&self, root: Mat4, shell: f32, core: f32, time: f32, draws: &mut Vec<ModelDraw>) {
        let ripple = PoleRipple {
            radius: EMP_CORE_RADIUS,
            pole_z: EMP_CORE_POLE_Z,
            strength: core,
            time,
            tint: CORE_TINT,
            warp: WARP,
            ring_frequency: RING_FREQUENCY,
            ring_speed: RING_SPEED,
        };
        let base = root * gltf_root();
        for (primitive, node) in self.nodes.iter().enumerate() {
            let world = base * *node;
            draws.push(ModelDraw {
                pole_ripple: Some(ripple),
                ..ModelDraw::new(self.handle, primitive, world)
            });
            if let Some(glow) = self.glow
                && self.masked[primitive]
            {
                draws.push(ModelDraw {
                    tint: Some(Vec3::splat(shell)),
                    ..ModelDraw::new(glow, primitive, world)
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purple_glows_and_grey_white_black_red_and_blue_stay_dark() {
        assert_eq!(purple_glow_weight(190.0, 30.0, 240.0), 1.0);
        assert_eq!(purple_glow_weight(90.0, 90.0, 90.0), 0.0);
        assert_eq!(purple_glow_weight(255.0, 255.0, 255.0), 0.0);
        assert_eq!(purple_glow_weight(0.0, 0.0, 0.0), 0.0);
        assert_eq!(purple_glow_weight(255.0, 40.0, 40.0), 0.0);
        assert_eq!(purple_glow_weight(30.0, 60.0, 255.0), 0.0);
    }

    #[test]
    fn it_fades_in_as_red_and_blue_pull_ahead_of_green() {
        let faint = purple_glow_weight(110.0, 70.0, 120.0);
        let clear = purple_glow_weight(140.0, 70.0, 150.0);
        assert!(faint > 0.0);
        assert!(clear > faint);
        assert!(clear < 1.0);
    }

    #[test]
    fn the_mask_keeps_a_glowing_texels_colour_and_blacks_out_the_rest() {
        let mask = purple_glow_mask(&[190, 30, 240, 0, 90, 90, 90, 255]);
        assert_eq!(mask, vec![190, 30, 240, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn the_root_mirrors_x_as_babylons_root_does() {
        let at = gltf_root().transform_point3(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(at, Vec3::new(-1.0, 2.0, 3.0));
    }
}
