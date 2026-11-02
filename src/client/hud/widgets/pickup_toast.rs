//! "+ ROCKET LAUNCHER" over the bottom centre (wash-fps
//! `client/hud/widgets/PickupToast.ts`, styles `.fps-pickup-toast`): the
//! server's `pickup_notify` label on a gold plate that slams in, holds, and
//! tumbles away; a new pickup replaces the one showing.

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, hsla};

const LIFE: f64 = 2.3;

/// `FPSPickupToast`.
#[derive(Debug, Default)]
pub struct PickupToast {
    label: String,
    age: f64,
}

const CLIP: [[f32; 2]; 5] = [[0.02, 0.08], [0.98, 0.0], [1.0, 0.86], [0.52, 1.0], [0.0, 0.92]];

impl PickupToast {
    /// `showPickup`.
    pub fn show_pickup(&mut self, text: &str) {
        self.label = text.trim_start().trim_start_matches('+').trim_start().to_uppercase();
        self.age = 0.0;
    }

    /// `fps-pickup-life`: scale, opacity and drop at this age.
    fn life(&self) -> (f32, f32, f32) {
        let t = (self.age / LIFE) as f32;
        let lerp = |a: f32, b: f32, f: f32| a + (b - a) * f;
        let keys = [
            (0.0, 1.8, 0.0),
            (0.07, 0.93, 1.0),
            (0.13, 1.05, 1.0),
            (0.18, 1.0, 1.0),
            (0.72, 1.0, 1.0),
        ];
        if t >= 0.72 {
            let f = ((t - 0.72) / 0.28).min(1.0);
            return (lerp(1.0, 0.88, f), lerp(1.0, 0.0, f), 46.0 * f);
        }
        for pair in keys.windows(2) {
            let ((t0, s0, o0), (t1, s1, o1)) = (pair[0], pair[1]);
            if t <= t1 {
                let f = (t - t0) / (t1 - t0);
                return (lerp(s0, s1, f), lerp(o0, o1, f), 0.0);
            }
        }
        (1.0, 1.0, 0.0)
    }
}

impl FpsHudWidget for PickupToast {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.age += frame.dt;
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        if self.label.is_empty() || self.age >= LIFE {
            return [0.0, 0.0];
        }
        let s = UI_SCALE;
        let plus = hud.measure("+", &fonts.display(26.0, [0.0; 4]));
        let label = hud.measure(&self.label, &fonts.display(22.0, [0.0; 4]).spacing(22.0 * s * 0.04));
        [plus + 10.0 * s + label + 80.0 * s, 26.0 * s + 25.0 * s]
    }

    /// Centred on `x`, its bottom at `y`.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let [w0, h0] = self.size(hud, fonts);
        if w0 <= 0.0 {
            return;
        }
        let s = UI_SCALE;
        let (k, opacity, drop) = self.life();
        let (w, h) = (w0 * k, h0 * k);
        let left = x - w / 2.0;
        let top = y - 52.0 * s - h0 / 2.0 - h / 2.0 + drop * s;
        let fade = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * opacity];
        Slab {
            clip: &CLIP,
            background: Gradient::new(
                178.0,
                &[
                    (fade(hsla(46.0, 100.0, 64.0, 1.0)), None),
                    (fade(hsla(41.0, 96.0, 50.0, 1.0)), Some(0.72)),
                ],
            ),
            skew_deg: -5.0,
            shadow: Some(([5.0, 6.0], fade(hsla(250.0, 45.0, 4.0, 0.55)))),
        }
        .draw(hud, left, top, w, h);
        let plus = fonts
            .display(26.0 * k, fade(hsla(140.0, 78.0, 26.0, 1.0)))
            .shadow([1.0 * s, 1.0 * s], fade(hsla(46.0, 100.0, 82.0, 0.8)));
        let label = fonts
            .display(22.0 * k, fade(hsla(246.0, 40.0, 8.0, 1.0)))
            .spacing(22.0 * s * k * 0.04)
            .shadow([1.0 * s, 1.0 * s], fade(hsla(45.0, 100.0, 78.0, 0.7)))
            .align(Align::Left);
        let content_top = top + 11.0 * s * k;
        let content_h = h - 25.0 * s * k;
        let px = left + 40.0 * s * k;
        let pw = hud.text_in_box(px, content_top, content_h, "+", &plus);
        hud.text_in_box(px + pw + 10.0 * s * k, content_top, content_h, &self.label, &label);
    }
}
