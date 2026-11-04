//! Washoi's look for the native screens' egui (the region picker, the data
//! sync and the in-match menus): Rajdhani and Orbitron, cream on the night's
//! ink, gold for what is chosen (wash-fps `styles/LobbyPanel.css`, kept from
//! the retired native lobby, `client/LobbyPanel.ts`).

use std::sync::Arc;

use shagger::ui::egui_layer::egui;

use crate::client::hud::washiki;

/// A CSS `hsl()` colour, saturation and lightness in percent.
pub fn hsl(h: f32, s: f32, l: f32) -> egui::Color32 {
    rgba(washiki::hsla(h, s, l, 1.0))
}

fn rgba(c: [f32; 4]) -> egui::Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]))
}

/// The display face's family (Orbitron, `--w-font-display`).
#[must_use]
pub fn display_family() -> egui::FontFamily {
    egui::FontFamily::Name("display".into())
}

/// Washoi's look for egui: Rajdhani for text and Orbitron for titles, cream
/// on the night's ink, gold for what is chosen (`styles/LobbyPanel.css`).
pub fn install_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "rajdhani".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Rajdhani-Bold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "orbitron".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Orbitron-Black.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Proportional,
        vec!["rajdhani".into(), "orbitron".into()],
    );
    fonts
        .families
        .insert(egui::FontFamily::Monospace, vec!["rajdhani".into(), "orbitron".into()]);
    fonts
        .families
        .insert(display_family(), vec!["orbitron".into(), "rajdhani".into()]);
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|style| {
        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, display_family())),
            (TextStyle::Body, FontId::proportional(19.0)),
            (TextStyle::Button, FontId::proportional(19.0)),
            (TextStyle::Small, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(17.0)),
        ]
        .into();
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        let cream = rgba(washiki::cream());
        v.override_text_color = Some(cream);
        v.panel_fill = hsl(252.0, 32.0, 9.0);
        v.window_fill = hsl(252.0, 32.0, 11.0);
        v.extreme_bg_color = hsl(250.0, 40.0, 5.0);
        v.faint_bg_color = hsl(252.0, 30.0, 13.0);
        v.widgets.inactive.weak_bg_fill = hsl(252.0, 28.0, 18.0);
        v.widgets.inactive.bg_fill = hsl(252.0, 28.0, 18.0);
        v.widgets.hovered.weak_bg_fill = hsl(252.0, 30.0, 26.0);
        v.widgets.active.weak_bg_fill = hsl(41.0, 96.0, 50.0);
        v.selection.bg_fill = hsl(41.0, 96.0, 40.0);
        v.selection.stroke = egui::Stroke::new(1.0, cream);
        v.hyperlink_color = hsl(46.0, 100.0, 64.0);
    });
}
