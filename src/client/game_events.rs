//! wash-fps `client/GameEvents.ts`: what the snapshot events show and
//! sound like. So far the effect colours, the shots (`handleRemoteWeaponFire`,
//! `handleVoltaicBeam`, `handleElectroRail`, and our own through SHAG
//! `WeaponViewManager.spawnShotEffects`), the chain-lightning arcs, and the
//! combat and reload sounds (`FPSAudio.ts`'s preload with them).
//!
//! Not yet: another player's blade slash animation.

use shagger::rendering::glam::{DVec3, Vec3};
use shagger::shared::net::message_codec::MatchPauseState;
use shagger::weapon::hitscan_effects::{
    HitscanEffects, PelletConfig, ShotWorld, TracerConfig, server_pellet_directions,
};
use shagger::weapon::point_light_pool::PointLightPool;
use shagger::weapon::rail::rail_profiles::{RailEffectSample, RailImpactKind};

use std::collections::HashMap;

use shagger::audio::audio_cvars::apply_audio_cvars;
use shagger::audio::audio_v2_sfx::AudioV2Sfx;
use shagger::audio::music::{MusicPlayer, Playlist};
use shagger::core::cvar::with_cvars;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::network::client::snapshot_buffer::EntitySnapshot;
use std::path::Path;

use crate::client::fps_announcer::{FpsAnnouncer, Lead, MatchMoment, fps_announcer_sound_paths};
use crate::client::fps_bomb_audio::{FPS_BOMB_SOUND_PATHS, FpsBombAudio};
use crate::client::fps_combat_audio::{FpsCombatAudio, fps_combat_sound_paths};
use crate::client::fps_liquid_audio::FPS_LIQUID_SOUND_PATHS;
use crate::client::fps_movement_audio::{FPS_MOVEMENT_SOUND_PATHS, FpsMovementAudio};
use crate::client::fps_pickup_audio::{
    FPS_PICKUP_SOUND_PATHS, is_powerup_label, play_pickup_sound, play_powerup_pickup_sound, play_powerup_spawn_sound,
};
use crate::client::fps_reload_audio::FpsReloadAudio;
use crate::client::fps_voltaic_audio::FpsVoltaicAudio;
use crate::client::match_state_manager::{MatchPhase, MatchStateManager};
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

/// Where shots are drawn from: our own gun on screen, or another
/// player's in their hand.
pub trait ShotMuzzles {
    /// Our own entity, whose shots leave the gun on screen.
    fn local(&self) -> Option<&str>;
    /// `getMuzzleWorldPosition(distance)`: `distance` units out along the
    /// screen ray through our gun's muzzle; `None` without a gun in hand.
    fn view_muzzle(&self, distance: f64) -> Option<DVec3>;
    /// `getWeaponMuzzleWorldPosition`: the muzzle of another player's gun,
    /// shooting along `direction`.
    fn remote_muzzle(&self, entity_id: &str, direction: DVec3) -> Option<DVec3>;
    /// `FollowedViewWeapon.beamMuzzle`: the gun on screen when `entity_id`
    /// is the player watched through their eyes.
    fn followed_muzzle(&self, _entity_id: &str) -> Option<DVec3> {
        None
    }
}

/// No guns drawn: every shot leaves the eye.
impl ShotMuzzles for () {
    fn local(&self) -> Option<&str> {
        None
    }
    fn view_muzzle(&self, _: f64) -> Option<DVec3> {
        None
    }
    fn remote_muzzle(&self, _: &str, _: DVec3) -> Option<DVec3> {
        None
    }
}

/// `spawnShotEffects`'s start: a point on the screen ray through the
/// muzzle socket, far enough out to clear the near plane.
const LOCAL_SHOT_START_DISTANCE: f64 = 24.0;
/// `RAIL_START_DISTANCE`: how far along the screen ray through the muzzle
/// our rail starts.
const RAIL_START_DISTANCE: f64 = 32.0;

/// The hitscan side of one snapshot's events: `weapon_fire`,
/// `voltaic_beam`, `electro_rail` and `weapon_chain`. Our own shots go
/// through `spawnShotEffects` and leave the gun on screen; other
/// players' through `handleRemoteWeaponFire` and leave theirs. `weapons`
/// names each player's weapon, for the beams' colours.
pub fn play_shots(
    events: &[SnapshotEvent],
    fx: &mut EffectTargets<'_>,
    weapons: &dyn Fn(&str) -> Option<String>,
    muzzles: &dyn ShotMuzzles,
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
                if muzzles.local() == Some(entity_id.as_str()) {
                    spawn_shot_effects(fx, weapon_id, vec(origin), vec(dir), *pellet, muzzles);
                    continue;
                }
                // Live snapshots carry the beam's exact contact as voltaic_beam;
                // the fallback trace is only for packets and demos without one.
                let beam_truth = canonical_weapon_id(weapon_id) == "voltaic"
                    && events.iter().any(|other| {
                        matches!(other, SnapshotEvent::Beam { electro_rail: false, entity_id: id, .. } if id == entity_id)
                    });
                let start = muzzles.remote_muzzle(entity_id, vec(dir));
                handle_remote_weapon_fire(
                    fx,
                    entity_id,
                    weapon_id,
                    vec(origin),
                    vec(dir),
                    *pellet,
                    beam_truth,
                    start,
                );
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
                let local_hero = muzzles.local() == Some(entity_id.as_str());
                let weapon = weapons(entity_id);
                let fallback = if *electro_rail { "electrobolt" } else { "voltaic" };
                let color = weapon
                    .as_deref()
                    .and_then(weapon_effect_color)
                    .unwrap_or_else(|| weapon_effect_color(fallback).expect("a beam colour"));
                let mut sample = RailEffectSample {
                    start: vec(origin),
                    end: vec(end),
                    color,
                    impact_kind: impact_kind(*kind),
                    impact_normal: Some(vec(normal)),
                    intensity: None,
                };
                if *electro_rail {
                    // handleElectroRail: our rail leaves the gun on screen; the
                    // damage endpoint stays the server's.
                    let muzzle = if local_hero {
                        muzzles.view_muzzle(RAIL_START_DISTANCE)
                    } else {
                        muzzles.followed_muzzle(entity_id)
                    };
                    if let Some(muzzle) = muzzle {
                        sample.start = muzzle;
                    }
                    fx.hitscan.fire_rail(fx.now, &sample);
                } else if local_hero {
                    // handleVoltaicBeam: the contact for our own beam, which
                    // is drawn from the muzzle every frame.
                    fx.hitscan.update_local_lightning_truth(fx.now, &sample, vec(dir));
                } else {
                    fx.hitscan
                        .update_remote_lightning_beam(fx.now, entity_id, &sample, vec(dir));
                }
            }
            SnapshotEvent::WeaponChain { points, .. } => {
                // The chain-lightning arcs between the server's link points
                // (primary, hop 1, hop 2), for everyone, the shooter too.
                let color = weapon_effect_color("voltaic");
                for pair in points.windows(2) {
                    fx.hitscan
                        .fire_lightning_arc(fx.now, vec(&pair[0]), vec(&pair[1]), color);
                }
            }
            _ => {}
        }
    }
}

/// SHAG `WeaponViewManager.spawnShotEffects`: our own hitscan shot, along
/// the ray the server fired, drawn from the muzzle on screen. The rail and
/// the beam come from their own events; projectiles from theirs.
fn spawn_shot_effects(
    fx: &mut EffectTargets<'_>,
    weapon_id: &str,
    origin: DVec3,
    direction: DVec3,
    pellet: Option<(i32, f64)>,
    muzzles: &dyn ShotMuzzles,
) {
    let weapon_id = canonical_weapon_id(weapon_id);
    let start = muzzles.view_muzzle(LOCAL_SHOT_START_DISTANCE);
    // The muzzle flash's colour (`muzzleFlash.color`) and the weapon's range.
    match weapon_id {
        "machinegun" => {
            fx.hitscan.fire_tracer(
                fx.now,
                fx.world,
                fx.lights,
                &TracerConfig {
                    origin,
                    direction,
                    start,
                    color: Some(Vec3::new(1.0, 0.9, 0.3)),
                    max_range: Some(6000.0),
                    intensity: Some(18.0),
                },
            );
        }
        "riotgun" => {
            // `pelletCount`, and `spread` (it has no `adsSpread`).
            let pellet_count = 18;
            let spread_angle = 8.0;
            fx.hitscan.fire_pellets(
                fx.now,
                fx.world,
                fx.lights,
                &PelletConfig {
                    origin,
                    direction,
                    start,
                    color: Some(Vec3::new(1.0, 0.7, 0.2)),
                    max_range: Some(8192.0),
                    pellet_count: Some(pellet_count),
                    spread_angle: Some(spread_angle),
                    directions: pellet
                        .map(|(seed, spread)| server_pellet_directions(direction, seed, pellet_count, spread)),
                    seed: None,
                    duration: None,
                    visible_tracers: None,
                },
            );
        }
        _ => {}
    }
}

/// `handleRemoteWeaponFire`: another player's shot, drawn from `start`
/// (their gun's muzzle) when it is known.
#[allow(clippy::too_many_arguments)]
pub fn handle_remote_weapon_fire(
    fx: &mut EffectTargets<'_>,
    entity_id: &str,
    weapon_id: &str,
    origin: DVec3,
    dir: DVec3,
    pellet: Option<(i32, f64)>,
    beam_truth: bool,
    start: Option<DVec3>,
) {
    let weapon_id = canonical_weapon_id(weapon_id);
    let color = weapon_effect_color(weapon_id).unwrap_or(Vec3::new(1.0, 1.0, 0.5));
    match weapon_id {
        "machinegun" => {
            // The event's direction carries the shot's spread; the streak
            // leaves their gun.
            fx.hitscan.fire_tracer(
                fx.now,
                fx.world,
                fx.lights,
                &TracerConfig {
                    origin,
                    direction: dir,
                    start,
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
                    start,
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
    /// The Voltaic's hum, and the effect clock it last heard (ms): a clock
    /// that stands still is a paused demo, and the hum stops.
    voltaic: FpsVoltaicAudio,
    voltaic_clock: f64,
    movement: FpsMovementAudio,
    announcer: FpsAnnouncer,
    bomb: FpsBombAudio,
    /// The match phase the announcer last saw (`_lastAnnouncedPhase`).
    last_announced_phase: Option<MatchPhase>,
    /// The playlists (`AudioManager`'s music half) and the packages that
    /// hold their tracks.
    pub music: MusicPlayer,
    playlists: HashMap<String, Playlist>,
    music_packages: Pk3Loader,
    /// The cvar registry's serial when the audio cvars were last applied.
    cvar_serial: Option<u64>,
    music_phase: Option<MatchPhase>,
}

/// The game's playlists, as `data/<name>.playlist.json` beside the
/// packages (`Game.ts`, `DemoService.ts`).
pub const GAME_PLAYLIST: &str = "data/game.playlist.json";
pub const ENDGAME_PLAYLIST: &str = "data/endgame.playlist.json";
pub const MENU_PLAYLIST: &str = "data/menu.playlist.json";

impl GameAudio {
    /// A demo seek (`resetFPSMovementAudio`, `demoReloadAudio.reset()`):
    /// footsteps and reloads start again from the new position.
    pub fn discontinuity(&mut self) {
        self.movement.reset();
        self.reload.reset();
        self.voltaic.reset(&mut self.sfx);
    }

    #[must_use]
    pub fn new(sfx: AudioV2Sfx) -> Self {
        let mut audio = Self {
            sfx,
            combat: FpsCombatAudio::default(),
            reload: FpsReloadAudio::default(),
            voltaic: FpsVoltaicAudio::default(),
            voltaic_clock: f64::NEG_INFINITY,
            movement: FpsMovementAudio::default(),
            announcer: FpsAnnouncer::default(),
            bomb: FpsBombAudio::default(),
            last_announced_phase: None,
            music: MusicPlayer::new(),
            playlists: HashMap::new(),
            music_packages: Pk3Loader::new(),
            cvar_serial: None,
            music_phase: None,
        };
        // The audio cvars before the first sound, so a muted game is muted
        // from the start and music that is off never starts.
        let serial = with_cvars(|c| {
            apply_audio_cvars(c, &mut audio.sfx, &mut audio.music, &mut |_| None);
            c.serial()
        });
        audio.cvar_serial = Some(serial);
        audio
    }

    /// Reads the playlists from `data` (the packages' directory) and
    /// loads the packages in it that hold their tracks; `pk3` says which
    /// those are. Returns how many tracks can play.
    pub fn load_music(&mut self, pk3: &Pk3Loader, data: &Path) -> usize {
        let mut packages: Vec<String> = Vec::new();
        let mut playable = 0;
        for key in [GAME_PLAYLIST, ENDGAME_PLAYLIST, MENU_PLAYLIST] {
            let file = data.join(key.trim_start_matches("data/"));
            let Some(playlist) = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| Playlist::parse(&text).ok())
            else {
                continue;
            };
            for track in &playlist.tracks {
                let package = pk3
                    .package_for_file(&track.file)
                    .or_else(|| pk3.package_for_file(&format!("music/{}", track.file)));
                if let Some(package) = package {
                    playable += 1;
                    if !packages.iter().any(|p| p == package) {
                        packages.push(package.to_owned());
                    }
                }
            }
            self.playlists.insert(key.to_owned(), playlist);
        }
        for package in packages {
            if let Ok(bytes) = std::fs::read(data.join(&package)) {
                let _ = self.music_packages.add_package(&package, bytes);
            }
        }
        playable
    }

    /// `loadPlaylist`: a playlist read by `load_music`, by its path.
    pub fn load_playlist(&mut self, key: &str) {
        let Some(playlist) = self.playlists.get(key).cloned() else {
            return;
        };
        let packages = &mut self.music_packages;
        self.music
            .start_playlist(&mut self.sfx, &mut |p| packages.get_file(p), playlist, Some(key));
    }

    /// The music of a match being watched (`DemoService`'s `GAME_PLAYLIST`,
    /// and `Game.ts`'s `watchMatchPhase`: the endgame's in PostGame), and
    /// the fades and track changes. Call every frame.
    pub fn update_music(&mut self, match_state: &MatchStateManager, dt: f64) {
        let phase = match_state.phase();
        match self.music_phase {
            None => self.load_playlist(if phase == MatchPhase::PostGame {
                ENDGAME_PLAYLIST
            } else {
                GAME_PLAYLIST
            }),
            Some(old) if old != phase => {
                if phase == MatchPhase::PostGame {
                    self.load_playlist(ENDGAME_PLAYLIST);
                } else if old == MatchPhase::PostGame {
                    self.load_playlist(GAME_PLAYLIST);
                }
            }
            Some(_) => {}
        }
        self.music_phase = Some(phase);
        let packages = &mut self.music_packages;
        // The audio cvars' onChange: applied when the registry changed.
        let serial = with_cvars(|c| c.serial());
        if self.cvar_serial != Some(serial) {
            self.cvar_serial = Some(serial);
            with_cvars(|c| apply_audio_cvars(c, &mut self.sfx, &mut self.music, &mut |p| packages.get_file(p)));
        }
        self.music.update(&mut self.sfx, &mut |p| packages.get_file(p), dt);
    }

    /// `preloadFPSAudio`: the combat, movement, pickup and powerup banks,
    /// from the packages. Returns how many sounds are loaded.
    pub fn preload(&mut self, pk3: &mut Pk3Loader) -> usize {
        let combat = fps_combat_sound_paths();
        let announcer = fps_announcer_sound_paths();
        let paths = combat
            .iter()
            .map(String::as_str)
            .chain(FPS_MOVEMENT_SOUND_PATHS)
            .chain(FPS_PICKUP_SOUND_PATHS)
            .chain(FPS_BOMB_SOUND_PATHS)
            .chain(FPS_LIQUID_SOUND_PATHS)
            .chain(announcer.iter().map(String::as_str));
        self.sfx.preload(paths, |p| pk3.get_file(p))
    }

    /// The announcer's frame (`Game.ts`'s `announcer.update`, and the
    /// bomb's sounds in Search and Destroy): `playing` with the local
    /// player's `lead` when in the match, `attacking` for the side whose
    /// lines to read, `dt` in seconds, `now` in ms.
    #[allow(clippy::too_many_arguments)]
    pub fn update_announcer(
        &mut self,
        match_state: &MatchStateManager,
        playing: bool,
        lead: Option<Lead>,
        attacking: bool,
        dt: f64,
        now: f64,
    ) {
        let bomb_mode = match_state.bomb().is_some();
        let moment = MatchMoment {
            phase: match_state.phase(),
            time_remaining: match_state.time_remaining(),
            overtime: match_state.overtime(),
            rounds_called_by_mode: bomb_mode,
            playing,
            ready: match_state.is_ready(),
            lead,
            team_based: match_state.team_based(),
            pause: match_state.pause().map_or(MatchPauseState::Running, |p| p.state),
            pause_seconds_left: match_state.pause_seconds_left(),
        };
        self.announcer.update(&mut self.sfx, &moment, dt, now);
        let phase = match_state.phase();
        if bomb_mode {
            let (sfx, announcer, bomb) = (&mut self.sfx, &mut self.announcer, match_state.bomb());
            self.bomb.update(sfx, announcer, bomb, dt, attacking, now);
            if Some(phase) != self.last_announced_phase {
                self.bomb
                    .on_match_phase(sfx, announcer, phase, self.last_announced_phase, attacking, now);
            }
            self.bomb.update_round(
                sfx,
                announcer,
                phase,
                match_state.time_remaining(),
                bomb,
                attacking,
                now,
            );
        }
        self.last_announced_phase = Some(phase);
    }

    /// `processFPSMovementAudio` for each snapshot that arrived.
    pub fn play_movement_sounds(
        &mut self,
        snapshots: &[HashMap<String, EntitySnapshot>],
        view_entity_id: Option<&str>,
        voice_of: &dyn Fn(&str) -> &'static str,
        ground_flags: &mut dyn FnMut(DVec3) -> i32,
    ) {
        for players in snapshots {
            self.movement
                .process(&mut self.sfx, players, view_entity_id, voice_of, ground_flags);
        }
    }

    /// The sounds of one snapshot's events, heard from the player on
    /// screen (`view_entity_id`, none for a free camera). `voice_of` is the
    /// voice of the body a player is drawn with, `position` where an entity
    /// stands, `now` in ms.
    pub fn play_event_sounds(
        &mut self,
        events: &[SnapshotEvent],
        view_entity_id: Option<&str>,
        voice_of: &dyn Fn(&str) -> &'static str,
        position: &dyn Fn(&str) -> Option<DVec3>,
        now: f64,
    ) {
        let present = now > self.voltaic_clock;
        self.voltaic_clock = now;
        self.voltaic
            .present(&mut self.sfx, events, view_entity_id, position, now / 1000.0, present);
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
                    let voice = voice_of(entity_id);
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
                    let voice = voice_of(entity_id);
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
