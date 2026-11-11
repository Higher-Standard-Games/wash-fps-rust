//! What the HUD reads (wash-fps `client/HUDState.ts`): the scoreboard and
//! the match's names and scores, and whose view it is. The transient
//! parts (damage numbers, kill feed, pickups) come with their widgets.

use crate::client::local_powerup_state::LocalPowerupState;
use std::collections::BTreeMap;

use shagger::shared::json::Value;
use shagger::shared::net::game_state::PlayerState;

use crate::client::ammo_status::HudWeaponAmmo;

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

/// `KillFeedEntry`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KillFeedEntry {
    pub victim_id: String,
    pub killer_id: String,
    pub victim_name: String,
    pub killer_name: String,
    pub weapon_id: String,
    pub by_local: bool,
    pub of_local: bool,
    pub by_viewed: bool,
    pub of_viewed: bool,
    /// Counts every death seen, so a reader can tell which are new.
    pub seq: u64,
    /// Seconds on the HUD's clock when it came.
    pub timestamp: f64,
}

/// `DeathParties`.
#[derive(Debug, Clone, Default)]
pub struct DeathParties<'a> {
    pub killer_name: &'a str,
    pub victim_name: &'a str,
    pub local_entity_id: Option<&'a str>,
    pub view_entity_id: Option<&'a str>,
}

const KILL_FEED_EXPIRE: f64 = 5.0;
const KILL_FEED_MAX: usize = 5;

/// `HUDState`: combat, weapon, scoreboard and match.
#[derive(Debug, Clone)]
pub struct HudState {
    /// The subject's health and armor (`combat_state`).
    pub health: f64,
    pub armor: f64,
    /// The subject's dead flag.
    pub dead: bool,
    /// Our own, whoever the camera follows.
    pub local_dead: bool,
    /// The subject's weapon in hand, inventory and ammo (`weapon_state`).
    pub current_weapon: String,
    pub weapons: Vec<String>,
    pub ammo: BTreeMap<String, HudWeaponAmmo>,
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
    pub kill_feed: Vec<KillFeedEntry>,
    death_seq: u64,
    /// The HUD's clock in seconds (`performance.now()`).
    pub now: f64,
    /// The subject's powerup timers (`LocalPowerupState`).
    pub powerups: LocalPowerupState,
    /// Our own air, 0 to 1, and whether it has run out
    /// (`localAirFraction`, `localDrowning`).
    pub air: f64,
    pub drowning: bool,
}

impl Default for HudState {
    fn default() -> Self {
        Self {
            health: 100.0,
            armor: 0.0,
            dead: false,
            local_dead: false,
            current_weapon: "blade".to_owned(),
            weapons: vec!["blade".to_owned()],
            ammo: BTreeMap::new(),
            scoreboard: Vec::new(),
            local_player_id: String::new(),
            spectating: String::new(),
            watcher: false,
            playback: false,
            gametype_name: String::new(),
            team_based: false,
            team_scores: BTreeMap::new(),
            team_names: BTreeMap::new(),
            kill_feed: Vec::new(),
            death_seq: 0,
            now: 0.0,
            powerups: LocalPowerupState::default(),
            air: 1.0,
            drowning: false,
        }
    }
}

impl HudState {
    /// `processCombatState`.
    pub fn process_combat_state(&mut self, health: u16, armor: u16, dead: bool) {
        self.health = f64::from(health);
        self.armor = f64::from(armor);
        self.dead = dead;
    }

    /// `processWeaponState`.
    pub fn process_weapon_state(&mut self, current_weapon: &str, weapons: &Value, ammo: &Value) {
        current_weapon.clone_into(&mut self.current_weapon);
        self.weapons = weapons
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter_map(|w| w.as_str().map(str::to_owned))
            .collect();
        self.ammo.clear();
        if let Value::Object(members) = ammo {
            for (id, value) in members {
                if let Some(a) = HudWeaponAmmo::from_json(value) {
                    self.ammo.insert(id.clone(), a);
                }
            }
        }
    }

    /// `currentClip`: the number for the weapon in hand (its clip, for a
    /// clip weapon).
    #[must_use]
    pub fn current_clip(&self) -> f64 {
        match self.ammo.get(&self.current_weapon) {
            Some(HudWeaponAmmo::Clip { clip, .. }) => *clip,
            Some(HudWeaponAmmo::Pool(n)) => *n,
            None => 0.0,
        }
    }

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

    /// `processDeath`.
    pub fn process_death(&mut self, victim_id: &str, killer_id: &str, weapon_id: &str, parties: &DeathParties<'_>) {
        let is = |id: Option<&str>, who: &str| id.is_some_and(|id| !id.is_empty() && id == who);
        self.death_seq += 1;
        self.kill_feed.push(KillFeedEntry {
            victim_id: victim_id.to_owned(),
            killer_id: killer_id.to_owned(),
            victim_name: parties.victim_name.to_owned(),
            killer_name: parties.killer_name.to_owned(),
            weapon_id: weapon_id.to_owned(),
            by_local: is(parties.local_entity_id, killer_id),
            of_local: is(parties.local_entity_id, victim_id),
            by_viewed: is(parties.view_entity_id, killer_id),
            of_viewed: is(parties.view_entity_id, victim_id),
            seq: self.death_seq,
            timestamp: self.now,
        });
        if self.kill_feed.len() > KILL_FEED_MAX {
            self.kill_feed.remove(0);
        }
    }

    /// `expireTransients`, with the clock moved on by `dt` seconds.
    pub fn expire_transients(&mut self, dt: f64) {
        self.now += dt;
        let now = self.now;
        self.kill_feed.retain(|k| now - k.timestamp < KILL_FEED_EXPIRE);
    }

    /// `resolvePlayerName`: a player's name from their entity id.
    #[must_use]
    pub fn resolve_player_name(players: &[(String, PlayerState)], entity_id: &str) -> String {
        players
            .iter()
            .find(|(_, p)| p.entity_id == entity_id)
            .map_or_else(|| entity_id.replace("player_", "Player "), |(_, p)| p.name.clone())
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

    #[test]
    fn the_kill_feed_caps_and_expires() {
        let mut hud = HudState::default();
        let parties = DeathParties {
            view_entity_id: Some("e1"),
            ..DeathParties::default()
        };
        for i in 0..7 {
            hud.process_death(&format!("v{i}"), "e1", "rail", &parties);
        }
        assert_eq!(hud.kill_feed.len(), 5);
        assert_eq!(hud.kill_feed[0].seq, 3);
        assert!(hud.kill_feed[0].by_viewed && !hud.kill_feed[0].of_viewed);
        hud.expire_transients(5.0);
        assert!(hud.kill_feed.is_empty());
        assert_eq!(HudState::resolve_player_name(&[], "player_3"), "Player 3");
    }
}
