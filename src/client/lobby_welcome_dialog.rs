//! wash-fps `client/LobbyWelcomeDialog.ts`: the first-run guide. Three
//! pages (welcome and terms, your keys, a tip), the terms agreed to before
//! anything else, and "don't show this again".
//!
//! The browser's local storage entries (`washoi.terms.agreed`,
//! `washoi.welcome.hideAutomatically`) are entries of the same storage file
//! the bindings use; only synced keys ever leave the machine. The keyboard
//! picture of the keys page is the action list alone.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use shagger::input::input_system::with_input;
use shagger::lobby::settings_sync::{load_synced_storage, save_synced_storage_entry};
use shagger::ui::egui_layer::egui;

use crate::client::credits::markdown_lite::{Block, render_markdown, show};
use crate::client::fps_actions::fps;
use crate::client::settings::settings_model::{ActionGroupId, SETTINGS_ACTIONS, codes_for, input_name};

/// `WELCOME_PREFERENCE`.
pub const WELCOME_PREFERENCE: &str = "washoi.welcome.hideAutomatically";
/// `TERMS_PREFERENCE`: holds the `TERMS_VERSION` the player agreed to.
pub const TERMS_PREFERENCE: &str = "washoi.terms.agreed";
/// `TERMS.md`, as the browser bundles it.
pub const TERMS: &str = include_str!("../../assets/docs/TERMS.md");

/// `DEFAULT_FPS_MOVEMENT_CONFIG.slideMinSpeed`.
const SLIDE_MIN_SPEED: f64 = 375.0;

/// `TERMS_VERSION`: the terms' "Last Updated" date; new terms ask again.
#[must_use]
pub fn terms_version() -> &'static str {
    TERMS
        .split_once("**Last Updated:**")
        .map_or("", |(_, rest)| rest.lines().next().unwrap_or("").trim())
}

fn preference(dir: Option<&Path>, key: &str) -> Option<String> {
    load_synced_storage(dir?).remove(key)
}

/// `termsAgreed`.
#[must_use]
pub fn terms_agreed(dir: Option<&Path>) -> bool {
    preference(dir, TERMS_PREFERENCE).as_deref() == Some(terms_version())
}

/// `welcomeIsHidden`: never before the player agreed to the terms.
#[must_use]
pub fn welcome_is_hidden(dir: Option<&Path>) -> bool {
    terms_agreed(dir) && preference(dir, WELCOME_PREFERENCE).as_deref() == Some("true")
}

/// `writePreference`: false when it could not be saved.
pub(crate) fn write_preference(dir: Option<&Path>, key: &str, value: &str) -> bool {
    let Some(dir) = dir else { return false };
    save_synced_storage_entry(dir, key, Some(value));
    preference(Some(dir), key).as_deref() == Some(value)
}

fn terms_blocks() -> &'static [Block] {
    static BLOCKS: OnceLock<Vec<Block>> = OnceLock::new();
    BLOCKS.get_or_init(|| render_markdown(TERMS, 2))
}

/// What the dialog asks of the guide around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogEvent {
    Close,
    /// "Go to keyboard setup".
    Setup,
}

/// `LobbyWelcomeDialog`.
#[derive(Debug)]
pub struct LobbyWelcomeDialog {
    dir: Option<PathBuf>,
    pub tab: usize,
    /// Until the terms are agreed to, the guide stays on its first page and
    /// cannot be closed.
    agreed: bool,
    hide: bool,
    save_status: String,
    secondary_tip: usize,
}

/// `bindings(action)`: an action's keys as caps, or a note that it has none.
fn bindings(action: &str) -> String {
    let codes = with_input(|i| codes_for(action, i));
    if codes.is_empty() {
        return format!(
            "{} is unbound — open keyboard setup",
            if action == fps::JUMP { "Jump" } else { "Crouch" }
        );
    }
    let mut labels: Vec<String> = Vec::new();
    for code in codes
        .iter()
        .filter(|c| !c.contains(':'))
        .chain(codes.iter().filter(|c| c.contains(':')))
    {
        let name = input_name(code);
        if !labels.contains(&name) {
            labels.push(name);
        }
    }
    format!("[{}]", labels.join("] / ["))
}

impl LobbyWelcomeDialog {
    #[must_use]
    pub fn new(dir: Option<PathBuf>, tab: usize, secondary_tip: usize) -> Self {
        let agreed = terms_agreed(dir.as_deref());
        Self {
            hide: welcome_is_hidden(dir.as_deref()),
            dir,
            tab: if agreed { tab } else { 0 },
            agreed,
            save_status: String::new(),
            secondary_tip,
        }
    }

    fn page_welcome(&mut self, ui: &mut egui::Ui, strong: egui::Color32) {
        ui.heading("Fast movement. Loud weapons. One more round.");
        ui.label(
            "Welcome to Washoi ALPHA, an arena shooter where movement makes the difference. Before your first match, please read our terms: everything played on our servers is public.",
        );
        egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                show(ui, terms_blocks(), strong);
            });
        });
    }

    fn page_keys(ui: &mut egui::Ui) {
        ui.heading("Your controls, ready to go.");
        ui.weak("You can change these in keyboard setup.");
        with_input(|input| {
            for action in SETTINGS_ACTIONS.iter().filter(|a| a.group != ActionGroupId::Other) {
                let codes = codes_for(action.id, input);
                let keys = if codes.is_empty() {
                    "Unbound".to_owned()
                } else {
                    codes.iter().map(|c| input_name(c)).collect::<Vec<_>>().join(" / ")
                };
                ui.label(format!("• {}: {keys}", action.name));
            }
        });
    }

    fn page_tips(&self, ui: &mut egui::Ui) {
        ui.heading("Did you know?");
        let lesson = |ui: &mut egui::Ui, title: &str, text: String| {
            egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.strong(title);
                ui.label(text);
            });
        };
        lesson(
            ui,
            "Keep your momentum",
            format!(
                "Crouchsliding is essential. Build at least {SLIDE_MIN_SPEED} u/s, then press {} to slide and carry your speed through the arena.",
                bindings(fps::CROUCH)
            ),
        );
        lesson(
            ui,
            "Reach a little higher",
            format!(
                "Can't quite reach that ledge? While airborne beside a wall, hold {} to charge a wall jump, then release to push off.",
                bindings(fps::JUMP)
            ),
        );
        if self.secondary_tip == 0 {
            lesson(
                ui,
                "Land softer",
                format!(
                    "Hold {} before landing from a high fall to soften the impact.",
                    bindings(fps::CROUCH)
                ),
            );
        } else {
            lesson(
                ui,
                "Chain your movement",
                format!(
                    "Jump out of a crouchslide with {} to turn the slide into another boosted jump.",
                    bindings(fps::JUMP)
                ),
            );
        }
        ui.weak("Tips describe standard movement rules.");
    }

    /// The dialog over the lobby, which it keeps inert while open.
    pub fn ui(&mut self, ctx: &egui::Context, strong: egui::Color32) -> Option<DialogEvent> {
        let mut event = None;
        let modal = egui::Modal::new(egui::Id::unique("lobby-guide")).show(ctx, |ui| {
            ui.set_width(760.0_f32.min(ctx.content_rect().width() - 80.0));
            ui.label(egui::RichText::new("WASHOI / READY TO PLAY").small().color(strong));
            ui.label(egui::RichText::new("Find your flow.").size(34.0).strong());
            ui.horizontal(|ui| {
                for (index, name) in ["Welcome & terms", "Your keys", "Tip of the day"].iter().enumerate() {
                    let enabled = index == 0 || self.agreed;
                    let selected = self.tab == index;
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::selectable(selected, format!("{}. {name}", index + 1)),
                        )
                        .clicked()
                    {
                        self.tab = index;
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(460.0)
                .show(ui, |ui| match self.tab {
                    0 => self.page_welcome(ui, strong),
                    1 => Self::page_keys(ui),
                    _ => self.page_tips(ui),
                });
            if !self.save_status.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), &self.save_status);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if self.tab == 1 && ui.button("Go to keyboard setup").clicked() {
                    event = Some(DialogEvent::Setup);
                }
                if self.tab == 0 {
                    let mut agreed = self.agreed;
                    if ui.checkbox(&mut agreed, "I agree to the terms of service").changed() {
                        self.agreed = agreed;
                        let value = if agreed { terms_version() } else { "" };
                        let saved = write_preference(self.dir.as_deref(), TERMS_PREFERENCE, value);
                        self.save_status = if saved || !agreed {
                            String::new()
                        } else {
                            "Your agreement couldn't be saved. The terms will be asked again next time.".to_owned()
                        };
                    }
                }
                if self.tab == 2 {
                    let mut hide = self.hide;
                    if ui
                        .checkbox(&mut hide, "Don't show this again")
                        .on_hover_text("Reopen this guide with ? in the lobby.")
                        .changed()
                    {
                        self.hide = hide;
                        let saved = write_preference(self.dir.as_deref(), WELCOME_PREFERENCE, &hide.to_string());
                        self.save_status = match (saved, hide) {
                            (true, _) => String::new(),
                            (false, true) => {
                                "This choice couldn't be saved. The guide may appear next time.".to_owned()
                            }
                            (false, false) => {
                                "This choice couldn't be saved. Automatic display may still be disabled next time."
                                    .to_owned()
                            }
                        };
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.tab == 2 { "Close" } else { "Next »" };
                    let next = ui
                        .add_enabled(self.agreed, egui::Button::new(label))
                        .on_disabled_hover_text("Agree to the terms to continue");
                    if next.clicked() {
                        if self.tab == 2 {
                            event = Some(DialogEvent::Close);
                        } else {
                            self.tab += 1;
                        }
                    }
                });
            });
        });
        // Escape closes once the terms are agreed to.
        if modal.should_close() && self.agreed && event.is_none() {
            event = Some(DialogEvent::Close);
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiding_waits_for_the_terms() {
        assert_eq!(terms_version(), "October 22, 2026");
        let dir = std::env::temp_dir().join(format!("washoi-welcome-dialog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!terms_agreed(Some(&dir)));
        write_preference(Some(&dir), WELCOME_PREFERENCE, "true");
        assert!(!welcome_is_hidden(Some(&dir)));
        write_preference(Some(&dir), TERMS_PREFERENCE, terms_version());
        assert!(welcome_is_hidden(Some(&dir)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
