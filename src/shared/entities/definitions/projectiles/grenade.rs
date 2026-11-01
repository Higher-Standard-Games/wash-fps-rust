//! wash-fps `shared/entities/definitions/projectiles/grenade.ts`: the
//! bouncing grenade.

use shagger::shared::entities::projectile_def::{DefColor, PROJECTILE_DEFAULTS, ProjectileDef};

pub const GRENADE: ProjectileDef = ProjectileDef {
    id: "grenade",
    name: "Grenade",
    classname: Some("projectile_grenade"),
    model_path: Some("models/objects/projectile/glauncher/grenadestrong.md3"),
    scale: Some(1.0),
    speed: 1080.0,
    damage: 80.0,
    splash_damage: Some(15.0),
    splash_radius: Some(125.0),
    lifetime: 1.625,
    gravity: Some(1.163),
    // Bounces before exploding.
    bounces: Some(3),
    bounce_factor: Some(0.45),
    light_color: Some(DefColor { r: 0.3, g: 0.5, b: 1.0 }),
    light_intensity: Some(6.0),
    light_range: Some(20.0),
    trail_effect: Some("grenade_trail"),
    impact_effect: Some("grenade_impact"),
    ..PROJECTILE_DEFAULTS
};
