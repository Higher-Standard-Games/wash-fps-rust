//! wash-fps `client/settings/FPSSettingsView.ts` and its pages
//! (`ControlsPage.ts`, `GameplayPage.ts`, `VideoPage.ts`, `AudioPage.ts`):
//! the same fields, ranges, steps and words, drawn with egui. Every field
//! reads and writes a cvar, which saves itself.
//!
//! The video settings are the renderer's own variables; what they do to the
//! picture lands with milestone 11's renderer work.

use shagger::core::cvar::{CvarType, with_cvars};
use shagger::ui::egui_layer::egui;

use shagger::input::input_system::{InputSystem, with_input};

use super::controls_page::{ControlsPage, hsl};
use super::settings_model::{
    BOOST_CHOICES, CROSSHAIR_COLORS, CvarChoice, FrameCost, GODRAY_CHOICES, HIT_COLORS, OUTLINE_CHOICES,
    SETTINGS_PAGES, SettingsPageId, Side, TEAM_COLORS, VIDEO_PRESETS, VIDEO_SECTIONS, VideoSection, apply_choice,
    color_blind_side_color, current_choice, load_level, reset_page, reset_side_colors, section_load, set_side_color,
    side_color, side_colors_are_default,
};
use crate::client::fps_player_audio::{PLAYER_VOICE_SETS, PlayerVoiceSide, player_voice_set, set_player_voice_set};

/// `formatPercent`.
fn format_percent(v: f64) -> String {
    format!("{}%", (v * 100.0).round())
}

/// `FPSSettingsView`.
#[derive(Debug)]
pub struct FpsSettingsView {
    pub page: SettingsPageId,
    pub controls: ControlsPage,
    /// The video section open (`openSection`).
    pub video_section: Option<VideoSection>,
    /// The colour-blind team colour notice, with its undo.
    color_notice: bool,
}

impl Default for FpsSettingsView {
    fn default() -> Self {
        Self::open(SettingsPageId::Controls)
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

pub(super) fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).inner_margin(12.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(title).heading());
        body(ui);
    });
    ui.add_space(8.0);
}

impl FpsSettingsView {
    /// The screen on `page`, with the bindings as saved (`open`).
    #[must_use]
    pub fn open(page: SettingsPageId) -> Self {
        with_input(InputSystem::reload_bindings_from_storage);
        Self {
            page,
            controls: ControlsPage::default(),
            video_section: None,
            color_notice: false,
        }
    }

    /// The page list, for the rail.
    pub fn page_nav(&mut self, ui: &mut egui::Ui) {
        for (id, title, hint) in SETTINGS_PAGES {
            let text = format!("{title}  ·  {hint}");
            if ui.selectable_value(&mut self.page, id, text).changed() {
                self.controls.deactivate();
            }
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
                    with_cvars(|c| with_input(|i| reset_page(c, i, self.page)));
                }
            });
        });
        egui::ScrollArea::vertical().show(ui, |ui| match self.page {
            SettingsPageId::Controls => {
                self.controls.ui(ui);
                controls(ui);
            }
            SettingsPageId::Gameplay => gameplay(ui),
            SettingsPageId::Video => self.video(ui),
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

/// `seg(...)` over a cvar's numeric values.
fn seg_values(
    ui: &mut egui::Ui,
    name: &str,
    help: Option<&str>,
    cvar: &str,
    choices: &[(f64, &str)],
    enabled: bool,
    cost: Option<FrameCost>,
) {
    let Some(current) = with_cvars(|c| c.exists(cvar).then(|| c.get_number(cvar))) else {
        return;
    };
    ui.horizontal(|ui| {
        field_label(ui, name);
        ui.add_enabled_ui(enabled, |ui| {
            for (value, label) in choices {
                let r = ui.selectable_label((current - value).abs() < 1e-6, *label);
                let r = match help {
                    Some(h) => r.on_hover_text(h),
                    None => r,
                };
                if r.clicked() {
                    with_cvars(|c| c.set(cvar, *value));
                }
            }
        });
        if let Some(cost) = cost {
            cost_pips(ui, cost);
        }
    });
}

/// `seg(...)` over choices that each set several cvars.
fn seg_choices(ui: &mut egui::Ui, name: &str, help: Option<&str>, choices: &[CvarChoice], cost: Option<FrameCost>) {
    let current = with_cvars(|c| current_choice(c, choices).map(|c| c.id));
    ui.horizontal(|ui| {
        field_label(ui, name);
        for choice in choices {
            let r = ui.selectable_label(current == Some(choice.id), choice.label);
            let r = match help {
                Some(h) => r.on_hover_text(h),
                None => r,
            };
            if r.clicked() {
                with_cvars(|c| apply_choice(c, choice));
            }
        }
        if let Some(cost) = cost {
            cost_pips(ui, cost);
        }
    });
}

/// `costPips`: three pips, as many lit as the cost.
fn cost_pips(ui: &mut egui::Ui, cost: FrameCost) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 10.0), egui::Sense::hover());
    for i in 0..3u8 {
        let x = rect.left() + f32::from(i) * 10.0;
        let pip = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), egui::vec2(7.0, 7.0));
        let color = if i < cost.pips() {
            egui::Color32::from_rgb(255, 196, 51)
        } else {
            egui::Color32::from_gray(70)
        };
        ui.painter().rect_filled(pip, 2.0, color);
    }
}

/// `swatches(...)`: colour keys, one of them current.
fn swatches(
    ui: &mut egui::Ui,
    name: &str,
    help: Option<&str>,
    choices: &[(egui::Color32, &str)],
    current: Option<usize>,
    blocked: Option<usize>,
    enabled: bool,
) -> Option<usize> {
    let mut picked = None;
    ui.horizontal(|ui| {
        field_label(ui, name);
        ui.add_enabled_ui(enabled, |ui| {
            for (i, (color, label)) in choices.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
                let is_blocked = blocked == Some(i);
                let fill = if is_blocked { color.gamma_multiply(0.3) } else { *color };
                ui.painter().rect_filled(rect.shrink(3.0), 5.0, fill);
                if current == Some(i) {
                    ui.painter().rect_stroke(
                        rect,
                        6.0,
                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                        egui::StrokeKind::Inside,
                    );
                }
                let tip = match (is_blocked, help) {
                    (true, _) => format!("{label}: the other side has it"),
                    (false, Some(h)) => format!("{label}. {h}"),
                    (false, None) => (*label).to_owned(),
                };
                if response.on_hover_text(tip).clicked() && !is_blocked {
                    picked = Some(i);
                }
            }
        });
    });
    picked
}

fn hex_color32(hex: &str) -> egui::Color32 {
    let channel = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0)
    };
    egui::Color32::from_rgb(channel(0), channel(2), channel(4))
}

/// A crosshair colour row (`swatches` over `CROSSHAIR_COLORS` or `HIT_COLORS`).
fn hex_swatches(ui: &mut egui::Ui, name: &str, help: Option<&str>, cvar: &str, colors: &[(&str, &str)], enabled: bool) {
    let current = with_cvars(|c| c.get_string(cvar)).trim().to_uppercase();
    let choices: Vec<(egui::Color32, &str)> = colors.iter().map(|(hex, label)| (hex_color32(hex), *label)).collect();
    let at = colors.iter().position(|(hex, _)| *hex == current);
    if let Some(i) = swatches(ui, name, help, &choices, at, None, enabled) {
        with_cvars(|c| c.set(cvar, colors[i].0));
    }
}

/// `GameplayPage`: who is drawn in which colour, the crosshair, and the view.
/// The previews over a map slice are not drawn.
fn gameplay(ui: &mut egui::Ui) {
    card(ui, "Who's who · your side, and everyone you can shoot", |ui| {
        let choices: Vec<(egui::Color32, &str)> = TEAM_COLORS
            .iter()
            .map(|t| (hsl(t.hsl[0], t.hsl[1], t.hsl[2], 1.0), t.label))
            .collect();
        for (side, voice_side, name, help) in [
            (Side::Ally, PlayerVoiceSide::Ally, "Your side", None),
            (
                Side::Enemy,
                PlayerVoiceSide::Enemy,
                "Enemies",
                Some("In free-for-all, that's everyone else"),
            ),
        ] {
            let other = match side {
                Side::Ally => Side::Enemy,
                Side::Enemy => Side::Ally,
            };
            let index = |s| with_cvars(|c| side_color(c, s)).and_then(|t| TEAM_COLORS.iter().position(|o| o == t));
            if let Some(i) = swatches(ui, name, help, &choices, index(side), index(other), true) {
                with_cvars(|c| set_side_color(c, side, &TEAM_COLORS[i]));
            }
            ui.horizontal(|ui| {
                field_label(ui, "Sounds");
                let current = player_voice_set(voice_side);
                for voice in PLAYER_VOICE_SETS {
                    let label = format!("{}{}", voice[..1].to_uppercase(), &voice[1..]);
                    if ui.selectable_label(current == voice, label).clicked() {
                        set_player_voice_set(voice_side, voice);
                    }
                }
            });
        }
        seg_choices(ui, "Outline", None, &OUTLINE_CHOICES, None);
        seg_choices(
            ui,
            "Brighter players",
            Some("Makes players stand out in dark corners"),
            &BOOST_CHOICES,
            None,
        );
    });
    card(ui, "Crosshair", |ui| {
        let on_off = [(0.0, "Off"), (1.0, "On")];
        seg_values(ui, "Crosshair", None, "cg_crosshair", &on_off, true, None);
        seg_values(
            ui,
            "Opens up when moving",
            Some("Tightens again while you aim"),
            "cg_crosshairSpread",
            &on_off,
            true,
            None,
        );
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
        hex_swatches(ui, "Colour", None, "cg_crosshairColor", &CROSSHAIR_COLORS, on);
        hex_swatches(ui, "When you hit", None, "cg_crosshairDmgColor", &HIT_COLORS, on);
    });
    card(ui, "View & info", |ui| {
        slider(
            ui,
            "Field of view",
            Some("How wide you see. Wider shows more, but things look smaller"),
            "cl_fov",
            (80.0, 110.0, 1.0),
            |v| format!("{v}°"),
            true,
        );
        let on_off = [(0.0, "Off"), (1.0, "On")];
        for (cvar, name, help) in [
            ("show_fps", "FPS counter", "Frames per second in the top-right corner"),
            (
                "cl_showspeed",
                "Movement speed",
                "Your speed at the bottom of the screen",
            ),
            (
                "cl_hints",
                "Hints",
                "Tips about movement, weapons and items, above your weapons. Shown in warmup and for the first 2 minutes of a match",
            ),
        ] {
            seg_values(ui, name, Some(help), cvar, &on_off, true, None);
        }
        seg_values(
            ui,
            "Match clock",
            Some("Count the match time down to the end, or up from the start"),
            "cl_invertClock",
            &[(0.0, "Time left"), (1.0, "Time played")],
            true,
            None,
        );
        seg_values(
            ui,
            "Switch to picked-up weapons",
            Some("Take out a weapon you walk over. Off keeps what you hold, so a blade fight stays a blade fight"),
            "cl_autoswitch",
            &on_off,
            true,
            None,
        );
        seg_values(
            ui,
            "Connection info",
            Some("Ping and packet loss in a corner of the screen"),
            "cl_netgraph",
            &[(0.0, "Off"), (1.0, "Numbers"), (2.0, "Graph")],
            true,
            None,
        );
    });
}

impl FpsSettingsView {
    /// `VideoPage`: three presets for what costs frames, then the settings in
    /// five sections, one open at a time. Fullscreen is the window's own
    /// (Alt+Enter is not wired yet), so the screen section has no switch.
    fn video(&mut self, ui: &mut egui::Ui) {
        let current = with_cvars(|c| current_choice(c, &VIDEO_PRESETS.map(|p| p.0)).map(|c| c.id));
        let width = ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0 - 22.0).max(120.0);
        ui.horizontal(|ui| {
            for (preset, line, bars) in &VIDEO_PRESETS {
                let selected = current == Some(preset.id);
                let response = egui::Frame::group(ui.style())
                    .fill(if selected {
                        egui::Color32::from_rgb(90, 70, 20)
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.strong(preset.label);
                            ui.add(egui::Label::new(egui::RichText::new(*line).small()).wrap());
                            cost_pips(
                                ui,
                                match bars {
                                    1 => FrameCost::Low,
                                    2 => FrameCost::Mid,
                                    _ => FrameCost::High,
                                },
                            );
                        });
                    })
                    .response
                    .interact(egui::Sense::click());
                if response.clicked() {
                    with_cvars(|c| apply_choice(c, preset));
                }
            }
        });
        let now = with_cvars(|c| current_choice(c, &VIDEO_PRESETS.map(|p| p.0)).map(|c| c.label));
        ui.label(match now {
            Some(label) => format!("Using {label}"),
            None => "Using your own mix · pick a preset to start over from it".to_owned(),
        });
        ui.horizontal_wrapped(|ui| {
            for (id, name) in VIDEO_SECTIONS {
                let level = load_level(with_cvars(|c| section_load(c, id)));
                let open = self.video_section == Some(id);
                ui.vertical(|ui| {
                    if ui.selectable_label(open, name).clicked() {
                        self.video_section = if open { None } else { Some(id) };
                    }
                    ui.horizontal(|ui| {
                        cost_pips(ui, level);
                        ui.small(level.word());
                    });
                });
            }
        });
        let Some(section) = self.video_section else { return };
        let on_off = [(0.0, "Off"), (1.0, "On")];
        card(
            ui,
            VIDEO_SECTIONS.iter().find(|s| s.0 == section).map_or("", |s| s.1),
            |ui| match section {
                VideoSection::Screen => {
                    for (cvar, name) in [("r_exposure", "Brightness"), ("r_contrast", "Contrast")] {
                        slider(ui, name, None, cvar, (0.5, 2.0, 0.05), format_percent, true);
                    }
                }
                VideoSection::Effects => {
                    slider(
                        ui,
                        "Bloom",
                        Some("Soft light around bright things"),
                        "r_bloom",
                        (0.0, 3.0, 0.1),
                        plain,
                        true,
                    );
                    seg_values(
                        ui,
                        "Reflections",
                        Some("Shiny floors and metal"),
                        "r_ssr",
                        &on_off,
                        true,
                        Some(FrameCost::High),
                    );
                    seg_choices(
                        ui,
                        "Light shafts",
                        Some("Sunbeams, on maps that have a sun"),
                        &GODRAY_CHOICES,
                        Some(FrameCost::Mid),
                    );
                    seg_values(
                        ui,
                        "Soft halos",
                        Some("Lamp halos fade into walls instead of ending in a line"),
                        "r_softhalos",
                        &on_off,
                        true,
                        Some(FrameCost::Low),
                    );
                    slider(
                        ui,
                        "Glow",
                        Some("Signs, strips and energy shine"),
                        "r_glow",
                        (0.0, 2.0, 0.05),
                        plain,
                        true,
                    );
                    let ssr = with_cvars(|c| c.get_number("r_ssr") > 0.0);
                    slider(
                        ui,
                        "Reflection strength",
                        None,
                        "r_ssr_strength",
                        (0.0, 2.0, 0.05),
                        plain,
                        ssr,
                    );
                    seg_values(
                        ui,
                        "Comic outlines",
                        Some("Inked edges on the world"),
                        "r_cel_outlines",
                        &on_off,
                        true,
                        Some(FrameCost::Low),
                    );
                }
                VideoSection::World => {
                    seg_values(
                        ui,
                        "Water",
                        None,
                        "r_water_quality",
                        &[(1.0, "Low"), (2.0, "Medium"), (3.0, "High")],
                        true,
                        Some(FrameCost::Mid),
                    );
                    let high = with_cvars(|c| c.get_number("r_water_quality") >= 3.0);
                    seg_values(
                        ui,
                        "Players in water",
                        Some("Reflections of nearby players. High water only"),
                        "r_water_actors",
                        &on_off,
                        high,
                        Some(FrameCost::Low),
                    );
                    seg_values(
                        ui,
                        "Glowing panel light",
                        Some("Light cast from the map's glowing panels. Next map"),
                        "r_arealights",
                        &on_off,
                        true,
                        Some(FrameCost::Mid),
                    );
                    slider(
                        ui,
                        "Underwater wobble",
                        Some("How much the view warps under water"),
                        "r_underwater_distortion",
                        (0.0, 1.0, 0.05),
                        format_percent,
                        true,
                    );
                }
                VideoSection::Textures => {
                    seg_values(
                        ui,
                        "Sharpness",
                        Some("Keeps floors crisp at an angle"),
                        "r_anisotropy",
                        &[(1.0, "1×"), (2.0, "2×"), (4.0, "4×"), (8.0, "8×"), (16.0, "16×")],
                        true,
                        Some(FrameCost::Low),
                    );
                    seg_values(
                        ui,
                        "Detailed surfaces",
                        Some("Richer metal, stone and wear on map surfaces. Next map"),
                        "r_bsp_pbr",
                        &on_off,
                        true,
                        Some(FrameCost::Mid),
                    );
                }
                VideoSection::Advanced => {
                    seg_values(
                        ui,
                        "Map texture detail",
                        Some(
                            "Softer or flat-color walls and floors. Lighting and transparent effects stay intact. Next map",
                        ),
                        "r_picmip",
                        &[
                            (0.0, "Full"),
                            (1.0, "Soft"),
                            (2.0, "Softer"),
                            (4.0, "Blurred"),
                            (8.0, "Flat color"),
                        ],
                        true,
                        None,
                    );
                    self.color_correction(ui);
                }
            },
        );
    }

    /// `buildColorCorrection`: accessibility, not frame cost; no preset
    /// touches it.
    fn color_correction(&mut self, ui: &mut egui::Ui) {
        let current = with_cvars(|c| c.get_number("r_colorcorrection"));
        let kinds = [
            (0.0, "Off", "Colors as the maps were made."),
            (
                1.0,
                "Red-weak (protanopia)",
                "Reds look dark and close to greens. Their difference moves into blue and brightness.",
            ),
            (
                2.0,
                "Green-weak (deuteranopia)",
                "The most common kind: reds and greens look alike. Their difference moves into blue and brightness.",
            ),
            (
                3.0,
                "Blue-yellow (tritanopia)",
                "Blues look like greens, yellows like pinks. Their difference moves into red and green.",
            ),
        ];
        ui.horizontal(|ui| {
            field_label(ui, "Color blindness");
            let label = kinds.iter().find(|k| k.0 == current).map_or("Off", |k| k.1);
            egui::ComboBox::from_id_salt("colorcorrection")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (value, name, help) in kinds {
                        if ui
                            .selectable_label(current == value, name)
                            .on_hover_text(help)
                            .clicked()
                        {
                            with_cvars(|c| {
                                let turning_on = c.get_number("r_colorcorrection") == 0.0 && value > 0.0;
                                c.set("r_colorcorrection", value);
                                // The default team colours are a pair colour-blind
                                // eyes mix up; swap them, but only when the player
                                // never picked their own, and say so.
                                if turning_on && side_colors_are_default(c) {
                                    set_side_color(c, Side::Ally, color_blind_side_color(Side::Ally));
                                    set_side_color(c, Side::Enemy, color_blind_side_color(Side::Enemy));
                                    self.color_notice = true;
                                } else if value == 0.0 {
                                    self.color_notice = false;
                                }
                            });
                        }
                    }
                })
                .response
                .on_hover_text("Not sure which? Try each and keep the one where colors are easiest to tell apart.");
            cost_pips(ui, FrameCost::Low);
        });
        if self.color_notice {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Team colors changed to {} for your side and {} for enemies, which stay easier to tell apart.",
                    color_blind_side_color(Side::Ally).label,
                    color_blind_side_color(Side::Enemy).label
                ));
                if ui.button("Undo").clicked() {
                    with_cvars(reset_side_colors);
                    self.color_notice = false;
                }
            });
        }
        let on = current > 0.0;
        slider(
            ui,
            "Correction strength",
            None,
            "r_colorcorrection_strength",
            (0.0, 1.0, 0.05),
            format_percent,
            on,
        );
    }
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
