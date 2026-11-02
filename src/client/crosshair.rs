//! The crosshair (wash-fps `client/Crosshair.ts`): the weapon's reticle at
//! the centre, its gap opened by speed, red on a hit, a red cross when the
//! gun is empty. The cvars' defaults (`cg_crosshairSize` 32, colour
//! `00FF00`, alpha 0.9, spread on, hit colour `FF0000`).
//!
//! Not yet: aiming down sights and the spread bloom of sustained fire.

use shagger::ui::hud_2d::Hud2d;

use crate::shared::entities::definitions::weapons::weapons_table::{CrosshairConfig, CrosshairStyle, weapon_crosshair};

const DEFAULT_CROSSHAIR: CrosshairConfig = CrosshairConfig {
    style: CrosshairStyle::Cross,
    base_gap: 4.0,
    line_length: 6.0,
    line_width: 2.0,
};

const SIZE: f64 = 32.0;
const ALPHA: f32 = 0.9;
const HIT_FLASH_DURATION: f64 = 0.2;
const HEAD_HIT_FLASH_DURATION: f64 = 0.3;
const OUTLINE: [f32; 4] = [0.0, 0.0, 0.0, 0.85];

/// `Crosshair`.
#[derive(Debug, Default)]
pub struct Crosshair {
    /// Seconds of hit flash left.
    flash: f64,
}

/// What a frame's reticle depends on.
#[derive(Debug, Clone, Copy)]
pub struct CrosshairFrame<'a> {
    pub weapon: &'a str,
    /// Horizontal speed, units a second.
    pub speed: f64,
    pub out_of_ammo: bool,
    pub dt: f64,
}

impl Crosshair {
    /// `notifyHit`: one of our shots landed.
    pub fn notify_hit(&mut self, head: bool) {
        self.flash = if head {
            HEAD_HIT_FLASH_DURATION
        } else {
            HIT_FLASH_DURATION
        };
    }

    /// `update`, drawn into `hud` at the centre of a `w` by `h` frame.
    pub fn draw(&mut self, hud: &mut Hud2d, frame: &CrosshairFrame<'_>, w: f32, h: f32) {
        self.flash = (self.flash - frame.dt).max(0.0);
        let config = weapon_crosshair(frame.weapon).unwrap_or(DEFAULT_CROSSHAIR);
        let color = if self.flash > 0.0 {
            [1.0, 0.0, 0.0, ALPHA]
        } else {
            [0.0, 1.0, 0.0, ALPHA]
        };
        let movement = (frame.speed / 400.0).min(1.0) * 8.0;
        let gap = (config.base_gap + movement).round() as f32;
        let scale = SIZE / 24.0;
        let len = (config.line_length * scale).round() as f32;
        let lw = (config.line_width * scale).round().max(1.0) as f32;
        // The canvas is 2 * size, centred; its centre is a whole pixel.
        let (cx, cy) = ((w / 2.0).round(), (h / 2.0).round());
        if frame.out_of_ammo {
            big_x(hud, cx, cy);
            return;
        }
        match config.style {
            CrosshairStyle::Dot => dot(hud, cx, cy, lw, color),
            CrosshairStyle::Cross => cross(hud, cx, cy, gap, len, lw, color),
            CrosshairStyle::CrossDot => {
                cross(hud, cx, cy, gap, len, lw, color);
                dot(hud, cx, cy, lw, color);
            }
            CrosshairStyle::Circle => {
                ring(hud, cx, cy, (gap + 3.0).max(3.0), lw + 2.0, OUTLINE);
                ring(hud, cx, cy, (gap + 3.0).max(3.0), lw, color);
                dot(hud, cx, cy, lw, color);
            }
        }
    }
}

/// A rectangle over a 1-pixel dark border.
fn outlined_rect(hud: &mut Hud2d, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    hud.rect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, OUTLINE);
    hud.rect(x, y, w, h, color);
}

fn dot(hud: &mut Hud2d, cx: f32, cy: f32, size: f32, color: [f32; 4]) {
    let half = (size / 2.0).floor().max(1.0);
    outlined_rect(hud, cx - half, cy - half, half * 2.0, half * 2.0, color);
}

fn cross(hud: &mut Hud2d, cx: f32, cy: f32, gap: f32, length: f32, width: f32, color: [f32; 4]) {
    let half = (width / 2.0).floor().max(1.0);
    outlined_rect(hud, cx - half, cy - gap - length, width, length, color);
    outlined_rect(hud, cx - half, cy + gap, width, length, color);
    outlined_rect(hud, cx - gap - length, cy - half, length, width, color);
    outlined_rect(hud, cx + gap, cy - half, length, width, color);
}

/// A stroked circle as a band of quads.
fn ring(hud: &mut Hud2d, cx: f32, cy: f32, radius: f32, width: f32, color: [f32; 4]) {
    const SEGMENTS: usize = 32;
    let (r0, r1) = (radius - width / 2.0, radius + width / 2.0);
    for i in 0..SEGMENTS {
        let a0 = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let p = |r: f32, a: f32| [cx + r * a.cos(), cy + r * a.sin()];
        hud.polygon(&[p(r0, a0), p(r1, a0), p(r1, a1), p(r0, a1)], &[color; 4]);
    }
}

/// A thick line from `a` to `b` with round ends left square.
fn line(hud: &mut Hud2d, a: [f32; 2], b: [f32; 2], width: f32, color: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy).max(1e-6);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    // Stretch along the line by half a width, as a round cap reaches.
    let (ex, ey) = (dx / len * width / 2.0, dy / len * width / 2.0);
    hud.polygon(
        &[
            [a[0] - ex + nx, a[1] - ey + ny],
            [b[0] + ex + nx, b[1] + ey + ny],
            [b[0] + ex - nx, b[1] + ey - ny],
            [a[0] - ex - nx, a[1] - ey - ny],
        ],
        &[color; 4],
    );
}

/// `drawBigX`: out of ammo.
fn big_x(hud: &mut Hud2d, cx: f32, cy: f32) {
    let size = SIZE as f32;
    let reach = size * 0.7;
    let gap = (size * 0.18).max(3.0);
    let w = (size / 8.0).round().max(3.0);
    let strokes = [
        ([cx - reach, cy - reach], [cx - gap, cy - gap]),
        ([cx + gap, cy + gap], [cx + reach, cy + reach]),
        ([cx + reach, cy - reach], [cx + gap, cy - gap]),
        ([cx - gap, cy + gap], [cx - reach, cy + reach]),
    ];
    for (a, b) in strokes {
        line(hud, a, b, w + 4.0, OUTLINE);
    }
    let red = [1.0, 40.0 / 255.0, 40.0 / 255.0, ALPHA.max(0.9)];
    for (a, b) in strokes {
        line(hud, a, b, w, red);
    }
}
