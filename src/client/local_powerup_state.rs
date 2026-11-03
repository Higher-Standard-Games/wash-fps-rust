//! Port of wash-fps `client/LocalPowerupState.ts`: the player on screen's
//! own powerup timers, for the HUD. The server sends `powerup_state` with
//! every snapshot while one runs, so each update stores a deadline and the
//! powerup reads as over once it passes; a snapshot without it ends it at
//! once. Cosmetic only. A struct on the HUD's clock (seconds) rather than
//! module state on `performance.now()`.

use std::collections::{BTreeMap, BTreeSet};

/// `deadlines`, by kind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalPowerupState {
    deadlines: BTreeMap<String, f64>,
}

impl LocalPowerupState {
    /// `setLocalPowerup`: `seconds` whole seconds left, as the server sends.
    pub fn set(&mut self, kind: &str, seconds: f64, now: f64) {
        if kind.is_empty() {
            return;
        }
        if seconds <= 0.0 {
            self.deadlines.remove(kind);
            return;
        }
        self.deadlines.insert(kind.to_owned(), now + seconds);
    }

    /// `isLocalPowerupActive`.
    pub fn is_active(&mut self, kind: &str, now: f64) -> bool {
        match self.deadlines.get(kind) {
            None => false,
            Some(&deadline) if now >= deadline => {
                self.deadlines.remove(kind);
                false
            }
            Some(_) => true,
        }
    }

    /// `localPowerupSeconds`: 0 when not held.
    #[must_use]
    pub fn seconds(&self, kind: &str, now: f64) -> f64 {
        self.deadlines.get(kind).map_or(0.0, |d| (d - now).max(0.0))
    }

    /// `activeLocalPowerups`: every one held, with seconds left, by kind.
    pub fn active(&mut self, now: f64) -> Vec<(String, f64)> {
        self.deadlines.retain(|_, d| now < *d);
        self.deadlines.iter().map(|(k, d)| (k.clone(), d - now)).collect()
    }

    /// `keepOnlyLocalPowerups`: one missing from a snapshot that arrived
    /// has ended.
    pub fn keep_only(&mut self, running: &BTreeSet<String>) {
        self.deadlines.retain(|k, _| running.contains(k));
    }

    /// `clearLocalPowerups`: on death, a map change or a disconnect.
    pub fn clear(&mut self) {
        self.deadlines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timer_runs_out_on_its_own() {
        let mut p = LocalPowerupState::default();
        p.set("quad", 30.0, 10.0);
        assert!(p.is_active("quad", 39.9));
        assert!((p.seconds("quad", 20.0) - 20.0).abs() < 1e-12);
        assert_eq!(p.active(25.0), vec![("quad".to_owned(), 15.0)]);
        assert!(!p.is_active("quad", 40.0));
        assert_eq!(p.seconds("quad", 40.0), 0.0);
        assert!(p.active(40.0).is_empty());
    }

    #[test]
    fn zero_seconds_or_a_missing_snapshot_end_it() {
        let mut p = LocalPowerupState::default();
        p.set("quad", 30.0, 0.0);
        p.set("quad", 0.0, 1.0);
        assert!(!p.is_active("quad", 1.0));
        p.set("quad", 30.0, 0.0);
        p.keep_only(&BTreeSet::new());
        assert!(p.active(1.0).is_empty());
        p.set("", 30.0, 0.0);
        assert!(p.active(0.0).is_empty());
    }
}
