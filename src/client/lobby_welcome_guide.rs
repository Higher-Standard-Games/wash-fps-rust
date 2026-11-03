//! wash-fps `client/LobbyWelcomeGuide.ts`: when the welcome dialog opens by
//! itself, the `?` that reopens it, and the way back from keyboard setup.

use std::path::PathBuf;

use shagger::ui::egui_layer::egui;

use crate::client::lobby_welcome_dialog::{DialogEvent, LobbyWelcomeDialog, welcome_is_hidden};

/// The guide was closed in this run (the browser keeps it per page load).
static COMPLETED_THIS_RUN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `LobbyWelcomeGuide`: when the dialog opens by itself, the `?` that
/// reopens it, and the way back from keyboard setup. The boot state
/// (`welcomeBootState`) is the guide's own: one per client run.
#[derive(Debug)]
pub struct LobbyWelcomeGuide {
    dir: Option<PathBuf>,
    pub dialog: Option<LobbyWelcomeDialog>,
    authenticated: bool,
    manual: bool,
    tab: usize,
    secondary_tip: usize,
    completed: bool,
    /// Keyboard setup is open from the guide; "« Back to welcome" resumes.
    pub suspended: bool,
}

impl LobbyWelcomeGuide {
    #[must_use]
    pub fn new(dir: Option<PathBuf>) -> Self {
        let secondary_tip = usize::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos())
                % 2
                == 1,
        );
        Self {
            dir,
            dialog: None,
            authenticated: false,
            manual: false,
            tab: 0,
            secondary_tip,
            // One boot state per run: the menu after a match does not ask again.
            completed: COMPLETED_THIS_RUN.load(std::sync::atomic::Ordering::Relaxed),
            suspended: false,
        }
    }

    /// `readyForJoin`.
    #[must_use]
    pub fn ready_for_join(&self) -> bool {
        self.completed && self.dialog.is_none() && !self.suspended
    }

    /// `setAuthenticated`, then `refresh`.
    pub fn set_authenticated(&mut self, value: bool) {
        self.authenticated = value;
        self.refresh();
    }

    /// The `?` key.
    pub fn open_help(&mut self) {
        if !self.authenticated || self.dialog.is_some() {
            return;
        }
        self.suspended = false;
        self.manual = true;
        self.tab = 0;
        self.refresh();
    }

    /// `refresh`.
    pub fn refresh(&mut self) {
        if !self.authenticated {
            self.dialog = None;
            return;
        }
        if self.suspended || (!self.manual && self.completed) {
            return;
        }
        if !self.manual && welcome_is_hidden(self.dir.as_deref()) {
            self.completed = true;
            return;
        }
        if self.dialog.is_none() {
            self.dialog = Some(LobbyWelcomeDialog::new(self.dir.clone(), self.tab, self.secondary_tip));
        }
    }

    /// `resume`: back from keyboard setup.
    pub fn resume(&mut self) {
        if self.suspended {
            self.suspended = false;
            self.refresh();
        }
    }

    /// The dialog, if open. True when it asks for keyboard setup.
    pub fn ui(&mut self, ctx: &egui::Context, strong: egui::Color32) -> bool {
        let Some(dialog) = &mut self.dialog else { return false };
        let event = dialog.ui(ctx, strong);
        self.tab = dialog.tab;
        match event {
            Some(DialogEvent::Close) => {
                self.completed = true;
                COMPLETED_THIS_RUN.store(true, std::sync::atomic::Ordering::Relaxed);
                self.manual = false;
                self.tab = 0;
                self.dialog = None;
                false
            }
            Some(DialogEvent::Setup) => {
                self.suspended = true;
                self.tab = 1;
                self.dialog = None;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::lobby_welcome_dialog::{TERMS_PREFERENCE, WELCOME_PREFERENCE, terms_version, write_preference};

    #[test]
    fn the_guide_opens_until_hidden() {
        let dir = std::env::temp_dir().join(format!("washoi-welcome-guide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut guide = LobbyWelcomeGuide::new(Some(dir.clone()));
        guide.set_authenticated(true);
        assert!(guide.dialog.is_some() && !guide.ready_for_join());
        write_preference(Some(&dir), WELCOME_PREFERENCE, "true");
        write_preference(Some(&dir), TERMS_PREFERENCE, terms_version());
        let mut later = LobbyWelcomeGuide::new(Some(dir.clone()));
        later.set_authenticated(true);
        assert!(later.dialog.is_none() && later.ready_for_join());
        later.open_help();
        assert!(later.dialog.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
