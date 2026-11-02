//! "OUT OF AMMO" and "RELOAD" over the bottom of the screen (wash-fps
//! `client/hud/widgets/AmmoWarning.ts`, styles `.fps-ammo-warning`), read
//! from the same `AmmoStatus` as the crosshair's red cross.

use shagger::ui::hud_2d::Hud2d;

use crate::client::ammo_status::get_ammo_status;
use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream, hsla};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Kind {
    #[default]
    None,
    Out,
    Reload,
}

/// `FPSAmmoWarning`.
#[derive(Debug, Default)]
pub struct AmmoWarning {
    kind: Kind,
    /// For the pulse (`fps-ammo-warning-pulse 0.9s`).
    clock: f64,
}

const CLIP: [[f32; 2]; 4] = [[0.02, 0.0], [1.0, 0.0], [0.98, 1.0], [0.0, 1.0]];
/// The reload key's label (`FPS.RELOAD`'s default bind).
const RELOAD_KEY: &str = "R";

impl AmmoWarning {
    fn label_style(&self, fonts: &Fonts) -> shagger::ui::hud_2d::TextStyle {
        let s = UI_SCALE;
        let (color, shadow) = if self.kind == Kind::Out {
            (hsla(48.0, 100.0, 94.0, 1.0), hsla(0.0, 60.0, 12.0, 0.8))
        } else {
            (hsla(246.0, 40.0, 8.0, 1.0), hsla(45.0, 100.0, 78.0, 0.7))
        };
        fonts
            .display(20.0, color)
            .spacing(20.0 * s * 0.06)
            .shadow([1.0 * s, 1.0 * s], shadow)
    }

    fn keycap_style(fonts: &Fonts) -> shagger::ui::hud_2d::TextStyle {
        fonts.display(20.0 * 0.84, cream())
    }
}

impl FpsHudWidget for AmmoWarning {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.clock += frame.dt;
        let state = frame.state;
        self.kind = Kind::None;
        if !state.dead {
            let status = get_ammo_status(&state.current_weapon, state.ammo.get(&state.current_weapon).copied());
            if status.out_of_ammo {
                self.kind = Kind::Out;
            } else if status.low_clip {
                self.kind = Kind::Reload;
            }
        }
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let s = UI_SCALE;
        let label = self.label_style(fonts);
        let text = match self.kind {
            Kind::None => return [0.0, 0.0],
            Kind::Out => hud.measure("OUT OF AMMO", &label),
            Kind::Reload => {
                hud.measure("RELOAD ", &label)
                    + hud.measure(RELOAD_KEY, &Self::keycap_style(fonts)).max(4.0 * s)
                    + 22.0 * s
            }
        };
        [text + 60.0 * s, 20.0 * s * 1.25 + 18.0 * s]
    }

    /// Centred on `x`, its bottom at `y`.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        if self.kind == Kind::None {
            return;
        }
        let s = UI_SCALE;
        // The pulse: up to 4 % larger halfway through.
        let phase = (self.clock % 0.9) / 0.9;
        let k = 1.0 + 0.04 * (0.5 - 0.5 * (phase * std::f64::consts::TAU).cos()) as f32;
        let [w, h] = self.size(hud, fonts).map(|v| v * k);
        let (left, top) = (x - w / 2.0, y - h);
        let background = if self.kind == Kind::Out {
            Gradient::new(
                178.0,
                &[
                    (hsla(2.0, 92.0, 60.0, 1.0), None),
                    (hsla(356.0, 82.0, 46.0, 1.0), Some(0.72)),
                ],
            )
        } else {
            Gradient::new(
                178.0,
                &[
                    (hsla(45.0, 100.0, 62.0, 1.0), None),
                    (hsla(42.0, 96.0, 52.0, 1.0), Some(0.7)),
                ],
            )
        };
        Slab {
            clip: &CLIP,
            background,
            skew_deg: 0.0,
            shadow: Some(([5.0, 6.0], hsla(250.0, 45.0, 4.0, 0.55))),
        }
        .draw(hud, left, top, w, h);
        let label = self.label_style(fonts);
        let text_left = left + 30.0 * s * k;
        if self.kind == Kind::Out {
            hud.text_in_box(text_left, top + 8.0 * s, h - 18.0 * s, "OUT OF AMMO", &label);
            return;
        }
        let word = hud.text_in_box(text_left, top + 8.0 * s, h - 18.0 * s, "RELOAD ", &label);
        // The keycap chip.
        let cap = Self::keycap_style(fonts);
        let cap_w = (hud.measure(RELOAD_KEY, &cap) + 16.0 * s).max(20.0 * s);
        let cap_h = 20.0 * 0.84 * s + 4.0 * s;
        let cap_x = text_left + word + 3.0 * s;
        let cap_y = top + (h - cap_h) / 2.0;
        hud.rect(cap_x, cap_y, cap_w, cap_h, hsla(246.0, 42.0, 10.0, 1.0));
        hud.text_in_box(cap_x + 8.0 * s, cap_y, cap_h, RELOAD_KEY, &cap);
    }
}
