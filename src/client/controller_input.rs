//! Ported from wash-fps `client/ControllerInput.ts`.
//!
//! The controller, in the match and its menu. Each frame the window samples
//! the pads (`GamepadInput`) and calls [`ControllerInput::update`], which
//! holds the controller's actions as virtual actions while in play, and
//! says what the window does with the rest: the move stick (`setMoveAxis`),
//! the turn (`controllerLook`), Menu, A to start play, and the menu's
//! navigation (`UINavigation`'s intents).
//!
//! Native differences: the browser's `UINavigation` walks any page's
//! focusable elements; here the only menu a pad walks is the match's Esc
//! menu, so the intents go back to the window. The `controllerMatchInput`
//! events are [`ControllerFrame::pressed`] and
//! [`ControllerFrame::scoreboard`].

use shagger::input::gamepad_input::{GamepadInput, pad_stick};
use shagger::input::input_system::InputSystem;
use shagger::shared::js::js_hypot;

use crate::client::fps_actions::fps;
use crate::client::move_stick::MoveAxis;
use crate::client::settings::controller_model::{CONTROLLER_ACTIONS, controller_buttons};

/// The source the pad's virtual actions are held under.
const SOURCE: &str = "gamepad";

/// `UIIntent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiIntent {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

/// The `in_pad_*` cvars this reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadCvars {
    pub swap: bool,
    pub move_dead_zone: f64,
    pub look_dead_zone: f64,
    pub curve: f64,
    /// Degrees a second.
    pub yaw: f64,
    pub pitch: f64,
    pub invert: bool,
}

impl Default for PadCvars {
    fn default() -> Self {
        Self {
            swap: false,
            move_dead_zone: 0.15,
            look_dead_zone: 0.12,
            curve: 1.7,
            yaw: 240.0,
            pitch: 180.0,
            invert: false,
        }
    }
}

impl PadCvars {
    /// From the cvars.
    #[must_use]
    pub fn read() -> Self {
        shagger::core::cvar::with_cvars(|c| {
            let d = Self::default();
            let num = |name: &str, default: f64| {
                if c.exists(name) { c.get_number(name) } else { default }
            };
            Self {
                swap: num("in_pad_swap", 0.0) != 0.0,
                move_dead_zone: num("in_pad_move_deadzone", d.move_dead_zone),
                look_dead_zone: num("in_pad_look_deadzone", d.look_dead_zone),
                curve: num("in_pad_curve", d.curve),
                yaw: num("in_pad_yaw", d.yaw),
                pitch: num("in_pad_pitch", d.pitch),
                invert: num("in_pad_invert", 0.0) != 0.0,
            }
        })
    }
}

/// Where the window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControllerContext {
    /// A controller is the device (`InputDevice.current === "gamepad"`).
    pub enabled: bool,
    /// Playing with the view captured (`inPlay`).
    pub in_play: bool,
    /// The Esc menu owns the screen (`UINavigation.active`).
    pub menu: bool,
    /// Typing, loading, or a page over the game: the pad does nothing.
    pub blocked: bool,
}

/// What the window does this frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ControllerFrame {
    /// The move stick, filtered and unit length (`setMoveAxis`); `None`
    /// when centred or not in play.
    pub move_axis: Option<MoveAxis>,
    /// The turn this frame, radians (yaw, pitch), before the aim's share.
    pub look: (f64, f64),
    /// Menu: open the match menu, leaving play.
    pub menu: bool,
    /// A with nothing on screen: start play.
    pub enter_play: bool,
    /// The pad went away mid-play: leave it.
    pub leave_play: bool,
    /// The menu's navigation.
    pub nav: Vec<UiIntent>,
    /// The controller's actions pressed this frame.
    pub pressed: Vec<&'static str>,
    /// The scoreboard's hold, when it changed.
    pub scoreboard: Option<bool>,
}

/// `ControllerInput`.
#[derive(Debug, Default)]
pub struct ControllerInput {
    direction: Option<UiIntent>,
    repeat: f64,
    connection: Option<String>,
    menu: bool,
    scoreboard_held: bool,
}

impl ControllerInput {
    /// `update`: one frame, `seconds` long.
    pub fn update(
        &mut self,
        seconds: f64,
        pad: &mut GamepadInput,
        input: &mut InputSystem,
        ctx: ControllerContext,
        cvars: PadCvars,
    ) -> ControllerFrame {
        let mut frame = ControllerFrame::default();
        let (move_dz, look_dz) = (cvars.move_dead_zone, cvars.look_dead_zone);
        if cvars.swap {
            pad.set_stick_dead_zones(look_dz, move_dz);
        } else {
            pad.set_stick_dead_zones(move_dz, look_dz);
        }
        let standard = pad.pad.as_ref().filter(|p| p.mapping == "standard");
        let connection = standard.map(|p| format!("{}:{}", p.index, p.id));
        if ctx.enabled && self.connection.is_some() && self.connection != connection && ctx.in_play {
            frame.leave_play = true;
        }
        let connected = connection.is_some();
        self.connection = connection;
        if !ctx.enabled || !connected || ctx.blocked {
            if ctx.enabled {
                pad.suppress_until_neutral();
            }
            self.clear(input, &mut frame);
            return frame;
        }
        if self.menu != ctx.menu {
            pad.suppress_until_neutral();
            self.direction = None;
            self.menu = ctx.menu;
        }
        if ctx.menu {
            self.clear(input, &mut frame);
            // Back wins over a simultaneous Confirm or direction.
            if pad.pressed.contains(&1) || pad.pressed.contains(&9) {
                frame.nav.push(UiIntent::Back);
                return frame;
            }
            let (x, y) = (pad.axis(0), pad.axis(1));
            let next = if pad.down.contains(&12) {
                Some(UiIntent::Up)
            } else if pad.down.contains(&13) {
                Some(UiIntent::Down)
            } else if pad.down.contains(&14) {
                Some(UiIntent::Left)
            } else if pad.down.contains(&15) {
                Some(UiIntent::Right)
            } else if x.abs().max(y.abs()) < if self.direction.is_some() { 0.4 } else { 0.6 } {
                None
            } else if x.abs() > y.abs() {
                Some(if x > 0.0 { UiIntent::Right } else { UiIntent::Left })
            } else {
                Some(if y > 0.0 { UiIntent::Down } else { UiIntent::Up })
            };
            self.repeat -= seconds;
            if let Some(intent) = next
                && (next != self.direction || self.repeat <= 0.0)
            {
                frame.nav.push(intent);
                self.repeat = if next == self.direction { 0.1 } else { 0.35 };
            }
            self.direction = next;
            if pad.pressed.contains(&0) {
                frame.nav.push(UiIntent::Confirm);
            }
            return frame;
        }
        if pad.pressed.contains(&9) {
            frame.menu = true;
            pad.suppress_until_neutral();
            self.clear(input, &mut frame);
            return frame;
        }
        for action in CONTROLLER_ACTIONS {
            let buttons = controller_buttons(input, action.id);
            let held = buttons
                .iter()
                .any(|b| usize::try_from(*b).is_ok_and(|b| pad.down.contains(&b)));
            let pressed = buttons
                .iter()
                .any(|b| usize::try_from(*b).is_ok_and(|b| pad.pressed.contains(&b)));
            self.emit(&mut frame, action.id, held, pressed);
            input.set_virtual_action(action.id, ctx.in_play && held, SOURCE);
        }
        // A starts play when no menu owns the screen.
        if !ctx.in_play {
            if pad.pressed.contains(&0) {
                frame.enter_play = true;
            }
            return frame;
        }
        let (move_index, look_index) = if cvars.swap { (2, 0) } else { (0, 2) };
        let (mx, my) = pad_stick(pad.axis(move_index), pad.axis(move_index + 1), move_dz, 1.0);
        let length = js_hypot(&[mx, my]);
        if length > 0.0 {
            frame.move_axis = Some(MoveAxis {
                forward: -my / length,
                right: mx / length,
            });
        }
        let (lx, ly) = pad_stick(pad.axis(look_index), pad.axis(look_index + 1), look_dz, cvars.curve);
        let radians = std::f64::consts::PI / 180.0 * seconds;
        frame.look = (
            lx * cvars.yaw * radians,
            ly * cvars.pitch * radians * if cvars.invert { -1.0 } else { 1.0 },
        );
        // Freefly uses the game's jump and crouch bindings for vertical travel too.
        let held = |action: &str| {
            controller_buttons(input, action)
                .iter()
                .any(|b| usize::try_from(*b).is_ok_and(|b| pad.down.contains(&b)))
        };
        let (up, down) = (held(fps::JUMP), held(fps::CROUCH));
        input.set_virtual_action(fps::FLY_UP, up, SOURCE);
        input.set_virtual_action(fps::FLY_DOWN, down, SOURCE);
        frame
    }

    fn emit(&mut self, frame: &mut ControllerFrame, action: &'static str, held: bool, pressed: bool) {
        if action == fps::SCOREBOARD {
            if held == self.scoreboard_held {
                return;
            }
            self.scoreboard_held = held;
            frame.scoreboard = Some(held);
            return;
        }
        if pressed {
            frame.pressed.push(action);
        }
    }

    /// `clear`: the pad holds nothing.
    fn clear(&mut self, input: &mut InputSystem, frame: &mut ControllerFrame) {
        for action in CONTROLLER_ACTIONS {
            input.set_virtual_action(action.id, false, SOURCE);
        }
        input.set_virtual_action(fps::FLY_UP, false, SOURCE);
        input.set_virtual_action(fps::FLY_DOWN, false, SOURCE);
        self.emit(frame, fps::SCOREBOARD, false, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::fps_actions::register_fps_actions;
    use crate::client::settings::controller_model::register_controller_settings;
    use shagger::input::gamepad_input::{PadButton, PadState};

    fn pad(buttons: &[usize], axes: [f64; 4]) -> PadState {
        PadState {
            index: 0,
            id: "Xbox test".into(),
            mapping: "standard".into(),
            axes: axes.to_vec(),
            buttons: (0..17)
                .map(|i| PadButton {
                    value: if buttons.contains(&i) { 1.0 } else { 0.0 },
                    pressed: buttons.contains(&i),
                })
                .collect(),
        }
    }

    fn setup() -> (ControllerInput, GamepadInput, InputSystem) {
        let mut input = InputSystem::new(None);
        register_fps_actions(&mut input);
        register_controller_settings(&mut input);
        (ControllerInput::default(), GamepadInput::default(), input)
    }

    const PLAY: ControllerContext = ControllerContext {
        enabled: true,
        in_play: true,
        menu: false,
        blocked: false,
    };

    #[test]
    fn the_triggers_and_buttons_hold_their_actions_in_play() {
        let (mut controller, mut pads, mut input) = setup();
        pads.sample(vec![pad(&[], [0.0; 4])]);
        controller.update(0.016, &mut pads, &mut input, PLAY, PadCvars::default());
        pads.sample(vec![pad(&[7, 0], [0.0; 4])]);
        let frame = controller.update(0.016, &mut pads, &mut input, PLAY, PadCvars::default());
        input.update();
        assert!(input.is_active(fps::FIRE) && input.is_active(fps::JUMP));
        assert!(frame.pressed.contains(&fps::FIRE));
        assert!(!input.is_active(fps::AIM));
    }

    #[test]
    fn the_left_stick_moves_and_the_right_stick_turns() {
        let (mut controller, mut pads, mut input) = setup();
        pads.sample(vec![pad(&[], [0.0; 4])]);
        pads.sample(vec![pad(&[], [0.0, -1.0, 1.0, 0.0])]);
        let frame = controller.update(0.5, &mut pads, &mut input, PLAY, PadCvars::default());
        let axis = frame.move_axis.unwrap();
        assert!((axis.forward - 1.0).abs() < 1e-12 && axis.right.abs() < 1e-12);
        // Full right for half a second at 240°/s: 120°.
        assert!((frame.look.0 - 120f64.to_radians()).abs() < 1e-9);
        assert_eq!(frame.look.1, 0.0);
        let swapped = PadCvars {
            swap: true,
            ..PadCvars::default()
        };
        let frame = controller.update(0.5, &mut pads, &mut input, PLAY, swapped);
        assert!((frame.move_axis.unwrap().right - 1.0).abs() < 1e-12);
    }

    #[test]
    fn menu_opens_the_match_menu_and_the_menu_takes_the_pad() {
        let (mut controller, mut pads, mut input) = setup();
        pads.sample(vec![pad(&[], [0.0; 4])]);
        controller.update(0.016, &mut pads, &mut input, PLAY, PadCvars::default());
        pads.sample(vec![pad(&[9], [0.0; 4])]);
        assert!(
            controller
                .update(0.016, &mut pads, &mut input, PLAY, PadCvars::default())
                .menu
        );
        let menu = ControllerContext {
            in_play: false,
            menu: true,
            ..PLAY
        };
        // Released, then the D-pad steps and A confirms.
        pads.sample(vec![pad(&[], [0.0; 4])]);
        controller.update(0.016, &mut pads, &mut input, menu, PadCvars::default());
        pads.sample(vec![pad(&[13], [0.0; 4])]);
        let frame = controller.update(0.016, &mut pads, &mut input, menu, PadCvars::default());
        assert_eq!(frame.nav, [UiIntent::Down]);
        // Held: repeats after 0.35 s, then every 0.1 s.
        pads.sample(vec![pad(&[13], [0.0; 4])]);
        assert!(
            controller
                .update(0.2, &mut pads, &mut input, menu, PadCvars::default())
                .nav
                .is_empty()
        );
        pads.sample(vec![pad(&[13], [0.0; 4])]);
        assert_eq!(
            controller
                .update(0.2, &mut pads, &mut input, menu, PadCvars::default())
                .nav,
            [UiIntent::Down]
        );
        pads.sample(vec![pad(&[], [0.0; 4])]);
        controller.update(0.016, &mut pads, &mut input, menu, PadCvars::default());
        pads.sample(vec![pad(&[0], [0.0; 4])]);
        assert_eq!(
            controller
                .update(0.016, &mut pads, &mut input, menu, PadCvars::default())
                .nav,
            [UiIntent::Confirm]
        );
        pads.sample(vec![pad(&[1], [0.0; 4])]);
        assert_eq!(
            controller
                .update(0.016, &mut pads, &mut input, menu, PadCvars::default())
                .nav,
            [UiIntent::Back]
        );
    }

    #[test]
    fn a_keyboard_player_is_left_alone() {
        let (mut controller, mut pads, mut input) = setup();
        pads.sample(vec![pad(&[7], [0.0, -1.0, 0.0, 0.0])]);
        let off = ControllerContext { enabled: false, ..PLAY };
        let frame = controller.update(0.016, &mut pads, &mut input, off, PadCvars::default());
        input.update();
        assert!(!input.is_active(fps::FIRE));
        assert_eq!(frame.move_axis, None);
    }

    #[test]
    fn the_scoreboard_hold_is_told_when_it_changes() {
        let (mut controller, mut pads, mut input) = setup();
        pads.sample(vec![pad(&[], [0.0; 4])]);
        controller.update(0.016, &mut pads, &mut input, PLAY, PadCvars::default());
        pads.sample(vec![pad(&[8], [0.0; 4])]);
        let frame = controller.update(0.016, &mut pads, &mut input, PLAY, PadCvars::default());
        assert_eq!(frame.scoreboard, Some(true));
        pads.sample(vec![pad(&[8], [0.0; 4])]);
        let frame = controller.update(0.016, &mut pads, &mut input, PLAY, PadCvars::default());
        assert_eq!(frame.scoreboard, None);
    }
}
