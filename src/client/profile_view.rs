//! wash-fps `client/ProfileView.ts`: this account's record per mode (`stats:get`):
//! matches, wins, losses and draws, and its rating and rank. The match
//! history pages and the per-weapon totals are left for a later pass.

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::ui::egui_layer::egui;

use crate::client::lobby_panel::GAME_MODES;

#[derive(Debug)]
pub struct ProfileView {
    pub mode: &'static str,
    pub private: bool,
    request: Option<String>,
    pub summary: Option<Value>,
}

impl Default for ProfileView {
    fn default() -> Self {
        Self {
            mode: "duel",
            private: false,
            request: None,
            summary: None,
        }
    }
}

impl ProfileView {
    /// The connection went, or the account changed.
    pub fn clear(&mut self) {
        self.request = None;
        self.summary = None;
    }

    pub fn refresh(&mut self, client: &mut LobbyClient) {
        self.request = Some(client.request_stats(self.mode, "all", self.private));
    }

    /// `stats:summary` for the newest request.
    pub fn on_message(&mut self, kind: &str, message: &Value) {
        let id = message.get("requestId").and_then(Value::as_str);
        if kind == "stats:summary" && id.is_some() && id == self.request.as_deref() {
            self.request = None;
            self.summary = message.get("data").cloned();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, client: &mut LobbyClient, name: &str) {
        ui.heading(name);
        let mut changed = false;
        ui.horizontal(|ui| {
            for mode in &GAME_MODES {
                changed |= ui.selectable_value(&mut self.mode, mode.id, mode.title).changed();
            }
            ui.separator();
            changed |= ui.selectable_value(&mut self.private, false, "Public").changed();
            changed |= ui.selectable_value(&mut self.private, true, "Private").changed();
        });
        if changed {
            self.refresh(client);
        }
        let Some(s) = &self.summary else {
            ui.weak(if self.request.is_some() {
                "Loading…"
            } else {
                "No record yet."
            });
            return;
        };
        let n = |k: &str| s.get(k).and_then(Value::as_u64).unwrap_or(0);
        egui::Grid::new("profile").num_columns(2).show(ui, |ui| {
            for (label, key) in [
                ("Matches", "matches"),
                ("Wins", "wins"),
                ("Losses", "losses"),
                ("Draws", "draws"),
            ] {
                ui.label(label);
                ui.strong(n(key).to_string());
                ui.end_row();
            }
            ui.label("Rating");
            match s.get("rating").filter(|r| !r.is_null()) {
                Some(r) => {
                    let value = r.get("value").and_then(Value::as_f64).unwrap_or(0.0);
                    let rank = r.get("rank").and_then(Value::as_u64).unwrap_or(0);
                    let provisional = r.get("provisional").and_then(Value::as_bool).unwrap_or(false);
                    ui.strong(format!(
                        "{} (#{rank}){}",
                        shagger::shared::js::js_number_to_string(value),
                        if provisional { " · Provisional" } else { "" }
                    ));
                }
                None => {
                    ui.weak("Unrated");
                }
            }
            ui.end_row();
        });
    }
}
