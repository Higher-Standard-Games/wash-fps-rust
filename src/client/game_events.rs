//! wash-fps `client/GameEvents.ts`: what the snapshot events show. So far
//! the effect colours and the shots of other players
//! (`handleRemoteWeaponFire`, `handleVoltaicBeam`, `handleElectroRail`).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_canonical_id() {
        assert_eq!(weapon_effect_color("lasergun"), weapon_effect_color("voltaic"));
        assert_eq!(weapon_effect_color("plasmagun"), None);
    }
}
