//! The three-slot loadout at the bottom right (wash-fps
//! `client/hud/widgets/WeaponSlots.ts`, styles `.fps-weapon-slot`): the
//! blade in slot 1, the weapons picked up in 2 and 3, each in its colour,
//! the one in hand larger.

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, drop_print_sm, hsla, ink_text_sm};

/// `WEAPON_SLOT_COLORS`: hue, saturation and lightness.
#[must_use]
pub fn weapon_slot_color(id: &str) -> Option<[f32; 3]> {
    Some(match id {
        "blade" => [172.0, 72.0, 44.0],
        "machinegun" => [210.0, 6.0, 62.0],
        "riotgun" => [28.0, 92.0, 56.0],
        "glauncher" => [218.0, 88.0, 60.0],
        "rlauncher" => [0.0, 84.0, 56.0],
        "plasmagun" => [125.0, 74.0, 50.0],
        "voltaic" | "lasergun" => [52.0, 96.0, 56.0],
        "electrobolt" => [186.0, 96.0, 60.0],
        _ => return None,
    })
}

/// `WEAPON_ABBR`.
fn abbreviation(id: &str) -> String {
    match id {
        "blade" => "BL".into(),
        "machinegun" => "MG".into(),
        "riotgun" => "RG".into(),
        "glauncher" => "GL".into(),
        "rlauncher" => "RL".into(),
        "plasmagun" => "PG".into(),
        "voltaic" => "VT".into(),
        "electrobolt" => "EB".into(),
        other => other.chars().take(2).collect::<String>().to_uppercase(),
    }
}

/// `FPSWeaponSlots`.
#[derive(Debug, Default)]
pub struct WeaponSlots {
    loadout: [Option<String>; 3],
    current: String,
}

const CLIP: [[f32; 2]; 4] = [[0.12, 0.0], [1.0, 0.08], [0.91, 1.0], [0.0, 0.88]];
const GAP: f32 = 7.0;
/// `fps_weapon_1` to `_3`'s default keys.
const KEYS: [&str; 3] = ["1", "2", "3"];

impl WeaponSlots {
    fn slot_size(&self, i: usize) -> [f32; 2] {
        let active = self.loadout[i].as_deref().is_some_and(|w| w == self.current);
        if active { [100.0, 128.0] } else { [86.0, 116.0] }.map(|v| v * UI_SCALE)
    }
}

impl FpsHudWidget for WeaponSlots {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let state = frame.state;
        let mut loadout: [Option<String>; 3] = [None, None, None];
        let mut pick = 1;
        for w in &state.weapons {
            if w == "blade" {
                loadout[0] = Some(w.clone());
            } else if pick <= 2 {
                loadout[pick] = Some(w.clone());
                pick += 1;
            }
        }
        if loadout[0].is_none() {
            loadout[0] = Some("blade".into());
        }
        self.loadout = loadout;
        self.current.clone_from(&state.current_weapon);
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        let w: f32 = (0..3).map(|i| self.slot_size(i)[0]).sum::<f32>() + 2.0 * GAP * UI_SCALE;
        let h = (0..3).map(|i| self.slot_size(i)[1]).fold(0.0, f32::max);
        [w, h]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let [_, height] = self.size(hud, fonts);
        let mut left = x;
        for (i, key_label) in KEYS.iter().enumerate() {
            let [w, h] = self.slot_size(i);
            // align-items: flex-end.
            let top = y + height - h;
            let id = self.loadout[i].as_deref();
            let active = id.is_some_and(|w| w == self.current);
            let [hue, sat, light] = id.and_then(weapon_slot_color).unwrap_or([190.0, 84.0, 54.0]);
            let slot = |a: f32| hsla(hue, sat, light, a);
            // `--empty` fades the whole slot to half.
            let fade = if id.is_none() { 0.5 } else { 1.0 };
            let background = match (id, active) {
                (None, _) => Gradient::new(
                    120.0,
                    &[
                        (hsla(200.0, 20.0, 38.0, 0.12 * fade), None),
                        (hsla(230.0, 25.0, 9.0, 0.72 * fade), Some(0.58)),
                        (hsla(255.0, 22.0, 5.0, 0.86 * fade), Some(1.0)),
                    ],
                ),
                (Some(_), true) => Gradient::new(
                    120.0,
                    &[
                        (slot(0.72), None),
                        (slot(0.3), Some(0.5)),
                        (hsla(245.0, 30.0, 7.0, 0.96), Some(1.0)),
                    ],
                ),
                (Some(_), false) => Gradient::new(
                    120.0,
                    &[
                        (slot(0.42), None),
                        (hsla(230.0, 42.0, 11.0, 0.82), Some(0.58)),
                        (hsla(255.0, 26.0, 5.0, 0.92), Some(1.0)),
                    ],
                ),
            };
            let shadow = if active {
                Some(([8.0, 9.0], hsla(250.0, 45.0, 4.0, 0.8)))
            } else {
                drop_print_sm().map(|(o, c)| (o, [c[0], c[1], c[2], c[3] * fade]))
            };
            Slab {
                clip: &CLIP,
                background,
                skew_deg: 0.0,
                shadow,
            }
            .draw(hud, left, top, w, h);
            if id.is_some() {
                // `::before`: the weapon's colour along the bottom.
                hud.rect(
                    left + 8.0 * s,
                    top + h - 9.0 * s - 4.0 * s,
                    w - 18.0 * s,
                    4.0 * s,
                    slot(0.62),
                );
            }
            let (key_color, name_color) = match (id, active) {
                (None, _) => (
                    hsla(210.0, 18.0, 72.0, 0.28 * fade),
                    hsla(210.0, 18.0, 72.0, 0.28 * fade),
                ),
                (Some(_), true) => (hsla(42.0, 95.0, 92.0, 1.0), hsla(42.0, 95.0, 92.0, 1.0)),
                (Some(_), false) => (slot(0.75), hsla(42.0, 95.0, 92.0, 0.82)),
            };
            let key = fonts.display(13.0, key_color).align(Align::Right);
            let (ascent, _) = hud.line_metrics(&key);
            hud.text(left + w - 12.0 * s, top + 10.0 * s + ascent, key_label, &key);
            let name = fonts.display(30.0, name_color).align(Align::Center);
            let name = match (id, active) {
                (None, _) => name,
                (Some(_), true) => name.shadow([3.0 * s, 4.0 * s], hsla(250.0, 40.0, 5.0, 1.0)),
                (Some(_), false) => ink_text_sm(name),
            };
            let label = id.map_or_else(|| "--".to_owned(), abbreviation);
            hud.text_in_box(left + w / 2.0, top, h, &label, &name);
            left += w + GAP * s;
        }
    }
}
