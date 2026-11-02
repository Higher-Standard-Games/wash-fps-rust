//! The frag wing left of the clock (wash-fps
//! `client/hud/widgets/FragCounter.ts`, styles `.fps-frag-counter`): the
//! score of the player on screen.

use shagger::ui::hud_2d::Hud2d;

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Slab, TILT_TOP_SKEW, UI_SCALE, cream, drop_print_sm, hsla, ink_text_sm};
use crate::client::hud::widgets::timer::gold;

/// `FPSFragCounter`.
#[derive(Debug, Default)]
pub struct FragCounter {
    score: Option<f64>,
}

const CLIP: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.06, 0.86]];
const LABEL: &str = "FRAGS";

impl FragCounter {
    fn number(&self) -> String {
        format!("{}", self.score.unwrap_or(0.0))
    }
}

impl FpsHudWidget for FragCounter {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let state = frame.state;
        let subject = state.subject();
        if subject.is_empty() {
            self.score = None;
            return;
        }
        let score = state
            .scoreboard
            .iter()
            .find(|e| e.player_id == subject || e.entity_id == subject)
            .map_or(0.0, |e| e.score);
        self.score = Some(score);
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        if self.score.is_none() {
            return [0.0, 0.0];
        }
        let s = UI_SCALE;
        let number = hud.measure(&self.number(), &fonts.display(28.0, cream()));
        let label = hud.measure(LABEL, &fonts.body(11.0, cream()).spacing(11.0 * s * 0.2));
        [26.0 * s + number + 8.0 * s + label + 36.0 * s, 54.0 * s]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        if self.score.is_none() {
            return;
        }
        let s = UI_SCALE;
        let [w, h] = self.size(hud, fonts);
        Slab {
            clip: &CLIP,
            background: gold(),
            skew_deg: TILT_TOP_SKEW,
            shadow: drop_print_sm(),
        }
        .draw(hud, x, y, w, h);
        let number_style = ink_text_sm(fonts.display(28.0, cream()));
        let (ascent, descent) = hud.line_metrics(&number_style);
        // align-items: baseline in the 30-pixel content box.
        let baseline = y + 12.0 * s + (30.0 * s + ascent - descent) / 2.0;
        let width = hud.text(x + 26.0 * s, baseline, &self.number(), &number_style);
        let label = fonts.body(11.0, hsla(255.0, 45.0, 10.0, 0.78)).spacing(11.0 * s * 0.2);
        hud.text(x + 26.0 * s + width + 8.0 * s, baseline, LABEL, &label);
    }
}
