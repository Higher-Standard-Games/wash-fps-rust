//! wash-fps `shared/entities/definitions/projectiles/plasma_bolt.ts`: the
//! plasma gun's bolt.

use shagger::shared::entities::projectile_def::{DefColor, PROJECTILE_DEFAULTS, ProjectileDef};

pub const PLASMA_BOLT: ProjectileDef = ProjectileDef {
    id: "plasma_bolt",
    name: "Plasma Bolt",
    classname: Some("projectile_plasma"),
    model_path: Some("models/objects/projectile/plasmagun/proj_plasmagun.md3"),
    texture_paths: &["models/objects/projectile/plasmagun/plnew1.tga"],
    scale: Some(1.0),
    // 50% faster than the original 2500.
    speed: 3750.0,
    damage: 15.0,
    splash_damage: Some(5.0),
    splash_radius: Some(40.0),
    lifetime: 5.0,
    gravity: Some(0.0),
    bounces: Some(0),
    bounce_factor: None,
    // Plasma green, as the gun model.
    light_color: Some(DefColor { r: 0.3, g: 1.0, b: 0.3 }),
    light_intensity: Some(8.0),
    light_range: Some(25.0),
    trail_effect: Some("plasma_trail"),
    trail_length: Some(120.0),
    impact_effect: Some("plasma_impact"),
    ..PROJECTILE_DEFAULTS
};
