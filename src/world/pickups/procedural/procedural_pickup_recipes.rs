//! wash-fps `world/pickups/procedural/ProceduralPickupRecipes.ts`: each
//! health, armour, ammo and powerup tier's recipe, the one source of tunable
//! data. Dimensions are Quake units.

use std::sync::LazyLock;

use super::types::{CollectStyle, ProceduralPickupFamily, ProceduralPickupRecipe, RespawnStyle, Rgb, rgb};

/// `PROCEDURAL_RECIPE_REVISION`.
pub const PROCEDURAL_RECIPE_REVISION: &str = "v11";

/// `PROCEDURAL_GLOBAL_SCALE`: on top of each recipe's own scale.
pub const PROCEDURAL_GLOBAL_SCALE: f64 = 1.9;

/// The fields every recipe in the list shares, or starts from.
fn recipe(id: &'static str, family: ProceduralPickupFamily, kind: &'static str) -> ProceduralPickupRecipe {
    ProceduralPickupRecipe {
        id,
        family,
        kind,
        color: rgb(0.0, 0.0, 0.0),
        accent_color: rgb(0.0, 0.0, 0.0),
        scale: 1.0,
        ring_count: 0,
        plate_count: 0,
        mote_count: 0,
        shell_pieces: 0,
        satellite_count: 0,
        bob_amplitude: 0.0,
        bob_frequency: 0.0,
        spin_speed: 0.0,
        pulse_frequency: 0.0,
        figure_eight: false,
        beacon_strength: 0.0,
        collect_style: CollectStyle::Collapse,
        respawn_style: RespawnStyle::Assemble,
        high_tier: false,
        unstable: false,
        icon_texture: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn health(
    kind: &'static str,
    id: &'static str,
    color: Rgb,
    accent: Rgb,
    scale: f64,
    motion: [f64; 4],
    respawn: RespawnStyle,
    high_tier: bool,
    unstable: bool,
) -> ProceduralPickupRecipe {
    let [bob_amplitude, bob_frequency, spin_speed, pulse_frequency] = motion;
    ProceduralPickupRecipe {
        color,
        accent_color: accent,
        scale,
        ring_count: 1,
        bob_amplitude,
        bob_frequency,
        spin_speed,
        pulse_frequency,
        collect_style: CollectStyle::Collapse,
        respawn_style: respawn,
        high_tier,
        unstable,
        ..recipe(id, ProceduralPickupFamily::Health, kind)
    }
}

#[allow(clippy::too_many_arguments)]
fn armor(
    kind: &'static str,
    id: &'static str,
    color: Rgb,
    accent: Rgb,
    scale: f64,
    plate_count: u32,
    motion: [f64; 4],
    respawn: RespawnStyle,
    high_tier: bool,
) -> ProceduralPickupRecipe {
    let [bob_amplitude, bob_frequency, spin_speed, pulse_frequency] = motion;
    ProceduralPickupRecipe {
        color,
        accent_color: accent,
        scale,
        plate_count,
        bob_amplitude,
        bob_frequency,
        spin_speed,
        pulse_frequency,
        collect_style: CollectStyle::Shatter,
        respawn_style: respawn,
        high_tier,
        ..recipe(id, ProceduralPickupFamily::Armor, kind)
    }
}

/// `RECIPE_LIST`.
static RECIPE_LIST: LazyLock<Vec<ProceduralPickupRecipe>> = LazyLock::new(|| {
    use RespawnStyle::{Assemble, Materialize};
    let mut list = vec![
        // Health: glowing energy core and cross emblem, scrolling gradient.
        health(
            "small",
            "health_small",
            rgb(0.3, 0.95, 1.0),
            rgb(0.8, 1.0, 1.0),
            0.7,
            [1.8, 0.3, 12.0, 0.26],
            Materialize,
            false,
            false,
        ),
        // 25 HP: bright yellow.
        health(
            "medium",
            "health_medium",
            rgb(1.0, 0.82, 0.08),
            rgb(1.0, 0.95, 0.35),
            0.82,
            [2.2, 0.28, 12.0, 0.24],
            Assemble,
            false,
            false,
        ),
        // 50 HP: orange scrolling to yellow.
        health(
            "large",
            "health_large",
            rgb(1.0, 0.48, 0.06),
            rgb(1.0, 0.86, 0.16),
            0.95,
            [2.6, 0.26, 14.0, 0.24],
            Assemble,
            false,
            false,
        ),
        health(
            "mega",
            "health_mega",
            rgb(1.0, 0.2, 0.2),
            rgb(1.0, 0.5, 0.22),
            1.1,
            [3.0, 0.26, 16.0, 0.3],
            Assemble,
            true,
            false,
        ),
        health(
            "ultra",
            "health_ultra",
            rgb(1.0, 0.15, 0.6),
            rgb(1.0, 0.5, 0.9),
            1.15,
            [3.0, 0.26, 22.0, 0.34],
            Assemble,
            true,
            true,
        ),
        // Armour: broad plates over a backer; tier is colour and size.
        armor(
            "shard",
            "armor_shard",
            rgb(0.35, 1.0, 0.4),
            rgb(0.75, 1.0, 0.78),
            0.65,
            1,
            [1.6, 0.28, 16.0, 0.28],
            Materialize,
            false,
        ),
        armor(
            "ga",
            "armor_ga",
            rgb(0.2, 1.0, 0.3),
            rgb(0.6, 1.0, 0.7),
            0.85,
            1,
            [2.0, 0.26, 8.0, 0.26],
            Assemble,
            false,
        ),
        armor(
            "ya",
            "armor_ya",
            rgb(1.0, 0.88, 0.12),
            rgb(1.0, 1.0, 0.6),
            0.95,
            2,
            [2.2, 0.24, 8.0, 0.26],
            Assemble,
            false,
        ),
        armor(
            "ra",
            "armor_ra",
            rgb(1.0, 0.22, 0.22),
            rgb(1.0, 0.5, 0.25),
            1.1,
            4,
            [2.6, 0.24, 8.0, 0.3],
            Assemble,
            true,
        ),
    ];
    // Ammo: dark crates with coloured panels and generated symbols.
    list.extend(ammo_crates());
    // Powerup: a stable, two-sided badge. Gold flowing to hot red; the gold is
    // Warsow's quad colour (gfx/hud/icons/powerup/quad.tga).
    list.push(ProceduralPickupRecipe {
        color: rgb(1.0, 0.84, 0.05),
        accent_color: rgb(1.0, 0.25, 0.1),
        scale: 1.15,
        bob_amplitude: 3.0,
        bob_frequency: 0.24,
        spin_speed: 12.0,
        pulse_frequency: 0.4,
        high_tier: true,
        ..recipe("powerup_quad", ProceduralPickupFamily::Powerup, "quad")
    });
    list
});

/// `ammoCrates`: a shared chassis; colour and the mark tell the types
/// apart, in each weapon's identity colour.
fn ammo_crates() -> Vec<ProceduralPickupRecipe> {
    let crates: [(&'static str, &'static str, Rgb, Rgb, &str); 7] = [
        // Machinegun: brass.
        (
            "ammo_bullets",
            "bullets",
            rgb(1.0, 0.76, 0.26),
            rgb(1.0, 0.9, 0.6),
            "bulletsammo",
        ),
        // Riotgun: burnt orange.
        (
            "ammo_shells",
            "shells",
            rgb(1.0, 0.46, 0.18),
            rgb(1.0, 0.72, 0.42),
            "riotammo",
        ),
        // Grenade launcher: olive green.
        (
            "ammo_grenades",
            "grenades",
            rgb(0.55, 0.88, 0.32),
            rgb(0.78, 1.0, 0.6),
            "grenadeammo",
        ),
        // Rocket launcher: red.
        (
            "ammo_rockets",
            "rockets",
            rgb(1.0, 0.32, 0.22),
            rgb(1.0, 0.62, 0.5),
            "rocketammo",
        ),
        // Plasmagun: teal cells.
        (
            "ammo_plasmagun",
            "cells",
            rgb(0.3, 0.95, 0.78),
            rgb(0.7, 1.0, 0.92),
            "plasmaammo",
        ),
        // Lasergun: violet, its own pool.
        (
            "ammo_lasers",
            "lasers",
            rgb(0.62, 0.42, 1.0),
            rgb(0.84, 0.74, 1.0),
            "laserammo",
        ),
        // Electrobolt: electric blue.
        (
            "ammo_bolts",
            "bolts",
            rgb(0.36, 0.62, 1.0),
            rgb(0.7, 0.85, 1.0),
            "electroammo",
        ),
    ];
    crates
        .into_iter()
        .map(|(id, kind, color, accent, icon)| ProceduralPickupRecipe {
            color,
            accent_color: accent,
            scale: 0.8,
            // A crate is heavy: barely bobs, turns slowly.
            bob_amplitude: 1.4,
            bob_frequency: 0.24,
            spin_speed: 14.0,
            pulse_frequency: 0.2,
            collect_style: CollectStyle::Shatter,
            respawn_style: RespawnStyle::Materialize,
            icon_texture: Some(format!("gfx/hud/icons/ammo/{icon}")),
            ..recipe(id, ProceduralPickupFamily::Ammo, kind)
        })
        .collect()
}

/// `getProceduralPickupRecipe`.
#[must_use]
pub fn get_procedural_pickup_recipe(id: &str) -> Option<&'static ProceduralPickupRecipe> {
    RECIPE_LIST.iter().find(|r| r.id == id)
}

/// `hasProceduralPickupRecipe`.
#[must_use]
pub fn has_procedural_pickup_recipe(id: &str) -> bool {
    get_procedural_pickup_recipe(id).is_some()
}

/// `allProceduralPickupRecipes`.
#[must_use]
pub fn all_procedural_pickup_recipes() -> &'static [ProceduralPickupRecipe] {
    &RECIPE_LIST
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_has_its_recipes() {
        assert_eq!(all_procedural_pickup_recipes().len(), 17);
        let quad = get_procedural_pickup_recipe("powerup_quad").unwrap();
        assert!(quad.high_tier);
        let cells = get_procedural_pickup_recipe("ammo_plasmagun").unwrap();
        assert_eq!(cells.kind, "cells");
        assert_eq!(cells.icon_texture.as_deref(), Some("gfx/hud/icons/ammo/plasmaammo"));
        assert!(!has_procedural_pickup_recipe("weapon_rocketlauncher"));
    }
}
