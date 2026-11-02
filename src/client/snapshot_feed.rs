//! The snapshots a spectator draws from, shared by demo playback and a
//! live match: the remote players' interpolation buffer as
//! wash-fps `client/RemotePlayerRenderer.ts` feeds it (`pushSnapshot`),
//! and the projectiles' own buffer as `client/GameEvents.ts` feeds it
//! (`processProjectileEntities`).

use std::collections::HashMap;

use shagger::network::client::snapshot_buffer::{EntitySnapshot, InterpolatedEntity, SnapshotBuffer};
use shagger::shared::net::game_state::{GameState, PlayerState};

use crate::client::match_state_manager::MatchStateManager;
use crate::client::remote_player_logic::{apply_roster, extract_entities};
use crate::shared::net::snapshot_events::SnapshotEvent;

/// A projectile in flight, interpolated, in Babylon axes.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProjectile {
    pub id: String,
    /// From its `projectile_spawn` event.
    pub def_id: String,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

/// What a frame shows.
#[derive(Debug, Clone, Default)]
pub struct DemoView {
    pub players: Vec<(String, InterpolatedEntity)>,
    pub projectiles: Vec<LiveProjectile>,
    /// The events with an effect to draw or a sound to play
    /// (`projectile_impact`, `weapon_fire`, `voltaic_beam`, `electro_rail`,
    /// `damage`, `death`, `weapon_state`, `pickup_notify`, `powerup_spawn`)
    /// that arrived since the last frame.
    pub effects: Vec<SnapshotEvent>,
    /// Each player's weapon in hand, from `weapon_state` events
    /// (`enemyWeapons`).
    pub weapons: HashMap<String, String>,
    /// Each player's team from the roster.
    pub teams: HashMap<String, String>,
    /// The players of every snapshot that arrived since the last frame, as
    /// sent (`processFPSMovementAudio` reads them).
    pub snapshots: Vec<HashMap<String, EntitySnapshot>>,
}

/// The two buffers and what the snapshots told them.
#[derive(Debug, Clone, Default)]
pub struct SnapshotFeed {
    buffer: SnapshotBuffer,
    /// The projectiles' own buffer.
    projectile_buffer: SnapshotBuffer,
    /// `proj_` entity → its definition, from `projectile_spawn` events.
    def_ids: HashMap<String, String>,
    /// Each player's team from the roster ("ffa" for none).
    pub teams: HashMap<String, String>,
    /// The match as its `match_state` events tell it.
    pub match_state: MatchStateManager,
    /// The newest snapshot's players (`state.players`): names, scores,
    /// deaths and pings for the HUD.
    pub roster: Vec<(String, PlayerState)>,
    /// Effect events waiting for the next frame.
    effects: Vec<SnapshotEvent>,
    weapons: HashMap<String, String>,
    snapshots: Vec<HashMap<String, EntitySnapshot>>,
}

impl SnapshotFeed {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes a snapshot that arrived (or was recorded) at `now` ms; the
    /// local player's own body is left out.
    pub fn push(&mut self, now: f64, state: &GameState<SnapshotEvent>, local_entity_id: &str) {
        apply_roster(&mut self.teams, state, local_entity_id);
        self.roster = state.players.iter().map(|(k, p)| (k.to_owned(), p.clone())).collect();
        let entities = extract_entities(state, local_entity_id);
        // The sounds hear every player, our own footsteps too
        // (`processFPSMovementAudio` reads the whole state); only the
        // drawing leaves our body out.
        self.snapshots.push(if local_entity_id.is_empty() {
            entities.clone()
        } else {
            extract_entities(state, "")
        });
        self.buffer.push(now, state.tick, entities, None);
        for event in &state.events {
            match event {
                SnapshotEvent::ProjectileSpawn { entity_id, def_id } => {
                    self.def_ids.insert(entity_id.clone(), def_id.clone());
                }
                SnapshotEvent::ProjectileImpact { .. }
                | SnapshotEvent::Beam { .. }
                | SnapshotEvent::WeaponFire { .. }
                | SnapshotEvent::Damage { .. }
                | SnapshotEvent::Death { .. }
                | SnapshotEvent::PickupNotify { .. }
                | SnapshotEvent::PowerupSpawn { .. }
                | SnapshotEvent::CombatState { .. }
                | SnapshotEvent::Chat { .. } => {
                    self.effects.push(event.clone());
                }
                SnapshotEvent::MatchState(payload) => {
                    self.match_state.process_event(payload);
                }
                SnapshotEvent::WeaponState {
                    entity_id,
                    current_weapon,
                    ..
                } => {
                    self.weapons.insert(entity_id.clone(), current_weapon.clone());
                    self.effects.push(event.clone());
                }
                _ => {}
            }
        }
        let projectiles = extract_projectiles(state);
        self.projectile_buffer.push(now, state.tick, projectiles, None);
    }

    /// Every player and projectile, sampled at `now` ms.
    pub fn sample(&mut self, now: f64) -> DemoView {
        let players = sample_all(&mut self.buffer, now);
        let projectiles = sample_all(&mut self.projectile_buffer, now)
            .into_iter()
            .filter_map(|(id, e)| {
                let def_id = self.def_ids.get(&id)?.clone();
                Some(LiveProjectile {
                    id,
                    def_id,
                    position: [e.x, e.y, e.z],
                    velocity: [e.vx, e.vy, e.vz],
                })
            })
            .collect();
        DemoView {
            players,
            projectiles,
            effects: std::mem::take(&mut self.effects),
            weapons: self.weapons.clone(),
            teams: self.teams.clone(),
            snapshots: std::mem::take(&mut self.snapshots),
        }
    }
}

/// Every entity in the newest snapshot, sampled at `now`, by id.
fn sample_all(buffer: &mut SnapshotBuffer, now: f64) -> Vec<(String, InterpolatedEntity)> {
    buffer.begin_frame(now);
    let mut ids: Vec<&str> = buffer.latest_entity_ids().into_iter().collect();
    ids.sort_unstable();
    ids.into_iter()
        .filter_map(|id| buffer.sample_in_frame(id).map(|e| (id.to_owned(), e)))
        .collect()
}

/// `processProjectileEntities`' extraction: each `proj_` entity's position
/// and velocity.
fn extract_projectiles(state: &GameState<SnapshotEvent>) -> HashMap<String, EntitySnapshot> {
    let mut out = HashMap::new();
    for (id, entity) in state.entities.iter() {
        if !id.starts_with("proj_") {
            continue;
        }
        let Some(position) = entity.position else {
            continue;
        };
        let velocity = entity.velocity.map(|v| v.linear).unwrap_or_default();
        out.insert(
            id.to_owned(),
            EntitySnapshot {
                x: position.x,
                y: position.y,
                z: position.z,
                vx: velocity.x,
                vy: velocity.y,
                vz: velocity.z,
                ..EntitySnapshot::default()
            },
        );
    }
    out
}
