//! "PRESS [F] TO SWAP TO RLAUNCHER" over the bottom centre (wash-fps
//! `world/PickupPrompt.ts`, styles `.fps-pickup-prompt`), with when it asks:
//! `FPSPickup.onProximity` for map weapons and
//! `DroppedWeaponRenderer.checkPickupProximity` for weapons on the floor.
//!
//! A gold WEAPON cell, then the dark slab with the line; the `[KEY]` token
//! is a gold key cap and the last word (the weapon) is cyan. Each system
//! asks under its own name; the newest request is on screen, and withdrawing
//! it brings back the one under it.

use shagger::rendering::glam::DVec3;
use shagger::ui::hud_2d::Hud2d;

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, drop_print_sm, hsla, void};

/// `InventoryManager.MAX_WEAPONS`: the blade and two picked up.
pub const MAX_WEAPONS: usize = 3;

/// `Pickup.runProximityCheck`'s cylinder: 96 units across the floor, from
/// 24 below the item to 88 above it.
#[must_use]
pub fn in_pickup_range(player: DVec3, target: DVec3) -> bool {
    let (dx, dy, dz) = (player.x - target.x, player.y - target.y, player.z - target.z);
    dx * dx + dz * dz < 96.0 * 96.0 && dy > -24.0 && dy < 88.0
}

/// Horizontal distance squared, for the nearest weapon in range.
#[must_use]
pub fn horizontal_distance_sq(player: DVec3, target: DVec3) -> f64 {
    let (dx, dz) = (player.x - target.x, player.z - target.z);
    dx * dx + dz * dz
}

/// Whether standing at `weapon` asks to swap: the server picks it up by
/// itself when we have it or have room, and swaps silently while the blade
/// (which cannot be dropped) is in hand.
#[must_use]
pub fn asks_to_swap(weapon: &str, inventory: &[String], current: &str) -> bool {
    let already_has = inventory.iter().any(|w| w == weapon);
    let has_room = inventory.len() < MAX_WEAPONS;
    !(already_has || has_room || current == "blade")
}

/// `PRESS [F] TO SWAP TO <NAME>`.
#[must_use]
pub fn swap_text(key: &str, weapon: &str) -> String {
    format!("PRESS [{key}] TO SWAP TO {}", weapon.replace('_', " ").to_uppercase())
}

/// `FPSPickupPrompt`'s requests.
#[derive(Debug, Default)]
pub struct PickupPrompt {
    /// Each owner's line, oldest first.
    requests: Vec<(&'static str, String)>,
}

const CLIP: [[f32; 2]; 4] = [[0.015, 0.0], [1.0, 0.0], [0.985, 1.0], [0.0, 0.94]];
const CELL_CLIP: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.88, 1.0], [0.0, 1.0]];
const KEY_CLIP: [[f32; 2]; 4] = [[0.06, 0.0], [1.0, 0.08], [0.94, 1.0], [0.0, 0.92]];

/// The line in its three parts: before the key, the key, and after it, the
/// last word split off.
struct Parts<'a> {
    prefix: &'a str,
    key: Option<&'a str>,
    middle: &'a str,
    name: &'a str,
}

fn parts(text: &str) -> Parts<'_> {
    let (Some(open), Some(close)) = (text.find('['), text.find(']')) else {
        return Parts {
            prefix: text,
            key: None,
            middle: "",
            name: "",
        };
    };
    if close < open {
        return Parts {
            prefix: text,
            key: None,
            middle: "",
            name: "",
        };
    }
    let tail = text[close + 1..].trim_end();
    let (middle, name) = match tail.rfind(' ') {
        Some(i) => (&tail[..=i], &tail[i + 1..]),
        None => (tail, ""),
    };
    Parts {
        prefix: &text[..open],
        key: Some(&text[open + 1..close]),
        middle,
        name,
    }
}

impl PickupPrompt {
    /// `showPickupPrompt`.
    pub fn show(&mut self, owner: &'static str, text: String) {
        self.requests.retain(|(o, _)| *o != owner);
        self.requests.push((owner, text));
    }

    /// `hidePickupPrompt`.
    pub fn hide(&mut self, owner: &'static str) {
        self.requests.retain(|(o, _)| *o != owner);
    }

    /// Asks for `owner`'s line, or withdraws it, keeping its place when the
    /// line is unchanged.
    pub fn set(&mut self, owner: &'static str, text: Option<String>) {
        match text {
            Some(text) if self.requests.iter().any(|(o, t)| *o == owner && *t == text) => {}
            Some(text) => self.show(owner, text),
            None => self.hide(owner),
        }
    }

    /// The line on screen.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.requests.last().map(|(_, t)| t.as_str())
    }

    fn widths(&self, hud: &Hud2d, fonts: &Fonts, text: &str) -> (f32, f32, f32) {
        let s = UI_SCALE;
        let p = parts(text);
        let body = Self::text_style(fonts, hsla(42.0, 30.0, 92.0, 0.85));
        let cell = hud.measure("WEAPON", &Self::cell_style(fonts)) + 38.0 * s;
        let key = p
            .key
            .map_or(0.0, |k| hud.measure(k, &Self::key_style(fonts)) + 32.0 * s);
        let slab =
            hud.measure(p.prefix, &body) + key + hud.measure(p.middle, &body) + hud.measure(p.name, &body) + 44.0 * s;
        (cell, slab, 18.0 * s * 1.2 + 26.0 * s)
    }

    fn cell_style(fonts: &Fonts) -> shagger::ui::hud_2d::TextStyle {
        fonts
            .display(19.0, hsla(255.0, 45.0, 10.0, 0.92))
            .spacing(19.0 * UI_SCALE * 0.08)
    }

    fn text_style(fonts: &Fonts, color: [f32; 4]) -> shagger::ui::hud_2d::TextStyle {
        fonts.body(18.0, color).spacing(18.0 * UI_SCALE * 0.1)
    }

    fn key_style(fonts: &Fonts) -> shagger::ui::hud_2d::TextStyle {
        fonts.display(16.0, hsla(255.0, 45.0, 10.0, 1.0))
    }
}

impl FpsHudWidget for PickupPrompt {
    fn update(&mut self, _frame: &HudFrame<'_>) {}

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let Some(text) = self.text() else {
            return [0.0, 0.0];
        };
        let (cell, slab, h) = self.widths(hud, fonts, text);
        [cell + slab, h]
    }

    /// Centred on `x`, its bottom at `y`.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let Some(text) = self.text() else { return };
        let s = UI_SCALE;
        let (cell_w, slab_w, h) = self.widths(hud, fonts, text);
        let w = cell_w + slab_w;
        let (left, top) = (x - w / 2.0, y - h);
        Slab {
            clip: &CLIP,
            background: Gradient::new(172.0, &[(hsla(250.0, 30.0, 11.0, 0.95), None), (void(), Some(0.6))]),
            skew_deg: -5.0,
            shadow: drop_print_sm(),
        }
        .draw(hud, left, top, w, h);
        Slab {
            clip: &CELL_CLIP,
            background: Gradient::new(
                105.0,
                &[(hsla(42.0, 96.0, 58.0, 1.0), None), (hsla(36.0, 90.0, 46.0, 1.0), None)],
            ),
            skew_deg: -5.0,
            shadow: None,
        }
        .draw(hud, left, top, cell_w, h);
        hud.text_in_box(left + 16.0 * s, top, h, "WEAPON", &Self::cell_style(fonts));

        let p = parts(text);
        let body = Self::text_style(fonts, hsla(42.0, 30.0, 92.0, 0.85));
        let mut at = left + cell_w + 18.0 * s;
        at += hud.text_in_box(at, top, h, p.prefix, &body);
        if let Some(key) = p.key {
            let style = Self::key_style(fonts);
            let cap_w = hud.measure(key, &style) + 20.0 * s;
            let cap_h = 16.0 * s * 1.2 + 5.0 * s;
            let cap_x = at + 6.0 * s;
            Slab {
                clip: &KEY_CLIP,
                background: Gradient::solid(hsla(42.0, 96.0, 56.0, 1.0)),
                skew_deg: 0.0,
                shadow: None,
            }
            .draw(hud, cap_x, top + (h - cap_h) / 2.0, cap_w, cap_h);
            hud.text_in_box(cap_x + 10.0 * s, top + (h - cap_h) / 2.0, cap_h, key, &style);
            at = cap_x + cap_w + 6.0 * s;
        }
        at += hud.text_in_box(at, top, h, p.middle, &body);
        let name = Self::text_style(fonts, hsla(182.0, 95.0, 60.0, 1.0));
        hud.text_in_box(at, top, h, p.name, &name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(w: &[&str]) -> Vec<String> {
        w.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn it_asks_only_when_full_and_holding_a_gun() {
        let full = inv(&["blade", "riotgun", "plasmagun"]);
        assert!(asks_to_swap("rlauncher", &full, "riotgun"));
        // Already have it, or room for it: picked up without asking.
        assert!(!asks_to_swap("riotgun", &full, "riotgun"));
        assert!(!asks_to_swap("rlauncher", &inv(&["blade", "riotgun"]), "riotgun"));
        // The blade in hand: the server swaps silently.
        assert!(!asks_to_swap("rlauncher", &full, "blade"));
        assert_eq!(swap_text("F", "rlauncher"), "PRESS [F] TO SWAP TO RLAUNCHER");
    }

    #[test]
    fn the_range_is_a_cylinder_over_the_item() {
        let item = DVec3::new(0.0, 0.0, 0.0);
        assert!(in_pickup_range(DVec3::new(90.0, 40.0, 0.0), item));
        assert!(!in_pickup_range(DVec3::new(70.0, 0.0, 70.0), item));
        assert!(!in_pickup_range(DVec3::new(0.0, 90.0, 0.0), item));
        assert!(!in_pickup_range(DVec3::new(0.0, -30.0, 0.0), item));
    }

    #[test]
    fn the_newest_request_shows_and_the_older_comes_back() {
        let mut p = PickupPrompt::default();
        p.set("pickup", Some("A".into()));
        p.set("dropped", Some("B".into()));
        assert_eq!(p.text(), Some("B"));
        // Asking again with the same line keeps the order.
        p.set("pickup", Some("A".into()));
        assert_eq!(p.text(), Some("B"));
        p.set("dropped", None);
        assert_eq!(p.text(), Some("A"));
        p.set("pickup", None);
        assert_eq!(p.text(), None);
    }

    #[test]
    fn the_line_splits_around_its_key_and_name() {
        let p = parts("PRESS [F] TO SWAP TO RLAUNCHER");
        assert_eq!(
            (p.prefix, p.key, p.middle, p.name),
            ("PRESS ", Some("F"), " TO SWAP TO ", "RLAUNCHER")
        );
    }
}
