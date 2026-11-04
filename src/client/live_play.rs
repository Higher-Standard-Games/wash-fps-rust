//! Playing a live match: the connection joined as a player, the local
//! player's input sent 125 times a second and predicted
//! (wash-fps `Game.ts`'s `sendInput` and `processSnapshots`, with
//! `V2NetworkClient`), everyone else drawn from [`SnapshotFeed`] as when
//! watching.
//!
//! Snapshots are stamped with their arrival on this machine's clock, as
//! [`crate::client::live_watch`] does.

use crate::client::game_events::canonical_weapon_id;
use crate::liquid::client::local_air_tracker::{AirEvent, LocalAirTracker};
use shagger::shared::js::{js_cos, js_sin};
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
use shagger::shared::json::Value;

/// shared/server/gametype/Loadout.ts `DEFAULT_LOADOUT`: the kit the browser's
/// picker starts on, sent for the player until the native client has a picker.
pub const DEFAULT_LOADOUT: (&str, &str) = ("rlauncher", "machinegun");

/// The request that chooses [`DEFAULT_LOADOUT`].
#[must_use]
pub fn default_loadout() -> SessionRequest {
    SessionRequest::Loadout {
        primary: DEFAULT_LOADOUT.0.to_owned(),
        secondary: DEFAULT_LOADOUT.1.to_owned(),
    }
}

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
    /// ADS asked for by the trigger on a weapon that cannot fire from the hip.
    pub const AUTO_ADS: u16 = 1 << 12;
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
    /// The aim was put up by the trigger, not the aim key (`autoAds`).
    pub auto_ads: bool,
    /// Radians: yaw about +y, Babylon left-handed (forward is
    /// (sin yaw, 0, cos yaw)); pitch positive looks down.
    pub yaw: f64,
    pub pitch: f64,
    /// 1 to 9 picks a weapon; 0 keeps the one in hand.
    pub weapon_slot: u8,
}

/// `bomb?.disabledModels`: the brush models a detonation took out.
fn disabled_models(match_state: &MatchStateManager) -> Vec<usize> {
    match_state
        .bomb()
        .and_then(|b| b.get("disabledModels"))
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(Value::as_f64)
                .filter(|i| *i >= 0.0 && i.fract() == 0.0)
                .map(|i| i as usize)
                .collect()
        })
        .unwrap_or_default()
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
        (input.fire && input.alt_fire && input.auto_ads, buttons::AUTO_ADS),
    ] {
        if on {
            b |= bit;
        }
    }
    let (sin, cos) = (js_sin(input.yaw), js_cos(input.yaw));
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

/// `InventoryManager.getSlotCount()`: the blade and two weapons.
const WEAPON_SLOT_COUNT: u8 = 3;

/// The 0-based slot a number key (1-based `slot`) or a wheel step picks:
/// the wheel skips the blade in slot 0 and wraps (`FPSController`).
#[must_use]
pub fn weapon_pick(current: u8, count: u8, slot: u8, next: bool, prev: bool) -> Option<u8> {
    if slot > 0 {
        return Some(slot - 1);
    }
    if count <= 1 {
        return None;
    }
    let pick = if next {
        let n = current + 1;
        if n >= count { 1 } else { n }
    } else if prev {
        if current <= 1 { count - 1 } else { current - 1 }
    } else {
        return None;
    };
    Some(pick)
}

/// The simulation's step: 125 commands a second.
const COMMAND_MS: f64 = 8.0;
/// How long after a death the respawn may be asked for.
/// How long after a death the respawn may be asked for
/// (`GameEvents`' `_respawnCooldownEnd`).
const RESPAWN_COOLDOWN_MS: f64 = 3000.0;

pub struct LivePlay {
    /// The side picked in the lobby room (`shag.lobby_team`), asked for on
    /// join; `None` lets the server balance.
    team: Option<String>,
    /// The brush models last switched off in prediction.
    disabled_models: Vec<usize>,
    session: NetworkSession<SnapshotEvent>,
    feed: SnapshotFeed,
    predictor: V2NetworkClient,
    started: Instant,
    pub map: String,
    /// The game server's address.
    pub url: String,
    /// Our player and entity ids, once joined.
    pub player_id: String,
    pub entity_id: String,
    joined: bool,
    /// The game server's messages for the player, not yet shown.
    pub notices: Vec<String>,
    /// The session has the seat: false from a drop until the resumed
    /// connection's first snapshot (`gameHost.isConnected()`).
    connected: bool,
    /// The body is in the world (not dead, not yet spawned).
    pub alive: bool,
    died_at: f64,
    respawn_requested: bool,
    /// `currentWeaponSlot`: 0-based, as the last pick left it.
    current_weapon_slot: u8,
    /// `_pendingWeaponSlot`: 1-based, sent with the next command only.
    pending_weapon_slot: u8,
    /// The newest snapshot's tick.
    tick: u32,
    next_command: f64,
    /// The weapon in hand, from `weapon_state`.
    pub weapon: String,
    pub closed: Option<String>,
    spawn_facing: Rc<Cell<Option<f64>>>,
    teleport: Rc<Cell<Option<LocalTeleport>>>,
    was_jumppad_launching: bool,
    /// The local air, mirrored for the HUD and the gasp (`LocalLiquid`).
    pub air: LocalAirTracker,
    /// A respawn since the last [`LivePlay::take_local_liquid`].
    respawned: bool,
    /// Our body has been in a snapshot: a player, not a spectator.
    spawned: bool,
}

/// Our own prediction went through a teleporter (`onTeleport`): where
/// from and to, the yaw we came in at and the exit's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalTeleport {
    pub exit_yaw: f64,
    pub from: Vec3,
    pub to: Vec3,
    pub entry_yaw: f64,
}

/// `LocalLiquid.emitLocalCrossing`'s frame: the predicted body's water level
/// for its own splash, and what the air asks to be heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalLiquidFrame {
    pub level: u8,
    pub position: Vec3,
    pub vertical_speed: f64,
    pub air: Option<AirEvent>,
    /// Back from the dead: forget the body's level and refill the air.
    pub respawned: bool,
}

impl LivePlay {
    /// Connects, waits up to `timeout` for the welcome, installs the map's
    /// prediction world from the packages in `data`, and asks to join.
    pub fn connect(
        url: &str,
        name: &str,
        ticket: Option<String>,
        team: Option<String>,
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
                    // MatchMapLoader's `onTeleport`: turn to the exit and
                    // play the effect, on fresh predictions only.
                    let teleport = Rc::new(Cell::new(None));
                    let teleported = Rc::clone(&teleport);
                    predictor.on_teleport(Box::new(move |exit_yaw, from, to, entry_yaw| {
                        teleported.set(Some(LocalTeleport {
                            exit_yaw,
                            from,
                            to,
                            entry_yaw,
                        }));
                    }));
                    return Ok(Self {
                        session,
                        feed: SnapshotFeed::new(),
                        predictor,
                        started: Instant::now(),
                        map: w.map.clone(),
                        url: url.to_owned(),
                        player_id: String::new(),
                        entity_id: String::new(),
                        joined: false,
                        team,
                        disabled_models: Vec::new(),
                        notices: Vec::new(),
                        connected: true,
                        alive: false,
                        died_at: f64::NEG_INFINITY,
                        respawn_requested: false,
                        current_weapon_slot: 0,
                        pending_weapon_slot: 0,
                        tick: 0,
                        next_command: 0.0,
                        weapon: String::new(),
                        closed: None,
                        spawn_facing,
                        teleport,
                        was_jumppad_launching: false,
                        air: LocalAirTracker::default(),
                        respawned: false,
                        spawned: false,
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

    /// The feet: the predicted position with its correction smoothing;
    /// `None` while dead or before the first snapshot.
    #[must_use]
    pub fn feet(&self) -> Option<Vec3> {
        (self.alive && self.predictor.has_valid_position()).then(|| self.predictor.visual_position())
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

    /// Our last teleport since the last call, once.
    pub fn take_teleport(&self) -> Option<LocalTeleport> {
        self.teleport.take()
    }

    /// Says `text` to everyone, or the team (`sendChat`).
    pub fn say(&self, text: &str, team_only: bool) {
        self.session.send(SessionRequest::Chat {
            text: text.to_owned(),
            team_only,
        });
    }

    /// `Game.sendReady`: ready for the match to start (warmup).
    pub fn send_ready(&mut self) {
        if !self.connected {
            return;
        }
        self.session.send(SessionRequest::Ready);
        self.feed.match_state.set_ready(true);
    }

    /// `LocalLiquid.emitLocalCrossing`'s jump pad: where to play the
    /// launch, on the rising edge of the predicted launch, so it lands on
    /// the frame the kick is felt rather than a round trip later.
    pub fn take_jumppad_launch(&mut self) -> Option<Vec3> {
        let launching = self.alive && self.predictor.movement_state().jumppad_launch;
        let rising = launching && !self.was_jumppad_launching;
        self.was_jumppad_launching = launching;
        rising.then(|| self.predictor.position())
    }

    /// The local body's liquid this frame, read from prediction rather than
    /// the snapshot so the splash and the gasp land when the water is felt.
    /// The dead don't breathe.
    pub fn take_local_liquid(&mut self, dt: f64) -> LocalLiquidFrame {
        let respawned = std::mem::take(&mut self.respawned);
        if respawned {
            self.air.reset();
        }
        let level = self.predictor.movement_state().water_level;
        let air = if self.alive {
            self.air.update(level, dt.min(0.1))
        } else {
            None
        };
        LocalLiquidFrame {
            level,
            position: self.predictor.position(),
            vertical_speed: self.predictor.velocity().y,
            air,
            respawned,
        }
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
                        // A mode with kits refuses the join until one is chosen
                        // ("Choose a loadout before joining."); the others ignore it.
                        self.session.send(default_loadout());
                        self.session.send(SessionRequest::JoinGame {
                            team_id: self.team.clone(),
                            manual_pick: false,
                            admission_ticket: None,
                        });
                        self.joined = true;
                        self.next_command = self.now_ms();
                    }
                }
                SessionEvent::Resuming { .. } => self.connected = false,
                SessionEvent::Host(HostEvent::Snapshot(state)) => {
                    self.connected = true;
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
                            // `canonicalWeaponId`: an old name for a weapon draws its model.
                            let weapon = canonical_weapon_id(current_weapon);
                            self.weapon = weapon.to_owned();
                            self.predictor.set_speed_bonus(weapon_speed_bonus(weapon));
                        }
                    }
                    match prediction::server_snapshot(&state, &self.entity_id) {
                        Some(snapshot) => {
                            self.spawned = true;
                            if !self.alive {
                                // Back from the dead (or in for the first time).
                                self.predictor.reset_for_respawn();
                                self.respawned = true;
                                self.alive = true;
                                self.respawn_requested = false;
                            }
                            self.predictor.buffer_snapshot(snapshot, now);
                        }
                        None => {
                            if self.alive {
                                self.died_at = now;
                                self.respawn_requested = false;
                            }
                            self.alive = false;
                        }
                    }
                    self.feed.push(now, &state, &self.entity_id);
                }
                // `onServerMessage`: why a join was refused, a place in the
                // queue, a round to wait for; every one is for the player.
                SessionEvent::Host(HostEvent::ServerMessage(m)) if !m.message.is_empty() => {
                    self.notices.push(m.message);
                }
                SessionEvent::Host(HostEvent::Failed(why)) | SessionEvent::Closed(why) => {
                    self.closed.get_or_insert(why);
                }
                _ => {}
            }
        }
        let now = self.now_ms();
        // `MatchStateManager`: the brushes the bomb destroyed are gone from
        // prediction too, before the snapshots replay against them.
        let disabled = disabled_models(&self.feed.match_state);
        if disabled != self.disabled_models {
            self.predictor.restore_brush_models(&disabled);
            self.disabled_models = disabled;
        }
        self.predictor.process_buffered_snapshots(now);
        self.predictor.decay_visual_offset(dt);
        self.send_commands(input, now);
        self.feed.sample(now)
    }

    /// `syncKeysFromInput`'s weapon select: a number key (1-based `slot`,
    /// 0 for none) or a wheel step picks the weapon the next command asks
    /// for.
    pub fn select_weapon(&mut self, slot: u8, next: bool, prev: bool) {
        if let Some(pick) = weapon_pick(self.current_weapon_slot, WEAPON_SLOT_COUNT, slot, next, prev) {
            self.current_weapon_slot = pick;
            self.pending_weapon_slot = pick + 1;
        }
    }

    fn send_commands(&mut self, input: &PlayInput, now: f64) {
        // `sendInput`: nothing is predicted or sent while the connection is
        // being restored, so the predictor's numbering stays the wire's.
        // A spectator (queued, refused, before the first spawn) sends nothing
        // (`Game.sendInput` returns for spectators).
        if !self.joined || !self.spawned || self.closed.is_some() || !self.connected {
            if !self.connected {
                self.next_command = now;
            }
            return;
        }
        let mut input = *input;
        // `Game.ts` order: the round freeze strips first, so no fire or
        // jump during it asks a dead player back; then death strips.
        if self.feed.match_state.is_movement_frozen() {
            input = PlayInput {
                yaw: input.yaw,
                pitch: input.pitch,
                ..PlayInput::default()
            };
        }
        if !self.alive {
            // Fire or jump asks to come back, once the cooldown is over.
            if (input.fire || input.jump) && !self.respawn_requested && now - self.died_at >= RESPAWN_COOLDOWN_MS {
                self.session.send(SessionRequest::RequestSpawn);
                self.respawn_requested = true;
            }
            // The dead keep their aim, use and weapon choice (`Game.ts`
            // strips the movement and attack buttons only).
            input = PlayInput {
                yaw: input.yaw,
                pitch: input.pitch,
                use_key: input.use_key,
                weapon_slot: input.weapon_slot,
                ..PlayInput::default()
            };
        }
        // Catch up in whole steps, at most a quarter second of them.
        if now - self.next_command > 250.0 {
            self.next_command = now - 250.0;
        }
        while self.next_command <= now {
            self.next_command += COMMAND_MS;
            // A spawn or a teleporter turns the view before the next
            // command goes (`processSnapshots` runs before `sendInput`), not
            // a frame later when the camera takes it.
            if let Some(yaw) = self.spawn_facing.get() {
                input.yaw = yaw;
                input.pitch = 0.0;
            }
            if let Some(t) = self.teleport.get() {
                input.yaw = t.exit_yaw;
            }
            // Fire-and-forget: the pick goes with one command.
            input.weapon_slot = std::mem::take(&mut self.pending_weapon_slot);
            let command = command_for(&input, self.tick);
            // Every command sent is predicted, dead or alive (`sendInput`):
            // the server acks the wire's sequence, which counts them all.
            {
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
    fn the_wheel_cycles_the_weapons_past_the_blade() {
        assert_eq!(weapon_pick(0, 3, 0, true, false), Some(1));
        assert_eq!(weapon_pick(1, 3, 0, true, false), Some(2));
        assert_eq!(weapon_pick(2, 3, 0, true, false), Some(1));
        assert_eq!(weapon_pick(2, 3, 0, false, true), Some(1));
        assert_eq!(weapon_pick(1, 3, 0, false, true), Some(2));
        assert_eq!(weapon_pick(0, 3, 0, false, true), Some(2));
        assert_eq!(weapon_pick(1, 3, 3, true, false), Some(2));
        assert_eq!(weapon_pick(1, 3, 0, false, false), None);
        assert_eq!(weapon_pick(0, 1, 0, true, false), None);
    }

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

    #[test]
    fn auto_ads_goes_only_with_the_trigger_and_the_aim() {
        let aimed = PlayInput {
            fire: true,
            alt_fire: true,
            auto_ads: true,
            ..PlayInput::default()
        };
        let both = buttons::FIRE | buttons::ALT_FIRE;
        assert_eq!(command_for(&aimed, 0).buttons, both | buttons::AUTO_ADS);
        let by_hand = PlayInput {
            auto_ads: false,
            ..aimed
        };
        assert_eq!(command_for(&by_hand, 0).buttons, both);
    }

    #[test]
    fn the_bombs_destroyed_models_are_read_from_the_match_state() {
        let mut state = MatchStateManager::new();
        assert!(disabled_models(&state).is_empty());
        let event =
            shagger::shared::json::parse(r#"{"phase":2,"objective":{"kind":"bomb","disabledModels":[3,7]}}"#).unwrap();
        state.process_event(&event);
        assert_eq!(disabled_models(&state), [3, 7]);
    }
}
