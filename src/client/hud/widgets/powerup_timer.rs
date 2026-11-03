//! The countdown slab while the player on screen holds a powerup (wash-fps
//! `client/hud/widgets/PowerupTimer.ts`, styles `.fps-powerup-timer`): above
//! the vitals, gold for Quad Damage, flipping to a pulsing hot red for the
//! last five seconds. Reads [`LocalPowerupState`] through the HUD's state, so
//! it cannot outlive the effect it advertises.
//!
//! [`LocalPowerupState`]: crate::client::local_powerup_state::LocalPowerupState

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, hsla};

/// Below this many seconds the slab turns urgent.
const EXPIRING_SECONDS: f64 = 5.0;

/// `clip-path: polygon(3% 0, 100% 0, 97% 100%, 0 100%)`.
const CLIP: [[f32; 2]; 4] = [[0.03, 0.0], [1.0, 0.0], [0.97, 1.0], [0.0, 1.0]];

/// `POWERUP_STYLE`: the label per kind.
fn powerup_label(kind: &str) -> Option<&'static str> {
    (kind == "quad").then_some("QUAD")
}

/// One slab: its label, whole seconds and whether it is expiring.
#[derive(Debug, Clone, PartialEq)]
struct TimerSlab {
    label: &'static str,
    count: String,
    expiring: bool,
}

/// `FPSPowerupTimer`.
#[derive(Debug, Default)]
pub struct PowerupTimer {
    slabs: Vec<TimerSlab>,
    /// The HUD clock, for the expiring pulse.
    now: f64,
}

impl PowerupTimer {
    /// The slabs for these timers (kind, seconds left), or none while
    /// `dead`.
    fn slabs_for(active: &[(String, f64)], dead: bool) -> Vec<TimerSlab> {
        if dead {
            return Vec::new();
        }
        let mut active = active.to_vec();
        active.sort_by(|a, b| a.0.cmp(&b.0));
        active
            .iter()
            .filter_map(|(kind, seconds)| {
                powerup_label(kind).map(|label| TimerSlab {
                    label,
                    count: format!("{}", seconds.ceil() as i64),
                    expiring: *seconds <= EXPIRING_SECONDS,
                })
            })
            .collect()
    }

    fn slab_size(hud: &Hud2d, fonts: &Fonts, slab: &TimerSlab) -> [f32; 2] {
        let s = UI_SCALE;
        let label = hud.measure(slab.label, &fonts.display(15.0, [0.0; 4]).spacing(15.0 * s * 0.08));
        let count = hud.measure(&slab.count, &fonts.display(21.0, [0.0; 4]));
        [14.0 * s + label + 8.0 * s + count + 14.0 * s, (4.0 + 21.0 + 6.0) * s]
    }

    /// `fps-powerup-timer-expiring`: 1 to 1.06 and back each half second.
    fn pulse(&self) -> f32 {
        let t = (self.now / 0.5).fract();
        let half = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
        // ease-in-out
        let eased = half * half * (3.0 - 2.0 * half);
        (1.0 + 0.06 * eased) as f32
    }
}

impl FpsHudWidget for PowerupTimer {
    fn update(&mut self, frame: &HudFrame<'_>) {
        self.now = frame.state.now;
        let active = frame.state.powerups.clone().active(frame.state.now);
        self.slabs = Self::slabs_for(&active, frame.state.dead);
    }

    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2] {
        if self.slabs.is_empty() {
            return [0.0, 0.0];
        }
        let s = UI_SCALE;
        let mut w: f32 = 0.0;
        let mut h = 8.0 * s;
        for (i, slab) in self.slabs.iter().enumerate() {
            let [sw, sh] = Self::slab_size(hud, fonts, slab);
            w = w.max(sw);
            h += sh + if i > 0 { 4.0 * s } else { 0.0 };
        }
        [w, h]
    }

    /// At `x`, the stack's top at `y` (its 8 px margin below).
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32) {
        let s = UI_SCALE;
        let mut top = y;
        for slab in &self.slabs {
            let [w0, h0] = Self::slab_size(hud, fonts, slab);
            let k = if slab.expiring { self.pulse() } else { 1.0 };
            let (w, h) = (w0 * k, h0 * k);
            let (left, slab_top) = (x + (w0 - w) / 2.0, top + (h0 - h) / 2.0);
            let (from, to, stop) = if slab.expiring {
                (hsla(2.0, 92.0, 60.0, 1.0), hsla(356.0, 82.0, 46.0, 1.0), 0.72)
            } else {
                (hsla(48.0, 100.0, 62.0, 1.0), hsla(43.0, 96.0, 50.0, 1.0), 0.70)
            };
            Slab {
                clip: &CLIP,
                background: Gradient::new(178.0, &[(from, None), (to, Some(stop))]),
                skew_deg: 0.0,
                shadow: Some(([4.0, 5.0], hsla(250.0, 45.0, 4.0, 0.55))),
            }
            .draw(hud, left, slab_top, w, h);
            let (ink, glow) = if slab.expiring {
                (hsla(48.0, 100.0, 94.0, 1.0), hsla(0.0, 60.0, 12.0, 0.8))
            } else {
                (hsla(246.0, 40.0, 8.0, 1.0), hsla(45.0, 100.0, 78.0, 0.7))
            };
            let label = fonts
                .display(15.0 * k, ink)
                .spacing(15.0 * s * k * 0.08)
                .align(Align::Left);
            let label = if slab.expiring {
                label.shadow([1.0 * s, 1.0 * s], glow)
            } else {
                label
            };
            let count = fonts
                .display(21.0 * k, ink)
                .shadow([1.0 * s, 1.0 * s], glow)
                .align(Align::Left);
            let content_top = slab_top + 4.0 * s * k;
            let content_h = 21.0 * s * k;
            let lw = hud.text_in_box(left + 14.0 * s * k, content_top, content_h, slab.label, &label);
            hud.text_in_box(
                left + 14.0 * s * k + lw + 8.0 * s * k,
                content_top,
                content_h,
                &slab.count,
                &count,
            );
            top += h0 + 4.0 * s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_seconds_and_the_urgent_look() {
        let slabs = PowerupTimer::slabs_for(&[("quad".to_owned(), 12.2)], false);
        assert_eq!(
            slabs,
            vec![TimerSlab {
                label: "QUAD",
                count: "13".to_owned(),
                expiring: false
            }]
        );
        let slabs = PowerupTimer::slabs_for(&[("quad".to_owned(), 4.5)], false);
        assert!(slabs[0].expiring);
        assert_eq!(slabs[0].count, "5");
        assert!(PowerupTimer::slabs_for(&[("quad".to_owned(), 4.5)], true).is_empty());
        assert!(PowerupTimer::slabs_for(&[("regen".to_owned(), 4.5)], false).is_empty());
    }
}
