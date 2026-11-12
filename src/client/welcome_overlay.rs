//! wash-fps `client/WelcomeOverlay.ts`: the first thing a new player sees, a
//! name or the key they already have. The lobby has already made them an
//! account by now, so the name screen renames it and "Use my key" swaps it
//! for the one the key opens.
//!
//! The gold panel is the rail before it has a menu in it: it narrows to the
//! rail's width as the lobby takes over. Built on the Profile's frame and
//! fields (`identity_frame.rs`, `identity_fields.rs`).

use std::time::{Duration, Instant};

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::lobby::player_identity::{KEY_LENGTH, normalize_key};
use shagger::ui::components::lobby_twin::{RAIL_SKEW, TwinStyle, paint_brand};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Rect, pos2, vec2};

use crate::client::identity_fields::{Slot, SlotLook, Tone, judge_name, show_slot, tidy_key_slot};
use crate::client::identity_frame::{IdentityFrame, alt_row, gap, go_key, go_key_width, note, screen_head, warn};
use crate::client::profile_view::{Account, ProfileAction, rename_failure_text};

const NAME_RULES: &str = "2–16 characters: letters, numbers, _ and -";
/// `ANSWER_TIMEOUT_MS`.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Name,
    Restore,
}

/// `WelcomeOverlay`.
#[derive(Debug)]
pub struct WelcomeOverlay {
    frame: IdentityFrame,
    screen: Screen,
    ghost: String,
    content_h: f32,
    name_slot: Slot,
    key_slot: Slot,
    renaming: Option<Instant>,
    switching: bool,
    /// A key that worked: go once the greeting has been seen.
    leave_at: Option<Instant>,
    focus: Option<&'static str>,
}

impl Default for WelcomeOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl WelcomeOverlay {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame: IdentityFrame::default(),
            screen: Screen::Name,
            ghost: "HELLO".into(),
            content_h: 0.0,
            name_slot: Slot::new(NAME_RULES),
            key_slot: Slot::new(""),
            renaming: None,
            switching: false,
            leave_at: None,
            focus: None,
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.frame.is_open()
    }

    /// Open and not leaving.
    #[must_use]
    pub fn showing(&self) -> bool {
        self.frame.is_open() && !self.frame.closing()
    }

    #[must_use]
    pub fn covers(&self) -> bool {
        self.frame.covers()
    }

    /// `open`: over the lobby, at the name.
    pub fn open(&mut self, account: &Account<'_>) {
        if self.showing() {
            return;
        }
        *self = Self::new();
        self.frame.open_now();
        self.show(Screen::Name, account);
    }

    /// Opens on "Use my key": for screenshots.
    pub fn open_restore(&mut self, account: &Account<'_>) {
        self.open(account);
        self.show(Screen::Restore, account);
    }

    /// Esc, or a controller's back: to the name.
    pub fn back(&mut self, account: &Account<'_>) {
        self.show(Screen::Name, account);
    }

    /// `keepName`: the name the lobby already gave, kept if nothing is typed.
    fn keep_name(account: &Account<'_>) -> String {
        if account.name.is_empty() {
            NAME_RULES.into()
        } else {
            format!("Leave it empty to play as {}", account.name)
        }
    }

    fn show(&mut self, screen: Screen, account: &Account<'_>) {
        self.screen = screen;
        self.content_h = 0.0;
        match screen {
            Screen::Name => {
                self.ghost = self.name_ghost();
                if self.renaming.is_none() {
                    self.name_slot.tell(Self::keep_name(account), Tone::Plain);
                }
                self.focus = Some("welcome-name");
            }
            Screen::Restore => {
                "AGAIN".clone_into(&mut self.ghost);
                if !self.switching && self.key_slot.say.is_empty() {
                    self.key_slot.tell(
                        format!("{KEY_LENGTH} characters. Dashes and spaces don't matter."),
                        Tone::Plain,
                    );
                }
                self.focus = Some("welcome-key");
            }
        }
    }

    fn name_ghost(&self) -> String {
        let typed = self.name_slot.value.trim().to_uppercase();
        if typed.is_empty() { "HELLO".into() } else { typed }
    }

    /// `leave`: narrow onto the rail; the lobby is the player's again.
    fn leave(&mut self) {
        self.frame.close();
    }

    /// `takeName`: nothing typed keeps the lobby's name; a good one is asked for.
    pub fn take_name(&mut self, client: &mut LobbyClient) {
        let name = self.name_slot.value.trim().to_owned();
        if name.is_empty() {
            self.leave();
            return;
        }
        if !judge_name(&name).0 || self.renaming.is_some() {
            return;
        }
        self.renaming = Some(Instant::now());
        self.name_slot.tell("Asking the lobby…", Tone::Plain);
        client.rename(&name);
    }

    /// `useKey`: the pasted key, for the panel to log in with.
    pub fn use_key(&mut self) -> Option<ProfileAction> {
        if self.switching || normalize_key(&self.key_slot.value).is_none() {
            return None;
        }
        self.switching = true;
        self.key_slot.tell("Asking the lobby…", Tone::Plain);
        Some(ProfileAction::Switch(self.key_slot.value.clone()))
    }

    /// Types into the screen's slot, as the player would: for tests.
    pub fn type_into(&mut self, restore: bool, text: &str, account: &Account<'_>) {
        if restore {
            self.show(Screen::Restore, account);
            text.clone_into(&mut self.key_slot.value);
            tidy_key_slot(&mut self.key_slot);
        } else {
            text.clone_into(&mut self.name_slot.value);
            self.name_typed(account);
        }
    }

    fn name_typed(&mut self, account: &Account<'_>) {
        let typed = self.name_slot.value.trim().to_owned();
        if typed.is_empty() {
            self.name_slot.tell(Self::keep_name(account), Tone::Plain);
        } else {
            let (_, text, tone) = judge_name(&typed);
            self.name_slot.tell(text, tone);
        }
        self.ghost = self.name_ghost();
    }

    /// The lobby's answers to a rename.
    pub fn on_message(&mut self, kind: &str, message: &Value) {
        match kind {
            "identity:renamed" if self.renaming.take().is_some() => {
                if let Some(name) = message.pointer("/identity/name").and_then(Value::as_str) {
                    self.ghost = name.to_uppercase();
                }
                self.leave();
            }
            "identity:error" if self.renaming.take().is_some() => {
                let code = message.get("code").and_then(Value::as_str).unwrap_or("error");
                self.name_slot.tell(rename_failure_text(code), Tone::Bad);
            }
            _ => {}
        }
    }

    /// The connection dropped: a rename waiting on it fails.
    pub fn disconnected(&mut self) {
        if self.renaming.take().is_some() {
            self.name_slot.tell(rename_failure_text("not_connected"), Tone::Bad);
        }
    }

    /// The new connection's answer to "Use my key".
    pub fn switch_answered(&mut self, answer: Result<String, String>) {
        self.switching = false;
        match answer {
            Ok(name) => {
                self.key_slot.tell(format!("Welcome back, {name}"), Tone::Ok);
                self.ghost = name.to_uppercase();
                self.leave_at = Some(Instant::now() + Duration::from_millis(600));
            }
            Err(why) => self.key_slot.tell(why, Tone::Bad),
        }
    }

    /// The status line of a screen's slot.
    #[must_use]
    pub fn said(&self, restore: bool) -> (&str, Tone) {
        let slot = if restore { &self.key_slot } else { &self.name_slot };
        (&slot.say, slot.tone)
    }

    /// Answers that never came, and a greeting that has been seen.
    pub fn tick(&mut self) {
        if self.renaming.is_some_and(|t| t.elapsed() > ANSWER_TIMEOUT) {
            self.renaming = None;
            self.name_slot.tell(rename_failure_text("timeout"), Tone::Bad);
        }
        if self.leave_at.is_some_and(|at| Instant::now() >= at) {
            self.leave_at = None;
            self.leave();
        }
    }

    /// Draws the overlay while it is open.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        screen: Rect,
        time: f64,
        accent: Color32,
        style: &TwinStyle,
        account: &Account<'_>,
        client: &mut LobbyClient,
    ) -> Option<ProfileAction> {
        if self.frame.finished() || !self.frame.is_open() {
            return None;
        }
        if self.showing() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.back(account);
        }
        let mut action = None;
        ui.scope(|ui| {
            let _ = ui.interact(screen, egui::Id::NULL.with("welcome-block"), egui::Sense::click());
            let layout = self
                .frame
                .paint(ui, screen, time, &self.ghost, false, accent, |_, _| {});
            ui.set_opacity(ui.opacity() * layout.inner_alpha);
            let scroll = Rect::from_min_max(
                layout.panel.min,
                pos2(layout.panel.right() - RAIL_SKEW - 8.0, layout.panel.bottom()),
            );
            let pad = layout.column.top() - layout.panel.top();
            let total = (self.content_h + 2.0 * pad).max(scroll.height());
            ui.scope_builder(egui::UiBuilder::new().max_rect(scroll), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("welcome", self.screen as u8))
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let (r, _) = ui.allocate_exact_size(vec2(scroll.width(), total), egui::Sense::hover());
                        let top = r.top() + pad + ((scroll.height() - 2.0 * pad - self.content_h) / 2.0).max(0.0);
                        let (left, width) = (layout.column.left(), layout.column.width());
                        let bottom =
                            self.screen_ui(ui, left, top, width, layout.short, style, account, client, &mut action);
                        self.content_h = bottom - top;
                    });
            });
        });
        if self.frame.covers()
            && let Some(id) = self.focus.take()
        {
            ui.ctx()
                .memory_mut(|m| m.request_focus(egui::Id::NULL.with(("identity-slot", id))));
        }
        action
    }

    #[allow(clippy::too_many_arguments)]
    fn screen_ui(
        &mut self,
        ui: &mut egui::Ui,
        left: f32,
        top: f32,
        width: f32,
        short: bool,
        style: &TwinStyle,
        account: &Account<'_>,
        client: &mut LobbyClient,
        action: &mut Option<ProfileAction>,
    ) -> f32 {
        let painter = ui.painter().clone();
        let screen_w = ui.ctx().content_rect().width();
        let logo_w = width.min(if short { 300.0 } else { 460.0 });
        let brand_bottom = paint_brand(
            &painter,
            left - 8.0 - 22.0,
            logo_w + RAIL_SKEW + 38.0,
            top,
            None,
            screen_w,
            style,
            1.0,
        );
        let mut y = if style.logo.is_some() {
            brand_bottom - 16.0 + if short { 4.0 } else { 18.0 }
        } else {
            brand_bottom
        };
        let g = gap(short);
        match self.screen {
            Screen::Name => {
                y = screen_head(&painter, left, y, width, short, "Welcome", "Who are you?", &[]) + g;
                let look = SlotLook {
                    placeholder: "Your name",
                    max_chars: Some(16),
                    counted: true,
                    ..SlotLook::default()
                };
                let r = show_slot(
                    ui,
                    "welcome-name",
                    left,
                    y,
                    width,
                    short,
                    &mut self.name_slot,
                    look,
                    true,
                );
                if r.changed {
                    self.name_typed(account);
                }
                y = r.bottom + g;
                let typed = self.name_slot.value.trim().to_owned();
                let disabled = (!typed.is_empty() && !judge_name(&typed).0) || self.renaming.is_some();
                let (go, bottom) = go_key(ui, "welcome-play", left, y, short, "Play", "That's it", true, disabled);
                // `welcome__go`'s gap of 30 to the note.
                let key_w = go_key_width(&painter, "Play", "That's it", true);
                note(
                    &painter,
                    left + key_w + 10.2 * 0.75 + 4.0 + 30.0,
                    y + 8.0 + if short { 37.0 } else { 46.0 },
                    &["No email.", "No password."],
                );
                if (go || r.submitted) && !disabled {
                    self.take_name(client);
                }
                y = bottom + g;
                let (pressed, bottom) = alt_row(ui, "welcome-alt", left, y, width, "Played before?", &["Use my key"]);
                if pressed.is_some() {
                    self.show(Screen::Restore, account);
                }
                y = bottom;
            }
            Screen::Restore => {
                y = screen_head(
                    &painter,
                    left,
                    y,
                    width,
                    short,
                    "Welcome back",
                    "Paste your key",
                    &[("Your name comes back with it.", false)],
                ) + g;
                let look = SlotLook {
                    placeholder: "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX",
                    prefix: Some("WASHOI-"),
                    ..SlotLook::default()
                };
                let r = show_slot(ui, "welcome-key", left, y, width, short, &mut self.key_slot, look, true);
                if r.changed {
                    tidy_key_slot(&mut self.key_slot);
                }
                y = r.bottom + g;
                let whole = normalize_key(&self.key_slot.value).is_some();
                let disabled = !whole || self.switching || !account.connected;
                let (go, bottom) = go_key(
                    ui,
                    "welcome-restore",
                    left,
                    y,
                    short,
                    "Restore",
                    "Back to it",
                    true,
                    disabled,
                );
                if (go || r.submitted) && !disabled {
                    *action = self.use_key();
                }
                y = bottom + g;
                let (pressed, bottom) = alt_row(ui, "welcome-new", left, y, width, "New here?", &["Pick a name"]);
                if pressed.is_some() {
                    self.show(Screen::Name, account);
                }
                y = bottom;
            }
        }
        if account.key_unsaved {
            y = warn(
                &painter,
                left,
                y + g,
                width,
                "",
                "Your key could not be saved on this computer. After choosing your name, open Profile and copy \
                 or save your key before quitting.",
            );
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account<'static> {
        Account {
            name: "Player_42",
            key: None,
            key_unsaved: false,
            id: Some("a1"),
            connected: true,
            in_room: false,
            modes: &[],
        }
    }

    #[test]
    fn nothing_typed_keeps_the_lobby_s_name() {
        let mut client = LobbyClient::connect("ws://127.0.0.1:9");
        let mut welcome = WelcomeOverlay::new();
        welcome.open(&account());
        assert!(welcome.covers());
        assert_eq!(welcome.said(false).0, "Leave it empty to play as Player_42");
        welcome.take_name(&mut client);
        assert!(!welcome.showing(), "it leaves");
    }

    #[test]
    fn a_name_is_asked_for_and_the_overlay_leaves_when_it_is_taken() {
        let mut client = LobbyClient::connect("ws://127.0.0.1:9");
        let mut welcome = WelcomeOverlay::new();
        welcome.open(&account());
        welcome.type_into(false, "Tarmo", &account());
        assert_eq!(welcome.ghost, "TARMO");
        welcome.take_name(&mut client);
        assert_eq!(welcome.said(false).0, "Asking the lobby…");
        welcome.on_message("identity:error", &serde_json::json!({ "code": "name_taken" }));
        assert_eq!(welcome.said(false), ("Someone already has that name.", Tone::Bad));
        welcome.take_name(&mut client);
        welcome.on_message(
            "identity:renamed",
            &serde_json::json!({ "identity": { "name": "Tarmo" } }),
        );
        assert!(!welcome.showing());
    }

    #[test]
    fn a_whole_key_is_handed_to_the_panel() {
        let mut welcome = WelcomeOverlay::new();
        welcome.open(&account());
        welcome.type_into(true, "0123 4567", &account());
        assert_eq!(welcome.use_key(), None);
        welcome.type_into(true, "0123456789ABCDEFGHJKMNPQ", &account());
        assert!(matches!(welcome.use_key(), Some(ProfileAction::Switch(_))));
        welcome.switch_answered(Err("No player has that key.".into()));
        assert_eq!(welcome.said(true).1, Tone::Bad);
    }
}
