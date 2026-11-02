//! wash-fps `client/settings/SettingsModel.ts`, the pages and their cvars.
//! Every choice writes the game's own cvars; nothing here is a setting of
//! its own. The keys half reads and writes the input system's bindings.
//!
//! [`register_client_cvars`] stands in for the browser's scattered
//! `Cvar.register` calls at module load, naming each owner.

use shagger::audio::audio_cvars::register_audio_cvars;
use shagger::core::cvar::{CvarOptions, CvarType, Cvars};
use shagger::input::input_system::InputSystem;
use shagger::input::input_types::{InputBinding, MouseButton, ScrollDirection};
use shagger::movement::movement_vars::register_movement_vars;

use crate::client::crosshair::register_crosshair_cvars;
use crate::client::fps_actions::fps;

// ═══════════════════════════════ KEYS ═══════════════════════════════

/// `ActionGroupId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionGroupId {
    Move,
    Weapons,
    Fight,
    Other,
}

/// `ActionGroup`: its name and the colour its keys, chips and list are
/// drawn in, as HSL.
#[derive(Debug, Clone, Copy)]
pub struct ActionGroup {
    pub id: ActionGroupId,
    pub name: &'static str,
    pub hsl: [f32; 3],
}

/// `ACTION_GROUPS`.
pub const ACTION_GROUPS: [ActionGroup; 4] = [
    ActionGroup {
        id: ActionGroupId::Move,
        name: "Move",
        hsl: [182.0, 0.95, 0.52],
    },
    ActionGroup {
        id: ActionGroupId::Weapons,
        name: "Weapons",
        hsl: [46.0, 1.0, 0.57],
    },
    ActionGroup {
        id: ActionGroupId::Fight,
        name: "Fight",
        hsl: [322.0, 0.95, 0.62],
    },
    ActionGroup {
        id: ActionGroupId::Other,
        name: "Other",
        hsl: [276.0, 0.9, 0.7],
    },
];

/// `SettingsAction.fixed`: the keys the game reads itself, and what they do.
#[derive(Debug, Clone, Copy)]
pub struct Fixed {
    pub codes: &'static [&'static str],
    pub does: &'static str,
}

/// `SettingsAction`.
#[derive(Debug, Clone, Copy)]
pub struct SettingsAction {
    pub id: &'static str,
    pub group: ActionGroupId,
    pub name: &'static str,
    /// Written on a key where the name fits, and where it does not.
    pub on_key: Option<&'static str>,
    pub on_narrow_key: Option<&'static str>,
    pub fixed: Option<Fixed>,
}

const fn act(id: &'static str, group: ActionGroupId, name: &'static str) -> SettingsAction {
    SettingsAction {
        id,
        group,
        name,
        on_key: None,
        on_narrow_key: None,
        fixed: None,
    }
}

const fn keyed(mut a: SettingsAction, on_key: Option<&'static str>, narrow: Option<&'static str>) -> SettingsAction {
    a.on_key = on_key;
    a.on_narrow_key = narrow;
    a
}

const fn fixed(mut a: SettingsAction, codes: &'static [&'static str], does: &'static str) -> SettingsAction {
    a.fixed = Some(Fixed { codes, does });
    a
}

/// `SETTINGS_ACTIONS`: only what the game acts on. A player carries three
/// weapons, so the weapon keys stop at three; the scoreboard, chat, console
/// and join keys are read directly by the game, so they show as fixed.
pub const SETTINGS_ACTIONS: [SettingsAction; 23] = {
    use ActionGroupId::{Fight, Move, Other, Weapons};
    [
        keyed(act(fps::MOVE_FORWARD, Move, "Forward"), None, Some("Fwd")),
        act(fps::MOVE_BACKWARD, Move, "Back"),
        act(fps::MOVE_LEFT, Move, "Left"),
        act(fps::MOVE_RIGHT, Move, "Right"),
        act(fps::JUMP, Move, "Jump"),
        keyed(act(fps::CROUCH, Move, "Crouch / slide"), Some("Crouch"), Some("Crch")),
        keyed(act(fps::SPRINT, Move, "Sprint"), None, Some("Sprnt")),
        act(fps::WEAPON_1, Weapons, "Blade"),
        keyed(act(fps::WEAPON_2, Weapons, "Primary"), None, Some("Pri")),
        keyed(act(fps::WEAPON_3, Weapons, "Secondary"), Some("Second."), Some("Sec")),
        keyed(
            act(fps::WEAPON_NEXT, Weapons, "Next weapon"),
            Some("Next wpn"),
            Some("Next"),
        ),
        keyed(
            act(fps::WEAPON_PREV, Weapons, "Previous weapon"),
            Some("Prev wpn"),
            Some("Prev"),
        ),
        act(fps::FIRE, Fight, "Fire"),
        act(fps::AIM, Fight, "Aim"),
        keyed(act(fps::RELOAD, Fight, "Reload"), None, Some("Rld")),
        keyed(act(fps::USE, Fight, "Use"), Some("Use"), None),
        act(fps::READY, Other, "Ready"),
        fixed(
            keyed(act("cancel", Other, "Menu / cancel"), Some("Menu"), None),
            &["Escape"],
            "opens the match menu or cancels a dialog",
        ),
        fixed(
            keyed(act(fps::SCOREBOARD, Other, "Scoreboard"), Some("Scores"), Some("Score")),
            &["Tab"],
            "opens the scoreboard",
        ),
        fixed(act(fps::CHAT, Other, "Chat"), &["KeyT", "Enter"], "opens chat"),
        fixed(act(fps::TEAM_CHAT, Other, "Team chat"), &["KeyY"], "opens team chat"),
        fixed(
            keyed(act(fps::CONSOLE, Other, "Console"), None, Some("Con")),
            &["Backquote"],
            "opens the console",
        ),
        fixed(
            keyed(act(fps::JOIN_GAME, Other, "Join game"), Some("Join"), None),
            &["KeyJ"],
            "joins the game",
        ),
    ]
};

/// Every listed action, in order.
pub fn settings_actions() -> impl Iterator<Item = &'static SettingsAction> {
    SETTINGS_ACTIONS.iter()
}

/// `UNLISTED_ACTIONS`: slots four to nine, which hold keys by default.
const UNLISTED_ACTIONS: [&str; 6] = [
    fps::WEAPON_4,
    fps::WEAPON_5,
    fps::WEAPON_6,
    fps::WEAPON_7,
    fps::WEAPON_8,
    fps::WEAPON_9,
];

fn action_by_id(id: &str) -> Option<&'static SettingsAction> {
    settings_actions().find(|a| a.id == id)
}

/// `groupOf`.
#[must_use]
pub fn group_of(action: &SettingsAction) -> &'static ActionGroup {
    ACTION_GROUPS
        .iter()
        .find(|g| g.id == action.group)
        .unwrap_or(&ACTION_GROUPS[0])
}

/// `FIXED_KEYS.get(code)`: the fixed action that owns a key.
#[must_use]
pub fn fixed_key(code: &str) -> Option<&'static SettingsAction> {
    settings_actions().find(|a| a.fixed.is_some_and(|f| f.codes.contains(&code)))
}

/// `KeyCap`: code, legend, and width in key units.
pub type KeyCap = (String, String, f32);

fn cap(code: &str, legend: &str, width: f32) -> KeyCap {
    (code.to_owned(), legend.to_owned(), width)
}

fn letters(text: &str) -> Vec<KeyCap> {
    text.chars()
        .map(|c| cap(&format!("Key{c}"), &c.to_string(), 1.0))
        .collect()
}

/// `KEYBOARD_ROWS`: fifteen keys wide, the arrows tucked into the bottom
/// row like a laptop's. Legends and chips are written out where the
/// TypeScript draws a symbol (⌫, the arrows) that Washoi's fonts lack.
#[must_use]
pub fn keyboard_rows() -> Vec<Vec<KeyCap>> {
    let mut f = vec![cap("Escape", "Esc", 1.0)];
    f.extend((1..=12).map(|i| cap(&format!("F{i}"), &format!("F{i}"), 1.0)));
    let mut digits = vec![cap("Backquote", "`", 1.0)];
    digits.extend([1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map(|n| cap(&format!("Digit{n}"), &n.to_string(), 1.0)));
    digits.extend([
        cap("Minus", "-", 1.0),
        cap("Equal", "=", 1.0),
        cap("Backspace", "Bksp", 2.0),
    ]);
    let mut top = vec![cap("Tab", "Tab", 1.5)];
    top.extend(letters("QWERTYUIOP"));
    top.extend([
        cap("BracketLeft", "[", 1.0),
        cap("BracketRight", "]", 1.0),
        cap("Backslash", "\\", 1.5),
    ]);
    let mut home = vec![cap("CapsLock", "Caps", 1.75)];
    home.extend(letters("ASDFGHJKL"));
    home.extend([
        cap("Semicolon", ";", 1.0),
        cap("Quote", "'", 1.0),
        cap("Enter", "Enter", 2.25),
    ]);
    let mut bottom = vec![cap("ShiftLeft", "Shift", 2.25)];
    bottom.extend(letters("ZXCVBNM"));
    bottom.extend([
        cap("Comma", ",", 1.0),
        cap("Period", ".", 1.0),
        cap("Slash", "/", 1.0),
        cap("ShiftRight", "Shift", 2.75),
    ]);
    let space = vec![
        cap("ControlLeft", "Ctrl", 1.25),
        cap("MetaLeft", "Win", 1.25),
        cap("AltLeft", "Alt", 1.25),
        cap("Space", "Space", 5.75),
        cap("AltRight", "Alt", 1.25),
        cap("ControlRight", "Ctrl", 1.25),
    ];
    vec![f, digits, top, home, bottom, space]
}

/// `ARROW_KEYS`.
pub const ARROW_KEYS: [(&str, &str); 4] = [
    ("ArrowUp", "Up"),
    ("ArrowLeft", "Left"),
    ("ArrowDown", "Down"),
    ("ArrowRight", "Right"),
];

/// `MOUSE_INPUTS`.
pub const MOUSE_INPUTS: [(&str, &str); 5] = [
    ("mouse:left", "Left click"),
    ("mouse:right", "Right click"),
    ("wheel:up", "Wheel up"),
    ("mouse:middle", "Wheel click"),
    ("wheel:down", "Wheel down"),
];

fn legend(code: &str) -> Option<String> {
    keyboard_rows()
        .into_iter()
        .flatten()
        .find(|(c, ..)| c == code)
        .map(|(_, l, _)| l)
        .or_else(|| {
            ARROW_KEYS
                .iter()
                .find(|(c, _)| *c == code)
                .map(|(_, l)| (*l).to_owned())
        })
}

/// `codeName`: what a key is called on a chip.
#[must_use]
pub fn code_name(code: &str) -> String {
    let chip = match code {
        "ShiftLeft" => "L-Shift",
        "ShiftRight" => "R-Shift",
        "ControlLeft" => "L-Ctrl",
        "ControlRight" => "R-Ctrl",
        "AltLeft" => "L-Alt",
        "AltRight" => "R-Alt",
        "mouse:left" => "L-Click",
        "mouse:right" => "R-Click",
        "mouse:middle" => "Wheel",
        "wheel:up" => "Wheel up",
        "wheel:down" => "Wheel dn",
        _ => "",
    };
    if !chip.is_empty() {
        return chip.to_owned();
    }
    legend(code).unwrap_or_else(|| {
        code.strip_prefix("Key")
            .or_else(|| code.strip_prefix("Digit"))
            .unwrap_or(code)
            .to_owned()
    })
}

/// `inputName`: the legend on the cap, or the mouse part.
#[must_use]
pub fn input_name(code: &str) -> String {
    MOUSE_INPUTS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| (*n).to_owned())
        .or_else(|| legend(code))
        .unwrap_or_else(|| code_name(code))
}

/// `bindingToCode`.
#[must_use]
pub fn binding_to_code(binding: &InputBinding) -> Option<String> {
    if binding.modifiers.is_some() {
        return None;
    }
    if let Some(key) = &binding.key {
        return Some(key.clone());
    }
    if let Some(button) = binding.mouse_button {
        return Some(format!("mouse:{}", button.name()));
    }
    binding.scroll_direction.map(|d| format!("wheel:{}", d.name()))
}

/// `codeToBinding`.
#[must_use]
pub fn code_to_binding(code: &str) -> InputBinding {
    if let Some(button) = code.strip_prefix("mouse:").and_then(MouseButton::parse) {
        return InputBinding::mouse(button);
    }
    if let Some(direction) = code.strip_prefix("wheel:").and_then(ScrollDirection::parse) {
        return InputBinding::scroll(direction);
    }
    InputBinding::key(code)
}

fn codes_of(bindings: &[InputBinding]) -> Vec<String> {
    bindings.iter().filter_map(binding_to_code).collect()
}

/// `codesFor`: the keys an action answers to, in order. A fixed action
/// answers to the game's keys whatever was saved for it.
#[must_use]
pub fn codes_for(id: &str, input: &InputSystem) -> Vec<String> {
    if let Some(f) = action_by_id(id).and_then(|a| a.fixed) {
        return f.codes.iter().map(|c| (*c).to_owned()).collect();
    }
    let mut codes: Vec<String> = Vec::new();
    for code in codes_of(input.bindings_for_action(id)) {
        if !codes.contains(&code) && fixed_key(&code).is_none() {
            codes.push(code);
        }
    }
    codes
}

/// `actionFor`: the listed action a key does.
#[must_use]
pub fn action_for(code: &str, input: &InputSystem) -> Option<&'static SettingsAction> {
    fixed_key(code)
        .or_else(|| settings_actions().find(|a| a.fixed.is_none() && codes_for(a.id, input).iter().any(|c| c == code)))
}

fn set_codes(id: &str, codes: &[String], input: &mut InputSystem) {
    // Matching the defaults again is stored as "no override", so a later
    // change to the defaults reaches this player too.
    if codes == codes_of(input.default_bindings(id)) {
        input.reset_binding(id);
    } else {
        let bindings: Vec<InputBinding> = codes.iter().map(|c| code_to_binding(c)).collect();
        input.rebind(id, &bindings);
    }
}

/// `AssignResult`.
#[derive(Debug, Clone, Copy)]
pub enum AssignResult {
    Fixed(&'static SettingsAction),
    Cleared {
        previous: Option<&'static SettingsAction>,
    },
    Assigned {
        action: &'static SettingsAction,
        previous: Option<&'static SettingsAction>,
    },
}

/// `assignKey`: give `code` to `action_id`, or to nothing. A key does one
/// thing: giving it to an action takes it from whichever action had it.
pub fn assign_key(code: &str, action_id: Option<&str>, input: &mut InputSystem) -> AssignResult {
    if let Some(f) = fixed_key(code) {
        return AssignResult::Fixed(f);
    }
    let previous = action_for(code, input);
    let ids: Vec<&str> = settings_actions()
        .filter(|a| a.fixed.is_none())
        .map(|a| a.id)
        .chain(UNLISTED_ACTIONS)
        .collect();
    for id in ids {
        let current = codes_of(input.bindings_for_action(id));
        if current.iter().any(|c| c == code) {
            let rest: Vec<String> = current.into_iter().filter(|c| c != code).collect();
            set_codes(id, &rest, input);
        }
    }
    let Some(action) = action_id.and_then(action_by_id).filter(|a| a.fixed.is_none()) else {
        return AssignResult::Cleared { previous };
    };
    let mut codes = codes_for(action.id, input);
    codes.push(code.to_owned());
    set_codes(action.id, &codes, input);
    AssignResult::Assigned { action, previous }
}

/// `resetKeys`: every listed action, and the slots it hides, back to the
/// game's keys.
pub fn reset_keys(input: &mut InputSystem) {
    for action in settings_actions() {
        input.reset_binding(action.id);
    }
    for id in UNLISTED_ACTIONS {
        input.reset_binding(id);
    }
}

// ═══════════════════════════════ CVARS ═══════════════════════════════

/// `SettingsPageId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsPageId {
    Controls,
    Gameplay,
    Video,
    Audio,
}

/// `SETTINGS_PAGES`: id, title, hint.
pub const SETTINGS_PAGES: [(SettingsPageId, &str, &str); 4] = [
    (SettingsPageId::Controls, "Controls", "Keys · mouse"),
    (SettingsPageId::Gameplay, "Gameplay", "Aim · colours"),
    (SettingsPageId::Video, "Video", "Quality"),
    (SettingsPageId::Audio, "Audio", "Volume"),
];

/// `PAGE_CVARS`: the cvars each page's reset puts back.
#[must_use]
pub fn page_cvars(page: SettingsPageId) -> &'static [&'static str] {
    match page {
        SettingsPageId::Controls => &["m_sensitivity", "cl_ads_sensitivity", "cl_autorun"],
        SettingsPageId::Gameplay => &[
            "cl_team_ally_sounds",
            "cl_team_enemy_sounds",
            "cl_team_ally_color",
            "cl_team_ally_outline_color",
            "cl_team_enemy_color",
            "cl_team_enemy_outline_color",
            "cl_team_ffa_color",
            "cl_team_ffa_outline_color",
            "cl_player_outline",
            "cl_player_outline_size",
            "cl_player_fullbright",
            "cl_player_fullbright_intensity",
            "cg_crosshair",
            "cg_crosshairSize",
            "cg_crosshairAlpha",
            "cg_crosshairColor",
            "cg_crosshairSpread",
            "cg_crosshairDmgColor",
            "cl_fov",
            "show_fps",
            "cl_showspeed",
            "cl_hints",
            "cl_invertClock",
            "cl_autoswitch",
            "cl_netgraph",
        ],
        SettingsPageId::Video => &[
            "r_picmip",
            "r_bloom",
            "r_glow",
            "r_ssr",
            "r_ssr_strength",
            "r_anisotropy",
            "r_godrays",
            "r_godrays_samples",
            "r_softhalos",
            "r_water_quality",
            "r_water_actors",
            "r_arealights",
            "r_bsp_pbr",
            "r_cel_outlines",
            "r_exposure",
            "r_contrast",
            "r_underwater_distortion",
        ],
        SettingsPageId::Audio => &[
            "s_musicenabled",
            "s_volume",
            "s_sfxenabled",
            "s_sfxvolume",
            "cg_volume_effects",
            "cg_volume_players",
            "cg_volume_hitsound",
            "cg_volume_announcer",
        ],
    }
}

/// `resetPage`: the page's cvars, and on the controls page the keys.
pub fn reset_page(cvars: &mut Cvars, input: &mut InputSystem, page: SettingsPageId) {
    for name in page_cvars(page) {
        if cvars.exists(name) {
            cvars.reset(name);
        }
    }
    if page == SettingsPageId::Controls {
        reset_keys(input);
    }
}

fn int_flag(cvars: &mut Cvars, name: &str, default: i32, description: &str) {
    cvars.register(
        name,
        default,
        CvarOptions::describe(description).range(0.0, 1.0).kind(CvarType::Int),
    );
}

/// The cvars this client reads, as their owners register them.
pub fn register_client_cvars(cvars: &mut Cvars) {
    // shagger: MovementVars.ts, AudioManager.ts.
    register_movement_vars(cvars);
    register_audio_cvars(cvars);
    // Crosshair.ts.
    register_crosshair_cvars(cvars);
    // Game.ts.
    int_flag(
        cvars,
        "cl_autoswitch",
        1,
        "Switch to a weapon when you pick it up. 0 keeps what you hold, blade included",
    );
    // hud/widgets/HintPanel.ts.
    int_flag(
        cvars,
        "cl_hints",
        1,
        "Show hints for new players (movement, weapons, items), above your weapons",
    );
    // MatchStateManager.ts.
    int_flag(
        cvars,
        "cl_invertClock",
        0,
        "Match clock counts up from 0:00 (time played) instead of down (time left)",
    );
    // SHAG core/FpsDisplaySettings.ts.
    int_flag(cvars, "show_fps", 1, "Show the FPS counter");
    // SHAG network/client/NetGraph.ts.
    cvars.register(
        "cl_netgraph",
        0,
        CvarOptions::describe("Network graph overlay (0=off, 1=text, 2=text+graph)")
            .range(0.0, 2.0)
            .kind(CvarType::Int),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_reset_puts_its_cvars_back() {
        let mut cvars = Cvars::new(None, false);
        register_client_cvars(&mut cvars);
        cvars.set("cl_fov", 105);
        cvars.set("s_volume", 0.1);
        reset_page(&mut cvars, &mut InputSystem::new(None), SettingsPageId::Gameplay);
        assert_eq!(cvars.get_number("cl_fov"), 90.0);
        assert_eq!(cvars.get_number("s_volume"), 0.1);
    }

    fn input() -> InputSystem {
        let mut input = InputSystem::new(None);
        crate::client::fps_actions::register_fps_actions(&mut input);
        input
    }

    #[test]
    fn a_key_does_one_thing() {
        let mut input = input();
        // Q is the blade's second key; giving it to Reload takes it away.
        let result = assign_key("KeyQ", Some(fps::RELOAD), &mut input);
        assert!(matches!(
            result,
            AssignResult::Assigned { action, previous: Some(p) } if action.id == fps::RELOAD && p.id == fps::WEAPON_1
        ));
        assert_eq!(codes_for(fps::RELOAD, &input), ["KeyR", "KeyQ"]);
        assert_eq!(codes_for(fps::WEAPON_1, &input), ["Digit1"]);
        // Digit4 is slot four's, which the page does not list.
        assign_key("Digit4", Some(fps::JUMP), &mut input);
        assert!(input.bindings_for_action(fps::WEAPON_4).is_empty());
        assert!(matches!(
            assign_key("Tab", Some(fps::JUMP), &mut input),
            AssignResult::Fixed(_)
        ));
        assert!(matches!(
            assign_key("KeyQ", None, &mut input),
            AssignResult::Cleared { previous: Some(p) } if p.id == fps::RELOAD
        ));
        // Back to the defaults is no override at all.
        assert_eq!(codes_for(fps::RELOAD, &input), ["KeyR"]);
        assert!(!input.bindings_json().contains(fps::RELOAD));
        assert_eq!(code_name("ShiftLeft"), "L-Shift");
        assert_eq!(code_name("KeyW"), "W");
        assert_eq!(input_name("wheel:up"), "Wheel up");
        assert_eq!(
            codes_for(fps::CROUCH, &input),
            ["mouse:middle", "KeyC", "ControlLeft", "ControlRight"]
        );
        reset_keys(&mut input);
        assert_eq!(input.bindings_json(), "{}");
    }
}
