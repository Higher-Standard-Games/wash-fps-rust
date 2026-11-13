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

/// `CONTROLLER_CVARS`: what the Controller page's reset puts back.
pub const CONTROLLER_CVARS: [&str; 9] = [
    "in_pad_yaw",
    "in_pad_pitch",
    "in_pad_curve",
    "in_pad_move_deadzone",
    "in_pad_look_deadzone",
    "in_pad_ads",
    "in_pad_invert",
    "in_pad_swap",
    "in_pad_assist",
];

/// A preset of the Controller page's layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerPreset {
    Standard,
    /// Jump on the left bumper, Previous weapon on A.
    Bumper,
}

fn preset_button(preset: ControllerPreset, a: &ControllerAction) -> u64 {
    match preset {
        ControllerPreset::Bumper if a.id == fps::JUMP => 4,
        ControllerPreset::Bumper if a.id == fps::WEAPON_PREV => 0,
        _ => a.button,
    }
}

fn pad(button: u64) -> InputBinding {
    InputBinding {
        gamepad_button: Some(button),
        ..InputBinding::default()
    }
}

/// `assignControllerButton`: `button` to `action`, taken off any other
/// action that had it; `None` unbinds. Start (9) stays the menu's.
pub fn assign_controller_button(input: &mut InputSystem, action: &str, button: Option<u64>) {
    if button.is_some_and(|b| b > 15 || b == 9) || !CONTROLLER_ACTIONS.iter().any(|a| a.id == action) {
        return;
    }
    if let Some(button) = button {
        for other in CONTROLLER_ACTIONS {
            if other.id == action {
                continue;
            }
            let existing = controller_buttons(input, other.id);
            if existing.contains(&button) {
                let kept: Vec<InputBinding> = existing.into_iter().filter(|&b| b != button).map(pad).collect();
                input.rebind(&controller_key(other.id), &kept);
            }
        }
    }
    let bindings: Vec<InputBinding> = button.into_iter().map(pad).collect();
    input.rebind(&controller_key(action), &bindings);
}

/// `applyControllerPreset`.
pub fn apply_controller_preset(input: &mut InputSystem, preset: ControllerPreset) {
    for a in CONTROLLER_ACTIONS {
        input.rebind(&controller_key(a.id), &[pad(preset_button(preset, &a))]);
    }
}

/// `controllerPreset`: the preset the bindings match, if any.
#[must_use]
pub fn controller_preset(input: &InputSystem) -> Option<ControllerPreset> {
    [ControllerPreset::Standard, ControllerPreset::Bumper]
        .into_iter()
        .find(|&preset| {
            CONTROLLER_ACTIONS
                .iter()
                .all(|a| controller_buttons(input, a.id) == [preset_button(preset, a)])
        })
}

/// `resetController`.
pub fn reset_controller(input: &mut InputSystem, cvars: &mut Cvars) {
    for a in CONTROLLER_ACTIONS {
        input.reset_binding(&controller_key(a.id));
    }
    for name in CONTROLLER_CVARS {
        if cvars.exists(name) {
            cvars.reset(name);
        }
    }
}

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

    #[test]
    fn a_button_moves_to_the_action_it_is_given() {
        let mut input = InputSystem::new(None);
        register_controller_settings(&mut input);
        assert_eq!(controller_preset(&input), Some(ControllerPreset::Standard));
        assign_controller_button(&mut input, fps::FIRE, Some(0));
        assert_eq!(controller_buttons(&input, fps::FIRE), [0]);
        assert!(controller_buttons(&input, fps::JUMP).is_empty());
        assert_eq!(controller_preset(&input), None);
        assign_controller_button(&mut input, fps::FIRE, Some(9));
        assert_eq!(controller_buttons(&input, fps::FIRE), [0]);
        apply_controller_preset(&mut input, ControllerPreset::Bumper);
        assert_eq!(controller_preset(&input), Some(ControllerPreset::Bumper));
        assert_eq!(controller_buttons(&input, fps::JUMP), [4]);
    }
}
