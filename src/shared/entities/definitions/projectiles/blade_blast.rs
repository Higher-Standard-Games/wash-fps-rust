//! wash-fps `shared/entities/definitions/projectiles/blade_blast.ts`: the
//! gunblade's charged blast.

use shagger::shared::entities::projectile_def::{DefColor, PROJECTILE_DEFAULTS, ProjectileDef};

pub const BLADE_BLAST: ProjectileDef = ProjectileDef {
    id: "blade_blast",
    name: "Blade Blast",
    classname: Some("projectile_blade"),
    model_path: Some("models/objects/projectile/gunblade/proj_gunbladeblast.md3"),
    scale: Some(1.5),
    speed: 2500.0,
    damage: 50.0,
    splash_damage: Some(50.0),
    // Twice the plasma's 40.
    splash_radius: Some(90.0),
    lifetime: 5.0,
    gravity: Some(0.0),
    bounces: Some(0),
    bounce_factor: None,
    light_color: Some(DefColor {
        r: 0.05,
        g: 1.0,
        b: 0.9,
    }),
    light_intensity: Some(12.0),
    light_range: Some(35.0),
    trail_effect: Some("blade_trail"),
    impact_effect: Some("blade_impact"),
    ..PROJECTILE_DEFAULTS
};
