//! The big centre-top messages (wash-fps
//! `client/hud/widgets/PhaseMessage.ts`, styles `.fps-phase-*`): warmup,
//! the countdown's numbers, "Fight!", the result, overtime; a timeout, who
//! called it and the time left, its count back in, and "Fight!" when play
//! resumes.

use shagger::shared::json::Value;
use shagger::shared::net::message_codec::MatchPauseState;
use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{
    Fonts, Gradient, Slab, UI_SCALE, cream, drop_print_md, drop_print_sm, hsla, ink, ink_text_md, ink_text_sm, void,
};
use crate::client::match_state_manager::{MatchPhase, MatchStateManager};

#[derive(Debug, Clone, PartialEq)]
enum Line {
    /// `.fps-phase-title`: a cyan slab.
    Title(String),
    /// `.fps-phase-subtitle`.
    Subtitle(String),
    /// `.fps-phase-fight`: a red slab.
    Fight(String),
    /// `.fps-phase-countdown`.
    Countdown(u32),
    /// `.fps-phase-ready`: a line to act on.
    Ready(String),
}

/// `FPSPhaseMessage`.
#[derive(Debug, Default)]
pub struct PhaseMessage {
    lines: Vec<Line>,
    hidden: bool,
    /// Seconds until a timed message hides.
    hide_in: Option<f64>,
    last_phase: Option<MatchPhase>,
    last_countdown: u32,
    last_overtime: u32,
    /// The timeout on screen, so its clock ticks without redrawing the rest.
    shown_pause: Option<String>,
}

impl PhaseMessage {
    fn show(&mut self, lines: Vec<Line>) {
        self.lines = lines;
        self.hidden = false;
    }

    /// `updatePause`: the timeout banner and its count back in. True while
    /// a pause holds the match.
    fn update_pause(&mut self, m: &MatchStateManager) -> bool {
        let Some(pause) = m.pause() else {
            if self.shown_pause.take().is_none() {
                return false;
            }
            self.show(vec![Line::Fight("FIGHT!".into())]);
            self.hide_in = Some(2.0);
            return true;
        };
        self.hide_in = None;
        let left = m.pause_seconds_left().ceil() as u64;
        if pause.state == MatchPauseState::Timein {
            let key = format!("timein:{left}");
            if self.shown_pause.as_ref() != Some(&key) && left > 0 {
                self.shown_pause = Some(key);
                self.show(vec![Line::Countdown(u32::try_from(left).unwrap_or(u32::MAX))]);
            }
            return true;
        }
        self.shown_pause = Some(format!("timeout:{}:{}", pause.caller, pause.for_drop));
        let why = if pause.for_drop {
            format!("{} dropped. Waiting for them to come back", pause.caller)
        } else {
            format!("Called by {}", pause.caller)
        };
        let clock = format!("{}:{:02}", left / 60, left % 60);
        self.show(vec![
            Line::Title("TIMEOUT".into()),
            Line::Subtitle(why.to_uppercase()),
            Line::Ready(format!("{clock} left. Type /timein in chat to resume")),
        ]);
        true
    }

    fn on_phase_change(&mut self, frame: &HudFrame<'_>, phase: MatchPhase) {
        self.hide_in = None;
        let m = frame.match_state;
        match phase {
            MatchPhase::Warmup => {
                self.last_countdown = 0;
                self.show(vec![
                    Line::Title("WARMUP".into()),
                    Line::Subtitle(m.gametype_name().to_uppercase()),
                ]);
            }
            MatchPhase::Countdown => self.last_countdown = 0,
            MatchPhase::Active => {
                self.show(vec![Line::Fight("FIGHT!".into())]);
                self.hide_in = Some(2.0);
            }
            MatchPhase::PostGame => {
                let winner = m.winner();
                let kind = winner.and_then(|w| w.get("kind")).and_then(Value::as_str);
                let name = winner.and_then(|w| w.get("name")).and_then(Value::as_str);
                let title = match (kind, name) {
                    (Some(k), Some(n)) if k != "draw" && !n.is_empty() => format!("{n} wins!"),
                    _ => "Draw!".to_owned(),
                };
                let subtitle = if m.single_flow() {
                    "GG — return to lobby for the next one"
                } else {
                    "Match over — next match starting soon..."
                };
                self.show(vec![
                    Line::Fight(title.to_uppercase()),
                    Line::Subtitle(subtitle.to_uppercase()),
                ]);
                self.hide_in = Some(3.5);
            }
            MatchPhase::RoundEnd => {}
        }
    }
}

impl FpsHudWidget for PhaseMessage {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let m = frame.match_state;
        if self.update_pause(m) {
            return;
        }
        let phase = m.phase();
        // `onPhaseChange` fires on a change, not on the first state seen.
        if let Some(last) = self.last_phase
            && last != phase
        {
            self.on_phase_change(frame, phase);
        }
        if self.last_phase.is_none() && phase != MatchPhase::Warmup {
            self.hidden = true;
        }
        self.last_phase = Some(phase);
        if let Some(t) = &mut self.hide_in {
            *t -= frame.dt;
            if *t <= 0.0 {
                self.hide_in = None;
                self.hidden = true;
            }
        }
        if phase == MatchPhase::Warmup {
            // A spectator is not asked to ready up.
            self.show(vec![
                Line::Title("WARMUP".into()),
                Line::Subtitle(m.gametype_name().to_uppercase()),
            ]);
        }
        if phase == MatchPhase::Countdown {
            let n = m.get_countdown_number();
            if n != self.last_countdown && n > 0 {
                self.last_countdown = n;
                self.show(vec![Line::Countdown(n)]);
            }
        }
        let overtime = m.overtime();
        if overtime != self.last_overtime {
            self.last_overtime = overtime;
            if overtime > 0 && phase == MatchPhase::Active {
                self.show(vec![
                    Line::Fight("OVERTIME!".into()),
                    Line::Subtitle("+1 MINUTE — NO DRAWS".into()),
                ]);
                self.hide_in = Some(3.0);
            }
        }
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [0.0, 0.0]
    }

    /// Centred on `x`, from `y` down.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        if self.hidden || self.lines.is_empty() {
            return;
        }
        let s = UI_SCALE;
        let mut top = y;
        for line in &self.lines {
            match line {
                Line::Title(text) => {
                    let style = ink_text_sm(fonts.display(26.0, cream()).spacing(26.0 * s * 0.14));
                    let w = hud.measure(text, &style) + 88.0 * s;
                    let h = 26.0 * s + 23.0 * s + 6.0 * s;
                    Slab {
                        clip: &[[0.02, 0.0], [1.0, 0.0], [0.97, 0.88], [0.52, 1.0], [0.0, 0.9]],
                        background: Gradient::new(
                            115.0,
                            &[
                                (hsla(190.0, 98.0, 58.0, 0.3), None),
                                (hsla(210.0, 62.0, 20.0, 0.9), Some(0.55)),
                                (void(), Some(1.0)),
                            ],
                        ),
                        skew_deg: -5.0,
                        shadow: drop_print_sm(),
                    }
                    .draw(hud, x - w / 2.0, top, w, h);
                    hud.text_in_box(x, top + 10.0 * s, h - 23.0 * s, text, &style.align(Align::Center));
                    top += h;
                }
                Line::Fight(text) => {
                    let style = ink_text_md(fonts.display(44.0, cream()).spacing(44.0 * s * 0.18));
                    let w = hud.measure(text, &style) + 120.0 * s;
                    let h = 44.0 * s + 32.0 * s + 8.0 * s;
                    Slab {
                        clip: &[[0.03, 0.04], [0.97, 0.0], [1.0, 0.9], [0.52, 1.0], [0.0, 0.86]],
                        background: Gradient::new(
                            105.0,
                            &[
                                (hsla(7.0, 92.0, 58.0, 0.97), None),
                                (hsla(350.0, 80.0, 42.0, 0.95), None),
                            ],
                        ),
                        skew_deg: -5.0,
                        shadow: drop_print_md(),
                    }
                    .draw(hud, x - w / 2.0, top, w, h);
                    hud.text_in_box(x, top + 14.0 * s, h - 32.0 * s, text, &style.align(Align::Center));
                    top += h;
                }
                Line::Subtitle(text) => {
                    let style = fonts
                        .body(13.0, hsla(190.0, 100.0, 78.0, 0.6))
                        .spacing(13.0 * s * 0.24)
                        .align(Align::Center);
                    let h = 13.0 * s * 1.3;
                    hud.text_in_box(x, top, h, text, &style);
                    top += h;
                }
                Line::Ready(text) => {
                    let style = fonts
                        .display(14.0, hsla(186.0, 80.0, 70.0, 1.0))
                        .spacing(14.0 * s * 0.08)
                        .align(Align::Center);
                    let h = 14.0 * s * 1.3;
                    top += 8.0 * s;
                    hud.text_in_box(x, top, h, text, &style);
                    top += h;
                }
                Line::Countdown(n) => {
                    let style = fonts
                        .display(150.0, cream())
                        .shadow([6.0 * s, 8.0 * s], ink())
                        .align(Align::Center);
                    let h = 150.0 * s;
                    hud.text_in_box(x, top, h, &n.to_string(), &style);
                    top += h;
                }
            }
            top += 10.0 * s;
        }
    }
}
