//! The Esc menu (`FPSPauseMenu`): the gold rail with the logo, the keys and
//! the plate over the match, drawn into a frame. Writes it to
//! `PAUSE_MENU_PNG` when set (over `PAUSE_MENU_BACKDROP`, a PNG of a
//! match, when that is set too), to look at.
//!
//! Needs a GPU adapter, software ones included; it reports a skip without.

use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};
use washoi::client::hud::widgets::pause_menu::{PauseMenu, PauseMenuAction};
use washoi::client::washoi_style::install_style;

#[test]
fn the_esc_menu_is_the_gold_rail() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let (width, height) = std::env::var("PAUSE_MENU_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1280u32, 720u32));
    let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
    install_style(&layer.ctx);
    let backdrop = std::env::var("PAUSE_MENU_BACKDROP")
        .ok()
        .and_then(|p| image::open(p).ok())
        .map(|i| {
            let i = i.to_rgba8();
            let image = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
            layer.ctx.load_texture("backdrop", image, egui::TextureOptions::LINEAR)
        });
    let mut menu = PauseMenu::default();
    menu.settings = true;
    menu.open();
    let mut pixels = Vec::new();
    let mut asked = None;
    // The first frame starts the slide-in; the second is past it.
    for t in [0.0, 1.0] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32),
            )),
            time: Some(t),
            ..Default::default()
        };
        let frame = layer.run_input(input, |ui| {
            if let Some(b) = &backdrop {
                ui.painter().image(
                    b.id(),
                    ui.max_rect(),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            asked = menu.ui(ui.ctx(), "Deathmatch · washoi_dm2", "Player_8186");
        });
        pixels = render_encoded_to_rgba(&gpu, width, height, wgpu::Color::BLACK, |encoder, target| {
            layer.paint(&gpu, encoder, target, [width, height], frame);
        });
    }
    assert_eq!(asked, None::<PauseMenuAction>);
    // The rail is gold down the left edge, the match shows to its right.
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let [r, g, b] = at(10, height - 60);
    assert!(r > 180 && g > 110 && b < 60, "rail colour {r},{g},{b}");
    let [r, g, b] = at(width - 10, height / 2);
    assert!(r < 60 && g < 60 && b < 60, "veil colour {r},{g},{b}");
    if let Ok(path) = std::env::var("PAUSE_MENU_PNG") {
        image::save_buffer(&path, &pixels, width, height, image::ColorType::Rgba8).unwrap();
    }
}
