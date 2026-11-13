//! wash-fps `client/settings/ControllerPage.ts` (and `ControllerPage.css`):
//! the controller found, the layout preset, the pad drawn with what each
//! button does, the actions with their buttons, and how the sticks feel.
//!
//! Native difference: the device follows the last input
//! (`shagger::input::input_device`), so the browser's Use controller and
//! Use keyboard keys are not here. The page reads the pads itself while it
//! is shown, through its own `GamepadBackend`.

use shagger::core::cvar::with_cvars;
use shagger::input::gamepad_backend::GamepadBackend;
use shagger::input::gamepad_input::{GamepadInput, pad_label};
use shagger::input::input_system::{InputSystem, with_input};
use shagger::ui::components::lobby_twin::{cream, hsl, hsla, ink};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Rect, Shape, Stroke, pos2, vec2};

use super::controller_model::{
    CONTROLLER_ACTIONS, ControllerPreset, apply_controller_preset, assign_controller_button, controller_buttons,
    controller_label, controller_preset,
};
use super::controls_page::group_color;
use super::fps_settings_view::{cvar_slider, on_off};
use super::settings_fields::{
    Head, Range, body, card, columns, format_percent, gold_text, line, plain, seg, seg_row, wrapped,
};
use super::settings_model::{ACTION_GROUPS, ActionGroupId};
use crate::client::washoi_style::display_family;

/// `POSITIONS`: each button's place on the drawing, in percent.
const POSITIONS: [(f32, f32); 16] = [
    (77.0, 53.0),
    (85.0, 41.0),
    (69.0, 41.0),
    (77.0, 29.0),
    (23.0, 12.0),
    (77.0, 12.0),
    (23.0, 2.0),
    (77.0, 2.0),
    (42.0, 36.0),
    (58.0, 36.0),
    (26.0, 40.0),
    (60.0, 68.0),
    (39.0, 61.0),
    (39.0, 81.0),
    (32.0, 71.0),
    (46.0, 71.0),
];

/// The open sheet: waiting for a button, picking an action, or replacing.
#[derive(Debug, Clone)]
enum Picker {
    Capture(&'static str),
    Choose(u64),
    Conflict {
        action: &'static str,
        button: u64,
        other: &'static str,
    },
}

/// `ControllerPage`.
#[derive(Default)]
pub struct ControllerPage {
    backend: Option<GamepadBackend>,
    pad: GamepadInput,
    picker: Option<Picker>,
    /// The action or button pointed at, lighting its partner.
    hl_action: Option<&'static str>,
    hl_button: Option<u64>,
    said: Option<String>,
}

impl std::fmt::Debug for ControllerPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControllerPage")
            .field("picker", &self.picker)
            .finish_non_exhaustive()
    }
}

fn group_of(group: &str) -> ActionGroupId {
    match group {
        "move" => ActionGroupId::Move,
        "weapons" => ActionGroupId::Weapons,
        "fight" => ActionGroupId::Fight,
        _ => ActionGroupId::Other,
    }
}

impl ControllerPage {
    /// `deactivate`.
    pub fn deactivate(&mut self) {
        self.picker = None;
    }

    /// Escape closes the sheet. True when it did.
    pub fn back(&mut self) -> bool {
        self.picker.take().is_some()
    }

    pub fn take_toast(&mut self) -> Option<String> {
        self.said.take()
    }

    /// `tick`: the pads this frame, and a press for the action waiting.
    fn tick(&mut self) {
        let backend = self.backend.get_or_insert_with(GamepadBackend::new);
        self.pad.available = backend.available();
        let pads = backend.poll();
        self.pad.sample(pads);
        if let Some(Picker::Capture(action)) = self.picker {
            if self.pad.pad.is_none() || self.pad.pressed.contains(&9) {
                self.picker = None;
            } else if let Some(&button) = self.pad.pressed.iter().find(|&&b| b < 16) {
                self.assign(action, button as u64);
            }
        }
    }

    fn assign(&mut self, action: &'static str, button: u64) {
        let conflict = with_input(|i| {
            CONTROLLER_ACTIONS
                .iter()
                .find(|a| a.id != action && controller_buttons(i, a.id).contains(&button))
                .map(|a| a.name)
        });
        match conflict {
            Some(other) => self.picker = Some(Picker::Conflict { action, button, other }),
            None => self.commit(action, button),
        }
    }

    fn commit(&mut self, action: &str, button: u64) {
        with_input(|i| assign_controller_button(i, action, Some(button)));
        self.picker = None;
        self.pad.suppress_until_neutral();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _now: f64) {
        self.tick();
        ui.ctx().request_repaint();
        let hl_action = self.hl_action.take();
        let hl_button = self.hl_button.take();
        let two = ui.available_width() > 1000.0;
        let fractions: &[f32] = if two { &[1.0, 1.0] } else { &[1.0] };
        // Row 1: the controller and the layout.
        columns(ui, fractions, 20.0, |ui, i| {
            if i == 0 {
                self.connection(ui);
            } else {
                self.layout(ui);
            }
        });
        if !two {
            ui.add_space(20.0);
            self.layout(ui);
        }
        ui.add_space(20.0);
        // Row 2: the drawing and the actions.
        with_input(|input| {
            columns(ui, fractions, 20.0, |ui, i| {
                if i == 0 {
                    self.diagram(ui, input, hl_action);
                } else {
                    self.actions(ui, input, hl_button);
                }
            });
            if !two {
                ui.add_space(20.0);
                self.actions(ui, input, hl_button);
            }
        });
        ui.add_space(20.0);
        feel(ui);
        self.sheet(ui);
    }

    fn connection(&mut self, ui: &mut egui::Ui) {
        card(ui, "Controller", "", |ui| {
            let status = if !self.pad.available {
                "Controllers can't be read on this computer.".to_owned()
            } else {
                match &self.pad.pad {
                    None => "Press a controller button to detect it.".to_owned(),
                    Some(p) if p.mapping != "standard" => format!("{}: unsupported mapping", p.id),
                    Some(p) => format!("{} · the game switches to it when you press a button", p.id),
                }
            };
            let width = ui.available_width();
            let painter = ui.painter().clone();
            let g = wrapped(&painter, &status, egui::FontId::new(16.0, body()), cream(), width);
            let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
            painter.galley(r.min, g, cream());
            // `fps-pad-devices`: a key per pad, to pick which one plays.
            if self.pad.pads.len() > 1 {
                ui.add_space(12.0);
                let names: Vec<String> = self.pad.pads.iter().map(|p| p.id.clone()).collect();
                let labels: Vec<&str> = names.iter().map(String::as_str).collect();
                let at = self
                    .pad
                    .pad
                    .as_ref()
                    .and_then(|p| self.pad.pads.iter().position(|q| q.index == p.index));
                if let Some(i) = seg_row(ui, &labels, at, width, 1.0) {
                    let index = self.pad.pads[i].index;
                    self.pad.select(index);
                }
            }
            ui.add_space(12.0);
            let live = match &self.pad.pad {
                Some(p) => {
                    let axis = |i: usize| p.axes.get(i).copied().unwrap_or(0.0);
                    let trigger = |i: usize| p.buttons.get(i).map_or(0.0, |b| b.value);
                    format!(
                        "Left {:.2}, {:.2} · Right {:.2}, {:.2} · LT {:.2} · RT {:.2}",
                        axis(0),
                        axis(1),
                        axis(2),
                        axis(3),
                        trigger(6),
                        trigger(7)
                    )
                }
                None => String::new(),
            };
            let g = line(&painter, &live, 12.0, body(), hsla(45.0, 100.0, 95.0, 0.6), 0.0);
            let (r, _) = ui.allocate_exact_size(vec2(width, 24.0), egui::Sense::hover());
            painter.galley(r.min, g, cream());
        });
    }

    fn layout(&mut self, ui: &mut egui::Ui) {
        card(ui, "Layout", "", |ui| {
            let painter = ui.painter().clone();
            let width = ui.available_width();
            let note = wrapped(
                &painter,
                "Menu navigation always uses A to select, B to go back, and Menu to pause.",
                egui::FontId::new(13.0, body()),
                hsla(45.0, 100.0, 95.0, 0.45),
                width,
            );
            let (r, _) = ui.allocate_exact_size(note.size(), egui::Sense::hover());
            painter.galley(r.min, note, cream());
            ui.add_space(14.0);
            let current = with_input(|i| controller_preset(i));
            let at = current.map(|p| usize::from(p == ControllerPreset::Bumper));
            if let Some(i) = seg(ui, Head::new("Preset"), &["Standard", "Bumper jump"], at, None, true) {
                let preset = if i == 0 {
                    ControllerPreset::Standard
                } else {
                    ControllerPreset::Bumper
                };
                with_input(|input| apply_controller_preset(input, preset));
            }
            ui.add_space(12.0);
            let text = if current.is_some() {
                "Edit any action below to make a custom layout."
            } else {
                "Custom layout"
            };
            let g = line(&painter, text, 15.0, body(), hsla(45.0, 100.0, 95.0, 0.7), 0.0);
            let (r, _) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
            painter.galley(r.min, g, cream());
        });
    }

    /// `fps-pad-diagram`: the pad's outline and a cap per button.
    fn diagram(&mut self, ui: &mut egui::Ui, input: &InputSystem, hl_action: Option<&'static str>) {
        card(ui, "Your controller", "", |ui| {
            let width = ui.available_width();
            let painter = ui.painter().clone();
            let note = wrapped(
                &painter,
                "Select a button to choose its action. Point at an action to light its button.",
                egui::FontId::new(13.0, body()),
                hsla(45.0, 100.0, 95.0, 0.45),
                width,
            );
            let (r, _) = ui.allocate_exact_size(note.size(), egui::Sense::hover());
            painter.galley(r.min, note, cream());
            ui.add_space(30.0);
            let h = width * 310.0 / 480.0;
            let (area, _) = ui.allocate_exact_size(vec2(width, h), egui::Sense::hover());
            let k = width / 480.0;
            let p = |x: f32, y: f32| pos2(area.left() + x * k, area.top() + y * k);
            let stroke = Stroke::new(2.0, cream().gamma_multiply(0.6));
            // The body: the SVG path's curves, flattened.
            let quad = |a: egui::Pos2, c: egui::Pos2, b: egui::Pos2, out: &mut Vec<egui::Pos2>| {
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    out.push(pos2(
                        u * u * a.x + 2.0 * u * t * c.x + t * t * b.x,
                        u * u * a.y + 2.0 * u * t * c.y + t * t * b.y,
                    ));
                }
            };
            let mut outline = vec![p(110.0, 48.0)];
            quad(p(110.0, 48.0), p(240.0, 10.0), p(370.0, 48.0), &mut outline);
            quad(p(370.0, 48.0), p(415.0, 62.0), p(446.0, 235.0), &mut outline);
            quad(p(446.0, 235.0), p(452.0, 293.0), p(407.0, 281.0), &mut outline);
            outline.push(p(330.0, 213.0));
            quad(p(330.0, 213.0), p(240.0, 240.0), p(150.0, 213.0), &mut outline);
            outline.push(p(73.0, 281.0));
            quad(p(73.0, 281.0), p(28.0, 293.0), p(34.0, 235.0), &mut outline);
            quad(p(34.0, 235.0), p(65.0, 62.0), p(110.0, 48.0), &mut outline);
            painter.add(Shape::closed_line(outline, stroke));
            painter.circle_stroke(p(125.0, 124.0), 36.0 * k, stroke);
            painter.circle_stroke(p(288.0, 211.0), 33.0 * k, stroke);
            painter.rect_stroke(
                Rect::from_min_max(p(165.0, 188.0), p(208.0, 231.0)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            let lit_buttons: Vec<u64> = hl_action.map(|a| controller_buttons(input, a)).unwrap_or_default();
            for (index, (x, y)) in POSITIONS.iter().enumerate() {
                let b = index as u64;
                let label = match index {
                    12 => "Up".to_owned(),
                    13 => "Down".to_owned(),
                    14 => "Left".to_owned(),
                    15 => "Right".to_owned(),
                    _ => pad_label(b),
                };
                let action = CONTROLLER_ACTIONS
                    .iter()
                    .find(|a| controller_buttons(input, a.id).contains(&b));
                let c = action.map_or(Color32::from_rgb(0xc9, 0xc5, 0xb9), |a| group_color(group_of(a.group)));
                let g = line(&painter, &label, 12.0, body(), cream(), 0.0);
                let size = vec2((g.size().x + 14.0).max(32.0), g.size().y + 10.0);
                let center = pos2(
                    area.left() + area.width() * x / 100.0,
                    area.top() + area.height() * y / 100.0,
                );
                let r = Rect::from_center_size(center, size);
                let response = ui.interact(r, ui.scope_id().with(("pad-cap", index)), egui::Sense::click());
                if response.hovered() {
                    self.hl_button = Some(b);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let pressed = self
                    .pad
                    .pad
                    .as_ref()
                    .and_then(|p| p.buttons.get(index))
                    .is_some_and(|v| v.value > 0.3);
                let on = pressed || lit_buttons.contains(&b) || response.hovered();
                if on {
                    for k in 1..=3 {
                        painter.rect_filled(r.expand(k as f32 * 3.0), 8.0 + k as f32 * 3.0, c.gamma_multiply(0.08));
                    }
                }
                painter.rect_filled(r, 8.0, if on { c } else { Color32::from_rgb(0x24, 0x23, 0x36) });
                painter.rect_stroke(r, 8.0, Stroke::new(2.0, c), egui::StrokeKind::Inside);
                let color = if on {
                    Color32::from_rgb(0x17, 0x16, 0x24)
                } else {
                    Color32::WHITE
                };
                let g = line(&painter, &label, 12.0, body(), color, 0.0);
                painter.galley(r.center() - g.size() / 2.0, g, color);
                if response.clicked() {
                    if index == 9 {
                        self.said = Some("Menu is reserved for pause and recovery.".into());
                    } else {
                        self.picker = Some(Picker::Choose(b));
                    }
                }
            }
            ui.add_space(8.0);
            let g = line(
                &painter,
                "Sticks: move / look · LS: walk with Always run · Menu: pause / resume",
                13.0,
                body(),
                hsla(45.0, 100.0, 95.0, 0.55),
                0.0,
            );
            let (r, _) = ui.allocate_exact_size(vec2(width, g.size().y), egui::Sense::hover());
            painter.galley(r.min, g, cream());
        });
    }

    /// `What each button does`: the actions by group, two across.
    fn actions(&mut self, ui: &mut egui::Ui, input: &InputSystem, hl_button: Option<u64>) {
        card(ui, "What each button does", "", |ui| {
            let painter = ui.painter().clone();
            let width = ui.available_width();
            let note = wrapped(
                &painter,
                "Select an action, release the controls, then press its new button. Menu cancels.",
                egui::FontId::new(13.0, body()),
                hsla(45.0, 100.0, 95.0, 0.45),
                width,
            );
            let (r, _) = ui.allocate_exact_size(note.size(), egui::Sense::hover());
            painter.galley(r.min, note, cream());
            ui.add_space(14.0);
            for pair in ACTION_GROUPS.chunks(2) {
                columns(ui, &[1.0, 1.0], 26.0, |ui, i| {
                    let Some(group) = pair.get(i) else { return };
                    let c = group_color(group.id);
                    let painter = ui.painter().clone();
                    let title = line(&painter, &group.name.to_uppercase(), 14.0, display_family(), c, 0.16);
                    let (r, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), title.size().y + 11.0), egui::Sense::hover());
                    painter.galley(r.min, title, c);
                    painter.hline(r.left()..=r.right(), r.bottom() - 1.5, Stroke::new(3.0, c));
                    ui.add_space(8.0);
                    let id = match group.id {
                        ActionGroupId::Move => "move",
                        ActionGroupId::Weapons => "weapons",
                        ActionGroupId::Fight => "fight",
                        ActionGroupId::Other => "other",
                    };
                    for action in CONTROLLER_ACTIONS.iter().filter(|a| a.group == id) {
                        let w = ui.available_width();
                        let (row, response) = ui.allocate_exact_size(vec2(w - 30.0, 32.0), egui::Sense::click());
                        let clear = Rect::from_min_size(pos2(row.right() + 4.0, row.top()), vec2(26.0, 32.0));
                        let clear_r = ui.interact(
                            clear,
                            ui.scope_id().with(("pad-clear", action.id)),
                            egui::Sense::click(),
                        );
                        let lit = response.hovered()
                            || hl_button.is_some_and(|b| controller_buttons(input, action.id).contains(&b));
                        if response.hovered() {
                            self.hl_action = Some(action.id);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if lit {
                            painter.rect_filled(row, 0.0, hsla(276.0, 90.0, 64.0, 0.18));
                            painter.rect_filled(Rect::from_min_size(row.min, vec2(3.0, row.height())), 0.0, c);
                        }
                        let text = format!("{} · {}", action.name, controller_label(input, action.id));
                        let g = line(&painter, &text, 14.0, body(), cream(), 0.02);
                        painter.galley(pos2(row.left() + 10.0, row.center().y - g.size().y / 2.0), g, cream());
                        let xc = if clear_r.hovered() {
                            hsl(356.0, 95.0, 66.0)
                        } else {
                            cream()
                        };
                        let k = 5.0;
                        let cc = clear.center();
                        painter.line_segment([cc + vec2(-k, -k), cc + vec2(k, k)], Stroke::new(2.0, xc));
                        painter.line_segment([cc + vec2(-k, k), cc + vec2(k, -k)], Stroke::new(2.0, xc));
                        if clear_r.clicked() {
                            with_input(|i| assign_controller_button(i, action.id, None));
                        } else if response.clicked() {
                            self.picker = Some(Picker::Capture(action.id));
                            self.pad.suppress_until_neutral();
                        }
                    }
                });
                ui.add_space(14.0);
            }
        });
    }

    /// `fps-pad-pop`: a gold sheet in the middle of the screen.
    fn sheet(&mut self, ui: &egui::Ui) {
        let Some(picker) = self.picker.clone() else { return };
        let screen = ui.ctx().content_rect();
        let width = 440.0_f32.min(screen.width() * 0.85);
        let mut close = false;
        let mut chosen = None;
        let mut replace = None;
        let area = egui::Area::new(ui.scope_id().with("pad-pop"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                let painter = ui.painter().clone();
                let bg = painter.add(Shape::Noop);
                let top = ui.cursor().min;
                let inner = width - 48.0;
                let mut y = top.y + 24.0;
                let x = top.x + 24.0;
                let heading = |painter: &egui::Painter, s: &str, y: &mut f32| {
                    let g = wrapped(painter, s, egui::FontId::new(20.0, display_family()), ink(), inner);
                    let h = g.size().y;
                    painter.galley(pos2(x, *y), g, ink());
                    *y += h + 10.0;
                };
                let key = |ui: &egui::Ui, label: &str, y: &mut f32| -> bool {
                    let r = Rect::from_min_size(pos2(x, *y), vec2(inner, 40.0));
                    *y += 40.0 + 4.0 + 8.0;
                    let response = ui.interact(r, ui.scope_id().with(("pad-pop-key", label)), egui::Sense::click());
                    let (top_c, foot) = if response.hovered() {
                        (hsl(278.0, 92.0, 72.0), hsl(268.0, 72.0, 52.0))
                    } else {
                        (hsl(250.0, 24.0, 25.0), hsl(250.0, 30.0, 17.0))
                    };
                    painter.rect_filled(r.translate(vec2(0.0, 4.0)), 2.0, hsl(250.0, 40.0, 8.0));
                    shagger::ui::components::lobby_twin::gradient_polygon(
                        &painter,
                        &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()],
                        &[(0.0, top_c), (1.0, foot)],
                    );
                    let g = line(&painter, &label.to_uppercase(), 13.0, display_family(), cream(), 0.08);
                    painter.galley(r.center() - g.size() / 2.0, g, cream());
                    response.clicked()
                };
                match picker {
                    Picker::Capture(_) => {
                        heading(&painter, "Release controls, then press a button", &mut y);
                        let g = line(
                            &painter,
                            "Press Menu to cancel. B can be assigned.",
                            15.0,
                            body(),
                            ink(),
                            0.0,
                        );
                        let gh = g.size().y;
                        painter.galley(pos2(x, y), g, ink());
                        y += gh + 12.0;
                        if key(ui, "Cancel", &mut y) {
                            close = true;
                        }
                    }
                    Picker::Choose(button) => {
                        heading(&painter, &format!("{}: choose an action", pad_label(button)), &mut y);
                        for action in CONTROLLER_ACTIONS {
                            if key(ui, action.name, &mut y) {
                                chosen = Some((action.id, button));
                            }
                        }
                        if key(ui, "Cancel", &mut y) {
                            close = true;
                        }
                    }
                    Picker::Conflict { action, button, other } => {
                        heading(&painter, &format!("{} already does {other}", pad_label(button)), &mut y);
                        if key(ui, "Cancel", &mut y) {
                            close = true;
                        }
                        if key(ui, "Replace binding", &mut y) {
                            replace = Some((action, button));
                        }
                    }
                }
                let sheet = Rect::from_min_max(top, pos2(top.x + width, y + 16.0));
                painter.set(
                    bg,
                    Shape::Vec(vec![
                        Shape::rect_filled(
                            sheet.translate(vec2(0.0, 24.0)).expand(16.0),
                            24.0,
                            hsla(250.0, 80.0, 2.0, 0.35),
                        ),
                        super::settings_fields::mesh_polygon(
                            &[
                                sheet.left_top(),
                                pos2(sheet.right() - 26.0, sheet.top()),
                                pos2(sheet.right(), sheet.top() + 26.0),
                                sheet.right_bottom(),
                                sheet.left_bottom(),
                            ],
                            &[(0.0, hsl(48.0, 100.0, 60.0)), (1.0, hsl(42.0, 97.0, 50.0))],
                        ),
                    ]),
                );
                ui.allocate_rect(sheet, egui::Sense::hover());
            });
        let _ = (area, gold_text());
        if close {
            self.picker = None;
        }
        if let Some((action, button)) = chosen {
            self.assign(action, button);
        }
        if let Some((action, button)) = replace {
            self.commit(action, button);
        }
    }
}

/// `Stick feel`: turn speeds, curve, dead zones, and the switches.
fn feel(ui: &mut egui::Ui) {
    card(
        ui,
        "Stick feel",
        "Movement uses full speed outside the dead zone",
        |ui| {
            let sliders: [(&str, &str, Range); 6] = [
                (
                    "Horizontal turn speed (°/s)",
                    "in_pad_yaw",
                    Range::new(30.0, 720.0, 10.0, plain),
                ),
                (
                    "Vertical turn speed (°/s)",
                    "in_pad_pitch",
                    Range::new(30.0, 720.0, 10.0, plain),
                ),
                ("Look response curve", "in_pad_curve", Range::new(1.0, 3.0, 0.1, plain)),
                (
                    "Move dead zone",
                    "in_pad_move_deadzone",
                    Range::new(0.05, 0.4, 0.01, format_percent),
                ),
                (
                    "Look dead zone",
                    "in_pad_look_deadzone",
                    Range::new(0.05, 0.4, 0.01, format_percent),
                ),
                ("While aiming", "in_pad_ads", Range::new(0.1, 1.0, 0.05, format_percent)),
            ];
            for (name, cvar, range) in sliders {
                cvar_slider(ui, name, cvar, range, None, true);
                ui.add_space(16.0);
            }
            let width = ui.available_width().min(320.0);
            for (name, cvar) in [
                ("Invert vertical look", "in_pad_invert"),
                ("Swap move and look sticks", "in_pad_swap"),
                ("Aim assist", "in_pad_assist"),
            ] {
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(ui.cursor().min, vec2(width, f32::INFINITY)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                on_off(&mut child, Head::new(name), cvar, None, true);
                let used = child.min_rect();
                ui.allocate_rect(used, egui::Sense::hover());
                ui.add_space(16.0);
            }
            let _ = with_cvars(|c| c.exists("in_pad_yaw"));
        },
    );
}
