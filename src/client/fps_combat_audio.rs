//! wash-fps `client/FPSCombatAudio.ts`: weapon fire, projectile impacts,
//! hit confirmation, pain, death and the kill echo.

use std::collections::HashMap;

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions, attenuation};
use shagger::audio::mixer::SfxChannel;
use shagger::rendering::glam::DVec3;

use crate::client::fps_player_audio::{fps_player_sound_paths, player_voice_sound};
use crate::client::fps_reload_audio::RELOAD_SOUND_PATH;
use crate::client::game_events::canonical_weapon_id;

/// `weaponFireSounds`.
#[must_use]
pub fn get_weapon_fire_sounds(weapon_id: &str) -> &'static [&'static str] {
    match canonical_weapon_id(weapon_id) {
        "blade" => &[
            "sounds/weapons/blade_strike1.ogg",
            "sounds/weapons/blade_strike2.ogg",
            "sounds/weapons/blade_strike3.ogg",
        ],
        "machinegun" => &["sounds/weapons/machinegun_fire.ogg"],
        "riotgun" => &["sounds/weapons/riotgun_fire_strong.ogg"],
        "glauncher" => &["sounds/weapons/grenlaunch_strong.ogg"],
        "rlauncher" => &["sounds/weapons/rocket_fire_strong.ogg"],
        "plasmagun" => &[
            "sounds/weapons/plasmagun_strong1.ogg",
            "sounds/weapons/plasmagun_strong2.ogg",
            "sounds/weapons/plasmagun_strong3.ogg",
        ],
        "voltaic" => &["sounds/weapons/laser_strong_fire.ogg"],
        "electrobolt" => &["sounds/weapons/electrobolt_strong.ogg"],
        _ => &[],
    }
}

const GRENADE_EXPLOSIONS: &[&str] = &[
    "sounds/weapons/gren_strong_explosion1.ogg",
    "sounds/weapons/gren_strong_explosion2.ogg",
];
const PLASMA_EXPLOSION: &[&str] = &["sounds/weapons/plasmagun_strong_explosion.ogg"];
const ROCKET_EXPLOSION: &[&str] = &["sounds/weapons/rocket_strong_explosion.ogg"];

/// `getProjectileImpactSounds`: a rocket's for anything unknown.
#[must_use]
pub fn get_projectile_impact_sounds(def_id: &str) -> &'static [&'static str] {
    match def_id {
        "grenade" | "glauncher" => GRENADE_EXPLOSIONS,
        "plasma_bolt" | "plasmagun" => PLASMA_EXPLOSION,
        "blade_blast" => &[
            "sounds/weapons/bladegun_strong_hit_1.ogg",
            "sounds/weapons/bladegun_strong_hit_2.ogg",
            "sounds/weapons/bladegun_strong_hit_3.ogg",
        ],
        _ => ROCKET_EXPLOSION,
    }
}

/// `fireAttenuation`: plasma fire is quieter at range.
fn fire_attenuation(weapon_id: &str) -> f64 {
    match canonical_weapon_id(weapon_id) {
        "plasmagun" => attenuation::IDLE,
        _ => attenuation::NORMAL,
    }
}

/// `impactAttenuation`: explosions carry, small impacts fade quickly.
fn impact_attenuation(def_id: &str) -> f64 {
    match def_id {
        "plasma_bolt" | "plasmagun" => attenuation::IDLE,
        "blade_blast" => attenuation::NORMAL,
        _ => attenuation::DISTANT,
    }
}

const DAMAGE_CONFIRMATION_SOUNDS: [&str; 4] = [
    "sounds/misc/hit_3.wav",
    "sounds/misc/hit_2.wav",
    "sounds/misc/hit_1.wav",
    "sounds/misc/hit_0.wav",
];

pub const KILL_ECHO_SOUND: &str = "sounds/misc/kill.wav";
pub const EMPTY_CLIP_SOUND: &str = "sounds/weapons/weapon_noammo.ogg";
const FIRE_VOLUME: f32 = 0.9;
/// A player in pain is heard at most this often (ms).
const PAIN_INTERVAL_MS: f64 = 400.0;

/// `FPS_COMBAT_SOUND_PATHS`.
#[must_use]
pub fn fps_combat_sound_paths() -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let weapons = [
        "blade",
        "machinegun",
        "riotgun",
        "glauncher",
        "rlauncher",
        "plasmagun",
        "voltaic",
        "electrobolt",
    ];
    let impacts = ["grenade", "plasma_bolt", "blade_blast", "rocket"];
    let all = weapons
        .iter()
        .flat_map(|w| get_weapon_fire_sounds(w).iter())
        .chain(impacts.iter().flat_map(|d| get_projectile_impact_sounds(d).iter()))
        .chain(DAMAGE_CONFIRMATION_SOUNDS.iter())
        .map(|s| (*s).to_owned())
        .chain(fps_player_sound_paths())
        .chain([KILL_ECHO_SOUND, EMPTY_CLIP_SOUND, RELOAD_SOUND_PATH].map(str::to_owned));
    for path in all {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// `damageTier`: 0 for a light hit up to 3 for a heavy one.
#[must_use]
pub fn damage_tier(damage: u16) -> usize {
    match damage {
        75.. => 3,
        50.. => 2,
        25.. => 1,
        _ => 0,
    }
}

/// The module's state: the variant counter and the pain debounce.
#[derive(Debug, Clone, Default)]
pub struct FpsCombatAudio {
    variant_sequence: usize,
    last_pain_at: HashMap<String, f64>,
}

fn options(volume: f32, max_instances: usize, channel: SfxChannel) -> SfxPlayOptions {
    SfxPlayOptions {
        volume: Some(volume),
        max_instances: Some(max_instances),
        channel,
        ..SfxPlayOptions::default()
    }
}

impl FpsCombatAudio {
    /// `chooseVariant`: round robin over the variants.
    fn choose_variant(&mut self, paths: &'static [&'static str]) -> Option<&'static str> {
        if paths.is_empty() {
            return None;
        }
        let path = paths[self.variant_sequence % paths.len()];
        self.variant_sequence += 1;
        Some(path)
    }

    /// `playWeaponFireSound`.
    pub fn play_weapon_fire_sound(
        &mut self,
        audio: &mut AudioV2Sfx,
        weapon_id: &str,
        origin: DVec3,
        local_shooter: bool,
    ) {
        let Some(path) = self.choose_variant(get_weapon_fire_sounds(weapon_id)) else {
            return;
        };
        if local_shooter {
            audio.play_2d(path, &options(FIRE_VOLUME, 6, SfxChannel::Effects));
        } else {
            audio.play_3d(
                path,
                origin,
                &SfxPlayOptions {
                    volume: Some(FIRE_VOLUME),
                    attenuation: Some(fire_attenuation(weapon_id)),
                    ..SfxPlayOptions::default()
                },
            );
        }
    }

    /// `playProjectileImpactSound`.
    pub fn play_projectile_impact_sound(&mut self, audio: &mut AudioV2Sfx, def_id: &str, at: DVec3) {
        let Some(path) = self.choose_variant(get_projectile_impact_sounds(def_id)) else {
            return;
        };
        audio.play_3d(
            path,
            at,
            &SfxPlayOptions {
                volume: Some(1.0),
                attenuation: Some(impact_attenuation(def_id)),
                ..SfxPlayOptions::default()
            },
        );
    }

    /// `playDamageConfirmationSound`: only the attacker hears it.
    pub fn play_damage_confirmation_sound(audio: &mut AudioV2Sfx, damage: u16) {
        if damage == 0 {
            return;
        }
        audio.play_2d(
            DAMAGE_CONFIRMATION_SOUNDS[damage_tier(damage)],
            &options(0.72, 4, SfxChannel::Hits),
        );
    }

    /// `playPlayerPainSound` at `now` ms.
    #[allow(clippy::too_many_arguments)]
    pub fn play_player_pain_sound(
        &mut self,
        audio: &mut AudioV2Sfx,
        entity_id: &str,
        damage: u16,
        pain_tier: Option<u8>,
        at: DVec3,
        local_victim: bool,
        voice: &str,
        now: f64,
    ) {
        if damage == 0 {
            return;
        }
        if let Some(&last) = self.last_pain_at.get(entity_id)
            && now - last < PAIN_INTERVAL_MS
        {
            return;
        }
        self.last_pain_at.insert(entity_id.to_owned(), now);
        // Entity ids change across respawns; forget the long-quiet ones.
        if self.last_pain_at.len() > 64 {
            self.last_pain_at.retain(|_, &mut at| now - at < PAIN_INTERVAL_MS);
        }
        let tier = pain_tier.map_or_else(|| damage_tier(damage), |t| usize::from(t.min(3)));
        let path = player_voice_sound(["pain25", "pain50", "pain75", "pain100"][tier], voice);
        if local_victim {
            audio.play_2d(&path, &options(0.65, 2, SfxChannel::Players));
        } else {
            audio.play_3d(
                &path,
                at,
                &SfxPlayOptions {
                    volume: Some(0.72),
                    channel: SfxChannel::Players,
                    slot: Some(format!("{entity_id}:pain")),
                    ..SfxPlayOptions::default()
                },
            );
        }
    }

    /// `playPlayerDeathSound`: dying cuts the last pain short.
    pub fn play_player_death_sound(
        audio: &mut AudioV2Sfx,
        entity_id: &str,
        at: DVec3,
        local_victim: bool,
        voice: &str,
    ) {
        let path = player_voice_sound("death", voice);
        if local_victim {
            audio.play_2d(&path, &options(0.9, 1, SfxChannel::Players));
        } else {
            audio.play_3d(
                &path,
                at,
                &SfxPlayOptions {
                    volume: Some(0.95),
                    channel: SfxChannel::Players,
                    slot: Some(format!("{entity_id}:pain")),
                    ..SfxPlayOptions::default()
                },
            );
        }
    }

    /// `playKillEcho`.
    pub fn play_kill_echo(audio: &mut AudioV2Sfx) {
        audio.play_2d(KILL_ECHO_SOUND, &options(1.0, 2, SfxChannel::Hits));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bank_has_every_sound_once() {
        let paths = fps_combat_sound_paths();
        assert!(paths.contains(&"sounds/weapons/machinegun_fire.ogg".to_owned()));
        assert!(paths.contains(&"sounds/players/bobot/death.ogg".to_owned()));
        let mut sorted = paths.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), paths.len());
        assert_eq!(damage_tier(24), 0);
        assert_eq!(damage_tier(75), 3);
    }
}
