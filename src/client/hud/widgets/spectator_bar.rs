//! "Spectating: name" while the camera is behind someone else's eyes
//! (wash-fps `client/SpectatorOverlay.ts`'s first-person bar, styles
//! `.fps-spectator-bar`): a cyan-edged ink plate 210 px up from the
//! bottom, centred, the name between the keys that step to the previous
//! and the next player. Shown for a dead player watching a teammate and
//! for a spectator chasing someone; hidden in a recording, which has its
//! own controls.

use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream, hsla};

/// `.fps-spectator-bar-name`'s `max-width`.
const NAME_MAX: f32 = 320.0;

/// `FPSSpectatorBar` (the overlay's `spectatingBar`).
#[derive(Debug, Default)]
pub struct SpectatorBar {
    name: Option<String>,
}

impl SpectatorBar {
    /// The name shown, or none when the bar is hidden.
    fn shown(playback: bool, spectating: &str, name: &str) -> Option<String> {
        (!playback && !spectating.is_empty()).then(|| name.to_owned())
    }

    fn label_style(fonts: &Fonts) -> TextStyle {
        fonts
            .body(11.0, hsla(182.0, 95.0, 60.0, 1.0))
            .spacing(11.0 * UI_SCALE * 0.32)
            .align(Align::Left)
    }

    fn name_style(fonts: &Fonts) -> TextStyle {
        fonts
            .display(22.0, cream())
            .spacing(22.0 * UI_SCALE * 0.03)
            .align(Align::Left)
    }

    /// `text-overflow: ellipsis` at `max-width`.
    fn fitted(hud: &Hud2d, name: &str, style: &TextStyle) -> String {
        let max = NAME_MAX * UI_SCALE;
        if hud.measure(name, style) <= max {
            return name.to_owned();
        }
        let mut chars: Vec<char> = name.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let cut: String = chars.iter().collect::<String>() + "…";
            if hud.measure(&cut, style) <= max {
                return cut;
            }
        }
        "…".to_owned()
    }

    /// The middle column's width, and the plate's size.
    fn layout(hud: &Hud2d, fonts: &Fonts, name: &str) -> (f32, [f32; 2]) {
        let s = UI_SCALE;
        let label = hud.measure("SPECTATING", &Self::label_style(fonts));
        let name = hud.measure(
            &Self::fitted(hud, name, &Self::name_style(fonts)),
            &Self::name_style(fonts),
        );
        let who = label.max(name).max(180.0 * s);
        let content_h = (30.0 * s).max(11.0 * s + 6.0 * s + 22.0 * s * 1.05);
        let w = 6.0 * s + 18.0 * s + 34.0 * s + 22.0 * s + who + 22.0 * s + 34.0 * s + 18.0 * s;
        (who, [w, 10.0 * s + content_h + 10.0 * s])
    }
}

impl FpsHudWidget for SpectatorBar {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let state = frame.state;
        self.name = Self::shown(state.playback, &state.spectating, &state.spectating_name);
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        self.name
            .as_ref()
            .map_or([0.0, 0.0], |name| Self::layout(hud, fonts, name).1)
    }

    /// Centred on `x`, its bottom at `y`.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let Some(name) = &self.name else { return };
        let s = UI_SCALE;
        let (who, [w, h]) = Self::layout(hud, fonts, name);
        let (left, top) = (x - w / 2.0, y - h);
        // `clip-path`: the top right corner cut 14 px.
        let cut = 14.0 * s;
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
        let key_y = top + (h - 30.0 * s) / 2.0;
        let mut at = left + 6.0 * s + 18.0 * s;
        for i in 0..2 {
            Slab {
                clip: &[[0.1, 0.0], [1.0, 0.0], [0.9, 1.0], [0.0, 1.0]],
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
            .draw(hud, at, key_y, 34.0 * s, 30.0 * s);
            // The fonts have no arrows (the browser falls back to a system
            // face): drawn instead, a stem and a head.
            let ink = hsla(250.0, 45.0, 8.0, 1.0);
            let (cx, cy) = (at + 17.0 * s, key_y + 15.0 * s);
            let dir = if i == 0 { -1.0 } else { 1.0 };
            hud.rect(cx - 5.0 * s, cy - 1.25 * s, 10.0 * s, 2.5 * s, ink);
            hud.polygon(
                &[
                    [cx + dir * 8.0 * s, cy],
                    [cx + dir * 2.0 * s, cy - 5.5 * s],
                    [cx + dir * 2.0 * s, cy + 5.5 * s],
                ],
                &[ink; 3],
            );
            if i == 0 {
                at += 34.0 * s + 22.0 * s;
                // `.fps-spectator-bar-who`: a centred column.
                let column_h = 11.0 * s + 6.0 * s + 22.0 * s * 1.05;
                let column_top = top + (h - column_h) / 2.0;
                let label = Self::label_style(fonts);
                let lw = hud.measure("SPECTATING", &label);
                hud.text_in_box(at + (who - lw) / 2.0, column_top, 11.0 * s, "SPECTATING", &label);
                let style = Self::name_style(fonts);
                let shown = Self::fitted(hud, name, &style);
                let nw = hud.measure(&shown, &style);
                hud.text_in_box(
                    at + (who - nw) / 2.0,
                    column_top + 17.0 * s,
                    22.0 * s * 1.05,
                    &shown,
                    &style,
                );
                at += who + 22.0 * s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_behind_someone_elses_eyes_outside_a_recording() {
        assert_eq!(
            SpectatorBar::shown(false, "player_3", "Noodlesoup"),
            Some("Noodlesoup".into())
        );
        assert_eq!(SpectatorBar::shown(false, "", "Noodlesoup"), None);
        assert_eq!(SpectatorBar::shown(true, "player_3", "Noodlesoup"), None);
    }
}
