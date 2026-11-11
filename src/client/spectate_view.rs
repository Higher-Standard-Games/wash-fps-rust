//! Who the camera watches while we are dead in a round (wash-fps
//! `client/SpectateView.ts`'s teammate hand-off and round-end camera).
//!
//! Search & Destroy has no respawn, so a dead player would spend the rest
//! of the round on their corpse. After [`SPECTATE_AFTER_DEATH`] the view
//! goes behind the eyes of the first living teammate and stays with them
//! until they die; at the round's end it goes to what decided the round
//! (the server's `roundResult.focus`). This module decides; the window
//! moves the cameras (the follow path for a teammate, the death camera for
//! the corpse and the focus).
//!
//! Not ported: `cycleTeammateSpectate` (nothing in the browser calls it)
//! and the chase cam of a true spectator, which the native watch mode does
//! with its own follow keys.

use shagger::rendering::glam::DVec3;
use shagger::shared::json::Value;

use crate::client::hud_state::client_key_display_id;
use crate::client::match_state_manager::{MatchPhase, MatchStateManager};

/// `SPECTATE_AFTER_DEATH_MS`: the corpse gets this long first.
pub const SPECTATE_AFTER_DEATH: f64 = 3.0;

/// What the death camera has to do this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeathCameraCue {
    /// Leave it as it is.
    Keep,
    /// Start it on the round's focus point.
    Focus(DVec3),
    /// Stop it: a teammate's eyes take the view, or we are alive.
    Stop,
    /// Start it where the view is: the one watched is gone and nobody on
    /// our side is left.
    Resume,
}

/// `SpectateView`'s teammate and round-end state.
#[derive(Debug, Default)]
pub struct SpectateView {
    /// `_spectatingEntityId`: the teammate watched while dead.
    watching: Option<String>,
    /// Seconds since we died (`_deathAtMs`), zero while alive.
    dead_for: f64,
    /// `_roundEndCameraActive`.
    round_end_camera: bool,
}

/// `roundResult.focus`.
fn focus_of(result: &Value) -> Option<DVec3> {
    let focus = result.get("focus")?;
    let axis = |k: &str| focus.get(k).and_then(Value::as_f64);
    Some(DVec3::new(axis("x")?, axis("y")?, axis("z")?))
}

impl SpectateView {
    /// The teammate whose eyes the view is behind, if any.
    #[must_use]
    pub fn watching(&self) -> Option<&str> {
        self.watching.as_deref()
    }

    /// `reset`: a new connection starts as a fresh player.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `updateRoundEndCamera` and `updateTeammateSpectate`, once a frame.
    /// `alive` lists the players drawn this frame (the dead are not), in
    /// the order they are drawn.
    pub fn update<'a>(
        &mut self,
        dt: f64,
        dead: bool,
        local_id: &str,
        m: &MatchStateManager,
        alive: impl Iterator<Item = &'a str> + Clone,
    ) -> DeathCameraCue {
        // `trackDeathEdge` and `trackLifeState`.
        self.dead_for = if dead { self.dead_for + dt } else { 0.0 };
        if !dead {
            self.watching = None;
            self.round_end_camera = false;
        }
        // The intermission wins for the dead: they watch what decided the
        // round; a living player stays where they are.
        let focus = (dead && m.phase() == MatchPhase::RoundEnd)
            .then(|| m.round_result().and_then(focus_of))
            .flatten();
        if let Some(focus) = focus {
            if self.round_end_camera {
                return DeathCameraCue::Keep;
            }
            self.round_end_camera = true;
            self.watching = None;
            return DeathCameraCue::Focus(focus);
        }
        if self.round_end_camera {
            self.round_end_camera = false;
            self.watching = None;
            if !dead {
                return DeathCameraCue::Stop;
            }
        }
        if !(dead && m.round() > 0 && m.phase() == MatchPhase::Active) {
            let was = self.watching.take();
            // Released on every living frame, not only when the teammate
            // view is dropped (a round's end revives us while dead).
            return if !dead {
                DeathCameraCue::Stop
            } else if was.is_some() {
                DeathCameraCue::Resume
            } else {
                DeathCameraCue::Keep
            };
        }
        if self.dead_for < SPECTATE_AFTER_DEATH {
            return DeathCameraCue::Keep;
        }
        // Still following someone alive? Leave it be.
        if let Some(current) = &self.watching
            && alive.clone().any(|id| id == current)
        {
            return DeathCameraCue::Keep;
        }
        let teams = m.player_teams();
        let Some(mine) = teams.get(client_key_display_id(local_id)) else {
            return DeathCameraCue::Keep;
        };
        let chosen = alive
            .filter(|id| *id != local_id)
            .find(|id| teams.get(client_key_display_id(id)) == Some(mine));
        match chosen {
            Some(id) => {
                self.watching = Some(id.to_owned());
                DeathCameraCue::Stop
            }
            // Everyone on our side is down: the round is about to end, so
            // stay rather than cut to an enemy.
            None if self.watching.take().is_some() => DeathCameraCue::Resume,
            None => DeathCameraCue::Keep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn match_state(phase: i64, round: u32, extra: &str) -> MatchStateManager {
        let mut m = MatchStateManager::new();
        let event = format!(
            r#"{{"type":"match_state","phase":{phase},"round":{round},"playerTeams":{{"1":"red","2":"red","3":"red","9":"blue"}}{extra}}}"#
        );
        m.process_event(&shagger::shared::json::parse(&event).unwrap());
        m
    }

    #[test]
    fn hands_over_to_the_first_living_teammate_after_the_corpse() {
        let m = match_state(2, 1, "");
        let alive = ["player_9", "player_3", "player_2"];
        let mut view = SpectateView::default();
        let cue = view.update(SPECTATE_AFTER_DEATH - 0.01, true, "player_1", &m, alive.into_iter());
        assert_eq!((cue, view.watching()), (DeathCameraCue::Keep, None));
        let cue = view.update(0.02, true, "player_1", &m, alive.into_iter());
        // Draw order, skipping the enemy.
        assert_eq!((cue, view.watching()), (DeathCameraCue::Stop, Some("player_3")));
        // Sticks with them while they live, then the next.
        view.update(0.1, true, "player_1", &m, ["player_2", "player_3"].into_iter());
        assert_eq!(view.watching(), Some("player_3"));
        view.update(0.1, true, "player_1", &m, ["player_2"].into_iter());
        assert_eq!(view.watching(), Some("player_2"));
        // Nobody left: back to the death camera.
        let cue = view.update(0.1, true, "player_1", &m, ["player_9"].into_iter());
        assert_eq!((cue, view.watching()), (DeathCameraCue::Resume, None));
        // Alive again: released.
        let cue = view.update(0.1, false, "player_1", &m, alive.into_iter());
        assert_eq!((cue, view.watching()), (DeathCameraCue::Stop, None));
    }

    #[test]
    fn no_hand_over_outside_a_live_round() {
        let mut view = SpectateView::default();
        let warm = match_state(2, 0, "");
        view.update(5.0, true, "player_1", &warm, ["player_2"].into_iter());
        assert_eq!(view.watching(), None);
    }

    #[test]
    fn the_dead_watch_what_decided_the_round() {
        let end = match_state(
            4,
            2,
            r#","roundResult":{"round":2,"reason":"detonated","focus":{"x":1,"y":2,"z":3}}"#,
        );
        let mut view = SpectateView::default();
        let cue = view.update(4.0, true, "player_1", &end, ["player_2"].into_iter());
        assert_eq!(cue, DeathCameraCue::Focus(DVec3::new(1.0, 2.0, 3.0)));
        assert_eq!(
            view.update(0.1, true, "player_1", &end, ["player_2"].into_iter()),
            DeathCameraCue::Keep
        );
        // A living player keeps their own view.
        let mut living = SpectateView::default();
        assert_eq!(
            living.update(0.1, false, "player_1", &end, ["player_2"].into_iter()),
            DeathCameraCue::Stop
        );
    }
}
