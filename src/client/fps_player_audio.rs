//! wash-fps `client/FPSPlayerAudio.ts`: which voice a player speaks with,
//! by side: the voice of the body they wear (`cl_team_ally_model`,
//! `cl_team_enemy_model`).

use std::collections::HashMap;

use shagger::core::cvar::Cvars;

use crate::client::player_model_selection::{
    PlayerModelSide, player_model_choice, player_model_voice, register_player_model_selection, set_player_model_choice,
};

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

/// `registerPlayerAudioCvars`: the voices follow the bodies.
pub fn register_player_audio_cvars(cvars: &mut Cvars) {
    register_player_model_selection(cvars);
}

const fn model_side(side: PlayerVoiceSide) -> PlayerModelSide {
    match side {
        PlayerVoiceSide::Ally => PlayerModelSide::Ally,
        PlayerVoiceSide::Enemy => PlayerModelSide::Enemy,
    }
}

/// `playerVoiceSet(side)`: the side's body's voice.
#[must_use]
pub fn player_voice_set(side: PlayerVoiceSide) -> &'static str {
    player_model_voice(player_model_choice(model_side(side)))
}

/// `setPlayerVoiceSet`: the body that speaks with `voice`.
pub fn set_player_voice_set(side: PlayerVoiceSide, voice: &str) {
    let choice = match voice {
        "female" => "female",
        "bobot" => "robot",
        _ => "police",
    };
    set_player_model_choice(model_side(side), choice);
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
        // Both sides wear Police by default, which speaks male.
        assert_eq!(player_voice_for_entity(&teams, "b", Some("a")), "male");
        assert_eq!(fps_player_sound_paths().len(), 24);
    }
}
