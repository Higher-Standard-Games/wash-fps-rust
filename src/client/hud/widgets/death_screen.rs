//! "YOU GOT FRAGGED" (wash-fps `client/hud/widgets/DeathScreen.ts`,
//! styles `.fps-death-*`): a red veil, the stamp, and a card saying who
//! fragged you and how, then the way back: the respawn protection counting
//! down on a bar, then the key that respawns you, or in a round mode, that
//! you are out until the next round.

use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream, drop_print_md, hsla, ink_text_md};
use crate::client::hud_state::KillFeedEntry;

const RESPAWN_DELAY_SECS: f64 = 3.0;
/// How close to the panel's own moment a kill feed entry must be to be this
/// death, seconds.
const DEATH_MATCH_WINDOW: f64 = 1.5;
/// `getActionKeyLabel(FPS.FIRE)` with the default binds.
const FIRE_KEY: &str = "MOUSE1";

/// `KILLED_BY`.
fn killed_by(damage_type: &str) -> &'static str {
    match damage_type {
        "explosion" => "Blown up by",
        "rail" => "Railed by",
        "plasma" => "Melted by",
        "bullet" => "Gunned down by",
        "melee" => "Cut down by",
        "telefrag" => "Telefragged by",
        "fall_crush" => "Crushed by",
        _ => "Fragged by",
    }
}

/// `noKiller`.
fn no_killer(damage_type: &str) -> &'static str {
    match damage_type {
        "fall" | "fall_crush" => "The fall",
        "world" => "The map",
        "explosion" => "Your own blast",
        "bomb" => "The bomb",
        _ => "Yourself",
    }
}

/// `obituary`: the kicker and the name the card shows.
#[must_use]
pub fn obituary(entry: &KillFeedEntry) -> (&'static str, String) {
    let damage_type = entry.weapon_id.to_lowercase();
    if entry.killer_name.is_empty() || entry.killer_id == entry.victim_id {
        return ("No one to blame", no_killer(&damage_type).to_owned());
    }
    (killed_by(&damage_type), entry.killer_name.clone())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Next {
    #[default]
    Countdown,
    Ready,
    Round,
}

/// `FPSDeathScreen`.
#[derive(Debug, Default)]
pub struct DeathScreen {
    visible: bool,
    shown_for_this_death: bool,
    named: Option<(&'static str, String)>,
    death_time: f64,
    now: f64,
    next: Next,
}

impl DeathScreen {
    fn name_killer(&mut self, feed: &[KillFeedEntry]) {
        for entry in feed.iter().rev() {
            if !entry.of_local {
                continue;
            }
            if entry.timestamp < self.death_time - DEATH_MATCH_WINDOW {
                return;
            }
            self.named = Some(obituary(entry));
            return;
        }
    }
}

impl FpsHudWidget for DeathScreen {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let state = frame.state;
        self.now = state.now;
        if !state.local_dead {
            self.visible = false;
            self.shown_for_this_death = false;
            return;
        }
        if !state.spectating.is_empty() {
            self.visible = false;
            return;
        }
        if self.shown_for_this_death {
            if self.named.is_none() {
                self.name_killer(&state.kill_feed);
            }
        } else {
            self.shown_for_this_death = true;
            self.visible = true;
            self.death_time = state.now;
            self.named = None;
            self.name_killer(&state.kill_feed);
        }
        self.next = if frame.match_state.round() > 0 {
            Next::Round
        } else if state.now - self.death_time < RESPAWN_DELAY_SECS {
            Next::Countdown
        } else {
            Next::Ready
        };
    }

    /// The whole screen; drawn with its own layout.
    fn size(&self, hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        if self.visible { hud.size() } else { [0.0, 0.0] }
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, _x: f32, _y: f32) {
        if !self.visible {
            return;
        }
        let s = UI_SCALE;
        let [w, h] = hud.size();
        // The veil: dark over all, the red closing in from the edges.
        hud.rect(0.0, 0.0, w, h, hsla(250.0, 45.0, 4.0, 0.3));
        let reach = (w * w + h * h).sqrt() / 2.0;
        hud.radial_gradient(
            w / 2.0,
            h / 2.0,
            &[
                (reach * 0.36, hsla(352.0, 80.0, 14.0, 0.0)),
                (reach * 0.76, hsla(352.0, 80.0, 14.0, 0.5)),
                (reach * 1.02, hsla(350.0, 85.0, 8.0, 0.8)),
            ],
        );
        // The stamp.
        let title_style = ink_text_md(fonts.display(60.0, cream()).spacing(60.0 * s * 0.14));
        let title = "YOU GOT FRAGGED";
        let title_w = hud.measure(title, &title_style) + 128.0 * s;
        let (ascent, descent) = hud.line_metrics(&title_style);
        let title_h = ascent + descent + 32.0 * s;
        // The card under it.
        let (card_w, card_h) = self.card_size(hud, fonts);
        let gap = 26.0 * s;
        let top = (h - (title_h + gap + card_h)) / 2.0;
        let title_x = (w - title_w) / 2.0;
        Slab {
            clip: &[[0.03, 0.06], [0.97, 0.0], [1.0, 0.88], [0.52, 1.0], [0.0, 0.9]],
            background: Gradient::new(
                105.0,
                &[
                    (hsla(7.0, 92.0, 56.0, 0.98), None),
                    (hsla(350.0, 82.0, 40.0, 0.96), None),
                ],
            ),
            skew_deg: -5.0,
            shadow: drop_print_md(),
        }
        .draw(hud, title_x, top, title_w, title_h);
        hud.text_in_box(
            title_x + title_w / 2.0,
            top + 14.0 * s,
            title_h - 32.0 * s,
            title,
            &title_style.align(Align::Center),
        );
        self.draw_card(hud, fonts, (w - card_w) / 2.0, top + title_h + gap, card_w, card_h);
    }
}

impl DeathScreen {
    fn how_style(fonts: &Fonts) -> TextStyle {
        fonts
            .body(13.0, hsla(356.0, 95.0, 70.0, 1.0))
            .spacing(13.0 * UI_SCALE * 0.28)
    }

    fn killer_style(fonts: &Fonts) -> TextStyle {
        let s = UI_SCALE;
        fonts
            .display(34.0, cream())
            .spacing(34.0 * s * 0.02)
            .shadow([3.0 * s, 4.0 * s], hsla(250.0, 50.0, 3.0, 1.0))
    }

    fn label_style(&self, fonts: &Fonts) -> TextStyle {
        let color = if self.next == Next::Ready {
            hsla(96.0, 85.0, 55.0, 1.0)
        } else {
            hsla(45.0, 100.0, 95.0, 0.55)
        };
        fonts.body(12.0, color).spacing(12.0 * UI_SCALE * 0.26)
    }

    fn timer_style(fonts: &Fonts) -> TextStyle {
        let s = UI_SCALE;
        fonts
            .display(50.0, cream())
            .shadow([3.0 * s, 4.0 * s], hsla(250.0, 50.0, 3.0, 1.0))
    }

    fn respawn_style(fonts: &Fonts) -> TextStyle {
        fonts.display(16.0, cream()).spacing(16.0 * UI_SCALE * 0.1)
    }

    /// The label, then the timer or the respawn line.
    fn next_lines(&self) -> (&'static str, String, bool) {
        match self.next {
            Next::Countdown => (
                "RESPAWN IN",
                format!("{:.1}", (RESPAWN_DELAY_SECS - (self.now - self.death_time)).max(0.0)),
                true,
            ),
            Next::Ready => ("READY", format!("PRESS {FIRE_KEY} TO RESPAWN"), false),
            Next::Round => ("OUT THIS ROUND", "WAITING FOR THE NEXT ROUND".into(), false),
        }
    }

    fn next_size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let s = UI_SCALE;
        let (label, line, timer) = self.next_lines();
        let label_w = hud.measure(label, &self.label_style(fonts));
        let (line_w, line_h) = if timer {
            (hud.measure(line.as_str(), &Self::timer_style(fonts)), 50.0 * s * 0.9)
        } else {
            (hud.measure(line.as_str(), &Self::respawn_style(fonts)), 16.0 * s * 1.3)
        };
        [label_w.max(line_w), 12.0 * s + 8.0 * s + line_h]
    }

    fn card_size(&self, hud: &Hud2d, fonts: &Fonts) -> (f32, f32) {
        let s = UI_SCALE;
        let [next_w, next_h] = self.next_size(hud, fonts);
        let pad_h = 18.0 * s + 24.0 * s;
        if self.named.is_some() {
            let who_h = 13.0 * s + 8.0 * s + 34.0 * s * 1.05;
            ((620.0 * s).min(hud.size()[0] * 0.94), who_h.max(next_h) + pad_h)
        } else {
            ((next_w + 60.0 * s + 8.0 * s).max(360.0 * s), next_h + pad_h)
        }
    }

    fn draw_card(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32, w: f32, h: f32) {
        let s = UI_SCALE;
        let cut = 20.0 * s / w;
        let clip = [
            [0.0, 0.0],
            [1.0 - cut, 0.0],
            [1.0, 20.0 * s / h],
            [1.0, 1.0],
            [0.0, 1.0],
        ];
        Slab {
            clip: &clip,
            background: Gradient::new(
                100.0,
                &[
                    (hsla(352.0, 70.0, 17.0, 0.96), Some(0.0)),
                    (hsla(250.0, 44.0, 7.0, 0.95), Some(0.58)),
                ],
            ),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, x, y, w, h);
        hud.rect(x, y, 8.0 * s, h, hsla(356.0, 88.0, 56.0, 1.0));
        let content_top = y + 18.0 * s;
        let content_h = h - 42.0 * s;
        let [next_w, next_h] = self.next_size(hud, fonts);
        // The way back, right-aligned beside a name, centred without one.
        let (next_x, align) = if self.named.is_some() {
            (x + w - 34.0 * s, Align::Right)
        } else {
            (x + 8.0 * s + (w - 8.0 * s) / 2.0, Align::Center)
        };
        let _ = next_w;
        if let Some((how, who)) = &self.named {
            let left = x + 8.0 * s + 26.0 * s;
            let how_style = Self::how_style(fonts);
            let killer = Self::killer_style(fonts);
            let block = 13.0 * s + 8.0 * s + 34.0 * s * 1.05;
            let top = content_top + (content_h - block) / 2.0;
            let (a, _) = hud.line_metrics(&how_style);
            hud.text(left, top + a, &how.to_uppercase(), &how_style);
            let (ka, kd) = hud.line_metrics(&killer);
            let line = 34.0 * s * 1.05;
            let baseline = top + 21.0 * s + (line - (ka + kd)) / 2.0 + ka;
            hud.text(left, baseline, who, &killer);
        }
        let (label, line, timer) = self.next_lines();
        let top = content_top + (content_h - next_h) / 2.0;
        let label_style = self.label_style(fonts).align(align);
        let (a, _) = hud.line_metrics(&label_style);
        hud.text(next_x, top + a, label, &label_style);
        let line_top = top + 12.0 * s + 8.0 * s;
        if timer {
            let style = Self::timer_style(fonts).align(align);
            hud.text_in_box(next_x, line_top, 50.0 * s * 0.9, &line, &style);
        } else {
            let mut style = Self::respawn_style(fonts).align(align);
            if self.next == Next::Ready && (self.now % 1.1) > 0.275 && (self.now % 1.1) < 0.825 {
                // `fps-death-call`: dimmer halfway through.
                style.color[3] = 0.6;
            }
            hud.text_in_box(next_x, line_top, 16.0 * s * 1.3, &line, &style);
        }
        // The bar along the bottom, filling over the protection.
        if self.next != Next::Round {
            let bar_h = 6.0 * s;
            let by = y + h - bar_h;
            hud.rect(x, by, w, bar_h, hsla(0.0, 0.0, 100.0, 0.07));
            let fill = ((self.now - self.death_time) / RESPAWN_DELAY_SECS).clamp(0.0, 1.0) as f32;
            let (from, to) = if self.next == Next::Ready {
                (hsla(96.0, 70.0, 38.0, 1.0), hsla(96.0, 85.0, 55.0, 1.0))
            } else {
                (hsla(352.0, 80.0, 42.0, 1.0), hsla(356.0, 95.0, 64.0, 1.0))
            };
            Slab {
                clip: &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                background: Gradient::new(90.0, &[(from, None), (to, None)]),
                skew_deg: 0.0,
                shadow: None,
            }
            .draw(hud, x, by, w * fill, bar_h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_the_obituary_as_the_card_does() {
        let entry = KillFeedEntry {
            victim_id: "a".into(),
            killer_id: "b".into(),
            killer_name: "Ramen".into(),
            weapon_id: "bullet".into(),
            ..KillFeedEntry::default()
        };
        assert_eq!(obituary(&entry), ("Gunned down by", "Ramen".to_owned()));
        let fall = KillFeedEntry {
            weapon_id: "fall".into(),
            ..KillFeedEntry::default()
        };
        assert_eq!(obituary(&fall), ("No one to blame", "The fall".to_owned()));
    }
}
