//! wash-fps `world/pickups/procedural/ProceduralPickupFactory.ts`, with the
//! part of `EntityManager.resolvePickupVisualFactory` that picks it: each
//! map health, armour, ammo and powerup pickup's procedural visual, and its
//! draws.
//!
//! Each recipe's prototypes go up as one model, a primitive per visual part
//! with that part's `StandardMaterial`; every pickup of the recipe draws
//! the same primitives at its own pose. Babylon's builders wind front faces
//! counter-clockwise on screen, so the triangles are turned to the model
//! renderer's clockwise front.

use std::collections::HashMap;
use std::io::Cursor;

use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::materials::texture::get_texture_matrix;
use shagger::babylon::meshes::vertex_data::VertexData;
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelImage, ModelMaterial, ModelPrimitive, Skeleton};
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::rendering::glam::{DMat3, DVec3, Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};

use super::procedural_pickup_asset_cache::{ProceduralPickupAssetCache, PrototypeSet};
use super::procedural_pickup_materials::{
    RING_GRADIENT_WIDTH, create_fresnel_shell_material, create_ring_gradient_material, ring_gradient_pixels,
};
use super::procedural_pickup_recipes::all_procedural_pickup_recipes;
use super::procedural_pickup_visual::{PartMaterial, ProceduralPickupVisual};
use super::types::{ProceduralPickupFamily, ProceduralPickupRecipe};
use crate::shared::bsp::pickup_table::{PickupEntry, PickupType};
use crate::world::pickups::ammo_icon_texture::load_ammo_icon_texture;

/// The recipe for a pickup the table names by type and kind: its
/// definition's registry id (`health_medium`, `ammo_plasmagun`, ...).
/// Weapons have none: the pickup is the gun.
#[must_use]
pub fn recipe_for_pickup(kind_type: PickupType, kind: &str) -> Option<&'static ProceduralPickupRecipe> {
    let family = match kind_type {
        PickupType::Weapon => return None,
        PickupType::Ammo => ProceduralPickupFamily::Ammo,
        PickupType::Armor => ProceduralPickupFamily::Armor,
        PickupType::Health => ProceduralPickupFamily::Health,
        PickupType::Powerup => ProceduralPickupFamily::Powerup,
    };
    all_procedural_pickup_recipes()
        .iter()
        .find(|r| r.family == family && r.kind == kind)
}

/// The map's procedural pickups.
pub struct ProceduralPickups {
    visuals: Vec<(ProceduralPickupVisual, ModelHandle)>,
    /// Icons that did not load, by path.
    pub missing_icons: Vec<String>,
}

fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .expect("encoding a PNG in memory");
    out.into_inner()
}

/// A prototype as a model primitive: winding turned, bounds measured.
fn primitive(name: &str, data: &VertexData, material: usize, order: usize) -> ModelPrimitive {
    let positions: Vec<[f32; 3]> = data.positions.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
    let (min, max) = positions
        .iter()
        .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| {
            (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
        });
    let uvs = if data.uvs.len() == positions.len() * 2 {
        data.uvs.chunks_exact(2).map(|t| [t[0], t[1]]).collect()
    } else {
        vec![[0.0; 2]; positions.len()]
    };
    ModelPrimitive {
        name: name.to_owned(),
        node: 0,
        node_scale: Vec3::ONE,
        node_matrix: Mat4::IDENTITY,
        normals: data.normals.chunks_exact(3).map(|n| [n[0], n[1], n[2]]).collect(),
        positions,
        uvs,
        indices: data.indices.chunks_exact(3).flat_map(|f| [f[2], f[1], f[0]]).collect(),
        material: Some(material),
        bounds_center: (min + max) * 0.5,
        creation_order: order,
        skin: None,
    }
}

fn material_params(
    set: &PrototypeSet,
    recipe: &ProceduralPickupRecipe,
    material: PartMaterial,
) -> StandardMaterialParams {
    match material {
        PartMaterial::Structural => set.structural,
        PartMaterial::ArmorPlate => set.armor_plate,
        PartMaterial::EmblemFill => set.emblem_fill,
        PartMaterial::AmmoIcon => set.ammo_icon_mat.unwrap_or_default(),
        PartMaterial::Energy => set.energy_core,
        PartMaterial::Accent => set.energy_accent,
        PartMaterial::Soft => set.energy_soft,
        PartMaterial::Shell => create_fresnel_shell_material(recipe.color),
        PartMaterial::Ring => create_ring_gradient_material(),
    }
}

/// Uploads one recipe's parts, as a visual of it lists them.
fn upload_recipe(
    gpu: &Gpu,
    models: &mut ModelRenderer,
    set: &PrototypeSet,
    visual: &ProceduralPickupVisual,
    icon: Option<&(u32, u32, Vec<u8>)>,
) -> ModelHandle {
    let recipe = &visual.recipe;
    let mut images = Vec::new();
    let mut materials = Vec::new();
    let mut modes = Vec::new();
    let mut primitives = Vec::new();
    for (i, part) in visual.parts.iter().enumerate() {
        let image = match part.material {
            PartMaterial::Ring => {
                let pixels = ring_gradient_pixels(recipe.color, recipe.accent_color);
                images.push(ModelImage {
                    name: format!("pp_ring_{}_grad.png", recipe.id),
                    mime_type: Some("image/png".to_owned()),
                    bytes: png(RING_GRADIENT_WIDTH, 1, &pixels),
                });
                Some(images.len() - 1)
            }
            PartMaterial::AmmoIcon => icon.map(|(w, h, pixels)| {
                images.push(ModelImage {
                    name: format!("pp_icon_{}.png", recipe.id),
                    mime_type: Some("image/png".to_owned()),
                    bytes: png(*w, *h, pixels),
                });
                images.len() - 1
            }),
            _ => None,
        };
        materials.push(ModelMaterial {
            name: format!("{i}"),
            base_color: [1.0; 4],
            base_color_image: image,
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        });
        modes.push(material_params(set, recipe, part.material));
        let data = set.geometry(part.proto).expect("a visual part has its prototype");
        primitives.push(primitive(&format!("pp_{}_{}", part.name, recipe.id), data, i, i));
    }
    let model = GltfModel {
        primitives,
        materials,
        images,
        attachment_points: Default::default(),
        skeleton: Skeleton::default(),
    };
    models.upload(gpu, &model, |m| {
        let i: usize = m.name.parse().unwrap_or(0);
        ModelMaterialMode::Standard(modes[i])
    })
}

/// `createProceduralPickupFactory` for every non-weapon map pickup:
/// builds each visual at its landed position and uploads each recipe once.
pub fn build_procedural_pickups(
    gpu: &Gpu,
    pk3: &mut Pk3Loader,
    models: &mut ModelRenderer,
    entries: &[PickupEntry],
) -> ProceduralPickups {
    let mut cache = ProceduralPickupAssetCache::default();
    // Each recipe's prototypes, as uploaded, and the model.
    let mut uploaded: HashMap<&'static str, (PrototypeSet, ModelHandle)> = HashMap::new();
    let mut visuals = Vec::new();
    let mut missing_icons = Vec::new();
    for entry in entries {
        let Some(recipe) = recipe_for_pickup(entry.kind_type, entry.kind) else {
            continue;
        };
        let position = DVec3::new(entry.position.x, entry.position.y, entry.position.z);
        let (set, model) = uploaded.entry(recipe.id).or_insert_with(|| {
            let mut set = cache.get_prototype_set(recipe).clone();
            let icon = recipe.icon_texture.as_ref().and_then(|path| {
                let icon = load_ammo_icon_texture(pk3, path).map(|t| (t.width, t.height, t.pixels));
                if icon.is_none() {
                    // The decal stays invisible without its art.
                    missing_icons.push(path.clone());
                    set.ammo_icon_mat = None;
                }
                icon
            });
            let first = ProceduralPickupVisual::new(recipe, &set, position, entry.id);
            let model = upload_recipe(gpu, models, &set, &first, icon.as_ref());
            (set, model)
        });
        visuals.push((ProceduralPickupVisual::new(recipe, set, position, entry.id), *model));
    }
    ProceduralPickups { visuals, missing_icons }
}

impl ProceduralPickups {
    /// How many pickups have a visual.
    #[must_use]
    pub fn len(&self) -> usize {
        self.visuals.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.visuals.is_empty()
    }

    /// Every part's draw `elapsed` seconds after the visuals were built;
    /// `camera_rotation` turns camera axes into world axes, for billboards.
    pub fn draws(&self, elapsed: f64, camera_rotation: DMat3, out: &mut Vec<ModelDraw>) {
        for (visual, model) in &self.visuals {
            let pose = visual.pose(elapsed);
            for (i, part) in visual.parts.iter().enumerate() {
                let mut draw = ModelDraw::new(*model, i, visual.part_world(part, &pose, camera_rotation).as_mat4());
                draw.emissive = visual.part_emissive(part, &pose);
                if part.material == PartMaterial::Ring {
                    draw.texture_matrix = Some(get_texture_matrix(pose.ring_offset, 0.0, 1.0, 1.0, 0.0));
                }
                out.push(draw);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_kinds_find_their_recipes() {
        assert_eq!(
            recipe_for_pickup(PickupType::Health, "medium").unwrap().id,
            "health_medium"
        );
        assert_eq!(
            recipe_for_pickup(PickupType::Ammo, "cells").unwrap().id,
            "ammo_plasmagun"
        );
        assert_eq!(recipe_for_pickup(PickupType::Armor, "ra").unwrap().id, "armor_ra");
        assert_eq!(
            recipe_for_pickup(PickupType::Powerup, "quad").unwrap().id,
            "powerup_quad"
        );
        assert!(recipe_for_pickup(PickupType::Weapon, "rocketlauncher").is_none());
    }
}
