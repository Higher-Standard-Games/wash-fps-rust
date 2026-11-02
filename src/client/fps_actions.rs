//! wash-fps `client/FPSActions.ts`: the game's input actions, their default
//! keys, the gameplay and spectator contexts, and the key labels the HUD shows.

use shagger::input::input_system::InputSystem;
use shagger::input::input_types::{ActionDefinition, ContextDefinition, InputBinding, MouseButton, ScrollDirection};

/// `FPS`: the action ids.
pub mod fps {
    pub const MOVE_FORWARD: &str = "fps_move_forward";
    pub const MOVE_BACKWARD: &str = "fps_move_backward";
    pub const MOVE_LEFT: &str = "fps_move_left";
    pub const MOVE_RIGHT: &str = "fps_move_right";
    pub const JUMP: &str = "fps_jump";
    pub const CROUCH: &str = "fps_crouch";
    pub const SPRINT: &str = "fps_sprint";
    pub const FLY_UP: &str = "fps_fly_up";
    pub const FLY_DOWN: &str = "fps_fly_down";
    pub const FIRE: &str = "fps_fire";
    pub const AIM: &str = "fps_aim";
    pub const RELOAD: &str = "fps_reload";
    pub const USE: &str = "fps_use";
    pub const WEAPON_1: &str = "fps_weapon_1";
    pub const WEAPON_2: &str = "fps_weapon_2";
    pub const WEAPON_3: &str = "fps_weapon_3";
    pub const WEAPON_4: &str = "fps_weapon_4";
    pub const WEAPON_5: &str = "fps_weapon_5";
    pub const WEAPON_6: &str = "fps_weapon_6";
    pub const WEAPON_7: &str = "fps_weapon_7";
    pub const WEAPON_8: &str = "fps_weapon_8";
    pub const WEAPON_9: &str = "fps_weapon_9";
    pub const WEAPON_NEXT: &str = "fps_weapon_next";
    pub const WEAPON_PREV: &str = "fps_weapon_prev";
    pub const SCOREBOARD: &str = "fps_scoreboard";
    pub const CHAT: &str = "fps_chat";
    pub const TEAM_CHAT: &str = "fps_team_chat";
    pub const READY: &str = "fps_ready";
    pub const JOIN_GAME: &str = "fps_join_game";
    pub const CONSOLE: &str = "fps_console";

    /// `fps_weapon_1` to `fps_weapon_9`.
    pub const WEAPONS: [&str; 9] = [
        WEAPON_1, WEAPON_2, WEAPON_3, WEAPON_4, WEAPON_5, WEAPON_6, WEAPON_7, WEAPON_8, WEAPON_9,
    ];
}

fn keys(codes: &[&str]) -> Vec<InputBinding> {
    codes.iter().map(|c| InputBinding::key(c)).collect()
}

/// `FPS_ACTION_DEFS`.
#[must_use]
pub fn fps_action_defs() -> Vec<ActionDefinition> {
    use fps::*;
    let button = ActionDefinition::button;
    let mut defs = vec![
        button(MOVE_FORWARD, "Move Forward", keys(&["KeyW"])).lock_in_fullscreen(),
        button(MOVE_BACKWARD, "Move Backward", keys(&["KeyS"])).lock_in_fullscreen(),
        button(MOVE_LEFT, "Strafe Left", keys(&["KeyA"])).lock_in_fullscreen(),
        button(MOVE_RIGHT, "Strafe Right", keys(&["KeyD"])).lock_in_fullscreen(),
        button(JUMP, "Jump", keys(&["Space"])),
        button(
            CROUCH,
            "Crouch",
            [InputBinding::mouse(MouseButton::Middle)]
                .into_iter()
                .chain(keys(&["KeyC", "ControlLeft", "ControlRight"]))
                .collect(),
        ),
        button(SPRINT, "Sprint", keys(&["ShiftLeft", "ShiftRight"])),
        button(FLY_UP, "Fly Up (Spectator)", keys(&["Space"])),
        button(FLY_DOWN, "Fly Down (Spectator)", keys(&["ShiftLeft", "ShiftRight"])),
        button(FIRE, "Fire", vec![InputBinding::mouse(MouseButton::Left)]),
        button(AIM, "Aim / ADS", vec![InputBinding::mouse(MouseButton::Right)]),
        button(RELOAD, "Reload", keys(&["KeyR"])),
        button(USE, "Use / Interact", keys(&["KeyF"])),
        button(WEAPON_1, "Weapon 1 (Blade)", keys(&["Digit1", "KeyQ"])),
    ];
    for (i, id) in WEAPONS.iter().enumerate().skip(1) {
        let n = i + 1;
        defs.push(button(id, &format!("Weapon {n}"), keys(&[&format!("Digit{n}")])));
    }
    defs.extend([
        button(
            WEAPON_NEXT,
            "Next Weapon",
            vec![InputBinding::scroll(ScrollDirection::Down)],
        ),
        button(
            WEAPON_PREV,
            "Prev Weapon",
            vec![InputBinding::scroll(ScrollDirection::Up)],
        ),
        button(SCOREBOARD, "Scoreboard", keys(&["Tab"])),
        button(CHAT, "Chat", keys(&["KeyT"])),
        button(TEAM_CHAT, "Team chat", keys(&["KeyY"])),
        button(READY, "Ready Up", keys(&["F4"])),
        button(JOIN_GAME, "Join Game", keys(&["KeyJ"])),
        button(CONSOLE, "Console", keys(&["Backquote"])),
    ]);
    defs
}

fn context(id: &str, actions: &[&str]) -> ContextDefinition {
    ContextDefinition {
        id: id.to_owned(),
        blocking: false,
        actions: actions.iter().map(|a| (*a).to_owned()).collect(),
        bindings: Vec::new(),
    }
}

/// `FPS_GAMEPLAY_CONTEXT`: every combat and movement action.
pub const GAMEPLAY_CONTEXT: &str = "fps_gameplay";
/// `FPS_SPECTATOR_CONTEXT`: movement and the HUD, no combat.
pub const SPECTATOR_CONTEXT: &str = "fps_spectator";

/// `registerFPSActions`: the actions and contexts, gameplay active.
pub fn register_fps_actions(input: &mut InputSystem) {
    use fps::*;
    for def in fps_action_defs() {
        input.register_action(def);
    }
    input.register_context(context(
        GAMEPLAY_CONTEXT,
        &[
            MOVE_FORWARD,
            MOVE_BACKWARD,
            MOVE_LEFT,
            MOVE_RIGHT,
            JUMP,
            CROUCH,
            SPRINT,
            FIRE,
            AIM,
            RELOAD,
            USE,
            WEAPON_1,
            WEAPON_2,
            WEAPON_3,
            WEAPON_4,
            WEAPON_5,
            WEAPON_6,
            WEAPON_7,
            WEAPON_8,
            WEAPON_9,
            WEAPON_NEXT,
            WEAPON_PREV,
            SCOREBOARD,
            CHAT,
            TEAM_CHAT,
            READY,
            CONSOLE,
        ],
    ));
    input.register_context(context(
        SPECTATOR_CONTEXT,
        &[
            MOVE_FORWARD,
            MOVE_BACKWARD,
            MOVE_LEFT,
            MOVE_RIGHT,
            FLY_UP,
            FLY_DOWN,
            SPRINT,
            SCOREBOARD,
            CHAT,
            TEAM_CHAT,
            JOIN_GAME,
            CONSOLE,
        ],
    ));
    if !input.is_context_active(GAMEPLAY_CONTEXT) {
        input.push_context(GAMEPLAY_CONTEXT);
    }
}

/// `getWeaponSlotPressed`: 1 to 9 for a weapon key pressed this frame, else 0.
#[must_use]
pub fn weapon_slot_pressed(input: &InputSystem) -> u8 {
    fps::WEAPONS
        .iter()
        .position(|id| input.was_pressed(id))
        .map_or(0, |i| i as u8 + 1)
}

/// `getKeyLabel`: a key code as the HUD writes it.
#[must_use]
pub fn key_label(code: &str) -> String {
    if let Some(rest) = code.strip_prefix("Key") {
        return rest.to_owned();
    }
    if let Some(rest) = code.strip_prefix("Digit") {
        return rest.to_owned();
    }
    let fixed = match code {
        "Space" => "SPACE",
        "ShiftLeft" | "ShiftRight" => "SHIFT",
        "ControlLeft" | "ControlRight" => "CTRL",
        "AltLeft" | "AltRight" => "ALT",
        "Backquote" => "`",
        "Tab" => "TAB",
        "Escape" => "ESC",
        "Enter" => "ENTER",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_owned();
    }
    if let Some(rest) = code.strip_prefix("Arrow") {
        return rest.to_uppercase();
    }
    code.to_owned()
}

/// `getActionKeyLabel`: the first binding of an action.
#[must_use]
pub fn action_key_label(input: &InputSystem, id: &str) -> String {
    let Some(b) = input.bindings_for_action(id).first() else {
        return "?".to_owned();
    };
    if let Some(key) = &b.key {
        return key_label(key);
    }
    match b.mouse_button {
        Some(MouseButton::Left) => "MOUSE1",
        Some(MouseButton::Right) => "MOUSE2",
        Some(MouseButton::Middle) => "MOUSE3",
        None => "?",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_keys_drive_the_gameplay_actions() {
        let mut input = InputSystem::new(None);
        register_fps_actions(&mut input);
        assert_eq!(input.active_context(), GAMEPLAY_CONTEXT);
        input.key_down("KeyQ");
        input.key_down("ControlRight");
        input.update();
        assert_eq!(weapon_slot_pressed(&input), 1);
        assert!(input.is_active(fps::CROUCH));
        assert_eq!(action_key_label(&input, fps::CROUCH), "MOUSE3");
        assert_eq!(action_key_label(&input, fps::SPRINT), "SHIFT");
        assert_eq!(key_label("ArrowUp"), "UP");
        assert_eq!(key_label("F4"), "F4");
        assert_eq!(fps_action_defs().len(), 30);
    }
}
