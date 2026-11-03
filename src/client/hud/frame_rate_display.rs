//! wash-fps `client/hud/FrameRateDisplay.ts` (styles
//! `client/hud/styles/FrameRateDisplay.css`): the `show_fps` readout in the
//! top-right corner, rewritten four times a second.

use std::collections::VecDeque;

use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use super::washiki::{Fonts, Gradient, Slab, hsla};

/// Babylon's `engine.getFps()` averages the last 60 frames.
const SAMPLES: usize = 60;
/// `setInterval(update, 250)`.
const REFRESH_SECONDS: f64 = 0.25;

/// The readout and the frame times it averages.
#[derive(Debug, Default)]
pub struct FrameRateDisplay {
    frames: VecDeque<f64>,
    since_refresh: f64,
    /// The number shown; `None` while `show_fps` is off.
    text: Option<String>,
}

impl FrameRateDisplay {
    /// Counts a frame of `dt` seconds; `show` is `show_fps`.
    pub fn tick(&mut self, dt: f64, show: bool) {
        if dt > 0.0 {
            if self.frames.len() == SAMPLES {
                self.frames.pop_front();
            }
            self.frames.push_back(dt);
        }
        self.since_refresh += dt;
        let due = self.since_refresh >= REFRESH_SECONDS || self.text.is_none() != !show;
        if !due {
            return;
        }
        self.since_refresh = 0.0;
        self.text = show.then(|| format_fps(self.fps()));
    }

    /// Frames a second over the last [`SAMPLES`] frames.
    #[must_use]
    pub fn fps(&self) -> f64 {
        let total: f64 = self.frames.iter().sum();
        if total > 0.0 {
            self.frames.len() as f64 / total
        } else {
            0.0
        }
    }

    /// The number shown, while `show_fps` is on.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Draws the badge flush with the top-right corner of a `width` wide
    /// screen.
    pub fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, width: f32) {
        let Some(text) = &self.text else { return };
        // Outside the HUD's zoom: `--ui-scale` alone, which is 1.
        let ink = [23.0 / 255.0, 19.0 / 255.0, 38.0 / 255.0, 1.0];
        let value = TextStyle::new(fonts.display, 17.0, ink).align(Align::Right);
        let label = TextStyle::new(fonts.display, 10.0, ink);
        // `min-width: 4ch` keeps the badge still as the digits change.
        let value_w = hud.measure("0000", &value).max(hud.measure(text, &value));
        let label_w = hud.measure("FPS", &label) * 1.08;
        let (pad_l, pad_r, pad_t, pad_b) = (22.0, 14.0, 8.0, 9.0);
        let gap = 7.0;
        let (ascent, descent) = hud.line_metrics(&value);
        let h = pad_t + ascent + descent + pad_b;
        let w = pad_l + value_w + gap + label_w + pad_r;
        let x = width - w;
        // `clip-path: polygon(10px 0, 100% 0, 100% 100%, 0 100%)`.
        let clip = [[10.0 / w, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        Slab {
            clip: &clip,
            background: Gradient::new(
                120.0,
                &[
                    (hsla(43.0, 100.0, 59.0, 1.0), None),
                    (hsla(42.0, 100.0, 47.0, 1.0), Some(1.0)),
                ],
            ),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, x, 0.0, w, h);
        // `box-shadow: inset 0 -3px 0 #cd8600`.
        hud.rect(x + 3.0, h - 3.0, w - 3.0, 3.0, hsla(39.0, 100.0, 40.0, 1.0));
        let baseline = pad_t + ascent;
        hud.text(x + pad_l + value_w, baseline, text, &value);
        hud.text(x + pad_l + value_w + gap, baseline, "FPS", &label);
    }
}

/// `fps.toFixed(0)`, or a dash before there is a number.
#[must_use]
pub fn format_fps(fps: f64) -> String {
    if fps.is_finite() && fps > 0.0 {
        format!("{fps:.0}")
    } else {
        "\u{2014}".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_the_last_frames_and_refreshes_four_times_a_second() {
        let mut d = FrameRateDisplay::default();
        assert_eq!(d.text(), None);
        d.tick(1.0 / 50.0, true);
        assert_eq!(d.text(), Some("50"));
        for _ in 0..5 {
            d.tick(1.0 / 100.0, true);
        }
        // Not yet a quarter second since it was written.
        assert_eq!(d.text(), Some("50"));
        for _ in 0..30 {
            d.tick(1.0 / 100.0, true);
        }
        // Rewritten at the quarter second: 26 frames over 0.27 s.
        assert_eq!(d.text(), Some("96"));
        d.tick(0.01, false);
        assert_eq!(d.text(), None);
    }

    #[test]
    fn no_frames_shows_a_dash() {
        assert_eq!(format_fps(0.0), "\u{2014}");
        assert_eq!(format_fps(f64::NAN), "\u{2014}");
        assert_eq!(format_fps(143.6), "144");
    }
}
