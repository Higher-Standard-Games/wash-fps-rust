//! wash-fps `world/pickups/procedural/ProceduralPickupAssetCache.ts`: each
//! recipe's prototype geometry and shared materials, built once and shared
//! by every pickup of the recipe.

use std::collections::HashMap;

use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::meshes::vertex_data::VertexData;
use shagger::rendering::glam::Vec3;

use super::procedural_pickup_geometry as geo;
use super::procedural_pickup_materials::{
    ENERGY_BOOST, create_ammo_icon_material, create_armor_plate_material, create_emblem_fill_material,
    create_energy_material_template, create_structural_material,
};
use super::procedural_pickup_recipes::PROCEDURAL_RECIPE_REVISION;
use super::types::{ProceduralPickupFamily, ProceduralPickupRecipe};

/// One of a [`PrototypeSet`]'s meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Proto {
    Cross,
    CrossCaps,
    Core,
    Armor,
    ArmorFrame,
    AmmoCrate,
    AmmoPanel,
    AmmoMark,
    AmmoIcon,
    PowerupCore,
    PowerupMark,
    Ring,
}

/// `PrototypeSet`: one recipe's prototype geometry and shared materials.
#[derive(Debug, Clone)]
pub struct PrototypeSet {
    pub recipe_id: &'static str,
    pub cross: Option<VertexData>,
    /// Plates over the cross's up and down faces (health).
    pub cross_caps: Option<VertexData>,
    pub core: Option<VertexData>,
    pub armor: Option<VertexData>,
    pub armor_frame: Option<VertexData>,
    pub ammo_crate: Option<VertexData>,
    pub ammo_panel: Option<VertexData>,
    pub ammo_mark: Option<VertexData>,
    /// The crate's narrow-side decals (ammo).
    pub ammo_icon: Option<VertexData>,
    pub powerup_core: Option<VertexData>,
    pub powerup_mark: Option<VertexData>,
    pub ring: Option<VertexData>,
    /// Dark, lit relic material.
    pub structural: StandardMaterialParams,
    /// Solid, saturated, lit armour plate.
    pub armor_plate: StandardMaterialParams,
    /// Tier colour for the cross's up and down faces.
    pub emblem_fill: StandardMaterialParams,
    /// The icon decal for ammo crates.
    pub ammo_icon_mat: Option<StandardMaterialParams>,
    /// Core colour emissive (pulsed per pickup).
    pub energy_core: StandardMaterialParams,
    /// Accent colour emissive.
    pub energy_accent: StandardMaterialParams,
    /// Low-alpha soft emissive.
    pub energy_soft: StandardMaterialParams,
}

impl PrototypeSet {
    /// A prototype's geometry, when the recipe has it.
    #[must_use]
    pub fn geometry(&self, proto: Proto) -> Option<&VertexData> {
        match proto {
            Proto::Cross => self.cross.as_ref(),
            Proto::CrossCaps => self.cross_caps.as_ref(),
            Proto::Core => self.core.as_ref(),
            Proto::Armor => self.armor.as_ref(),
            Proto::ArmorFrame => self.armor_frame.as_ref(),
            Proto::AmmoCrate => self.ammo_crate.as_ref(),
            Proto::AmmoPanel => self.ammo_panel.as_ref(),
            Proto::AmmoMark => self.ammo_mark.as_ref(),
            Proto::AmmoIcon => self.ammo_icon.as_ref(),
            Proto::PowerupCore => self.powerup_core.as_ref(),
            Proto::PowerupMark => self.powerup_mark.as_ref(),
            Proto::Ring => self.ring.as_ref(),
        }
    }
}

fn cache_key(recipe: &ProceduralPickupRecipe) -> String {
    format!("{}:high:{PROCEDURAL_RECIPE_REVISION}", recipe.id)
}

/// `buildPrototypeSet`.
#[must_use]
pub fn build_prototype_set(recipe: &ProceduralPickupRecipe) -> PrototypeSet {
    let mut structural = create_structural_material(recipe.color);
    if recipe.family == ProceduralPickupFamily::Powerup {
        structural.diffuse_color = Vec3::new(0.24, 0.15, 0.045);
        structural.emissive_color = Vec3::new(0.09, 0.055, 0.015);
    }
    let is_health = recipe.family == ProceduralPickupFamily::Health;
    let is_armor = recipe.family == ProceduralPickupFamily::Armor;
    let is_ammo = recipe.family == ProceduralPickupFamily::Ammo;
    let is_powerup = recipe.family == ProceduralPickupFamily::Powerup;
    let has_icon = is_ammo && recipe.icon_texture.is_some();
    let build = |on: bool, f: fn(&ProceduralPickupRecipe) -> VertexData| on.then(|| f(recipe));
    PrototypeSet {
        recipe_id: recipe.id,
        cross: build(is_health, geo::build_cross_proto),
        cross_caps: build(is_health, geo::build_cross_caps_proto),
        core: build(is_health, geo::build_core_proto),
        armor: build(is_armor, geo::build_armor_proto),
        armor_frame: build(is_armor, geo::build_armor_frame_proto),
        ammo_crate: build(is_ammo, geo::build_ammo_crate_proto),
        ammo_panel: build(is_ammo, geo::build_ammo_panel_proto),
        ammo_mark: build(is_ammo, geo::build_ammo_mark_proto),
        ammo_icon: build(has_icon, geo::build_ammo_icon_proto),
        powerup_core: build(is_powerup, geo::build_powerup_core_proto),
        powerup_mark: build(is_powerup, geo::build_powerup_mark_proto),
        ring: build(recipe.ring_count > 0, geo::build_ring_proto),
        structural,
        armor_plate: create_armor_plate_material(recipe.color),
        emblem_fill: create_emblem_fill_material(recipe.color),
        ammo_icon_mat: has_icon.then(create_ammo_icon_material),
        energy_core: create_energy_material_template(recipe.color, 1.0, ENERGY_BOOST),
        energy_accent: create_energy_material_template(recipe.accent_color, 1.0, ENERGY_BOOST),
        energy_soft: create_energy_material_template(recipe.color, 0.3, ENERGY_BOOST),
    }
}

/// The scene's cache (`sceneCaches` for one scene): prototype sets by
/// recipe, built on first request.
#[derive(Debug, Default)]
pub struct ProceduralPickupAssetCache {
    sets: HashMap<String, PrototypeSet>,
}

impl ProceduralPickupAssetCache {
    /// `getPrototypeSet`.
    pub fn get_prototype_set(&mut self, recipe: &ProceduralPickupRecipe) -> &PrototypeSet {
        self.sets
            .entry(cache_key(recipe))
            .or_insert_with(|| build_prototype_set(recipe))
    }
}
