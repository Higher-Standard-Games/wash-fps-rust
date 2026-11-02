//! wash-fps `src/games/fps/client/InputHistory.ts`: the player's commands
//! kept for reconciliation replay. Each keeps the state its prediction
//! reached, so a snapshot can be checked against what was predicted for
//! the same command, and a replay can resume from the state the
//! acknowledged command left.
//!
//! The browser stamps each command with `performance.now()`; here the
//! caller passes the clock.

use std::collections::BTreeMap;

use shagger::shared::bsp::lift_system::LiftState;
use shagger::shared::server::simulation::types::Vec3;

use crate::simulation::movement_profile::MovementProfileState;

/// `PredictedStep`: the predicted state right after a command ran.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictedStep {
    pub position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
    /// Knockback drag age after the step, or `None` when no drag is active.
    pub knockback_age: Option<f64>,
    pub movement: MovementProfileState,
    /// Map movers after the step, on maps that have them.
    pub lifts: Option<Vec<LiftState>>,
}

/// What a caller gives [`InputHistory::add`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewInput {
    /// World-space wish direction (pre-rotated).
    pub direction: Vec3,
    pub pitch: f64,
    pub yaw: f64,
    pub buttons: u32,
    /// Client tick when the input was generated.
    pub client_tick: u32,
}

/// `StoredInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredInput {
    pub seq: u32,
    pub direction: Vec3,
    pub pitch: f64,
    pub yaw: f64,
    pub buttons: u32,
    pub client_tick: u32,
    /// When the command was generated (ms).
    pub timestamp: f64,
    /// State after the command, from its latest prediction or replay.
    pub predicted: Option<PredictedStep>,
}

/// Two seconds at 125 Hz, beyond the client's replay budget, and a bound
/// if the server goes silent.
pub const MAX_INPUTS: usize = 256;

#[derive(Debug, Clone)]
pub struct InputHistory {
    inputs: BTreeMap<u32, StoredInput>,
    next_seq: u32,
}

impl Default for InputHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl InputHistory {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs: BTreeMap::new(),
            next_seq: 1,
        }
    }

    /// Adds a command made at `now` and returns its sequence number.
    pub fn add(&mut self, input: NewInput, now: f64) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.inputs.insert(
            seq,
            StoredInput {
                seq,
                direction: input.direction,
                pitch: input.pitch,
                yaw: input.yaw,
                buttons: input.buttons,
                client_tick: input.client_tick,
                timestamp: now,
                predicted: None,
            },
        );
        // Sequence order is insertion order, so the first key is the oldest.
        if self.inputs.len() > MAX_INPUTS {
            self.inputs.pop_first();
        }
        seq
    }

    /// Records the state a command's prediction (or replay) reached.
    pub fn set_predicted(&mut self, seq: u32, predicted: PredictedStep) {
        if let Some(input) = self.inputs.get_mut(&seq) {
            input.predicted = Some(predicted);
        }
    }

    #[must_use]
    pub fn get(&self, seq: u32) -> Option<&StoredInput> {
        self.inputs.get(&seq)
    }

    /// The commands after `acked_seq`, oldest first.
    pub fn unacked(&self, acked_seq: u32) -> impl Iterator<Item = &StoredInput> {
        self.inputs.range(acked_seq.saturating_add(1)..).map(|(_, input)| input)
    }

    /// Removes the commands older than the acknowledged one, which stays:
    /// it holds the state the next replay resumes from.
    pub fn prune_acked(&mut self, acked_seq: u32) {
        self.inputs = self.inputs.split_off(&acked_seq);
    }

    /// The sequence the next command gets.
    #[must_use]
    pub fn current_seq(&self) -> u32 {
        self.next_seq
    }

    #[must_use]
    pub fn count_unacked(&self, acked_seq: u32) -> usize {
        self.unacked(acked_seq).count()
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.inputs.len()
    }

    /// Clears all commands (on respawn, for one).
    pub fn clear(&mut self) {
        self.inputs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(tick: u32) -> NewInput {
        NewInput {
            direction: Vec3::new(1.0, 0.0, 0.0),
            pitch: 0.0,
            yaw: 0.0,
            buttons: 0,
            client_tick: tick,
        }
    }

    #[test]
    fn sequences_prune_and_bound() {
        let mut h = InputHistory::new();
        for t in 0..10 {
            assert_eq!(h.add(input(t), f64::from(t)), t + 1);
        }
        assert_eq!(h.count_unacked(4), 6);
        assert_eq!(h.unacked(8).map(|i| i.seq).collect::<Vec<_>>(), [9, 10]);
        h.prune_acked(4);
        // The acknowledged command stays.
        assert_eq!(h.count(), 7);
        assert!(h.get(4).is_some() && h.get(3).is_none());
        for t in 0..300 {
            h.add(input(t), 0.0);
        }
        assert_eq!(h.count(), MAX_INPUTS);
        assert_eq!(h.current_seq(), 311);
        h.clear();
        assert_eq!(h.count(), 0);
    }
}
