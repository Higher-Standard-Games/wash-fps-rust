//! wash-fps `client/MatchStateManager.ts`: the match as the `match_state`
//! events tell it (phase, clock, ready count, scores, rosters, rounds), for
//! the HUD and the announcer.
//!
//! The bomb's brush models (`brushModels.restore`) are applied by
//! `LivePlay`, which holds the prediction world.
//!
//! Not yet: the phase callbacks (callers compare phases themselves), and
//! typed objectives, round results and explosions, which stay JSON until
//! Search and Destroy needs them.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use shagger::shared::json::Value;
use shagger::shared::net::message_codec::{MatchPauseNotice, MatchPauseState};

/// `MatchPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchPhase {
    #[default]
    Warmup,
    Countdown,
    Active,
    PostGame,
    RoundEnd,
}

impl MatchPhase {
    #[must_use]
    pub fn from_number(n: i64) -> Self {
        match n {
            1 => Self::Countdown,
            2 => Self::Active,
            3 => Self::PostGame,
            4 => Self::RoundEnd,
            _ => Self::Warmup,
        }
    }
}

/// A standings row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Standing {
    pub player_id: String,
    pub name: String,
    pub score: f64,
    pub frags: Option<f64>,
    pub deaths: f64,
    pub ping: f64,
}

/// `MatchPause`: a timeout as the client shows it, until `ends_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchPause {
    /// `Timeout` or `Timein`, never `Running`.
    pub state: MatchPauseState,
    pub ends_at: Instant,
    pub caller: String,
    pub for_drop: bool,
}

/// A spectator row.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spectator {
    pub player_id: String,
    pub name: String,
    pub ping: f64,
}

#[derive(Debug, Clone, Default)]
pub struct MatchStateManager {
    phase: MatchPhase,
    time_remaining: f64,
    time_elapsed: Option<f64>,
    ready_count: u32,
    total_players: u32,
    gametype_name: String,
    loadout_mode: Option<String>,
    team_based: bool,
    score_limit: f64,
    team_scores: BTreeMap<String, f64>,
    player_teams: BTreeMap<String, String>,
    team_names: BTreeMap<String, String>,
    spectators: Vec<Spectator>,
    standings: Vec<Standing>,
    winner: Option<Value>,
    overtime: u32,
    flow_single: bool,
    round: u32,
    team_roles: BTreeMap<String, String>,
    round_state: Option<String>,
    objective: Option<Value>,
    round_result: Option<Value>,
    explosion: Option<Value>,
    is_ready: bool,
    /// A timeout holding the match, or its count back in; `None` while it runs.
    pause: Option<MatchPause>,
}

fn number(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn map<T>(v: &Value, key: &str, f: impl Fn(&Value) -> Option<T>) -> BTreeMap<String, T> {
    match v.get(key) {
        Some(Value::Object(o)) => o.iter().filter_map(|(k, v)| Some((k.clone(), f(v)?))).collect(),
        _ => BTreeMap::new(),
    }
}

fn present(v: &Value, key: &str) -> Option<Value> {
    v.get(key).filter(|x| !matches!(x, Value::Null)).cloned()
}

impl MatchStateManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `processEvent`: a `match_state` event's payload. Returns the phase
    /// before it when the phase changed (the phase callbacks' moment).
    pub fn process_event(&mut self, event: &Value) -> Option<MatchPhase> {
        let old = self.phase;
        self.phase = event
            .get("phase")
            .and_then(Value::as_f64)
            .map_or(MatchPhase::Warmup, |n| MatchPhase::from_number(n as i64));
        self.time_remaining = number(event, "timeRemaining").unwrap_or(0.0);
        self.time_elapsed = number(event, "timeElapsed");
        self.ready_count = number(event, "readyCount").unwrap_or(0.0) as u32;
        self.total_players = number(event, "totalPlayers").unwrap_or(0.0) as u32;
        self.gametype_name = string(event, "gametypeName").unwrap_or_default();
        self.team_based = event.get("teamBased").and_then(Value::as_bool).unwrap_or(false);
        self.loadout_mode = string(event, "loadouts").filter(|m| m == "round" || m == "respawn");
        self.score_limit = number(event, "scoreLimit").unwrap_or(0.0);
        self.team_scores = map(event, "teamScores", Value::as_f64);
        self.player_teams = map(event, "playerTeams", |v| v.as_str().map(str::to_owned));
        self.team_names = map(event, "teamNames", |v| v.as_str().map(str::to_owned));
        // The server leaves the roster out once we hold it, and says so.
        if event.get("rosterOmitted").and_then(Value::as_bool) != Some(true) {
            self.spectators = event
                .get("spectators")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|s| Spectator {
                            player_id: string(s, "playerId").unwrap_or_default(),
                            name: string(s, "name").unwrap_or_default(),
                            ping: number(s, "ping").unwrap_or(0.0),
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.standings = event
                .get("standings")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|s| Standing {
                            player_id: string(s, "playerId").unwrap_or_default(),
                            name: string(s, "name").unwrap_or_default(),
                            score: number(s, "score").unwrap_or(0.0),
                            frags: number(s, "frags"),
                            deaths: number(s, "deaths").unwrap_or(0.0),
                            ping: number(s, "ping").unwrap_or(0.0),
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        self.winner = present(event, "winner");
        self.overtime = number(event, "overtime").unwrap_or(0.0) as u32;
        self.flow_single = event.get("flow").and_then(Value::as_str) == Some("single");
        self.round = number(event, "round").unwrap_or(0.0) as u32;
        self.team_roles = map(event, "teamRoles", |v| v.as_str().map(str::to_owned));
        self.round_state = string(event, "roundState");
        self.objective = present(event, "objective");
        self.round_result = present(event, "roundResult");
        self.explosion = present(event, "explosion");
        (old != self.phase).then_some(old)
    }

    #[must_use]
    pub fn phase(&self) -> MatchPhase {
        self.phase
    }
    #[must_use]
    pub fn time_remaining(&self) -> f64 {
        self.time_remaining
    }
    #[must_use]
    pub fn ready_count(&self) -> u32 {
        self.ready_count
    }
    #[must_use]
    pub fn total_players(&self) -> u32 {
        self.total_players
    }
    #[must_use]
    pub fn gametype_name(&self) -> &str {
        &self.gametype_name
    }
    #[must_use]
    pub fn team_based(&self) -> bool {
        self.team_based
    }
    #[must_use]
    pub fn loadout_mode(&self) -> Option<&str> {
        self.loadout_mode.as_deref()
    }
    #[must_use]
    pub fn score_limit(&self) -> f64 {
        self.score_limit
    }
    #[must_use]
    pub fn round(&self) -> u32 {
        self.round
    }
    #[must_use]
    pub fn team_roles(&self) -> &BTreeMap<String, String> {
        &self.team_roles
    }
    #[must_use]
    pub fn round_state(&self) -> Option<&str> {
        self.round_state.as_deref()
    }
    #[must_use]
    pub fn objective(&self) -> Option<&Value> {
        self.objective.as_ref()
    }
    #[must_use]
    pub fn round_result(&self) -> Option<&Value> {
        self.round_result.as_ref()
    }
    #[must_use]
    pub fn explosion(&self) -> Option<&Value> {
        self.explosion.as_ref()
    }
    /// `bomb`: the objective when it is the bomb.
    #[must_use]
    pub fn bomb(&self) -> Option<&Value> {
        self.objective
            .as_ref()
            .filter(|o| o.get("kind").and_then(Value::as_str) == Some("bomb"))
    }
    /// `isAttacking`.
    #[must_use]
    pub fn is_attacking(&self, team_id: Option<&str>) -> bool {
        team_id.is_some_and(|t| self.team_roles.get(t).map(String::as_str) == Some("attacker"))
    }
    #[must_use]
    pub fn team_scores(&self) -> &BTreeMap<String, f64> {
        &self.team_scores
    }
    #[must_use]
    pub fn player_teams(&self) -> &BTreeMap<String, String> {
        &self.player_teams
    }
    #[must_use]
    pub fn team_names(&self) -> &BTreeMap<String, String> {
        &self.team_names
    }
    #[must_use]
    pub fn spectators(&self) -> &[Spectator] {
        &self.spectators
    }
    #[must_use]
    pub fn standings(&self) -> &[Standing] {
        &self.standings
    }
    #[must_use]
    pub fn winner(&self) -> Option<&Value> {
        self.winner.as_ref()
    }
    #[must_use]
    pub fn overtime(&self) -> u32 {
        self.overtime
    }
    /// `flow === "single"`: the post-game returns to the lobby.
    #[must_use]
    pub fn single_flow(&self) -> bool {
        self.flow_single
    }
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }
    pub fn set_ready(&mut self, ready: bool) {
        self.is_ready = ready;
    }

    /// `resolveWinnerPlayerId`: the winning player, or a one-player team's.
    #[must_use]
    pub fn resolve_winner_player_id(&self) -> Option<String> {
        let winner = self.winner.as_ref()?;
        let kind = winner.get("kind").and_then(Value::as_str)?;
        let id = winner.get("id").and_then(Value::as_str).filter(|i| !i.is_empty())?;
        match kind {
            "draw" => None,
            "player" => Some(id.to_owned()),
            _ => {
                let members: Vec<&String> = self
                    .player_teams
                    .iter()
                    .filter(|(_, team)| team.as_str() == id)
                    .map(|(player, _)| player)
                    .collect();
                (members.len() == 1).then(|| members[0].clone())
            }
        }
    }

    /// `setPause`: the server's pause state, from its Pause message.
    pub fn set_pause(&mut self, notice: &MatchPauseNotice) {
        self.set_pause_at(notice, Instant::now());
    }

    /// [`Self::set_pause`] as of `now`.
    pub fn set_pause_at(&mut self, notice: &MatchPauseNotice, now: Instant) {
        self.pause = (notice.state != MatchPauseState::Running).then(|| MatchPause {
            state: notice.state,
            ends_at: now + Duration::from_millis(u64::from(notice.ms_left)),
            caller: notice.caller.clone(),
            for_drop: notice.for_drop,
        });
    }

    /// `pause`: the timeout holding the match, or `None` while it runs.
    #[must_use]
    pub fn pause(&self) -> Option<&MatchPause> {
        self.pause.as_ref()
    }

    /// `pauseSecondsLeft`: until the timeout runs out or the count ends; 0
    /// while running.
    #[must_use]
    pub fn pause_seconds_left(&self) -> f64 {
        self.pause_seconds_left_at(Instant::now())
    }

    /// [`Self::pause_seconds_left`] as of `now`.
    #[must_use]
    pub fn pause_seconds_left_at(&self, now: Instant) -> f64 {
        self.pause
            .as_ref()
            .map_or(0.0, |p| p.ends_at.saturating_duration_since(now).as_secs_f64())
    }

    /// `isMovementFrozen`: a timeout, the post-game, and a round mode's
    /// countdown and round end.
    #[must_use]
    pub fn is_movement_frozen(&self) -> bool {
        if self.pause.is_some() {
            return true;
        }
        match self.phase {
            MatchPhase::PostGame => true,
            MatchPhase::RoundEnd | MatchPhase::Countdown => self.round > 0,
            _ => false,
        }
    }

    /// `getTimeString`: time left, or with `invert_clock` (`cl_invertClock
    /// 1`) and a live match, time played.
    #[must_use]
    pub fn get_time_string(&self, invert_clock: bool) -> String {
        let count_up = self.phase == MatchPhase::Active && self.time_elapsed.is_some() && invert_clock;
        let total = if count_up {
            self.time_elapsed.unwrap_or(0.0).floor().max(0.0)
        } else {
            self.time_remaining.ceil().max(0.0)
        } as u64;
        format!("{}:{:02}", total / 60, total % 60)
    }

    /// `getCountdownNumber`.
    #[must_use]
    pub fn get_countdown_number(&self) -> u32 {
        if self.phase == MatchPhase::Countdown {
            self.time_remaining.ceil().max(0.0) as u32
        } else {
            0
        }
    }

    /// `getReadyStatus`.
    #[must_use]
    pub fn get_ready_status(&self) -> String {
        format!("{}/{} Ready", self.ready_count, self.total_players)
    }

    /// `getPhaseName`.
    #[must_use]
    pub fn get_phase_name(&self) -> &'static str {
        match self.phase {
            MatchPhase::Warmup => "Warmup",
            MatchPhase::Countdown => "Starting",
            MatchPhase::Active => "Active",
            MatchPhase::PostGame => "Match Over",
            MatchPhase::RoundEnd => "Round Over",
        }
    }

    /// `reset`: the rosters cannot outlive their match.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use shagger::shared::json::parse;

    use super::*;

    fn json(text: &str) -> Value {
        parse(text).unwrap()
    }

    #[test]
    fn a_timeout_freezes_movement_and_counts_down() {
        let mut m = MatchStateManager::new();
        m.process_event(&json(r#"{"phase": 2}"#));
        assert!(!m.is_movement_frozen());
        let t0 = Instant::now();
        m.set_pause_at(
            &MatchPauseNotice {
                state: MatchPauseState::Timeout,
                ms_left: 120_000,
                caller: "Alice".into(),
                for_drop: false,
            },
            t0,
        );
        assert!(m.is_movement_frozen());
        assert_eq!(m.pause().map(|p| p.caller.as_str()), Some("Alice"));
        assert_eq!(m.pause_seconds_left_at(t0 + Duration::from_secs(30)), 90.0);
        assert_eq!(m.pause_seconds_left_at(t0 + Duration::from_secs(300)), 0.0);
        m.set_pause_at(&MatchPauseNotice::default(), t0);
        assert!(m.pause().is_none());
        assert!(!m.is_movement_frozen());
        assert_eq!(m.pause_seconds_left_at(t0), 0.0);
    }

    #[test]
    fn reads_a_match_state() {
        let mut m = MatchStateManager::new();
        let changed = m.process_event(&json(
            r#"{"phase": 2, "timeRemaining": 61.2, "timeElapsed": 118.9, "overtime": 1, "teamBased": false,
                "standings": [{"playerId": "1", "name": "a", "score": 3, "deaths": 1, "ping": 20}], "winner": null}"#,
        ));
        assert_eq!(changed, Some(MatchPhase::Warmup));
        assert_eq!(m.phase(), MatchPhase::Active);
        assert_eq!(m.get_time_string(false), "1:02");
        assert_eq!(m.get_time_string(true), "1:58");
        assert_eq!(m.standings()[0].score, 3.0);
        assert_eq!(m.winner(), None);
        // A roster left out keeps the last one.
        m.process_event(&json(r#"{"phase": 2, "rosterOmitted": true}"#));
        assert_eq!(m.standings().len(), 1);
        assert!(!m.is_movement_frozen());
    }

    #[test]
    fn a_one_player_team_wins_as_its_player() {
        let mut m = MatchStateManager::new();
        m.process_event(&json(
            r#"{"phase": 3, "winner": {"kind": "team", "id": "udon"}, "playerTeams": {"4": "udon", "5": "soba"}}"#,
        ));
        assert_eq!(m.resolve_winner_player_id().as_deref(), Some("4"));
        assert!(m.is_movement_frozen());
    }
}
