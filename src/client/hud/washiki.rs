//! The HUD's look (wash-fps `client/hud/styles/WashikiTheme.css`): its
//! colours, the fonts, and the hand-cut "slabs" (a `clip-path` polygon
//! over a `linear-gradient`, a hard offset print shadow, a skew).
//!
//! Approximations: the scanline overlays, `perspective` tilts and blurred
//! glows are left out; a slab's gradient is sampled along its edges and
//! blended to the middle.

use shagger::ui::font::Font;
use shagger::ui::hud_2d::{FontId, Hud2d, TextStyle};

/// `--hud-zoom` times the default `--ui-scale` (1).
pub const UI_SCALE: f32 = 1.15;

/// CSS `hsla(h, s%, l%, a)`.
#[must_use]
pub fn hsla(h: f32, s: f32, l: f32, a: f32) -> [f32; 4] {
    let (s, l) = (s / 100.0, l / 100.0);
    let k = |n: f32| (n + h / 30.0).rem_euclid(12.0);
    let c = s * l.min(1.0 - l);
    let f = |n: f32| l - c * (k(n) - 3.0).min(9.0 - k(n)).clamp(-1.0, 1.0);
    [f(0.0), f(8.0), f(4.0), a]
}

/// `--w-ink`, `--w-cream`, `--w-cream-dim`, `--w-void`.
#[must_use]
pub fn ink() -> [f32; 4] {
    hsla(250.0, 40.0, 5.0, 1.0)
}
#[must_use]
pub fn cream() -> [f32; 4] {
    hsla(42.0, 95.0, 92.0, 1.0)
}
#[must_use]
pub fn cream_dim() -> [f32; 4] {
    hsla(42.0, 95.0, 92.0, 0.62)
}
#[must_use]
pub fn void() -> [f32; 4] {
    hsla(255.0, 28.0, 6.0, 0.94)
}

/// The two faces the HUD uses: `--w-font-display` (Orbitron, at 900) and
/// `--w-font-body` (Rajdhani, at 700).
#[derive(Debug, Clone, Copy)]
pub struct Fonts {
    pub display: FontId,
    pub body: FontId,
}

impl Fonts {
    /// Loads the fonts shipped with the client into `hud`.
    pub fn load(hud: &mut Hud2d) -> Result<Self, String> {
        let display = Font::from_bytes(include_bytes!("../../../assets/fonts/Orbitron-Black.ttf").to_vec())?;
        let body = Font::from_bytes(include_bytes!("../../../assets/fonts/Rajdhani-Bold.ttf").to_vec())?;
        Ok(Self {
            display: hud.add_font(display),
            body: hud.add_font(body),
        })
    }

    /// Display text at `px` CSS pixels before the HUD's scale.
    #[must_use]
    pub fn display(&self, px: f32, color: [f32; 4]) -> TextStyle {
        TextStyle::new(self.display, px * UI_SCALE, color)
    }

    #[must_use]
    pub fn body(&self, px: f32, color: [f32; 4]) -> TextStyle {
        TextStyle::new(self.body, px * UI_SCALE, color)
    }
}

/// `--w-ink-text-sm` and `-md`: hard ink shadows under cream numerals.
#[must_use]
pub fn ink_text_sm(style: TextStyle) -> TextStyle {
    style.shadow([2.0 * UI_SCALE, 3.0 * UI_SCALE], ink())
}
#[must_use]
pub fn ink_text_md(style: TextStyle) -> TextStyle {
    style.shadow([3.0 * UI_SCALE, 4.0 * UI_SCALE], ink())
}

/// A `linear-gradient(angle, stops...)`; stops at fractions 0 to 1.
#[derive(Debug, Clone)]
pub struct Gradient {
    pub angle_deg: f32,
    pub stops: Vec<([f32; 4], f32)>,
}

impl Gradient {
    #[must_use]
    pub fn new(angle_deg: f32, stops: &[([f32; 4], Option<f32>)]) -> Self {
        // Stops without a position spread evenly, as CSS places them.
        let n = stops.len().max(2) - 1;
        let stops = stops
            .iter()
            .enumerate()
            .map(|(i, &(c, at))| (c, at.unwrap_or(i as f32 / n as f32)))
            .collect();
        Self { angle_deg, stops }
    }

    #[must_use]
    pub fn solid(color: [f32; 4]) -> Self {
        Self {
            angle_deg: 180.0,
            stops: vec![(color, 0.0), (color, 1.0)],
        }
    }

    /// The colour at (`x`, `y`) of a `w` by `h` box.
    #[must_use]
    pub fn at(&self, w: f32, h: f32, x: f32, y: f32) -> [f32; 4] {
        let a = self.angle_deg.to_radians();
        let (sin, cos) = a.sin_cos();
        let length = (w * sin).abs() + (h * cos).abs();
        let t = if length > 0.0 {
            ((x - w / 2.0) * sin - (y - h / 2.0) * cos) / length + 0.5
        } else {
            0.0
        };
        let stops = &self.stops;
        if t <= stops[0].1 {
            return stops[0].0;
        }
        for pair in stops.windows(2) {
            let ((c0, p0), (c1, p1)) = (pair[0], pair[1]);
            if t <= p1 {
                let f = if p1 > p0 { (t - p0) / (p1 - p0) } else { 1.0 };
                return std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * f);
            }
        }
        stops[stops.len() - 1].0
    }
}

/// A clipped, skewed slab with a hard print shadow.
#[derive(Debug, Clone)]
pub struct Slab<'a> {
    /// `clip-path: polygon(...)` in fractions of the box.
    pub clip: &'a [[f32; 2]],
    pub background: Gradient,
    /// `skewX`, degrees, about the top centre (`transform-origin: center
    /// top`).
    pub skew_deg: f32,
    /// `drop-shadow(dx dy 0 colour)` in CSS pixels before the HUD's scale.
    pub shadow: Option<([f32; 2], [f32; 4])>,
}

/// `--w-tilt-top`'s skew.
pub const TILT_TOP_SKEW: f32 = -5.0;

/// `--w-drop-print-sm` and `-md`.
#[must_use]
pub fn drop_print_sm() -> Option<([f32; 2], [f32; 4])> {
    Some(([6.0, 7.0], hsla(250.0, 45.0, 4.0, 0.6)))
}
#[must_use]
pub fn drop_print_md() -> Option<([f32; 2], [f32; 4])> {
    Some(([9.0, 9.0], hsla(250.0, 45.0, 4.0, 0.65)))
}

/// Points along each edge of a slab, for its gradient.
const EDGE_STEPS: usize = 6;

impl Slab<'_> {
    /// Fills the slab over the box at (`x`, `y`), `w` by `h` CSS pixels.
    pub fn draw(&self, hud: &mut Hud2d, x: f32, y: f32, w: f32, h: f32) {
        let skew = self.skew_deg.to_radians().tan();
        let mut outline: Vec<[f32; 2]> = Vec::new();
        let n = self.clip.len();
        for i in 0..n {
            let (a, b) = (self.clip[i], self.clip[(i + 1) % n]);
            for k in 0..EDGE_STEPS {
                let f = k as f32 / EDGE_STEPS as f32;
                outline.push([(a[0] + (b[0] - a[0]) * f) * w, (a[1] + (b[1] - a[1]) * f) * h]);
            }
        }
        let centre = outline
            .iter()
            .fold([0.0, 0.0], |c, p| [c[0] + p[0], c[1] + p[1]])
            .map(|v| v / outline.len() as f32);
        let mut local = vec![centre];
        local.extend_from_slice(&outline);
        local.push(outline[0]);
        let place = |p: &[f32; 2], dx: f32, dy: f32| [x + dx + p[0] + skew * p[1], y + dy + p[1]];
        if let Some((offset, color)) = self.shadow {
            let points: Vec<_> = local
                .iter()
                .map(|p| place(p, offset[0] * UI_SCALE, offset[1] * UI_SCALE))
                .collect();
            hud.polygon(&points, &vec![color; points.len()]);
        }
        let points: Vec<_> = local.iter().map(|p| place(p, 0.0, 0.0)).collect();
        let colors: Vec<_> = local.iter().map(|p| self.background.at(w, h, p[0], p[1])).collect();
        hud.polygon(&points, &colors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_hsl_as_css_does() {
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
        assert!(close(hsla(0.0, 100.0, 50.0, 1.0), [1.0, 0.0, 0.0, 1.0]));
        assert!(close(hsla(180.0, 100.0, 50.0, 0.5), [0.0, 1.0, 1.0, 0.5]));
        assert!(close(hsla(45.0, 100.0, 55.0, 1.0), [1.0, 0.775, 0.1, 1.0]));
    }

    #[test]
    fn gradients_follow_the_css_angle() {
        let g = Gradient::new(90.0, &[([0.0; 4], None), ([1.0; 4], None)]);
        assert_eq!(g.at(100.0, 10.0, 0.0, 5.0)[0], 0.0);
        assert!((g.at(100.0, 10.0, 50.0, 5.0)[0] - 0.5).abs() < 1e-6);
        let down = Gradient::new(180.0, &[([0.0; 4], None), ([1.0; 4], Some(0.5))]);
        assert_eq!(down.at(10.0, 100.0, 5.0, 80.0)[0], 1.0);
    }
}
