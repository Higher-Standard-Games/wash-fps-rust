//! wash-fps `client/FPSPlayerAudio.ts`: which voice a player speaks with:
//! the voice of the body they are drawn with, the server's in a
//! free-for-all, otherwise by side (`cl_team_ally_model`,
//! `cl_team_enemy_model`).

use std::collections::HashMap;
use std::sync::Mutex;

use shagger::core::cvar::Cvars;

use crate::client::player_model_selection::{
    PlayerModelSide, player_model_choice, player_model_voice, register_player_model_selection, set_player_model_choice,
};

use crate::client::player_model_visual_settings::{PlayerTeamSlot, get_player_team_slot};
use crate::shared::players::player_look::decode_look;

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

static LOCAL_VOICE: Mutex<Option<&'static str>> = Mutex::new(None);

/// `setLocalPlayerVoice`: your own voice, that of the body you are drawn
/// with, set from each snapshot.
pub fn set_local_player_voice(voice: Option<&'static str>) {
    *LOCAL_VOICE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = voice;
}

/// `localPlayerVoice`.
#[must_use]
pub fn local_player_voice() -> &'static str {
    let voice = *LOCAL_VOICE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    voice.unwrap_or_else(|| player_voice_set(PlayerVoiceSide::Ally))
}

/// `playerVoiceForEntity`: the voice of the body a player is drawn with:
/// the server-assigned one in a free-for-all (`looks`, yours included),
/// otherwise by colour allegiance, relative to the local or followed POV
/// (`teams` from the roster).
#[must_use]
pub fn player_voice_for_entity(
    teams: &HashMap<String, String>,
    looks: &HashMap<String, u8>,
    entity_id: &str,
    view_entity_id: Option<&str>,
) -> &'static str {
    if let Some(look) = looks.get(entity_id).copied().and_then(decode_look) {
        return player_model_voice(look.model);
    }
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
    fn the_viewed_player_is_an_ally_and_a_server_look_speaks_for_its_body() {
        let teams: HashMap<String, String> = [("a".into(), "ffa".into()), ("b".into(), "ffa".into())].into();
        let none = HashMap::new();
        // Your side wears Police by default, which speaks male.
        assert_eq!(player_voice_for_entity(&teams, &none, "a", Some("a")), "male");
        // PlayerVoiceModel.test.ts: the assigned body's voice, yours too.
        let looks: HashMap<String, u8> = [("a".into(), 0x10), ("b".into(), 0x42)].into();
        assert_eq!(player_voice_for_entity(&teams, &looks, "a", Some("a")), "female");
        assert_eq!(player_voice_for_entity(&teams, &looks, "b", Some("a")), "bobot");
        assert_eq!(fps_player_sound_paths().len(), 24);
    }
}
