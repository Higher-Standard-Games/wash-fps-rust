//! wash-fps `client/settings/FPSSettingsView.ts` and its pages
//! (`ControlsPage.ts`, `GameplayPage.ts`, `VideoPage.ts`, `AudioPage.ts`):
//! the same fields, ranges, steps and words, drawn with egui. Every field
//! reads and writes a cvar, which saves itself.
//!
//! Not yet: the key binding board (it comes with the input system's
//! bindings), the team colour and voice pickers, and the video page's
//! effects, which land with milestone 11's renderer work.

use shagger::core::cvar::{CvarType, with_cvars};
use shagger::ui::egui_layer::egui;

use super::settings_model::{SETTINGS_PAGES, SettingsPageId, reset_page};

/// `formatPercent`.
fn format_percent(v: f64) -> String {
    format!("{}%", (v * 100.0).round())
}

/// `FPSSettingsView`.
#[derive(Debug)]
pub struct FpsSettingsView {
    pub page: SettingsPageId,
}

impl Default for FpsSettingsView {
    fn default() -> Self {
        Self {
            page: SettingsPageId::Controls,
        }
    }
}

/// A field's name, left in a fixed column.
fn field_label(ui: &mut egui::Ui, name: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(160.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(160.0);
            ui.label(name);
        },
    );
}

/// `slider(host, parent, { name, help, min, max, step, format, get, set })`.
fn slider(
    ui: &mut egui::Ui,
    name: &str,
    help: Option<&str>,
    cvar: &str,
    (min, max, step): (f64, f64, f64),
    format: fn(f64) -> String,
    enabled: bool,
) {
    let Some(mut value) = with_cvars(|c| c.exists(cvar).then(|| c.get_number(cvar))) else {
        return;
    };
    ui.horizontal(|ui| {
        field_label(ui, name);
        let response = ui.add_enabled(
            enabled,
            egui::Slider::new(&mut value, min..=max)
                .step_by(step)
                .custom_formatter(move |v, _| format(v)),
        );
        let response = match help {
            Some(h) => response.on_hover_text(h),
            None => response,
        };
        if response.changed() {
            with_cvars(|c| c.set(cvar, value));
        }
    });
}

/// `seg(...)` with Off and On, for a cvar stored as a boolean or as 0 and 1.
fn toggle(ui: &mut egui::Ui, name: &str, help: Option<&str>, cvar: &str) {
    let Some((mut on, kind)) = with_cvars(|c| c.get_cvar(cvar).map(|v| (c.get_bool(cvar), v.kind))) else {
        return;
    };
    ui.horizontal(|ui| {
        field_label(ui, name);
        let before = on;
        let off = ui.selectable_value(&mut on, false, "Off");
        let on_key = ui.selectable_value(&mut on, true, "On");
        if let Some(h) = help {
            off.on_hover_text(h);
            on_key.on_hover_text(h);
        }
        if on != before {
            with_cvars(|c| {
                if kind == CvarType::Bool {
                    c.set(cvar, on)
                } else {
                    c.set(cvar, i32::from(on))
                }
            });
        }
    });
}

/// A hex colour field (`RRGGBB`, as the crosshair cvars hold it).
fn hex_field(ui: &mut egui::Ui, name: &str, cvar: &str) {
    let Some(text) = with_cvars(|c| c.exists(cvar).then(|| c.get_string(cvar))) else {
        return;
    };
    let channel = |i: usize| {
        text.get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0)
    };
    let mut rgb = [channel(0), channel(2), channel(4)];
    ui.horizontal(|ui| {
        field_label(ui, name);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            let hex = format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
            with_cvars(|c| c.set(cvar, hex));
        }
        ui.weak(&text);
    });
}

fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).inner_margin(12.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(title).heading());
        body(ui);
    });
    ui.add_space(8.0);
}

impl FpsSettingsView {
    /// The page list, for the rail.
    pub fn page_nav(&mut self, ui: &mut egui::Ui) {
        for (id, title, hint) in SETTINGS_PAGES {
            let text = format!("{title}  ·  {hint}");
            ui.selectable_value(&mut self.page, id, text);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let title = SETTINGS_PAGES
                .iter()
                .find(|(id, ..)| *id == self.page)
                .map_or("", |p| p.1);
            ui.heading(title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Reset page").clicked() {
                    with_cvars(|c| reset_page(c, self.page));
                }
            });
        });
        egui::ScrollArea::vertical().show(ui, |ui| match self.page {
            SettingsPageId::Controls => controls(ui),
            SettingsPageId::Gameplay => gameplay(ui),
            SettingsPageId::Video => video(ui),
            SettingsPageId::Audio => audio(ui),
        });
    }
}

fn plain(v: f64) -> String {
    shagger::shared::js::js_number_to_string((v * 100.0).round() / 100.0)
}

/// `ControlsPage`'s mouse feel card.
fn controls(ui: &mut egui::Ui) {
    card(ui, "Mouse feel", |ui| {
        slider(ui, "Sensitivity", None, "m_sensitivity", (0.1, 10.0, 0.1), plain, true);
        slider(
            ui,
            "While aiming",
            Some("Share of your sensitivity while you hold Aim"),
            "cl_ads_sensitivity",
            (0.1, 1.0, 0.05),
            format_percent,
            true,
        );
        toggle(
            ui,
            "Always run",
            Some("On: you run all the time, and holding Sprint walks"),
            "cl_autorun",
        );
    });
}

/// `GameplayPage`: the crosshair, the view and the HUD's extras.
fn gameplay(ui: &mut egui::Ui) {
    card(ui, "Crosshair", |ui| {
        toggle(ui, "Crosshair", None, "cg_crosshair");
        let on = with_cvars(|c| c.get_number("cg_crosshair") > 0.0);
        slider(ui, "Size", None, "cg_crosshairSize", (8.0, 64.0, 1.0), plain, on);
        slider(
            ui,
            "Opacity",
            None,
            "cg_crosshairAlpha",
            (0.1, 1.0, 0.05),
            format_percent,
            on,
        );
        hex_field(ui, "Colour", "cg_crosshairColor");
        hex_field(ui, "Hit colour", "cg_crosshairDmgColor");
        toggle(
            ui,
            "Spread",
            Some("The gap opens as you move and closes as you aim"),
            "cg_crosshairSpread",
        );
    });
    card(ui, "View", |ui| {
        slider(
            ui,
            "Field of view",
            Some("How wide you see. Wider shows more, but things look smaller"),
            "cl_fov",
            (80.0, 110.0, 1.0),
            |v| format!("{v}°"),
            true,
        );
        toggle(
            ui,
            "FPS counter",
            Some("Frames per second in the top-right corner"),
            "show_fps",
        );
        toggle(
            ui,
            "Movement speed",
            Some("Your speed at the bottom of the screen"),
            "cl_showspeed",
        );
        toggle(
            ui,
            "Hints",
            Some(
                "Tips about movement, weapons and items, above your weapons. Shown in warmup and for the first 2 minutes of a match",
            ),
            "cl_hints",
        );
        toggle(
            ui,
            "Clock counts up",
            Some("The match clock shows time played instead of time left"),
            "cl_invertClock",
        );
        toggle(
            ui,
            "Switch on pickup",
            Some("Switch to a weapon when you pick it up"),
            "cl_autoswitch",
        );
    });
}

fn video(ui: &mut egui::Ui) {
    card(ui, "Quality", |ui| {
        ui.label("The video effects arrive with the renderer's effects (milestone 11).");
    });
}

/// `AudioPage`.
fn audio(ui: &mut egui::Ui) {
    for (title, enabled, volume) in [
        ("Music", "s_musicenabled", "s_volume"),
        ("Effects", "s_sfxenabled", "s_sfxvolume"),
    ] {
        card(ui, title, |ui| {
            let percent = with_cvars(|c| (c.get_number(volume) * 100.0).round());
            ui.label(egui::RichText::new(format!("{percent}%")).size(40.0).strong());
            toggle(ui, "Sound", None, enabled);
            slider(ui, "Volume", None, volume, (0.0, 1.0, 0.05), format_percent, true);
        });
    }
    card(ui, "Mix", |ui| {
        for (name, cvar) in [
            ("Effects", "cg_volume_effects"),
            ("Players", "cg_volume_players"),
            ("Hit sound", "cg_volume_hitsound"),
            ("Announcer", "cg_volume_announcer"),
        ] {
            slider(ui, name, None, cvar, (0.0, 1.0, 0.05), format_percent, true);
        }
    });
}
