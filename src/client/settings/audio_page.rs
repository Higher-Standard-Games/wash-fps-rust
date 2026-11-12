//! wash-fps `client/settings/AudioPage.ts`: music and effects, each with its
//! volume in large numbers, and the effects' mix: each channel's share of
//! the effects volume.

use shagger::core::cvar::with_cvars;
use shagger::shared::js::js_round;
use shagger::ui::components::lobby_twin::{cream, hsla};
use shagger::ui::egui_layer::egui;

use egui::{Rect, pos2, vec2};

use super::fps_settings_view::{cvar_slider, flag, on_off};
use super::settings_fields::{Head, Range, card, columns, format_percent, line};
use crate::client::washoi_style::display_family;

/// `MIX`: the effects' channels.
const MIX: [(&str, &str); 4] = [
    ("Effects", "cg_volume_effects"),
    ("Players", "cg_volume_players"),
    ("Hit sound", "cg_volume_hitsound"),
    ("Announcer", "cg_volume_announcer"),
];

const VOLUME: Range = Range::new(0.0, 1.0, 0.05, format_percent);

/// `AudioPage`: two cards side by side (one column when narrow), the mix
/// under them; at most 1100px wide.
pub fn ui(ui: &mut egui::Ui) {
    let width = ui.available_width().min(1100.0);
    let two = ui.available_width() > 900.0;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(ui.cursor().min, vec2(width, f32::INFINITY)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui_ = &mut child;
    let pairs = [
        ("Music", "s_musicenabled", "s_volume"),
        ("Effects", "s_sfxenabled", "s_sfxvolume"),
    ];
    if two {
        columns(ui_, &[1.0, 1.0], 20.0, |ui, i| volume_card(ui, pairs[i]));
        ui_.add_space(20.0);
        columns(ui_, &[1.0, 1.0], 20.0, |ui, i| {
            if i == 0 {
                mix(ui);
            }
        });
    } else {
        for pair in pairs {
            volume_card(ui_, pair);
            ui_.add_space(20.0);
        }
        mix(ui_);
    }
    let used = child.min_rect();
    ui.allocate_rect(used, egui::Sense::hover());
}

fn volume_card(ui: &mut egui::Ui, (title, enabled, volume): (&str, &str, &str)) {
    card(ui, title, "", |ui| {
        let on = flag(enabled);
        let percent = with_cvars(|c| js_round(c.get_number(volume) * 100.0));
        // `fps-set-vol__big`: the volume in large numbers, then the fields.
        let painter = ui.painter().clone();
        let color = if on { cream() } else { hsla(45.0, 100.0, 95.0, 0.25) };
        let big = line(&painter, &format!("{percent}"), 64.0, display_family(), color, 0.0);
        let pct = line(
            &painter,
            "%",
            22.0,
            display_family(),
            hsla(45.0, 100.0, 95.0, 0.45),
            0.0,
        );
        let digits = line(&painter, "000", 64.0, display_family(), color, 0.0).size().x * 1.07;
        let big_w = digits + pct.size().x;
        columns(ui, &[big_w, ui.available_width() - big_w - 22.0], 22.0, |ui, i| {
            if i == 0 {
                return;
            }
            on_off(ui, Head::new("Sound"), enabled, None, true);
            ui.add_space(16.0);
            // The big number already says the volume: the slider's head
            // keeps only its `· default`.
            cvar_slider(
                ui,
                "Volume",
                volume,
                Range::new(0.0, 1.0, 0.05, |_| String::new()),
                None,
                true,
            );
            let _ = on;
        });
        let rows = ui.min_rect();
        let top = rows.top() + 30.0 + 14.0;
        let mid = pos2(rows.left(), (top + rows.bottom()) / 2.0);
        let bh = big.size().y;
        let bw = big.size().x;
        painter.galley(pos2(mid.x + digits - bw, mid.y - bh / 2.0), big, color);
        painter.galley(pos2(mid.x + digits + 2.0, mid.y + bh * 0.42 - pct.size().y), pct, color);
    });
}

fn mix(ui: &mut egui::Ui) {
    card(ui, "Mix", "", |ui| {
        for (i, (name, cvar)) in MIX.iter().enumerate() {
            if i > 0 {
                ui.add_space(16.0);
            }
            cvar_slider(ui, name, cvar, VOLUME, None, true);
        }
    });
}
