//! wash-fps `world/FPSPickup.ts` over SHAG's `Pickup.loadModel`: map weapon
//! pickups drawn with their models.
//!
//! Each weapon pickup draws its model as the pickup cache hands it out
//! (flattened: a leader mesh at the landed position, turned by
//! [`map_weapon_yaw`], scaled by the definition's scale, the rest following
//! it), with `_glow` layers drawn as added light. Its barrels
//! (`_loadBarrels`) hang on the weapon's barrel mount, kept whole.
//!
//! The armor, health, ammo and powerup pickups draw their packaged models
//! ([`ItemModels`]), floating, bobbing and turning (`Pickup.bob`): the
//! megahealth's blue core glows (the ultra's purple, the quad's Q turning
//! in its ring), the health bubbles take their tier's colour, the ammo
//! boxes their weapon's symbol, and sparkles rise from the major items.
//! They draw as procedural visuals (`pickups/procedural`) while their
//! package is missing. Each pickup's draws are kept by its id, so the
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
use super::pickups::ammo_box_symbols::{ammo_box_symbols, ammo_symbol_path, symbol_material};
use super::pickups::ammo_icon_texture::load_ammo_icon_texture;
use super::pickups::blue_glow::{blue_glow_intensity, blue_glow_layer};
use super::pickups::health_tint::{GLOW_INTENSITY, recolor_blue_parts, tint_health_bubble};
use super::pickups::item_sparkles::{CHOOSE_EVERY, ItemSparkles, choose_sparkling, upload_sparkle_quad};
use super::pickups::quad_core::{CORE_GLOW, add_spinning_core, core_matrix};
use crate::shared::bsp::pickup_table::{PickupEntry, PickupType};
use crate::shared::entities::definitions::items::{ItemModelDef, item_model};
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

/// How many sparkles rise from each of the major items; ammo boxes take
/// [`AMMO_SPARKLES`].
const ITEM_SPARKLES: [(&str, usize); 8] = [
    ("health_mega", 16),
    ("health_ultra", 16),
    ("powerup_quad", 16),
    ("armor_ra", 10),
    ("armor_ya", 10),
    ("armor_ga", 8),
    ("health_large", 8),
    ("health_medium", 6),
];
const AMMO_SPARKLES: usize = 5;
/// The ultra health's core: the megahealth's blue turned purple.
const ULTRA_CORE_HUE: f64 = 285.0;

/// How a glow layer's strength moves.
#[derive(Debug, Clone, Copy, PartialEq)]
enum GlowPace {
    /// With every blue core (`glowBlueParts`).
    Breathing,
    /// Held at this emissive intensity (a health bubble's).
    Steady(f32),
}

/// A quad's turning core on the GPU (`addSpinningCore`).
struct CoreAsset {
    /// Its primitive in the lit model.
    primitive: usize,
    centre: Vec3,
    /// Its added-light copy, drawn at [`CORE_GLOW`].
    glow: ModelHandle,
}

/// One packaged item model on the GPU: its lit model, its glow layer
/// and each glowing primitive's colour, its turning core and its ammo
/// symbols.
struct ItemModelAsset {
    handle: ModelHandle,
    model: GltfModel,
    glow: Option<(ModelHandle, Vec<Option<Vec3>>, GlowPace)>,
    core: Option<CoreAsset>,
    /// The ammo box's symbols and how many quads they are.
    symbols: Option<(ModelHandle, usize)>,
    /// The bounding sphere's radius in the model's own units.
    radius: f64,
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

/// Every pickup with a packaged model, drawn from it.
pub struct ItemModels {
    assets: Vec<ItemModelAsset>,
    items: Vec<ItemModel>,
    sparkle_quad: Option<ModelHandle>,
    /// The sparkles' clock (`SparkleField`).
    last: Option<f64>,
    since_choice: f64,
    /// Model paths that failed to load, with why.
    pub failures: Vec<String>,
}

impl Default for ItemModels {
    fn default() -> Self {
        Self {
            assets: Vec::new(),
            items: Vec::new(),
            sparkle_quad: None,
            last: None,
            since_choice: CHOOSE_EVERY,
            failures: Vec::new(),
        }
    }
}

/// The radius of the sphere around a primitive's box (Babylon's
/// `boundingSphere.radius`).
fn bounding_radius(model: &GltfModel) -> f64 {
    let Some(root) = model.primitives.first() else {
        return 0.0;
    };
    let (min, max) = root.positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))),
    );
    if min.is_finite() && max.is_finite() {
        f64::from((max - min).length()) / 2.0
    } else {
        0.0
    }
}

/// Loads one item's model and dresses it as `FPSPickup.loadModel` does.
fn build_item_asset(
    gpu: &Gpu,
    pk3: &mut Pk3Loader,
    models: &mut ModelRenderer,
    def: &ItemModelDef,
    failures: &mut Vec<String>,
) -> Option<ItemModelAsset> {
    let package = package_path(def.model_path);
    let mut model = match pk3.get_file(&package).map(|bytes| GltfModel::from_glb(&bytes)) {
        Some(Ok(model)) => model,
        Some(Err(e)) => {
            failures.push(format!("{package}: {e}"));
            return None;
        }
        None => {
            failures.push(format!("{package}: in no package"));
            return None;
        }
    };
    let id = def.id;
    // The ultra health is the megahealth's model with a purple core
    if id == "health_ultra"
        && let Some(recolored) = recolor_blue_parts(&model, ULTRA_CORE_HUE)
    {
        model = recolored;
    }
    // The quad's Q turns inside its ring, glowing in its own colours
    let spinning = (id == "powerup_quad").then(|| add_spinning_core(&model)).flatten();
    let core = spinning.map(|split| {
        model = split.model;
        CoreAsset {
            primitive: split.primitive,
            centre: split.centre,
            glow: models.upload(gpu, &split.glow, |_| ModelMaterialMode::AddedLight),
        }
    });
    // The health bubbles take their tier's colour and glow
    let mut pace = GlowPace::Breathing;
    if matches!(id, "health_small" | "health_medium" | "health_large") {
        if let Some(tinted) = tint_health_bubble(&model, id) {
            model = tinted;
        }
        pace = GlowPace::Steady(GLOW_INTENSITY as f32);
    }
    // The megahealth's blue core glows; so do the ultra's and the quad's
    let glows = matches!(
        id,
        "health_mega" | "health_ultra" | "powerup_quad" | "health_small" | "health_medium" | "health_large"
    );
    let glow = glows
        .then(|| blue_glow_layer(&model))
        .flatten()
        .map(|(layer, mut tints)| {
            // The core glows on its own, in its own colours
            if let Some(core) = &core {
                tints[core.primitive] = None;
            }
            (
                models.upload(gpu, &layer, |_| ModelMaterialMode::AddedLight),
                tints,
                pace,
            )
        });
    // An ammo box shows its weapon's symbol on each side
    let symbols = (def.pickup == PickupType::Ammo)
        .then(|| ammo_symbol_path(id))
        .flatten()
        .and_then(|path| {
            let texture = load_ammo_icon_texture(pk3, &path);
            if texture.is_none() {
                failures.push(format!("{path}: no symbol"));
            }
            ammo_box_symbols(&model, &texture?)
        })
        .map(|symbols| {
            let count = symbols.primitives.len();
            let handle = models.upload(gpu, &symbols, |_| ModelMaterialMode::Standard(symbol_material()));
            (handle, count)
        });
    // The glow layer adds the materials' emissive light (and breathes it):
    // the lit model leaves it out, or it would shine twice.
    let handle = if glow.is_some() {
        let mut lit = model.clone();
        for m in &mut lit.materials {
            m.emissive = [0.0; 3];
            m.emissive_image = None;
        }
        models.upload(gpu, &lit, world_weapon_material_mode)
    } else {
        models.upload(gpu, &model, world_weapon_material_mode)
    };
    let radius = bounding_radius(&model);
    Some(ItemModelAsset {
        handle,
        model,
        glow,
        core,
        symbols,
        radius,
    })
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
    // By definition: the tiers share a model but not its colours.
    let mut by_id: HashMap<&'static str, Option<usize>> = HashMap::new();
    for entry in entries {
        let Some(def) = item_model(entry.kind_type, entry.kind) else {
            continue;
        };
        let asset = *by_id.entry(def.id).or_insert_with(|| {
            let asset = build_item_asset(gpu, pk3, models, def, &mut out.failures)?;
            out.assets.push(asset);
            Some(out.assets.len() - 1)
        });
        let Some(asset) = asset else { continue };
        let scale = pickup_scale(Some(def.scale));
        let position = DVec3::new(entry.position.x, entry.position.y, entry.position.z);
        let leader = leader_matrix(position.as_vec3(), 0.0, scale);
        let min_y = placed_min_y(&out.assets[asset].model, leader);
        // Small sparkles rise from the major items
        let count = if def.pickup == PickupType::Ammo {
            AMMO_SPARKLES
        } else {
            ITEM_SPARKLES
                .iter()
                .find(|(id, _)| *id == def.id)
                .map_or(0, |(_, n)| *n)
        };
        let sparkles = (count > 0).then(|| {
            out.sparkle_quad.get_or_insert_with(|| upload_sparkle_quad(gpu, models));
            ItemSparkles::new(u64::from(entry.id) + 1, count, out.assets[asset].radius)
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
    /// item clock; the camera at `camera_position`, whose `camera` turns
    /// camera axes to world, picks and faces the sparkles.
    pub fn draws(
        &mut self,
        seconds: f64,
        camera_position: DVec3,
        camera: DMat3,
        shown: impl Fn(u32, DVec3) -> bool,
        draws: &mut Vec<ModelDraw>,
    ) {
        let now_ms = seconds * 1000.0;
        let breath = blue_glow_intensity(seconds) as f32;
        // Clamped, so a stall or an item coming back into range does not
        // jump the sparkles
        let dt = self.last.map_or(0.0, |last| (seconds - last).clamp(0.0, 0.1));
        self.last = Some(seconds);
        self.since_choice += dt;
        let placed: Vec<(DVec3, f64)> = self
            .items
            .iter()
            .map(|item| {
                let (y, yaw) = item_bob(item.rest_y, item.id, 0.0, now_ms);
                (DVec3::new(item.position.x, y, item.position.z), yaw)
            })
            .collect();
        if self.since_choice >= CHOOSE_EVERY {
            self.since_choice = 0.0;
            let distances: Vec<Option<f64>> = self
                .items
                .iter()
                .zip(&placed)
                .map(|(item, (at, _))| {
                    (item.sparkles.is_some() && shown(item.id, *at)).then(|| camera_position.distance(*at))
                })
                .collect();
            for item in &mut self.items {
                if let Some(sparkles) = &mut item.sparkles {
                    sparkles.active = false;
                }
            }
            for i in choose_sparkling(&distances) {
                if let Some(sparkles) = &mut self.items[i].sparkles {
                    sparkles.active = true;
                }
            }
        }
        for (item, (at, yaw)) in self.items.iter_mut().zip(placed) {
            if !shown(item.id, at) {
                continue;
            }
            let asset = &self.assets[item.asset];
            let leader = leader_matrix(at.as_vec3(), yaw as f32, item.scale);
            let mut worlds = flattened_pickup_matrices(&asset.model, leader);
            if let Some(core) = &asset.core {
                let world = core_matrix(leader, core.centre, seconds);
                worlds[core.primitive] = world;
                let mut draw = ModelDraw::new(core.glow, 0, world);
                draw.tint = Some(Vec3::splat(CORE_GLOW));
                draws.push(draw);
            }
            for (primitive, world) in worlds.iter().enumerate() {
                draws.push(ModelDraw::new(asset.handle, primitive, *world));
            }
            if let Some((handle, tints, pace)) = &asset.glow {
                let strength = match pace {
                    GlowPace::Breathing => breath,
                    GlowPace::Steady(s) => *s,
                };
                for (primitive, (world, tint)) in worlds.iter().zip(tints).enumerate() {
                    if let Some(color) = tint {
                        let mut draw = ModelDraw::new(*handle, primitive, *world);
                        draw.tint = Some(*color * strength);
                        draws.push(draw);
                    }
                }
            }
            if let Some((handle, count)) = asset.symbols {
                for primitive in 0..count {
                    draws.push(ModelDraw::new(handle, primitive, leader));
                }
            }
            if let (Some(sparkles), Some(quad)) = (&mut item.sparkles, self.sparkle_quad)
                && sparkles.active
            {
                sparkles.advance(dt, f64::from(item.scale));
                sparkles.draws(quad, leader, camera, draws);
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
