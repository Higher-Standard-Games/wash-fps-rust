//! wash-fps `client/styles/Cursors.css`: the gold Washoi cursors (arrow,
//! hand, disabled and text) for the native windows, through shagger's
//! [`cursor_theme`]. The art is the 46 px SVGs (`assets/cursors/*.svg`)
//! drawn at each display scale by `tools/render_cursors.mjs`, with the
//! stylesheet's hotspots.

use shagger::ui::cursor_theme::{self, CursorArt, CursorImage};
use winit::event_loop::ActiveEventLoop;

/// `--ui-cursor*` hotspots at 46 px: arrow, hand, disabled, text.
const HOTSPOTS: [[f32; 2]; 4] = [[4.0, 2.0], [17.0, 1.0], [4.0, 2.0], [21.0, 21.0]];

macro_rules! scale_art {
    ($scale:expr, $tag:literal) => {
        (
            $scale,
            [
                include_bytes!(concat!("../../assets/cursors/arrow-", $tag, ".png")).as_slice(),
                include_bytes!(concat!("../../assets/cursors/hand-", $tag, ".png")).as_slice(),
                include_bytes!(concat!("../../assets/cursors/disabled-", $tag, ".png")).as_slice(),
                include_bytes!(concat!("../../assets/cursors/text-", $tag, ".png")).as_slice(),
            ],
        )
    };
}

/// The PNGs by display scale: arrow, hand, disabled, text.
const ART: [(f32, [&[u8]; 4]); 7] = [
    scale_art!(1.0, "100"),
    scale_art!(1.25, "125"),
    scale_art!(1.5, "150"),
    scale_art!(1.75, "175"),
    scale_art!(2.0, "200"),
    scale_art!(2.5, "250"),
    scale_art!(3.0, "300"),
];

/// One PNG as a cursor, its hotspot scaled with it.
fn image(png: &[u8], hotspot: [f32; 2], scale: f32) -> Option<CursorImage> {
    let rgba = image::load_from_memory(png).ok()?.into_rgba8();
    let (w, h) = rgba.dimensions();
    let size = [u16::try_from(w).ok()?, u16::try_from(h).ok()?];
    let at = |v: f32, max: u16| ((v * scale).round() as u16).min(max.saturating_sub(1));
    Some(CursorImage {
        rgba: rgba.into_raw(),
        size,
        hotspot: [at(hotspot[0], size[0]), at(hotspot[1], size[1])],
    })
}

/// Every scale's art, decoded.
#[must_use]
pub fn art() -> Vec<CursorArt> {
    ART.iter()
        .filter_map(|&(scale, pngs)| {
            let [arrow, hand, disabled, text] = std::array::from_fn(|i| image(pngs[i], HOTSPOTS[i], scale));
            Some(CursorArt {
                scale,
                arrow: arrow?,
                hand: hand?,
                disabled: disabled?,
                text: text?,
            })
        })
        .collect()
}

/// Installs the cursors once the event loop runs; later calls do nothing.
pub fn install(event_loop: &ActiveEventLoop) {
    if !cursor_theme::installed() {
        cursor_theme::install(event_loop, &art());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The art's size in CSS pixels.
    const CSS_SIZE: f32 = 46.0;

    #[test]
    fn every_scale_decodes_at_its_size() {
        let arts = art();
        assert_eq!(arts.len(), ART.len());
        for a in &arts {
            let px = (CSS_SIZE * a.scale).round() as u16;
            for img in [&a.arrow, &a.hand, &a.disabled, &a.text] {
                assert!(img.size[0].abs_diff(px) <= 1, "{} at {}", img.size[0], a.scale);
                assert_eq!(img.rgba.len(), usize::from(img.size[0]) * usize::from(img.size[1]) * 4);
                assert!(img.hotspot[0] < img.size[0] && img.hotspot[1] < img.size[1]);
            }
        }
        assert_eq!(arts[4].text.hotspot, [42, 42]);
        assert_eq!(arts[0].hand.hotspot, [17, 1]);
    }
}
