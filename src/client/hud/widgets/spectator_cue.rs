//! The freefly spectator's controls card (wash-fps
//! `client/SpectatorOverlay.ts`'s `freeflyCue`, styles
//! `.fps-spectator-cue`): a cyan-edged ink card, bottom left above the
//! HUD, headed SPECTATOR, with the keys as cream chips beside what they
//! do, in two columns. Shown to a spectator whose camera flies free; it
//! gives way to the spectator bar once they chase someone, and a
//! recording has its own controls.
//!
//! The browser reads the keys from the bindings (`getActionKeyLabel`) and
//! offers Join Game; the native watch mode flies on fixed keys, follows
//! with F and has no join, so the card names those. With a controller in
//! hand it names the sticks and the pad's buttons instead
//! ([`pad_bindings`]).

use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, hsla};

/// A key and what it does.
pub type Binding = (&'static str, &'static str);

/// The native watch mode's keys and what they do, in the browser's order.
pub const BINDINGS: [Binding; 7] = [
    ("WASD", "Fly"),
    ("MOUSE", "Look"),
    ("SHIFT", "Fast"),
    ("SPACE", "Fly Up"),
    ("C", "Fly Down"),
    ("F", "Chase Player"),
    ("ESC", "Menu"),
];

/// A controller's keys for the native watch mode, given the labels of the
/// buttons bound to jump, crouch and fire (`refreshControls` with a
/// controller): the sticks fly and look, jump and crouch fly up and down,
/// and fire chases a player.
#[must_use]
pub fn pad_bindings(swap: bool, jump: &str, crouch: &str, fire: &str) -> Vec<(String, &'static str)> {
    let (fly, look) = if swap {
        ("Right stick", "Left stick")
    } else {
        ("Left stick", "Right stick")
    };
    vec![
        (fly.to_owned(), "Fly"),
        (look.to_owned(), "Look"),
        (jump.to_owned(), "Fly Up"),
        (crouch.to_owned(), "Fly Down"),
        (fire.to_owned(), "Chase Player"),
    ]
}

/// `FPSSpectatorCue` (the overlay's `freeflyCue`).
#[derive(Debug, Default)]
pub struct SpectatorCue {
    shown: bool,
    /// A controller's keys, when one is in hand; the keyboard's otherwise.
    pub pad: Option<Vec<(String, &'static str)>>,
}

/// A row: a key and what it does on the left, and maybe one on the right.
pub type Row<K> = ((K, &'static str), Option<(K, &'static str)>);

/// The grid's rows: the first half of the bindings down the left
/// columns, the rest down the right (`refreshControls`).
#[must_use]
pub fn rows<K: Clone>(bindings: &[(K, &'static str)]) -> Vec<Row<K>> {
    let half = bindings.len().div_ceil(2);
    (0..half)
        .map(|i| (bindings[i].clone(), bindings.get(i + half).cloned()))
        .collect()
}

impl SpectatorCue {
    /// `setMode("freefly")`: a spectator, live, behind no one's eyes.
    #[must_use]
    pub fn shown_for(watcher: bool, playback: bool, spectating: &str) -> bool {
        watcher && !playback && spectating.is_empty()
    }

    fn header_style(fonts: &Fonts) -> TextStyle {
        fonts
            .body(12.0, hsla(182.0, 95.0, 60.0, 1.0))
            .spacing(12.0 * UI_SCALE * 0.32)
            .align(Align::Left)
    }

    fn key_style(fonts: &Fonts) -> TextStyle {
        fonts
            .display(12.0, hsla(250.0, 45.0, 8.0, 1.0))
            .spacing(12.0 * UI_SCALE * 0.06)
            .align(Align::Left)
    }

    fn action_style(fonts: &Fonts) -> TextStyle {
        fonts
            .body(15.0, hsla(45.0, 100.0, 95.0, 0.86))
            .spacing(15.0 * UI_SCALE * 0.04)
            .align(Align::Left)
    }

    /// A key chip's width: its padding around the label, at least 28.
    fn chip_w(hud: &Hud2d, fonts: &Fonts, key: &str) -> f32 {
        let s = UI_SCALE;
        (hud.measure(key, &Self::key_style(fonts)) + 18.0 * s).max(28.0 * s)
    }

    /// The keys on the card.
    fn bindings(&self) -> Vec<(String, &'static str)> {
        self.pad
            .clone()
            .unwrap_or_else(|| BINDINGS.iter().map(|(k, a)| ((*k).to_owned(), *a)).collect())
    }

    /// The four columns' widths (key, action, key, action), the row
    /// height, and the card's size.
    fn layout(&self, hud: &Hud2d, fonts: &Fonts) -> ([f32; 4], f32, [f32; 2]) {
        let s = UI_SCALE;
        let mut cols = [0.0f32; 4];
        let rows = rows(&self.bindings());
        for (left, right) in &rows {
            for (i, pair) in [Some(left), right.as_ref()].into_iter().enumerate() {
                let Some((key, action)) = pair else { continue };
                cols[i * 2] = cols[i * 2].max(Self::chip_w(hud, fonts, key));
                cols[i * 2 + 1] = cols[i * 2 + 1].max(hud.measure(action, &Self::action_style(fonts)));
            }
        }
        // The chip (12 + 4 + 5) or the action's line (15 * 1.2).
        let row = (21.0 * s).max(18.0 * s);
        let n = rows.len() as f32;
        let grid_w = cols.iter().sum::<f32>() + 3.0 * 12.0 * s + 18.0 * s;
        let grid_h = n * row + (n - 1.0) * 8.0 * s;
        let w = 6.0 * s + 20.0 * s + grid_w + 30.0 * s;
        let h = 16.0 * s + 12.0 * s + 12.0 * s + grid_h + 18.0 * s;
        (cols, row, [w, h])
    }
}

impl FpsHudWidget for SpectatorCue {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let state = frame.state;
        self.shown = Self::shown_for(state.watcher, state.playback, &state.spectating);
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        if self.shown {
            self.layout(hud, fonts).2
        } else {
            [0.0, 0.0]
        }
    }

    /// Its bottom left corner at (`x`, `y`).
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        if !self.shown {
            return;
        }
        let s = UI_SCALE;
        let (cols, row, [w, h]) = self.layout(hud, fonts);
        let (left, top) = (x, y - h);
        // `clip-path`: the top right corner cut 16 px.
        let cut = 16.0 * s;
        let clip = [[0.0, 0.0], [1.0 - cut / w, 0.0], [1.0, cut / h], [1.0, 1.0], [0.0, 1.0]];
        Slab {
            clip: &clip,
            background: Gradient::new(
                100.0,
                &[
                    (hsla(186.0, 70.0, 16.0, 0.94), None),
                    (hsla(250.0, 44.0, 7.0, 0.94), Some(0.62)),
                ],
            ),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, left, top, w, h);
        // `border-left: 6px solid`.
        hud.rect(left, top, 6.0 * s, h, hsla(182.0, 95.0, 52.0, 1.0));
        let inner = left + 6.0 * s + 20.0 * s;
        hud.text_in_box(inner, top + 16.0 * s, 12.0 * s, "SPECTATOR", &Self::header_style(fonts));
        let grid_top = top + 16.0 * s + 12.0 * s + 12.0 * s;
        let key_style = Self::key_style(fonts);
        let action_style = Self::action_style(fonts);
        for (r, (first, second)) in rows(&self.bindings()).into_iter().enumerate() {
            let row_top = grid_top + r as f32 * (row + 8.0 * s);
            let mut at = inner;
            for (i, pair) in [Some(first), second].into_iter().enumerate() {
                if i == 1 {
                    // The second key column stands off from the first's actions.
                    at = inner + cols[0] + 12.0 * s + cols[1] + 12.0 * s + 18.0 * s;
                }
                let Some((key, action)) = pair else { continue };
                let key = key.as_str();
                // The cream chip, its sides slanted (`clip-path` 8 %).
                let chip_w = Self::chip_w(hud, fonts, key);
                Slab {
                    clip: &[[0.08, 0.0], [1.0, 0.0], [0.92, 1.0], [0.0, 1.0]],
                    background: Gradient::new(
                        180.0,
                        &[
                            (hsla(45.0, 100.0, 97.0, 1.0), None),
                            (hsla(42.0, 58.0, 86.0, 1.0), Some(1.0)),
                        ],
                    ),
                    skew_deg: 0.0,
                    shadow: None,
                }
                .draw(hud, at, row_top, chip_w, row);
                let kw = hud.measure(key, &key_style);
                hud.text_in_box(at + (chip_w - kw) / 2.0, row_top, row, key, &key_style);
                hud.text_in_box(at + cols[i * 2] + 12.0 * s, row_top, row, action, &action_style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_to_a_live_spectator_flying_free() {
        assert!(SpectatorCue::shown_for(true, false, ""));
        assert!(!SpectatorCue::shown_for(true, false, "player_3"));
        assert!(!SpectatorCue::shown_for(true, true, ""));
        assert!(!SpectatorCue::shown_for(false, false, ""));
    }

    #[test]
    fn rows_split_the_keys_into_two_columns() {
        let rows = rows(&BINDINGS);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], (("WASD", "Fly"), Some(("C", "Fly Down"))));
        assert_eq!(rows[3], (("SPACE", "Fly Up"), None));
    }

    #[test]
    fn a_controller_names_the_sticks_and_its_buttons() {
        let pad = pad_bindings(false, "A", "B", "RT");
        assert_eq!(pad[0], ("Left stick".to_owned(), "Fly"));
        assert_eq!(pad[4], ("RT".to_owned(), "Chase Player"));
        assert_eq!(pad_bindings(true, "A", "B", "RT")[1].0, "Left stick");
        assert_eq!(rows(&pad).len(), 3);
    }
}
