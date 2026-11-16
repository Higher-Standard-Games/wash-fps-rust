//! wash-fps `client/PlayerModelSelection.ts`: which body (and so which
//! voice) each side's players wear, by `cl_team_ally_model` and
//! `cl_team_enemy_model`. Enemies may be `random`: a body the own side
//! does not wear, drawn again each game.

use std::collections::HashMap;
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

/// A campaign's models (`forced`, `forcedByName`, `scaleByName`).
#[derive(Debug, Default)]
struct Forced {
    ally: Option<&'static str>,
    enemy: Option<&'static str>,
    by_name: HashMap<String, &'static str>,
    scale_by_name: HashMap<String, f64>,
}

static FORCED: Mutex<Option<Forced>> = Mutex::new(None);

fn forced<T>(read: impl FnOnce(&Forced) -> Option<T>) -> Option<T> {
    FORCED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(read)
}

fn known_model(model: &str) -> Option<&'static str> {
    PLAYER_MODELS.iter().find(|m| **m == model).copied()
}

/// `forcePlayerModels`: draws this game's sides with these models (ally,
/// enemy) whatever the settings say, as a campaign mission does, and the
/// players named in `by_name` with a model of their own (a mixed crew);
/// `None` goes back to the settings. Unknown ids are ignored.
pub fn force_player_models<'a>(
    models: Option<(&str, &str)>,
    by_name: impl IntoIterator<Item = (&'a str, &'a str)>,
    scales_by_name: impl IntoIterator<Item = (&'a str, f64)>,
) {
    let mut forced = FORCED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some((ally, enemy)) = models else {
        *forced = None;
        return;
    };
    *forced = Some(Forced {
        ally: known_model(ally),
        enemy: known_model(enemy),
        by_name: by_name
            .into_iter()
            .filter_map(|(name, model)| Some((name.to_owned(), known_model(model)?)))
            .collect(),
        scale_by_name: scales_by_name
            .into_iter()
            .filter(|(_, scale)| scale.is_finite() && *scale > 0.0)
            .map(|(name, scale)| (name.to_owned(), scale))
            .collect(),
    });
}

/// `forcedScaleForName`: how many times a player's size a campaign draws
/// this player (a boss); 1 for everyone else.
#[must_use]
pub fn forced_scale_for_name(name: Option<&str>) -> f64 {
    name.and_then(|name| forced(|f| f.scale_by_name.get(name).copied()))
        .unwrap_or(1.0)
}

/// `forcedModelForName`: the model a campaign gives this player by name,
/// if it gives them one of their own.
#[must_use]
pub fn forced_model_for_name(name: Option<&str>) -> Option<&'static str> {
    name.and_then(|name| forced(|f| f.by_name.get(name).copied()))
}

/// `playerModelChoice`: the body a side is drawn with; `random` is drawn
/// once a game, never your own side's body.
#[must_use]
pub fn player_model_choice(side: PlayerModelSide) -> &'static str {
    let over = forced(|f| match side {
        PlayerModelSide::Ally => f.ally,
        PlayerModelSide::Enemy => f.enemy,
    });
    if let Some(model) = over {
        return model;
    }
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

/// `Math.floor(Math.random() * len)`.
fn random_index(len: usize) -> usize {
    shagger::shared::math_random::MathRandom::from_entropy().index(len)
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
