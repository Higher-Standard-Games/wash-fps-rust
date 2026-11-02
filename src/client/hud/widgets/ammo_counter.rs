//! The ammo at the bottom right (wash-fps
//! `client/hud/widgets/AmmoCounter.ts`, styles `.fps-ammo-counter`): the
//! clip or pool, a clip gun's reserve, the weapon's name and a bar.
//!
//! Left out: the loss flash, the low-ammo neon pulse, the bottom tilt.

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::ammo_status::HudWeaponAmmo;
use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{
    Fonts, Gradient, Slab, UI_SCALE, cream, drop_print_md, hsla, ink_text_md, ink_text_sm,
};
use crate::shared::entities::definitions::weapons::weapons_table::{
    weapon_clip_size, weapon_max_ammo, weapon_reload_model,
};

/// `FPSAmmoCounter`.
#[derive(Debug, Default)]
pub struct AmmoCounter {
    weapon: String,
    number: String,
    reserve: Option<String>,
    fill: f32,
}

const CLIP: [[f32; 2]; 4] = [[0.12, 0.0], [1.0, 0.1], [0.94, 1.0], [0.0, 0.88]];

impl FpsHudWidget for AmmoCounter {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let state = frame.state;
        let weapon = state.current_weapon.as_str();
        let ammo = state.ammo.get(weapon).copied();
        let clip_gun = weapon_reload_model(weapon) == Some("clip");
        let (primary, reserve) = match ammo {
            Some(HudWeaponAmmo::Clip { clip, reserve }) => (clip, clip_gun.then_some(reserve)),
            Some(HudWeaponAmmo::Pool(n)) => (n, None),
            None => (-1.0, None),
        };
        weapon.to_uppercase().clone_into(&mut self.weapon);
        if primary < 0.0 {
            self.number = "INF".into();
            self.reserve = None;
            self.fill = 1.0;
        } else {
            self.number = format!("{primary}");
            self.reserve = reserve.map(|r| format!("RES {r}"));
            let max = if clip_gun {
                weapon_clip_size(weapon).unwrap_or(1.0)
            } else {
                weapon_max_ammo(weapon).unwrap_or(100.0)
            };
            self.fill = (primary / max).clamp(0.0, 1.0) as f32;
        }
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [300.0 * UI_SCALE, 136.0 * UI_SCALE]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let [w, h] = self.size(hud, fonts);
        Slab {
            clip: &CLIP,
            background: Gradient::new(
                105.0,
                &[
                    (hsla(42.0, 96.0, 58.0, 0.32), Some(0.0)),
                    (hsla(38.0, 92.0, 46.0, 0.88), Some(0.34)),
                    (hsla(252.0, 28.0, 7.0, 0.96), Some(0.35)),
                ],
            ),
            skew_deg: 0.0,
            shadow: drop_print_md(),
        }
        .draw(hud, x, y, w, h);
        // "AM" in the corner.
        let icon = fonts.body(13.0, hsla(42.0, 95.0, 92.0, 0.72)).spacing(13.0 * s * 0.16);
        let (ascent, _) = hud.line_metrics(&icon);
        hud.text(x + 20.0 * s, y + 20.0 * s + ascent, "AM", &icon);
        // The content column, centred between 18 px and 16 px of padding.
        let (left, right) = (x + 42.0 * s, x + w - 24.0 * s);
        let number = ink_text_md(fonts.display(64.0, cream()));
        let weapon = fonts
            .body(13.0, hsla(42.0, 95.0, 88.0, 0.6))
            .spacing(13.0 * s * 0.12)
            .align(Align::Right);
        let (n_ascent, n_descent) = hud.line_metrics(&number);
        let (w_ascent, w_descent) = hud.line_metrics(&weapon);
        let number_h = 64.0 * s;
        let weapon_h = w_ascent + w_descent;
        let track_h = 8.0 * s;
        let gap = 10.0 * s;
        let content_h = number_h + gap + weapon_h + gap + track_h;
        let top = y + 18.0 * s + (h - 34.0 * s - content_h) / 2.0;
        let baseline = top + (number_h - (n_ascent + n_descent)) / 2.0 + n_ascent;
        hud.text(left, baseline, &self.number, &number);
        if let Some(reserve) = &self.reserve {
            let style = ink_text_sm(fonts.display(20.0, hsla(42.0, 95.0, 88.0, 0.55)))
                .spacing(20.0 * s * 0.06)
                .align(Align::Right);
            hud.text(right, baseline, reserve, &style);
        }
        let weapon_top = top + number_h + gap;
        hud.text(right, weapon_top + w_ascent, &self.weapon, &weapon);
        // The bar.
        let track_y = weapon_top + weapon_h + gap;
        let track_w = right - left;
        Slab {
            clip: &[[0.0, 0.0], [1.0, 0.0], [0.96, 1.0], [0.0, 1.0]],
            background: Gradient::solid(hsla(250.0, 50.0, 5.0, 0.88)),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, left, track_y, track_w, track_h);
        hud.rect(left, track_y, track_w * self.fill, track_h, hsla(42.0, 96.0, 55.0, 1.0));
    }
}
