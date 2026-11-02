//! wash-fps `client/settings/ControlsPage.ts` (and `InputDiagram.ts`'s
//! board): the keyboard and mouse coloured by what each key does, the list of
//! actions with their keys, and how the mouse feels.
//!
//! A key is changed from either end: click the key and pick what it does,
//! or click an action and press the key, click or wheel turn you want for it.
//! The mouse is a row of its parts here rather than a drawing.

use shagger::input::input_system::{InputSystem, with_input};
use shagger::ui::egui_layer::egui;

use super::fps_settings_view::card;
use super::settings_model::{
    ACTION_GROUPS, ARROW_KEYS, AssignResult, MOUSE_INPUTS, SettingsAction, action_for, assign_key, code_name,
    codes_for, fixed_key, group_of, input_name, keyboard_rows,
};

/// `hsl(h s% l%)` as a colour.
#[must_use]
pub fn hsl(h: f32, s: f32, l: f32, alpha: f32) -> egui::Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(byte(r), byte(g), byte(b), (alpha * 255.0).round() as u8)
}

fn group_color(action: &SettingsAction, alpha: f32) -> egui::Color32 {
    let [h, s, l] = group_of(action).hsl;
    hsl(h, s, l, alpha)
}

/// `ControlsPage`.
#[derive(Debug, Default)]
pub struct ControlsPage {
    /// The action waiting for a key.
    pub listening: Option<&'static str>,
    /// The key whose menu is open, and where.
    pop: Option<(String, egui::Pos2)>,
    /// The line at the bottom of the screen, and when it goes.
    toast: Option<(String, f64)>,
    /// The action pointed at, whose keys light up.
    focus: Option<&'static str>,
    /// This frame's time, seconds.
    now: f64,
}

impl ControlsPage {
    /// `deactivate`: leaving the page drops a half-made change.
    pub fn deactivate(&mut self) {
        self.pop = None;
        self.listening = None;
    }

    /// Press-to-bind (`listen`): a key, button or wheel turn under the
    /// browser's names. True when the page took it. Escape cancels.
    pub fn take_input(&mut self, code: &str, now: f64) -> bool {
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
        // A left click is the page's own; Fire gets it from the mouse row.
        if code == "mouse:left" {
            return false;
        }
        if fixed_key(code).is_some() {
            self.say_fixed(code, now);
            return true;
        }
        let id = self.listening.take();
        self.assign(code, id, now);
        true
    }

    fn assign(&mut self, code: &str, id: Option<&str>, now: f64) {
        let result = with_input(|i| assign_key(code, id, i));
        let key = code_name(code);
        let text = match result {
            AssignResult::Fixed(_) => return self.say_fixed(code, now),
            AssignResult::Cleared { .. } => format!("{key} does nothing now"),
            AssignResult::Assigned {
                action,
                previous: Some(p),
            } if p.id != action.id => format!("{key} was {}, now {}", p.name, action.name),
            AssignResult::Assigned { previous: Some(_), .. } => return,
            AssignResult::Assigned { action, previous: None } => format!("{key} → {}", action.name),
        };
        self.toast = Some((text, now + 3.0));
    }

    fn say_fixed(&mut self, code: &str, now: f64) {
        if let Some(f) = fixed_key(code).and_then(|a| a.fixed) {
            self.toast = Some((
                format!("{} always {}. That can't be changed yet", code_name(code), f.does),
                now + 3.0,
            ));
        }
    }

    fn click_input(&mut self, code: &str, at: egui::Pos2, now: f64) {
        if fixed_key(code).is_some() {
            self.say_fixed(code, now);
        } else {
            self.listening = None;
            self.pop = Some((code.to_owned(), at));
        }
    }

    /// One key cap or mouse part.
    fn input_cap(
        &mut self,
        ui: &mut egui::Ui,
        input: &InputSystem,
        code: &str,
        legend: &str,
        size: egui::Vec2,
        narrow: bool,
    ) {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let action = action_for(code, input);
        let lit = self
            .focus
            .is_some_and(|f| codes_for(f, input).iter().any(|c| c == code));
        let open = self.pop.as_ref().is_some_and(|(c, _)| c == code);
        let (fill, stroke) = match action {
            Some(a) => (
                group_color(a, if lit || open { 0.55 } else { 0.28 }),
                group_color(a, 1.0),
            ),
            None => (egui::Color32::from_gray(28), egui::Color32::from_gray(60)),
        };
        let painter = ui.painter_at(rect);
        let fill = if response.hovered() {
            fill.gamma_multiply(1.4)
        } else {
            fill
        };
        painter.rect(
            rect,
            4.0,
            fill,
            egui::Stroke::new(if lit || open { 2.0 } else { 1.0 }, stroke),
            egui::StrokeKind::Inside,
        );
        let text = ui.visuals().text_color();
        painter.text(
            rect.left_top() + egui::vec2(5.0, 3.0),
            egui::Align2::LEFT_TOP,
            legend,
            egui::FontId::proportional(12.0),
            text,
        );
        if let Some(a) = action {
            let label = if narrow {
                a.on_narrow_key.or(a.on_key).unwrap_or(a.name)
            } else {
                a.on_key.unwrap_or(a.name)
            };
            painter.text(
                rect.left_bottom() + egui::vec2(5.0, -3.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(10.0),
                group_color(a, 1.0),
            );
        }
        if response.hovered() {
            self.focus = action.map(|a| a.id);
        }
        if response.clicked() {
            self.click_input(code, rect.center_bottom(), self.now);
        }
    }

    fn keyboard(&mut self, ui: &mut egui::Ui, input: &InputSystem) {
        let gap = 4.0;
        let unit = ((ui.available_width() - 14.0 * gap) / 15.0).clamp(26.0, 52.0);
        let narrow = unit < 44.0;
        ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
        for row in keyboard_rows() {
            ui.horizontal(|ui| {
                for (code, legend, width) in row {
                    let w = width * unit + (width - 1.0).max(0.0) * gap;
                    self.input_cap(ui, input, &code, &legend, egui::vec2(w, unit), narrow);
                }
            });
        }
        ui.horizontal(|ui| {
            for (code, legend) in ARROW_KEYS {
                self.input_cap(ui, input, code, legend, egui::vec2(unit, unit), narrow);
            }
        });
    }

    fn mouse(&mut self, ui: &mut egui::Ui, input: &InputSystem) {
        ui.horizontal_wrapped(|ui| {
            for (code, name) in MOUSE_INPUTS {
                self.input_cap(ui, input, code, name, egui::vec2(110.0, 44.0), false);
            }
        });
    }

    /// `renderActs`: the actions by group, with their keys.
    fn actions(&mut self, ui: &mut egui::Ui, input: &InputSystem) -> Option<(String, Option<&'static str>)> {
        let mut change = None;
        ui.columns(ACTION_GROUPS.len(), |columns| {
            for (column, group) in columns.iter_mut().zip(ACTION_GROUPS) {
                let [h, s, l] = group.hsl;
                column.label(egui::RichText::new(group.name).strong().color(hsl(h, s, l, 1.0)));
                for action in super::settings_model::SETTINGS_ACTIONS
                    .iter()
                    .filter(|a| a.group == group.id)
                {
                    let codes = codes_for(action.id, input);
                    let listening = self.listening == Some(action.id);
                    let response = egui::Frame::NONE
                        .inner_margin(4.0)
                        .fill(if listening {
                            group_color(action, 0.25)
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .show(column, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let mut name = egui::RichText::new(action.name);
                                if action.fixed.is_some() {
                                    name = name.weak();
                                }
                                ui.label(name);
                                for code in &codes {
                                    let chip = egui::RichText::new(code_name(code)).color(group_color(action, 1.0));
                                    if action.fixed.is_some() {
                                        ui.label(chip.weak());
                                    } else if ui
                                        .small_button(chip)
                                        .on_hover_text(format!("Take {} off {}", code_name(code), action.name))
                                        .clicked()
                                    {
                                        change = Some((code.clone(), None));
                                    }
                                }
                                if codes.is_empty() {
                                    ui.weak("none");
                                }
                                if action.fixed.is_some() {
                                    ui.weak("Fixed");
                                }
                                if listening {
                                    ui.label("Press a key, wheel or button…");
                                }
                            });
                        })
                        .response
                        .interact(egui::Sense::click());
                    if response.hovered() {
                        self.focus = Some(action.id);
                    }
                    if response.clicked() {
                        self.pop = None;
                        if action.fixed.is_none() {
                            self.listening = if listening { None } else { Some(action.id) };
                        }
                    }
                }
            }
        });
        change
    }

    /// The key menu (`openPop`): what should this key do?
    fn pop(&mut self, ctx: &egui::Context, input: &InputSystem) -> Option<(String, Option<&'static str>)> {
        let (code, at) = self.pop.clone()?;
        let current = action_for(&code, input);
        let mut change = None;
        let mut close = false;
        egui::Area::new(egui::Id::unique("controls-pop"))
            .order(egui::Order::Foreground)
            .fixed_pos(at + egui::vec2(-160.0, 8.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(input_name(&code));
                        ui.label("What should it do?");
                        if ui.small_button("×").clicked() {
                            close = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        for group in ACTION_GROUPS {
                            ui.vertical(|ui| {
                                let [h, s, l] = group.hsl;
                                ui.label(egui::RichText::new(group.name).strong().color(hsl(h, s, l, 1.0)));
                                for action in super::settings_model::SETTINGS_ACTIONS
                                    .iter()
                                    .filter(|a| a.group == group.id && a.fixed.is_none())
                                {
                                    let is_current = current.is_some_and(|c| c.id == action.id);
                                    if ui.selectable_label(is_current, action.name).clicked() {
                                        change = Some((code.clone(), Some(action.id)));
                                    }
                                }
                            });
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| match current {
                        Some(c) => {
                            ui.label(format!("Now: {}", c.name));
                            if ui.button("Make it do nothing").clicked() {
                                change = Some((code.clone(), None));
                            }
                        }
                        None => {
                            ui.weak("Does nothing yet");
                        }
                    });
                });
            });
        if close || change.is_some() {
            self.pop = None;
        }
        change
    }

    /// The keyboard, the mouse and the action list (the mouse feel card is
    /// the view's).
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        self.now = now;
        self.focus = None;
        let mut change = None;
        with_input(|input| {
            card(ui, "Keyboard · click a key to choose what it does", |ui| {
                self.keyboard(ui, input);
            });
            card(ui, "Mouse", |ui| self.mouse(ui, input));
            card(
                ui,
                "What each key does · click an action, then press the key you want",
                |ui| change = self.actions(ui, input),
            );
            if let Some(c) = self.pop(ui.ctx(), input) {
                change = Some(c);
            }
        });
        if let Some((code, id)) = change {
            self.assign(&code, id, now);
        }
        if let Some((text, until)) = &self.toast {
            if now < *until {
                egui::Area::new(egui::Id::unique("controls-toast"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(text.as_str()));
                    });
            } else {
                self.toast = None;
            }
        }
    }
}
