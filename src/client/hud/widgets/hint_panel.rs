//! Ported from wash-fps `client/hud/widgets/HintPanel.ts` (styles
//! `.fps-hint` in `WashikiTheme.css`).
//!
//! A short tip for new players above the weapons, bottom right: a gold
//! HINT cell and one line naming the player's own keys as gold key caps.
//! [`HintCoach`] decides when; the panel fades in, holds, and fades away.
//! Off with `cl_hints 0`.
//!
//! The browser hears the coach's events through module functions
//! (`noteLocalShotForHints` and the rest); here the window calls the
//! panel's methods, and gives it the keys (`set_keys`), the fire button
//! held from the hip and the water level each frame. The panel's clock is
//! the HUD's frame time, in milliseconds.

use std::collections::BTreeMap;

use shagger::input::input_system::InputSystem;
use shagger::input::input_types::MouseButton;
use shagger::ui::hud_2d::{Hud2d, TextStyle};

use crate::client::fps_actions::{action_key_label, fps, key_label};
use crate::client::game_events::canonical_weapon_id;
use crate::client::hud::hint_coach::{HINT_SHOW_MS, HintCoach, HintId, MovementSample, StatusSample};
use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, drop_print_sm, hsla, void};
use crate::client::match_state_manager::MatchPhase;
use crate::shared::entities::definitions::weapons::weapons_table::{weapon_model, weapon_name};

/// Hints show all through warmup, and for this long into a live match (ms).
const LIVE_HINT_MS: f64 = 2.0 * 60_000.0;
/// Aim progress that counts as aiming down sights.
pub const AIMING: f64 = 0.8;
/// Keys listed per action: more make the line too long to read at a glance.
const MAX_KEYS: usize = 2;

/// `.fps-hint--shown`'s fade in, and `.fps-hint`'s fade out (ms).
const FADE_IN_MS: f64 = 180.0;
const FADE_OUT_MS: f64 = 600.0;

/// The actions a hint names.
const NAMED: [&str; 13] = [
    fps::CROUCH,
    fps::AIM,
    fps::JUMP,
    fps::MOVE_FORWARD,
    fps::MOVE_LEFT,
    fps::MOVE_RIGHT,
    fps::WEAPON_1,
    fps::WEAPON_2,
    fps::WEAPON_3,
    fps::WEAPON_PREV,
    fps::WEAPON_NEXT,
    fps::FIRE,
    fps::RELOAD,
];

/// A hint's line: text, and the key caps in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Key(String),
}

/// `keyLabels`: the first keys bound to an action, as the player would name
/// them.
#[must_use]
pub fn key_labels(input: &InputSystem, action: &str) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for binding in input.bindings_for_action(action) {
        let label = match (&binding.key, binding.mouse_button) {
            (Some(key), _) => key_label(key),
            (None, Some(MouseButton::Left)) => "MOUSE1".to_owned(),
            (None, Some(MouseButton::Right)) => "MOUSE2".to_owned(),
            (None, Some(MouseButton::Middle)) => "MOUSE3".to_owned(),
            (None, None) => String::new(),
        };
        if !label.is_empty() && !labels.contains(&label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        return vec![action_key_label(input, action)];
    }
    labels.truncate(MAX_KEYS);
    labels
}

/// `hintHtml`: a hint's line, with `keys` naming each action's keys.
#[must_use]
pub fn hint_pieces(id: HintId, keys: &dyn Fn(&str) -> Vec<String>, gamepad: bool, weapon: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let text = |out: &mut Vec<Piece>, t: &str| out.push(Piece::Text(t.to_owned()));
    let key = |out: &mut Vec<Piece>, action: &str| {
        for (i, label) in keys(action).into_iter().enumerate() {
            if i > 0 {
                out.push(Piece::Text(" or ".to_owned()));
            }
            out.push(Piece::Key(label));
        }
    };
    let first = |out: &mut Vec<Piece>, action: &str| {
        out.push(Piece::Key(
            keys(action).into_iter().next().unwrap_or_else(|| "?".to_owned()),
        ));
    };
    match id {
        HintId::Slide => {
            text(&mut out, "When moving 400+ ups, hold ");
            key(&mut out, fps::CROUCH);
            text(&mut out, " to crouchslide!");
        }
        HintId::Aim => {
            text(&mut out, "Hold ");
            key(&mut out, fps::AIM);
            text(&mut out, " to aim!");
        }
        HintId::Walljump => {
            text(&mut out, "Press ");
            key(&mut out, fps::JUMP);
            text(&mut out, " on a wall to walljump!");
        }
        HintId::Bunnyhop => {
            text(&mut out, "Hold ");
            key(&mut out, fps::MOVE_FORWARD);
            text(&mut out, " and ");
            key(&mut out, fps::JUMP);
            text(&mut out, " to bunny hop!");
        }
        HintId::Slidespeed => {
            text(&mut out, "At 400+ ups, hold ");
            key(&mut out, fps::CROUCH);
            text(&mut out, " to crouchslide for a speed boost!");
        }
        HintId::Fall => {
            text(&mut out, "Hold ");
            key(&mut out, fps::CROUCH);
            text(&mut out, " when falling to absorb damage!");
        }
        HintId::Strafe => {
            text(&mut out, "In the air, hold ");
            key(&mut out, fps::MOVE_LEFT);
            text(&mut out, " or ");
            key(&mut out, fps::MOVE_RIGHT);
            text(
                &mut out,
                if gamepad {
                    " and turn with the look stick to gain speed!"
                } else {
                    " and turn the mouse to gain speed!"
                },
            );
        }
        HintId::Ledge => {
            text(&mut out, "Jump at a ledge holding ");
            key(&mut out, fps::MOVE_FORWARD);
            text(&mut out, " to climb up; ");
            key(&mut out, fps::CROUCH);
            text(&mut out, " lets go!");
        }
        HintId::Health => text(&mut out, "Look for health items to recover!"),
        HintId::Armor => text(&mut out, "Find armor items for protection!"),
        HintId::Shock => {
            let name = weapon_name(canonical_weapon_id(weapon)).unwrap_or("this gun");
            text(
                &mut out,
                &format!("Firing the {name} underwater shocks you and everyone in the water!"),
            );
        }
        HintId::Air => text(&mut out, "Surface for air!"),
        HintId::Switch if gamepad => {
            text(&mut out, "Press ");
            first(&mut out, fps::WEAPON_PREV);
            text(&mut out, " or ");
            first(&mut out, fps::WEAPON_NEXT);
            text(&mut out, " to switch weapons!");
        }
        HintId::Switch => {
            text(&mut out, "Press ");
            first(&mut out, fps::WEAPON_1);
            text(&mut out, ", ");
            first(&mut out, fps::WEAPON_2);
            text(&mut out, ", ");
            first(&mut out, fps::WEAPON_3);
            text(&mut out, " or scroll to switch weapons!");
        }
        HintId::Blade => {
            text(&mut out, "With the blade, hold ");
            key(&mut out, fps::AIM);
            text(&mut out, " to charge a blast!");
        }
        HintId::Rocketjump => {
            text(&mut out, "Press ");
            key(&mut out, fps::JUMP);
            text(&mut out, " and fire ");
            key(&mut out, fps::FIRE);
            text(&mut out, " at your feet to rocket jump!");
        }
        HintId::Grenade => text(&mut out, "Grenades bounce: bank them around corners!"),
        HintId::Reload => {
            text(&mut out, "Press ");
            key(&mut out, fps::RELOAD);
            text(&mut out, " to reload before your clip runs dry!");
        }
        HintId::Scoreboard => {
            text(&mut out, "Hold ");
            key(&mut out, fps::SCOREBOARD);
            text(&mut out, " to see the scores!");
        }
        HintId::Plant => text(&mut out, "Stand still on flat ground at a bomb site to arm the bomb!"),
        HintId::Defuse => text(&mut out, "Stand by the planted bomb to defuse it!"),
    }
    out
}

/// The line as plain text, key caps in brackets.
#[must_use]
pub fn plain(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|p| match p {
            Piece::Text(t) => t.clone(),
            Piece::Key(k) => format!("[{k}]"),
        })
        .collect()
}

/// `canAimWith`: every gun with an ADS setup, not the blade.
#[must_use]
pub fn can_aim_with(weapon_id: &str) -> bool {
    let id = canonical_weapon_id(weapon_id);
    id != "blade" && weapon_model(id).is_some()
}

/// CSS `ease` and `ease-out`, as cubic Béziers through (0,0) and (1,1).
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let at = |a: f64, b: f64, t: f64| 3.0 * a * t * (1.0 - t).powi(2) + 3.0 * b * t * t * (1.0 - t) + t * t * t;
    // Bisection on x(t): monotonic for these control points.
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..30 {
        let mid = f64::midpoint(lo, hi);
        if at(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(y1, y2, f64::midpoint(lo, hi))
}

/// `FPSHintPanel`.
pub struct HintPanel {
    coach: HintCoach,
    /// The HUD's clock, ms.
    clock: f64,
    /// Whether hints may show now, from the last HUD tick.
    live: bool,
    /// When the live match began, as this client saw it; `None` in warmup.
    match_live_since: Option<f64>,
    /// On the ground, from the last movement step.
    grounded: bool,
    /// What the window saw this frame: fire held from the hip with a gun
    /// that aims, and the water level.
    pub holding_fire: bool,
    pub water_level: u8,
    /// Fire held on an empty clip with ammo in reserve, last frame.
    dry: bool,
    /// The keys for each action a hint names, and the device in hand.
    keys: BTreeMap<&'static str, Vec<String>>,
    gamepad: bool,
    /// The line on screen, when it came, its opacity then, and when it was
    /// dismissed with its opacity then.
    shown: Option<Shown>,
}

#[derive(Debug, Clone)]
struct Shown {
    pieces: Vec<Piece>,
    at: f64,
    from: f64,
    dismissed: Option<(f64, f64)>,
}

impl Default for HintPanel {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        Self {
            coach: HintCoach::new(shagger::shared::spread_utils::mulberry32(seed)),
            clock: 0.0,
            live: false,
            match_live_since: None,
            grounded: true,
            holding_fire: false,
            water_level: 0,
            dry: false,
            keys: BTreeMap::new(),
            gamepad: false,
            shown: None,
        }
    }
}

const CLIP: [[f32; 2]; 4] = [[0.015, 0.0], [1.0, 0.0], [0.985, 1.0], [0.0, 0.94]];
const CELL_CLIP: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.88, 1.0], [0.0, 1.0]];
const KEY_CLIP: [[f32; 2]; 4] = [[0.06, 0.0], [1.0, 0.08], [0.94, 1.0], [0.0, 0.92]];

/// A word or a key cap, laid out on a line.
enum Run<'a> {
    Word(String),
    Key(&'a str),
}

impl HintPanel {
    /// The player's keys for the actions a hint names, and whether a pad
    /// is in hand (`InputDevice.current`).
    pub fn set_keys(&mut self, input: &InputSystem, gamepad: bool) {
        self.gamepad = gamepad;
        for action in NAMED.into_iter().chain([fps::SCOREBOARD]) {
            let labels = if gamepad {
                vec![action_key_label(input, action)]
            } else {
                key_labels(input, action)
            };
            self.keys.insert(action, labels);
        }
    }

    /// `moved`: a predicted movement step.
    pub fn moved(&mut self, sample: &MovementSample, weapon: &str) {
        self.grounded = sample.grounded;
        if !self.live {
            return;
        }
        if let Some(hint) = self.coach.observe(sample, self.clock) {
            self.present(hint, weapon);
        }
    }

    /// `clipEmptied`, on the frame fire is first held on an empty clip with
    /// ammo in reserve (where `FPSPlayer` auto-reloads).
    pub fn dry_fire(&mut self, dry: bool, weapon: &str) {
        let started = dry && !self.dry;
        self.dry = dry;
        if started
            && self.live
            && let Some(hint) = self.coach.clip_emptied(self.clock)
        {
            self.present(hint, weapon);
        }
    }

    /// `charged`: the local player began charging a blast.
    pub fn charged(&mut self) {
        self.coach.charged(self.clock);
    }

    /// `fell`: fall damage to the local player.
    pub fn fell(&mut self, weapon: &str) {
        if !self.live {
            return;
        }
        if let Some(hint) = self.coach.fell(self.clock) {
            self.present(hint, weapon);
        }
    }

    /// `shot`: a shot from the local player, from the hip or not, with
    /// whether its weapon can aim down sights.
    pub fn shot(&mut self, from_hip: bool, can_aim: bool, weapon: &str) {
        if !self.live {
            return;
        }
        if let Some(hint) = self.coach.shot(self.clock, from_hip, can_aim) {
            self.present(hint, weapon);
        }
    }

    /// The line on screen, as plain text.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        self.shown.as_ref().map(|s| plain(&s.pieces))
    }

    /// `playing`: not while dead, watching, or after the match; all through
    /// warmup, and only the first minutes of the live match.
    fn playing(&mut self, frame: &HudFrame<'_>) -> bool {
        match frame.match_state.phase() {
            MatchPhase::Warmup => self.match_live_since = None,
            MatchPhase::Active => {
                self.match_live_since.get_or_insert(self.clock);
            }
            _ => {}
        }
        let on = shagger::core::cvar::with_cvars(|c| !c.exists("cl_hints") || c.get_number("cl_hints") > 0.0);
        on && !frame.state.local_dead
            && !frame.state.watcher
            && frame.match_state.phase() != MatchPhase::PostGame
            && self
                .match_live_since
                .is_none_or(|since| self.clock - since < LIVE_HINT_MS)
    }

    fn status_of(&self, frame: &HudFrame<'_>) -> StatusSample {
        let state = frame.state;
        let me = state.scoreboard.iter().find(|e| e.player_id == state.local_player_id);
        let bomb = frame.match_state.bomb();
        let field = |k: &str| bomb.and_then(|b| b.get(k)).and_then(|v| v.as_str());
        let phase = field("phase");
        StatusSample {
            health: state.health,
            armor: state.armor,
            live_for_ms: self.match_live_since.map(|since| self.clock - since),
            weapon: canonical_weapon_id(&state.current_weapon).to_owned(),
            carries_gun: state.weapons.iter().any(|w| canonical_weapon_id(w) != "blade"),
            grounded: self.grounded,
            water_level: self.water_level,
            air: state.air,
            carries_bomb: me.is_some_and(|me| field("carrier") == Some(me.entity_id.as_str())),
            defending_planted_bomb: matches!(phase, Some("planted" | "armed"))
                && me.is_some_and(|me| me.team_id.is_some() && !frame.match_state.is_attacking(me.team_id.as_deref())),
        }
    }

    /// `present`: puts a hint on screen, replacing the one there.
    pub fn present(&mut self, id: HintId, weapon: &str) {
        let keys = |action: &str| self.keys.get(action).cloned().unwrap_or_else(|| vec!["?".to_owned()]);
        let pieces = hint_pieces(id, &keys, self.gamepad, weapon);
        let from = self.opacity_and_shift().0;
        self.shown = Some(Shown {
            pieces,
            at: self.clock,
            from,
            dismissed: None,
        });
    }

    fn dismiss(&mut self) {
        let (opacity, _) = self.opacity_and_shift();
        if let Some(shown) = &mut self.shown
            && shown.dismissed.is_none()
        {
            shown.dismissed = Some((self.clock, opacity));
        }
    }

    /// The panel's opacity now, and how far right of its place it is (0 to
    /// 1 of its 24 px slide).
    fn opacity_and_shift(&self) -> (f64, f64) {
        let Some(shown) = &self.shown else {
            return (0.0, 1.0);
        };
        match shown.dismissed {
            None => {
                let k = cubic_bezier(0.0, 0.0, 0.58, 1.0, (self.clock - shown.at) / FADE_IN_MS);
                let o = shown.from + (1.0 - shown.from) * k;
                (o, 1.0 - o)
            }
            Some((at, from)) => {
                let k = cubic_bezier(0.25, 0.1, 0.25, 1.0, (self.clock - at) / FADE_OUT_MS);
                let o = from * (1.0 - k);
                (o, 1.0 - o)
            }
        }
    }

    fn header_style(fonts: &Fonts, a: f32) -> TextStyle {
        fonts
            .display(19.0, hsla(255.0, 45.0, 10.0, 0.92 * a))
            .spacing(19.0 * UI_SCALE * 0.08)
    }

    fn text_style(fonts: &Fonts, a: f32) -> TextStyle {
        fonts
            .body(18.0, hsla(42.0, 30.0, 92.0, 0.85 * a))
            .spacing(18.0 * UI_SCALE * 0.1)
    }

    fn key_style(fonts: &Fonts, a: f32) -> TextStyle {
        fonts.display(16.0, hsla(255.0, 45.0, 10.0, a))
    }

    /// The line's runs, uppercase.
    fn runs(pieces: &[Piece]) -> Vec<(Run<'_>, bool)> {
        // Each run, and whether a space comes before it.
        let mut runs = Vec::new();
        for piece in pieces {
            match piece {
                Piece::Text(t) => {
                    let mut space = t.starts_with(' ');
                    for word in t.split(' ').filter(|w| !w.is_empty()) {
                        runs.push((Run::Word(word.to_uppercase()), space));
                        space = true;
                    }
                }
                Piece::Key(k) => runs.push((Run::Key(k.as_str()), false)),
            }
        }
        runs
    }

    /// The lines, each a list of (x from the text's left, run), and the
    /// widest line, for text at most `max` wide.
    fn lay_out<'a>(hud: &Hud2d, fonts: &Fonts, pieces: &'a [Piece], max: f32) -> (Vec<Vec<(f32, Run<'a>)>>, f32) {
        let s = UI_SCALE;
        let body = Self::text_style(fonts, 1.0);
        let key = Self::key_style(fonts, 1.0);
        let space = hud.measure(" ", &body);
        let mut lines: Vec<Vec<(f32, Run<'a>)>> = vec![Vec::new()];
        let (mut at, mut widest) = (0.0f32, 0.0f32);
        let mut after_key = false;
        for (run, spaced) in Self::runs(pieces) {
            let (w, is_key) = match &run {
                Run::Word(word) => (hud.measure(word, &body), false),
                Run::Key(k) => (hud.measure(k, &key) + 20.0 * s + 12.0 * s, true),
            };
            // Beside a key cap its margins are the gap (the text runs are
            // flex items, trimmed at their ends).
            let gap = if spaced && !is_key && !after_key { space } else { 0.0 };
            let line = lines.last_mut().expect("a line");
            if !line.is_empty() && at + gap + w > max {
                lines.push(Vec::new());
                at = 0.0;
            } else if !line.is_empty() {
                at += gap;
            }
            lines.last_mut().expect("a line").push((at, run));
            at += w;
            widest = widest.max(at);
            after_key = is_key;
        }
        (lines, widest)
    }

    fn box_size(hud: &Hud2d, fonts: &Fonts, pieces: &[Piece]) -> (f32, f32, f32, usize) {
        let s = UI_SCALE;
        let [vw, _] = hud.size();
        let header = hud.measure("HINT", &Self::header_style(fonts, 1.0)) + 38.0 * s;
        let max = (640.0 * s).min(vw / 2.0) - header - 44.0 * s;
        let (lines, widest) = Self::lay_out(hud, fonts, pieces, max.max(80.0 * s));
        let line_h = Self::line_height();
        let n = lines.len();
        let h = n as f32 * line_h + (n.saturating_sub(1)) as f32 * 6.0 * s + 26.0 * s;
        (header, header + widest + 44.0 * s, h, n)
    }

    fn line_height() -> f32 {
        let s = UI_SCALE;
        (18.0 * s * 1.3).max(16.0 * s * 1.2 + 5.0 * s)
    }
}

impl FpsHudWidget for HintPanel {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.clock += frame.dt * 1000.0;
        self.live = self.playing(frame);
        if let Some(shown) = &self.shown {
            match shown.dismissed {
                None if self.clock - shown.at >= HINT_SHOW_MS => self.dismiss(),
                Some((at, _)) if self.clock - at >= FADE_OUT_MS => self.shown = None,
                _ => {}
            }
        }
        if !self.live {
            self.dismiss();
            return;
        }
        let weapon = frame.state.current_weapon.clone();
        let hint = match self.coach.holding_fire(self.clock, self.holding_fire) {
            Some(hint) => Some(hint),
            None => {
                let status = self.status_of(frame);
                self.coach.status(&status, self.clock)
            }
        };
        if let Some(hint) = hint {
            self.present(hint, &weapon);
        }
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        match &self.shown {
            Some(shown) => {
                let (_, w, h, _) = Self::box_size(hud, fonts, &shown.pieces);
                [w, h]
            }
            None => [0.0, 0.0],
        }
    }

    /// Its right edge at `x`, its bottom at `y`.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let Some(shown) = &self.shown else { return };
        let (opacity, shift) = self.opacity_and_shift();
        if opacity <= 0.001 {
            return;
        }
        let a = opacity as f32;
        let s = UI_SCALE;
        let fade = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * a];
        let (header_w, w, h, _) = Self::box_size(hud, fonts, &shown.pieces);
        let left = x - w + shift as f32 * 24.0 * s;
        let top = y - h;
        Slab {
            clip: &CLIP,
            background: Gradient::new(
                172.0,
                &[(fade(hsla(250.0, 30.0, 11.0, 0.95)), None), (fade(void()), Some(0.6))],
            ),
            skew_deg: -5.0,
            shadow: drop_print_sm().map(|(offset, color)| (offset, fade(color))),
        }
        .draw(hud, left, top, w, h);
        Slab {
            clip: &CELL_CLIP,
            background: Gradient::new(
                105.0,
                &[
                    (fade(hsla(42.0, 96.0, 58.0, 1.0)), None),
                    (fade(hsla(36.0, 90.0, 46.0, 1.0)), None),
                ],
            ),
            skew_deg: -5.0,
            shadow: None,
        }
        .draw(hud, left, top, header_w, h);
        hud.text_in_box(left + 16.0 * s, top, h, "HINT", &Self::header_style(fonts, a));

        let [vw, _] = hud.size();
        let max = (640.0 * s).min(vw / 2.0) - header_w - 44.0 * s;
        let (lines, _) = Self::lay_out(hud, fonts, &shown.pieces, max.max(80.0 * s));
        let body = Self::text_style(fonts, a);
        let key = Self::key_style(fonts, a);
        let line_h = Self::line_height();
        let text_left = left + header_w + 18.0 * s;
        let cap_h = 16.0 * s * 1.2 + 5.0 * s;
        for (i, line) in lines.iter().enumerate() {
            let line_top = top + 12.0 * s + i as f32 * (line_h + 6.0 * s);
            for (dx, run) in line {
                let at = text_left + dx;
                match run {
                    Run::Word(word) => {
                        hud.text_in_box(at, line_top, line_h, word, &body);
                    }
                    Run::Key(label) => {
                        let cap_w = hud.measure(label, &key) + 20.0 * s;
                        let cap_x = at + 6.0 * s;
                        let cap_top = line_top + (line_h - cap_h) / 2.0;
                        Slab {
                            clip: &KEY_CLIP,
                            background: Gradient::solid(fade(hsla(42.0, 96.0, 56.0, 1.0))),
                            skew_deg: 0.0,
                            shadow: None,
                        }
                        .draw(hud, cap_x, cap_top, cap_w, cap_h);
                        hud.text_in_box(cap_x + 10.0 * s, cap_top, cap_h, label, &key);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasd(action: &str) -> Vec<String> {
        match action {
            fps::CROUCH => vec!["C".into(), "CTRL".into()],
            fps::AIM => vec!["MOUSE2".into()],
            fps::JUMP => vec!["SPACE".into()],
            fps::MOVE_FORWARD => vec!["W".into()],
            fps::MOVE_LEFT => vec!["A".into()],
            fps::MOVE_RIGHT => vec!["D".into()],
            fps::WEAPON_1 => vec!["1".into()],
            fps::WEAPON_2 => vec!["2".into()],
            fps::WEAPON_3 => vec!["3".into()],
            _ => vec!["?".into()],
        }
    }

    #[test]
    fn a_hint_names_the_players_own_keys() {
        let line = |id| plain(&hint_pieces(id, &wasd, false, "machinegun"));
        assert_eq!(
            line(HintId::Slide),
            "When moving 400+ ups, hold [C] or [CTRL] to crouchslide!"
        );
        assert_eq!(line(HintId::Bunnyhop), "Hold [W] and [SPACE] to bunny hop!");
        assert_eq!(
            line(HintId::Strafe),
            "In the air, hold [A] or [D] and turn the mouse to gain speed!"
        );
        assert_eq!(line(HintId::Switch), "Press [1], [2], [3] or scroll to switch weapons!");
        assert_eq!(line(HintId::Health), "Look for health items to recover!");
    }

    #[test]
    fn the_shock_hint_names_the_gun_in_hand() {
        let line = |w| plain(&hint_pieces(HintId::Shock, &wasd, false, w));
        assert_eq!(
            line("lasergun"),
            "Firing the Voltaic underwater shocks you and everyone in the water!"
        );
        assert_eq!(
            line("electrobolt"),
            "Firing the Electrobolt underwater shocks you and everyone in the water!"
        );
        assert!(line("nothing").contains("the this gun"));
    }

    #[test]
    fn a_pad_turns_with_the_look_stick() {
        let line = plain(&hint_pieces(HintId::Strafe, &wasd, true, ""));
        assert!(line.ends_with("turn with the look stick to gain speed!"));
        assert!(plain(&hint_pieces(HintId::Switch, &wasd, true, "")).starts_with("Press [?] or [?]"));
    }

    #[test]
    fn every_gun_aims_and_the_blade_does_not() {
        assert!(can_aim_with("machinegun"));
        assert!(can_aim_with("lasergun"));
        assert!(!can_aim_with("blade"));
        assert!(!can_aim_with(""));
    }

    #[test]
    fn the_default_keys_are_listed_two_at_most() {
        let mut input = InputSystem::new(None);
        crate::client::fps_actions::register_fps_actions(&mut input);
        let aim = key_labels(&input, fps::AIM);
        assert_eq!(aim[0], "MOUSE2");
        assert!(key_labels(&input, fps::CROUCH).len() <= MAX_KEYS);
    }

    #[test]
    fn the_fades_follow_the_css_curves() {
        assert!((cubic_bezier(0.0, 0.0, 0.58, 1.0, 0.0)).abs() < 1e-6);
        assert!((cubic_bezier(0.0, 0.0, 0.58, 1.0, 1.0) - 1.0).abs() < 1e-6);
        // ease-out is past halfway at the half.
        assert!(cubic_bezier(0.0, 0.0, 0.58, 1.0, 0.5) > 0.6);
    }
}
