//! What the HUD reads (wash-fps `client/HUDState.ts`): the scoreboard and
//! the match's names and scores, and whose view it is. The transient
//! parts (damage numbers, kill feed, pickups) come with their widgets.

use std::collections::BTreeMap;

use shagger::shared::net::game_state::PlayerState;

use crate::client::match_state_manager::{MatchPhase, MatchStateManager};
use crate::client::remote_player_logic::CLIENT_KEY_PREFIX;

/// `ScoreboardEntry`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreboardEntry {
    pub frags: Option<f64>,
    pub player_id: String,
    pub entity_id: String,
    pub name: String,
    pub score: f64,
    pub deaths: f64,
    pub ping: f64,
    pub team_id: Option<String>,
}

/// `clientKeyDisplayId`.
#[must_use]
pub fn client_key_display_id(key: &str) -> &str {
    key.strip_prefix(CLIENT_KEY_PREFIX).unwrap_or(key)
}

/// `HUDState`'s scoreboard and match half.
#[derive(Debug, Clone, Default)]
pub struct HudState {
    pub scoreboard: Vec<ScoreboardEntry>,
    pub local_player_id: String,
    /// The player whose eyes the view is behind, or "" for our own.
    pub spectating: String,
    /// A spectator rather than a player.
    pub watcher: bool,
    /// A recording, not a match we are in.
    pub playback: bool,
    pub gametype_name: String,
    pub team_based: bool,
    pub team_scores: BTreeMap<String, f64>,
    pub team_names: BTreeMap<String, String>,
}

impl HudState {
    /// `subjectless`: a spectator flying free.
    #[must_use]
    pub fn subjectless(&self) -> bool {
        self.watcher && self.spectating.is_empty()
    }

    /// The player on screen: the one followed, else our own.
    #[must_use]
    pub fn subject(&self) -> &str {
        if self.spectating.is_empty() {
            &self.local_player_id
        } else {
            &self.spectating
        }
    }

    /// The scoreboard half of `updateFromGameState`.
    pub fn update_from_game_state(&mut self, players: &[(String, PlayerState)], match_state: &MatchStateManager) {
        self.scoreboard.clear();
        self.gametype_name = match_state.gametype_name().to_owned();
        self.team_based = match_state.team_based();
        self.team_scores.clone_from(match_state.team_scores());
        self.team_names.clone_from(match_state.team_names());
        let team_based = self.team_based;
        let side_of = |raw: &str| -> Option<String> {
            if team_based {
                match_state.player_teams().get(raw).cloned()
            } else {
                None
            }
        };
        let post_game = match_state.phase() == MatchPhase::PostGame;
        for (player_id, player) in players {
            let team_id = side_of(client_key_display_id(player_id));
            let fin = post_game
                .then(|| match_state.standings().iter().find(|e| &e.player_id == player_id))
                .flatten();
            self.scoreboard.push(ScoreboardEntry {
                player_id: player_id.clone(),
                entity_id: player.entity_id.clone(),
                name: player.name.clone(),
                score: f64::from(player.score),
                deaths: fin.map_or(f64::from(player.deaths), |f| f.deaths),
                frags: fin.and_then(|f| f.frags),
                ping: f64::from(player.ping),
                team_id,
            });
        }
        // Sort by score descending (stable, as `Array.prototype.sort`).
        self.scoreboard.sort_by(|a, b| b.score.total_cmp(&a.score));
        if self.scoreboard.is_empty() && !match_state.standings().is_empty() {
            for entry in match_state.standings() {
                self.scoreboard.push(ScoreboardEntry {
                    player_id: entry.player_id.clone(),
                    entity_id: String::new(),
                    name: entry.name.clone(),
                    score: entry.score,
                    deaths: entry.deaths,
                    frags: entry.frags,
                    ping: entry.ping,
                    team_id: side_of(client_key_display_id(&entry.player_id)),
                });
            }
            self.scoreboard.sort_by(|a, b| b.score.total_cmp(&a.score));
        }
        let seen: Vec<String> = self.scoreboard.iter().map(|e| e.player_id.clone()).collect();
        for spec in match_state.spectators() {
            if seen.contains(&spec.player_id) {
                continue;
            }
            self.scoreboard.push(ScoreboardEntry {
                player_id: spec.player_id.clone(),
                name: spec.name.clone(),
                ping: spec.ping,
                team_id: Some("spectator".to_owned()),
                ..ScoreboardEntry::default()
            });
        }
    }

    /// The scoreboard row of the player whose entity is `entity_id`.
    #[must_use]
    pub fn entry_for_entity(&self, entity_id: &str) -> Option<&ScoreboardEntry> {
        self.scoreboard.iter().find(|e| e.entity_id == entity_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(entity: &str, name: &str, score: i32) -> PlayerState {
        PlayerState {
            entity_id: entity.into(),
            name: name.into(),
            score,
            ..PlayerState::default()
        }
    }

    #[test]
    fn sorts_by_score_and_keeps_ties_in_order() {
        let mut hud = HudState::default();
        let players = vec![
            ("player_a".to_owned(), player("e1", "A", 1)),
            ("player_b".to_owned(), player("e2", "B", 5)),
            ("player_c".to_owned(), player("e3", "C", 1)),
        ];
        hud.update_from_game_state(&players, &MatchStateManager::new());
        let names: Vec<_> = hud.scoreboard.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["B", "A", "C"]);
        assert_eq!(hud.entry_for_entity("e3").unwrap().player_id, "player_c");
        assert_eq!(client_key_display_id("player_x"), "x");
    }
}
