//! wash-fps `client/FPSPlayerAudio.ts`: which voice a player speaks with.
//! The cvars (`cl_team_ally_sounds`, `cl_team_enemy_sounds`) are not
//! ported yet, so the voices are their defaults.

use std::collections::HashMap;

use crate::client::player_model_visual_settings::{PlayerTeamSlot, get_player_team_slot};

/// `PLAYER_VOICE_SETS`.
pub const PLAYER_VOICE_SETS: [&str; 3] = ["male", "female", "bobot"];
/// `CUES`.
pub const CUES: [&str; 8] = [
    "pain25", "pain50", "pain75", "pain100", "death", "jump_1", "jump_2", "drown",
];

/// `PlayerVoiceSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerVoiceSide {
    Ally,
    Enemy,
}

/// `playerVoiceSet(side)`, at the cvars' defaults.
#[must_use]
pub fn player_voice_set(side: PlayerVoiceSide) -> &'static str {
    match side {
        PlayerVoiceSide::Ally => "male",
        PlayerVoiceSide::Enemy => "bobot",
    }
}

/// `playerVoiceForEntity`: the viewed player speaks as an ally; others by
/// their team against the viewer's (`teams` from the roster).
#[must_use]
pub fn player_voice_for_entity(
    teams: &HashMap<String, String>,
    entity_id: &str,
    view_entity_id: Option<&str>,
) -> &'static str {
    if Some(entity_id) == view_entity_id {
        return player_voice_set(PlayerVoiceSide::Ally);
    }
    let team = teams.get(entity_id).map(String::as_str);
    let viewer = view_entity_id.and_then(|v| teams.get(v)).map(String::as_str);
    match get_player_team_slot(team, viewer) {
        PlayerTeamSlot::Ally => player_voice_set(PlayerVoiceSide::Ally),
        _ => player_voice_set(PlayerVoiceSide::Enemy),
    }
}

/// `playerVoiceSound`.
#[must_use]
pub fn player_voice_sound(cue: &str, voice: &str) -> String {
    format!("sounds/players/{voice}/{cue}.ogg")
}

/// `FPS_PLAYER_SOUND_PATHS`.
#[must_use]
pub fn fps_player_sound_paths() -> Vec<String> {
    PLAYER_VOICE_SETS
        .iter()
        .flat_map(|voice| CUES.iter().map(move |cue| player_voice_sound(cue, voice)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_viewed_player_is_an_ally() {
        let teams: HashMap<String, String> = [("a".into(), "ffa".into()), ("b".into(), "ffa".into())].into();
        assert_eq!(player_voice_for_entity(&teams, "a", Some("a")), "male");
        assert_eq!(player_voice_for_entity(&teams, "b", Some("a")), "bobot");
        assert_eq!(fps_player_sound_paths().len(), 24);
    }
}
