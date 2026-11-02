//! "FRAGGED <name>" at the centre when the player on screen takes a frag
//! (wash-fps `client/hud/widgets/FragMessage.ts`, styles
//! `.fps-frag-message`, `fps-float-up` over 2.5 s, removed at 2.6 s).

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, hsla};

const LIFETIME: f64 = 2.6;
const FLOAT: f64 = 2.5;

/// `FPSFragMessage`.
#[derive(Debug, Default)]
pub struct FragMessage {
    seen_seq: Option<u64>,
    /// Victim name and age in seconds, oldest first.
    messages: Vec<(String, f64)>,
}

impl FpsHudWidget for FragMessage {
    fn update(&mut self, frame: &HudFrame<'_>) {
        for m in &mut self.messages {
            m.1 += frame.dt;
        }
        self.messages.retain(|m| m.1 < LIFETIME);
        let feed = &frame.state.kill_feed;
        let newest = feed.last().map_or(0, |k| k.seq);
        let Some(seen) = self.seen_seq else {
            self.seen_seq = Some(newest);
            return;
        };
        if newest <= seen {
            return;
        }
        for kill in feed.iter().filter(|k| k.seq > seen) {
            if kill.by_viewed && !kill.of_viewed {
                self.messages
                    .push((format!("FRAGGED {}", kill.victim_name).to_uppercase(), 0.0));
            }
        }
        self.seen_seq = Some(newest);
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [0.0, 0.0]
    }

    /// Centred on `x`, the column starting at `y` (31 % down).
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let mut top = y;
        for (text, age) in &self.messages {
            // fps-float-up: in over the first 20 %, out over the last 20 %.
            let t = (age / FLOAT).min(1.0) as f32;
            let (alpha, shift) = if t < 0.2 {
                (t / 0.2, 10.0 * (1.0 - t / 0.2))
            } else if t > 0.8 {
                (1.0 - (t - 0.8) / 0.2, -10.0 * (t - 0.8) / 0.2)
            } else {
                (1.0, 0.0)
            };
            let fade = |mut c: [f32; 4]| {
                c[3] *= alpha;
                c
            };
            let style = fonts
                .display(20.0, fade(hsla(250.0, 45.0, 8.0, 1.0)))
                .spacing(20.0 * s * 0.08)
                .align(Align::Center);
            let w = hud.measure(text, &style) + 52.0 * s;
            let h = 20.0 * s + 16.0 * s;
            let box_y = top + shift * s;
            Slab {
                clip: &[[0.03, 0.0], [1.0, 0.0], [0.97, 1.0], [0.0, 1.0]],
                background: Gradient::new(
                    180.0,
                    &[
                        (fade(hsla(51.0, 100.0, 73.0, 1.0)), None),
                        (fade(hsla(46.0, 100.0, 58.0, 1.0)), Some(0.55)),
                        (fade(hsla(42.0, 100.0, 50.0, 1.0)), Some(1.0)),
                    ],
                ),
                skew_deg: 0.0,
                shadow: None,
            }
            .draw(hud, x - w / 2.0, box_y, w, h);
            hud.text_in_box(x, box_y + 7.0 * s, 20.0 * s, text, &style);
            top += h + 4.0;
        }
    }
}
