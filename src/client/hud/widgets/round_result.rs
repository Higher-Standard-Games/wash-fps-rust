//! The round-end banner (wash-fps `client/hud/widgets/RoundResult.ts`,
//! styles `.fps-round-result`): during the round-end intermission, centre
//! screen, why the round ended (BOMB DEFUSED, BOMB DETONATED, TIME UP) over
//! who won it. A detonation gets 1.5 s of the explosion before the banner
//! covers it.

use shagger::shared::json::Value;
use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream, drop_print_sm, hsla};
use crate::client::match_state_manager::MatchPhase;

/// How long the explosion gets before the banner covers it.
const EXPLOSION_GRACE_SECONDS: f64 = 1.5;

const LEAD_CLIP: [[f32; 2]; 4] = [[0.03, 0.0], [1.0, 0.0], [0.97, 1.0], [0.0, 1.0]];
const TEXT_CLIP: [[f32; 2]; 4] = [[0.02, 0.0], [1.0, 0.0], [0.98, 1.0], [0.0, 1.0]];

/// The banner: its lead (if the reason has one), the result line, and the
/// reason.
#[derive(Debug, Clone, PartialEq)]
struct Banner {
    lead: Option<&'static str>,
    result: String,
    reason: String,
}

/// `headline`: the server sends a key; the wording lives here.
fn headline(reason: &str, winner: &str) -> (Option<&'static str>, String) {
    let lead = match reason {
        "defused" => Some("BOMB DEFUSED"),
        "detonated" => Some("BOMB DETONATED"),
        "timeout" => Some("TIME UP"),
        _ => None,
    };
    (lead, format!("{winner} wins the round").to_uppercase())
}

/// `FPSRoundResult`.
#[derive(Debug)]
pub struct RoundResult {
    banner: Option<Banner>,
    /// The longest intermission seen this match (`intermissionLength`):
    /// the client is told only what is left of it.
    intermission: f64,
    /// Seconds since the banner appeared, for its entrance.
    age: f64,
}

impl Default for RoundResult {
    fn default() -> Self {
        Self {
            banner: None,
            intermission: 5.0,
            age: 0.0,
        }
    }
}

impl RoundResult {
    fn banner_for(result: &Value, elapsed: f64) -> Option<Banner> {
        let reason = result.get("reason").and_then(Value::as_str).unwrap_or_default();
        if elapsed < EXPLOSION_GRACE_SECONDS && reason == "detonated" {
            return None;
        }
        let winner = result.get("winnerName").and_then(Value::as_str).unwrap_or_default();
        let (lead, result) = headline(reason, winner);
        Some(Banner {
            lead,
            result,
            reason: reason.to_owned(),
        })
    }

    fn lead_style(fonts: &Fonts, color: [f32; 4]) -> TextStyle {
        fonts
            .display(30.0, color)
            .spacing(30.0 * UI_SCALE * 0.1)
            .align(Align::Left)
    }

    fn text_style(fonts: &Fonts, color: [f32; 4]) -> TextStyle {
        fonts
            .display(20.0, color)
            .spacing(20.0 * UI_SCALE * 0.06)
            .align(Align::Left)
    }
}

impl FpsHudWidget for RoundResult {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let m = frame.match_state;
        let result = m.round_result().filter(|_| m.phase() == MatchPhase::RoundEnd);
        let shown = result.and_then(|r| Self::banner_for(r, self.intermission - m.time_remaining()));
        if shown.is_none() && m.phase() == MatchPhase::RoundEnd {
            self.intermission = self.intermission.max(m.time_remaining());
        }
        if shown.is_some() && self.banner.is_some() {
            self.age += frame.dt;
        } else {
            self.age = 0.0;
        }
        self.banner = shown;
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let Some(banner) = &self.banner else {
            return [0.0, 0.0];
        };
        let s = UI_SCALE;
        let text_w = 28.0 * s + hud.measure(&banner.result, &Self::text_style(fonts, cream())) + 28.0 * s;
        let text_h = (8.0 + 24.0 + 10.0) * s;
        match banner.lead {
            Some(lead) => {
                let lead_w = 22.0 * s + hud.measure(lead, &Self::lead_style(fonts, [0.0; 4])) + 22.0 * s;
                [lead_w.max(text_w), (5.0 + 36.0 + 7.0) * s + 6.0 * s + text_h]
            }
            None => [text_w, text_h],
        }
    }

    /// Centred on (`x`, `y`).
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let Some(banner) = &self.banner else { return };
        let s = UI_SCALE;
        // `fps-round-result-in`: 220 ms, from 10 px up and 0.94.
        let t = (self.age / 0.22).min(1.0) as f32;
        let ease = 1.0 - (1.0 - t) * (1.0 - t);
        let lift = -10.0 * s * (1.0 - ease);
        let fade = |mut c: [f32; 4]| {
            c[3] *= ease;
            c
        };
        let [_, h] = self.size(hud, fonts);
        let mut top = y - h / 2.0 + lift;
        if let Some(lead) = banner.lead {
            let (from, to, stop, ink) = match banner.reason.as_str() {
                "detonated" => (
                    hsla(2.0, 92.0, 62.0, 1.0),
                    hsla(356.0, 84.0, 48.0, 1.0),
                    0.72,
                    hsla(48.0, 100.0, 95.0, 1.0),
                ),
                "defused" => (
                    hsla(186.0, 82.0, 62.0, 1.0),
                    hsla(196.0, 76.0, 44.0, 1.0),
                    0.72,
                    hsla(200.0, 100.0, 96.0, 1.0),
                ),
                _ => (
                    hsla(210.0, 14.0, 90.0, 1.0),
                    hsla(210.0, 12.0, 76.0, 1.0),
                    0.70,
                    hsla(246.0, 40.0, 8.0, 1.0),
                ),
            };
            let style = Self::lead_style(fonts, fade(ink));
            let w = 22.0 * s + hud.measure(lead, &style) + 22.0 * s;
            let lead_h = (5.0 + 36.0 + 7.0) * s;
            Slab {
                clip: &LEAD_CLIP,
                background: Gradient::new(178.0, &[(fade(from), None), (fade(to), Some(stop))]),
                skew_deg: 0.0,
                shadow: drop_print_sm().map(|(o, c)| (o, fade(c))),
            }
            .draw(hud, x - w / 2.0, top, w, lead_h);
            hud.text_in_box(x - w / 2.0 + 22.0 * s, top + 5.0 * s, 36.0 * s, lead, &style);
            top += lead_h + 6.0 * s;
        }
        let mut ink = cream();
        ink[3] *= ease;
        let style = Self::text_style(fonts, ink);
        let w = 28.0 * s + hud.measure(&banner.result, &style) + 28.0 * s;
        let text_h = (8.0 + 24.0 + 10.0) * s;
        Slab {
            clip: &TEXT_CLIP,
            background: Gradient::solid(fade(hsla(250.0, 44.0, 7.0, 0.92))),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, x - w / 2.0, top, w, text_h);
        hud.text_in_box(x - w / 2.0 + 28.0 * s, top + 8.0 * s, 24.0 * s, &banner.result, &style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reason_leads_the_winner() {
        let result =
            shagger::shared::json::parse(r#"{"round":2,"reason":"defused","winnerName":"Defenders"}"#).unwrap();
        let banner = RoundResult::banner_for(&result, 0.0).unwrap();
        assert_eq!(banner.lead, Some("BOMB DEFUSED"));
        assert_eq!(banner.result, "DEFENDERS WINS THE ROUND");
        let blast =
            shagger::shared::json::parse(r#"{"round":2,"reason":"detonated","winnerName":"Attackers"}"#).unwrap();
        assert_eq!(RoundResult::banner_for(&blast, 1.0), None);
        assert_eq!(
            RoundResult::banner_for(&blast, 1.6).unwrap().lead,
            Some("BOMB DETONATED")
        );
        let other = shagger::shared::json::parse(r#"{"reason":"eliminated","winnerName":"A"}"#).unwrap();
        assert_eq!(RoundResult::banner_for(&other, 0.0).unwrap().lead, None);
    }
}
