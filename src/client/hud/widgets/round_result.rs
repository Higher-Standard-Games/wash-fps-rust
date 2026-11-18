//! The intermission between rounds (wash-fps `client/hud/widgets/RoundResult.ts`,
//! styles `.fps-round-result`): who won the round, the score, and when the
//! next one starts.
//!
//! Covers the whole gap between rounds of a round mode (S&D, Amphi): the
//! round-end beat and the next round's freeze, on a dimmed screen like the
//! end screen's. What ended the round leads, the winner stands on a gold
//! plate, the teams' rounds under it, and a count runs down to the next
//! round going live, when the overlay lifts. A detonation gets 1.5 s of
//! the explosion before the overlay covers it.

use shagger::shared::json::Value;
use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, cream, cream_dim, hsla};
use crate::client::match_state_manager::{MatchPhase, MatchStateManager};

/// How long the explosion gets before the overlay covers it.
const EXPLOSION_GRACE_SECONDS: f64 = 1.5;

const LEAD_CLIP: [[f32; 2]; 4] = [[0.03, 0.0], [1.0, 0.0], [0.97, 1.0], [0.0, 1.0]];
const WINNER_CLIP: [[f32; 2]; 4] = [[0.0, 0.08], [1.0, 0.0], [0.96, 1.0], [0.03, 0.92]];
const TEAM_CLIP: [[f32; 2]; 4] = [[0.02, 0.0], [1.0, 0.08], [0.97, 1.0], [0.0, 0.9]];

/// `leadFor`: what ended the round, over the winner. The server sends a
/// key; the wording lives here.
fn lead_for(reason: &str) -> Option<&'static str> {
    match reason {
        "defused" => Some("Bomb defused"),
        "detonated" => Some("Bomb detonated"),
        "timeout" => Some("Time up"),
        _ => None,
    }
}

/// One side of the score line.
#[derive(Debug, Clone, PartialEq)]
struct Side {
    name: String,
    rounds: i64,
    won: bool,
}

/// What the overlay shows.
#[derive(Debug, Clone, PartialEq)]
struct Card {
    lead: Option<&'static str>,
    reason: String,
    /// The winner's name, or "Round drawn".
    winner: String,
    won: bool,
    udon: Side,
    soba: Side,
    /// Seconds to the next round going live, rounded up, at least 1.
    count: u32,
}

/// `FPSRoundResult`.
#[derive(Debug)]
pub struct RoundResult {
    card: Option<Card>,
    /// The longest round-end beat seen this match (`intermissionLength`):
    /// the client is told only what is left of it.
    intermission: f64,
    /// Seconds since the overlay came up, for its entrance and fade.
    age: f64,
    /// Seconds since the count last changed, for its stamp.
    tick_age: f64,
    /// The overlay fading out after it lifts, from 1 to 0.
    leaving: f32,
    last: Option<Card>,
}

impl Default for RoundResult {
    fn default() -> Self {
        Self {
            card: None,
            intermission: 5.0,
            age: 0.0,
            tick_age: 0.0,
            leaving: 0.0,
            last: None,
        }
    }
}

impl RoundResult {
    /// `currentResult`: the round just decided, while its intermission
    /// runs.
    fn current_result<'a>(&self, m: &'a MatchStateManager) -> Option<&'a Value> {
        let result = m.last_round_result()?;
        let round = result.get("round").and_then(Value::as_f64).unwrap_or(-1.0);
        match m.phase() {
            MatchPhase::RoundEnd => {
                if round != f64::from(m.round()) {
                    return None;
                }
                let reason = result.get("reason").and_then(Value::as_str).unwrap_or_default();
                if reason == "detonated" && self.intermission - m.time_remaining() < EXPLOSION_GRACE_SECONDS {
                    return None;
                }
                Some(result)
            }
            // The next round's freeze: the round before it is still the news.
            MatchPhase::Countdown if m.next_round_in().is_some() && round == f64::from(m.round()) - 1.0 => Some(result),
            _ => None,
        }
    }

    fn card_for(m: &MatchStateManager, result: &Value) -> Card {
        let reason = result.get("reason").and_then(Value::as_str).unwrap_or_default();
        let winner_id = result.get("winner").and_then(Value::as_str).filter(|w| !w.is_empty());
        let side = |team: &str, fallback: &str| Side {
            name: m.team_names().get(team).cloned().unwrap_or_else(|| fallback.to_owned()),
            rounds: m.team_scores().get(team).copied().unwrap_or(0.0) as i64,
            won: winner_id == Some(team),
        };
        // Through the round end and the freeze; an older recording knows
        // only the phase's own time.
        let left = m.next_round_in().unwrap_or_else(|| m.time_remaining());
        Card {
            lead: lead_for(reason),
            reason: reason.to_owned(),
            winner: match winner_id {
                Some(_) => result
                    .get("winnerName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                None => "Round drawn".into(),
            },
            won: winner_id.is_some(),
            udon: side("udon", "Team Udon"),
            soba: side("soba", "Team Soba"),
            count: left.ceil().max(1.0) as u32,
        }
    }

    fn lead_style(fonts: &Fonts, color: [f32; 4]) -> TextStyle {
        fonts
            .display(22.0, color)
            .spacing(22.0 * UI_SCALE * 0.12)
            .align(Align::Left)
    }

    /// `clamp(34px, 4.4vw, 64px)`.
    fn winner_style(fonts: &Fonts, color: [f32; 4], vw: f32) -> TextStyle {
        let px = (vw * 0.044).clamp(34.0, 64.0) / UI_SCALE;
        fonts
            .display(px, color)
            .spacing(px * UI_SCALE * 0.08)
            .align(Align::Left)
    }
}

impl FpsHudWidget for RoundResult {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let m = frame.match_state;
        let card = self.current_result(m).map(|r| Self::card_for(m, r));
        if card.is_none() && m.phase() == MatchPhase::RoundEnd {
            self.intermission = self.intermission.max(m.time_remaining());
        }
        match (&card, &self.card) {
            (Some(new), Some(old)) => {
                self.age += frame.dt;
                if new.count == old.count {
                    self.tick_age += frame.dt;
                } else {
                    self.tick_age = 0.0;
                }
            }
            (Some(_), None) => {
                self.age = 0.0;
                self.tick_age = 0.0;
                self.leaving = 0.0;
            }
            (None, Some(_)) => {
                self.last.clone_from(&self.card);
                self.leaving = 1.0;
            }
            (None, None) => self.leaving = (self.leaving - frame.dt as f32 / 0.45).max(0.0),
        }
        self.card = card;
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [0.0, 0.0]
    }

    /// Over the whole screen, `x`, `y` its middle.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let (card, alpha, age) = match (&self.card, &self.last) {
            (Some(card), _) => (card, (self.age / 0.25).min(1.0) as f32, self.age),
            (None, Some(last)) if self.leaving > 0.0 => (last, self.leaving, 1.0),
            _ => return,
        };
        let s = UI_SCALE;
        let (vw, vh) = (x * 2.0, y * 2.0);
        let fade = |mut c: [f32; 4]| {
            c[3] *= alpha;
            c
        };
        // The dimmed screen.
        hud.rect(0.0, 0.0, vw, vh, fade(hsla(250.0, 45.0, 4.0, 0.4)));
        let reach = vw.hypot(vh) * 0.5 * 0.7;
        hud.radial_gradient(
            x,
            y,
            &[
                (0.0, fade(hsla(250.0, 45.0, 4.0, 0.53))),
                (reach, fade(hsla(250.0, 45.0, 4.0, 0.0))),
            ],
        );

        // `fps-round-result-in`: from 1.12 down, springing, 0.45 s.
        let t = (age / 0.45).min(1.0) as f32;
        let ease = 1.0 - (1.0 - t).powi(3);
        let card_alpha = alpha * ease;
        let ink = |mut c: [f32; 4]| {
            c[3] *= card_alpha;
            c
        };
        let shadow = ink(hsla(250.0, 45.0, 4.0, 0.9));

        // Heights of the card's rows, top to bottom, 14 px apart.
        let lead_h = (5.0 + 28.0 + 7.0) * s;
        let winner_px = (vw * 0.044).clamp(34.0, 64.0);
        let winner_h = 14.0 * s + winner_px * 1.25 + 16.0 * s;
        let won_h = 26.0 * s * 1.25;
        let score_h = 46.0 * s * 1.1 + 10.0 * s;
        let gap = 14.0 * s;
        let mut total = winner_h + score_h + gap;
        if card.lead.is_some() {
            total += lead_h + gap;
        }
        if card.won {
            total += won_h + gap;
        }
        let mut top = y - total / 2.0;

        if let Some(lead) = card.lead {
            let (from, to, stop, text) = match card.reason.as_str() {
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
            let lead = lead.to_uppercase();
            let style = Self::lead_style(fonts, ink(text));
            let w = 22.0 * s + hud.measure(&lead, &style) + 22.0 * s;
            Slab {
                clip: &LEAD_CLIP,
                background: Gradient::new(178.0, &[(ink(from), None), (ink(to), Some(stop))]),
                skew_deg: 0.0,
                shadow: None,
            }
            .draw(hud, x - w / 2.0, top, w, lead_h);
            hud.text_in_box(x - w / 2.0 + 22.0 * s, top + 5.0 * s, 28.0 * s, &lead, &style);
            top += lead_h + gap;
        }

        // The winner on the gold plate.
        let winner = card.winner.to_uppercase();
        let style = Self::winner_style(fonts, ink(hsla(246.0, 42.0, 10.0, 1.0)), vw);
        let text_w = hud.measure(&winner, &style).min(vw * 0.8 - 82.0 * s);
        let w = 34.0 * s + text_w + 48.0 * s;
        Slab {
            clip: &WINNER_CLIP,
            background: Gradient::new(
                100.0,
                &[
                    (ink(hsla(45.0, 100.0, 55.0, 1.0)), None),
                    (ink(hsla(36.0, 96.0, 46.0, 1.0)), None),
                ],
            ),
            skew_deg: -5.0,
            shadow: None,
        }
        .draw(hud, x - w / 2.0, top, w, winner_h);
        hud.text_in_box(
            x - w / 2.0 + 34.0 * s,
            top + 14.0 * s,
            winner_px * 1.25,
            &winner,
            &style,
        );
        top += winner_h + gap;

        if card.won {
            let style = fonts
                .display(26.0, ink(cream()))
                .spacing(26.0 * s * 0.1)
                .align(Align::Center)
                .shadow([2.0 * s, 3.0 * s], shadow);
            hud.text_in_box(x, top, won_h, "WON THE ROUND!", &style);
            top += won_h + gap;
        }

        // The rounds: each side's name on its colour, the numbers big
        // between them.
        top += 10.0 * s;
        let number = |won: bool| {
            fonts
                .display(46.0, ink(if won { hsla(45.0, 100.0, 58.0, 1.0) } else { cream_dim() }))
                .align(Align::Left)
                .shadow([3.0 * s, 4.0 * s], shadow)
        };
        let team_style = fonts
            .display(16.0, ink(cream()))
            .spacing(16.0 * s * 0.12)
            .align(Align::Left);
        let row_h = 46.0 * s * 1.1;
        let dash_style = number(false);
        let dash_w = hud.measure(":", &dash_style);
        let (row_gap, inner_gap) = (18.0 * s, 14.0 * s);
        let plate_h = (5.0 + 19.0 + 7.0) * s;
        let plate = |hud: &mut Hud2d, name: &str, hue: [f32; 4], px: f32| {
            let name = name.to_uppercase();
            let w = 18.0 * s + hud.measure(&name, &team_style).min(vw * 0.28) + 18.0 * s;
            Slab {
                clip: &TEAM_CLIP,
                background: Gradient::new(100.0, &[(ink(hue), None), (ink(hsla(255.0, 30.0, 10.0, 0.9)), None)]),
                skew_deg: -5.0,
                shadow: None,
            }
            .draw(hud, px, top + (row_h - plate_h) / 2.0, w, plate_h);
            hud.text_in_box(
                px + 18.0 * s,
                top + (row_h - plate_h) / 2.0 + 5.0 * s,
                19.0 * s,
                &name,
                &team_style,
            );
            w
        };
        let udon_hue = hsla(320.0, 100.0, 60.0, 1.0);
        let soba_hue = hsla(186.0, 98.0, 58.0, 1.0);
        let measure_plate =
            |hud: &Hud2d, name: &str| 36.0 * s + hud.measure(&name.to_uppercase(), &team_style).min(vw * 0.28);
        let udon_num = card.udon.rounds.to_string();
        let soba_num = card.soba.rounds.to_string();
        let udon_w = measure_plate(hud, &card.udon.name) + inner_gap + hud.measure(&udon_num, &number(card.udon.won));
        let soba_w = measure_plate(hud, &card.soba.name) + inner_gap + hud.measure(&soba_num, &number(card.soba.won));
        // Soba left and Udon right, as the scoreboard puts them.
        let row_w = soba_w + row_gap + dash_w + row_gap + udon_w;
        let mut px = x - row_w / 2.0;
        let w = plate(hud, &card.soba.name, soba_hue, px);
        px += w + inner_gap;
        px += hud.text_in_box(px, top, row_h, &soba_num, &number(card.soba.won)) - px + row_gap;
        px = hud.text_in_box(px, top, row_h, ":", &dash_style) + row_gap;
        px = hud.text_in_box(px, top, row_h, &udon_num, &number(card.udon.won)) + inner_gap;
        plate(hud, &card.udon.name, udon_hue, px);

        // Lower middle: how long until the next round goes live.
        let label = "GET READY FOR THE NEXT ROUND";
        let label_style = fonts
            .body(20.0, fade(cream()))
            .spacing(20.0 * s * 0.14)
            .align(Align::Left)
            .shadow([2.0 * s, 2.0 * s], fade(hsla(250.0, 45.0, 4.0, 0.9)));
        let count = card.count.to_string();
        // `fps-round-result-tick`: each new number stamps in.
        let stamp = 1.0 + 0.35 * (1.0 - (self.tick_age / 0.35).min(1.0) as f32).powi(2);
        let count_style = fonts
            .display(34.0 * stamp, fade(hsla(45.0, 100.0, 58.0, 1.0)))
            .align(Align::Center);
        let label_w = hud.measure(label, &label_style);
        let count_w = (34.0 * s * 1.4).max(hud.measure(&count, &count_style));
        let line_w = label_w + 14.0 * s + count_w;
        let base = vh - vh * 0.22;
        let line_h = 34.0 * s * 1.2;
        let left = x - line_w / 2.0;
        hud.text_in_box(left, base - line_h, line_h, label, &label_style);
        hud.text_in_box(
            left + label_w + 14.0 * s + count_w / 2.0,
            base - line_h,
            line_h,
            &count,
            &count_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_ended_the_round_leads() {
        assert_eq!(lead_for("defused"), Some("Bomb defused"));
        assert_eq!(lead_for("detonated"), Some("Bomb detonated"));
        assert_eq!(lead_for("timeout"), Some("Time up"));
        assert_eq!(lead_for("eliminated"), None);
    }

    fn state(events: &[&str]) -> MatchStateManager {
        let mut m = MatchStateManager::new();
        for e in events {
            m.process_event(&shagger::shared::json::parse(e).unwrap());
        }
        m
    }

    #[test]
    fn the_overlay_covers_the_round_end_and_the_next_freeze() {
        let widget = RoundResult::default();
        let end = state(&[
            r#"{"phase":4,"timeRemaining":4.9,"round":2,"nextRoundIn":9.9,"teamScores":{"udon":1,"soba":1},
                "teamNames":{"udon":"Attackers","soba":"Defenders"},
                "roundResult":{"round":2,"reason":"defused","winner":"soba","winnerName":"Defenders"}}"#,
        ]);
        let result = widget.current_result(&end).unwrap();
        let card = RoundResult::card_for(&end, result);
        assert_eq!(card.lead, Some("Bomb defused"));
        assert_eq!(card.winner, "Defenders");
        assert!(card.soba.won && !card.udon.won);
        assert_eq!(card.count, 10);

        // The next round's freeze: the server cleared the result, the last
        // one is still the news while the count runs.
        let freeze = state(&[
            r#"{"phase":4,"timeRemaining":4.9,"round":2,
                "roundResult":{"round":2,"reason":"timeout","winner":"","winnerName":""}}"#,
            r#"{"phase":1,"timeRemaining":2.2,"round":3,"nextRoundIn":2.2}"#,
        ]);
        let result = widget.current_result(&freeze).unwrap();
        let card = RoundResult::card_for(&freeze, result);
        assert_eq!((card.winner.as_str(), card.won, card.count), ("Round drawn", false, 3));

        // A detonation plays out first.
        let blast = state(&[r#"{"phase":4,"timeRemaining":4.5,"round":1,
                "roundResult":{"round":1,"reason":"detonated","winner":"udon","winnerName":"A"}}"#]);
        assert!(widget.current_result(&blast).is_none());
    }
}
