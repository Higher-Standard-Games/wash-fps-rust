//! wash-fps `client/settings/SettingsModel.ts`, the pages and their cvars.
//! Every choice writes the game's own cvars; nothing here is a setting of
//! its own. The keys half reads and writes the input system's bindings.
//!
//! [`register_client_cvars`] stands in for the browser's scattered
//! `Cvar.register` calls at module load, naming each owner.

use shagger::audio::audio_cvars::register_audio_cvars;
use shagger::core::cvar::{CvarOptions, CvarType, CvarValue, Cvars};
use shagger::input::input_system::InputSystem;
use shagger::input::input_types::{InputBinding, MouseButton, ScrollDirection};
use shagger::movement::movement_vars::register_movement_vars;

use shagger::rendering::render_vars::register_render_vars;
use shagger::rendering::world_fog_controller::register_fog_cvars;

use crate::client::crosshair::register_crosshair_cvars;
use crate::client::fps_actions::fps;
use crate::client::fps_player_audio::register_player_audio_cvars;
use crate::client::player_model_visual_settings::register_player_model_visual_cvars;

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

/// `CvarChoice`: a choice that sets several cvars at once, and knows when
/// they are all set. Every value the screen's choices set is a number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CvarChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub values: &'static [(&'static str, f64)],
}

/// `valueMatches`, for a wanted number.
fn value_matches(current: Option<&CvarValue>, wanted: f64) -> bool {
    matches!(current, Some(CvarValue::Number(n)) if (n - wanted).abs() < 1e-6)
}

/// `currentChoice`: the choice whose every value is set now, or `None` for
/// a mix of one's own.
#[must_use]
pub fn current_choice<'a>(cvars: &Cvars, choices: &'a [CvarChoice]) -> Option<&'a CvarChoice> {
    choices
        .iter()
        .find(|c| c.values.iter().all(|(name, v)| value_matches(cvars.get(name), *v)))
}

/// `applyChoice`.
pub fn apply_choice(cvars: &mut Cvars, choice: &CvarChoice) {
    for (name, value) in choice.values {
        cvars.set(name, *value);
    }
}

// ── Video presets ──

/// `VIDEO_PRESETS`: what costs frames; brightness, outlines and the like are
/// left alone. Balanced is the game's own defaults. With each preset's line
/// and meter bars.
pub const VIDEO_PRESETS: [(CvarChoice, &str, u8); 3] = [
    (
        CvarChoice {
            id: "smooth",
            label: "Smooth",
            values: &[
                ("r_bloom", 0.0),
                ("r_glow", 0.0),
                ("r_ssr", 0.0),
                ("r_ssr_strength", 0.4),
                ("r_anisotropy", 4.0),
                ("r_picmip", 1.0),
                ("r_godrays", 0.0),
                ("r_softhalos", 0.0),
                ("r_water_quality", 1.0),
                ("r_water_actors", 0.0),
                ("r_arealights", 0.0),
                ("r_bsp_pbr", 0.0),
            ],
        },
        "The most frames. Plain water, no light shafts, no bloom, hard-edged halos.",
        1,
    ),
    (
        CvarChoice {
            id: "balanced",
            label: "Balanced",
            values: &[
                ("r_bloom", 0.0),
                ("r_glow", 0.0),
                ("r_ssr", 0.0),
                ("r_ssr_strength", 0.4),
                ("r_anisotropy", 4.0),
                ("r_picmip", 0.0),
                ("r_godrays", 1.0),
                ("r_godrays_samples", 60.0),
                ("r_softhalos", 1.0),
                ("r_water_quality", 2.0),
                ("r_water_actors", 1.0),
                ("r_arealights", 1.0),
                ("r_bsp_pbr", 1.0),
            ],
        },
        "The look the game ships with.",
        2,
    ),
    (
        CvarChoice {
            id: "pretty",
            label: "Pretty",
            values: &[
                ("r_bloom", 1.3),
                // A hint of glow on lit strips and signs; more turns every
                // lamp into a blurred blob.
                ("r_glow", 0.35),
                ("r_ssr", 1.0),
                ("r_ssr_strength", 0.5),
                ("r_anisotropy", 16.0),
                ("r_picmip", 0.0),
                ("r_godrays", 1.0),
                ("r_godrays_samples", 60.0),
                ("r_softhalos", 1.0),
                ("r_water_quality", 3.0),
                ("r_water_actors", 1.0),
                ("r_arealights", 1.0),
                ("r_bsp_pbr", 1.0),
            ],
        },
        "Adds glow, reflections and see-through water. Wants a strong graphics card.",
        3,
    ),
];

/// `GODRAY_CHOICES`: light shafts, one choice over the gate and the samples.
pub const GODRAY_CHOICES: [CvarChoice; 3] = [
    CvarChoice {
        id: "off",
        label: "Off",
        values: &[("r_godrays", 0.0)],
    },
    CvarChoice {
        id: "low",
        label: "Low",
        values: &[("r_godrays", 1.0), ("r_godrays_samples", 30.0)],
    },
    CvarChoice {
        id: "high",
        label: "High",
        values: &[("r_godrays", 1.0), ("r_godrays_samples", 60.0)],
    },
];

fn number(cvars: &Cvars, name: &str) -> f64 {
    if cvars.exists(name) {
        cvars.get_number(name)
    } else {
        0.0
    }
}

/// `godrayChoice`: off whatever the sample count; on, fewer samples than
/// halfway to high is low.
#[must_use]
pub fn godray_choice(cvars: &Cvars) -> &'static CvarChoice {
    if number(cvars, "r_godrays") < 0.5 {
        &GODRAY_CHOICES[0]
    } else if number(cvars, "r_godrays_samples") > 45.0 {
        &GODRAY_CHOICES[2]
    } else {
        &GODRAY_CHOICES[1]
    }
}

/// `FrameCost`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameCost {
    Low,
    Mid,
    High,
}

impl FrameCost {
    /// `COST_WORD`.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Low => "Light",
            Self::Mid => "Medium",
            Self::High => "Heavy",
        }
    }

    /// The pips lit of three.
    #[must_use]
    pub const fn pips(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Mid => 2,
            Self::High => 3,
        }
    }
}

/// `VIDEO_SECTIONS`' ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSection {
    Screen,
    Effects,
    World,
    Textures,
    Advanced,
}

/// `VIDEO_SECTIONS`: id and name.
pub const VIDEO_SECTIONS: [(VideoSection, &str); 5] = [
    (VideoSection::Screen, "Screen"),
    (VideoSection::Effects, "Effects"),
    (VideoSection::World, "World"),
    (VideoSection::Textures, "Textures"),
    (VideoSection::Advanced, "Advanced"),
];

/// `VIDEO_SECTIONS[i].load`: how heavy a section is set, in points (light
/// 1, medium 2, heavy 3).
#[must_use]
pub fn section_load(cvars: &Cvars, section: VideoSection) -> f64 {
    let on = |name: &str, points: f64| if number(cvars, name) > 0.0 { points } else { 0.0 };
    match section {
        VideoSection::Screen => 0.0,
        VideoSection::Effects => {
            on("r_bloom", 1.0)
                + on("r_glow", 2.0)
                + on("r_ssr", 3.0)
                + match godray_choice(cvars).id {
                    "off" => 0.0,
                    "low" => 1.0,
                    _ => 2.0,
                }
                + on("r_softhalos", 1.0)
                + if cvars.exists("r_cel_outlines") && cvars.get_bool("r_cel_outlines") {
                    1.0
                } else {
                    0.0
                }
        }
        VideoSection::World => {
            let water = number(cvars, "r_water_quality");
            water - 1.0
                + if water >= 3.0 && number(cvars, "r_water_actors") > 0.0 {
                    1.0
                } else {
                    0.0
                }
                + on("r_arealights", 2.0)
        }
        VideoSection::Textures => {
            let aniso = number(cvars, "r_anisotropy");
            (if aniso >= 16.0 {
                2.0
            } else if aniso >= 4.0 {
                1.0
            } else {
                0.0
            }) + on("r_bsp_pbr", 2.0)
        }
        VideoSection::Advanced => {
            if number(cvars, "r_picmip") == 0.0 {
                1.0
            } else {
                0.0
            }
        }
    }
}

/// `loadLevel`: up to one point is light, up to three medium, more heavy.
#[must_use]
pub fn load_level(points: f64) -> FrameCost {
    if points <= 1.0 {
        FrameCost::Low
    } else if points <= 3.0 {
        FrameCost::Mid
    } else {
        FrameCost::High
    }
}

// ── Players ──

/// `TeamColor`: the swatch (HSL), and the model tint and outline as RGB
/// floats at full glow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamColor {
    pub id: &'static str,
    pub label: &'static str,
    pub hsl: [f32; 3],
    pub fill: &'static str,
    pub outline: &'static str,
}

const fn team(
    id: &'static str,
    label: &'static str,
    hsl: [f32; 3],
    fill: &'static str,
    outline: &'static str,
) -> TeamColor {
    TeamColor {
        id,
        label,
        hsl,
        fill,
        outline,
    }
}

/// `TEAM_COLORS`: each sets a side's body and its outline together. Pink
/// and cyan are the game's defaults for your side and enemies.
pub const TEAM_COLORS: [TeamColor; 8] = [
    team("pink", "Pink", [318.0, 1.0, 0.62], "1.55 0.2 1.35", "1.85 0.28 1.65"),
    team("cyan", "Cyan", [190.0, 1.0, 0.55], "0.1 1.15 1.85", "0.16 1.35 2.2"),
    team("lime", "Lime", [96.0, 0.9, 0.55], "0.55 1.7 0.2", "0.7 2 0.3"),
    team("orange", "Orange", [28.0, 1.0, 0.58], "1.8 0.7 0.1", "2.1 0.85 0.15"),
    team("yellow", "Yellow", [52.0, 1.0, 0.58], "1.7 1.5 0.15", "2 1.8 0.2"),
    team("red", "Red", [356.0, 0.95, 0.6], "1.8 0.2 0.25", "2.1 0.3 0.3"),
    team("violet", "Violet", [272.0, 0.95, 0.68], "0.9 0.3 1.9", "1.1 0.4 2.2"),
    team("white", "White", [45.0, 0.6, 0.95], "1.6 1.6 1.6", "1.9 1.9 1.9"),
];

/// `Side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ally,
    Enemy,
}

/// `SIDE_CVARS`: enemies also covers players seen from no side; FFA
/// opponents wear the FFA cvars.
const fn side_cvars(side: Side) -> &'static [(&'static str, &'static str)] {
    match side {
        Side::Ally => &[("cl_team_ally_color", "cl_team_ally_outline_color")],
        Side::Enemy => &[
            ("cl_team_enemy_color", "cl_team_enemy_outline_color"),
            ("cl_team_ffa_color", "cl_team_ffa_outline_color"),
        ],
    }
}

/// `sameRgb`.
fn same_rgb(a: &str, b: &str) -> bool {
    let parse = |t: &str| -> Vec<f64> { t.split_whitespace().map(|p| p.parse().unwrap_or(f64::NAN)).collect() };
    let (x, y) = (parse(a), parse(b));
    x.len() == 3 && y.len() == 3 && x.iter().zip(&y).all(|(p, q)| (p - q).abs() < 0.01)
}

fn text(cvars: &Cvars, name: &str) -> String {
    if cvars.exists(name) {
        cvars.get_string(name)
    } else {
        String::new()
    }
}

/// `sideColor`: the colour a side is drawn in now, or `None` when it was
/// set to something else.
#[must_use]
pub fn side_color(cvars: &Cvars, side: Side) -> Option<&'static TeamColor> {
    TEAM_COLORS.iter().find(|color| {
        side_cvars(side).iter().all(|(fill, outline)| {
            same_rgb(&text(cvars, fill), color.fill) && same_rgb(&text(cvars, outline), color.outline)
        })
    })
}

/// `setSideColor`.
pub fn set_side_color(cvars: &mut Cvars, side: Side, color: &TeamColor) {
    for (fill, outline) in side_cvars(side) {
        cvars.set(fill, color.fill);
        cvars.set(outline, color.outline);
    }
}

/// `sideColorsAreDefault`.
#[must_use]
pub fn side_colors_are_default(cvars: &Cvars) -> bool {
    side_cvars(Side::Ally)
        .iter()
        .chain(side_cvars(Side::Enemy))
        .flat_map(|(fill, outline)| [*fill, *outline])
        .all(|name| {
            let default = cvars.get_cvar(name).map(|c| match &c.default_value {
                CvarValue::String(s) => s.clone(),
                _ => String::new(),
            });
            same_rgb(&text(cvars, name), &default.unwrap_or_default())
        })
}

/// `COLOR_BLIND_SIDE_COLORS`: cyan and orange stay furthest apart under
/// every kind the correction covers.
#[must_use]
pub const fn color_blind_side_color(side: Side) -> &'static TeamColor {
    match side {
        Side::Ally => &TEAM_COLORS[1],
        Side::Enemy => &TEAM_COLORS[3],
    }
}

/// `resetSideColors`.
pub fn reset_side_colors(cvars: &mut Cvars) {
    for (fill, outline) in side_cvars(Side::Ally).iter().chain(side_cvars(Side::Enemy)) {
        cvars.reset(fill);
        cvars.reset(outline);
    }
}

/// `OUTLINE_CHOICES`: Thin is the game's default size, Bold twice it.
pub const OUTLINE_CHOICES: [CvarChoice; 3] = [
    CvarChoice {
        id: "off",
        label: "Off",
        values: &[("cl_player_outline", 0.0)],
    },
    CvarChoice {
        id: "thin",
        label: "Thin",
        values: &[("cl_player_outline", 1.0), ("cl_player_outline_size", 25.0)],
    },
    CvarChoice {
        id: "bold",
        label: "Bold",
        values: &[("cl_player_outline", 1.0), ("cl_player_outline_size", 50.0)],
    },
];

/// `BOOST_CHOICES`: a glow of the players' own for dark corners.
pub const BOOST_CHOICES: [CvarChoice; 3] = [
    CvarChoice {
        id: "off",
        label: "Off",
        values: &[("cl_player_fullbright", 0.0)],
    },
    CvarChoice {
        id: "some",
        label: "Some",
        values: &[("cl_player_fullbright", 1.0), ("cl_player_fullbright_intensity", 0.25)],
    },
    CvarChoice {
        id: "lots",
        label: "Lots",
        values: &[("cl_player_fullbright", 1.0), ("cl_player_fullbright_intensity", 0.6)],
    },
];

// ── Crosshair ──

/// `CROSSHAIR_COLORS`: the game's defaults (green, red on a hit) among them.
pub const CROSSHAIR_COLORS: [(&str, &str); 6] = [
    ("00FF00", "Green"),
    ("00E7FF", "Cyan"),
    ("FFB400", "Amber"),
    ("F4F7FF", "White"),
    ("FF4FD8", "Magenta"),
    ("FF3355", "Red"),
];

/// `HIT_COLORS`.
pub const HIT_COLORS: [(&str, &str); 5] = [
    ("FF0000", "Red"),
    ("F4F7FF", "White"),
    ("FFB400", "Amber"),
    ("FF4FD8", "Magenta"),
    ("00E7FF", "Cyan"),
];

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
    // shagger: RenderVars.ts and PostProcessingManager.ts.
    register_render_vars(cvars);
    // viewport/effects/*.ts: the first-person view effects.
    shagger::viewport::register_view_cvars(cvars);
    // AmmoCounterDisplay.ts.
    shagger::weapon::ammo_counter_display::register_ammo_counter_cvars(cvars);
    // WorldFogController.ts (MapLoader registers them at startup).
    register_fog_cvars(cvars);
    // Crosshair.ts.
    register_crosshair_cvars(cvars);
    // PlayerModelVisualSettings.ts, FPSPlayerAudio.ts.
    register_player_model_visual_cvars(cvars);
    register_player_audio_cvars(cvars);
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
    // Native only: the game window starts borderless fullscreen, and
    // Alt+Enter's choice is remembered here for the next start.
    int_flag(
        cvars,
        "vid_fullscreen",
        1,
        "Start the game in borderless fullscreen (Alt+Enter switches and is remembered)",
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

    #[test]
    fn presets_and_side_colours_read_back_from_the_cvars() {
        let mut cvars = Cvars::new(None, false);
        register_client_cvars(&mut cvars);
        // Balanced is the game's own defaults.
        assert_eq!(
            current_choice(&cvars, &VIDEO_PRESETS.map(|p| p.0)).map(|c| c.id),
            Some("balanced")
        );
        assert_eq!(load_level(section_load(&cvars, VideoSection::Effects)), FrameCost::Mid);
        apply_choice(&mut cvars, &VIDEO_PRESETS[2].0);
        assert_eq!(
            current_choice(&cvars, &VIDEO_PRESETS.map(|p| p.0)).map(|c| c.id),
            Some("pretty")
        );
        assert_eq!(load_level(section_load(&cvars, VideoSection::Effects)), FrameCost::High);
        apply_choice(&mut cvars, &GODRAY_CHOICES[1]);
        assert_eq!(godray_choice(&cvars).id, "low");
        assert!(current_choice(&cvars, &VIDEO_PRESETS.map(|p| p.0)).is_none());

        assert_eq!(side_color(&cvars, Side::Ally).map(|c| c.id), Some("pink"));
        assert_eq!(side_color(&cvars, Side::Enemy).map(|c| c.id), Some("cyan"));
        assert!(side_colors_are_default(&cvars));
        set_side_color(&mut cvars, Side::Enemy, color_blind_side_color(Side::Enemy));
        assert_eq!(side_color(&cvars, Side::Enemy).map(|c| c.id), Some("orange"));
        assert!(!side_colors_are_default(&cvars));
        reset_side_colors(&mut cvars);
        assert!(side_colors_are_default(&cvars));
        assert_eq!(current_choice(&cvars, &OUTLINE_CHOICES).map(|c| c.id), Some("thin"));
        assert!(current_choice(&cvars, &BOOST_CHOICES).is_none_or(|c| c.id == "off"));
    }
}
