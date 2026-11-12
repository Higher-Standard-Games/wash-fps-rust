//! The warmup ready bar (wash-fps `client/hud/widgets/ReadyBar.ts`, styles
//! `.fps-ready-bar`): a two-cell plate 176 px down, a gold count of who is
//! ready and "Press F4 when ready", green once we are; a magenta
//! "Spectating" plate for a spectator, in or out of warmup. Hidden in a
//! recording.

use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, drop_print_sm, hsla, void};
use crate::client::match_state_manager::MatchPhase;

/// `clip-path: polygon(1.5% 0, 100% 0, 98.5% 100%, 0 94%)`.
const CLIP: [[f32; 2]; 4] = [[0.015, 0.0], [1.0, 0.0], [0.985, 1.0], [0.0, 0.94]];
/// `.fps-ready-count`'s clip.
const COUNT_CLIP: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.9, 1.0], [0.0, 1.0]];
/// `.fps-ready-key`'s clip.
const KEY_CLIP: [[f32; 2]; 4] = [[0.06, 0.0], [1.0, 0.08], [0.94, 1.0], [0.0, 0.92]];

/// What the bar shows.
#[derive(Debug, Clone, PartialEq)]
enum Shown {
    Hidden,
    /// The ready count ("2/4 Ready") and whether we are.
    Player {
        count: String,
        ready: bool,
    },
    Spectating,
}

/// `FPSReadyBar`.
#[derive(Debug)]
pub struct ReadyBar {
    shown: Shown,
    now: f64,
    /// The ready key with a controller in hand (`getActionKeyLabel(READY)`);
    /// F4 otherwise.
    pub pad_key: Option<String>,
}

impl Default for ReadyBar {
    fn default() -> Self {
        Self {
            shown: Shown::Hidden,
            now: 0.0,
            pad_key: None,
        }
    }
}

impl ReadyBar {
    fn shown_for(playback: bool, watcher: bool, warmup: bool, count: String, ready: bool) -> Shown {
        if playback {
            Shown::Hidden
        } else if warmup && !watcher {
            Shown::Player { count, ready }
        } else if watcher {
            Shown::Spectating
        } else {
            Shown::Hidden
        }
    }

    fn count_style(fonts: &Fonts) -> TextStyle {
        fonts
            .display(19.0, hsla(255.0, 45.0, 10.0, 0.92))
            .spacing(19.0 * UI_SCALE * 0.08)
            .align(Align::Left)
    }

    fn text_style(fonts: &Fonts, color: [f32; 4], spacing: f32) -> TextStyle {
        fonts
            .body(20.0, color)
            .spacing(20.0 * UI_SCALE * spacing)
            .align(Align::Left)
    }

    fn key_style(fonts: &Fonts) -> TextStyle {
        fonts.display(18.0, hsla(255.0, 45.0, 10.0, 1.0)).align(Align::Left)
    }

    /// The prompt's pieces: text before the key, the key, text after.
    fn prompt(&self, ready: bool) -> (&'static str, Option<String>, &'static str) {
        if ready {
            ("READY! WAITING FOR OTHERS", None, "")
        } else {
            let key = self
                .pad_key
                .as_deref()
                .map_or_else(|| "F4".to_owned(), str::to_uppercase);
            ("PRESS", Some(key), "WHEN READY")
        }
    }

    /// The count cell's and the prompt's widths, and the height.
    fn layout(&self, hud: &Hud2d, fonts: &Fonts) -> (f32, f32, f32) {
        let s = UI_SCALE;
        match &self.shown {
            Shown::Hidden => (0.0, 0.0, 0.0),
            Shown::Player { count, ready } => {
                let count_w = 16.0 * s + hud.measure(&count.to_uppercase(), &Self::count_style(fonts)) + 20.0 * s;
                let text = Self::text_style(fonts, [0.0; 4], 0.1);
                let (before, key, after) = self.prompt(*ready);
                let mut prompt = 18.0 * s + hud.measure(before, &text) + 26.0 * s;
                if let Some(key) = key.as_deref() {
                    let gap = 4.0 * s;
                    prompt += gap
                        + 6.0 * s
                        + 10.0 * s
                        + hud.measure(key, &Self::key_style(fonts))
                        + 10.0 * s
                        + 6.0 * s
                        + gap
                        + hud.measure(after, &text);
                }
                (count_w, prompt, (12.0 + 24.0 + 14.0) * s)
            }
            Shown::Spectating => {
                let text = Self::text_style(fonts, [0.0; 4], 0.3);
                (
                    0.0,
                    30.0 * s + hud.measure("SPECTATING", &text) + 30.0 * s,
                    (10.0 + 20.0 + 12.0) * s,
                )
            }
        }
    }

    /// `fps-ready-attention`: 1 to 1.08 and back every 1.5 s.
    fn attention(&self) -> f32 {
        let t = (self.now / 1.5).fract();
        let half = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
        (1.0 + 0.08 * half * half * (3.0 - 2.0 * half)) as f32
    }
}

impl FpsHudWidget for ReadyBar {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let m = frame.match_state;
        self.now = frame.state.now;
        self.shown = Self::shown_for(
            frame.state.playback,
            frame.state.watcher,
            m.phase() == MatchPhase::Warmup,
            m.get_ready_status(),
            m.is_ready(),
        );
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let (count, prompt, h) = self.layout(hud, fonts);
        [count + prompt, h]
    }

    /// Centred on `x`, its top at `y`.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        if self.shown == Shown::Hidden {
            return;
        }
        let s = UI_SCALE;
        let (count_w, prompt_w, h) = self.layout(hud, fonts);
        let w = count_w + prompt_w;
        let left = x - w / 2.0;
        let (background, ready) = match &self.shown {
            Shown::Player { ready: true, .. } => (
                Gradient::new(
                    115.0,
                    &[
                        (hsla(145.0, 80.0, 45.0, 0.35), None),
                        (hsla(160.0, 60.0, 14.0, 0.94), Some(0.6)),
                        (void(), Some(1.0)),
                    ],
                ),
                true,
            ),
            Shown::Player { .. } => (
                Gradient::new(172.0, &[(hsla(250.0, 30.0, 11.0, 0.95), None), (void(), Some(0.6))]),
                false,
            ),
            _ => (
                Gradient::new(
                    115.0,
                    &[
                        (hsla(320.0, 90.0, 60.0, 0.26), None),
                        (hsla(285.0, 45.0, 12.0, 0.94), Some(0.55)),
                        (void(), Some(1.0)),
                    ],
                ),
                false,
            ),
        };
        Slab {
            clip: &CLIP,
            background,
            skew_deg: -5.0,
            shadow: drop_print_sm(),
        }
        .draw(hud, left, y, w, h);
        match &self.shown {
            Shown::Player { count, ready: is_ready } => {
                let (from, to) = if ready {
                    (hsla(140.0, 75.0, 52.0, 1.0), hsla(152.0, 70.0, 38.0, 1.0))
                } else {
                    (hsla(42.0, 96.0, 58.0, 1.0), hsla(36.0, 90.0, 46.0, 1.0))
                };
                Slab {
                    clip: &COUNT_CLIP,
                    background: Gradient::new(105.0, &[(from, None), (to, Some(1.0))]),
                    skew_deg: -5.0,
                    shadow: None,
                }
                .draw(hud, left, y, count_w, h);
                let k = if *is_ready { 1.0 } else { self.attention() };
                let label = count.to_uppercase();
                let style = fonts
                    .display(19.0 * k, hsla(255.0, 45.0, 10.0, 0.92))
                    .spacing(19.0 * s * k * 0.08)
                    .align(Align::Left);
                let lw = hud.measure(&label, &style);
                let base = hud.measure(&label, &Self::count_style(fonts));
                hud.text_in_box(
                    left + 16.0 * s - (lw - base) / 2.0,
                    y + 12.0 * s,
                    24.0 * s,
                    &label,
                    &style,
                );
                let text = Self::text_style(fonts, hsla(42.0, 30.0, 92.0, 0.85), 0.1);
                let (before, key, after) = self.prompt(*is_ready);
                let mut at = left + count_w + 18.0 * s;
                at += hud.text_in_box(at, y + 12.0 * s, 24.0 * s, before, &text);
                if let Some(key) = key.as_deref() {
                    at += 4.0 * s + 6.0 * s;
                    let key_w = 10.0 * s + hud.measure(key, &Self::key_style(fonts)) + 10.0 * s;
                    let key_h = 26.0 * s;
                    let key_y = y + (h - key_h) / 2.0;
                    Slab {
                        clip: &KEY_CLIP,
                        background: Gradient::solid(hsla(42.0, 96.0, 56.0, 1.0)),
                        skew_deg: 0.0,
                        shadow: None,
                    }
                    .draw(hud, at, key_y, key_w, key_h);
                    hud.text_in_box(
                        at + 10.0 * s,
                        key_y + 2.0 * s,
                        key_h - 5.0 * s,
                        key,
                        &Self::key_style(fonts),
                    );
                    at += key_w + 6.0 * s + 4.0 * s;
                    hud.text_in_box(at, y + 12.0 * s, 24.0 * s, after, &text);
                }
            }
            Shown::Spectating => {
                let text = Self::text_style(fonts, hsla(320.0, 90.0, 74.0, 1.0), 0.3);
                hud.text_in_box(left + 30.0 * s, y + 10.0 * s, 20.0 * s, "SPECTATING", &text);
            }
            Shown::Hidden => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_ready_up_and_spectators_are_told_so() {
        let shown =
            |playback, watcher, warmup| ReadyBar::shown_for(playback, watcher, warmup, "1/2 Ready".into(), false);
        assert_eq!(
            shown(false, false, true),
            Shown::Player {
                count: "1/2 Ready".into(),
                ready: false
            }
        );
        assert_eq!(shown(false, true, true), Shown::Spectating);
        assert_eq!(shown(false, true, false), Shown::Spectating);
        assert_eq!(shown(false, false, false), Shown::Hidden);
        assert_eq!(shown(true, true, true), Shown::Hidden);
    }
}
