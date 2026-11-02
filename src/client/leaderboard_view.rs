//! wash-fps `client/LeaderboardView.ts`: the top players per mode, over 30
//! days or all time, in the public or the private pools. Ratings are the
//! lobby's TrueSkill numbers as it sends them; "Provisional" follows a
//! rating with too few matches. The browser's podium and avatars are left
//! for the visuals pass.

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::ui::egui_layer::egui;

use crate::client::lobby_panel::GAME_MODES;

const PAGE: u32 = 50;

/// `PERIOD_NAME`.
const PERIODS: [(&str, &str); 2] = [("month", "30 days"), ("all", "All time")];

/// One row (`LeaderboardEntry`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub rank: u32,
    pub name: String,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub rating: Option<f64>,
    pub provisional: bool,
    pub bot: bool,
}

impl Entry {
    fn from_json(v: &Value) -> Self {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
        Self {
            rank: n("rank"),
            name: v.get("name").and_then(Value::as_str).unwrap_or("?").to_owned(),
            wins: n("wins"),
            losses: n("losses"),
            draws: n("draws"),
            rating: v.get("rating").and_then(Value::as_f64),
            provisional: v.get("provisional").and_then(Value::as_bool).unwrap_or(false),
            bot: v.get("bot").and_then(Value::as_bool).unwrap_or(false),
        }
    }

    /// `ratingText`.
    fn rating_text(&self) -> String {
        match self.rating {
            Some(r) => format!(
                "{}{}",
                shagger::shared::js::js_number_to_string(r),
                if self.provisional { " · Provisional" } else { "" }
            ),
            None => "-".into(),
        }
    }
}

#[derive(Debug)]
pub struct LeaderboardView {
    pub mode: &'static str,
    pub period: &'static str,
    pub private: bool,
    request: Option<String>,
    pub entries: Vec<Entry>,
    pub you: Option<Entry>,
    pub total: u32,
    pub error: Option<String>,
}

impl Default for LeaderboardView {
    fn default() -> Self {
        Self {
            mode: "duel",
            period: "month",
            private: false,
            request: None,
            entries: Vec::new(),
            you: None,
            total: 0,
            error: None,
        }
    }
}

impl LeaderboardView {
    /// Asks for the board the filters name.
    pub fn refresh(&mut self, client: &mut LobbyClient) {
        self.error = None;
        self.request = Some(client.request_leaderboard(self.mode, self.period, 0, PAGE, self.private));
    }

    /// `leaderboard:page` and `leaderboard:error` for the newest request.
    pub fn on_message(&mut self, kind: &str, message: &Value) {
        let id = message.get("requestId").and_then(Value::as_str);
        if id.is_none() || id != self.request.as_deref() {
            return;
        }
        match kind {
            "leaderboard:page" => {
                self.request = None;
                self.entries = message
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|e| e.iter().map(Entry::from_json).collect())
                    .unwrap_or_default();
                self.you = message.get("localPlayerEntry").map(Entry::from_json);
                self.total = message.get("total").and_then(Value::as_u64).unwrap_or(0) as u32;
            }
            "leaderboard:error" => {
                self.request = None;
                let reason = message.get("reason").and_then(Value::as_str).unwrap_or("unavailable");
                self.error = Some(match reason {
                    "rate_limited" => "Too many requests; try again in a moment.".into(),
                    "invalid_query" => "The lobby did not understand that board.".into(),
                    _ => "The leaderboard is unavailable right now.".into(),
                });
            }
            _ => {}
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, client: &mut LobbyClient) {
        ui.heading("Leaderboard");
        let mut changed = false;
        ui.horizontal(|ui| {
            for mode in &GAME_MODES {
                changed |= ui.selectable_value(&mut self.mode, mode.id, mode.title).changed();
            }
            ui.separator();
            changed |= ui.selectable_value(&mut self.private, false, "Public").changed();
            changed |= ui.selectable_value(&mut self.private, true, "Private").changed();
            ui.separator();
            for (id, name) in PERIODS {
                changed |= ui.selectable_value(&mut self.period, id, name).changed();
            }
        });
        if changed {
            self.refresh(client);
        }
        if let Some(e) = &self.error {
            ui.label(e);
        }
        if self.request.is_some() && self.entries.is_empty() {
            ui.weak("Loading…");
        }
        if let Some(you) = &self.you {
            ui.label(format!("You: #{} · {}", you.rank, you.rating_text()));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("leaderboard")
                .striped(true)
                .num_columns(4)
                .show(ui, |ui| {
                    ui.strong("#");
                    ui.strong("Player");
                    ui.strong("Rating");
                    ui.strong("W-L-D");
                    ui.end_row();
                    for e in &self.entries {
                        ui.label(e.rank.to_string());
                        ui.label(if e.bot {
                            format!("{} (bot)", e.name)
                        } else {
                            e.name.clone()
                        });
                        ui.label(e.rating_text());
                        ui.label(format!("{}-{}-{}", e.wins, e.losses, e.draws));
                        ui.end_row();
                    }
                });
            if self.entries.is_empty() && self.request.is_none() && self.error.is_none() {
                ui.weak("No ranked players yet.");
            }
        });
    }
}
