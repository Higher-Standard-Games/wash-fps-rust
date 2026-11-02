//! wash-fps `client/FPSPlayerAudio.ts`: which voice a player speaks with,
//! by side (`cl_team_ally_sounds`, `cl_team_enemy_sounds`).

use std::collections::HashMap;

use shagger::core::cvar::{CvarOptions, CvarType, Cvars, with_cvars};

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

/// `VOICE_CVARS`.
#[must_use]
pub const fn voice_cvar(side: PlayerVoiceSide) -> &'static str {
    match side {
        PlayerVoiceSide::Ally => "cl_team_ally_sounds",
        PlayerVoiceSide::Enemy => "cl_team_enemy_sounds",
    }
}

/// `DEFAULTS`.
const fn default_voice(side: PlayerVoiceSide) -> &'static str {
    match side {
        PlayerVoiceSide::Ally => "male",
        PlayerVoiceSide::Enemy => "bobot",
    }
}

/// `registerPlayerAudioCvars`.
pub fn register_player_audio_cvars(cvars: &mut Cvars) {
    for (side, who) in [(PlayerVoiceSide::Ally, "Your side"), (PlayerVoiceSide::Enemy, "Enemy")] {
        cvars.register(
            voice_cvar(side),
            default_voice(side),
            CvarOptions::describe(&format!("{who} player sounds: male, female, bobot")).kind(CvarType::String),
        );
    }
}

/// `playerVoiceSet(side)`: the cvar's voice, or the default for anything else.
#[must_use]
pub fn player_voice_set(side: PlayerVoiceSide) -> &'static str {
    let value = with_cvars(|c| c.get(voice_cvar(side)).map(|_| c.get_string(voice_cvar(side))))
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    PLAYER_VOICE_SETS
        .iter()
        .find(|v| **v == value)
        .copied()
        .unwrap_or(default_voice(side))
}

/// `setPlayerVoiceSet`.
pub fn set_player_voice_set(side: PlayerVoiceSide, voice: &str) {
    with_cvars(|c| {
        c.set(voice_cvar(side), voice);
    });
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
