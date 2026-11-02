//! The gametype tab right of the clock (wash-fps
//! `client/hud/widgets/GametypeBadge.ts`, styles `.fps-gametype-badge`).

use shagger::ui::hud_2d::Hud2d;

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, TILT_TOP_SKEW, UI_SCALE, drop_print_sm, hsla};

/// `FPSGametypeBadge`.
#[derive(Debug, Default)]
pub struct GametypeBadge {
    name: String,
}

const CLIP: [[f32; 2]; 5] = [[0.0, 0.0], [1.0, 0.0], [0.94, 0.9], [0.42, 1.0], [0.0, 1.0]];

impl GametypeBadge {
    fn style(fonts: &Fonts) -> shagger::ui::hud_2d::TextStyle {
        fonts
            .body(12.0, hsla(190.0, 100.0, 78.0, 0.78))
            .spacing(12.0 * UI_SCALE * 0.2)
    }
}

impl FpsHudWidget for GametypeBadge {
    fn update(&mut self, frame: &HudFrame<'_>) {
        // text-transform: uppercase.
        self.name = frame.match_state.gametype_name().to_uppercase();
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let s = UI_SCALE;
        let text = hud.measure(&self.name, &Self::style(fonts));
        [32.0 * s + text + 24.0 * s, 46.0 * s]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let [w, h] = self.size(hud, fonts);
        Slab {
            clip: &CLIP,
            background: Gradient::new(
                115.0,
                &[
                    (hsla(190.0, 98.0, 58.0, 0.22), None),
                    (hsla(230.0, 70.0, 10.0, 0.85), Some(0.54)),
                    (hsla(255.0, 30.0, 5.0, 0.94), Some(1.0)),
                ],
            ),
            skew_deg: TILT_TOP_SKEW,
            shadow: drop_print_sm(),
        }
        .draw(hud, x, y, w, h);
        hud.text_in_box(x + 32.0 * s, y + 8.0 * s, h - 20.0 * s, &self.name, &Self::style(fonts));
    }
}
