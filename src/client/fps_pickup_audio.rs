//! wash-fps `client/FPSPickupAudio.ts` and `client/FPSPowerupAudio.ts`:
//! pickup feedback for the player on screen, and the quad's cues.

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions, attenuation};
use shagger::rendering::glam::DVec3;

const WEAPON: &str = "sounds/items/weapon_pickup.ogg";
const AMMO: &str = "sounds/items/ammo_pickup.ogg";
const ARMOR_SHARD: &str = "sounds/items/armor_shard.ogg";
const ARMOR_GREEN: &str = "sounds/items/armor_green.ogg";
const ARMOR_YELLOW: &str = "sounds/items/armor_yellow.ogg";
const ARMOR_RED: &str = "sounds/items/armor_red.ogg";
const HEALTH: &str = "sounds/items/health_25.ogg";
const MEGA_HEALTH: &str = "sounds/items/megahealth.ogg";

const QUAD_SPAWN: &str = "sounds/items/quad_spawn.ogg";
const QUAD_PICKUP: &str = "sounds/items/quad_pickup.ogg";
const QUAD_FIRE: &str = "sounds/items/quad_fire.ogg";

/// `FPS_PICKUP_SOUND_PATHS` and `FPS_POWERUP_SOUND_PATHS`.
pub const FPS_PICKUP_SOUND_PATHS: [&str; 11] = [
    WEAPON,
    AMMO,
    ARMOR_SHARD,
    ARMOR_GREEN,
    ARMOR_YELLOW,
    ARMOR_RED,
    HEALTH,
    MEGA_HEALTH,
    QUAD_SPAWN,
    QUAD_PICKUP,
    QUAD_FIRE,
];

/// `getPickupSound`: chosen from the `pickup_notify` label's words.
#[must_use]
pub fn get_pickup_sound(label: &str) -> &'static str {
    let l = label.to_lowercase();
    if l.contains("armor") {
        if l.contains("shard") {
            return ARMOR_SHARD;
        }
        if l.contains("yellow") {
            return ARMOR_YELLOW;
        }
        if l.contains("red") {
            return ARMOR_RED;
        }
        return ARMOR_GREEN;
    }
    if l.contains("health") {
        return if l.contains("mega") || l.contains("ultra") {
            MEGA_HEALTH
        } else {
            HEALTH
        };
    }
    if l.contains("ammo") {
        return AMMO;
    }
    WEAPON
}

/// `isPowerupLabel`.
#[must_use]
pub fn is_powerup_label(label: &str) -> bool {
    label.to_lowercase().contains("quad")
}

fn two_d(volume: f32, max_instances: usize) -> SfxPlayOptions {
    SfxPlayOptions {
        volume: Some(volume),
        max_instances: Some(max_instances),
        ..SfxPlayOptions::default()
    }
}

/// `playPickupSound`.
pub fn play_pickup_sound(audio: &mut AudioV2Sfx, label: &str) {
    if label.is_empty() {
        return;
    }
    audio.play_2d(get_pickup_sound(label), &two_d(0.75, 4));
}

/// `playPowerupPickupSound` (the quad).
pub fn play_powerup_pickup_sound(audio: &mut AudioV2Sfx) {
    audio.play_2d(QUAD_PICKUP, &two_d(0.9, 2));
}

/// `playPowerupSpawnSound`: everyone hears the quad return, at the item.
pub fn play_powerup_spawn_sound(audio: &mut AudioV2Sfx, position: DVec3) {
    audio.play_3d(
        QUAD_SPAWN,
        position,
        &SfxPlayOptions {
            volume: Some(1.0),
            attenuation: Some(attenuation::DISTANT),
            max_instances: Some(1),
            ..SfxPlayOptions::default()
        },
    );
}

/// `playPowerupFireSound`: layered over the local player's own fire.
pub fn play_powerup_fire_sound(audio: &mut AudioV2Sfx) {
    audio.play_2d(QUAD_FIRE, &two_d(0.55, 2));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_pick_the_sound() {
        assert_eq!(get_pickup_sound("Armor Shard"), ARMOR_SHARD);
        assert_eq!(get_pickup_sound("Green Armor"), ARMOR_GREEN);
        assert_eq!(get_pickup_sound("Mega Health"), MEGA_HEALTH);
        assert_eq!(get_pickup_sound("25 Health"), HEALTH);
        assert_eq!(get_pickup_sound("25 Cells Ammo"), AMMO);
        assert_eq!(get_pickup_sound("Plasma Gun"), WEAPON);
        assert!(is_powerup_label("Quad Damage"));
    }
}
