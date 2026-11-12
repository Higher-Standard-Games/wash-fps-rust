//! Ported from wash-fps `client/settings/ControllerModel.ts`.
//!
//! The controller's actions, their default buttons (the W3C standard
//! layout's numbers) and the `in_pad_*` cvars. A controller binding lives
//! under its own versioned action, `gamepad:v1:<action>`, beside the
//! keyboard's, so the lobby's Controller settings page and settings sync
//! read and write the same entries. The page's presets and rebinding stay
//! the page's.

use shagger::core::cvar::{CvarOptions, CvarType, Cvars};
use shagger::input::gamepad_input::pad_label;
use shagger::input::input_system::InputSystem;
use shagger::input::input_types::{ActionDefinition, InputBinding};

use crate::client::fps_actions::fps;

/// `controllerKey`: the versioned action namespace.
#[must_use]
pub fn controller_key(action: &str) -> String {
    format!("gamepad:v1:{action}")
}

/// One of `CONTROLLER_ACTIONS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerAction {
    pub id: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub button: u64,
}

const fn action(id: &'static str, name: &'static str, group: &'static str, button: u64) -> ControllerAction {
    ControllerAction {
        id,
        name,
        group,
        button,
    }
}

/// `CONTROLLER_ACTIONS`.
pub const CONTROLLER_ACTIONS: [ControllerAction; 15] = [
    action(fps::JUMP, "Jump", "move", 0),
    action(fps::CROUCH, "Crouch / slide", "move", 1),
    action(fps::SPRINT, "Sprint / walk", "move", 10),
    action(fps::FIRE, "Fire", "fight", 7),
    action(fps::AIM, "Aim", "fight", 6),
    action(fps::USE, "Use", "fight", 2),
    action(fps::RELOAD, "Reload", "fight", 3),
    action(fps::WEAPON_PREV, "Previous weapon", "weapons", 4),
    action(fps::WEAPON_NEXT, "Next weapon", "weapons", 5),
    action(fps::WEAPON_1, "Blade", "weapons", 11),
    action(fps::WEAPON_2, "Primary", "weapons", 14),
    action(fps::WEAPON_3, "Secondary", "weapons", 15),
    action(fps::READY, "Ready", "other", 12),
    action(fps::JOIN_GAME, "Join game", "other", 13),
    action(fps::SCOREBOARD, "Scoreboard (hold)", "other", 8),
];

/// `registerControllerSettings`: each action's controller binding.
pub fn register_controller_settings(input: &mut InputSystem) {
    for a in CONTROLLER_ACTIONS {
        input.register_action(ActionDefinition::button(
            &controller_key(a.id),
            a.name,
            vec![InputBinding {
                gamepad_button: Some(a.button),
                ..InputBinding::default()
            }],
        ));
    }
}

/// `controllerButtons`: the buttons bound to an action.
#[must_use]
pub fn controller_buttons(input: &InputSystem, action: &str) -> Vec<u64> {
    input
        .bindings_for_action(&controller_key(action))
        .iter()
        .filter_map(|b| b.gamepad_button)
        .collect()
}

/// `controllerLabel`.
#[must_use]
pub fn controller_label(input: &InputSystem, action: &str) -> String {
    let labels: Vec<String> = controller_buttons(input, action).into_iter().map(pad_label).collect();
    if labels.is_empty() {
        "Unbound".to_owned()
    } else {
        labels.join(" / ")
    }
}

/// The `in_pad_*` cvars (`Cvar.register` at the module's load).
pub fn register_controller_cvars(cvars: &mut Cvars) {
    let float = |d: &str, min: f64, max: f64| CvarOptions::describe(d).range(min, max).kind(CvarType::Float);
    let int = |d: &str| CvarOptions::describe(d).range(0.0, 1.0).kind(CvarType::Int);
    cvars.register(
        "in_pad_yaw",
        240.0,
        float("Controller horizontal turn speed (degrees per second)", 30.0, 720.0),
    );
    cvars.register(
        "in_pad_pitch",
        180.0,
        float("Controller vertical turn speed (degrees per second)", 30.0, 720.0),
    );
    cvars.register(
        "in_pad_curve",
        1.7,
        float("Controller look response exponent", 1.0, 3.0),
    );
    cvars.register(
        "in_pad_move_deadzone",
        0.15,
        float("Controller move stick dead zone", 0.05, 0.4),
    );
    cvars.register(
        "in_pad_look_deadzone",
        0.12,
        float("Controller look stick dead zone", 0.05, 0.4),
    );
    cvars.register(
        "in_pad_ads",
        0.5,
        float("Controller sensitivity while aiming", 0.1, 1.0),
    );
    cvars.register("in_pad_invert", 0, int("Invert controller vertical look"));
    cvars.register("in_pad_swap", 0, int("Swap controller move and look sticks"));
    cvars.register("in_pad_assist", 1, int("Controller aim assist"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_action_has_its_default_button() {
        let mut input = InputSystem::new(None);
        register_controller_settings(&mut input);
        assert_eq!(controller_buttons(&input, fps::FIRE), [7]);
        assert_eq!(controller_label(&input, fps::JUMP), "A");
        assert_eq!(controller_label(&input, fps::WEAPON_3), "D-pad Right");
        assert_eq!(controller_label(&input, fps::MOVE_FORWARD), "Unbound");
    }
}
