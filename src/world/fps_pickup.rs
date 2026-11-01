//! wash-fps `world/FPSPickup.ts` over SHAG's `Pickup.loadModel`: map weapon
//! pickups drawn with their models.
//!
//! Each weapon pickup draws its model as the pickup cache hands it out
//! (flattened: a leader mesh at the landed position, turned by
//! [`map_weapon_yaw`], scaled by the definition's scale, the rest following
//! it), with `_glow` layers drawn as added light. Its barrels
//! (`_loadBarrels`) hang on the weapon's barrel mount, kept whole.
//!
//! Health, armour, ammo and powerups draw as procedural visuals in wash-fps;
//! they are not drawn here yet. Every pickup is drawn, as if all were
//! available.

use std::collections::HashMap;

use shagger::loader::model::gltf_model::GltfModel;
use shagger::loader::model::tags_loader::parse_tags;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::rendering::glam::Vec3;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelRenderer};
use shagger::weapon::weapon_barrels::{
    BarrelDef, barrel_material_mode, barrel_root_matrix, find_barrel_mount, world_weapon_material_mode,
};
use shagger::world::pickup::{flattened_pickup_matrices, leader_matrix, pickup_scale};

use super::entity_manager::map_weapon_yaw;
use crate::shared::bsp::pickup_table::{PickupEntry, PickupType};
use crate::shared::entities::definitions::weapons::weapons_table::weapon_model;

/// A model path as the definitions give it (`/data/models/...`), as a
/// package path (`readIndexedModelAsset`).
fn package_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .trim_start_matches("data/")
        .trim_start_matches('/')
        .to_owned()
}

/// A model's directory, with its trailing slash.
fn directory(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..=i])
}

/// Every weapon pickup's draws, built once.
pub struct WeaponPickups {
    pub draws: Vec<ModelDraw>,
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
    for entry in entries {
        if entry.kind_type != PickupType::Weapon {
            continue;
        }
        let Some(def) = weapon_model(entry.kind) else { continue };
        let Some((handle, model)) = load(def.model_path, false, models, pk3) else {
            continue;
        };
        let position = Vec3::new(
            entry.position.x as f32,
            entry.position.y as f32,
            entry.position.z as f32,
        );
        let leader = leader_matrix(position, map_weapon_yaw(entry.id) as f32, pickup_scale(Some(def.scale)));
        for (primitive, world) in flattened_pickup_matrices(&model, leader).into_iter().enumerate() {
            draws.push(ModelDraw {
                model: handle,
                primitive,
                world,
            });
        }
        let Some(first) = def.barrels.first() else { continue };
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
                draws.push(ModelDraw {
                    model: barrel_handle,
                    primitive,
                    world: root * p.node_matrix,
                });
            }
        }
    }
    WeaponPickups { draws, failures }
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
