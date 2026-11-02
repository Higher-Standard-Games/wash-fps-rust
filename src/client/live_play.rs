//! Playing a live match: the connection joined as a player, the local
//! player's input sent 125 times a second and predicted
//! (wash-fps `Game.ts`'s `sendInput` and `processSnapshots`, with
//! `V2NetworkClient`), everyone else drawn from [`SnapshotFeed`] as when
//! watching.
//!
//! Snapshots are stamped with their arrival on this machine's clock, as
//! [`crate::client::live_watch`] does.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use shagger::network::client::hosts::network_game_host::HostEvent;
use shagger::network::client::network_session::{NetworkSession, SessionConfig, SessionEvent, SessionRequest};
use shagger::shared::net::command::Command;
use shagger::shared::net::game_state::PlayerState;
use shagger::shared::server::simulation::types::Vec3;

use crate::client::match_state_manager::MatchStateManager;
use crate::client::prediction;
use crate::client::snapshot_feed::{DemoView, SnapshotFeed};
use crate::client::v2_network_client::{ClientInput, PredictedMovementState, V2NetworkClient};
use crate::shared::entities::definitions::weapons::weapons_table::weapon_speed_bonus;
use crate::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

/// `InputButtons` (SHAG `InputComponent.ts`).
pub mod buttons {
    pub const FORWARD: u16 = 1 << 0;
    pub const BACK: u16 = 1 << 1;
    pub const LEFT: u16 = 1 << 2;
    pub const RIGHT: u16 = 1 << 3;
    pub const JUMP: u16 = 1 << 4;
    pub const CROUCH: u16 = 1 << 5;
    pub const SPRINT: u16 = 1 << 6;
    pub const FIRE: u16 = 1 << 8;
    pub const ALT_FIRE: u16 = 1 << 9;
    pub const RELOAD: u16 = 1 << 10;
    pub const USE: u16 = 1 << 11;
}

/// `PlayerInputState`: what the keys and mouse ask for this tick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub crouch: bool,
    pub sprint: bool,
    pub fire: bool,
    pub alt_fire: bool,
    pub reload: bool,
    pub use_key: bool,
    /// Radians: yaw about +y, Babylon left-handed (forward is
    /// (sin yaw, 0, cos yaw)); pitch positive looks down.
    pub yaw: f64,
    pub pitch: f64,
    /// 1 to 9 picks a weapon; 0 keeps the one in hand.
    pub weapon_slot: u8,
}

/// One tick's command from the input (`sendInput`'s buttons and world
/// direction).
#[must_use]
pub fn command_for(input: &PlayInput, tick: u32) -> Command {
    let mut b = 0;
    for (on, bit) in [
        (input.forward, buttons::FORWARD),
        (input.backward, buttons::BACK),
        (input.left, buttons::LEFT),
        (input.right, buttons::RIGHT),
        (input.jump, buttons::JUMP),
        (input.crouch, buttons::CROUCH),
        (input.sprint, buttons::SPRINT),
        (input.fire, buttons::FIRE),
        (input.alt_fire, buttons::ALT_FIRE),
        (input.reload, buttons::RELOAD),
        (input.use_key, buttons::USE),
    ] {
        if on {
            b |= bit;
        }
    }
    let (sin, cos) = (input.yaw.sin(), input.yaw.cos());
    let (forward, right) = ([sin, cos], [cos, -sin]);
    let mut dir = [0.0f64; 2];
    let mut add = |v: [f64; 2], k: f64| {
        dir[0] += v[0] * k;
        dir[1] += v[1] * k;
    };
    if input.forward {
        add(forward, 1.0);
    }
    if input.backward {
        add(forward, -1.0);
    }
    if input.left {
        add(right, -1.0);
    }
    if input.right {
        add(right, 1.0);
    }
    // `Math.sqrt` is exact in both; the division matches V8's.
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    if len > 0.0 {
        dir = [dir[0] / len, dir[1] / len];
    }
    Command {
        direction: [dir[0], 0.0, dir[1]],
        pitch: input.pitch,
        yaw: input.yaw,
        buttons: b,
        tick,
        weapon_slot: input.weapon_slot,
    }
}

/// The simulation's step: 125 commands a second.
const COMMAND_MS: f64 = 8.0;
/// How long after a death the respawn may be asked for.
/// How long after a death the respawn may be asked for
/// (`GameEvents`' `_respawnCooldownEnd`).
const RESPAWN_COOLDOWN_MS: f64 = 3000.0;

pub struct LivePlay {
    session: NetworkSession<SnapshotEvent>,
    feed: SnapshotFeed,
    predictor: V2NetworkClient,
    started: Instant,
    pub map: String,
    /// Our player and entity ids, once joined.
    pub player_id: String,
    pub entity_id: String,
    joined: bool,
    /// The body is in the world (not dead, not yet spawned).
    pub alive: bool,
    died_at: f64,
    respawn_requested: bool,
    /// The newest snapshot's tick.
    tick: u32,
    next_command: f64,
    /// The weapon in hand, from `weapon_state`.
    pub weapon: String,
    pub closed: Option<String>,
    spawn_facing: Rc<Cell<Option<f64>>>,
}

impl LivePlay {
    /// Connects, waits up to `timeout` for the welcome, installs the map's
    /// prediction world from the packages in `data`, and asks to join.
    pub fn connect(
        url: &str,
        name: &str,
        ticket: Option<String>,
        data: &Path,
        timeout: Duration,
    ) -> Result<Self, String> {
        let session = NetworkSession::start(
            SessionConfig {
                url: url.to_owned(),
                player_name: name.to_owned(),
                admission_ticket: ticket,
            },
            WashoiEvents,
        );
        let started = Instant::now();
        while started.elapsed() < timeout {
            match session.next_event(Duration::from_millis(100)) {
                Some(SessionEvent::Host(HostEvent::ServerInfo(w))) => {
                    println!(
                        "welcome: \"{}\", map {}, {} Hz, {}/{} players",
                        w.server_name, w.map, w.tick_rate, w.current_players, w.max_players
                    );
                    let mut predictor = V2NetworkClient::new(f64::from(w.tick_rate));
                    if let Err(why) = prediction::install_map(&mut predictor, &PathBuf::from(data), &w.map) {
                        println!("predicting without collision: {why}");
                    }
                    // Face the way the map's spawn point faces whenever the
                    // server places us there (`MatchMapLoader`'s `onSpawnFacing`).
                    let spawn_facing = Rc::new(Cell::new(None));
                    let facing = Rc::clone(&spawn_facing);
                    predictor.on_spawn_facing(Box::new(move |yaw| facing.set(Some(yaw))));
                    return Ok(Self {
                        session,
                        feed: SnapshotFeed::new(),
                        predictor,
                        started: Instant::now(),
                        map: w.map.clone(),
                        player_id: String::new(),
                        entity_id: String::new(),
                        joined: false,
                        alive: false,
                        died_at: f64::NEG_INFINITY,
                        respawn_requested: false,
                        tick: 0,
                        next_command: 0.0,
                        weapon: String::new(),
                        closed: None,
                        spawn_facing,
                    });
                }
                Some(SessionEvent::Host(HostEvent::Failed(why)) | SessionEvent::Closed(why)) => {
                    return Err(format!("{url}: {why}"));
                }
                _ => {}
            }
        }
        Err(format!("{url}: no welcome in {} s", timeout.as_secs()))
    }

    fn now_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    #[must_use]
    pub fn match_state(&self) -> &MatchStateManager {
        &self.feed.match_state
    }

    #[must_use]
    pub fn roster(&self) -> &[(String, PlayerState)] {
        &self.feed.roster
    }

    /// The eye: the predicted position (smoothed), lifted by the view
    /// height and the crouch.
    #[must_use]
    pub fn eye(&self) -> Option<Vec3> {
        if !self.alive || !self.predictor.has_valid_position() {
            return None;
        }
        let p = self.predictor.visual_position();
        let lift = shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT
            + self.predictor.movement_state().crouch_view_offset;
        Some(Vec3 {
            x: p.x,
            y: p.y + lift,
            z: p.z,
        })
    }

    #[must_use]
    pub fn movement(&self) -> PredictedMovementState {
        self.predictor.movement_state()
    }

    /// The yaw to turn the view to, once, after the server placed us at a
    /// spawn point.
    pub fn take_spawn_facing(&self) -> Option<f64> {
        self.spawn_facing.take()
    }

    /// Says `text` to everyone, or the team (`sendChat`).
    pub fn say(&self, text: &str, team_only: bool) {
        self.session.send(SessionRequest::Chat {
            text: text.to_owned(),
            team_only,
        });
    }

    /// The weapon to draw in hand: none while dead or before the server
    /// says which.
    #[must_use]
    pub fn weapon_in_hand(&self) -> Option<&str> {
        (self.alive && !self.weapon.is_empty()).then_some(self.weapon.as_str())
    }

    #[must_use]
    pub fn velocity(&self) -> Vec3 {
        self.predictor.velocity()
    }

    #[must_use]
    pub fn grounded(&self) -> bool {
        self.predictor.is_grounded()
    }

    /// Takes what arrived, sends this frame's commands, and samples everyone
    /// else at now.
    pub fn advance(&mut self, input: &PlayInput, dt: f64) -> DemoView {
        for event in self.session.poll_events() {
            match event {
                SessionEvent::Host(HostEvent::Joined { player_id, entity_id }) => {
                    println!("joined as {player_id} (entity {entity_id:?})");
                    self.player_id = player_id;
                    self.entity_id = entity_id;
                    if !self.joined {
                        self.session.send(SessionRequest::JoinGame {
                            team_id: None,
                            manual_pick: false,
                            admission_ticket: None,
                        });
                        self.joined = true;
                        self.next_command = self.now_ms();
                    }
                }
                SessionEvent::Host(HostEvent::Snapshot(state)) => {
                    let now = self.now_ms();
                    self.tick = state.tick;
                    for event in &state.events {
                        if let SnapshotEvent::WeaponState {
                            entity_id,
                            current_weapon,
                            ..
                        } = event
                            && *entity_id == self.entity_id
                        {
                            self.weapon.clone_from(current_weapon);
                            self.predictor.set_speed_bonus(weapon_speed_bonus(current_weapon));
                        }
                    }
                    match prediction::server_snapshot(&state, &self.entity_id) {
                        Some(snapshot) => {
                            if !self.alive {
                                // Back from the dead (or in for the first time).
                                self.predictor.reset_for_respawn();
                                self.alive = true;
                                self.respawn_requested = false;
                            }
                            self.predictor.buffer_snapshot(snapshot, now);
                        }
                        None => {
                            if self.alive {
                                self.died_at = now;
                            }
                            self.alive = false;
                        }
                    }
                    self.feed.push(now, &state, &self.entity_id);
                }
                SessionEvent::Host(HostEvent::Failed(why)) | SessionEvent::Closed(why) => {
                    self.closed.get_or_insert(why);
                }
                _ => {}
            }
        }
        let now = self.now_ms();
        self.predictor.process_buffered_snapshots(now);
        self.predictor.decay_visual_offset(dt);
        self.send_commands(input, now);
        self.feed.sample(now)
    }

    fn send_commands(&mut self, input: &PlayInput, now: f64) {
        if !self.joined || self.closed.is_some() {
            return;
        }
        let mut input = *input;
        let frozen = self.feed.match_state.is_movement_frozen();
        if !self.alive {
            // Fire or jump asks to come back, once the cooldown is over.
            if (input.fire || input.jump) && !self.respawn_requested && now - self.died_at >= RESPAWN_COOLDOWN_MS {
                self.session.send(SessionRequest::RequestSpawn);
                self.respawn_requested = true;
            }
        }
        if frozen || !self.alive {
            input = PlayInput {
                yaw: input.yaw,
                pitch: input.pitch,
                ..PlayInput::default()
            };
        }
        // Catch up in whole steps, at most a quarter second of them.
        if now - self.next_command > 250.0 {
            self.next_command = now - 250.0;
        }
        while self.next_command <= now {
            self.next_command += COMMAND_MS;
            let command = command_for(&input, self.tick);
            if self.alive {
                // Predict what the server simulates: the f32 wire values.
                let sent = command.as_sent();
                self.predictor.process_input(
                    ClientInput {
                        direction: Vec3 {
                            x: sent.direction[0],
                            y: sent.direction[1],
                            z: sent.direction[2],
                        },
                        pitch: sent.pitch,
                        yaw: sent.yaw,
                        buttons: u32::from(sent.buttons),
                        client_tick: sent.tick,
                    },
                    now,
                );
            }
            self.session.send(SessionRequest::Command(command));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_world_direction_as_send_input_does() {
        let input = PlayInput {
            forward: true,
            right: true,
            fire: true,
            yaw: 0.0,
            ..PlayInput::default()
        };
        let c = command_for(&input, 7);
        let h = std::f64::consts::FRAC_1_SQRT_2;
        assert!((c.direction[0] - h).abs() < 1e-12 && (c.direction[2] - h).abs() < 1e-12);
        assert_eq!(c.buttons, buttons::FORWARD | buttons::RIGHT | buttons::FIRE);
        assert_eq!(c.tick, 7);
        let still = command_for(&PlayInput::default(), 0);
        assert_eq!(still.direction, [0.0, 0.0, 0.0]);
    }
}
