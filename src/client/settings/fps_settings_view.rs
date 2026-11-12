//! wash-fps `client/settings/FPSSettingsView.ts`: the settings screen,
//! saved as you go. The page keys belong to the view; the lobby draws them
//! in its rail with [`FpsSettingsView::page_nav`]. Resets ask once: the
//! first press arms the key, a second within a few seconds does it.
//!
//! The pages are their own modules, as in the browser: `controls_page`,
//! `controller_page`, `gameplay_page`, `video_page` and `audio_page`. Every
//! field reads and writes a cvar (or a binding), which saves itself.

use shagger::core::cvar::{CvarType, CvarValue, Cvars, with_cvars};
use shagger::input::input_system::{InputSystem, with_input};
use shagger::ui::components::lobby_twin::{clamp_css, cream, hsl, hsla, ink};
use shagger::ui::components::raised_key::{KeyLook, KeyText, KeyTextLayout, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Rect, Shape, pos2, vec2};

use super::audio_page;
use super::controller_page::ControllerPage;
use super::controls_page::ControlsPage;
use super::gameplay_page::GameplayPage;
use super::settings_fields::{Head, Range, body, line, seg, slider};
use super::settings_model::{
    CvarChoice, SETTINGS_PAGES, SettingsPageId, apply_choice, current_choice, reset_page, settings_page,
};
use super::video_page::VideoPage;
use crate::client::washoi_style::display_family;

/// `ARM_MS`: how long a reset key stays armed for its second press.
const ARM_SECONDS: f64 = 3.5;

/// The toast's life (`fps-set-toast`'s 2.6 s animation).
const TOAST_SECONDS: f64 = 2.7;

/// Which reset is armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reset {
    Page,
    All,
}

/// `FPSSettingsView`.
#[derive(Debug)]
pub struct FpsSettingsView {
    pub page: SettingsPageId,
    /// Over a match the Video page is locked, and resets leave it alone.
    pub in_match: bool,
    pub controls: ControlsPage,
    pub controller: ControllerPage,
    pub gameplay: GameplayPage,
    pub video: VideoPage,
    armed: Option<(Reset, f64)>,
    /// The line at the foot of the screen: bold part, rest, and when it goes.
    toast: Option<(String, String, f64)>,
}

impl Default for FpsSettingsView {
    fn default() -> Self {
        Self::new(false)
    }
}

impl FpsSettingsView {
    /// The screen on its first page, with the bindings as saved.
    #[must_use]
    pub fn new(in_match: bool) -> Self {
        with_input(InputSystem::reload_bindings_from_storage);
        Self {
            page: SettingsPageId::Controls,
            in_match,
            controls: ControlsPage::default(),
            controller: ControllerPage::default(),
            gameplay: GameplayPage::default(),
            video: VideoPage::default(),
            armed: None,
            toast: None,
        }
    }

    /// `showPage`.
    pub fn show_page(&mut self, id: SettingsPageId) {
        if self.page != id {
            self.controls.deactivate();
            self.controller.deactivate();
        }
        self.page = id;
        self.armed = None;
    }

    /// The backdrop word of the page shown.
    #[must_use]
    pub fn ghost(&self) -> &'static str {
        settings_page(self.page).ghost
    }

    /// `pauseControls`: drop a half-made key change.
    pub fn pause_controls(&mut self) {
        self.controls.deactivate();
        self.controller.deactivate();
    }

    /// Escape: closes what the page has open first. True when it did.
    pub fn back(&mut self) -> bool {
        self.controls.back()
            || self.controller.back()
            || (self.page == SettingsPageId::Video && self.video.close_section())
    }

    /// A page whose settings cannot change here: Video, over a match.
    fn is_locked(&self, page: SettingsPageId) -> bool {
        self.in_match && page == SettingsPageId::Video
    }

    /// The toast, as `toast(...parts)` does: `bold`, then `rest`.
    pub fn toast(&mut self, bold: &str, rest: &str, now: f64) {
        self.toast = Some((bold.to_owned(), rest.to_owned(), now + TOAST_SECONDS));
    }

    /// The page keys (`takePageNav`), a column in the rail at `rect`'s width,
    /// `key_h` tall each. Returns the height used.
    pub fn page_nav(&mut self, ui: &mut egui::Ui, rect: Rect, key_h: f32, label_size: f32, hints: bool) -> f32 {
        let painter = ui.painter().clone();
        let mut y = rect.top();
        for page in SETTINGS_PAGES {
            let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), key_h));
            let gold = KeyLook::gold().flat();
            let face = RaisedKey::new(("settings-page", page.title), r, gold.clone())
                .hover(gold, -0.35)
                .current(self.page == page.id)
                .show(ui);
            KeyText::new(page.title, if hints { page.hint } else { "" }, display_family(), body())
                .size(label_size, 12.0)
                .pad(24.0, 20.0)
                .paint(&painter, &face);
            if face.clicked {
                self.show_page(page.id);
            }
            y += key_h + 6.0 + 4.0;
        }
        y - rect.top()
    }

    /// The head and the page, in `rect`.
    pub fn ui(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let now = ui.input(|i| i.time);
        if self.armed.is_some_and(|(_, until)| now > until) {
            self.armed = None;
        }
        let painter = ui.painter().clone();
        // `fps-set-head`: kicker and title, then the saved note and resets.
        let kicker = line(&painter, "SETTINGS", 12.0, body(), hsl(190.0, 100.0, 60.0), 0.32);
        let kh = kicker.size().y;
        painter.galley(rect.min, kicker, cream());
        let title_size = clamp_css(36.0, ui.ctx().content_rect().width() * 0.034, 60.0);
        let title = line(
            &painter,
            settings_page(self.page).title,
            title_size,
            display_family(),
            cream(),
            0.0,
        );
        let ty = rect.top() + kh + 8.0;
        let th = title.size().y;
        painter.galley_with_override_text_color(
            pos2(rect.left() + 5.0, ty + 6.0),
            title.clone(),
            hsl(250.0, 50.0, 3.0),
        );
        painter.galley(pos2(rect.left(), ty), title, cream());
        let head_bottom = ty + th;
        let key_h = 40.0;
        let mut right = rect.right();
        let mut reset_key = |ui: &mut egui::Ui, which: Reset, label: String, disabled: bool| {
            let g = line(&painter, &label.to_uppercase(), 12.0, display_family(), ink(), 0.1);
            let w = g.size().x + 34.0 + 12.0;
            let r = Rect::from_min_size(pos2(right - w, head_bottom - key_h - 12.0), vec2(w, key_h));
            right = r.left() - 16.0;
            let armed = self.armed.is_some_and(|(a, _)| a == which);
            let look = if armed {
                KeyLook {
                    face: vec![
                        (0.0, hsl(356.0, 95.0, 68.0)),
                        (0.55, hsl(356.0, 85.0, 54.0)),
                        (1.0, hsl(352.0, 80.0, 44.0)),
                    ],
                    side: [hsl(350.0, 75.0, 30.0), hsl(345.0, 72.0, 18.0)],
                    text: hsl(45.0, 100.0, 96.0),
                    ..KeyLook::cream().sm()
                }
            } else {
                KeyLook::cream().sm()
            };
            let face = RaisedKey::new(("settings-reset", label.as_str()), r, look.clone())
                .disabled(disabled)
                .show(ui);
            KeyText::new(&label, "", display_family(), body())
                .size(12.0, 12.0)
                .spacing(0.1)
                .pad(4.0, 4.0)
                .layout(KeyTextLayout::Centered)
                .paint(&painter, &face);
            face.clicked
        };
        let page_title = settings_page(self.page).title;
        let all_label = if self.armed.is_some_and(|(a, _)| a == Reset::All) {
            "Sure? Everything".to_owned()
        } else {
            "Reset all".to_owned()
        };
        let page_label = if self.armed.is_some_and(|(a, _)| a == Reset::Page) {
            format!("Sure? {page_title}")
        } else {
            "Reset page".to_owned()
        };
        let all = reset_key(ui, Reset::All, all_label, false);
        let page = reset_key(ui, Reset::Page, page_label, self.is_locked(self.page));
        // `fps-set-saved`: a green dot, and the words.
        let saved_text = line(
            &painter,
            "SAVED AS YOU GO",
            12.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.45),
            0.2,
        );
        let sy = head_bottom - 12.0 - key_h / 2.0 - saved_text.size().y / 2.0;
        let sx = right - saved_text.size().x;
        painter.circle_filled(
            pos2(sx - 12.0, sy + saved_text.size().y / 2.0),
            3.0,
            hsl(96.0, 85.0, 55.0),
        );
        painter.galley(pos2(sx, sy), saved_text, cream());
        if page {
            self.arm_or_run(Reset::Page, now);
        } else if all {
            self.arm_or_run(Reset::All, now);
        }

        // The page, scrolling under the head.
        let pane = Rect::from_min_max(pos2(rect.left(), head_bottom + 18.0), rect.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(pane), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("settings-pane", self.page as u8))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(pane.width() - 14.0);
                    ui.add_space(4.0);
                    match self.page {
                        SettingsPageId::Controls => self.controls.ui(ui, now),
                        SettingsPageId::Controller => self.controller.ui(ui, now),
                        SettingsPageId::Gameplay => self.gameplay.ui(ui, now),
                        SettingsPageId::Video => self.video.ui(ui, self.in_match),
                        SettingsPageId::Audio => audio_page::ui(ui),
                    }
                    ui.add_space(12.0);
                });
        });
        for said in [self.controls.take_toast(), self.controller.take_toast()]
            .into_iter()
            .flatten()
        {
            self.toast = Some((String::new(), said, now + TOAST_SECONDS));
        }
        self.paint_toast(ui, now);
    }

    fn arm_or_run(&mut self, which: Reset, now: f64) {
        if self.armed.is_some_and(|(a, _)| a == which) {
            self.armed = None;
            with_cvars(|c| {
                with_input(|i| match which {
                    Reset::Page => reset_page(c, i, self.page),
                    Reset::All => {
                        for page in SETTINGS_PAGES {
                            if !self.is_locked(page.id) {
                                reset_page(c, i, page.id);
                            }
                        }
                    }
                });
            });
            match which {
                Reset::Page => self.toast(settings_page(self.page).title, " back to defaults", now),
                Reset::All if self.in_match => self.toast("Everything but Video", " back to defaults", now),
                Reset::All => self.toast("Everything", " back to defaults", now),
            }
            return;
        }
        self.armed = Some((which, now + ARM_SECONDS));
    }

    /// `.fps-set-toast`: cream, slanted, at the foot of the screen.
    fn paint_toast(&mut self, ui: &mut egui::Ui, now: f64) {
        let Some((bold, rest, until)) = &self.toast else { return };
        if now > *until {
            self.toast = None;
            return;
        }
        let age = (TOAST_SECONDS - (until - now)) / 2.6;
        let alpha = if age < 0.08 {
            age / 0.08
        } else if age > 0.85 {
            ((1.0 - age) / 0.15).max(0.0)
        } else {
            1.0
        } as f32;
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::NULL.with("settings-toast"),
        ));
        let b = line(&painter, bold, 16.0, display_family(), ink(), 0.04);
        let r = line(&painter, rest, 16.0, body(), ink(), 0.04);
        let w = b.size().x + r.size().x + 44.0;
        let h = b.size().y.max(r.size().y) + 24.0;
        let screen = ui.ctx().content_rect();
        let rect = Rect::from_min_size(
            pos2(screen.center().x - w / 2.0, screen.bottom() - 26.0 - h),
            vec2(w, h),
        );
        let cut = w * 0.03;
        painter.add(Shape::convex_polygon(
            vec![
                pos2(rect.left() + cut, rect.top()),
                rect.right_top(),
                pos2(rect.right() - cut, rect.bottom()),
                rect.left_bottom(),
            ],
            cream().gamma_multiply(alpha),
            egui::Stroke::NONE,
        ));
        let y = rect.center().y - b.size().y.max(r.size().y) / 2.0;
        let bw = b.size().x;
        painter.galley(pos2(rect.left() + 22.0, y), b, ink().gamma_multiply(alpha));
        painter.galley(pos2(rect.left() + 22.0 + bw, y), r, ink().gamma_multiply(alpha));
        ui.ctx().request_repaint();
    }
}

// ── Fields over cvars, for the pages ──

/// A cvar's default, as a number (`cvarDefault`).
#[must_use]
pub fn cvar_default(cvars: &Cvars, name: &str) -> Option<f64> {
    match cvars.get_cvar(name).map(|c| &c.default_value) {
        Some(CvarValue::Number(n)) => Some(*n),
        Some(CvarValue::Bool(b)) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

/// A cvar as a number, a switch as 0 or 1.
fn number_of(c: &Cvars, name: &str) -> f64 {
    if c.get_cvar(name).is_some_and(|v| v.kind == CvarType::Bool) {
        f64::from(u8::from(c.get_bool(name)))
    } else {
        c.get_number(name)
    }
}

/// `flag(name)`: true when the cvar is on.
#[must_use]
pub fn flag(name: &str) -> bool {
    with_cvars(|c| number_of(c, name) > 0.0)
}

/// `setFlag`: on or off, whether it is stored as a boolean or as 0 and 1.
pub fn set_flag(name: &str, on: bool) {
    with_cvars(|c| {
        if c.get_cvar(name).is_some_and(|v| v.kind == CvarType::Bool) {
            c.set(name, on)
        } else {
            c.set(name, i32::from(on))
        }
    });
}

/// A slider over a cvar, hidden when this client has no such cvar.
pub fn cvar_slider(ui: &mut egui::Ui, name: &str, cvar: &str, range: Range, help: Option<&str>, enabled: bool) {
    let Some((value, default)) = with_cvars(|c| c.exists(cvar).then(|| (c.get_number(cvar), cvar_default(c, cvar))))
    else {
        return;
    };
    if let Some(v) = slider(ui, name, value, range, default, help, enabled) {
        with_cvars(|c| c.set(cvar, v));
    }
}

/// A row of keys over a cvar's values, hidden when there is no such cvar.
pub fn cvar_seg(
    ui: &mut egui::Ui,
    head: Head<'_>,
    cvar: &str,
    choices: &[(f64, &str)],
    help: Option<&str>,
    enabled: bool,
) {
    let Some(value) = with_cvars(|c| c.exists(cvar).then(|| number_of(c, cvar))) else {
        return;
    };
    let labels: Vec<&str> = choices.iter().map(|c| c.1).collect();
    let current = choices.iter().position(|c| (c.0 - value).abs() < 1e-6);
    if let Some(i) = seg(ui, head, &labels, current, help, enabled) {
        let v = choices[i].0;
        with_cvars(|c| {
            if c.get_cvar(cvar).is_some_and(|x| x.kind == CvarType::Bool) {
                c.set(cvar, v > 0.0)
            } else {
                c.set(cvar, v)
            }
        });
    }
}

/// Off and On over a flag.
pub fn on_off(ui: &mut egui::Ui, head: Head<'_>, cvar: &str, help: Option<&str>, enabled: bool) {
    cvar_seg(ui, head, cvar, &[(0.0, "Off"), (1.0, "On")], help, enabled);
}

/// A row of keys over choices that each set several cvars.
pub fn choice_seg(ui: &mut egui::Ui, head: Head<'_>, choices: &[CvarChoice], help: Option<&str>) {
    let current = with_cvars(|c| current_choice(c, choices).map(|c| c.id));
    let labels: Vec<&str> = choices.iter().map(|c| c.label).collect();
    let at = choices.iter().position(|c| Some(c.id) == current);
    if let Some(i) = seg(ui, head, &labels, at, help, true) {
        with_cvars(|c| apply_choice(c, &choices[i]));
    }
}

/// The settings over a match (`inMatch`): the stage's backdrop, the page
/// keys and Back down the left, the page at the right. Video is locked.
/// True when Back was pressed.
pub fn over_match(ui: &mut egui::Ui, view: &mut FpsSettingsView, kicker: &str) -> bool {
    use shagger::ui::components::lobby_twin::{paint_backdrop, paint_light_in};
    let screen = ui.ctx().content_rect();
    let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::NULL.with("settings-over-match"));
    let mut back = false;
    let time = ui.input(|i| i.time);
    ui.scope_builder(egui::UiBuilder::new().layer_id(layer).max_rect(screen), |ui| {
        // Block the game under it.
        let _ = ui.interact(
            screen,
            egui::Id::NULL.with("settings-over-match-block"),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter().clone();
        paint_backdrop(&painter, screen);
        paint_light_in(&painter, screen, time, hsl(276.0, 90.0, 64.0));
        let left_w = (screen.width() * 0.2).clamp(240.0, 320.0);
        let side = Rect::from_min_size(
            screen.min + vec2(28.0, 30.0),
            vec2(left_w - 28.0, screen.height() - 60.0),
        );
        let k = line(
            &painter,
            &kicker.to_uppercase(),
            12.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.6),
            0.2,
        );
        let kh = k.size().y;
        painter.galley(side.min, k, cream());
        let used = view.page_nav(
            ui,
            Rect::from_min_size(side.min + vec2(0.0, kh + 22.0), vec2(side.width(), 1.0)),
            48.0,
            15.0,
            side.width() > 280.0,
        );
        let back_r = Rect::from_min_size(
            pos2(side.left(), side.top() + kh + 22.0 + used + 18.0),
            vec2(side.width(), 48.0),
        );
        let face = RaisedKey::new("settings-back", back_r, KeyLook::cream().sm()).show(ui);
        KeyText::new("Back", "Esc", display_family(), body())
            .size(15.0, 12.0)
            .pad(24.0, 20.0)
            .paint(&painter, &face);
        back = face.clicked;
        let content = Rect::from_min_max(
            pos2(screen.left() + left_w + 28.0, screen.top() + 30.0),
            screen.max - vec2(40.0, 30.0),
        );
        view.ui(ui, content);
    });
    ui.ctx().request_repaint();
    back
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_draws_and_over_a_match_video_is_locked() {
        let ctx = egui::Context::default();
        let mut view = FpsSettingsView::new(true);
        for page in SETTINGS_PAGES {
            view.show_page(page.id);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 900.0))),
                ..Default::default()
            };
            let mut back = true;
            let mut output = ctx.run_ui(input, |ui| back = over_match(ui, &mut view, "Duel · Dojo"));
            output.textures_delta.clear();
            assert!(!back);
            assert!(!output.shapes.is_empty());
        }
        assert!(view.is_locked(SettingsPageId::Video));
        assert!(!view.is_locked(SettingsPageId::Audio));
    }

    #[test]
    fn a_reset_asks_once() {
        let mut view = FpsSettingsView::new(false);
        view.arm_or_run(Reset::Page, 0.0);
        assert!(view.armed.is_some());
        assert!(view.toast.is_none());
        view.arm_or_run(Reset::Page, 1.0);
        assert!(view.armed.is_none());
        assert!(view.toast.as_ref().is_some_and(|t| t.1.contains("back to defaults")));
    }
}
