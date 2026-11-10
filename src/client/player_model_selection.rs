//! wash-fps `client/PlayerModelSelection.ts`: which body (and so which
//! voice) each side's players wear, by `cl_team_ally_model` and
//! `cl_team_enemy_model`.

use shagger::animation::player_model_assets::{
    CARTEL_MODEL_ROOT, FEMALE_MODEL_ROOT, PLAYER_MODEL_FILE, PLAYER_MODEL_ROOT, RIOTKITTY_MODEL_ROOT, ROBOT_MODEL_ROOT,
};
use shagger::core::cvar::{CvarOptions, CvarType, Cvars, with_cvars};

/// `PLAYER_MODELS`.
pub const PLAYER_MODELS: [&str; 5] = ["police", "female", "cartel", "riotkitty", "robot"];

/// `PlayerModelSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerModelSide {
    Ally,
    Enemy,
}

/// `PLAYER_MODEL_LABELS`.
#[must_use]
pub fn player_model_label(choice: &str) -> &'static str {
    match choice {
        "female" => "Female Police",
        "cartel" => "Cartel",
        "riotkitty" => "RiotKitty",
        "robot" => "Robot",
        _ => "Police",
    }
}

/// `PLAYER_MODEL_VOICES`.
#[must_use]
pub fn player_model_voice(choice: &str) -> &'static str {
    match choice {
        "female" | "riotkitty" => "female",
        "robot" => "bobot",
        _ => "male",
    }
}

/// `PLAYER_MODEL_ASSETS`: a body's root and file.
#[must_use]
pub fn player_model_asset(choice: &str) -> (&'static str, &'static str) {
    match choice {
        "female" => (FEMALE_MODEL_ROOT, "female.glb"),
        "cartel" => (CARTEL_MODEL_ROOT, "cartel.glb"),
        "riotkitty" => (RIOTKITTY_MODEL_ROOT, "riotkitty.glb"),
        "robot" => (ROBOT_MODEL_ROOT, "robot.glb"),
        _ => (PLAYER_MODEL_ROOT, PLAYER_MODEL_FILE),
    }
}

/// `KEYS`.
#[must_use]
pub const fn player_model_cvar(side: PlayerModelSide) -> &'static str {
    match side {
        PlayerModelSide::Ally => "cl_team_ally_model",
        PlayerModelSide::Enemy => "cl_team_enemy_model",
    }
}

/// `registerPlayerModelSelection`.
pub fn register_player_model_selection(cvars: &mut Cvars) {
    for (side, who) in [(PlayerModelSide::Ally, "Your side"), (PlayerModelSide::Enemy, "Enemy")] {
        let mut options = CvarOptions::describe(&format!(
            "{who} player model and voice: police, female, cartel, riotkitty, robot"
        ))
        .kind(CvarType::String);
        options.flags.persist = true;
        cvars.register(player_model_cvar(side), "police", options);
    }
}

/// `playerModelChoice`: the cvar's body, or Police for anything else.
#[must_use]
pub fn player_model_choice(side: PlayerModelSide) -> &'static str {
    let name = player_model_cvar(side);
    let value = with_cvars(|c| c.get(name).map(|_| c.get_string(name)))
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    PLAYER_MODELS.iter().find(|m| **m == value).copied().unwrap_or("police")
}

/// `setPlayerModelChoice`.
pub fn set_player_model_choice(side: PlayerModelSide, choice: &str) {
    with_cvars(|c| {
        c.set(player_model_cvar(side), choice);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_body_has_a_voice_and_an_asset() {
        assert_eq!(player_model_voice("police"), "male");
        assert_eq!(player_model_voice("riotkitty"), "female");
        assert_eq!(player_model_voice("robot"), "bobot");
        assert_eq!(player_model_asset("cartel"), (CARTEL_MODEL_ROOT, "cartel.glb"));
        assert_eq!(player_model_asset("nonsense").1, PLAYER_MODEL_FILE);
        assert_eq!(player_model_label("female"), "Female Police");
    }
}
