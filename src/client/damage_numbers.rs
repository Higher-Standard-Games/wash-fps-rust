//! Port of wash-fps `client/DamageNumbers.ts`: Quake Live / Warfork style
//! floating damage numbers over a victim the player on screen hurt.
//!
//! The TypeScript draws each number into its own canvas texture
//! (`bold 60px Arial`, a 4 px black stroke under the fill) on a 20 unit
//! billboard. Here the digits are drawn once into an atlas, from the
//! client's own Rajdhani Bold (Arial is not shipped), and each digit is a
//! camera-facing quad at the place the canvas would have put it.

use shagger::rendering::effect_quad_batch::{EffectFrame, EffectInstance, EffectQuadBatch};
use shagger::rendering::glam::DVec3;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;
use shagger::rendering::world_renderer::Rgba8Image;
use shagger::ui::font::Font;

pub const LIFETIME: f64 = 1.5;
pub const FADE_START: f64 = 1.0;
pub const FLOAT_SPEED: f64 = 80.0;
pub const HEAD_OFFSET: f64 = 36.0;
pub const PLANE_SIZE: f64 = 20.0;
pub const FONT_SIZE: f32 = 60.0;
pub const TEXTURE_SIZE: f64 = 128.0;
pub const RANDOM_OFFSET_X: f64 = 15.0;
pub const MAX_ACTIVE: usize = 32;
/// The canvas stroke's `lineWidth`: half of it lies outside the glyph.
const STROKE_WIDTH: f32 = 4.0;
const HIGH_DAMAGE_THRESHOLD: u16 = 50;

/// `COLOR_NORMAL`, `COLOR_HIGH` and `COLOR_HEAD` (#FFFFFF, #FFDD00,
/// #FF7A1A), display space.
pub const COLOR_NORMAL: [f32; 3] = [1.0, 1.0, 1.0];
pub const COLOR_HIGH: [f32; 3] = [1.0, 221.0 / 255.0, 0.0];
pub const COLOR_HEAD: [f32; 3] = [1.0, 122.0 / 255.0, 26.0 / 255.0];

/// `damageNumberColor`: a head hit is orange, a big hit yellow, anything
/// else white.
#[must_use]
pub fn damage_number_color(damage: u16, hit_location: Option<&str>) -> [f32; 3] {
    if hit_location == Some("head") {
        return COLOR_HEAD;
    }
    if damage >= HIGH_DAMAGE_THRESHOLD {
        COLOR_HIGH
    } else {
        COLOR_NORMAL
    }
}

/// One floating number.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageNumber {
    pub text: String,
    pub color: [f32; 3],
    pub start: DVec3,
    pub age: f64,
}

impl DamageNumber {
    /// Where it floats now.
    #[must_use]
    pub fn position(&self) -> DVec3 {
        self.start + DVec3::new(0.0, FLOAT_SPEED * self.age, 0.0)
    }

    /// `mesh.visibility`: whole until `FADE_START`, then fading out.
    #[must_use]
    pub fn visibility(&self) -> f64 {
        if self.age > FADE_START {
            1.0 - (self.age - FADE_START) / (LIFETIME - FADE_START)
        } else {
            1.0
        }
    }
}

/// `DamageNumbers`: the live numbers (the pool's active ones).
#[derive(Debug, Clone)]
pub struct DamageNumbers {
    numbers: Vec<DamageNumber>,
    /// `Math.random`'s stand-in (xorshift64*); nothing depends on its draws.
    seed: u64,
}

impl Default for DamageNumbers {
    fn default() -> Self {
        Self::new()
    }
}

impl DamageNumbers {
    #[must_use]
    pub fn new() -> Self {
        Self {
            numbers: Vec::new(),
            seed: 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn random(&mut self) -> f64 {
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `spawn`: a number over the victim at `at`; `hit_location` tints a
    /// head hit.
    pub fn spawn(&mut self, at: DVec3, damage: u16, hit_location: Option<&str>) {
        if self.numbers.len() >= MAX_ACTIVE {
            return;
        }
        let x = at.x + (self.random() - 0.5) * RANDOM_OFFSET_X;
        let z = at.z + (self.random() - 0.5) * RANDOM_OFFSET_X;
        self.numbers.push(DamageNumber {
            text: damage.to_string(),
            color: damage_number_color(damage, hit_location),
            start: DVec3::new(x, at.y + HEAD_OFFSET, z),
            age: 0.0,
        });
    }

    /// `update`: float, fade and expire.
    pub fn update(&mut self, dt: f64) {
        for number in &mut self.numbers {
            number.age += dt;
        }
        self.numbers.retain(|n| n.age < LIFETIME);
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.numbers.clear();
    }

    #[must_use]
    pub fn numbers(&self) -> &[DamageNumber] {
        &self.numbers
    }
}

/// One digit in the atlas: its cell, where the cell sits against the pen
/// (texture pixels, y up from the baseline) and the pen's advance.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DigitCell {
    pub uv: [f32; 4],
    pub size: [f32; 2],
    pub left: f32,
    pub top: f32,
    pub advance: f32,
}

/// The digits drawn once: red is the fill's coverage, green the stroke's.
#[derive(Debug, Clone)]
pub struct DigitAtlas {
    pub image: Rgba8Image,
    pub digits: [DigitCell; 10],
    /// `textBaseline = "middle"`: how far the baseline sits below the
    /// texture's centre.
    pub baseline_drop: f32,
}

impl DigitAtlas {
    /// Draws `0` to `9` at `FONT_SIZE` with the stroke dilated around them.
    pub fn build(font: &Font) -> Self {
        let pad = (STROKE_WIDTH / 2.0).ceil() as i32 + 1;
        let radius = STROKE_WIDTH / 2.0;
        let glyphs: Vec<_> = ('0'..='9')
            .map(|ch| {
                let glyph = font.glyph(ch);
                (
                    font.rasterize_char(ch, FONT_SIZE).unwrap_or_default(),
                    font.advance(glyph, FONT_SIZE),
                )
            })
            .collect();
        let width: i32 = glyphs.iter().map(|(g, _)| g.width as i32 + 2 * pad).sum();
        let height: i32 = glyphs.iter().map(|(g, _)| g.height as i32 + 2 * pad).max().unwrap_or(1);
        let (w, h) = (width.max(1) as usize, height.max(1) as usize);
        let mut pixels = vec![0u8; w * h * 4];
        for p in pixels.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        let mut digits = [DigitCell::default(); 10];
        let mut x0 = 0i32;
        let r = radius.ceil() as i32;
        for (i, (glyph, advance)) in glyphs.iter().enumerate() {
            let cw = glyph.width as i32 + 2 * pad;
            let ch = glyph.height as i32 + 2 * pad;
            let coverage = |gx: i32, gy: i32| -> f32 {
                if gx < 0 || gy < 0 || gx >= glyph.width as i32 || gy >= glyph.height as i32 {
                    return 0.0;
                }
                f32::from(glyph.coverage[(gy * glyph.width as i32 + gx) as usize]) / 255.0
            };
            for cy in 0..ch {
                for cx in 0..cw {
                    let (gx, gy) = (cx - pad, cy - pad);
                    let fill = coverage(gx, gy);
                    // The stroke: coverage within half its width, eased at the rim.
                    let mut stroke = 0.0f32;
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let d = ((dx * dx + dy * dy) as f32).sqrt();
                            let weight = (radius + 0.5 - d).clamp(0.0, 1.0);
                            stroke = stroke.max(coverage(gx + dx, gy + dy) * weight);
                        }
                    }
                    let o = ((cy as usize) * w + (x0 + cx) as usize) * 4;
                    pixels[o] = (fill * 255.0).round() as u8;
                    pixels[o + 1] = (stroke * 255.0).round() as u8;
                }
            }
            digits[i] = DigitCell {
                uv: [
                    x0 as f32 / w as f32,
                    0.0,
                    (x0 + cw) as f32 / w as f32,
                    ch as f32 / h as f32,
                ],
                size: [cw as f32, ch as f32],
                left: (glyph.left - pad) as f32,
                top: (glyph.top + pad) as f32,
                advance: *advance,
            };
            x0 += cw;
        }
        let (ascent, descent) = font.vertical(FONT_SIZE);
        Self {
            image: Rgba8Image {
                width: w as u32,
                height: h as u32,
                pixels,
            },
            digits,
            baseline_drop: (ascent - descent) / 2.0,
        }
    }

    /// Lays `text` out centred on the texture: each digit's cell centre
    /// (texture pixels from the centre, y up), half size and uv rectangle.
    #[must_use]
    pub fn layout(&self, text: &str) -> Vec<([f32; 2], [f32; 2], [f32; 4])> {
        let cells: Vec<DigitCell> = text
            .chars()
            .filter_map(|c| c.to_digit(10).map(|d| self.digits[d as usize]))
            .collect();
        let total: f32 = cells.iter().map(|c| c.advance).sum();
        let mut pen = -total / 2.0;
        let mut out = Vec::with_capacity(cells.len());
        for cell in cells {
            let centre = [
                pen + cell.left + cell.size[0] / 2.0,
                -self.baseline_drop + cell.top - cell.size[1] / 2.0,
            ];
            out.push((centre, [cell.size[0] / 2.0, cell.size[1] / 2.0], cell.uv));
            pen += cell.advance;
        }
        out
    }
}

/// The numbers' shader after the shared quad half.
const DAMAGE_NUMBERS_WGSL: &str = include_str!("damage_numbers.wgsl");

/// Up to five digits a number (a `u16`), every number at once.
const MAX_DIGITS: usize = MAX_ACTIVE * 5;

/// The numbers' quads.
pub struct DamageNumberRenderer {
    atlas: DigitAtlas,
    batch: EffectQuadBatch,
    instances: Vec<EffectInstance>,
}

impl DamageNumberRenderer {
    #[must_use]
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat, font: &Font) -> Self {
        let atlas = DigitAtlas::build(font);
        let batch = EffectQuadBatch::textured(
            gpu,
            color_format,
            "damage-numbers",
            DAMAGE_NUMBERS_WGSL,
            MAX_DIGITS,
            &atlas.image,
            true,
        );
        Self {
            atlas,
            batch,
            instances: Vec::with_capacity(MAX_DIGITS),
        }
    }

    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame, numbers: &DamageNumbers) {
        self.instances.clear();
        let scale = (PLANE_SIZE / TEXTURE_SIZE) as f32;
        for number in numbers.numbers() {
            let p = number.position();
            let visibility = number.visibility() as f32;
            for (centre, half, uv) in self.atlas.layout(&number.text) {
                if self.instances.len() >= MAX_DIGITS {
                    break;
                }
                let mut instance = [0.0f32; 20];
                instance[..4].copy_from_slice(&[p.x as f32, p.y as f32, p.z as f32, visibility]);
                instance[4..8].copy_from_slice(&[centre[0], centre[1], half[0], half[1]]);
                instance[8..12].copy_from_slice(&uv);
                instance[12..16].copy_from_slice(&[number.color[0], number.color[1], number.color[2], scale]);
                self.instances.push(instance);
            }
        }
        self.batch.prepare(gpu, frame, &self.instances, true);
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.batch.draw(pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_hit() {
        assert_eq!(damage_number_color(10, None), COLOR_NORMAL);
        assert_eq!(damage_number_color(50, Some("body")), COLOR_HIGH);
        assert_eq!(damage_number_color(5, Some("head")), COLOR_HEAD);
    }

    #[test]
    fn numbers_float_fade_and_expire() {
        let mut numbers = DamageNumbers::new();
        numbers.spawn(DVec3::new(100.0, 0.0, -50.0), 42, None);
        let n = &numbers.numbers()[0];
        assert!((n.start.x - 100.0).abs() <= RANDOM_OFFSET_X / 2.0);
        assert!((n.start.z + 50.0).abs() <= RANDOM_OFFSET_X / 2.0);
        assert_eq!(n.start.y, HEAD_OFFSET);
        numbers.update(0.5);
        let n = &numbers.numbers()[0];
        assert_eq!(n.position().y, HEAD_OFFSET + 40.0);
        assert_eq!(n.visibility(), 1.0);
        numbers.update(0.75);
        assert!((numbers.numbers()[0].visibility() - 0.5).abs() < 1e-9);
        numbers.update(0.25);
        assert!(numbers.numbers().is_empty());
    }

    #[test]
    fn at_most_thirty_two_at_once() {
        let mut numbers = DamageNumbers::new();
        for _ in 0..40 {
            numbers.spawn(DVec3::ZERO, 7, None);
        }
        assert_eq!(numbers.numbers().len(), MAX_ACTIVE);
    }

    #[test]
    fn digits_lay_out_centred() {
        let font = Font::from_bytes(include_bytes!("../../assets/fonts/Rajdhani-Bold.ttf").to_vec()).unwrap();
        let atlas = DigitAtlas::build(&font);
        assert!(atlas.digits.iter().all(|d| d.advance > 0.0 && d.size[0] > 0.0));
        let one = atlas.layout("100");
        assert_eq!(one.len(), 3);
        // Symmetric about the centre, to within the glyphs' bearings.
        let left = one[0].0[0] - one[0].1[0];
        let right = one[2].0[0] + one[2].1[0];
        assert!((left + right).abs() < 12.0, "{left} {right}");
        // A 60 px number fits the 128 px texture.
        assert!(right - left < TEXTURE_SIZE as f32);
        // Some pixels are fill, more are stroke.
        let fill = atlas
            .image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 128)
            .count();
        let stroke = atlas
            .image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[1] > 128)
            .count();
        assert!(fill > 0 && stroke > fill);
    }
}
