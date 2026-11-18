//! Port of wash-fps `client/ReelEffects.ts`: the highlight reel's comic
//! effects over the picture: speed lines across a cut that jumps ahead, the
//! freeze frame's panel border, halftone and caption burst, the frag counter,
//! and the tag naming each scene's player and play in the lower right.
//!
//! The TypeScript draws on a 2D canvas; here the same shapes go to an egui
//! painter, with the canvas transforms worked out by hand. egui cannot shear
//! text, so the captions lean by their rotation only.
//!
//! Sizes are for a 1080-line picture and scale with its height; the layout
//! follows its width, so a wide screen keeps the shapes round.

use std::f64::consts::PI;

use shagger::ui::egui_layer::egui;

use egui::epaint::{Mesh, TextShape};
use egui::{Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::client::washoi_style::display_family;

/// `ReelEffects.wipe`: 0 – 1 across the whole wipe; the cut is at 0.5.
/// `angle` in degrees, 0 sweeping right to left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReelWipe {
    pub progress: f64,
    pub angle: f64,
    pub seed: i64,
}

/// `ReelEffects.freeze`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelFreezeEffect {
    pub elapsed_ms: f64,
    pub total_ms: f64,
    pub caption: String,
    pub seed: i64,
}

/// `ReelEffects.stamp`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelStampEffect {
    pub text: String,
    pub age_ms: f64,
    pub total_ms: f64,
}

/// `ReelEffects.tag`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelTagEffect {
    pub who: String,
    pub what: String,
    pub age_ms: f64,
    pub total_ms: f64,
}

/// `ReelEffects`; the default is `NO_REEL_EFFECTS`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReelEffects {
    pub wipe: Option<ReelWipe>,
    pub freeze: Option<ReelFreezeEffect>,
    pub stamp: Option<ReelStampEffect>,
    pub tag: Option<ReelTagEffect>,
}

impl ReelEffects {
    /// `hasReelEffects`.
    #[must_use]
    pub fn any(&self) -> bool {
        self.wipe.is_some() || self.freeze.is_some() || self.stamp.is_some() || self.tag.is_some()
    }
}

/// The comic's colours: warm paper, ink, and the game's gold.
const PAPER: Color32 = Color32::from_rgb(255, 250, 240);
const INK: Color32 = Color32::from_rgb(0x10, 0x13, 0x1c);
const GOLD: Color32 = Color32::from_rgb(0xff, 0xd2, 0x1c);

/// The freeze frame's panel slams in this fast, and lets go over this long at its end.
const PANEL_IN_MS: f64 = 140.0;
const PANEL_OUT_MS: f64 = 220.0;
/// The caption burst's pop: overshoot, then settle.
const BURST_POP_MS: f64 = 240.0;
/// The scene tag slides in this fast, and out over this long at its end.
const TAG_IN_MS: f64 = 260.0;
const TAG_OUT_MS: f64 = 260.0;

/// A canvas transform: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Debug, Clone, Copy)]
struct Xf {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Xf {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `context.transform(a, b, c, d, e, f)`.
    fn then(self, m: Self) -> Self {
        Self {
            a: self.a * m.a + self.c * m.b,
            b: self.b * m.a + self.d * m.b,
            c: self.a * m.c + self.c * m.d,
            d: self.b * m.c + self.d * m.d,
            e: self.a * m.e + self.c * m.f + self.e,
            f: self.b * m.e + self.d * m.f + self.f,
        }
    }

    fn translate(self, x: f64, y: f64) -> Self {
        self.then(Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        })
    }

    fn rotate(self, angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        self.then(Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        })
    }

    fn scale(self, x: f64, y: f64) -> Self {
        self.then(Self {
            a: x,
            d: y,
            ..Self::IDENTITY
        })
    }

    fn at(self, x: f64, y: f64) -> Pos2 {
        pos2(
            (self.a * x + self.c * y + self.e) as f32,
            (self.b * x + self.d * y + self.f) as f32,
        )
    }

    /// The rotation the transform turns the x axis by.
    fn angle(self) -> f64 {
        self.b.atan2(self.a)
    }
}

/// What the drawing works in: the painter, the picture's corner, and the opacity.
struct Canvas<'a> {
    painter: &'a Painter,
    origin: Pos2,
    alpha: f32,
}

impl Canvas<'_> {
    fn color(&self, c: Color32, alpha: f64) -> Color32 {
        c.gamma_multiply((f64::from(self.alpha) * alpha).clamp(0.0, 1.0) as f32)
    }

    fn base(&self) -> Xf {
        Xf::IDENTITY.translate(f64::from(self.origin.x), f64::from(self.origin.y))
    }

    /// A filled polygon, star shaped round its first point's centre: a fan.
    fn fill(&self, points: &[Pos2], center: Pos2, color: Color32) {
        if points.len() < 3 || color.a() == 0 {
            return;
        }
        let mut mesh = Mesh::default();
        mesh.colored_vertex(center, color);
        for &p in points {
            mesh.colored_vertex(p, color);
        }
        let n = points.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        self.painter.add(Shape::mesh(mesh));
    }

    fn outline(&self, points: &[Pos2], width: f64, color: Color32) {
        if color.a() == 0 {
            return;
        }
        self.painter
            .add(Shape::closed_line(points.to_vec(), Stroke::new(width as f32, color)));
    }

    fn rect(&self, xf: Xf, x: f64, y: f64, w: f64, h: f64) -> [Pos2; 4] {
        [xf.at(x, y), xf.at(x + w, y), xf.at(x + w, y + h), xf.at(x, y + h)]
    }

    fn fill_rect(&self, xf: Xf, x: f64, y: f64, w: f64, h: f64, color: Color32) {
        let quad = self.rect(xf, x, y, w, h);
        self.fill(&quad, xf.at(x + w / 2.0, y + h / 2.0), color);
    }

    /// Text in the caption face, its anchor (`ax`, `ay`: 0 left or top, 0.5
    /// middle) at (`x`, `y`) under `xf`; a `edge` outline round it first.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        xf: Xf,
        x: f64,
        y: f64,
        anchor: (f64, f64),
        text: &str,
        size: f64,
        fill: Color32,
        edge: Option<(f64, Color32)>,
    ) {
        if size <= 0.5 {
            return;
        }
        let font = FontId::new(size as f32, display_family());
        let galley = self.painter.layout_no_wrap(text.to_owned(), font, Color32::WHITE);
        let angle = xf.angle();
        let at = xf.at(x, y);
        let (w, h) = (f64::from(galley.size().x), f64::from(galley.size().y));
        let (sin, cos) = angle.sin_cos();
        let (ox, oy) = (w * anchor.0, h * anchor.1);
        let corner = pos2(at.x - (cos * ox - sin * oy) as f32, at.y - (sin * ox + cos * oy) as f32);
        let draw = |offset: egui::Vec2, color: Color32| {
            self.painter.add(
                TextShape::new(corner + offset, galley.clone(), color)
                    .with_override_text_color(color)
                    .with_angle(angle as f32),
            );
        };
        if let Some((width, color)) = edge {
            let r = (width / 2.0) as f32;
            for k in 0..8 {
                let a = f32::from(k as u8) * std::f32::consts::FRAC_PI_4;
                draw(vec2(a.cos() * r, a.sin() * r), color);
            }
        }
        draw(egui::Vec2::ZERO, fill);
    }

    fn measure(&self, text: &str, size: f64) -> f64 {
        let font = FontId::new(size as f32, display_family());
        f64::from(
            self.painter
                .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
                .size()
                .x,
        )
    }
}

/// `drawReelEffects`: draw `effects` over the picture in `screen`.
pub fn draw_reel_effects(painter: &Painter, effects: &ReelEffects, screen: Rect) {
    let width = f64::from(screen.width());
    let height = f64::from(screen.height());
    let unit = height / 1080.0;
    let canvas = Canvas {
        painter,
        origin: screen.min,
        alpha: 1.0,
    };
    if let Some(freeze) = &effects.freeze {
        draw_freeze(&canvas, freeze, width, height, unit);
    }
    if let Some(tag) = &effects.tag {
        draw_tag(&canvas, tag, width, height, unit);
    }
    if let Some(stamp) = &effects.stamp {
        draw_stamp(&canvas, stamp, width, height, unit);
    }
    if let Some(wipe) = &effects.wipe {
        draw_wipe(&canvas, wipe, width, height, unit);
    }
}

// ── Speed lines ──

/// `drawWipe`: streaks of warm white and gold sweeping across the cut, thick
/// at the middle of the wipe, clear along the middle of the picture where
/// the eye is.
fn draw_wipe(canvas: &Canvas<'_>, wipe: &ReelWipe, width: f64, height: f64, unit: f64) {
    let progress = wipe.progress.clamp(0.0, 1.0);
    let strength = (PI * progress).sin().max(0.0).sqrt();
    if strength <= 0.001 {
        return;
    }
    let mut random = seeded((wipe.seed * 7919 + 17) as f64);
    let diagonal = width.hypot(height);
    let xf = canvas
        .base()
        .translate(width / 2.0, height / 2.0)
        .rotate(wipe.angle * PI / 180.0);
    for _ in 0..150 {
        let y = (random() - 0.5) * diagonal;
        // Lighter along the middle, where the eye is; full toward the edges.
        let away = (y.abs() / (height * 0.45)).min(1.0);
        let alpha = strength * (0.35 + 0.65 * away) * (0.55 + 0.45 * random());
        let length = diagonal * (0.25 + random() * 0.55);
        let speed = 0.8 + random() * 0.5;
        // Each streak slides across, centred on the screen at the cut.
        let x = diagonal * (0.5 - progress) * speed * 1.6 - length / 2.0 + (random() - 0.5) * diagonal * 0.5;
        let tone = random();
        // Mostly warm white, some gold, a few ink streaks that show on bright walls.
        let color = if tone < 0.22 {
            canvas.color(Color32::from_rgb(255, 214, 61), alpha)
        } else if tone < 0.32 {
            canvas.color(Color32::from_rgb(18, 21, 33), alpha * 0.8)
        } else {
            canvas.color(Color32::from_rgb(255, 250, 235), alpha)
        };
        let line_width = (2.0 + random() * 7.0) * unit;
        canvas.painter.line_segment(
            [xf.at(x, y), xf.at(x + length, y)],
            Stroke::new(line_width as f32, color),
        );
    }
}

// ── Freeze frame ──

/// `drawFreeze`: the still picture as a comic panel: paper gutter and ink
/// frame, halftone corners, a caption burst.
fn draw_freeze(canvas: &Canvas<'_>, freeze: &ReelFreezeEffect, width: f64, height: f64, unit: f64) {
    let t = freeze.elapsed_ms;
    let fade_out = ((freeze.total_ms - t) / PANEL_OUT_MS).clamp(0.0, 1.0);
    let slam = (t / PANEL_IN_MS).min(1.0);
    let canvas = Canvas {
        alpha: canvas.alpha * fade_out as f32,
        ..*canvas
    };
    let base = canvas.base();

    // Halftone dots gathering in the corners.
    draw_halftone(&canvas, width, height, unit, (t / 200.0).min(1.0));

    // The panel: a paper gutter round the picture with an ink line inside,
    // slamming in from a little outside the screen.
    let gutter = (18.0 + (1.0 - ease_out(slam)) * -40.0) * unit;
    let ink = 7.0 * unit;
    if gutter > 0.0 {
        let paper = canvas.color(PAPER, 1.0);
        canvas.fill_rect(base, 0.0, 0.0, width, gutter, paper);
        canvas.fill_rect(base, 0.0, height - gutter, width, gutter, paper);
        canvas.fill_rect(base, 0.0, 0.0, gutter, height, paper);
        canvas.fill_rect(base, width - gutter, 0.0, gutter, height, paper);
    }
    let frame = canvas.rect(
        base,
        gutter + ink / 2.0,
        gutter + ink / 2.0,
        width - 2.0 * gutter - ink,
        height - 2.0 * gutter - ink,
    );
    canvas.outline(&frame, ink, canvas.color(INK, 1.0));

    // The caption burst, high on the left.
    let pop = pop_scale(t / BURST_POP_MS);
    if pop > 0.0 {
        draw_burst(
            &canvas,
            (width * 0.27, height * 0.26),
            200.0 * unit * pop,
            -0.14,
            freeze.seed as f64,
            &freeze.caption,
            86.0 * unit * pop,
        );
    }
}

/// `drawHalftone`: ink dots on a grid, larger toward the corners and none in the middle.
fn draw_halftone(canvas: &Canvas<'_>, width: f64, height: f64, unit: f64, amount: f64) {
    if amount <= 0.0 {
        return;
    }
    let step = 20.0 * unit;
    let max_r = step * 0.42;
    let color = canvas.color(INK, 0.5 * amount);
    let (cx, cy) = (width / 2.0, height / 2.0);
    let base = canvas.base();
    let mut y = step / 2.0;
    while y < height {
        let mut x = step / 2.0;
        while x < width {
            let (dx, dy) = ((x - cx) / cx, (y - cy) / cy);
            let corner = (dx.hypot(dy) / std::f64::consts::SQRT_2 - 0.62).max(0.0) / 0.38;
            if corner > 0.0 {
                canvas
                    .painter
                    .circle_filled(base.at(x, y), (max_r * corner.min(1.0)) as f32, color);
            }
            x += step;
        }
        y += step;
    }
}

// ── Frag counter ──

/// `drawStamp`: the count so far, popping up on the right as each frag lands.
fn draw_stamp(canvas: &Canvas<'_>, stamp: &ReelStampEffect, width: f64, height: f64, unit: f64) {
    let pop = pop_scale(stamp.age_ms / BURST_POP_MS);
    let fade = ((stamp.total_ms - stamp.age_ms) / 200.0).clamp(0.0, 1.0);
    if pop <= 0.0 || fade <= 0.0 {
        return;
    }
    let canvas = Canvas {
        alpha: canvas.alpha * fade as f32,
        ..*canvas
    };
    let seed = match stamp.text.parse::<i64>() {
        Ok(n) if n != 0 => n as f64,
        _ => stamp.text.chars().count() as f64,
    };
    draw_burst(
        &canvas,
        (width * 0.86, height * 0.52),
        96.0 * unit * pop,
        0.12,
        seed,
        &stamp.text,
        78.0 * unit * pop,
    );
}

// ── Scene tag ──

/// `drawTag`: who and what, as a comic caption in the lower right, clear of
/// the kill feed and chat on the left: the play in ink on a gold box, the
/// player's name on a paper box pinned over its corner, both with a hard ink
/// shadow. It slides in from the right and drops away at its end.
fn draw_tag(canvas: &Canvas<'_>, tag: &ReelTagEffect, width: f64, height: f64, unit: f64) {
    let t = tag.age_ms;
    let enter = (t / TAG_IN_MS).clamp(0.0, 1.0);
    let leave = ((tag.total_ms - t) / TAG_OUT_MS).clamp(0.0, 1.0);
    if enter <= 0.0 || leave <= 0.0 {
        return;
    }
    let room = width * 0.42;

    // Both boxes are measured first: the caption is placed by its right edge.
    let what = tag.what.to_uppercase();
    let (what_size, what_w) = fit_text(canvas, &what, 46.0 * unit, room);
    let pad_x = 26.0 * unit;
    let box_w = what_w + pad_x * 2.0;
    let box_h = what_size + 44.0 * unit;
    let (who_size, who_w) = fit_text(canvas, &tag.who, 28.0 * unit, room * 0.8);
    let who_box_w = who_w + 36.0 * unit;
    let who_box_h = who_size + 22.0 * unit;
    let who_x = -14.0 * unit;
    let span = box_w.max(who_x + who_box_w) + 8.0 * unit;

    let left = width - 64.0 * unit - span;
    let bottom = height - 150.0 * unit;
    let slide = (1.0 - ease_out_back(enter)) * (room + 120.0 * unit) - (1.0 - leave) * 40.0 * unit;
    let canvas = Canvas {
        alpha: canvas.alpha * (leave * 1.5).min(1.0) as f32,
        ..*canvas
    };
    let xf = canvas
        .base()
        .translate(left + slide, bottom + (1.0 - leave) * 30.0 * unit)
        .rotate(-0.045);

    // The play, on gold.
    caption_box(&canvas, xf, (0.0, -box_h, box_w, box_h), GOLD, unit);
    // A little below the middle, clear of the name box over the top edge.
    canvas.text(
        xf,
        pad_x + what_size * 0.1,
        -box_h / 2.0 + 5.0 * unit + what_size * 0.04,
        (0.0, 0.5),
        &what,
        what_size,
        canvas.color(INK, 1.0),
        None,
    );

    // The player, on paper, pinned over its top-left corner a beat later.
    let drop = ((t - TAG_IN_MS * 0.4) / TAG_IN_MS).clamp(0.0, 1.0);
    if drop > 0.0 {
        let (w, h) = (who_box_w, who_box_h);
        let y = -box_h - h + 6.0 * unit - (1.0 - ease_out_back(drop)) * 24.0 * unit;
        let canvas = Canvas {
            alpha: canvas.alpha * drop as f32,
            ..canvas
        };
        let xf = xf.translate(who_x, y).rotate(0.03);
        caption_box(&canvas, xf, (0.0, 0.0, w, h), PAPER, unit);
        canvas.text(
            xf,
            18.0 * unit,
            h / 2.0 + who_size * 0.04,
            (0.0, 0.5),
            &tag.who,
            who_size,
            canvas.color(INK, 1.0),
            None,
        );
    }
}

/// `captionBox`: a hard ink shadow, then the fill with an ink outline.
fn caption_box(canvas: &Canvas<'_>, xf: Xf, (x, y, w, h): (f64, f64, f64, f64), fill: Color32, unit: f64) {
    let shadow = 8.0 * unit;
    canvas.fill_rect(xf, x + shadow, y + shadow, w, h, canvas.color(INK, 1.0));
    canvas.fill_rect(xf, x, y, w, h, canvas.color(fill, 1.0));
    let outline = canvas.rect(xf, x, y, w, h);
    canvas.outline(&outline, (5.0 * unit).max(2.0), canvas.color(INK, 1.0));
}

/// `fitText`: the size `text` is drawn at, smaller if it is wider than
/// `room`, and its width.
fn fit_text(canvas: &Canvas<'_>, text: &str, size: f64, room: f64) -> (f64, f64) {
    let width = canvas.measure(text, size);
    if width > room {
        (size * room / width, room)
    } else {
        (size, width)
    }
}

// ── Shared shapes ──

/// `drawBurst`: a jagged comic burst in gold with an ink outline, with
/// `text` across it.
fn draw_burst(
    canvas: &Canvas<'_>,
    (cx, cy): (f64, f64),
    radius: f64,
    tilt: f64,
    seed: f64,
    text: &str,
    font_size: f64,
) {
    if radius <= 0.0 {
        return;
    }
    let mut random = seeded(seed * 104_729.0 + 3.0);
    let points = 18;
    let xf = canvas.base().translate(cx, cy).rotate(tilt);
    // Wider than tall, as a shout is.
    let wide = xf.scale(1.35, 1.0);
    let outline: Vec<Pos2> = (0..points * 2)
        .map(|i| {
            let angle = f64::from(i) / f64::from(points * 2) * PI * 2.0;
            let r = radius
                * if i % 2 == 0 {
                    1.0 - random() * 0.12
                } else {
                    0.7 + random() * 0.08
                };
            wide.at(angle.cos() * r, angle.sin() * r)
        })
        .collect();
    canvas.fill(&outline, wide.at(0.0, 0.0), canvas.color(GOLD, 1.0));
    canvas.outline(&outline, (radius * 0.045).max(2.0), canvas.color(INK, 1.0));

    // The words: ink with a paper edge.
    let room = radius * 2.3;
    let measured = canvas.measure(text, font_size);
    let size = if measured > room {
        font_size * room / measured
    } else {
        font_size
    };
    canvas.text(
        xf,
        0.0,
        size * 0.04,
        (0.5, 0.5),
        text,
        size,
        canvas.color(INK, 1.0),
        Some(((size * 0.14).max(2.0), canvas.color(PAPER, 1.0))),
    );
}

/// `popScale`: 0, past full size, then back to 1 over `t` in 0..1; 1 after.
fn pop_scale(t: f64) -> f64 {
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else if t < 0.6 {
        t / 0.6 * 1.18
    } else {
        1.18 - (t - 0.6) / 0.4 * 0.18
    }
}

/// `easeOutBack`: eases past 1 and back, for a slide that lands with a kick.
fn ease_out_back(t: f64) -> f64 {
    let k = 1.6;
    let u = t - 1.0;
    1.0 + (k + 1.0) * u * u * u + k * u * u
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t) * (1.0 - t)
}

/// `seeded`: a small repeatable random sequence, so every frame of a wipe
/// draws the same streaks.
fn seeded(seed: f64) -> impl FnMut() -> f64 {
    let mut state = (seed.floor().abs() % 2_147_483_646.0) + 1.0;
    move || {
        state = (state * 16807.0) % 2_147_483_647.0;
        (state - 1.0) / 2_147_483_646.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_sequence_repeats() {
        let mut a = seeded(17.0);
        let mut b = seeded(17.0);
        for _ in 0..10 {
            let x = a();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x.to_bits(), b().to_bits());
        }
    }

    #[test]
    fn the_pop_overshoots_and_settles() {
        assert_eq!(pop_scale(0.0), 0.0);
        assert!((pop_scale(0.6) - 1.18).abs() < 1e-12);
        assert_eq!(pop_scale(1.0), 1.0);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-12);
    }
}
