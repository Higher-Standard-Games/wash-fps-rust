//! wash-fps `world/FPSPickup.ts` over SHAG's `Pickup.loadModel`: map weapon
//! pickups drawn with their models.
//!
//! Each weapon pickup draws its model as the pickup cache hands it out
//! (flattened: a leader mesh at the landed position, turned by
//! [`map_weapon_yaw`], scaled by the definition's scale, the rest following
//! it), with `_glow` layers drawn as added light. Its barrels
//! (`_loadBarrels`) hang on the weapon's barrel mount, kept whole.
//!
//! The armors and the megahealth draw their packaged models
//! ([`ItemModels`]), floating, bobbing and turning (`Pickup.bob`); the
//! megahealth's blue core glows and sparkles rise from it. The other health,
//! ammo and powerups draw as procedural visuals (`pickups/procedural`), and
//! so do the armors and megahealth while their package is missing. Each pickup's draws are kept by its id, so the
//! renderer leaves out the ones the server says are taken
//! (`reconcileActiveSet`).

use std::collections::HashMap;

use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::model::tags_loader::parse_tags;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::rendering::glam::Vec3;
use shagger::rendering::glam::{DMat3, DVec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::ModelMaterialMode;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelRenderer};
use shagger::weapon::weapon_barrels::{
    BarrelDef, barrel_material_mode, barrel_root_matrix, find_barrel_mount, world_weapon_material_mode,
};
use shagger::world::pickup::{
    flattened_pickup_matrices, float_height, item_bob, leader_matrix, pickup_scale, placed_min_y,
};

use super::entity_manager::map_weapon_yaw;
use super::pickups::blue_glow::{blue_glow_intensity, blue_glow_layer};
use super::pickups::item_sparkles::{ItemSparkles, upload_sparkle_quad};
use crate::shared::bsp::pickup_table::{PickupEntry, PickupType};
use crate::shared::entities::definitions::items::item_model;
use crate::shared::entities::definitions::weapons::weapons_table::weapon_model;

/// A model path as the definitions give it (`/data/models/...`), as a
/// package path (`readIndexedModelAsset`).
pub fn package_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .trim_start_matches("data/")
        .trim_start_matches('/')
        .to_owned()
}

/// A model's directory, with its trailing slash.
#[must_use]
pub fn directory(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..=i])
}

/// Every weapon pickup's draws, built once.
pub struct WeaponPickups {
    pub draws: Vec<ModelDraw>,
    /// Each pickup's id and its draws in [`Self::draws`], in order.
    pub spans: Vec<(u32, std::ops::Range<usize>)>,
    /// Model paths that failed to load, with why.
    pub failures: Vec<String>,
}

/// Loads the models the map's weapon pickups use and lists their draws.
/// `entries` are the landed pickups (`landMapPickups`).
pub fn build_weapon_pickups(
    gpu: &Gpu,
    pk3: &mut Pk3Loader,
    models: &mut ModelRenderer,
    entries: &[PickupEntry],
) -> WeaponPickups {
    let mut loaded: HashMap<String, Option<(ModelHandle, GltfModel)>> = HashMap::new();
    let mut failures = Vec::new();
    let mut load = |path: &str,
                    barrel: bool,
                    models: &mut ModelRenderer,
                    pk3: &mut Pk3Loader|
     -> Option<(ModelHandle, GltfModel)> {
        let key = format!("{barrel}:{path}");
        if let Some(entry) = loaded.get(&key) {
            return entry.clone();
        }
        let package = package_path(path);
        let model = match pk3.get_file(&package).map(|bytes| GltfModel::from_glb(&bytes)) {
            Some(Ok(model)) => Some(model),
            Some(Err(e)) => {
                failures.push(format!("{package}: {e}"));
                None
            }
            None => {
                failures.push(format!("{package}: in no package"));
                None
            }
        };
        let entry = model.map(|model| {
            let handle = if barrel {
                models.upload(gpu, &model, barrel_material_mode)
            } else {
                models.upload(gpu, &model, world_weapon_material_mode)
            };
            (handle, model)
        });
        loaded.insert(key, entry.clone());
        entry
    };

    let mut draws = Vec::new();
    let mut spans = Vec::new();
    for entry in entries {
        if entry.kind_type != PickupType::Weapon {
            continue;
        }
        let first = draws.len();
        add_weapon_pickup(entry, models, pk3, &mut load, &mut draws);
        if draws.len() > first {
            spans.push((entry.id, first..draws.len()));
        }
    }
    WeaponPickups { draws, spans, failures }
}

/// One weapon pickup's model and barrels.
fn add_weapon_pickup(
    entry: &PickupEntry,
    models: &mut ModelRenderer,
    pk3: &mut Pk3Loader,
    load: &mut impl FnMut(&str, bool, &mut ModelRenderer, &mut Pk3Loader) -> Option<(ModelHandle, GltfModel)>,
    draws: &mut Vec<ModelDraw>,
) {
    let Some(def) = weapon_model(entry.kind) else { return };
    let Some((handle, model)) = load(def.model_path, false, models, pk3) else {
        return;
    };
    let position = Vec3::new(
        entry.position.x as f32,
        entry.position.y as f32,
        entry.position.z as f32,
    );
    let leader = leader_matrix(position, map_weapon_yaw(entry.id) as f32, pickup_scale(Some(def.scale)));
    for (primitive, world) in flattened_pickup_matrices(&model, leader).into_iter().enumerate() {
        draws.push(ModelDraw::new(handle, primitive, world));
    }
    let Some(first) = def.barrels.first() else { return };
    let tags = pk3
        .get_file(&format!("{}tags.txt", directory(&package_path(first.model_path))))
        .map(|bytes| parse_tags(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();
    let mount = find_barrel_mount(&tags, &model.attachment_points);
    for barrel in def.barrels {
        let Some((barrel_handle, barrel_model)) = load(barrel.model_path, true, models, pk3) else {
            continue;
        };
        let root = leader
            * barrel_root_matrix(
                &mount,
                &BarrelDef {
                    model_path: barrel.model_path.to_owned(),
                    offset: barrel.offset.map(Vec3::from),
                },
            );
        for (primitive, p) in barrel_model.primitives.iter().enumerate() {
            draws.push(ModelDraw::new(barrel_handle, primitive, root * p.node_matrix));
        }
    }
}

/// One packaged item model on the GPU: its lit model, its glow layer
/// and each glowing primitive's colour.
struct ItemModelAsset {
    handle: ModelHandle,
    model: GltfModel,
    glow: Option<(ModelHandle, Vec<Option<Vec3>>)>,
}

/// A placed item model.
struct ItemModel {
    id: u32,
    asset: usize,
    position: DVec3,
    scale: f32,
    /// `bobRestY`.
    rest_y: f64,
    sparkles: Option<ItemSparkles>,
}

/// The armors and the megahealth drawn from their packaged models.
#[derive(Default)]
pub struct ItemModels {
    assets: Vec<ItemModelAsset>,
    items: Vec<ItemModel>,
    sparkle_quad: Option<ModelHandle>,
    /// Model paths that failed to load, with why.
    pub failures: Vec<String>,
}

/// Loads the packaged item models the map's pickups use
/// (`EntityManager` with `bobbing` and `floatLift`); a pickup whose model
/// is missing is left to its procedural recipe.
pub fn build_item_models(
    gpu: &Gpu,
    pk3: &mut Pk3Loader,
    models: &mut ModelRenderer,
    entries: &[PickupEntry],
) -> ItemModels {
    let mut out = ItemModels::default();
    let mut by_path: HashMap<&'static str, Option<usize>> = HashMap::new();
    for entry in entries {
        let Some(def) = item_model(entry.kind_type, entry.kind) else {
            continue;
        };
        let asset = *by_path.entry(def.model_path).or_insert_with(|| {
            let package = package_path(def.model_path);
            let model = match pk3.get_file(&package).map(|bytes| GltfModel::from_glb(&bytes)) {
                Some(Ok(model)) => model,
                Some(Err(e)) => {
                    out.failures.push(format!("{package}: {e}"));
                    return None;
                }
                None => {
                    out.failures.push(format!("{package}: in no package"));
                    return None;
                }
            };
            let handle = models.upload(gpu, &model, world_weapon_material_mode);
            // The megahealth's blue core glows (`FPSPickup.loadModel`).
            let glow = (def.id == "health_mega")
                .then(|| blue_glow_layer(&model))
                .flatten()
                .map(|(layer, tints)| (models.upload(gpu, &layer, |_| ModelMaterialMode::AddedLight), tints));
            out.assets.push(ItemModelAsset { handle, model, glow });
            Some(out.assets.len() - 1)
        });
        let Some(asset) = asset else { continue };
        let scale = pickup_scale(Some(def.scale));
        let position = DVec3::new(entry.position.x, entry.position.y, entry.position.z);
        let leader = leader_matrix(position.as_vec3(), 0.0, scale);
        let min_y = placed_min_y(&out.assets[asset].model, leader);
        let sparkles = (def.id == "health_mega").then(|| {
            out.sparkle_quad.get_or_insert_with(|| upload_sparkle_quad(gpu, models));
            ItemSparkles::new(u64::from(entry.id) + 1)
        });
        out.items.push(ItemModel {
            id: entry.id,
            asset,
            position,
            scale,
            rest_y: float_height(position.y, f64::from(min_y), def.float_lift),
            sparkles,
        });
    }
    out
}

impl ItemModels {
    /// Whether the pickup draws a packaged model (and not its recipe).
    #[must_use]
    pub fn has(&self, id: u32) -> bool {
        self.items.iter().any(|i| i.id == id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Bobs, turns and draws the items `shown` keeps at `seconds` on the
    /// item clock; `camera` turns camera axes to world, for the sparkles.
    pub fn draws(&mut self, seconds: f64, camera: DMat3, shown: impl Fn(u32) -> bool, draws: &mut Vec<ModelDraw>) {
        let now_ms = seconds * 1000.0;
        let glow = blue_glow_intensity(seconds) as f32;
        for item in &mut self.items {
            let visible = shown(item.id);
            let (y, yaw) = item_bob(item.rest_y, item.id, 0.0, now_ms);
            let at = DVec3::new(item.position.x, y, item.position.z);
            if let Some(sparkles) = &mut item.sparkles {
                sparkles.update(seconds, visible);
            }
            if !visible {
                continue;
            }
            let asset = &self.assets[item.asset];
            let leader = leader_matrix(at.as_vec3(), yaw as f32, item.scale);
            let worlds = flattened_pickup_matrices(&asset.model, leader);
            for (primitive, world) in worlds.iter().enumerate() {
                draws.push(ModelDraw::new(asset.handle, primitive, *world));
            }
            if let Some((handle, tints)) = &asset.glow {
                for (primitive, (world, tint)) in worlds.iter().zip(tints).enumerate() {
                    if let Some(color) = tint {
                        let mut draw = ModelDraw::new(*handle, primitive, *world);
                        draw.tint = Some(*color * glow);
                        draws.push(draw);
                    }
                }
            }
            if let (Some(sparkles), Some(quad)) = (&item.sparkles, self.sparkle_quad) {
                sparkles.draws(quad, at, f64::from(item.scale), camera, draws);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_paths_become_package_paths() {
        assert_eq!(
            package_path("/data/models/weapons/rlauncher/rlauncher.glb"),
            "models/weapons/rlauncher/rlauncher.glb"
        );
        assert_eq!(
            directory("models/weapons/plasmagun/plasmagun_barrel.glb"),
            "models/weapons/plasmagun/"
        );
    }
}
