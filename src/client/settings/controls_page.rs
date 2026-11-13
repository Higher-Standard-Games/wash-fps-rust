//! wash-fps `client/settings/ControlsPage.ts` (and `InputDiagram.ts`): the
//! keyboard and mouse coloured by what each key does, the list of actions
//! with their keys, and how the mouse feels.
//!
//! A key is changed from either end: click the key and pick what it does,
//! or click an action and press the key, click or wheel turn you want for
//! it. The keys come from egui's events under the browser's names
//! ([`egui_code`]).

use shagger::input::input_system::{InputSystem, with_input};
use shagger::ui::components::lobby_twin::{cream, gold, gradient_polygon, hsl, hsla, ink};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use super::fps_settings_view::{cvar_slider, on_off};
use super::settings_fields::{Head, Range, body, card, columns, format_percent, gold_text, line, plain};
use super::settings_model::{
    ACTION_GROUPS, ARROW_KEYS, ActionGroupId, AssignResult, MOUSE_INPUTS, SETTINGS_ACTIONS, SettingsAction, action_for,
    assign_key, code_name, codes_for, fixed_key, group_of, input_name, keyboard_rows,
};
use crate::client::washoi_style::display_family;

/// A group's colour (`--c`).
#[must_use]
pub fn group_color(id: ActionGroupId) -> Color32 {
    let g = ACTION_GROUPS.iter().find(|g| g.id == id).unwrap_or(&ACTION_GROUPS[0]);
    hsl(g.hsl[0], g.hsl[1] * 100.0, g.hsl[2] * 100.0)
}

fn action_color(a: &SettingsAction) -> Color32 {
    group_color(group_of(a).id)
}

/// `color-mix(in srgb, a p, b)`.
fn mix(a: Color32, p: f32, b: Color32) -> Color32 {
    b.lerp_to_gamma(a, p)
}

/// The browser's `KeyboardEvent.code` for an egui key, where the board has it.
#[must_use]
pub fn egui_code(key: egui::Key) -> Option<String> {
    use egui::Key;
    let name = key.name();
    let code = match key {
        Key::Escape => "Escape",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Enter => "Enter",
        Key::Space => "Space",
        Key::ArrowUp => "ArrowUp",
        Key::ArrowDown => "ArrowDown",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowRight => "ArrowRight",
        Key::Minus => "Minus",
        Key::Equals => "Equal",
        Key::OpenBracket => "BracketLeft",
        Key::CloseBracket => "BracketRight",
        Key::Backslash => "Backslash",
        Key::Semicolon => "Semicolon",
        Key::Quote => "Quote",
        Key::Comma => "Comma",
        Key::Period => "Period",
        Key::Slash => "Slash",
        Key::Backtick => "Backquote",
        _ if name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase()) => return Some(format!("Key{name}")),
        _ if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) => return Some(format!("Digit{name}")),
        _ if name.starts_with('F') && name[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => {
            return Some(name.to_owned());
        }
        _ => return None,
    };
    Some(code.to_owned())
}

/// `ControlsPage`.
#[derive(Debug, Default)]
pub struct ControlsPage {
    /// The action waiting for a key.
    pub listening: Option<&'static str>,
    /// The key whose menu is open, and the key's rect.
    pop: Option<(String, Rect)>,
    /// What the page has to say, for the view's toast.
    said: Option<String>,
    /// The action pointed at, whose keys stay lit.
    focus: Option<&'static str>,
    /// Shift and the like held while listening, bound if released alone.
    modifiers: egui::Modifiers,
    /// The welcome guide's picture of the keys (`lobby-guide__preview`):
    /// nothing answers the pointer, and keys that do nothing fade.
    preview: bool,
}

impl ControlsPage {
    /// `deactivate`: leaving the page drops a half-made change.
    pub fn deactivate(&mut self) {
        self.pop = None;
        self.listening = None;
    }

    /// Escape: drops the key menu or the wait for a key. True when it did.
    pub fn back(&mut self) -> bool {
        self.pop.take().is_some() || self.listening.take().is_some()
    }

    /// The toast's text, once.
    pub fn take_toast(&mut self) -> Option<String> {
        self.said.take()
    }

    /// Press-to-bind (`listen`): a key, button or wheel turn under the
    /// browser's names. True when the page took it. Escape cancels.
    pub fn take_input(&mut self, code: &str) -> bool {
        if self.listening.is_none() {
            if code == "Escape" && self.pop.is_some() {
                self.pop = None;
                return true;
            }
            return false;
        }
        if code == "Escape" {
            self.listening = None;
            return true;
        }
        if fixed_key(code).is_some() {
            self.say_fixed(code);
            return true;
        }
        let id = self.listening.take();
        self.assign(code, id);
        true
    }

    fn assign(&mut self, code: &str, id: Option<&str>) {
        let result = with_input(|i| assign_key(code, id, i));
        let key = code_name(code);
        self.said = Some(match result {
            AssignResult::Fixed(_) => return self.say_fixed(code),
            AssignResult::Cleared { .. } => format!("{key} does nothing now"),
            AssignResult::Assigned {
                action,
                previous: Some(p),
            } if p.id != action.id => format!("{key} was {}, now {}", p.name, action.name),
            AssignResult::Assigned { previous: Some(_), .. } => return,
            AssignResult::Assigned { action, previous: None } => format!("{key} → {}", action.name),
        });
    }

    fn say_fixed(&mut self, code: &str) {
        if let Some(f) = fixed_key(code).and_then(|a| a.fixed) {
            self.said = Some(format!(
                "{} always {}. That can't be changed yet",
                code_name(code),
                f.does
            ));
        }
    }

    fn click_input(&mut self, code: &str, at: Rect) {
        if fixed_key(code).is_some() {
            self.say_fixed(code);
        } else {
            self.listening = None;
            self.pop = Some((code.to_owned(), at));
        }
    }

    /// While an action waits, the next key, click or wheel turn is its.
    fn listen(&mut self, ui: &egui::Ui) {
        if self.listening.is_none() && self.pop.is_none() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        let mods_now = ui.input(|i| i.modifiers);
        for event in events {
            let code = match event {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    repeat: false,
                    ..
                } => egui_code(physical_key.unwrap_or(key)),
                egui::Event::PointerButton {
                    button, pressed: true, ..
                } if self.listening.is_some() => match button {
                    egui::PointerButton::Secondary => Some("mouse:right".to_owned()),
                    egui::PointerButton::Middle => Some("mouse:middle".to_owned()),
                    // A left click is the page's own; Fire gets it from the
                    // mouse drawing.
                    _ => None,
                },
                egui::Event::MouseWheel { delta, .. } if self.listening.is_some() && delta.y != 0.0 => {
                    Some(if delta.y > 0.0 { "wheel:up" } else { "wheel:down" }.to_owned())
                }
                _ => None,
            };
            if let Some(code) = code
                && self.take_input(&code)
            {
                break;
            }
        }
        // A modifier on its own: Shift, Ctrl or Alt, once let go.
        if self.listening.is_some() {
            let was = self.modifiers;
            let released = |before: bool, now: bool| before && !now;
            let code = if released(was.shift, mods_now.shift) {
                Some("ShiftLeft")
            } else if released(was.ctrl, mods_now.ctrl) {
                Some("ControlLeft")
            } else if released(was.alt, mods_now.alt) {
                Some("AltLeft")
            } else {
                None
            };
            if let Some(code) = code {
                self.take_input(code);
            }
        }
        self.modifiers = mods_now;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _now: f64) {
        self.listen(ui);
        let focus_before = self.focus;
        self.focus = None;
        let side = (ui.available_width() * 0.19 * 1.25).clamp(280.0, 340.0);
        let main = ui.available_width() - side - 20.0;
        let mut change = None;
        with_input(|input| {
            columns(ui, &[main, side], 20.0, |ui, i| {
                if i == 0 {
                    card(ui, "Keyboard", "Click a key to choose what it does", |ui| {
                        self.keyboard(ui, input, focus_before);
                    });
                } else {
                    card(ui, "Mouse", "", |ui| self.mouse(ui, input, focus_before));
                }
            });
            ui.add_space(20.0);
            columns(ui, &[main, side], 20.0, |ui, i| {
                if i == 0 {
                    card(
                        ui,
                        "What each key does",
                        "Click an action, then press the key you want",
                        |ui| {
                            change = self.actions(ui, input);
                        },
                    );
                } else {
                    card(ui, "Mouse feel", "", |ui| {
                        cvar_slider(
                            ui,
                            "Sensitivity",
                            "m_sensitivity",
                            Range::new(0.1, 10.0, 0.1, plain),
                            None,
                            true,
                        );
                        ui.add_space(16.0);
                        cvar_slider(
                            ui,
                            "While aiming",
                            "cl_ads_sensitivity",
                            Range::new(0.1, 1.0, 0.05, format_percent),
                            Some("Share of your sensitivity while you hold Aim"),
                            true,
                        );
                        ui.add_space(16.0);
                        on_off(
                            ui,
                            Head::new("Always run"),
                            "cl_autorun",
                            Some("On: you run all the time, and holding Sprint walks"),
                            true,
                        );
                    });
                }
            });
            if let Some(c) = self.pop_sheet(ui, input) {
                change = Some(c);
            }
        });
        if let Some((code, id)) = change {
            self.assign(&code, id);
        }
        if self.listening.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// Keys answer clicks, except in the preview.
    fn sense(&self) -> egui::Sense {
        if self.preview {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        }
    }

    /// `.lobby-guide__preview`: the keyboard and the mouse as they are bound,
    /// to look at only, in `rect`'s width from its top; returns the bottom.
    /// The mouse takes a 250px column (225 on the web; the labels are
    /// wider here), one column under 820px.
    pub fn preview(ui: &mut egui::Ui, rect: Rect, screen_w: f32) -> f32 {
        let mut page = Self {
            preview: true,
            ..Self::default()
        };
        let painter = ui.painter().clone();
        let one_column = screen_w <= 820.0;
        let inner = rect.shrink2(vec2(18.0, 16.0));
        let (board_w, mouse_left) = if one_column {
            (inner.width(), inner.left())
        } else {
            (inner.width() - 250.0 - 24.0, inner.right() - 250.0)
        };
        let heading = |x: f32, y: f32, s: &str| {
            let g = line(&painter, s, 15.0, display_family(), cream(), 0.1);
            let h = g.size().y;
            painter.galley(pos2(x, y), g, cream());
            y + h + 12.0
        };
        // Painted under the keys once their height is known.
        let backdrop = painter.add(Shape::Noop);
        let mut bottom = inner.top();
        with_input(|input| {
            let board_top = heading(inner.left(), inner.top(), "KEYBOARD");
            let board = Rect::from_min_size(pos2(inner.left(), board_top), vec2(board_w, 400.0));
            let board_bottom = ui
                .scope_builder(egui::UiBuilder::new().max_rect(board), |ui| {
                    page.keyboard(ui, input, None);
                    ui.min_rect().bottom()
                })
                .inner;
            let mouse_y = if one_column { board_bottom + 24.0 } else { inner.top() };
            let mouse_top = heading(mouse_left, mouse_y, "MOUSE");
            let mouse = Rect::from_min_size(
                pos2(mouse_left, mouse_top),
                vec2(if one_column { 320.0 } else { 250.0 }, 300.0),
            );
            let mouse_bottom = ui
                .scope_builder(egui::UiBuilder::new().max_rect(mouse), |ui| {
                    page.mouse(ui, input, None);
                    ui.min_rect().bottom()
                })
                .inner;
            bottom = board_bottom.max(mouse_bottom) + 16.0;
        });
        let r = Rect::from_min_max(rect.min, pos2(rect.right(), bottom));
        let mut mesh = egui::Mesh::default();
        let (top_c, foot_c) = (hsl(250.0, 35.0, 16.0), hsl(250.0, 45.0, 9.0));
        mesh.colored_vertex(r.left_top(), top_c);
        mesh.colored_vertex(r.right_top(), top_c);
        mesh.colored_vertex(r.right_bottom(), foot_c);
        mesh.colored_vertex(r.left_bottom(), foot_c);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.set(backdrop, mesh);
        bottom
    }

    /// Whether `code` is dimmed: an action is pointed at, and not this key's.
    fn dimmed(&self, focus: Option<&'static str>, code: &str, input: &InputSystem) -> bool {
        focus.is_some_and(|f| !codes_for(f, input).iter().any(|c| c == code))
    }

    /// `.fps-set-cap`: one key, coloured by its action.
    #[allow(clippy::too_many_arguments)]
    fn cap(
        &mut self,
        ui: &mut egui::Ui,
        input: &InputSystem,
        focus: Option<&'static str>,
        rect: Rect,
        code: &str,
        legend: &str,
        show_act: bool,
    ) {
        let response = ui.interact(rect, ui.scope_id().with(("cap", code)), self.sense());
        let action = action_for(code, input);
        let fixed = action.is_some_and(|a| a.fixed.is_some());
        let open = self.pop.as_ref().is_some_and(|(c, _)| c == code);
        let hovered = response.hovered() && !self.preview;
        if hovered {
            ui.ctx().set_cursor_icon(if fixed {
                egui::CursorIcon::Help
            } else {
                egui::CursorIcon::PointingHand
            });
            if let Some(a) = action {
                self.focus = Some(a.id);
            }
        }
        let lit = focus.is_some() && !self.dimmed(focus, code, input);
        let alpha = if self.preview {
            // `.is-muted`: a key that does nothing, or only something minor.
            if action.is_none_or(|a| a.group == ActionGroupId::Other) {
                0.17
            } else {
                1.0
            }
        } else if self.dimmed(focus, code, input) {
            0.28
        } else {
            1.0
        };
        let held = response.is_pointer_button_down_on();
        let lift = if fixed {
            0.0
        } else if held {
            4.0
        } else if lit {
            -3.0
        } else if hovered || open {
            -2.0
        } else {
            0.0
        };
        let c = action.map(action_color);
        let (top, foot, side, text) = match (c, fixed, hovered || open) {
            (Some(c), true, _) => (hsl(250.0, 26.0, 22.0), hsl(250.0, 30.0, 15.0), hsl(250.0, 36.0, 9.0), c),
            (_, false, true) => (
                hsl(278.0, 92.0, 75.0),
                hsl(268.0, 72.0, 54.0),
                hsl(258.0, 72.0, 22.0),
                hsl(45.0, 100.0, 96.0),
            ),
            (Some(c), false, false) => (
                mix(c, 0.72, Color32::WHITE),
                c,
                mix(c, 0.45, hsl(250.0, 50.0, 6.0)),
                ink(),
            ),
            (None, ..) => (
                hsl(250.0, 26.0, 24.0),
                hsl(250.0, 30.0, 17.0),
                hsl(250.0, 36.0, 9.0),
                hsla(45.0, 100.0, 95.0, 0.72),
            ),
        };
        let a = |col: Color32| col.gamma_multiply(alpha);
        let painter = ui.painter();
        let face = rect.translate(vec2(0.0, lift));
        painter.rect_filled(
            rect.translate(vec2(0.0, 8.0 - lift.max(0.0))).shrink(1.0),
            4.0,
            a(hsla(250.0, 70.0, 2.0, 0.4)),
        );
        painter.rect_filled(rect.translate(vec2(0.0, 5.0)), 3.0, a(side));
        gradient_polygon(
            painter,
            &[
                face.left_top(),
                face.right_top(),
                face.right_bottom(),
                face.left_bottom(),
            ],
            &[(0.0, a(top)), (1.0, a(foot))],
        );
        if fixed {
            painter.rect_stroke(face, 3.0, Stroke::new(1.5, a(text)), egui::StrokeKind::Inside);
        } else {
            painter.hline(
                (face.left() + 2.0)..=(face.right() - 2.0),
                face.top() + 0.75,
                Stroke::new(1.5, a(hsla(0.0, 0.0, 100.0, 0.14))),
            );
        }
        let unit = rect.height();
        let legend_size = (unit * 0.26).clamp(10.0, 13.0);
        let g = line(painter, legend, legend_size, display_family(), a(text), 0.02);
        painter.galley(face.min + vec2(6.0, 5.0), g, cream());
        if show_act && let Some(act) = action {
            let narrow = rect.width() <= 42.0 + 12.0;
            let label = if narrow {
                act.on_narrow_key.or(act.on_key).unwrap_or(act.name)
            } else {
                act.on_key.unwrap_or(act.name)
            };
            let size = (unit * 0.21).clamp(9.0, 11.0);
            let text_w = rect.width() - 12.0;
            let label =
                shagger::ui::components::lobby_twin::elide(painter, &label.to_uppercase(), size, body(), text_w);
            let g = line(painter, &label, size, body(), a(text), 0.02);
            painter.galley(pos2(face.left() + 6.0, face.bottom() - 6.0 - g.size().y), g, cream());
        }
        if response.clicked() {
            self.click_input(code, rect);
        }
    }

    /// `.fps-set-board`: six rows, fifteen keys wide, the arrows tucked into
    /// the bottom row.
    fn keyboard(&mut self, ui: &mut egui::Ui, input: &InputSystem, focus: Option<&'static str>) {
        let width = ui.available_width();
        let kh = (width * 0.029 * 1.45).clamp(40.0, 54.0);
        let top = ui.cursor().min;
        let mut y = top.y;
        let rows = keyboard_rows();
        let count = rows.len();
        for (r, row) in rows.into_iter().enumerate() {
            let last = r == count - 1;
            let units: f32 = row.iter().map(|k| k.2).sum::<f32>() + if last { 3.0 } else { 0.0 };
            let n = row.len() + usize::from(last);
            let gap = 6.0;
            let unit = (width - gap * (n as f32 - 1.0)) / units;
            let mut x = top.x;
            for (code, legend, w) in &row {
                let kw = unit * w;
                self.cap(
                    ui,
                    input,
                    focus,
                    Rect::from_min_size(pos2(x, y), vec2(kw, kh)),
                    code,
                    legend,
                    true,
                );
                x += kw + gap;
            }
            if last {
                // `.fps-set-arrows`: left and right tall, up over down.
                let aw = ((unit * 3.0) - 12.0) / 3.0;
                let half = (kh - 3.0) / 2.0;
                for (code, legend) in ARROW_KEYS {
                    let (col, row_, tall) = match code {
                        "ArrowLeft" => (0.0, 0.0, true),
                        "ArrowUp" => (1.0, 0.0, false),
                        "ArrowDown" => (1.0, 1.0, false),
                        _ => (2.0, 0.0, true),
                    };
                    let h = if tall { kh } else { half };
                    let r = Rect::from_min_size(pos2(x + col * (aw + 6.0), y + row_ * (half + 3.0)), vec2(aw, h));
                    self.cap(ui, input, focus, r, code, legend, tall);
                }
            }
            y += kh + 7.0 + if r == 0 { 6.0 } else { 0.0 };
        }
        ui.allocate_rect(
            Rect::from_min_max(top, pos2(top.x + width, y - 7.0 + 5.0)),
            egui::Sense::hover(),
        );
    }

    /// `.fps-set-mouse-wrap`: the mouse drawn, and its parts named.
    fn mouse(&mut self, ui: &mut egui::Ui, input: &InputSystem, focus: Option<&'static str>) {
        let width = ui.available_width();
        let top = ui.cursor().min;
        let height = 300.0_f32;
        let body_r = Rect::from_min_size(pos2(top.x, top.y + (height - 172.0) / 2.0), vec2(104.0, 172.0));
        let painter = ui.painter().clone();
        painter.add(egui::epaint::RectShape::filled(
            body_r.translate(vec2(0.0, 7.0)),
            egui::CornerRadius {
                nw: 52,
                ne: 52,
                sw: 52,
                se: 52,
            },
            hsl(250.0, 40.0, 7.0),
        ));
        painter.add(egui::epaint::RectShape::filled(
            body_r,
            egui::CornerRadius {
                nw: 52,
                ne: 52,
                sw: 52,
                se: 52,
            },
            hsl(250.0, 30.0, 15.0),
        ));
        let zone = |code: &str| -> (Rect, egui::CornerRadius) {
            let (l, t, w) = (body_r.left(), body_r.top(), body_r.width());
            let h = body_r.height();
            match code {
                "mouse:left" => (
                    Rect::from_min_size(pos2(l, t), vec2(w / 2.0 - 2.0, h * 0.44)),
                    egui::CornerRadius {
                        nw: 52,
                        ..Default::default()
                    },
                ),
                "mouse:right" => (
                    Rect::from_min_size(pos2(l + w / 2.0 + 2.0, t), vec2(w / 2.0 - 2.0, h * 0.44)),
                    egui::CornerRadius {
                        ne: 52,
                        ..Default::default()
                    },
                ),
                "wheel:up" => (
                    Rect::from_min_size(pos2(l + w / 2.0 - 11.0, t + h * 0.10), vec2(22.0, h * 0.10)),
                    egui::CornerRadius {
                        nw: 11,
                        ne: 11,
                        sw: 2,
                        se: 2,
                    },
                ),
                "mouse:middle" => (
                    Rect::from_min_size(pos2(l + w / 2.0 - 11.0, t + h * 0.205), vec2(22.0, h * 0.07)),
                    egui::CornerRadius::same(2),
                ),
                _ => (
                    Rect::from_min_size(pos2(l + w / 2.0 - 11.0, t + h * 0.28), vec2(22.0, h * 0.10)),
                    egui::CornerRadius {
                        nw: 2,
                        ne: 2,
                        sw: 11,
                        se: 11,
                    },
                ),
            }
        };
        // Labels, right of the mouse.
        let labels_left = body_r.right() + 18.0;
        let label_w = width - (labels_left - top.x);
        let slot_h = 48.0;
        let labels_top = top.y + (height - (slot_h + 6.0) * 5.0 + 6.0) / 2.0;
        let mut hovered_code = None;
        for (i, (code, name)) in MOUSE_INPUTS.iter().enumerate() {
            let r = Rect::from_min_size(
                pos2(labels_left, labels_top + i as f32 * (slot_h + 6.0)),
                vec2(label_w, slot_h),
            );
            let response = ui.interact(r, ui.scope_id().with(("mlabel", *code)), self.sense());
            let action = action_for(code, input);
            let open = self.pop.as_ref().is_some_and(|(c, _)| c == code);
            let dim = if self.dimmed(focus, code, input) { 0.3 } else { 1.0 };
            let hovered = response.hovered() && !self.preview;
            if hovered || open {
                painter.rect_filled(r, 0.0, hsla(276.0, 90.0, 64.0, 0.2));
            }
            if hovered {
                hovered_code = Some(*code);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let c = action.map_or(hsla(45.0, 100.0, 95.0, 0.2), action_color);
            painter.hline(
                r.left()..=r.right(),
                r.bottom() - 1.0,
                Stroke::new(2.0, c.gamma_multiply(dim)),
            );
            let slot = line(
                &painter,
                &name.to_uppercase(),
                11.0,
                body(),
                hsla(45.0, 100.0, 95.0, 0.5 * dim),
                0.18,
            );
            let sh = slot.size().y;
            painter.galley(r.min + vec2(10.0, 6.0), slot, cream());
            let (act, color) = match action {
                Some(a) => (a.name, cream()),
                None => ("Nothing", hsla(45.0, 100.0, 95.0, 0.32)),
            };
            let g = line(&painter, act, 16.0, body(), color.gamma_multiply(dim), 0.04);
            painter.galley(r.min + vec2(10.0, 6.0 + sh + 3.0), g, cream());
            if response.clicked() {
                self.click_input(code, r);
            }
        }
        for (code, _) in MOUSE_INPUTS {
            let (r, radius) = zone(code);
            let response = ui.interact(r, ui.scope_id().with(("mzone", code)), self.sense());
            let action = action_for(code, input);
            let open = self.pop.as_ref().is_some_and(|(c, _)| c == code);
            let hovered = response.hovered() && !self.preview;
            let lit = hovered || open || hovered_code == Some(code);
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if let Some(a) = action {
                    self.focus = Some(a.id);
                }
            }
            let fill = if lit {
                hsl(273.0, 80.0, 61.0)
            } else {
                action.map_or(hsl(250.0, 24.0, 26.0), action_color)
            };
            let dim = if self.dimmed(focus, code, input) { 0.28 } else { 1.0 };
            if code.starts_with("wheel") || code == "mouse:middle" {
                painter.add(egui::epaint::RectShape::filled(
                    r.expand(3.0),
                    radius,
                    hsl(250.0, 32.0, 12.0),
                ));
            }
            painter.add(egui::epaint::RectShape::filled(r, radius, fill.gamma_multiply(dim)));
            if response.clicked() {
                self.click_input(code, r);
            }
        }
        ui.allocate_rect(Rect::from_min_size(top, vec2(width, height)), egui::Sense::hover());
    }

    /// `renderActs`: the actions by group, four across, with their keys.
    fn actions(&mut self, ui: &mut egui::Ui, input: &InputSystem) -> Option<(String, Option<&'static str>)> {
        let mut change = None;
        let per_row = if ui.available_width() < 900.0 { 2 } else { 4 };
        let fractions = vec![1.0; per_row];
        for chunk in ACTION_GROUPS.chunks(per_row) {
            columns(ui, &fractions, 26.0, |ui, i| {
                let Some(group) = chunk.get(i) else { return };
                let c = group_color(group.id);
                let painter = ui.painter().clone();
                let title = line(&painter, &group.name.to_uppercase(), 14.0, display_family(), c, 0.16);
                let (r, _) = ui.allocate_exact_size(
                    vec2(ui.available_width(), title.size().y + 8.0 + 3.0),
                    egui::Sense::hover(),
                );
                painter.galley(r.min, title, c);
                painter.hline(r.left()..=r.right(), r.bottom() - 1.5, Stroke::new(3.0, c));
                ui.add_space(8.0);
                for action in SETTINGS_ACTIONS.iter().filter(|a| a.group == group.id) {
                    if let Some(x) = self.act_row(ui, input, action, c) {
                        change = Some(x);
                    }
                }
            });
            ui.add_space(18.0);
        }
        change
    }

    /// `.fps-set-act`: an action's name and its chips.
    fn act_row(
        &mut self,
        ui: &mut egui::Ui,
        input: &InputSystem,
        action: &'static SettingsAction,
        c: Color32,
    ) -> Option<(String, Option<&'static str>)> {
        let width = ui.available_width();
        let painter = ui.painter().clone();
        let codes = codes_for(action.id, input);
        let listening = self.listening == Some(action.id);
        let fixed = action.fixed.is_some();
        // The name, wrapped to 55% of the row.
        let name_w = width * 0.55 - 10.0;
        let name_color = if codes.is_empty() && !fixed {
            hsla(45.0, 100.0, 95.0, 0.45)
        } else {
            cream()
        };
        let mut job = egui::text::LayoutJob::simple(
            action.name.to_owned(),
            egui::FontId::new(16.0, body()),
            name_color,
            name_w,
        );
        job.wrap.break_anywhere = false;
        let name = painter.layout_job(job);
        let badge = fixed.then(|| line(&painter, "FIXED", 10.0, body(), hsla(45.0, 100.0, 95.0, 0.5), 0.16));
        let name_h = name.size().y + badge.as_ref().map_or(0.0, |b| b.size().y + 7.0);
        // The chips, laid out right to left.
        let chips: Vec<(String, std::sync::Arc<egui::Galley>)> = codes
            .iter()
            .map(|code| {
                let color = if fixed { c } else { ink() };
                (
                    code.clone(),
                    line(&painter, &code_name(code), 11.0, display_family(), color, 0.02),
                )
            })
            .collect();
        let chip_h = 24.0;
        let height = (name_h + 8.0).max(36.0).max(chip_h + 12.0);
        let (row, response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
        let hovered = response.hovered();
        let hl = hovered || listening;
        if hovered {
            self.focus = Some(action.id);
            ui.ctx().set_cursor_icon(if fixed {
                egui::CursorIcon::Default
            } else {
                egui::CursorIcon::PointingHand
            });
        }
        if listening {
            painter.rect_filled(row, 0.0, c.gamma_multiply(0.22));
        } else if hovered && fixed {
            painter.rect_filled(row, 0.0, hsla(0.0, 0.0, 100.0, 0.04));
        } else if hl {
            painter.rect_filled(row, 0.0, hsla(276.0, 90.0, 64.0, 0.18));
        }
        if hl && !(hovered && fixed) {
            painter.rect_filled(Rect::from_min_size(row.min, vec2(3.0, row.height())), 0.0, c);
        }
        let ny = row.center().y - name_h / 2.0;
        let nh = name.size().y;
        painter.galley(pos2(row.left() + 13.0, ny), name, cream());
        if let Some(b) = badge {
            let r = Rect::from_min_size(pos2(row.left() + 13.0, ny + nh + 4.0), b.size() + vec2(12.0, 4.0));
            painter.rect_stroke(
                r,
                2.0,
                Stroke::new(1.0, hsla(45.0, 100.0, 95.0, 0.22)),
                egui::StrokeKind::Inside,
            );
            painter.galley(r.min + vec2(6.0, 2.0), b, cream());
        }
        let mut change = None;
        let mut x = row.right() - 8.0;
        if listening {
            let t = ui.input(|i| i.time);
            let pulse = 0.725 + 0.275 * (t * std::f64::consts::TAU).cos() as f32;
            let g = line(
                &painter,
                "PRESS A KEY, WHEEL OR BUTTON…",
                11.0,
                display_family(),
                ink(),
                0.08,
            );
            let w = (g.size().x + 20.0).min(width * 0.6);
            let r = Rect::from_min_size(pos2(x - w, row.center().y - 11.0), vec2(w, 22.0));
            let clip = painter.with_clip_rect(r);
            clip.rect_filled(r, 3.0, cream().gamma_multiply(pulse));
            clip.galley(r.min + vec2(10.0, 11.0 - g.size().y / 2.0), g, ink());
        } else if chips.is_empty() {
            let g = line(&painter, "none", 11.0, body(), hsla(45.0, 100.0, 95.0, 0.35), 0.02);
            let r = Rect::from_min_size(
                pos2(x - g.size().x - 14.0, row.center().y - chip_h / 2.0),
                vec2(g.size().x + 14.0, chip_h),
            );
            painter.rect_stroke(
                r,
                3.0,
                Stroke::new(1.5, hsla(45.0, 100.0, 95.0, 0.18)),
                egui::StrokeKind::Inside,
            );
            painter.galley(r.center() - g.size() / 2.0, g, cream());
        } else {
            for (code, g) in chips.iter().rev() {
                let w = (g.size().x + 14.0).max(26.0);
                let r = Rect::from_min_size(pos2(x - w, row.center().y - chip_h / 2.0 - 1.5), vec2(w, chip_h));
                x = r.left() - 5.0;
                if fixed {
                    painter.rect_stroke(r, 3.0, Stroke::new(1.5, c), egui::StrokeKind::Inside);
                } else {
                    painter.rect_filled(r.translate(vec2(0.0, 3.0)), 3.0, mix(c, 0.45, hsl(250.0, 50.0, 6.0)));
                    painter.rect_filled(r, 3.0, c);
                }
                painter.galley(r.center() - g.size() / 2.0, g.clone(), cream());
                // Taking a key off: a red badge on the chip's corner.
                if !fixed {
                    let chip = ui.interact(
                        r.expand(7.0),
                        ui.scope_id().with(("chip", action.id, code.as_str())),
                        egui::Sense::hover(),
                    );
                    if chip.hovered() {
                        let xr = Rect::from_center_size(r.right_top() + vec2(-1.0, 1.0), vec2(16.0, 16.0));
                        let x_hit = ui.interact(
                            xr,
                            ui.scope_id().with(("chip-x", action.id, code.as_str())),
                            egui::Sense::click(),
                        );
                        let fill = if x_hit.hovered() {
                            hsl(356.0, 95.0, 66.0)
                        } else {
                            hsl(356.0, 88.0, 56.0)
                        };
                        painter.circle_filled(xr.center(), 10.0, hsl(250.0, 42.0, 9.0));
                        painter.circle_filled(xr.center(), 8.0, fill);
                        let k = 3.0;
                        let cc = xr.center();
                        painter.line_segment([cc + vec2(-k, -k), cc + vec2(k, k)], Stroke::new(1.8, cream()));
                        painter.line_segment([cc + vec2(-k, k), cc + vec2(k, -k)], Stroke::new(1.8, cream()));
                        if x_hit.clicked() {
                            change = Some((code.clone(), None));
                        }
                    }
                }
            }
        }
        if change.is_none() && response.clicked() {
            self.pop = None;
            if let Some(f) = action.fixed {
                self.say_fixed(f.codes[0]);
            } else {
                self.listening = if listening { None } else { Some(action.id) };
            }
        }
        change
    }

    /// The key menu (`openPop`): a gold sheet over the page, asking what the
    /// key should do.
    fn pop_sheet(&mut self, ui: &egui::Ui, input: &InputSystem) -> Option<(String, Option<&'static str>)> {
        let (code, anchor) = self.pop.clone()?;
        let current = action_for(&code, input);
        let mut change = None;
        let mut close = false;
        let screen = ui.ctx().content_rect();
        let width = 700.0_f32.min(screen.width() * 0.92);
        let area = egui::Area::new(ui.scope_id().with("controls-pop"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(
                (anchor.center().x - width / 2.0).clamp(screen.left() + 16.0, screen.right() - width - 16.0),
                anchor.bottom() + 12.0,
            ))
            .show(ui.ctx(), |ui| {
                let painter = ui.painter().clone();
                let bg = painter.add(Shape::Noop);
                let top = ui.cursor().min;
                let inner_left = top.x + 30.0;
                let inner_w = width - 30.0 - 22.0;
                // The head: the key, the question, and ×.
                let key_g = line(&painter, &input_name(&code), 22.0, display_family(), gold_text(), 0.0);
                let key = Rect::from_min_size(
                    pos2(inner_left, top.y + 18.0),
                    vec2((key_g.size().x + 28.0).max(58.0), 54.0),
                );
                painter.rect_filled(key.translate(vec2(0.0, 5.0)), 3.0, hsl(252.0, 44.0, 3.0));
                gradient_polygon(
                    &painter,
                    &[key.left_top(), key.right_top(), key.right_bottom(), key.left_bottom()],
                    &[(0.0, hsl(250.0, 28.0, 21.0)), (1.0, hsl(252.0, 44.0, 7.0))],
                );
                painter.galley(key.center() - key_g.size() / 2.0, key_g, gold_text());
                let q = line(&painter, "WHAT SHOULD IT DO?", 22.0, display_family(), ink(), 0.04);
                painter.galley(pos2(key.right() + 16.0, key.center().y - q.size().y / 2.0), q, ink());
                let x = Rect::from_min_size(
                    pos2(top.x + width - 22.0 - 38.0, key.center().y - 19.0),
                    vec2(38.0, 38.0),
                );
                let xr = ui.interact(x, ui.scope_id().with("pop-close"), egui::Sense::click());
                let (bgc, fg) = if xr.hovered() {
                    (ink(), gold_text())
                } else {
                    (hsla(250.0, 45.0, 8.0, 0.12), ink())
                };
                painter.rect_filled(x, 3.0, bgc);
                let k = 6.0;
                painter.line_segment(
                    [x.center() + vec2(-k, -k), x.center() + vec2(k, k)],
                    Stroke::new(2.4, fg),
                );
                painter.line_segment(
                    [x.center() + vec2(-k, k), x.center() + vec2(k, -k)],
                    Stroke::new(2.4, fg),
                );
                if xr.clicked() {
                    close = true;
                }
                // Three columns: Move, Weapons, Fight and Other.
                let col_w = (inner_w - 40.0) / 3.0;
                let groups_top = key.bottom() + 5.0 + 16.0;
                let mut bottom = groups_top;
                for (col, ids) in [
                    &[ActionGroupId::Move][..],
                    &[ActionGroupId::Weapons],
                    &[ActionGroupId::Fight, ActionGroupId::Other],
                ]
                .iter()
                .enumerate()
                {
                    let cx = inner_left + col as f32 * (col_w + 20.0);
                    let mut y = groups_top;
                    for id in *ids {
                        let group = ACTION_GROUPS.iter().find(|g| g.id == *id).expect("a group");
                        let c = group_color(*id);
                        let t = line(
                            &painter,
                            &group.name.to_uppercase(),
                            13.0,
                            display_family(),
                            ink(),
                            0.18,
                        );
                        let sq = Rect::from_min_size(pos2(cx, y), vec2(14.0, 14.0));
                        painter.rect_filled(sq.expand(2.0), 0.0, ink());
                        painter.rect_filled(sq, 0.0, c);
                        painter.galley(pos2(cx + 22.0, y + 7.0 - t.size().y / 2.0), t, ink());
                        y += 14.0 + 8.0;
                        for action in SETTINGS_ACTIONS.iter().filter(|a| a.group == *id && a.fixed.is_none()) {
                            let r = Rect::from_min_size(pos2(cx, y), vec2(col_w, 36.0));
                            if option(
                                ui,
                                &painter,
                                r,
                                action.name,
                                c,
                                current.is_some_and(|x| x.id == action.id),
                            ) {
                                change = Some((code.clone(), Some(action.id)));
                            }
                            y += 36.0 + 8.0;
                        }
                        y += 10.0;
                    }
                    bottom = bottom.max(y);
                }
                // The foot: what it does now, and the way to clear it.
                let foot_top = bottom + 4.0;
                painter.hline(
                    inner_left..=(inner_left + inner_w),
                    foot_top,
                    Stroke::new(2.0, hsla(250.0, 45.0, 8.0, 0.14)),
                );
                let mut job = egui::text::LayoutJob::default();
                let fmt = |color| egui::TextFormat {
                    font_id: egui::FontId::new(15.0, body()),
                    color,
                    extra_letter_spacing: 0.9,
                    ..Default::default()
                };
                match current {
                    Some(c) => {
                        job.append("NOW: ", 0.0, fmt(hsla(250.0, 45.0, 8.0, 0.62)));
                        job.append(&c.name.to_uppercase(), 0.0, fmt(ink()));
                    }
                    None => job.append("DOES NOTHING YET", 0.0, fmt(hsla(250.0, 45.0, 8.0, 0.62))),
                }
                let hint = painter.layout_job(job);
                let foot_h = 40.0;
                let fy = foot_top + 14.0;
                painter.galley(pos2(inner_left, fy + foot_h / 2.0 - hint.size().y / 2.0), hint, ink());
                if current.is_some() {
                    let g = line(&painter, "Make it do nothing", 16.0, body(), ink(), 0.03);
                    let r = Rect::from_min_size(
                        pos2(inner_left + inner_w - g.size().x - 31.0, fy),
                        vec2(g.size().x + 31.0, 36.0),
                    );
                    if option(ui, &painter, r, "Make it do nothing", hsl(356.0, 88.0, 50.0), false) {
                        change = Some((code.clone(), None));
                    }
                }
                let sheet_bottom = fy + foot_h + 20.0;
                let sheet = Rect::from_min_max(top, pos2(top.x + width, sheet_bottom));
                let shape = [
                    sheet.left_top(),
                    pos2(sheet.right() - 26.0, sheet.top()),
                    pos2(sheet.right(), sheet.top() + 26.0),
                    sheet.right_bottom(),
                    sheet.left_bottom(),
                ];
                let mut shapes = vec![];
                for k in 1..=4 {
                    let k = k as f32;
                    shapes.push(Shape::rect_filled(
                        sheet.translate(vec2(0.0, 10.0 * k)).expand(6.0 * k),
                        12.0,
                        hsla(268.0, 80.0, 6.0, 0.1),
                    ));
                }
                shapes.push(super::settings_fields::mesh_polygon(
                    &shape,
                    &[(0.0, hsl(48.0, 100.0, 60.0)), (1.0, hsl(42.0, 97.0, 50.0))],
                ));
                shapes.push(super::settings_fields::mesh_polygon(
                    &[
                        sheet.left_top(),
                        pos2(sheet.left() + 8.0, sheet.top()),
                        pos2(sheet.left() + 8.0, sheet.bottom()),
                        sheet.left_bottom(),
                    ],
                    &[(0.0, hsl(276.0, 90.0, 64.0)), (1.0, hsl(262.0, 75.0, 42.0))],
                ));
                painter.set(bg, Shape::Vec(shapes));
                ui.allocate_rect(sheet, egui::Sense::hover());
            });
        // A click outside the sheet and its key closes it.
        let outside = ui.input(|i| i.pointer.any_pressed())
            && !area.response.contains_pointer()
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_none_or(|p| !anchor.contains(p));
        if close || change.is_some() || outside {
            self.pop = None;
        }
        change
    }
}

/// `.fps-set-opt`: a small cream key striped in its group's colour; the
/// current one pressed in and inked.
fn option(ui: &egui::Ui, painter: &Painter, r: Rect, label: &str, c: Color32, current: bool) -> bool {
    let response = ui.interact(
        r,
        ui.scope_id().with(("opt", label, r.min.x as i32, r.min.y as i32)),
        egui::Sense::click(),
    );
    let hovered = response.hovered() && !current;
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (face, top, foot, base, base_h, text) = if current {
        (
            r.translate(vec2(0.0, 3.0)),
            hsl(250.0, 28.0, 21.0),
            hsl(252.0, 44.0, 8.0),
            hsl(252.0, 44.0, 3.0),
            1.0,
            gold_text(),
        )
    } else if hovered {
        (
            r.translate(vec2(0.0, -2.0)),
            hsl(278.0, 92.0, 75.0),
            hsl(268.0, 72.0, 52.0),
            hsl(258.0, 72.0, 22.0),
            6.0,
            hsl(45.0, 100.0, 96.0),
        )
    } else {
        (
            r,
            hsl(45.0, 100.0, 97.0),
            hsl(42.0, 55.0, 86.0),
            hsl(33.0, 45.0, 40.0),
            4.0,
            ink(),
        )
    };
    painter.rect_filled(face.translate(vec2(0.0, base_h)), 2.0, base);
    gradient_polygon(
        painter,
        &[
            face.left_top(),
            face.right_top(),
            face.right_bottom(),
            face.left_bottom(),
        ],
        &[(0.0, top), (1.0, foot)],
    );
    painter.rect_filled(Rect::from_min_size(face.min, vec2(5.0, face.height())), 0.0, c);
    let g = line(painter, label, 16.0, body(), text, 0.03);
    painter.galley(pos2(face.left() + 18.0, face.center().y - g.size().y / 2.0), g, text);
    let _ = (gold(), Pos2::ZERO);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_keys_take_the_browser_s_names() {
        assert_eq!(egui_code(egui::Key::W).as_deref(), Some("KeyW"));
        assert_eq!(egui_code(egui::Key::Num1).as_deref(), Some("Digit1"));
        assert_eq!(egui_code(egui::Key::F4).as_deref(), Some("F4"));
        assert_eq!(egui_code(egui::Key::Space).as_deref(), Some("Space"));
        assert_eq!(egui_code(egui::Key::Backtick).as_deref(), Some("Backquote"));
    }

    #[test]
    fn escape_cancels_a_wait_for_a_key() {
        let mut page = ControlsPage {
            listening: Some(crate::client::fps_actions::fps::JUMP),
            ..ControlsPage::default()
        };
        assert!(page.take_input("Escape"));
        assert!(page.listening.is_none());
        assert!(!page.take_input("KeyQ"));
    }
}
