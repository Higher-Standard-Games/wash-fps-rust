//! wash-fps `src/games/fps/client/V2NetworkClient.ts`: Q3-style prediction
//! and reconciliation for the local player.
//!
//! Every command is stored in [`InputHistory`] and simulated at once with
//! the server's own movement profile. A snapshot that matches what was
//! predicted for the command it acknowledges confirms the prediction;
//! otherwise the client adopts the server state, resumes the movement
//! state the acknowledged command left and replays the commands after it.
//! Ledge grabs and wall jumps suppress reconciliation for ten snapshots
//! after them, bounded by time, divergence and knockback.
//!
//! The browser reads `performance.now()`; here the caller passes the clock
//! (ms). `NetGraph` and the dev log are not ported: the corrections and
//! path delays it would plot are kept in [`DebugInfo`].

use shagger::network::systems::movement::movement_profile::{MovementInput, ViewAngles};
use shagger::physics::sweep_backend_adapter::SweepBackendAdapter;
use shagger::shared::bsp::lift_system::{LiftActor, LiftState};
use shagger::shared::bsp::spawn_point_table::{SpawnFacing, spawn_facing_near};
use shagger::shared::bsp::trigger_table::TriggerVolume;
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::js::js_hypot;
use shagger::shared::server::simulation::input_sanitizer::{MovementFields, sanitize_movement_or_zero};
use shagger::shared::server::simulation::types::Vec3;

use crate::client::input_history::{InputHistory, NewInput, PredictedStep, StoredInput};
use crate::simulation::knockback::{apply_knockback_drag, read_knockback_age};
use crate::simulation::movement_config::{AdvancedMovementResult, LedgeGrabPhase, WallJumpPhase};
use crate::simulation::movement_profile::MovementProfile;

/// The collision world prediction runs in: the map's sweep backend, with
/// its lifts.
pub type PredictionWorld = SharedCollisionWorld<SweepBackendAdapter>;

/// `PlayerState`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerState {
    pub position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
}

/// `PredictedMovementState`: movement predicted locally, for visual effects.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PredictedMovementState {
    pub is_sliding: bool,
    pub is_crouching: bool,
    pub crouch_view_offset: f64,
    pub wall_jump_state: WallJumpPhase,
    pub ledge_grab_state: LedgeGrabPhase,
    pub actual_horiz_speed: f64,
    pub slide_phase: f64,
    pub ledge_grab_phase: f64,
    /// 0 dry, 1 wading, 2 swimming, 3 under; predicted so a splash lands
    /// on the frame the player feels the water.
    pub water_level: u8,
    pub on_ladder: bool,
    /// A jumppad replaced velocity this step; held while the player
    /// overlaps the pad, so consumers edge-detect.
    pub jumppad_launch: bool,
}

/// `ServerSnapshot`: the local player's authoritative state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServerSnapshot {
    pub lifts: Option<Vec<LiftState>>,
    pub tick: u32,
    pub acked_input_seq: u32,
    pub position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
    /// Knockback drag age after this tick, or `None` when no drag is active.
    pub knockback_age_ticks: Option<f64>,
    /// When the snapshot arrived (ms); stamped on buffering when absent.
    pub received_at_ms: Option<f64>,
}

/// `getDebugInfo`'s answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DebugInfo {
    pub reconcile_count: u32,
    pub confirmed_count: u32,
    pub last_error: f64,
    pub pending_inputs: usize,
    pub suppressed_snapshots: u32,
    pub suppressed_for_ms: f64,
    pub max_suppressed_divergence: f64,
    pub last_suppression_release: Option<&'static str>,
    pub resync_count: u32,
    /// The last visible correction (what `NetGraph.recordCorrection` takes).
    pub last_correction: Option<f64>,
    /// The last snapshot's one-way timings (`NetGraph.recordPathDelays`):
    /// command to server tick, server tick to arrival, in ms.
    pub last_path_delays: Option<(f64, f64)>,
}

/// Hard cap on continuous suppression.
const MAX_SUPPRESSION_MS: f64 = 500.0;
/// Divergence past which a snapshot is a discontinuity, adopted however
/// eligible the player looks.
const MAX_SUPPRESSED_DIVERGENCE: f64 = 400.0;
/// How far a snapshot may sit from the stored prediction and still confirm
/// it: the snapshot's float32 encoding.
const CONFIRM_POSITION_EPSILON: f64 = 0.01;
const CONFIRM_VELOCITY_EPSILON: f64 = 0.01;
const CONFIRM_LIFT_EPSILON: f64 = 0.01;
/// A reconciliation that moves the view further than this is a correction on the net graph.
const VISIBLE_CORRECTION: f64 = 0.5;
/// A server snap at least this far is a placement, not a correction.
const SPAWN_SNAP_DISTANCE: f64 = 128.0;
/// Large corrections (knockback, teleport, respawn) snap the camera.
const SNAP_THRESHOLD: f64 = 50.0;
/// Bound on the visual smoothing offset per axis.
const MAX_OFFSET: f64 = 5.0;

fn read_age(ticks: Option<f64>) -> Option<f64> {
    ticks.and_then(read_knockback_age)
}

/// `confirms`: does the snapshot describe the state predicted for the
/// command it acknowledges?
fn confirms(predicted: &PredictedStep, snapshot: &ServerSnapshot) -> bool {
    if predicted.grounded != snapshot.grounded {
        return false;
    }
    if predicted.knockback_age != read_age(snapshot.knockback_age_ticks) {
        return false;
    }
    if !near(predicted.position, snapshot.position, CONFIRM_POSITION_EPSILON) {
        return false;
    }
    if !near(predicted.velocity, snapshot.velocity, CONFIRM_VELOCITY_EPSILON) {
        return false;
    }
    let Some(server) = &snapshot.lifts else {
        return true;
    };
    predicted.lifts.as_ref().is_some_and(|p| lifts_match(p, server))
}

fn near(a: Vec3, b: Vec3, epsilon: f64) -> bool {
    (a.x - b.x).abs() <= epsilon && (a.y - b.y).abs() <= epsilon && (a.z - b.z).abs() <= epsilon
}

fn lifts_match(predicted: &[LiftState], server: &[LiftState]) -> bool {
    predicted.len() == server.len()
        && predicted.iter().zip(server).all(|(a, b)| {
            a.model_index == b.model_index
                && a.phase == b.phase
                && (a.offset - b.offset).abs() <= CONFIRM_LIFT_EPSILON
                && (a.remaining - b.remaining).abs() <= CONFIRM_LIFT_EPSILON
                && a.cooldowns.len() == b.cooldowns.len()
                && a.cooldowns
                    .iter()
                    .zip(&b.cooldowns)
                    .all(|(x, y)| (x - y).abs() <= CONFIRM_LIFT_EPSILON)
        })
}

/// A fresh prediction teleported the player: exit yaw, from, to, entry yaw.
pub type TeleportCallback = Box<dyn FnMut(f64, Vec3, Vec3, f64) + Send>;
/// The server placed the player at a spawn: the spawn's yaw.
pub type SpawnFacingCallback = Box<dyn FnMut(f64) + Send>;

/// One command as the caller makes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClientInput {
    pub direction: Vec3,
    pub pitch: f64,
    pub yaw: f64,
    pub buttons: u32,
    pub client_tick: u32,
}

pub struct V2NetworkClient {
    movement: MovementProfile,
    collision: Option<PredictionWorld>,
    input_history: InputHistory,
    local_state: PlayerState,
    predicted_movement: PredictedMovementState,
    dt: f64,
    /// Most unacknowledged commands a reconciliation replays: two seconds.
    max_replay: usize,
    reconcile_count: u32,
    confirmed_count: u32,
    last_reconcile_error: f64,
    visual_offset: Vec3,
    suppression_started_at_ms: Option<f64>,
    suppressed_snapshots: u32,
    max_suppressed_divergence: f64,
    last_suppression_release: Option<&'static str>,
    resync_count: u32,
    /// Knockback drag age after the last simulated step.
    kb_age: Option<f64>,
    /// Server tick of the last adopted impulse.
    authoritative_knockback_start_tick: Option<f64>,
    grab_reconcile_skip_count: u32,
    wall_jump_reconcile_skip_count: u32,
    has_received_snapshot: bool,
    pending_snapshots: Vec<ServerSnapshot>,
    /// Newest authoritative tick already reconciled against.
    last_applied_tick: Option<u32>,
    force_teleport_on_next_snapshot: bool,
    on_teleport: Option<TeleportCallback>,
    spawn_facings: Vec<SpawnFacing>,
    on_spawn_facing: Option<SpawnFacingCallback>,
    last_correction: Option<f64>,
    last_path_delays: Option<(f64, f64)>,
}

impl V2NetworkClient {
    /// `tick_rate`: the physics tick rate, which must match the server's.
    #[must_use]
    pub fn new(tick_rate: f64) -> Self {
        Self {
            movement: MovementProfile::default(),
            collision: None,
            input_history: InputHistory::new(),
            local_state: PlayerState::default(),
            predicted_movement: PredictedMovementState::default(),
            dt: 1.0 / tick_rate,
            max_replay: (2.0 * tick_rate) as usize,
            reconcile_count: 0,
            confirmed_count: 0,
            last_reconcile_error: 0.0,
            visual_offset: Vec3::default(),
            suppression_started_at_ms: None,
            suppressed_snapshots: 0,
            max_suppressed_divergence: 0.0,
            last_suppression_release: None,
            resync_count: 0,
            kb_age: None,
            authoritative_knockback_start_tick: None,
            grab_reconcile_skip_count: 0,
            wall_jump_reconcile_skip_count: 0,
            has_received_snapshot: false,
            pending_snapshots: Vec::new(),
            last_applied_tick: None,
            force_teleport_on_next_snapshot: false,
            on_teleport: None,
            spawn_facings: Vec::new(),
            on_spawn_facing: None,
            last_correction: None,
            last_path_delays: None,
        }
    }

    // ── Setup ───────────────────────────────────────────────────────

    pub fn set_collision_world(&mut self, collision: Option<PredictionWorld>) {
        self.collision = collision;
    }

    pub fn collision_world(&self) -> Option<&PredictionWorld> {
        self.collision.as_ref()
    }

    /// `brushModels.restore(disabled)`: the brush models in `disabled` stop
    /// colliding and every other comes back, as the server's state says.
    pub fn restore_brush_models(&mut self, disabled: &[usize]) {
        if let Some(models) = self
            .collision
            .as_mut()
            .and_then(|world| world.backend.backend.brush_models.as_mut())
        {
            models.restore(disabled);
        }
    }

    /// The map's teleporters and jumppads, as the server installs them.
    pub fn set_trigger_volumes(&mut self, volumes: &[TriggerVolume]) {
        self.movement.set_trigger_volumes(volumes);
    }

    pub fn set_spawn_facings(&mut self, facings: Vec<SpawnFacing>) {
        self.spawn_facings = facings;
    }

    pub fn on_spawn_facing(&mut self, callback: SpawnFacingCallback) {
        self.on_spawn_facing = Some(callback);
    }

    pub fn on_teleport(&mut self, callback: TeleportCallback) {
        self.on_teleport = Some(callback);
    }

    /// `reset`: on spawn or respawn.
    pub fn reset(&mut self, position: Vec3) {
        self.local_state = PlayerState {
            position,
            ..PlayerState::default()
        };
        self.input_history.clear();
        self.kb_age = None;
        self.authoritative_knockback_start_tick = None;
    }

    // ── Input ───────────────────────────────────────────────────────

    /// `processInput`: stores the command, predicts it at once with the
    /// server's physics, and returns its sequence number to send.
    pub fn process_input(&mut self, input: ClientInput, now: f64) -> u32 {
        // The same clamp the server applies on ingest.
        let clean = sanitize_movement_or_zero(MovementFields {
            direction: Some([input.direction.x, input.direction.y, input.direction.z]),
            pitch: Some(input.pitch),
            yaw: Some(input.yaw),
            buttons: Some(f64::from(input.buttons)),
        });
        let direction = Vec3::new(clean.direction[0], clean.direction[1], clean.direction[2]);
        let seq = self.input_history.add(
            NewInput {
                direction,
                pitch: clean.pitch,
                yaw: clean.yaw,
                buttons: clean.buttons,
                client_tick: input.client_tick,
            },
            now,
        );
        let movement_input = movement_input(direction, clean.pitch, clean.yaw, clean.buttons, seq);
        let result = self.step(&movement_input);
        // Only a fresh prediction turns the camera; replays never do.
        if result.teleported == Some(true)
            && let Some(callback) = &mut self.on_teleport
        {
            callback(
                result.teleport_yaw.unwrap_or(0.0),
                result.teleport_from.unwrap_or(self.local_state.position),
                result.position,
                clean.yaw,
            );
        }
        self.update_predicted_movement(&result);
        let predicted = self.capture_predicted();
        self.input_history.set_predicted(seq, predicted);
        seq
    }

    /// One simulated step, fresh or replayed: lifts, then knockback drag
    /// (reading the grounded state the step starts from), then movement.
    fn step(&mut self, input: &MovementInput) -> AdvancedMovementResult {
        if let Some(world) = &mut self.collision {
            let mut actors = [LiftActor {
                position: self.local_state.position,
                velocity: self.local_state.velocity,
                id: None,
                passive: false,
            }];
            world.backend.backend.step_lifts(self.dt, &mut actors);
            self.local_state.position = actors[0].position;
            self.local_state.velocity = actors[0].velocity;
        }
        self.step_knockback();
        let collision = self
            .collision
            .as_mut()
            .map(|c| c as &mut dyn shagger::shared::collision::collision_world::CollisionWorld);
        let result = self.movement.simulate(
            self.local_state.position,
            self.local_state.velocity,
            Some(input),
            self.dt,
            collision,
        );
        self.local_state.position = result.position;
        self.local_state.velocity = result.velocity;
        self.local_state.grounded = result.grounded;
        result
    }

    // ── Reconciliation ──────────────────────────────────────────────

    /// Buffers a snapshot for the next frame boundary.
    pub fn buffer_snapshot(&mut self, mut snapshot: ServerSnapshot, now: f64) {
        snapshot.received_at_ms.get_or_insert(now);
        self.pending_snapshots.push(snapshot);
    }

    /// Applies the newest buffered snapshot by tick; once per frame, before
    /// input. Returns whether one was applied.
    pub fn process_buffered_snapshots(&mut self, now: f64) -> bool {
        let pending = std::mem::take(&mut self.pending_snapshots);
        let mut newest: Option<ServerSnapshot> = None;
        for snapshot in pending {
            if newest.as_ref().is_none_or(|n| snapshot.tick > n.tick) {
                newest = Some(snapshot);
            }
        }
        match newest {
            Some(snapshot) => self.apply_snapshot(&snapshot, now),
            None => false,
        }
    }

    fn apply_snapshot(&mut self, snapshot: &ServerSnapshot, now: f64) -> bool {
        if self.last_applied_tick.is_some_and(|t| snapshot.tick <= t) {
            return false;
        }
        self.last_applied_tick = Some(snapshot.tick);
        self.record_path_delays(snapshot);

        let is_first_snapshot = !self.has_received_snapshot;
        self.has_received_snapshot = true;

        let in_active_grab = self.predicted_movement.ledge_grab_state != LedgeGrabPhase::None;
        if in_active_grab {
            self.grab_reconcile_skip_count = 10;
        }
        let in_active_wall_jump = self.predicted_movement.wall_jump_state != WallJumpPhase::None;
        if in_active_wall_jump {
            self.wall_jump_reconcile_skip_count = 10;
        }

        let divergence = js_hypot(&[
            self.local_state.position.x - snapshot.position.x,
            self.local_state.position.y - snapshot.position.y,
            self.local_state.position.z - snapshot.position.z,
        ]);

        let mut released_by = None;
        let authoritative_age = read_age(snapshot.knockback_age_ticks);
        let knockback_start_tick = authoritative_age.map(|age| f64::from(snapshot.acked_input_seq) - age);
        let knockback_changed = knockback_start_tick != self.authoritative_knockback_start_tick;

        let acked = self
            .input_history
            .get(snapshot.acked_input_seq)
            .and_then(|i| i.predicted.clone());
        if !is_first_snapshot
            && !self.force_teleport_on_next_snapshot
            && acked.as_ref().is_some_and(|a| confirms(a, snapshot))
        {
            self.authoritative_knockback_start_tick = knockback_start_tick;
            self.input_history.prune_acked(snapshot.acked_input_seq);
            self.count_down_suppression(in_active_grab, in_active_wall_jump);
            self.confirmed_count += 1;
            self.last_reconcile_error = 0.0;
            return true;
        }

        if std::env::var_os("WASHOI_PREDICT_DEBUG").is_some() {
            match &acked {
                Some(a) => eprintln!(
                    "miss seq {} tick {}: pos {:?} vs {:?}, vel {:?} vs {:?}, grounded {} vs {}, kb {:?} vs {:?}, lifts {:?}",
                    snapshot.acked_input_seq,
                    snapshot.tick,
                    a.position,
                    snapshot.position,
                    a.velocity,
                    snapshot.velocity,
                    a.grounded,
                    snapshot.grounded,
                    a.knockback_age,
                    read_age(snapshot.knockback_age_ticks),
                    snapshot
                        .lifts
                        .as_ref()
                        .map(|l| (l.len(), a.lifts.as_ref().map(|p| lifts_match(p, l))))
                ),
                None => eprintln!("miss seq {}: no prediction", snapshot.acked_input_seq),
            }
        }
        // Movers are world state: sync them even when this player's own
        // reconciliation is skipped.
        if let (Some(lifts), Some(world)) = (&snapshot.lifts, &mut self.collision)
            && let Some(system) = &mut world.backend.backend.lifts
        {
            system.restore(lifts);
        }
        let mut skip_reconcile =
            (self.grab_reconcile_skip_count > 0 || self.wall_jump_reconcile_skip_count > 0) && !is_first_snapshot;
        if skip_reconcile {
            let started = *self.suppression_started_at_ms.get_or_insert(now);
            self.max_suppressed_divergence = self.max_suppressed_divergence.max(divergence);
            let held_ms = now - started;
            let release = if knockback_changed {
                Some("knockback")
            } else if divergence > MAX_SUPPRESSED_DIVERGENCE {
                Some("divergence")
            } else if held_ms > MAX_SUPPRESSION_MS {
                Some("timeout")
            } else {
                None
            };
            if let Some(release) = release {
                eprintln!(
                    "[V2Client] Reconciliation suppression released ({release}): held={held_ms:.0}ms snapshots={} divergence={divergence:.1}",
                    self.suppressed_snapshots
                );
                released_by = Some(release);
                self.grab_reconcile_skip_count = 0;
                self.wall_jump_reconcile_skip_count = 0;
                skip_reconcile = false;
            }
        }

        if skip_reconcile {
            self.suppressed_snapshots += 1;
            self.input_history.prune_acked(snapshot.acked_input_seq);
            self.count_down_suppression(in_active_grab, in_active_wall_jump);
            return true;
        }

        if self.suppression_started_at_ms.is_some() {
            self.last_suppression_release = Some(released_by.unwrap_or("eligibility-ended"));
            self.suppression_started_at_ms = None;
            self.suppressed_snapshots = 0;
            self.max_suppressed_divergence = 0.0;
        }

        let unacked: Vec<StoredInput> = self.input_history.unacked(snapshot.acked_input_seq).cloned().collect();
        let pre_error = divergence;
        let pre_grounded = self.local_state.grounded;
        let pre_reconcile_pos = self.local_state.position;

        // Reset to the server's state.
        self.local_state = PlayerState {
            position: snapshot.position,
            velocity: snapshot.velocity,
            grounded: snapshot.grounded,
        };
        self.kb_age = authoritative_age;
        self.authoritative_knockback_start_tick = knockback_start_tick;

        // Resume from the movement state the acknowledged command left.
        match &acked {
            Some(acked) => {
                self.movement.load_state(&acked.movement);
                self.movement.set_grounded(snapshot.grounded);
            }
            None => self.movement.reset_physics_state(snapshot.grounded),
        }

        // Past the replay budget, resynchronise: adopt authority and drop
        // the commands that can no longer be honoured.
        let mut inputs_to_replay = unacked.as_slice();
        if unacked.len() > self.max_replay {
            self.resync_count += 1;
            inputs_to_replay = &[];
            self.input_history.clear();
            eprintln!(
                "[V2Client] Resynchronising: {} unacked inputs exceeds the {} replay budget. Adopting authority at tick {} (acked seq {}) and discarding them.",
                unacked.len(),
                self.max_replay,
                snapshot.tick,
                snapshot.acked_input_seq
            );
        }

        for input in inputs_to_replay {
            let movement_input = movement_input(input.direction, input.pitch, input.yaw, input.buttons, input.seq);
            let result = self.step(&movement_input);
            self.update_predicted_movement(&result);
            let predicted = self.capture_predicted();
            self.input_history.set_predicted(input.seq, predicted);
        }

        // With nothing replayed and a small error, keep the client's own
        // grounded flag (the server's 60 Hz flag flickers standing still).
        if inputs_to_replay.is_empty() && pre_error < 1.0 && authoritative_age.is_none() && !knockback_changed {
            self.local_state.grounded = pre_grounded;
            self.movement.reset_physics_state(pre_grounded);
        }

        self.input_history.prune_acked(snapshot.acked_input_seq);

        // Keep the camera where it was and let the offset decay.
        let mut snapped_far = false;
        if !is_first_snapshot {
            let post = self.local_state.position;
            let corr_x = pre_reconcile_pos.x + self.visual_offset.x - post.x;
            let corr_y = pre_reconcile_pos.y + self.visual_offset.y - post.y;
            let corr_z = pre_reconcile_pos.z + self.visual_offset.z - post.z;
            let corr_mag = (corr_x * corr_x + corr_y * corr_y + corr_z * corr_z).sqrt();
            if corr_mag > VISIBLE_CORRECTION && !self.force_teleport_on_next_snapshot {
                self.last_correction = Some(corr_mag);
            }
            let in_ledge_grab = self.predicted_movement.ledge_grab_state != LedgeGrabPhase::None;
            if corr_mag > SNAP_THRESHOLD || in_ledge_grab || self.force_teleport_on_next_snapshot {
                self.visual_offset = Vec3::default();
                snapped_far = corr_mag > SPAWN_SNAP_DISTANCE && !in_ledge_grab;
                self.force_teleport_on_next_snapshot = false;
            } else {
                self.visual_offset.x = corr_x;
                self.visual_offset.z = corr_z;
                // No vertical correction during crouch slides.
                if !self.predicted_movement.is_sliding {
                    self.visual_offset.y = corr_y;
                }
                self.visual_offset.x = self.visual_offset.x.clamp(-MAX_OFFSET, MAX_OFFSET);
                self.visual_offset.y = self.visual_offset.y.clamp(-MAX_OFFSET, MAX_OFFSET);
                self.visual_offset.z = self.visual_offset.z.clamp(-MAX_OFFSET, MAX_OFFSET);
            }
        }

        // Placed at a spawn: face the way the spawn point does.
        if (is_first_snapshot || snapped_far)
            && let Some(callback) = &mut self.on_spawn_facing
            && let Some(yaw) = spawn_facing_near(&self.spawn_facings, self.local_state.position)
        {
            callback(yaw);
        }

        self.reconcile_count += 1;
        self.last_reconcile_error = pre_error;
        true
    }

    fn count_down_suppression(&mut self, in_active_grab: bool, in_active_wall_jump: bool) {
        if !in_active_grab && self.grab_reconcile_skip_count > 0 {
            self.grab_reconcile_skip_count -= 1;
        }
        if !in_active_wall_jump && self.wall_jump_reconcile_skip_count > 0 {
            self.wall_jump_reconcile_skip_count -= 1;
        }
    }

    // ── Getters ─────────────────────────────────────────────────────

    /// The next snapshot snaps the camera (respawn).
    pub fn force_teleport(&mut self) {
        self.force_teleport_on_next_snapshot = true;
        self.visual_offset = Vec3::default();
    }

    /// `resetForRespawn`: clears everything from the death period, so the
    /// first snapshot after it takes the first-snapshot path.
    pub fn reset_for_respawn(&mut self) {
        self.input_history.clear();
        self.grab_reconcile_skip_count = 0;
        self.wall_jump_reconcile_skip_count = 0;
        self.suppression_started_at_ms = None;
        self.suppressed_snapshots = 0;
        self.max_suppressed_divergence = 0.0;
        self.has_received_snapshot = false;
        self.force_teleport_on_next_snapshot = true;
        self.visual_offset = Vec3::default();
        self.kb_age = None;
        self.authoritative_knockback_start_tick = None;
        self.predicted_movement = PredictedMovementState::default();
    }

    pub fn position(&self) -> Vec3 {
        self.local_state.position
    }

    /// The position plus the decaying correction offset, for the camera.
    pub fn visual_position(&self) -> Vec3 {
        let p = self.local_state.position;
        let o = self.visual_offset;
        Vec3::new(p.x + o.x, p.y + o.y, p.z + o.z)
    }

    /// Decays the smoothing offset (about 50 ms), snapping it to zero once
    /// it is below perception.
    pub fn decay_visual_offset(&mut self, dt: f64) {
        let factor = (dt * 20.0).min(1.0);
        self.visual_offset.x *= 1.0 - factor;
        self.visual_offset.y *= 1.0 - factor;
        self.visual_offset.z *= 1.0 - factor;
        let o = self.visual_offset;
        if o.x.abs() + o.y.abs() + o.z.abs() < 0.05 {
            self.visual_offset = Vec3::default();
        }
    }

    pub fn velocity(&self) -> Vec3 {
        self.local_state.velocity
    }

    pub fn is_grounded(&self) -> bool {
        self.local_state.grounded
    }

    pub fn state(&self) -> PlayerState {
        self.local_state
    }

    /// Whether a snapshot has arrived; the position is the origin until then.
    pub fn has_valid_position(&self) -> bool {
        self.has_received_snapshot
    }

    pub fn movement_state(&self) -> PredictedMovementState {
        self.predicted_movement
    }

    pub fn debug_info(&self, now: f64) -> DebugInfo {
        DebugInfo {
            reconcile_count: self.reconcile_count,
            confirmed_count: self.confirmed_count,
            last_error: self.last_reconcile_error,
            pending_inputs: self.input_history.count(),
            suppressed_snapshots: self.suppressed_snapshots,
            suppressed_for_ms: self.suppression_started_at_ms.map_or(0.0, |s| now - s),
            max_suppressed_divergence: self.max_suppressed_divergence,
            last_suppression_release: self.last_suppression_release,
            resync_count: self.resync_count,
            last_correction: self.last_correction,
            last_path_delays: self.last_path_delays,
        }
    }

    /// The weapon's speed bonus, as the server applies it per entity.
    pub fn set_speed_bonus(&mut self, bonus: f64) {
        self.movement.speed_bonus = bonus;
    }

    /// Command-to-tick and tick-to-arrival timings, each carrying the
    /// unknown clock offset.
    fn record_path_delays(&mut self, snapshot: &ServerSnapshot) {
        let Some(sent) = self.input_history.get(snapshot.acked_input_seq) else {
            return;
        };
        let Some(received) = snapshot.received_at_ms else {
            return;
        };
        let server_ms = f64::from(snapshot.tick) * self.dt * 1000.0;
        self.last_path_delays = Some((server_ms - sent.timestamp, received - server_ms));
    }

    fn capture_predicted(&self) -> PredictedStep {
        let lifts = self
            .collision
            .as_ref()
            .and_then(|c| c.backend.backend.lifts.as_ref())
            .filter(|l| !l.definitions.is_empty())
            .map(|l| l.snapshot());
        PredictedStep {
            position: self.local_state.position,
            velocity: self.local_state.velocity,
            grounded: self.local_state.grounded,
            knockback_age: self.kb_age,
            movement: self.movement.save_state(),
            lifts,
        }
    }

    /// Advances knockback drag by one step, before that step's movement.
    fn step_knockback(&mut self) {
        let Some(age) = self.kb_age else { return };
        self.kb_age = apply_knockback_drag(
            &mut self.local_state.velocity,
            self.local_state.grounded,
            age + 1.0,
            self.dt,
        );
    }

    fn update_predicted_movement(&mut self, result: &AdvancedMovementResult) {
        self.predicted_movement = PredictedMovementState {
            is_sliding: result.is_sliding,
            is_crouching: result.is_crouching,
            crouch_view_offset: result.crouch_view_offset,
            wall_jump_state: result.wall_jump_state,
            ledge_grab_state: result.ledge_grab_state,
            actual_horiz_speed: result.actual_horiz_speed,
            slide_phase: result.slide_phase,
            ledge_grab_phase: result.ledge_grab_phase,
            water_level: result.water_level.unwrap_or(0),
            on_ladder: result.on_ladder.unwrap_or(false),
            jumppad_launch: result.jumppad_launch.unwrap_or(false),
        };
    }
}

/// `toMovementInput`: one command is one client step, and a replay
/// re-simulates the same command, so the sequence is also the step index.
fn movement_input(direction: Vec3, pitch: f64, yaw: f64, buttons: u32, seq: u32) -> MovementInput {
    MovementInput {
        direction,
        view_angles: ViewAngles { pitch, yaw },
        buttons,
        sequence: Some(f64::from(seq)),
        sim_step: Some(f64::from(seq)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORWARD: u32 = 1;

    fn input(tick: u32) -> ClientInput {
        ClientInput {
            direction: Vec3::new(0.0, 0.0, 1.0),
            pitch: 0.0,
            yaw: 0.0,
            buttons: FORWARD,
            client_tick: tick,
        }
    }

    /// Without collision the player falls; a snapshot that echoes the
    /// prediction confirms it, and one that disagrees is replayed from.
    #[test]
    fn confirms_and_reconciles() {
        let mut server = V2NetworkClient::new(125.0);
        let mut client = V2NetworkClient::new(125.0);
        client.buffer_snapshot(
            ServerSnapshot {
                tick: 1,
                grounded: false,
                ..Default::default()
            },
            0.0,
        );
        assert!(client.process_buffered_snapshots(0.0));
        assert!(client.has_valid_position());
        for t in 0..10 {
            client.process_input(input(t), f64::from(t) * 8.0);
            server.process_input(input(t), f64::from(t) * 8.0);
        }
        // The server ran the same five commands.
        let mut replica = V2NetworkClient::new(125.0);
        for t in 0..5 {
            replica.process_input(input(t), 0.0);
        }
        let state = replica.state();
        let snapshot = ServerSnapshot {
            tick: 2,
            acked_input_seq: 5,
            position: state.position,
            velocity: state.velocity,
            grounded: state.grounded,
            ..Default::default()
        };
        client.buffer_snapshot(snapshot.clone(), 80.0);
        assert!(client.process_buffered_snapshots(80.0));
        assert_eq!(client.debug_info(80.0).confirmed_count, 1);
        assert_eq!(client.position(), server.position());

        // A stale tick is ignored.
        client.buffer_snapshot(snapshot.clone(), 90.0);
        assert!(!client.process_buffered_snapshots(90.0));

        // An authoritative nudge: replay the five newer commands from it.
        let nudged = ServerSnapshot {
            tick: 3,
            position: Vec3::new(state.position.x + 2.0, state.position.y, state.position.z),
            ..snapshot
        };
        client.buffer_snapshot(nudged, 100.0);
        assert!(client.process_buffered_snapshots(100.0));
        let info = client.debug_info(100.0);
        assert_eq!(info.reconcile_count, 2);
        assert!((client.position().x - (server.position().x + 2.0)).abs() < 1e-9);
        assert_eq!(client.position().z, server.position().z);
        // The camera holds where it was, then decays.
        assert!((client.visual_position().x - server.position().x).abs() < 1e-9);
        client.decay_visual_offset(1.0);
        assert_eq!(client.visual_position(), client.position());
    }

    #[test]
    fn resynchronises_past_the_replay_budget() {
        let mut client = V2NetworkClient::new(125.0);
        client.buffer_snapshot(
            ServerSnapshot {
                tick: 1,
                ..Default::default()
            },
            0.0,
        );
        client.process_buffered_snapshots(0.0);
        for t in 0..300 {
            client.process_input(input(t), 0.0);
        }
        client.buffer_snapshot(
            ServerSnapshot {
                tick: 2,
                acked_input_seq: 1,
                position: Vec3::new(500.0, 0.0, 0.0),
                ..Default::default()
            },
            10.0,
        );
        client.process_buffered_snapshots(10.0);
        assert_eq!(client.debug_info(10.0).resync_count, 1);
        assert_eq!(client.position(), Vec3::new(500.0, 0.0, 0.0));
    }
}
