//! Health and armor at the bottom left (wash-fps
//! `client/hud/widgets/VitalsBlock.ts`, styles `.fps-vitals-*`): the red HP
//! slab with its number and bar, the armor stack's four plates behind it.
//!
//! Left out: the ink-hit and plate-chip flashes on a change, the bottom
//! tilt, and the blurred drop shadow.

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream, cream_dim, hsla, ink_text_md};
use crate::client::hud::widgets::timer::brighten;

const ARMOR_PLATE_VALUE: f64 = 25.0;
const ARMOR_PLATES: usize = 4;

/// `FPSVitalsBlock`.
#[derive(Debug, Default)]
pub struct VitalsBlock {
    health: f64,
    armor: f64,
    /// For the critical flash (`w-crit-flash 0.72s steps(2)`).
    clock: f64,
}

const DEPTH_CLIP: [[f32; 2]; 5] = [[0.05, 0.05], [0.88, 0.0], [1.0, 0.28], [0.92, 1.0], [0.0, 0.94]];
const HP_CLIP: [[f32; 2]; 5] = [[0.0, 0.12], [0.88, 0.0], [1.0, 0.23], [0.91, 1.0], [0.06, 0.91]];
const ARMOR_CLIP: [[f32; 2]; 5] = [[0.18, 0.0], [1.0, 0.08], [0.93, 0.88], [0.08, 1.0], [0.0, 0.28]];
const PLATE_CLIP: [[f32; 2]; 4] = [[0.18, 0.0], [1.0, 0.1], [0.82, 1.0], [0.0, 0.88]];

fn fade(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * a]
}

impl VitalsBlock {
    fn critical(&self) -> bool {
        self.health <= 25.0
    }
}

impl FpsHudWidget for VitalsBlock {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.clock += frame.dt;
        self.health = js_round_nonneg(frame.state.health);
        self.armor = js_round_nonneg(frame.state.armor);
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [500.0 * UI_SCALE, 184.0 * UI_SCALE]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        // The depth slab under both.
        Slab {
            clip: &DEPTH_CLIP,
            background: Gradient::solid(hsla(252.0, 25.0, 4.0, 0.92)),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, x + 26.0 * s, y + 40.0 * s, 450.0 * s, 137.0 * s);
        self.draw_armor(hud, fonts, x + 292.0 * s, y + 10.0 * s);
        self.draw_hp(hud, fonts, x, y + 40.0 * s);
    }
}

impl VitalsBlock {
    fn draw_hp(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let (w, h) = (332.0 * s, 130.0 * s);
        let k = if self.critical() && (self.clock % 0.72) >= 0.36 {
            1.26
        } else {
            1.0
        };
        Slab {
            clip: &HP_CLIP,
            background: Gradient::new(
                95.0,
                &[
                    (brighten(hsla(7.0, 92.0, 58.0, 0.96), k), Some(0.0)),
                    (brighten(hsla(350.0, 80.0, 42.0, 0.94), k), Some(0.62)),
                    (brighten(hsla(255.0, 30.0, 8.0, 0.95), k), Some(0.63)),
                ],
            ),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, x, y, w, h);
        // `::after`: the cream bar along the bottom, filled to the health.
        let bar_w = w - 30.0 * s - 64.0 * s;
        let fill = (self.health / 100.0).clamp(0.0, 1.0) as f32;
        hud.rect(
            x + 30.0 * s,
            y + h - 14.0 * s - 8.0 * s,
            bar_w * fill,
            8.0 * s,
            hsla(42.0, 95.0, 90.0, 0.9 * 0.95),
        );
        // "HP" at the top left.
        let mark = fonts.body(16.0, hsla(42.0, 95.0, 92.0, 0.9)).spacing(16.0 * s * 0.16);
        let (ascent, _) = hud.line_metrics(&mark);
        hud.text(x + 24.0 * s, y + 15.0 * s + ascent, "HP", &mark);
        // The number, its line box 0.86 em tall, 26 px off the bottom.
        let number = ink_text_md(fonts.display(92.0, cream()));
        let (ascent, descent) = hud.line_metrics(&number);
        let line = 92.0 * s * 0.86;
        let line_top = y + h - 26.0 * s - line;
        let baseline = line_top + (line - (ascent + descent)) / 2.0 + ascent;
        hud.text(x + 36.0 * s, baseline, &format!("{}", self.health), &number);
        // "VITAL", written top to bottom at the right.
        let caption = fonts.body(13.0, cream_dim()).align(Align::Center);
        let step = 13.0 * s * 1.18;
        let letters = "VITAL".chars().count() as f32;
        let bottom = y + h - 30.0 * s;
        let column = x + w - 42.0 * s - 7.0 * s;
        for (i, ch) in "VITAL".chars().enumerate() {
            let baseline = bottom - (letters - 1.0 - i as f32) * step - 3.0 * s;
            hud.text(column, baseline, &ch.to_string(), &caption);
        }
    }

    fn draw_armor(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let (w, h) = (198.0 * s, 160.0 * s);
        // `.fps-vitals--no-armor`: the stack at 58 %.
        let a = if self.armor <= 0.0 { 0.58 } else { 1.0 };
        Slab {
            clip: &ARMOR_CLIP,
            background: Gradient::new(
                115.0,
                &[
                    (fade(hsla(190.0, 98.0, 58.0, 0.24), a), None),
                    (fade(hsla(230.0, 70.0, 10.0, 0.82), a), Some(0.54)),
                    (fade(hsla(255.0, 30.0, 5.0, 0.94), a), Some(1.0)),
                ],
            ),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, x, y, w, h);
        let right = x + w - 18.0 * s;
        let label = fonts
            .body(13.0, fade(hsla(190.0, 100.0, 78.0, 0.7), a))
            .spacing(13.0 * s * 0.18)
            .align(Align::Right);
        let (ascent, descent) = hud.line_metrics(&label);
        let label_top = y + 20.0 * s;
        hud.text(right, label_top + ascent, "PLATES", &label);
        // Four plates, right-aligned, 8 px apart.
        let (pw, ph, gap) = (30.0 * s, 72.0 * s, 8.0 * s);
        let plates_top = label_top + ascent + descent + 14.0 * s;
        let first = right - ARMOR_PLATES as f32 * pw - (ARMOR_PLATES - 1) as f32 * gap;
        for i in 0..ARMOR_PLATES {
            let px = first + i as f32 * (pw + gap);
            Slab {
                clip: &PLATE_CLIP,
                background: Gradient::solid(fade(hsla(205.0, 35.0, 10.0, 0.82), a)),
                skew_deg: 0.0,
                shadow: None,
            }
            .draw(hud, px, plates_top, pw, ph);
            let start = i as f64 * ARMOR_PLATE_VALUE;
            let fill = ((self.armor - start) / ARMOR_PLATE_VALUE).clamp(0.0, 1.0) as f32;
            if fill > 0.0 {
                // `scaleX(fill)` from the left: the plate's shape, narrowed.
                Slab {
                    clip: &PLATE_CLIP,
                    background: Gradient::new(
                        180.0,
                        &[
                            (fade(hsla(185.0, 98.0, 68.0, 1.0), a), None),
                            (fade(hsla(198.0, 92.0, 46.0, 1.0), a), None),
                        ],
                    ),
                    skew_deg: 0.0,
                    shadow: None,
                }
                .draw(hud, px, plates_top, pw * fill, ph);
            }
        }
        // The number at the bottom right.
        let number = fonts
            .display(28.0, fade(hsla(190.0, 100.0, 78.0, 1.0), a))
            .shadow([1.0 * s, 2.0 * s], hsla(250.0, 40.0, 4.0, a))
            .align(Align::Right);
        let (_, descent) = hud.line_metrics(&number);
        hud.text(
            x + w - 22.0 * s,
            y + h - 16.0 * s - descent,
            &format!("{}", self.armor),
            &number,
        );
    }
}

/// `Math.max(0, Math.round(v))`.
fn js_round_nonneg(v: f64) -> f64 {
    shagger::shared::js::js_round(v).max(0.0)
}
