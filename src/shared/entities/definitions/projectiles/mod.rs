//! wash-fps `shared/entities/definitions/projectiles`: the projectile types.

pub mod blade_blast;
pub mod grenade;
pub mod plasma_bolt;
pub mod rocket;

use shagger::shared::entities::projectile_def::ProjectileDef;

/// Every projectile definition, as `index.ts` lists them.
pub const PROJECTILES: [&ProjectileDef; 4] = [
    &blade_blast::BLADE_BLAST,
    &grenade::GRENADE,
    &plasma_bolt::PLASMA_BOLT,
    &rocket::ROCKET,
];

/// A projectile definition by id (the registry's `get`).
#[must_use]
pub fn projectile_def(id: &str) -> Option<&'static ProjectileDef> {
    PROJECTILES.into_iter().find(|d| d.id == id)
}
