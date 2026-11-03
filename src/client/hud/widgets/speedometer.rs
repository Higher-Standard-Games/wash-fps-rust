//! The movement speed readout under the crosshair's column (wash-fps
//! `camera/FPSController.ts` `updateSpeedDisplay`, styles
//! `.fps-speedometer` in `client/hud/styles/WashikiTheme.css`): the local
//! player's horizontal speed in units a second, while `cl_showspeed` is on
//! (it is by default).

use shagger::shared::js::js_round;
use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream_dim, drop_print_sm, hsla, ink_text_sm, void};

/// `clip-path: polygon(4% 0, 96% 6%, 100% 100%, 0 92%)`.
const CLIP: [[f32; 2]; 4] = [[0.04, 0.0], [0.96, 0.06], [1.0, 1.0], [0.0, 0.92]];

/// The readout; empty while hidden.
#[derive(Debug, Default)]
pub struct Speedometer {
    text: Option<String>,
}

impl Speedometer {
    /// `updateSpeedDisplay(show, speed)`.
    pub fn update(&mut self, show: bool, speed: f64) {
        self.text = show.then(|| speed_text(speed));
    }

    /// The box in CSS pixels; zero when hidden.
    #[must_use]
    pub fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let Some(text) = &self.text else { return [0.0, 0.0] };
        let s = UI_SCALE;
        let number = fonts.display(32.0, [1.0; 4]);
        let label = fonts.body(12.0, [1.0; 4]);
        let (ascent, descent) = hud.line_metrics(&number);
        let label_w = hud.measure("u/s", &label) * 1.15;
        [
            (26.0 + 26.0 + 8.0) * s + hud.measure(text, &number) + label_w,
            (10.0 + 12.0) * s + ascent + descent,
        ]
    }

    /// Draws with the box's top left at (`x`, `y`).
    pub fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let Some(text) = &self.text else { return };
        let s = UI_SCALE;
        let [w, h] = self.size(hud, fonts);
        Slab {
            clip: &CLIP,
            background: Gradient::new(
                115.0,
                &[
                    (hsla(190.0, 98.0, 58.0, 0.2), None),
                    (hsla(230.0, 60.0, 10.0, 0.85), Some(0.5)),
                    (void(), Some(1.0)),
                ],
            ),
            skew_deg: 0.0,
            shadow: drop_print_sm(),
        }
        .draw(hud, x, y, w, h);
        // `box-shadow: inset 0 1px 0`: a light line along the top.
        hud.rect(x + 0.04 * w, y, 0.92 * w, s, hsla(190.0, 95.0, 75.0, 0.3));
        let number = ink_text_sm(fonts.display(32.0, hsla(185.0, 98.0, 68.0, 1.0))).align(Align::Left);
        let label = fonts.body(12.0, cream_dim());
        let (ascent, _) = hud.line_metrics(&number);
        let baseline = y + 10.0 * s + ascent;
        let number_w = hud.measure(text, &number);
        hud.text(x + 26.0 * s, baseline, text, &number);
        hud.text(x + 26.0 * s + number_w + 8.0 * s, baseline, "u/s", &label);
    }
}

/// `String(Math.round(speed))`.
#[must_use]
pub fn speed_text(speed: f64) -> String {
    let rounded = js_round(speed);
    // `String(-0)` is "0".
    format!("{}", if rounded == 0.0 { 0.0 } else { rounded })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_as_math_round() {
        assert_eq!(speed_text(0.0), "0");
        assert_eq!(speed_text(319.5), "320");
        assert_eq!(speed_text(-0.5), "0");
        assert_eq!(speed_text(412.49), "412");
    }

    #[test]
    fn hidden_draws_nothing() {
        let mut m = Speedometer::default();
        m.update(false, 300.0);
        assert!(m.text.is_none());
        m.update(true, 300.2);
        assert_eq!(m.text.as_deref(), Some("300"));
    }
}
