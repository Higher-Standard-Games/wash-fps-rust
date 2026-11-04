//! The crosshair (wash-fps `client/Crosshair.ts`): the weapon's reticle at
//! the centre, its gap opened by speed, red on a hit, a red cross when the
//! gun is empty. The cvars (`cg_crosshair`, `cg_crosshairSize`,
//! `cg_crosshairColor`, `cg_crosshairAlpha`, `cg_crosshairSpread`,
//! `cg_crosshairDmgColor`) are registered by [`register_crosshair_cvars`].
//!
//! Not yet: aiming down sights and the spread bloom of sustained fire.

use shagger::core::cvar::{CvarOptions, CvarType, Cvars, with_cvars};
use shagger::ui::hud_2d::Hud2d;

use crate::shared::entities::definitions::weapons::weapons_table::{CrosshairConfig, CrosshairStyle, weapon_crosshair};

const DEFAULT_CROSSHAIR: CrosshairConfig = CrosshairConfig {
    style: CrosshairStyle::Cross,
    base_gap: 4.0,
    line_length: 6.0,
    line_width: 2.0,
};

const HIT_FLASH_DURATION: f64 = 0.2;
const HEAD_HIT_FLASH_DURATION: f64 = 0.3;
const OUTLINE: [f32; 4] = [0.0, 0.0, 0.0, 0.85];

/// `Cvar.register("cg_crosshair*", ...)`.
pub fn register_crosshair_cvars(cvars: &mut Cvars) {
    let int = |d: &str, min: f64, max: f64| CvarOptions::describe(d).range(min, max).kind(CvarType::Int);
    let string = |d: &str| CvarOptions::describe(d).kind(CvarType::String);
    cvars.register("cg_crosshair", 1, int("Enable crosshair (0 = off, 1 = on)", 0.0, 1.0));
    cvars.register(
        "cg_crosshairSize",
        32,
        int("Crosshair size in pixels (half-extent)", 8.0, 64.0),
    );
    cvars.register(
        "cg_crosshairColor",
        "00FF00",
        string("Crosshair color (hex RGB without #)"),
    );
    cvars.register(
        "cg_crosshairAlpha",
        0.9,
        CvarOptions::describe("Crosshair opacity")
            .range(0.1, 1.0)
            .kind(CvarType::Float),
    );
    cvars.register(
        "cg_crosshairSpread",
        1,
        int("Enable dynamic crosshair spread (ADS + movement)", 0.0, 1.0),
    );
    cvars.register(
        "cg_crosshairDmgColor",
        "FF0000",
        string("Crosshair color on hit confirmation (hex RGB without #)"),
    );
}

/// `parseInt(hex.substring(i, i + 2), 16)` for each channel, as a colour.
fn hex_color(hex: &str, alpha: f32) -> [f32; 4] {
    let channel = |i: usize| {
        hex.get(i..(i + 2).min(hex.len()))
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .map_or(0.0, |v| f32::from(v) / 255.0)
    };
    [channel(0), channel(2), channel(4), alpha]
}

/// The cvars one frame reads.
struct Settings {
    enabled: bool,
    size: f64,
    color: String,
    alpha: f32,
    spread: bool,
    hit_color: String,
}

fn settings() -> Settings {
    with_cvars(|c| {
        let or = |name: &str, d: f64| if c.exists(name) { c.get_number(name) } else { d };
        let text = |name: &str, d: &str| {
            if c.exists(name) {
                c.get_string(name)
            } else {
                d.to_owned()
            }
        };
        Settings {
            enabled: or("cg_crosshair", 1.0) > 0.0,
            size: or("cg_crosshairSize", 32.0),
            color: text("cg_crosshairColor", "00FF00"),
            alpha: or("cg_crosshairAlpha", 0.9) as f32,
            spread: or("cg_crosshairSpread", 1.0) > 0.0,
            hit_color: text("cg_crosshairDmgColor", "FF0000"),
        }
    })
}

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
        let set = settings();
        if !set.enabled {
            return;
        }
        let config = weapon_crosshair(frame.weapon).unwrap_or(DEFAULT_CROSSHAIR);
        let hex = if self.flash > 0.0 { &set.hit_color } else { &set.color };
        let color = hex_color(hex, set.alpha);
        let mut gap = config.base_gap;
        if set.spread {
            gap += (frame.speed / 400.0).min(1.0) * 8.0;
        }
        let gap = gap.round() as f32;
        let scale = set.size / 24.0;
        let len = (config.line_length * scale).round() as f32;
        let lw = (config.line_width * scale).round().max(1.0) as f32;
        // The canvas is 2 * size, centred; its centre is a whole pixel.
        let (cx, cy) = ((w / 2.0).round(), (h / 2.0).round());
        if frame.out_of_ammo {
            big_x(hud, cx, cy, set.size as f32, set.alpha);
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
        // Soft along the circles, hard where the segments meet.
        hud.shape(None, &[p(r0, a0), p(r1, a0), p(r1, a1), p(r0, a1)], &[color; 4], &|i| {
            i % 2 == 1
        });
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
fn big_x(hud: &mut Hud2d, cx: f32, cy: f32, size: f32, alpha: f32) {
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
    let red = [1.0, 40.0 / 255.0, 40.0 / 255.0, alpha.max(0.9)];
    for (a, b) in strokes {
        line(hud, a, b, w, red);
    }
}
