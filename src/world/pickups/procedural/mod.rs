//! wash-fps `world/pickups/procedural`: generated sculptures for health,
//! armour, ammo and the quad (docs/research/PROCEDURAL_ITEMS.md there).
//!
//! Recipes say what each tier looks like; the geometry is built from
//! Babylon's mesh builders (`shagger::babylon::meshes`) and drawn with
//! Babylon `StandardMaterial`s; the visual animates it from an elapsed clock.
//! Not ported yet: the collect and respawn transients
//! (`ProceduralPickupTransient.ts`) and the respawn assembly, as pickups are
//! all drawn as available; satellites and beacons, which no recipe uses.

pub mod procedural_pickup_asset_cache;
pub mod procedural_pickup_factory;
pub mod procedural_pickup_geometry;
pub mod procedural_pickup_materials;
pub mod procedural_pickup_recipes;
pub mod procedural_pickup_visual;
pub mod types;
