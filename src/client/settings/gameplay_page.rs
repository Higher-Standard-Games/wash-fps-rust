//! wash-fps `client/settings/GameplayPage.ts`: who is drawn in which colour,
//! the crosshair, and the view. The crosshair has a preview over a dark
//! scene; each side's card shows its colour and outline around the model's
//! name where the browser turns a 3D figure (not drawn here yet).

use shagger::core::cvar::with_cvars;
use shagger::shared::js::js_round;
use shagger::ui::components::lobby_twin::{cream, gradient_polygon, hsl, hsla};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Rect, Stroke, pos2, vec2};

use super::fps_settings_view::{choice_seg, cvar_seg, cvar_slider, flag, on_off};
use super::settings_fields::{
    Head, Range, body, card, columns, field_grid, format_percent, line, plain, select_menu, swatches,
};
use super::settings_model::{
    BOOST_CHOICES, CROSSHAIR_COLORS, HIT_COLORS, OUTLINE_CHOICES, Side, TEAM_COLORS, TeamColor, current_choice,
    set_side_color, side_color,
};
use crate::client::player_model_selection::{
    PLAYER_MODELS, PlayerModelSide, player_model_label, player_model_setting, set_player_model_choice,
};
use crate::client::washoi_style::display_family;

/// A team colour's swatch.
#[must_use]
pub fn team_color(t: &TeamColor) -> Color32 {
    hsl(t.hsl[0], t.hsl[1] * 100.0, t.hsl[2] * 100.0)
}

/// `#RRGGBB` (without the hash) as a colour.
#[must_use]
pub fn hex_color(hex: &str) -> Color32 {
    let channel = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0)
    };
    Color32::from_rgb(channel(0), channel(2), channel(4))
}

/// `GameplayPage`.
#[derive(Debug, Default)]
pub struct GameplayPage {
    /// The hit colour flashes in the preview until then (`hitUntil`).
    hit_until: f64,
}

impl GameplayPage {
    pub fn ui(&mut self, ui: &mut egui::Ui, now: f64) {
        teams(ui);
        ui.add_space(20.0);
        self.crosshair(ui, now);
        ui.add_space(20.0);
        view(ui);
    }

    /// `buildCrosshair`.
    fn crosshair(&mut self, ui: &mut egui::Ui, now: f64) {
        card(ui, "Crosshair", "", |ui| {
            columns(ui, &[0.8, 1.2], 22.0, |ui, i| {
                if i == 0 {
                    self.preview(ui, now);
                    return;
                }
                let on = flag("cg_crosshair");
                field_grid(ui, 6, 2, |ui, k| match k {
                    0 => on_off(ui, Head::new("Crosshair"), "cg_crosshair", None, true),
                    1 => on_off(
                        ui,
                        Head::new("Opens up when moving"),
                        "cg_crosshairSpread",
                        Some("Tightens again while you aim"),
                        true,
                    ),
                    2 => cvar_slider(
                        ui,
                        "Size",
                        "cg_crosshairSize",
                        Range::new(8.0, 64.0, 1.0, plain),
                        None,
                        on,
                    ),
                    3 => cvar_slider(
                        ui,
                        "Opacity",
                        "cg_crosshairAlpha",
                        Range::new(0.1, 1.0, 0.05, format_percent),
                        None,
                        on,
                    ),
                    4 => {
                        hex_swatches(ui, "Colour", "cg_crosshairColor", &CROSSHAIR_COLORS, None, on);
                    }
                    _ => {
                        if hex_swatches(
                            ui,
                            "When you hit",
                            "cg_crosshairDmgColor",
                            &HIT_COLORS,
                            Some("Pick one to see it flash"),
                            on,
                        ) {
                            self.hit_until = now + 0.5;
                        }
                    }
                });
            });
        });
    }

    /// The scene with the reticle drawn as the game draws it, 1.5× larger.
    fn preview(&self, ui: &mut egui::Ui, now: f64) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, width * 9.0 / 16.0), egui::Sense::hover());
        scene(ui, rect, Some("PREVIEW"));
        let (on, size, alpha, color, hit) = with_cvars(|c| {
            (
                c.get_number("cg_crosshair") > 0.0,
                c.get_number("cg_crosshairSize"),
                c.get_number("cg_crosshairAlpha"),
                c.get_string("cg_crosshairColor").trim().to_uppercase(),
                c.get_string("cg_crosshairDmgColor").trim().to_uppercase(),
            )
        });
        if !on {
            return;
        }
        let flashing = now < self.hit_until;
        if flashing {
            ui.ctx().request_repaint();
        }
        let color = hex_color(if flashing { &hit } else { &color });
        let scale = size / 24.0;
        let zoom = 1.5_f32;
        let gap = 4.0 * zoom;
        let length = js_round(6.0 * scale) as f32 * zoom;
        let w = (js_round(2.0 * scale).max(1.0)) as f32 * zoom;
        let c = rect.center();
        let bars = [
            Rect::from_min_size(pos2(c.x - w / 2.0, c.y - gap - length), vec2(w, length)),
            Rect::from_min_size(pos2(c.x - w / 2.0, c.y + gap), vec2(w, length)),
            Rect::from_min_size(pos2(c.x - gap - length, c.y - w / 2.0), vec2(length, w)),
            Rect::from_min_size(pos2(c.x + gap, c.y - w / 2.0), vec2(length, w)),
        ];
        let a = alpha as f32;
        for b in bars {
            ui.painter()
                .rect_filled(b.expand(zoom), 0.0, Color32::from_black_alpha(178).gamma_multiply(a));
        }
        for b in bars {
            ui.painter().rect_filled(b, 0.0, color.gamma_multiply(a));
        }
    }
}

/// `scene(map)`: a dark, lit backdrop with a vignette (the browser lays a
/// map's levelshot under it), and its tag.
pub fn scene(ui: &egui::Ui, rect: Rect, tag: Option<&str>) {
    let painter = ui.painter();
    gradient_polygon(
        painter,
        &[
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ],
        &[(0.0, hsl(250.0, 30.0, 22.0)), (1.0, hsl(250.0, 36.0, 10.0))],
    );
    // The vignette: darker towards the edges.
    for k in 0..6 {
        let k = k as f32;
        painter.rect_stroke(
            rect.shrink(k * 6.0),
            0.0,
            Stroke::new(6.0, hsla(250.0, 50.0, 4.0, 0.09 * (6.0 - k) / 6.0 * 1.5)),
            egui::StrokeKind::Inside,
        );
    }
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.3)),
        egui::StrokeKind::Inside,
    );
    if let Some(tag) = tag {
        let g = line(painter, tag, 10.0, display_family(), hsla(45.0, 100.0, 95.0, 0.7), 0.14);
        let r = Rect::from_min_size(rect.min + vec2(10.0, 10.0), g.size() + vec2(20.0, 10.0));
        painter.rect_filled(r, 0.0, hsla(250.0, 45.0, 5.0, 0.82));
        painter.galley(r.min + vec2(10.0, 5.0), g, cream());
    }
}

/// A crosshair colour row; true when one was picked.
fn hex_swatches(
    ui: &mut egui::Ui,
    name: &str,
    cvar: &str,
    colors: &[(&str, &str)],
    help: Option<&str>,
    enabled: bool,
) -> bool {
    let current = with_cvars(|c| c.get_string(cvar)).trim().to_uppercase();
    let choices: Vec<Color32> = colors.iter().map(|(hex, _)| hex_color(hex)).collect();
    let at = colors.iter().position(|(hex, _)| *hex == current);
    if let Some(i) = swatches(ui, name, &choices, at, None, help, enabled) {
        with_cvars(|c| c.set(cvar, colors[i].0));
        return true;
    }
    false
}

/// `buildTeams`: Your team and Enemy team, each with its model and colour,
/// then the outline and brightness.
fn teams(ui: &mut egui::Ui) {
    card(ui, "Who's who", "Your side, and everyone you can shoot", |ui| {
        columns(ui, &[1.0, 1.0], 20.0, |ui, i| {
            let (side, model_side, name) = if i == 0 {
                (Side::Ally, PlayerModelSide::Ally, "Your team")
            } else {
                (Side::Enemy, PlayerModelSide::Enemy, "Enemy team")
            };
            team_card(ui, side, model_side, name);
        });
        ui.add_space(18.0);
        columns(ui, &[1.0, 1.0], 26.0, |ui, i| {
            if i == 0 {
                choice_seg(ui, Head::new("Outline"), &OUTLINE_CHOICES, None);
            } else {
                choice_seg(
                    ui,
                    Head::new("Brighter players"),
                    &BOOST_CHOICES,
                    Some("Makes players stand out in dark corners"),
                );
            }
        });
    });
}

/// `.fps-set-team-card`: the side's colour along its top, the figure, the
/// model and the colour.
fn team_card(ui: &mut egui::Ui, side: Side, model_side: PlayerModelSide, name: &str) {
    let width = ui.available_width();
    let top = ui.cursor().min;
    let color = with_cvars(|c| side_color(c, side)).map_or_else(
        || team_color(&TEAM_COLORS[if side == Side::Ally { 0 } else { 1 }]),
        team_color,
    );
    let pad = 18.0;
    let painter = ui.painter().clone();
    let bg = painter.add(egui::Shape::Noop);
    let title_color = if side == Side::Ally { color } else { cream() };
    let title = line(
        &painter,
        &name.to_uppercase(),
        15.0,
        display_family(),
        title_color,
        0.12,
    );
    let th = title.size().y;
    painter.galley(top + vec2(pad, 18.0), title, cream());
    // The figure's stage.
    let stage = Rect::from_min_size(
        pos2(top.x + 1.5, top.y + 18.0 + th + 16.0),
        vec2(width - 3.0, width * 0.68),
    );
    gradient_polygon(
        &painter,
        &[
            stage.left_top(),
            stage.right_top(),
            stage.right_bottom(),
            stage.left_bottom(),
        ],
        &[
            (0.0, hsl(250.0, 34.0, 13.0)),
            (0.6, hsl(256.0, 30.0, 17.0)),
            (1.0, hsl(250.0, 36.0, 10.0)),
        ],
    );
    let setting = player_model_setting(model_side);
    let label = if setting == "random" {
        "Random"
    } else {
        player_model_label(setting)
    };
    // In the browser a 3D figure stands here; natively its silhouette: the
    // side's outline round the model's name.
    let outline = with_cvars(|c| current_choice(c, &OUTLINE_CHOICES).map_or("thin", |c| c.id));
    let figure = line(&painter, &label.to_uppercase(), 22.0, display_family(), cream(), 0.08);
    let fr = Rect::from_center_size(stage.center(), figure.size() + vec2(40.0, 26.0));
    painter.rect_filled(fr, 3.0, hsla(250.0, 45.0, 5.0, 0.6));
    let ow = match outline {
        "off" => 0.0,
        "bold" => 3.0,
        _ => 1.5,
    };
    if ow > 0.0 {
        painter.rect_stroke(fr, 3.0, Stroke::new(ow, color), egui::StrokeKind::Outside);
    }
    painter.galley(fr.center() - figure.size() / 2.0, figure, cream());
    // The fields under it.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(top.x + pad, stage.bottom() + 18.0),
                vec2(width - 2.0 * pad, f32::INFINITY),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let field_w = width - 2.0 * pad;
    let model = line(&painter, "MODEL", 15.0, body(), hsla(45.0, 100.0, 95.0, 0.82), 0.14);
    let mh = model.size().y;
    painter.galley(child.cursor().min, model, cream());
    child.add_space(mh + 9.0);
    let settings: Vec<&str> = if side == Side::Enemy {
        std::iter::once("random").chain(PLAYER_MODELS).collect()
    } else {
        PLAYER_MODELS.to_vec()
    };
    let labels: Vec<&str> = settings
        .iter()
        .map(|m| {
            if *m == "random" {
                "Random"
            } else {
                player_model_label(m)
            }
        })
        .collect();
    let at = settings.iter().position(|m| *m == setting).unwrap_or(0);
    if let Some(i) = select_menu(&mut child, name, &labels, at, field_w) {
        set_player_model_choice(model_side, settings[i]);
    }
    child.add_space(18.0);
    let other = if side == Side::Ally { Side::Enemy } else { Side::Ally };
    let index = |s| with_cvars(|c| side_color(c, s)).and_then(|t| TEAM_COLORS.iter().position(|o| o == t));
    let colors: Vec<Color32> = TEAM_COLORS.iter().map(team_color).collect();
    if let Some(i) = swatches(&mut child, "Colour", &colors, index(side), index(other), None, true) {
        with_cvars(|c| set_side_color(c, side, &TEAM_COLORS[i]));
    }
    let bottom = child.min_rect().bottom() + pad;
    let rect = Rect::from_min_max(top, pos2(top.x + width, bottom));
    painter.set(
        bg,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, 2.0, hsla(250.0, 40.0, 9.0, 0.8)),
            egui::Shape::rect_stroke(
                rect,
                2.0,
                Stroke::new(1.5, color.gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            ),
            egui::Shape::rect_filled(Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)), 0.0, color),
        ]),
    );
    ui.allocate_rect(rect, egui::Sense::hover());
}

/// `buildView`: field of view with its wedge, the gun, and what the screen
/// shows.
fn view(ui: &mut egui::Ui) {
    card(ui, "View & info", "", |ui| {
        let fields: [(&str, &str, &str); 5] = [
            (
                "cg_screenshake",
                "Explosion shake",
                "The view shakes when a rocket, grenade, plasma or blade blast goes off near you",
            ),
            ("cg_hitshake", "Shake when hit", "The view shakes when you take damage"),
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
        ];
        field_grid(ui, 10, 2, |ui, k| match k {
            0 => {
                cvar_slider(
                    ui,
                    "Field of view",
                    "cl_fov",
                    Range::new(80.0, 110.0, 1.0, |v| format!("{v}°")),
                    Some("How wide you see. Wider shows more, but things look smaller"),
                    true,
                );
                fov_wedge(ui);
            }
            1 => cvar_seg(
                ui,
                Head::new("Gun size"),
                "cl_gunsize",
                &[(0.0, "Normal"), (1.0, "Small"), (2.0, "Smallest")],
                Some("How large your own gun is on screen. Smaller shows more of the view around it"),
                true,
            ),
            2..=6 => {
                let (cvar, name, help) = fields[k - 2];
                on_off(ui, Head::new(name), cvar, Some(help), true);
            }
            7 => cvar_seg(
                ui,
                Head::new("Match clock"),
                "cl_invertClock",
                &[(0.0, "Time left"), (1.0, "Time played")],
                Some("Count the match time down to the end, or up from the start"),
                true,
            ),
            8 => on_off(
                ui,
                Head::new("Switch to picked-up weapons"),
                "cl_autoswitch",
                Some("Take out a weapon you walk over. Off keeps what you hold, so a blade fight stays a blade fight"),
                true,
            ),
            _ => cvar_seg(
                ui,
                Head::new("Connection info"),
                "cl_netgraph",
                &[(0.0, "Off"), (1.0, "Numbers"), (2.0, "Graph")],
                Some("Ping and packet loss in a corner of the screen"),
                true,
            ),
        });
    });
}

/// `.fps-set-fov`: the view's wedge from above.
fn fov_wedge(ui: &mut egui::Ui) {
    let fov = with_cvars(|c| c.get_number("cl_fov")) as f32;
    let width = ui.available_width();
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 64.0), egui::Sense::hover());
    // viewBox "-100 -8 200 100", fitted to the height, centred.
    let k = rect.height() / 100.0;
    let origin = pos2(rect.center().x, rect.top() + (8.0 + 84.0) * k);
    let half = (fov / 2.0).to_radians();
    let radius = 92.0 * k;
    let mut points = vec![origin];
    let steps = 24;
    for i in 0..=steps {
        let a = -half + 2.0 * half * i as f32 / steps as f32;
        points.push(pos2(origin.x + a.sin() * radius, origin.y - a.cos() * radius));
    }
    let gold = hsl(46.0, 100.0, 60.0);
    ui.painter().add(egui::Shape::convex_polygon(
        points.clone(),
        hsla(46.0, 100.0, 60.0, 0.16),
        Stroke::NONE,
    ));
    ui.painter()
        .add(egui::Shape::closed_line(points, Stroke::new(1.5 * k.max(0.6), gold)));
    ui.painter().circle_filled(origin, 5.0 * k, cream());
}
