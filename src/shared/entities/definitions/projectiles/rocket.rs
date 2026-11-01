//! wash-fps `shared/entities/definitions/projectiles/rocket.ts`: the rocket,
//! with splash damage (Warsow's gs_weapondefs.c).

use shagger::shared::entities::projectile_def::{DefColor, PROJECTILE_DEFAULTS, ProjectileDef};

pub const ROCKET: ProjectileDef = ProjectileDef {
    id: "rocket",
    name: "Rocket",
    classname: Some("projectile_rocket"),
    model_path: Some("models/objects/projectile/rlauncher/rocket_strong.md3"),
    scale: Some(1.0),
    speed: 1150.0,
    damage: 80.0,
    splash_damage: Some(15.0),
    splash_radius: Some(125.0),
    lifetime: 10.0,
    gravity: Some(0.0),
    bounces: Some(0),
    bounce_factor: None,
    light_color: Some(DefColor { r: 1.0, g: 0.5, b: 0.1 }),
    light_intensity: Some(10.0),
    light_range: Some(30.0),
    trail_effect: Some("rocket_trail"),
    impact_effect: Some("rocket_impact"),
    ..PROJECTILE_DEFAULTS
};
