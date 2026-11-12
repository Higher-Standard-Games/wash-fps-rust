//! wash-fps `client/settings/VideoPage.ts`: four presets for what costs
//! frames, then the settings themselves in five sections, closed until asked
//! for and one open at a time. Every setting is one of the renderer's own
//! variables; the pips are estimates of what each pass costs.
//!
//! In a match the page is locked: video settings change in the lobby.
//! The browser's Fullscreen switch and its renderer pick (WebGPU or WebGL2)
//! are the page's own; the native window has Alt+Enter and one renderer.

use shagger::core::cvar::with_cvars;
use shagger::ui::components::lobby_twin::{cream, gold, hsl, hsla, ink};
use shagger::ui::components::raised_key::{KeyLook, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Rect, Shape, Stroke, pos2, vec2};

use super::fps_settings_view::{cvar_seg, cvar_slider, flag, on_off};
use super::settings_fields::{
    Head, Range, body, columns, cost_pips, cut_panel, format_percent, line, notice, plain, select_menu, wrapped,
};
use super::settings_model::{
    FrameCost, GODRAY_CHOICES, Side, VIDEO_PRESETS, VIDEO_SECTIONS, VideoSection, apply_choice, color_blind_side_color,
    current_choice, load_level, reset_side_colors, section_load, set_side_color, side_colors_are_default,
};
use crate::client::washoi_style::display_family;

/// A field of a section: drawn by its index.
type Field = fn(&mut VideoPage, &mut egui::Ui);

/// `VideoPage`.
#[derive(Debug, Default)]
pub struct VideoPage {
    open: Option<VideoSection>,
    /// The colour-blind team colour notice, with its undo.
    color_notice: bool,
}

const ON_OFF: [(f64, &str); 2] = [(0.0, "Off"), (1.0, "On")];

impl VideoPage {
    /// Opens the section named `name` (`Screen`, `Effects`, ...), if any.
    pub fn open_section(&mut self, name: &str) {
        if let Some((id, _)) = VIDEO_SECTIONS.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)) {
            self.open = Some(*id);
        }
    }

    /// Esc closes an open section before it does anything else.
    pub fn close_section(&mut self) -> bool {
        self.open.take().is_some()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, locked: bool) {
        if locked {
            let width = ui.available_width();
            let painter = ui.painter().clone();
            let g = wrapped(
                &painter,
                "Video settings change in the lobby. Leave the match to adjust them; fullscreen still works with Alt+Enter.",
                egui::FontId::new(15.0, body()),
                hsla(45.0, 100.0, 95.0, 0.85),
                width - 32.0,
            );
            let (r, _) = ui.allocate_exact_size(vec2(width, g.size().y + 24.0), egui::Sense::hover());
            painter.rect_filled(r, 0.0, hsla(45.0, 100.0, 50.0, 0.08));
            painter.rect_filled(Rect::from_min_size(r.min, vec2(3.0, r.height())), 0.0, gold());
            painter.galley(r.min + vec2(16.0, 12.0), g, cream());
            ui.add_space(16.0);
            ui.disable();
            ui.set_opacity(0.4);
        }
        self.presets(ui);
        ui.add_space(8.0);
        status_bar(ui);
        ui.add_space(24.0);
        self.sections(ui);
        if let Some(section) = self.open {
            ui.add_space(18.0);
            self.panel(ui, section);
        }
    }

    /// `fps-set-presets`: four cream keys; the one in use is pressed in gold.
    fn presets(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let per_row = if width < 1000.0 { 2 } else { 4 };
        let current = with_cvars(|c| current_choice(c, &VIDEO_PRESETS.map(|p| p.0)).map(|c| c.id));
        let gap = 24.0;
        let key_w = (width - gap * (per_row as f32 - 1.0)) / per_row as f32;
        let key_h = 150.0;
        let top = ui.cursor().min;
        for (i, (preset, text, bars)) in VIDEO_PRESETS.iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let r = Rect::from_min_size(
                pos2(
                    top.x + col as f32 * (key_w + gap),
                    top.y + row as f32 * (key_h + 8.4 + gap),
                ),
                vec2(key_w, key_h),
            );
            let on = current == Some(preset.id);
            let look = KeyLook {
                depth: 8.4,
                cut: 0.026,
                shadow: hsla(250.0, 70.0, 2.0, 0.6),
                ..if on {
                    KeyLook::gold()
                } else {
                    KeyLook {
                        face: vec![
                            (0.0, hsl(45.0, 100.0, 97.0)),
                            (0.62, hsl(42.0, 58.0, 89.0)),
                            (1.0, hsl(40.0, 40.0, 80.0)),
                        ],
                        side: [hsl(36.0, 24.0, 56.0), hsl(30.0, 20.0, 38.0)],
                        ..KeyLook::cream()
                    }
                }
            };
            let key = RaisedKey::new(("video-preset", preset.id), r, look.clone());
            let face = if on {
                key.rest_press(-0.5).hover(look.clone(), -0.5).show(ui)
            } else {
                key.show(ui)
            };
            if face.clicked {
                with_cvars(|c| apply_choice(c, preset));
            }
            let painter = ui.painter().clone();
            let f = face.rect;
            let name_size = (width * 0.021).clamp(24.0, 36.0);
            let name = line(
                &painter,
                &preset.label.to_uppercase(),
                name_size,
                display_family(),
                ink(),
                0.0,
            );
            let nh = name.size().y;
            painter.galley(pos2(f.left() + 30.0, f.top() + 18.0), name, ink());
            let sub = hsla(250.0, 45.0, 8.0, 0.62);
            let g = wrapped(&painter, text, egui::FontId::new(15.0, body()), sub, f.width() - 56.0);
            painter.galley(pos2(f.left() + 30.0, f.top() + 18.0 + nh + 10.0), g, sub);
            // The meter: three slanted bars.
            for k in 0..3u8 {
                let x = f.left() + 30.0 + f32::from(k) * 26.0;
                let y = f.bottom() - 20.0 - 8.0;
                let fill = if k < *bars { ink() } else { hsla(250.0, 45.0, 8.0, 0.18) };
                painter.add(Shape::convex_polygon(
                    vec![
                        pos2(x + 4.4, y),
                        pos2(x + 22.0, y),
                        pos2(x + 17.6, y + 8.0),
                        pos2(x, y + 8.0),
                    ],
                    fill,
                    Stroke::NONE,
                ));
            }
        }
        let rows = VIDEO_PRESETS.len().div_ceil(per_row);
        ui.allocate_rect(
            Rect::from_min_size(top, vec2(width, rows as f32 * (key_h + 8.4 + gap) - gap + 8.4)),
            egui::Sense::hover(),
        );
    }

    /// `fps-set-sections`: five dark keys, each saying how heavy it is set.
    fn sections(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let per_row = if width < 1000.0 { 3 } else { 5 };
        let gap = 22.0;
        let key_w = (width - gap * (per_row as f32 - 1.0)) / per_row as f32;
        let key_h = 88.0;
        let top = ui.cursor().min;
        for (i, (id, name)) in VIDEO_SECTIONS.iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let r = Rect::from_min_size(
                pos2(
                    top.x + col as f32 * (key_w + gap),
                    top.y + row as f32 * (key_h + 10.6 + gap),
                ),
                vec2(key_w, key_h),
            );
            let open = self.open == Some(*id);
            let look = KeyLook {
                depth: 6.6,
                cut: 0.032,
                shadow: hsla(250.0, 70.0, 2.0, 0.6),
                ..if open {
                    KeyLook::gold()
                } else {
                    KeyLook {
                        face: vec![(0.0, hsl(250.0, 26.0, 25.0)), (1.0, hsl(250.0, 30.0, 16.0))],
                        side: [hsl(250.0, 36.0, 11.0), hsl(250.0, 40.0, 6.0)],
                        text: cream(),
                        sub: hsla(45.0, 100.0, 95.0, 0.58),
                        ..KeyLook::gold()
                    }
                }
            };
            let key = RaisedKey::new(("video-section", *name), r, look.clone());
            let face = if open {
                key.rest_press(0.45).hover(look.clone(), 0.45).show(ui)
            } else {
                key.show(ui)
            };
            if face.clicked {
                self.open = if open { None } else { Some(*id) };
            }
            let painter = ui.painter().clone();
            let f = face.rect;
            let size = (width * 0.012 * 1.3).clamp(16.0, 21.0);
            let title = line(&painter, &name.to_uppercase(), size, display_family(), look.text, 0.08);
            painter.galley(pos2(f.left() + 26.0, f.top() + 14.0), title, look.text);
            // The chevron, turned over when open.
            let c = pos2(f.right() - 24.0, f.top() + 14.0 + size / 2.0);
            let (a, b) = if open { (-1.0, 2.0) } else { (1.0, -2.0) };
            painter.add(Shape::convex_polygon(
                vec![c + vec2(-5.0, b), c + vec2(5.0, b), c + vec2(0.0, b + 6.0 * a)],
                look.sub,
                Stroke::NONE,
            ));
            let level = load_level(with_cvars(|c| section_load(c, *id)));
            let y = f.bottom() - 14.0 - 9.0;
            for k in 0..3u8 {
                let x = f.left() + 26.0 + f32::from(k) * 21.0;
                let lit = k < level.pips();
                let fill = if open {
                    if lit { ink() } else { hsla(250.0, 45.0, 8.0, 0.16) }
                } else if lit {
                    cost_color(level)
                } else {
                    hsla(45.0, 100.0, 95.0, 0.14)
                };
                painter.add(Shape::convex_polygon(
                    vec![
                        pos2(x + 4.5, y),
                        pos2(x + 18.0, y),
                        pos2(x + 13.5, y + 9.0),
                        pos2(x, y + 9.0),
                    ],
                    fill,
                    Stroke::NONE,
                ));
            }
            let word = line(&painter, &level.word().to_uppercase(), 12.0, body(), look.sub, 0.18);
            painter.galley(
                pos2(f.left() + 26.0 + 3.0 * 21.0 + 10.0, y + 4.5 - word.size().y / 2.0),
                word,
                look.sub,
            );
        }
        let rows = VIDEO_SECTIONS.len().div_ceil(per_row);
        ui.allocate_rect(
            Rect::from_min_size(top, vec2(width, rows as f32 * (key_h + 10.6 + gap) - gap + 10.6)),
            egui::Sense::hover(),
        );
    }

    /// `.fps-set-section-panel`: the open section's fields, three across.
    fn panel(&mut self, ui: &mut egui::Ui, section: VideoSection) {
        let width = ui.available_width();
        let top = ui.cursor().min;
        let bg = ui.painter().add(Shape::Noop);
        let fields: &[Field] = match section {
            VideoSection::Screen => &[
                |_, ui| {
                    cvar_slider(
                        ui,
                        "Brightness",
                        "r_exposure",
                        Range::new(0.5, 2.0, 0.05, format_percent),
                        None,
                        true,
                    )
                },
                |_, ui| {
                    cvar_slider(
                        ui,
                        "Contrast",
                        "r_contrast",
                        Range::new(0.5, 2.0, 0.05, format_percent),
                        None,
                        true,
                    )
                },
                |_, ui| {
                    cvar_slider_cost(
                        ui,
                        "Render resolution",
                        "r_renderscale",
                        Range::new(0.25, 1.0, 0.05, format_percent),
                        Some("Below 100% the world draws at fewer pixels and looks softer; the HUD stays sharp"),
                    );
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Antialiasing").cost(Some(2)).tag(Some("Restart")),
                        "r_msaa",
                        Some("Smooth edges on geometry"),
                        true,
                    );
                },
            ],
            VideoSection::Effects => &[
                |_, ui| {
                    cvar_slider(
                        ui,
                        "Bloom",
                        "r_bloom",
                        Range::new(0.0, 3.0, 0.1, plain),
                        Some("Soft light around bright things"),
                        true,
                    );
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Reflections").cost(Some(3)),
                        "r_ssr",
                        Some("Shiny floors and metal"),
                        true,
                    )
                },
                |_, ui| {
                    let current = with_cvars(|c| super::settings_model::godray_choice(c).id);
                    let labels: Vec<&str> = GODRAY_CHOICES.iter().map(|c| c.label).collect();
                    let at = GODRAY_CHOICES.iter().position(|c| c.id == current);
                    if let Some(i) = super::settings_fields::seg(
                        ui,
                        Head::new("Light shafts").cost(Some(2)),
                        &labels,
                        at,
                        Some("Sunbeams, on maps that have a sun"),
                        true,
                    ) {
                        with_cvars(|c| apply_choice(c, &GODRAY_CHOICES[i]));
                    }
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Soft halos").cost(Some(1)),
                        "r_softhalos",
                        Some("Lamp halos fade into walls instead of ending in a line"),
                        true,
                    );
                },
                |_, ui| {
                    cvar_slider(
                        ui,
                        "Glow",
                        "r_glow",
                        Range::new(0.0, 2.0, 0.05, plain),
                        Some("Signs, strips and energy shine"),
                        true,
                    )
                },
                |_, ui| {
                    let ssr = flag("r_ssr");
                    cvar_slider(
                        ui,
                        "Reflection strength",
                        "r_ssr_strength",
                        Range::new(0.0, 2.0, 0.05, plain),
                        None,
                        ssr,
                    );
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Comic outlines").cost(Some(1)),
                        "r_cel_outlines",
                        Some("Inked edges on the world"),
                        true,
                    )
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Dynamic lights").cost(Some(2)).tag(Some("Next map")),
                        "r_dynamiclight",
                        Some("Light from shots, rockets, explosions and muzzle flashes"),
                        true,
                    );
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Gibs").cost(Some(1)),
                        "r_gibs",
                        Some("The shards a player breaks into"),
                        true,
                    )
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Wall marks").cost(Some(1)),
                        "r_marks",
                        Some("Holes and scorches where shots hit"),
                        true,
                    )
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Teleport afterimage").cost(Some(1)),
                        "r_teleport_ghost",
                        Some("The silhouette left where someone teleported from"),
                        true,
                    );
                },
            ],
            VideoSection::World => &[
                |_, ui| {
                    cvar_seg(
                        ui,
                        Head::new("Water").cost(Some(2)),
                        "r_water_quality",
                        &[(1.0, "Low"), (2.0, "Medium"), (3.0, "High")],
                        None,
                        true,
                    );
                },
                |_, ui| {
                    let high = with_cvars(|c| c.get_number("r_water_quality") >= 3.0);
                    cvar_seg(
                        ui,
                        Head::new("Players in water").cost(Some(1)),
                        "r_water_actors",
                        &ON_OFF,
                        Some("Reflections of nearby players. High water only"),
                        high,
                    );
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Glowing panel light").cost(Some(2)).tag(Some("Next map")),
                        "r_arealights",
                        Some("Light cast from the map's glowing panels"),
                        true,
                    );
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Fog").cost(Some(1)),
                        "r_fog",
                        Some("The map's own haze and distance fog"),
                        true,
                    )
                },
                |_, ui| {
                    cvar_slider(
                        ui,
                        "Underwater wobble",
                        "r_underwater_distortion",
                        Range::new(0.0, 1.0, 0.05, format_percent),
                        Some("How much the view warps under water"),
                        true,
                    );
                },
            ],
            VideoSection::Textures => &[
                |_, ui| {
                    cvar_seg(
                        ui,
                        Head::new("Sharpness").cost(Some(1)),
                        "r_anisotropy",
                        &[(1.0, "1×"), (2.0, "2×"), (4.0, "4×"), (8.0, "8×"), (16.0, "16×")],
                        Some("Keeps floors crisp at an angle"),
                        true,
                    );
                },
                |_, ui| {
                    on_off(
                        ui,
                        Head::new("Detailed surfaces").cost(Some(2)).tag(Some("Next map")),
                        "r_bsp_pbr",
                        Some("Richer metal, stone and wear on map surfaces"),
                        true,
                    );
                },
            ],
            VideoSection::Advanced => &[
                |_, ui| {
                    cvar_seg(
                        ui,
                        Head::new("Map texture detail").tag(Some("Next map")),
                        "r_picmip",
                        &[
                            (0.0, "Full"),
                            (1.0, "Soft"),
                            (2.0, "Softer"),
                            (4.0, "Blurred"),
                            (8.0, "Flat color"),
                        ],
                        Some("Softer or flat-color walls and floors. Lighting and transparent effects stay intact."),
                        true,
                    );
                },
                |page, ui| page.color_correction(ui),
                |_, ui| {
                    let on = with_cvars(|c| c.get_number("r_colorcorrection") > 0.0);
                    cvar_slider(
                        ui,
                        "Correction strength",
                        "r_colorcorrection_strength",
                        Range::new(0.0, 1.0, 0.05, format_percent),
                        None,
                        on,
                    );
                },
            ],
        };
        let per_row = if width < 1000.0 { 2 } else { 3 };
        let mut inner = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    top + vec2(32.0, 26.0),
                    vec2(width - 64.0, f32::INFINITY),
                ))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let mut i = 0;
        while i < fields.len() {
            let n = per_row.min(fields.len() - i);
            columns(&mut inner, &vec![1.0; per_row], 34.0, |ui, k| {
                if k < n {
                    fields[i + k](self, ui);
                }
            });
            i += n;
            if i < fields.len() {
                inner.add_space(20.0);
            }
        }
        if section == VideoSection::Advanced && self.color_notice {
            inner.add_space(20.0);
            let text = format!(
                "Team colors changed to {} for your side and {} for enemies, which stay easier to tell apart. \
                 Change them under Gameplay, Who's who.",
                color_blind_side_color(Side::Ally).label,
                color_blind_side_color(Side::Enemy).label
            );
            if notice(&mut inner, &text) {
                with_cvars(reset_side_colors);
                self.color_notice = false;
            }
        }
        let bottom = inner.min_rect().bottom() + 28.0;
        let rect = Rect::from_min_max(top, pos2(top.x + width, bottom));
        let shape = [
            rect.left_top(),
            pos2(rect.right() - 20.0, rect.top()),
            pos2(rect.right(), rect.top() + 20.0),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
        ui.painter().set(
            bg,
            Shape::Vec(vec![
                super::settings_fields::mesh_polygon(
                    &shape,
                    &[
                        (0.0, hsla(250.0, 40.0, 11.0, 0.78)),
                        (1.0, hsla(250.0, 44.0, 7.0, 0.86)),
                    ],
                ),
                Shape::closed_line(shape.to_vec(), Stroke::new(1.5, hsla(46.0, 100.0, 60.0, 0.55))),
            ]),
        );
        let _ = cut_panel;
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    /// `buildColorCorrection`: accessibility, not frame cost; no preset
    /// touches it.
    fn color_correction(&mut self, ui: &mut egui::Ui) {
        if !with_cvars(|c| c.exists("r_colorcorrection")) {
            return;
        }
        let kinds = [
            (0.0, "Off"),
            (1.0, "Red-weak (protanopia)"),
            (2.0, "Green-weak (deuteranopia)"),
            (3.0, "Blue-yellow (tritanopia)"),
        ];
        let current = with_cvars(|c| c.get_number("r_colorcorrection"));
        let width = ui.available_width();
        let painter = ui.painter().clone();
        let name = line(
            &painter,
            "COLOR BLINDNESS",
            15.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.82),
            0.14,
        );
        let nh = name.size().y;
        let nw = name.size().x;
        let at = ui.cursor().min;
        painter.galley(at, name, cream());
        cost_pips(&painter, pos2(at.x + nw + 9.0, at.y + nh / 2.0 - 4.0), 1, 1.0);
        ui.add_space(nh + 9.0);
        let labels: Vec<&str> = kinds.iter().map(|k| k.1).collect();
        let index = kinds.iter().position(|k| k.0 == current).unwrap_or(0);
        if let Some(i) = select_menu(ui, "colorcorrection", &labels, index, width) {
            let value = kinds[i].0;
            with_cvars(|c| {
                let turning_on = c.get_number("r_colorcorrection") == 0.0 && value > 0.0;
                c.set("r_colorcorrection", value);
                // The default team colours are a pair colour-blind eyes mix
                // up; swap them, but only when the player never picked
                // their own, and say so.
                if turning_on && side_colors_are_default(c) {
                    set_side_color(c, Side::Ally, color_blind_side_color(Side::Ally));
                    set_side_color(c, Side::Enemy, color_blind_side_color(Side::Enemy));
                    self.color_notice = true;
                } else if value == 0.0 {
                    self.color_notice = false;
                }
            });
        }
        ui.add_space(9.0);
        let g = wrapped(
            &painter,
            "Not sure which? Try each and keep the one where colors are easiest to tell apart.",
            egui::FontId::new(13.0, body()),
            hsla(45.0, 100.0, 95.0, 0.45),
            width,
        );
        let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
        painter.galley(r.min, g, cream());
    }
}

/// A slider with frame-cost pips in its head.
fn cvar_slider_cost(ui: &mut egui::Ui, name: &str, cvar: &str, range: Range, help: Option<&str>) {
    // The slider draws the head; the pips go after its name.
    let at = ui.cursor().min;
    cvar_slider(ui, name, cvar, range, help, true);
    if with_cvars(|c| c.exists(cvar)) {
        let painter = ui.painter().clone();
        let w = line(&painter, &name.to_uppercase(), 15.0, body(), cream(), 0.14).size();
        cost_pips(
            &painter,
            pos2(at.x + w.x + 9.0, at.y + w.y / 2.0 - 4.0),
            FrameCost::High.pips(),
            1.0,
        );
    }
}

fn cost_color(level: FrameCost) -> egui::Color32 {
    match level {
        FrameCost::Low => hsl(96.0, 85.0, 55.0),
        FrameCost::Mid => hsl(40.0, 100.0, 58.0),
        FrameCost::High => hsl(356.0, 90.0, 62.0),
    }
}

/// `fps-set-video-bar`: which preset is in use, and the cost legend.
fn status_bar(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let painter = ui.painter().clone();
    let now = with_cvars(|c| current_choice(c, &VIDEO_PRESETS.map(|p| p.0)).map(|c| c.label));
    let dim = hsla(45.0, 100.0, 95.0, 0.55);
    let mut job = egui::text::LayoutJob::default();
    let fmt = |color| egui::TextFormat {
        font_id: egui::FontId::new(14.0, body()),
        color,
        extra_letter_spacing: 1.4,
        ..Default::default()
    };
    job.append("USING ", 0.0, fmt(dim));
    job.append(
        &now.unwrap_or("your own mix").to_uppercase(),
        0.0,
        fmt(super::settings_fields::gold_text()),
    );
    if now.is_none() {
        job.append(" · PICK A PRESET TO START OVER FROM IT", 0.0, fmt(dim));
    }
    let status = painter.layout_job(job);
    let (r, _) = ui.allocate_exact_size(vec2(width, status.size().y.max(14.0)), egui::Sense::hover());
    painter.galley(r.min, status, cream());
    // The legend, right-aligned.
    let mut x = r.right();
    for level in [FrameCost::High, FrameCost::Mid, FrameCost::Low] {
        let word = line(
            &painter,
            &level.word().to_uppercase(),
            12.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.75),
            0.16,
        );
        x -= word.size().x;
        let y = r.center().y - word.size().y / 2.0;
        painter.galley(pos2(x, y), word, cream());
        x -= 7.0 + 28.0;
        cost_pips(&painter, pos2(x, r.center().y - 4.0), level.pips(), 1.0);
        x -= 16.0;
    }
    let legend = line(
        &painter,
        "FRAME-RATE COST",
        12.0,
        body(),
        hsla(45.0, 100.0, 95.0, 0.5),
        0.16,
    );
    x -= legend.size().x;
    painter.galley(pos2(x, r.center().y - legend.size().y / 2.0), legend, cream());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_an_open_section_first() {
        let mut page = VideoPage {
            open: Some(VideoSection::World),
            ..VideoPage::default()
        };
        assert!(page.close_section());
        assert!(!page.close_section());
    }
}
