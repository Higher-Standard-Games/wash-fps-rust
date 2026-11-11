//! wash-fps `client/PlayerModelSelection.ts`: which body (and so which
//! voice) each side's players wear, by `cl_team_ally_model` and
//! `cl_team_enemy_model`. Enemies may be `random`: a body the own side
//! does not wear, drawn again each game.

use std::sync::Mutex;

use shagger::animation::player_model_assets::{
    CARTEL_MODEL_ROOT, FEMALE_MODEL_ROOT, PLAYER_MODEL_FILE, PLAYER_MODEL_ROOT, RIOTKITTY_MODEL_ROOT, ROBOT_MODEL_ROOT,
};
use shagger::core::cvar::{CvarOptions, CvarType, Cvars, with_cvars};

/// `PLAYER_MODELS`: `LOOK_MODELS`, the free-for-all look byte's order.
pub const PLAYER_MODELS: [&str; 5] = crate::shared::players::player_look::LOOK_MODELS;

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

/// `registerPlayerModelSelection`: Police for your side, Random for enemies.
pub fn register_player_model_selection(cvars: &mut Cvars) {
    for (side, default, description) in [
        (
            PlayerModelSide::Ally,
            "police",
            "Your side player model and voice: police, female, cartel, riotkitty, robot",
        ),
        (
            PlayerModelSide::Enemy,
            "random",
            "Enemy player model and voice: random (a new one each game), police, female, cartel, riotkitty, robot",
        ),
    ] {
        let mut options = CvarOptions::describe(description).kind(CvarType::String);
        options.flags.persist = true;
        cvars.register(player_model_cvar(side), default, options);
    }
}

/// `playerModelSetting`: what the setting says, `random` included (enemies
/// only); Police for anything else.
#[must_use]
pub fn player_model_setting(side: PlayerModelSide) -> &'static str {
    let name = player_model_cvar(side);
    let value = with_cvars(|c| c.get(name).map(|_| c.get_string(name)))
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if side == PlayerModelSide::Enemy && value == "random" {
        return "random";
    }
    PLAYER_MODELS.iter().find(|m| **m == value).copied().unwrap_or("police")
}

/// The random enemy body drawn for this game (`randomPick`).
static RANDOM_PICK: Mutex<Option<&'static str>> = Mutex::new(None);

/// `rerollRandomEnemyModel`: each game you join draws its own.
pub fn reroll_random_enemy_model() {
    *RANDOM_PICK.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// `playerModelChoice`: the body a side is drawn with; `random` is drawn
/// once a game, never your own side's body.
#[must_use]
pub fn player_model_choice(side: PlayerModelSide) -> &'static str {
    let setting = player_model_setting(side);
    if setting != "random" {
        return setting;
    }
    let own = player_model_choice(PlayerModelSide::Ally);
    let mut pick = RANDOM_PICK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match *pick {
        Some(model) if model != own => model,
        _ => {
            let others: Vec<&'static str> = PLAYER_MODELS.iter().copied().filter(|m| *m != own).collect();
            let model = others[random_index(others.len())];
            *pick = Some(model);
            model
        }
    }
}

/// `Math.floor(Math.random() * len)`, from the clock.
fn random_index(len: usize) -> usize {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    (nanos as usize / 1000) % len.max(1)
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

    #[test]
    fn a_random_enemy_never_wears_your_body_and_keeps_it_for_the_game() {
        with_cvars(|c| {
            register_player_model_selection(c);
            c.set("cl_team_ally_model", "cartel");
        });
        assert_eq!(player_model_setting(PlayerModelSide::Enemy), "random");
        reroll_random_enemy_model();
        let first = player_model_choice(PlayerModelSide::Enemy);
        assert_ne!(first, "cartel");
        assert_eq!(player_model_choice(PlayerModelSide::Enemy), first);
        with_cvars(|c| c.set("cl_team_ally_model", first));
        assert_ne!(player_model_choice(PlayerModelSide::Enemy), first);
        with_cvars(|c| {
            c.reset("cl_team_ally_model");
            c.set("cl_team_enemy_model", "robot");
        });
        assert_eq!(player_model_choice(PlayerModelSide::Enemy), "robot");
        with_cvars(|c| c.reset("cl_team_enemy_model"));
    }
}
