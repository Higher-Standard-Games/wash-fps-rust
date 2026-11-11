//! The Search & Destroy objective (wash-fps
//! `client/hud/widgets/BombStatus.ts`, styles `.fps-bomb-status` and
//! `.fps-bomb-plant-meter`): top left, which side we are on (ATTACK or
//! DEFEND) over what the bomb means to that side; the arm or defuse meter
//! under it and again below the crosshair, only for whoever has their hands
//! on the bomb (or is watching them). The fuse drives the urgency but is
//! never printed: the beep is the clock.

use shagger::shared::json::Value;
use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, drop_print_sm, hsla};

/// Below this the fuse display goes urgent.
const URGENT_SECONDS: f64 = 10.0;

/// What a defender is told before the bomb is armed.
const DEFEND: &str = "DEFEND THE SITES";
/// What it says when the bomb is on us.
const CARRIER_TEXT: &str = "YOU HAVE THE BOMB";

const PHASE_CLIP: [[f32; 2]; 4] = [[0.03, 0.0], [1.0, 0.0], [0.97, 1.0], [0.0, 1.0]];
const TRACK_CLIP: [[f32; 2]; 4] = [[0.02, 0.0], [1.0, 0.0], [0.98, 1.0], [0.0, 1.0]];

/// `PHASE_TEXT`: what a phase means to each side (attacker, defender).
fn phase_text(phase: &str) -> Option<(&'static str, &'static str)> {
    Some(match phase {
        "dropped" => ("TAKE THE BOMB", DEFEND),
        "carried" => ("PLANT THE BOMB", DEFEND),
        "planted" => ("ARMING", DEFEND),
        "armed" => ("BOMB ARMED", "DEFUSE"),
        "defused" => ("DEFUSED", "DEFUSED"),
        "detonated" => ("DETONATED", "DETONATED"),
        _ => return None,
    })
}

/// What the widget shows.
#[derive(Debug, Clone, PartialEq)]
struct Status {
    role: &'static str,
    attacking: bool,
    text: &'static str,
    carrying: bool,
    armed: bool,
    urgent: bool,
    /// The meter's share, when it is ours to see; and whether it is rising.
    meter: Option<(f32, bool)>,
}

/// The status for a bomb objective, seen as `local` (our id), `spectating`
/// (whom we watch) and `attacking`.
fn status(bomb: &Value, local: &str, spectating: &str, attacking: bool) -> Option<Status> {
    let phase = bomb.get("phase").and_then(Value::as_str)?;
    let (attack, defend) = phase_text(phase)?;
    let id = |k: &str| bomb.get(k).and_then(Value::as_str);
    let fuse = bomb.get("fuse").and_then(Value::as_f64).map(|f| f.max(0.0));
    let progress = (bomb.get("progress").and_then(Value::as_f64).unwrap_or(0.0) * 100.0).round();
    let carrying = id("carrier").is_some_and(|c| c == local);
    let mine = id("worker").is_some_and(|w| w == local || (!spectating.is_empty() && w == spectating));
    let advancing = bomb.get("advancing").and_then(Value::as_bool).unwrap_or(false);
    Some(Status {
        role: if attacking { "ATTACK" } else { "DEFEND" },
        attacking,
        text: if carrying {
            CARRIER_TEXT
        } else if attacking {
            attack
        } else {
            defend
        },
        carrying,
        armed: phase == "armed",
        urgent: fuse.is_some_and(|f| f <= URGENT_SECONDS),
        meter: (progress > 0.0 && mine).then_some(((progress / 100.0) as f32, advancing)),
    })
}

/// A triangle wave eased in and out, 0 to 1 and back each `period`.
fn pulse(now: f64, period: f64) -> f32 {
    let t = (now / period).fract();
    let half = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
    (half * half * (3.0 - 2.0 * half)) as f32
}

/// `FPSBombStatus`.
#[derive(Debug, Default)]
pub struct BombStatus {
    status: Option<Status>,
    now: f64,
}

impl BombStatus {
    fn role_style(fonts: &Fonts, attacking: bool) -> TextStyle {
        let color = if attacking {
            hsla(38.0, 92.0, 62.0, 1.0)
        } else {
            hsla(186.0, 70.0, 62.0, 1.0)
        };
        fonts
            .display(11.0, color)
            .spacing(11.0 * UI_SCALE * 0.28)
            .shadow([1.0, 1.0], hsla(250.0, 45.0, 4.0, 0.8))
            .align(Align::Left)
    }

    fn phase_style(fonts: &Fonts, k: f32, color: [f32; 4]) -> TextStyle {
        fonts
            .display(14.0 * k, color)
            .spacing(14.0 * UI_SCALE * k * 0.1)
            .align(Align::Left)
    }

    /// The centre meter, below the crosshair (`.fps-bomb-plant-meter`, 58 %
    /// down).
    pub fn draw_centre(&self, hud: &mut Hud2d, fonts: &Fonts, vw: f32, vh: f32) {
        let Some((share, advancing)) = self.status.as_ref().and_then(|s| s.meter) else {
            return;
        };
        let s = UI_SCALE;
        let defusing = self.status.as_ref().is_some_and(|s| s.armed);
        let (label, label_color, fill) = if defusing {
            ("DEFUSING", hsla(186.0, 90.0, 68.0, 1.0), hsla(186.0, 85.0, 55.0, 1.0))
        } else {
            ("ARMING", hsla(48.0, 100.0, 66.0, 1.0), hsla(48.0, 100.0, 58.0, 1.0))
        };
        let fill = if advancing { fill } else { hsla(210.0, 12.0, 60.0, 1.0) };
        let style = fonts
            .display(24.0, label_color)
            .spacing(24.0 * s * 0.22)
            .shadow([2.0, 2.0], hsla(0.0, 0.0, 0.0, 0.62))
            .align(Align::Center);
        let top = vh * 0.58;
        hud.text_in_box(vw / 2.0, top, 28.0 * s, label, &style);
        let (w, h) = (380.0 * s, 20.0 * s);
        let (x, y) = (vw / 2.0 - w / 2.0, top + 28.0 * s + 9.0 * s);
        Slab {
            clip: &TRACK_CLIP,
            background: Gradient::solid(hsla(250.0, 45.0, 4.0, 0.72)),
            skew_deg: 0.0,
            shadow: Some(([3.0, 3.0], hsla(0.0, 0.0, 0.0, 0.4))),
        }
        .draw(hud, x, y, w, h);
        hud.outline(x, y, w, h, 2.0 * s, hsla(210.0, 14.0, 88.0, 0.4));
        hud.rect(x + 2.0 * s, y + 2.0 * s, (w - 4.0 * s) * share, h - 4.0 * s, fill);
    }
}

impl FpsHudWidget for BombStatus {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.now = frame.state.now;
        let state = frame.state;
        let m = frame.match_state;
        self.status = m.bomb().and_then(|bomb| {
            let team = state
                .scoreboard
                .iter()
                .find(|e| e.player_id == state.local_player_id)
                .and_then(|e| e.team_id.as_deref());
            status(bomb, &state.local_player_id, &state.spectating, m.is_attacking(team))
        });
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let Some(status) = &self.status else {
            return [0.0, 0.0];
        };
        let s = UI_SCALE;
        let phase_w = 16.0 * s + hud.measure(status.text, &Self::phase_style(fonts, 1.0, [0.0; 4])) + 16.0 * s;
        let mut h = 14.0 * s + 3.0 * s + (3.0 + 18.0 + 5.0) * s;
        if status.meter.is_some() {
            h += 3.0 * s + 8.0 * s;
        }
        [phase_w.max(180.0 * s), h]
    }

    /// Its top left at (`x`, `y`).
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let Some(status) = &self.status else { return };
        let s = UI_SCALE;
        hud.text_in_box(x, y, 14.0 * s, status.role, &Self::role_style(fonts, status.attacking));
        let top = y + 14.0 * s + 3.0 * s;
        let (from, to, stop, ink) = if status.carrying {
            (
                hsla(48.0, 100.0, 62.0, 1.0),
                hsla(38.0, 96.0, 50.0, 1.0),
                0.72,
                hsla(246.0, 45.0, 8.0, 1.0),
            )
        } else if status.armed {
            (
                hsla(2.0, 92.0, 62.0, 1.0),
                hsla(356.0, 84.0, 48.0, 1.0),
                0.72,
                hsla(48.0, 100.0, 95.0, 1.0),
            )
        } else {
            (
                hsla(210.0, 14.0, 88.0, 1.0),
                hsla(210.0, 12.0, 74.0, 1.0),
                0.70,
                hsla(246.0, 40.0, 8.0, 1.0),
            )
        };
        // `fps-bomb-carry-pulse`: brightness 1 to 1.22 every 1.4 s.
        let bright = if status.carrying {
            1.0 + 0.22 * pulse(self.now, 1.4)
        } else {
            1.0
        };
        let lift = |c: [f32; 4]| {
            [
                (c[0] * bright).min(1.0),
                (c[1] * bright).min(1.0),
                (c[2] * bright).min(1.0),
                c[3],
            ]
        };
        // `fps-bomb-status-urgent`: 1 to 1.1 every 0.4 s.
        let k = if status.urgent {
            1.0 + 0.1 * pulse(self.now, 0.4)
        } else {
            1.0
        };
        let style = Self::phase_style(fonts, k, ink);
        let w0 = 16.0 * s + hud.measure(status.text, &Self::phase_style(fonts, 1.0, ink)) + 16.0 * s;
        let h0 = (3.0 + 18.0 + 5.0) * s;
        let (w, h) = (w0 * k, h0 * k);
        let (left, slab_top) = (x + (w0 - w) / 2.0, top + (h0 - h) / 2.0);
        Slab {
            clip: &PHASE_CLIP,
            background: Gradient::new(178.0, &[(lift(from), None), (lift(to), Some(stop))]),
            skew_deg: 0.0,
            shadow: drop_print_sm(),
        }
        .draw(hud, left, slab_top, w, h);
        let style = if status.armed {
            style.shadow([1.0, 1.0], hsla(0.0, 60.0, 12.0, 0.8))
        } else {
            style
        };
        hud.text_in_box(
            left + 16.0 * s * k,
            slab_top + 3.0 * s * k,
            18.0 * s * k,
            status.text,
            &style,
        );
        if let Some((share, advancing)) = status.meter {
            let my = top + h0 + 3.0 * s;
            let (mw, mh) = (180.0 * s, 8.0 * s);
            hud.rect(x, my, mw, mh, hsla(250.0, 45.0, 4.0, 0.6));
            hud.outline(x, my, mw, mh, 1.0 * s, hsla(210.0, 14.0, 88.0, 0.35));
            let fill = if advancing {
                hsla(48.0, 100.0, 58.0, 1.0)
            } else {
                hsla(210.0, 12.0, 60.0, 1.0)
            };
            hud.rect(x, my, mw * share, mh, fill);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bomb(json: &str) -> Value {
        shagger::shared::json::parse(json).unwrap()
    }

    #[test]
    fn each_side_reads_its_own_news() {
        let b = bomb(r#"{"kind":"bomb","phase":"carried","carrier":"p2","worker":null,"progress":0,"fuse":null}"#);
        assert_eq!(status(&b, "p1", "", true).unwrap().text, "PLANT THE BOMB");
        assert_eq!(status(&b, "p1", "", false).unwrap().text, DEFEND);
        let mine = status(&b, "p2", "", true).unwrap();
        assert!(mine.carrying);
        assert_eq!(mine.text, CARRIER_TEXT);
        assert_eq!(mine.role, "ATTACK");
    }

    #[test]
    fn the_meter_belongs_to_the_worker_and_their_watchers() {
        let b = bomb(
            r#"{"kind":"bomb","phase":"armed","carrier":null,"worker":"p3","progress":0.42,"advancing":true,"fuse":8}"#,
        );
        let worker = status(&b, "p3", "", false).unwrap();
        assert_eq!(worker.meter, Some((0.42, true)));
        assert!(worker.urgent && worker.armed);
        assert_eq!(worker.text, "DEFUSE");
        assert_eq!(status(&b, "p9", "p3", false).unwrap().meter, Some((0.42, true)));
        assert_eq!(status(&b, "p1", "", false).unwrap().meter, None);
    }
}
