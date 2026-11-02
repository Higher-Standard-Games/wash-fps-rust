//! The match clock (wash-fps `client/hud/widgets/Timer.ts`, styles
//! `.fps-timer`).

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{
    Fonts, Gradient, Slab, TILT_TOP_SKEW, UI_SCALE, cream, cream_dim, drop_print_md, hsla, ink_text_sm, void,
};
use crate::client::match_state_manager::MatchPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mood {
    #[default]
    Normal,
    /// Warmup with no time limit.
    Idle,
    /// The last 30 s.
    Warning,
    /// The last 10 s.
    Critical,
}

/// `FPSTimer`.
#[derive(Debug, Default)]
pub struct Timer {
    text: String,
    mood: Mood,
    /// For the critical flash (`w-crit-flash 0.72s steps(2)`).
    clock: f64,
}

const CLIP: [[f32; 2]; 5] = [[0.0, 0.0], [1.0, 0.0], [0.96, 0.88], [0.52, 1.0], [0.04, 0.92]];

impl FpsHudWidget for Timer {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.clock += frame.dt;
        let m = frame.match_state;
        let remaining = m.time_remaining();
        if remaining <= 0.0 && m.phase() == MatchPhase::Warmup {
            self.text = "--:--".into();
            self.mood = Mood::Idle;
            return;
        }
        self.text = m.get_time_string(false);
        self.mood = if remaining <= 10.0 {
            Mood::Critical
        } else if remaining <= 30.0 {
            Mood::Warning
        } else {
            Mood::Normal
        };
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let text = hud.measure(&self.text, &self.text_style(fonts));
        [(text + 72.0 * UI_SCALE).max(200.0 * UI_SCALE), 68.0 * UI_SCALE]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let [w, h] = self.size(hud, fonts);
        let background = match self.mood {
            Mood::Normal | Mood::Idle => Gradient::new(
                178.0,
                &[
                    (hsla(250.0, 30.0, 11.0, 0.97), None),
                    (void(), Some(0.6)),
                    (void(), Some(1.0)),
                ],
            ),
            Mood::Warning => gold(),
            Mood::Critical => {
                let bright = (self.clock % 0.72) >= 0.36;
                let k = if bright { 1.26 } else { 1.0 };
                Gradient::new(
                    105.0,
                    &[
                        (brighten(hsla(7.0, 92.0, 58.0, 0.97), k), None),
                        (brighten(hsla(350.0, 80.0, 42.0, 0.95), k), None),
                    ],
                )
            }
        };
        Slab {
            clip: &CLIP,
            background,
            skew_deg: TILT_TOP_SKEW,
            shadow: drop_print_md(),
        }
        .draw(hud, x, y, w, h);
        let s = UI_SCALE;
        // Content box: padding 6 top, 14 bottom; text centred in it.
        let content_top = y + 6.0 * s;
        let content_h = h - 20.0 * s;
        hud.text_in_box(
            x + w / 2.0,
            content_top,
            content_h,
            &self.text,
            &self.text_style(fonts).align(Align::Center),
        );
        // The caption, written top to bottom at the left.
        let caption = match self.mood {
            Mood::Warning => hsla(255.0, 45.0, 10.0, 0.7),
            Mood::Critical => hsla(42.0, 95.0, 92.0, 0.7),
            _ => hsla(190.0, 100.0, 78.0, 0.55),
        };
        let style = fonts.body(10.0, caption);
        let step = 10.0 * s * 1.22;
        for (i, ch) in "TIME".chars().enumerate() {
            hud.text(
                x + 14.0 * s,
                y + 10.0 * s + 8.0 * s + i as f32 * step,
                &ch.to_string(),
                &style,
            );
        }
    }
}

impl Timer {
    fn text_style(&self, fonts: &Fonts) -> shagger::ui::hud_2d::TextStyle {
        let color = if self.mood == Mood::Idle { cream_dim() } else { cream() };
        ink_text_sm(fonts.display(34.0, color).spacing(34.0 * UI_SCALE * 0.04))
    }
}

/// The gold field the frag wing and a late clock share.
#[must_use]
pub fn gold() -> Gradient {
    Gradient::new(
        105.0,
        &[
            (hsla(42.0, 96.0, 58.0, 0.96), None),
            (hsla(38.0, 92.0, 46.0, 0.94), Some(0.62)),
            (hsla(30.0, 86.0, 38.0, 0.92), Some(1.0)),
        ],
    )
}

pub fn brighten(c: [f32; 4], k: f32) -> [f32; 4] {
    [(c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0), c[3]]
}
