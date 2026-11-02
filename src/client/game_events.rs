//! wash-fps `client/GameEvents.ts`: what the snapshot events show and
//! sound like. So far the effect colours, the shots of other players
//! (`handleRemoteWeaponFire`, `handleVoltaicBeam`, `handleElectroRail`)
//! and the combat and reload sounds (`FPSAudio.ts`'s preload with them).
//!
//! Not yet: a remote shooter's muzzle (their weapon is not drawn yet, so
//! streaks start at the eye), the blade's slash animation, and the local
//! player's own shots.

use shagger::rendering::glam::{DVec3, Vec3};
use shagger::weapon::hitscan_effects::{
    HitscanEffects, PelletConfig, ShotWorld, TracerConfig, server_pellet_directions,
};
use shagger::weapon::point_light_pool::PointLightPool;
use shagger::weapon::rail::rail_profiles::{RailEffectSample, RailImpactKind};

use std::collections::HashMap;

use shagger::audio::audio_v2_sfx::AudioV2Sfx;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::network::client::snapshot_buffer::EntitySnapshot;

use crate::client::fps_combat_audio::{FpsCombatAudio, fps_combat_sound_paths};
use crate::client::fps_movement_audio::{FPS_MOVEMENT_SOUND_PATHS, FpsMovementAudio};
use crate::client::fps_pickup_audio::{
    FPS_PICKUP_SOUND_PATHS, is_powerup_label, play_pickup_sound, play_powerup_pickup_sound, play_powerup_spawn_sound,
};
use crate::client::fps_player_audio::player_voice_for_entity;
use crate::client::fps_reload_audio::FpsReloadAudio;
use crate::shared::net::snapshot_events::{Point, SnapshotEvent};

/// `WEAPON_EFFECT_COLORS`.
#[must_use]
pub fn weapon_effect_color(weapon_id: &str) -> Option<Vec3> {
    match canonical_weapon_id(weapon_id) {
        "machinegun" => Some(Vec3::new(1.0, 0.9, 0.3)),
        "riotgun" => Some(Vec3::new(1.0, 0.6, 0.2)),
        "voltaic" => Some(Vec3::new(1.0, 0.7, 0.1)),
        "electrobolt" => Some(Vec3::new(0.3, 0.6, 1.0)),
        _ => None,
    }
}

/// `canonicalWeaponId`: `lasergun` is the Voltaic's old id.
#[must_use]
pub fn canonical_weapon_id(weapon_id: &str) -> &str {
    if weapon_id == "lasergun" { "voltaic" } else { weapon_id }
}

fn vec(p: &Point) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}

fn impact_kind(kind: u8) -> RailImpactKind {
    match kind {
        2 => RailImpactKind::Player,
        1 => RailImpactKind::World,
        _ => RailImpactKind::None,
    }
}

/// Where the effects of a snapshot's events go.
pub struct EffectTargets<'a> {
    pub now: f64,
    pub hitscan: &'a mut HitscanEffects,
    pub world: &'a mut dyn ShotWorld,
    pub lights: &'a mut PointLightPool,
}

/// The hitscan side of one snapshot's events, from the remote players'
/// point of view: `weapon_fire`, `voltaic_beam` and `electro_rail`.
/// `weapons` names each player's weapon, for the beams' colours.
pub fn play_remote_shots(
    events: &[SnapshotEvent],
    fx: &mut EffectTargets<'_>,
    weapons: &dyn Fn(&str) -> Option<String>,
) {
    for event in events {
        match event {
            SnapshotEvent::WeaponFire {
                entity_id,
                weapon_id,
                origin,
                dir,
                pellet,
            } => {
                // Live snapshots carry the beam's exact contact as voltaic_beam;
                // the fallback trace is only for packets and demos without one.
                let beam_truth = canonical_weapon_id(weapon_id) == "voltaic"
                    && events.iter().any(|other| {
                        matches!(other, SnapshotEvent::Beam { electro_rail: false, entity_id: id, .. } if id == entity_id)
                    });
                handle_remote_weapon_fire(fx, entity_id, weapon_id, vec(origin), vec(dir), *pellet, beam_truth);
            }
            SnapshotEvent::Beam {
                electro_rail,
                entity_id,
                origin,
                dir,
                end,
                normal,
                impact_kind: kind,
            } => {
                let weapon = weapons(entity_id);
                let fallback = if *electro_rail { "electrobolt" } else { "voltaic" };
                let color = weapon
                    .as_deref()
                    .and_then(weapon_effect_color)
                    .unwrap_or_else(|| weapon_effect_color(fallback).expect("a beam colour"));
                let sample = RailEffectSample {
                    start: vec(origin),
                    end: vec(end),
                    color,
                    impact_kind: impact_kind(*kind),
                    impact_normal: Some(vec(normal)),
                    intensity: None,
                };
                if *electro_rail {
                    // handleElectroRail
                    fx.hitscan.fire_rail(fx.now, &sample);
                } else {
                    // handleVoltaicBeam
                    fx.hitscan
                        .update_remote_lightning_beam(fx.now, entity_id, &sample, vec(dir));
                }
            }
            _ => {}
        }
    }
}

/// `handleRemoteWeaponFire`: another player's shot.
pub fn handle_remote_weapon_fire(
    fx: &mut EffectTargets<'_>,
    entity_id: &str,
    weapon_id: &str,
    origin: DVec3,
    dir: DVec3,
    pellet: Option<(i32, f64)>,
    beam_truth: bool,
) {
    let weapon_id = canonical_weapon_id(weapon_id);
    let color = weapon_effect_color(weapon_id).unwrap_or(Vec3::new(1.0, 1.0, 0.5));
    match weapon_id {
        "machinegun" => {
            // The event's direction carries the shot's spread.
            fx.hitscan.fire_tracer(
                fx.now,
                fx.world,
                fx.lights,
                &TracerConfig {
                    origin,
                    direction: dir,
                    start: None,
                    color: Some(color),
                    // WEAPONS.machinegun.range and muzzleFlash.intensity
                    max_range: Some(6000.0),
                    intensity: Some(18.0),
                },
            );
        }
        "riotgun" => {
            // The server's own pellets when the event carries their seed;
            // otherwise (an older demo) a cone of the right size and count.
            let pellet_count = 18;
            let spread_angle = pellet.map_or(8.0, |(_, spread)| spread);
            fx.hitscan.fire_pellets(
                fx.now,
                fx.world,
                fx.lights,
                &PelletConfig {
                    origin,
                    direction: dir,
                    start: None,
                    color: Some(color),
                    max_range: Some(8192.0),
                    pellet_count: Some(pellet_count),
                    spread_angle: Some(spread_angle),
                    directions: pellet.map(|(seed, _)| server_pellet_directions(dir, seed, pellet_count, spread_angle)),
                    seed: None,
                    duration: None,
                    visible_tracers: None,
                },
            );
        }
        "voltaic" => {
            if !beam_truth {
                fx.hitscan.update_remote_lightning_fallback(
                    fx.now,
                    fx.world,
                    entity_id,
                    origin,
                    dir,
                    Some(color),
                    Some(850.0),
                );
            }
        }
        // The rail comes from electro_rail; projectiles from their own
        // events; the blade's slash is an animation not ported yet.
        "electrobolt" | "plasmagun" | "rlauncher" | "glauncher" | "blade" => {}
        _ => {
            fx.hitscan.fire_tracer(
                fx.now,
                fx.world,
                fx.lights,
                &TracerConfig {
                    origin,
                    direction: dir,
                    start: None,
                    color: Some(color),
                    max_range: Some(5000.0),
                    intensity: None,
                },
            );
        }
    }
}

/// The sounds of the match: the mixer and the players' audio state.
pub struct GameAudio {
    pub sfx: AudioV2Sfx,
    combat: FpsCombatAudio,
    reload: FpsReloadAudio,
    movement: FpsMovementAudio,
}

impl GameAudio {
    #[must_use]
    pub fn new(sfx: AudioV2Sfx) -> Self {
        Self {
            sfx,
            combat: FpsCombatAudio::default(),
            reload: FpsReloadAudio::default(),
            movement: FpsMovementAudio::default(),
        }
    }

    /// `preloadFPSAudio`: the combat, movement, pickup and powerup banks,
    /// from the packages. Returns how many sounds are loaded.
    pub fn preload(&mut self, pk3: &mut Pk3Loader) -> usize {
        let combat = fps_combat_sound_paths();
        let paths = combat
            .iter()
            .map(String::as_str)
            .chain(FPS_MOVEMENT_SOUND_PATHS)
            .chain(FPS_PICKUP_SOUND_PATHS);
        self.sfx.preload(paths, |p| pk3.get_file(p))
    }

    /// `processFPSMovementAudio` for each snapshot that arrived.
    pub fn play_movement_sounds(
        &mut self,
        snapshots: &[HashMap<String, EntitySnapshot>],
        view_entity_id: Option<&str>,
        teams: &HashMap<String, String>,
        ground_flags: &mut dyn FnMut(DVec3) -> i32,
    ) {
        for players in snapshots {
            self.movement
                .process(&mut self.sfx, players, view_entity_id, teams, ground_flags);
        }
    }

    /// The sounds of one snapshot's events, heard from the player on
    /// screen (`view_entity_id`, none for a free camera). `teams` is the
    /// roster, `position` where an entity stands, `now` in ms.
    pub fn play_event_sounds(
        &mut self,
        events: &[SnapshotEvent],
        view_entity_id: Option<&str>,
        teams: &HashMap<String, String>,
        position: &dyn Fn(&str) -> Option<DVec3>,
        now: f64,
    ) {
        let viewed = |id: &str| Some(id) == view_entity_id;
        for event in events {
            match event {
                SnapshotEvent::WeaponState {
                    entity_id,
                    current_weapon,
                    reloading,
                    ..
                } => self.reload.observe(
                    &mut self.sfx,
                    entity_id,
                    current_weapon,
                    *reloading,
                    viewed(entity_id),
                    position(entity_id),
                ),
                SnapshotEvent::WeaponFire {
                    entity_id,
                    weapon_id,
                    origin,
                    ..
                } => self
                    .combat
                    .play_weapon_fire_sound(&mut self.sfx, weapon_id, vec(origin), viewed(entity_id)),
                SnapshotEvent::ProjectileImpact { def_id, at, .. } => {
                    self.combat.play_projectile_impact_sound(&mut self.sfx, def_id, vec(at));
                }
                SnapshotEvent::Damage {
                    entity_id,
                    attacker_id,
                    damage,
                    at,
                    pain_tier,
                    ..
                } => {
                    let voice = player_voice_for_entity(teams, entity_id, view_entity_id);
                    self.combat.play_player_pain_sound(
                        &mut self.sfx,
                        entity_id,
                        *damage,
                        *pain_tier,
                        vec(at),
                        viewed(entity_id),
                        voice,
                        now,
                    );
                    // handleOutgoingDamage: hurting yourself is not a hit.
                    if viewed(attacker_id) && attacker_id != entity_id {
                        FpsCombatAudio::play_damage_confirmation_sound(&mut self.sfx, *damage);
                    }
                }
                SnapshotEvent::PickupNotify { entity_id, label } if viewed(entity_id) => {
                    // Powerups have their own, louder confirmation.
                    if is_powerup_label(label) {
                        play_powerup_pickup_sound(&mut self.sfx);
                    } else {
                        play_pickup_sound(&mut self.sfx, label);
                    }
                }
                SnapshotEvent::PowerupSpawn { at, .. } => play_powerup_spawn_sound(&mut self.sfx, vec(at)),
                SnapshotEvent::Death {
                    entity_id,
                    killer_id,
                    at,
                    ..
                } => {
                    let victim = viewed(entity_id);
                    let voice = player_voice_for_entity(teams, entity_id, view_entity_id);
                    FpsCombatAudio::play_player_death_sound(&mut self.sfx, entity_id, vec(at), victim, voice);
                    if viewed(killer_id) && !victim {
                        FpsCombatAudio::play_kill_echo(&mut self.sfx);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_canonical_id() {
        assert_eq!(weapon_effect_color("lasergun"), weapon_effect_color("voltaic"));
        assert_eq!(weapon_effect_color("plasmagun"), None);
    }
}
