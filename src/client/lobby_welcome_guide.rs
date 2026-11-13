//! wash-fps `client/LobbyWelcomeGuide.ts`: when the welcome guide opens.
//! Once a boot, after the lobby has let the player in and nothing else is
//! over the lobby, unless the player turned it off (never before they agreed
//! to the terms). The `?` key in the links bar opens it again, and "Go to
//! keyboard setup" sets it aside behind a "Back to welcome" key.

use std::sync::atomic::{AtomicBool, Ordering};

use shagger::ui::egui_layer::egui;

use crate::client::lobby_welcome_dialog::{GuideAnswer, LobbyWelcomeDialog, welcome_is_hidden};

/// `welcomeBootState.completed`: this boot has had its guide. Kept across
/// lobby panels, as the web keeps it across lobby connections.
static COMPLETED: AtomicBool = AtomicBool::new(false);

/// What the guide asks of the lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideEvent {
    /// `openControls`: the settings, on the keys, with the back key.
    OpenControls,
    /// A link in the terms, for the system's browser.
    Open(String),
}

/// `LobbyWelcomeGuide`.
#[derive(Debug)]
pub struct LobbyWelcomeGuide {
    pub dialog: Option<LobbyWelcomeDialog>,
    authenticated: bool,
    manual: bool,
    tab: usize,
    secondary_tip: usize,
    /// Set aside for the keyboard setup.
    pub suspended: bool,
}

impl Default for LobbyWelcomeGuide {
    fn default() -> Self {
        // `Math.random() < 0.5 ? 0 : 1`.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        Self {
            dialog: None,
            authenticated: false,
            manual: false,
            tab: 0,
            secondary_tip: (nanos / 1000 % 2) as usize,
            suspended: false,
        }
    }
}

impl LobbyWelcomeGuide {
    /// `setAuthenticated`.
    pub fn set_authenticated(&mut self, value: bool) {
        self.authenticated = value;
    }

    /// Whether the `?` key shows: let in, and nothing else over the lobby.
    #[must_use]
    pub fn help_shown(&self, can_open: bool) -> bool {
        self.authenticated && can_open
    }

    /// The `?` key.
    pub fn help(&mut self, can_open: bool) {
        if !self.help_shown(can_open) || self.dialog.is_some() {
            return;
        }
        self.suspended = false;
        self.manual = true;
        self.tab = 0;
        self.refresh(can_open);
    }

    /// `readyForJoin`: the guide is done with, so a deep link may open
    /// its room.
    #[must_use]
    pub fn ready_for_join(&self) -> bool {
        COMPLETED.load(Ordering::Relaxed) && self.dialog.is_none() && !self.suspended
    }

    /// Whether the guide is over the lobby.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// `refresh`: open the guide when it is due. `can_open` is false while
    /// the Welcome or the Profile is up.
    pub fn refresh(&mut self, can_open: bool) {
        if !(self.authenticated && can_open) {
            self.dialog = None;
            return;
        }
        if self.suspended || (!self.manual && COMPLETED.load(Ordering::Relaxed)) {
            return;
        }
        if !self.manual && welcome_is_hidden() {
            COMPLETED.store(true, Ordering::Relaxed);
            return;
        }
        if self.dialog.is_none() {
            self.dialog = Some(LobbyWelcomeDialog::new(self.tab, self.secondary_tip));
        }
    }

    /// `onClose`.
    pub fn close(&mut self) {
        COMPLETED.store(true, Ordering::Relaxed);
        self.manual = false;
        self.tab = 0;
        self.dialog = None;
    }

    /// Escape: closes the guide once the terms are agreed. True when the
    /// guide had it.
    pub fn back(&mut self) -> bool {
        match &self.dialog {
            Some(d) if d.agreed() => {
                self.close();
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// `resume`: back from the keyboard setup.
    pub fn resume(&mut self, can_open: bool) {
        if self.suspended {
            self.suspended = false;
            self.refresh(can_open);
        }
    }

    /// `leaveLobby`: a match starts.
    pub fn leave_lobby(&mut self) {
        COMPLETED.store(true, Ordering::Relaxed);
        self.manual = false;
        self.suspended = false;
        self.dialog = None;
    }

    /// Draws the open guide over `screen`, and the arrow keys between its
    /// steps; what it asks of the lobby.
    pub fn ui(&mut self, ui: &mut egui::Ui, screen: egui::Rect) -> Option<GuideEvent> {
        let dialog = self.dialog.as_mut()?;
        let (left, right) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if left {
            dialog.step(-1);
        } else if right {
            dialog.step(1);
        }
        let answer = dialog.ui(ui, screen);
        self.tab = dialog.tab;
        match answer? {
            GuideAnswer::Close => {
                self.close();
                None
            }
            GuideAnswer::Setup => {
                self.suspended = true;
                self.tab = 1;
                self.dialog = None;
                Some(GuideEvent::OpenControls)
            }
            GuideAnswer::Link => dialog_link(self.dialog.as_mut()).map(GuideEvent::Open),
        }
    }
}

fn dialog_link(dialog: Option<&mut LobbyWelcomeDialog>) -> Option<String> {
    dialog.and_then(|d| d.link.take())
}
