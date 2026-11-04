//! wash-fps `client/hud/widgets/PauseMenu.ts` (`FPSPauseMenu`): the Esc
//! menu, drawn over the match, which does not stop for it. Escape or a
//! lost mouse opens it (`FPSHud`'s `pointerlockchange`), Resume or Escape
//! takes the mouse back, and Leave match goes back to the lobby.
//!
//! Not yet: the Settings key (the settings live in the webview lobby, which
//! is hidden during a match), Spectate and Join, and the mode's keys
//! (Loadout).

use shagger::ui::egui_layer::egui;

use crate::client::washoi_style::{display_family, hsl};

/// What a key on the menu asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseMenuAction {
    /// Back to the fight: close and take the mouse again.
    Resume,
    /// Back to the lobby.
    Leave,
}

/// The menu and whether it is up.
#[derive(Debug, Clone, Default)]
pub struct PauseMenu {
    pub open: bool,
}

/// `fps-match-menu__note`.
pub const PAUSE_MENU_NOTE: &str = "The match keeps going while you're here.";

impl PauseMenu {
    /// `open`.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// `close`.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Draws the menu while it is open: `kicker` names the match
    /// (gametype · map) and `name` the player on the plate.
    pub fn ui(&self, ctx: &egui::Context, kicker: &str, name: &str) -> Option<PauseMenuAction> {
        if !self.open {
            return None;
        }
        let mut action = None;
        // The backdrop: the match, dimmed, still running behind.
        let screen = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::unique("pause-menu-dim"),
        ))
        .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(110));
        let gold = hsl(41.0, 96.0, 50.0);
        egui::Area::new(egui::Id::unique("pause-menu"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(hsl(252.0, 32.0, 11.0))
                    .stroke(egui::Stroke::new(2.0, gold))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_width(320.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("WASHOI")
                                    .font(egui::FontId::new(30.0, display_family()))
                                    .color(gold),
                            );
                            if !kicker.is_empty() {
                                ui.label(egui::RichText::new(kicker).strong());
                            }
                            ui.label(egui::RichText::new(PAUSE_MENU_NOTE).small().weak());
                            ui.add_space(12.0);
                            let key = |ui: &mut egui::Ui, label: &str, hint: &str, lit: bool| {
                                let text = egui::RichText::new(format!("{label}\n{hint}"));
                                let mut button = egui::Button::new(text).min_size(egui::vec2(280.0, 54.0));
                                if lit {
                                    button = button.fill(hsl(41.0, 96.0, 30.0));
                                }
                                ui.add(button).clicked()
                            };
                            if key(ui, "Resume", "Back to the fight", true) {
                                action = Some(PauseMenuAction::Resume);
                            }
                            ui.add_space(16.0);
                            ui.separator();
                            if !name.is_empty() {
                                ui.label(egui::RichText::new(format!("{name} · In a match")).small());
                            }
                            if key(ui, "Leave match", "Back to the lobby", false) {
                                action = Some(PauseMenuAction::Leave);
                            }
                        });
                    });
            });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of `menu`: what it asked and how many shapes it drew.
    fn frame(menu: &PauseMenu) -> (Option<PauseMenuAction>, usize) {
        let ctx = egui::Context::default();
        let mut action = Some(PauseMenuAction::Leave);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            action = menu.ui(ui.ctx(), "Deathmatch · dm2", "Player");
        });
        output.textures_delta.clear();
        (action, output.shapes.len())
    }

    #[test]
    fn a_closed_menu_draws_nothing() {
        assert_eq!(frame(&PauseMenu::default()), (None, 0));
    }

    #[test]
    fn an_open_menu_draws_and_waits_for_a_key() {
        let mut menu = PauseMenu::default();
        menu.open();
        let (action, shapes) = frame(&menu);
        // Nothing pressed, nothing asked.
        assert_eq!(action, None);
        assert!(shapes > 0);
        menu.close();
        assert!(!menu.open);
    }
}
