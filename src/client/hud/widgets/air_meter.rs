//! The breath gauge while our head is under (wash-fps
//! `client/hud/widgets/AirMeter.ts`, styles `.fps-air-meter`): a blue slab
//! above the powerup timer, amber when low, a pulsing red DROWNING once the
//! air is gone. Hidden at full air and once we are dead. Reads the local
//! air mirror (`LocalAirTracker`), not replicated state.

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, hsla};

/// Below this fraction the meter switches to its urgent look.
const LOW_AIR: f64 = 0.3;
/// The bar moves in 5 % jumps.
const STEPS: f64 = 20.0;

/// `clip-path: polygon(3% 0, 100% 0, 97% 100%, 0 100%)`.
const CLIP: [[f32; 2]; 4] = [[0.03, 0.0], [1.0, 0.0], [0.97, 1.0], [0.0, 1.0]];

/// What the meter shows: the filled share (in steps), low, drowning.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Gauge {
    fill: f32,
    low: bool,
    drowning: bool,
}

/// `FPSAirMeter`.
#[derive(Debug, Default)]
pub struct AirMeter {
    gauge: Option<Gauge>,
    now: f64,
}

impl AirMeter {
    /// The gauge for this air, or none at full air or while dead.
    fn gauge(air: f64, drowning: bool, local_dead: bool) -> Option<Gauge> {
        if local_dead || air >= 1.0 {
            return None;
        }
        Some(Gauge {
            fill: ((air * STEPS).round() / STEPS) as f32,
            low: air <= LOW_AIR,
            drowning,
        })
    }

    fn label(gauge: &Gauge) -> &'static str {
        if gauge.drowning { "DROWNING" } else { "AIR" }
    }
}

impl FpsHudWidget for AirMeter {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.now = frame.state.now;
        self.gauge = Self::gauge(frame.state.air, frame.state.drowning, frame.state.local_dead);
    }

    /// With its 8 px margin below.
    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        let Some(gauge) = &self.gauge else {
            return [0.0, 0.0];
        };
        let s = UI_SCALE;
        let label = hud.measure(
            Self::label(gauge),
            &fonts.display(13.0, [0.0; 4]).spacing(13.0 * s * 0.08),
        );
        [
            12.0 * s + label + 8.0 * s + 120.0 * s + 12.0 * s,
            (4.0 + 15.0 + 6.0 + 8.0) * s,
        ]
    }

    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let Some(gauge) = &self.gauge else { return };
        let s = UI_SCALE;
        let [w, h] = self.size(hud, fonts);
        let h = h - 8.0 * s;
        let (from, to, stop) = if gauge.drowning {
            (hsla(2.0, 92.0, 60.0, 1.0), hsla(356.0, 82.0, 46.0, 1.0), 0.72)
        } else if gauge.low {
            (hsla(38.0, 96.0, 62.0, 1.0), hsla(32.0, 92.0, 48.0, 1.0), 0.70)
        } else {
            (hsla(196.0, 70.0, 58.0, 1.0), hsla(202.0, 72.0, 42.0, 1.0), 0.70)
        };
        // `fps-air-meter-drowning`: opacity 1 to 0.55 and back each half second.
        let alpha = if gauge.drowning {
            let t = (self.now / 0.5).fract();
            let half = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
            1.0 - 0.45 * (half * half * (3.0 - 2.0 * half)) as f32
        } else {
            1.0
        };
        let fade = |mut c: [f32; 4]| {
            c[3] *= alpha;
            c
        };
        Slab {
            clip: &CLIP,
            background: Gradient::new(178.0, &[(fade(from), None), (fade(to), Some(stop))]),
            skew_deg: 0.0,
            shadow: Some(([4.0, 5.0], fade(hsla(250.0, 45.0, 4.0, 0.55)))),
        }
        .draw(hud, x, y, w, h);
        let ink = fade(hsla(246.0, 40.0, 8.0, 1.0));
        let label = fonts.display(13.0, ink).spacing(13.0 * s * 0.08).align(Align::Left);
        let content_top = y + 4.0 * s;
        let content_h = 15.0 * s;
        let lw = hud.text_in_box(x + 12.0 * s, content_top, content_h, Self::label(gauge), &label);
        let track_x = x + 12.0 * s + lw + 8.0 * s;
        let track_y = content_top + (content_h - 9.0 * s) / 2.0;
        hud.rect(track_x, track_y, 120.0 * s, 9.0 * s, fade(hsla(246.0, 40.0, 8.0, 0.42)));
        hud.rect(track_x, track_y, 120.0 * s * gauge.fill, 9.0 * s, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_only_while_air_runs_out() {
        assert_eq!(AirMeter::gauge(1.0, false, false), None);
        assert_eq!(AirMeter::gauge(0.5, false, true), None);
        let g = AirMeter::gauge(0.52, false, false).unwrap();
        assert_eq!(g.fill, 0.5);
        assert!(!g.low);
        let g = AirMeter::gauge(0.0, true, false).unwrap();
        assert!(g.low && g.drowning);
        assert_eq!(AirMeter::label(&g), "DROWNING");
    }
}
