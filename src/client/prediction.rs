//! The local player's prediction as wash-fps wires it: the map's collision
//! world, trigger volumes and spawn facings installed in
//! [`V2NetworkClient`] (wash-fps `src/games/fps/client/Game.ts`'s map
//! load), and the [`ServerSnapshot`] each snapshot buffers (the "local
//! player position update" in `src/games/fps/client/GameEvents.ts`).

use std::path::Path;

use shagger::loader::bsp::bsp_loader::load_collision;
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::physics::sweep_backend_adapter::SweepBackendAdapter;
use shagger::physics::sweep_collision_backend::SweepCollisionBackend;
use shagger::shared::bsp::lift_system::{LiftPhase, LiftState};
use shagger::shared::bsp::spawn_point_table::extract_spawn_facings;
use shagger::shared::bsp::trigger_table::extract_trigger_volumes;
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::net::game_state::GameState;

use crate::client::v2_network_client::{ServerSnapshot, V2NetworkClient};
use crate::shared::net::snapshot_events::{self, SnapshotEvent};

/// Loads `maps/<map>.bsp` from the packages in `data` and installs its
/// collision, lifts, teleporters, jumppads and spawn facings.
pub fn install_map(client: &mut V2NetworkClient, data: &Path, map: &str) -> Result<(), String> {
    let mut pk3 = Pk3Loader::new();
    let count = ingest_pk3_dir(&mut pk3, data);
    let path = format!("maps/{map}.bsp");
    let bytes = pk3
        .get_file(&path)
        .ok_or_else(|| format!("{path} is in none of the {count} packages in {}", data.display()))?;
    let collision = load_collision(&bytes).map_err(|e| format!("{path}: {e}"))?;
    client.set_trigger_volumes(&extract_trigger_volumes(&collision.entities, &collision.models));
    client.set_spawn_facings(extract_spawn_facings(&collision.entities));
    client.set_collision_world(Some(SharedCollisionWorld::new(SweepBackendAdapter {
        backend: SweepCollisionBackend::new(collision),
    })));
    Ok(())
}

fn lift_phase(name: &str) -> LiftPhase {
    match name {
        "up" => LiftPhase::Up,
        "top" => LiftPhase::Top,
        "down" => LiftPhase::Down,
        _ => LiftPhase::Bottom,
    }
}

/// A `lift_state` event's lifts as the simulation holds them.
fn sim_lifts(lifts: &[snapshot_events::LiftState]) -> Vec<LiftState> {
    lifts
        .iter()
        .map(|l| LiftState {
            model_index: usize::from(l.model_index),
            offset: l.offset,
            phase: lift_phase(l.phase),
            remaining: l.remaining,
            cooldowns: l.cooldowns.clone(),
        })
        .collect()
}

/// The local player's [`ServerSnapshot`] in `state`, or `None` when the
/// player's entity, transform or velocity is absent (dead, or not yet in).
pub fn server_snapshot(state: &GameState<SnapshotEvent>, entity_id: &str) -> Option<ServerSnapshot> {
    let entity = state.entities.get(entity_id)?;
    let transform = entity.transform?;
    let velocity = entity.velocity?;
    let lifts = state.events.iter().find_map(|e| match e {
        SnapshotEvent::LiftState { lifts } => Some(sim_lifts(lifts)),
        _ => None,
    });
    let knockback_age_ticks = state.events.iter().find_map(|e| match e {
        SnapshotEvent::KnockbackState {
            entity_id: id,
            age_ticks,
        } if id == entity_id => Some(age_ticks.map(f64::from)),
        _ => None,
    });
    Some(ServerSnapshot {
        lifts,
        tick: state.tick,
        acked_input_seq: state.acked_input_seq,
        position: transform.position,
        velocity: velocity.linear,
        grounded: velocity.grounded,
        knockback_age_ticks: knockback_age_ticks.flatten(),
        received_at_ms: None,
    })
}
