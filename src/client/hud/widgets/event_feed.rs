//! The kill feed at the top left (SHAG `src/engine/ui/hud/EventFeed.ts`,
//! fed by wash-fps `client/GameEvents.ts`'s `handleDeathEvent`, styled by
//! `.fps-event-*`): "killer [weapon] fragged victim", six at most, each for
//! six seconds then a one-second fade. The styles are Washoi's, so the
//! widget lives here rather than in shagger.

use shagger::ui::hud_2d::Hud2d;

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream_dim, hsla, void};

const MAX_MESSAGES: usize = 6;
const MESSAGE_LIFETIME: f64 = 6.0;
const MESSAGE_FADE_TIME: f64 = 1.0;

#[derive(Debug, Clone)]
struct Kill {
    killer: String,
    weapon: String,
    victim: String,
    age: f64,
}

/// `EventFeed`.
#[derive(Debug, Default)]
pub struct EventFeed {
    seen_seq: Option<u64>,
    messages: Vec<Kill>,
}

impl EventFeed {
    /// `addKill`.
    pub fn add_kill(&mut self, killer: &str, victim: &str, weapon: &str) {
        self.messages.push(Kill {
            killer: killer.to_owned(),
            weapon: weapon.to_uppercase(),
            victim: victim.to_owned(),
            age: 0.0,
        });
        while self.messages.len() > MAX_MESSAGES {
            self.messages.remove(0);
        }
    }
}

impl FpsHudWidget for EventFeed {
    fn update(&mut self, frame: &HudFrame<'_>) {
        for m in &mut self.messages {
            m.age += frame.dt;
        }
        self.messages.retain(|m| m.age < MESSAGE_LIFETIME + MESSAGE_FADE_TIME);
        let feed = &frame.state.kill_feed;
        let newest = feed.last().map_or(0, |k| k.seq);
        let seen = self.seen_seq.unwrap_or(0);
        for kill in feed.iter().filter(|k| k.seq > seen) {
            // A suicide or a fall is not a frag.
            if !kill.killer_name.is_empty() && kill.killer_id != kill.victim_id {
                self.add_kill(&kill.killer_name, &kill.victim_name, &kill.weapon_id);
            }
        }
        self.seen_seq = Some(newest.max(seen));
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [0.0, 0.0]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let mut top = y;
        for m in &self.messages {
            let alpha = if m.age < 0.08 {
                0.0
            } else if m.age > MESSAGE_LIFETIME {
                (1.0 - (m.age - MESSAGE_LIFETIME) / MESSAGE_FADE_TIME) as f32
            } else {
                1.0
            };
            let fade = |mut c: [f32; 4]| {
                c[3] *= alpha;
                c
            };
            let spacing = 15.0 * s * 0.04;
            let killer = fonts.body(15.0, fade(hsla(185.0, 98.0, 68.0, 1.0))).spacing(spacing);
            let weapon = fonts
                .body(15.0 * 0.85, fade(hsla(42.0, 95.0, 88.0, 0.55)))
                .spacing(15.0 * 0.85 * s * 0.08);
            let action = fonts.body(15.0, fade(cream_dim())).spacing(spacing);
            let victim = fonts.body(15.0, fade(hsla(7.0, 92.0, 64.0, 1.0))).spacing(spacing);
            let space = hud.measure(" ", &action);
            let weapon_text = if m.weapon.is_empty() {
                String::new()
            } else {
                format!("[{}]", m.weapon)
            };
            let parts = [
                (m.killer.as_str(), killer),
                (weapon_text.as_str(), weapon),
                ("fragged", action),
                (m.victim.as_str(), victim),
            ];
            let text_w: f32 = parts
                .iter()
                .filter(|(t, _)| !t.is_empty())
                .map(|(t, st)| hud.measure(t, st) + space)
                .sum::<f32>()
                - space;
            let (w, h) = ((text_w + 32.0 * s).min(430.0 * s), 15.0 * s * 1.2 + 10.0 * s);
            Slab {
                clip: &[[0.02, 0.0], [1.0, 0.0], [0.97, 1.0], [0.0, 1.0]],
                background: Gradient::new(
                    100.0,
                    &[
                        (fade(hsla(250.0, 30.0, 12.0, 0.9)), None),
                        (fade(void()), Some(0.7)),
                        (fade(void()), Some(1.0)),
                    ],
                ),
                skew_deg: -5.0,
                shadow: Some(([3.0, 4.0], fade(hsla(250.0, 45.0, 4.0, 0.55)))),
            }
            .draw(hud, x, top, w, h);
            let (ascent, descent) = hud.line_metrics(&action);
            let baseline = top + 4.0 * s + (h - 10.0 * s + ascent - descent) / 2.0;
            let mut pen = x + 12.0 * s;
            for (text, style) in parts {
                if text.is_empty() {
                    continue;
                }
                pen += hud.text(pen, baseline, text, &style) + space;
            }
            top += h + 5.0 * s;
        }
    }
}
