//! wash-fps `client/LobbyPanel.ts`: the Play page. The four modes with their
//! hooks, quick play (a private FFA that fills itself with four bots), the
//! queue, private rooms, the matches being played (to spectate), the lobby
//! chat and the player card.
//!
//! The browser builds this from HTML with Washoi's raised keys; this draws
//! the same choices with egui. The leaderboard, demos, settings and profile
//! pages follow on their own. The panel ends when the lobby hands out a game
//! server ([`LobbyChoice`]); the window then plays or watches there.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use shagger::lobby::lobby_client::{LobbyClient, LobbyEvent};
use shagger::lobby::types::{ChatMessage, GametypeInfo, LiveMatchInfo, LobbyConnect, LobbyIn};
use shagger::ui::components::match_lobby_view::{MatchLobbyView, RoomAction};
use shagger::ui::egui_layer::egui;

use crate::client::demo_download::demo_base_url;
use crate::client::demos_view::DemosView;
use crate::client::hud::washiki;
use crate::client::leaderboard_view::LeaderboardView;
use crate::client::lobby_welcome_guide::LobbyWelcomeGuide;
use crate::client::profile_view::ProfileView;
use crate::client::settings::fps_settings_view::FpsSettingsView;
use crate::client::settings::settings_model::SettingsPageId;
use shagger::core::cvar::with_cvars;
use shagger::lobby::settings_sync::SettingsSync;

/// `GameMode`: a mode's card on the Play page.
#[derive(Debug, Clone, Copy)]
pub struct GameMode {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub hook: &'static str,
    /// The face of the mode's keys (`accent[0]`), as HSL.
    pub accent: (f32, f32, f32),
}

pub const GAME_MODES: [GameMode; 4] = [
    GameMode {
        id: "duel",
        title: "Duel",
        subtitle: "1 vs 1",
        hook: "Timing, aim, and nerve. Nothing extra.",
        accent: (182.0, 95.0, 52.0),
    },
    GameMode {
        id: "amphi",
        title: "Amphi",
        subtitle: "Aim arena",
        hook: "Tiny arena, short rounds. Pick a kit, prove your aim.",
        accent: (100.0, 85.0, 52.0),
    },
    GameMode {
        id: "ffa",
        title: "FFA",
        subtitle: "Free for all",
        hook: "Spawn hot, scrap hard, steal the lead.",
        accent: (46.0, 100.0, 57.0),
    },
    GameMode {
        id: "snd",
        title: "S&D",
        subtitle: "Search & destroy",
        hook: "Plant it, or make sure they never do.",
        accent: (322.0, 95.0, 62.0),
    },
];

/// Quick play's mode, and the bots it fills the room with.
pub const QUICK_PLAY_MODE: &str = "ffa";
pub const QUICK_PLAY_BOTS: usize = 4;
/// Messages kept in the chat log; older ones scroll away for good.
const CHAT_LOG_LIMIT: usize = 100;

/// Where the lobby sent this player.
#[derive(Debug, Clone, PartialEq)]
pub struct LobbyChoice {
    pub server_url: String,
    pub map: String,
    pub admission_ticket: Option<String>,
    /// The account's name, to join the game server as.
    pub name: String,
    /// True to play; false to watch.
    pub play: bool,
    /// A recording to play instead (the Demos page), saved on disk.
    pub demo: Option<std::path::PathBuf>,
}

fn hsl(h: f32, s: f32, l: f32) -> egui::Color32 {
    rgba(washiki::hsla(h, s, l, 1.0))
}

fn rgba(c: [f32; 4]) -> egui::Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]))
}

/// The display face's family (Orbitron, `--w-font-display`).
#[must_use]
pub fn display_family() -> egui::FontFamily {
    egui::FontFamily::Name("display".into())
}

/// Washoi's look for egui: Rajdhani for text and Orbitron for titles, cream
/// on the night's ink, gold for what is chosen (`styles/LobbyPanel.css`).
pub fn install_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "rajdhani".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Rajdhani-Bold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "orbitron".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Orbitron-Black.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Proportional,
        vec!["rajdhani".into(), "orbitron".into()],
    );
    fonts
        .families
        .insert(egui::FontFamily::Monospace, vec!["rajdhani".into(), "orbitron".into()]);
    fonts
        .families
        .insert(display_family(), vec!["orbitron".into(), "rajdhani".into()]);
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|style| {
        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, display_family())),
            (TextStyle::Body, FontId::proportional(19.0)),
            (TextStyle::Button, FontId::proportional(19.0)),
            (TextStyle::Small, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(17.0)),
        ]
        .into();
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        let cream = rgba(washiki::cream());
        v.override_text_color = Some(cream);
        v.panel_fill = hsl(252.0, 32.0, 9.0);
        v.window_fill = hsl(252.0, 32.0, 11.0);
        v.extreme_bg_color = hsl(250.0, 40.0, 5.0);
        v.faint_bg_color = hsl(252.0, 30.0, 13.0);
        v.widgets.inactive.weak_bg_fill = hsl(252.0, 28.0, 18.0);
        v.widgets.inactive.bg_fill = hsl(252.0, 28.0, 18.0);
        v.widgets.hovered.weak_bg_fill = hsl(252.0, 30.0, 26.0);
        v.widgets.active.weak_bg_fill = hsl(41.0, 96.0, 50.0);
        v.selection.bg_fill = hsl(41.0, 96.0, 40.0);
        v.selection.stroke = egui::Stroke::new(1.0, cream);
        v.hyperlink_color = hsl(46.0, 100.0, 64.0);
    });
}

fn clock_time(ts: f64) -> String {
    let secs = (ts / 1000.0) as u64;
    format!("{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60)
}

fn now_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

/// The rail's pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Play,
    Leaderboard,
    Demos,
    Profile,
}

/// `LobbyPanel`.
pub struct LobbyPanel {
    /// The lobby connection.
    pub client: LobbyClient,
    key: Option<String>,
    /// Where a new account's key is saved.
    key_file: Option<std::path::PathBuf>,
    connected: bool,
    pub name: String,
    local_lobby_player_id: Option<String>,
    server_gametypes: Vec<GametypeInfo>,
    private_lobby_supported: bool,
    live_matches: Vec<LiveMatchInfo>,
    online: usize,
    chat_log: VecDeque<ChatMessage>,
    chat_draft: String,
    /// A private room asked for; quick play when the flag is set.
    pending_private_create: bool,
    pending_quick_play: bool,
    /// The mode queued for, until the room arrives.
    queued: Option<String>,
    room: Option<MatchLobbyView>,
    /// A room joined from the queue readies by itself.
    room_from_queue: bool,
    notice: String,
    choice: Option<LobbyChoice>,
    /// The Settings page, while it is open.
    pub settings: Option<FpsSettingsView>,
    /// The page in the middle (`switchTab`).
    pub tab: Tab,
    leaderboard: LeaderboardView,
    /// The Demos page.
    pub demos: DemosView,
    profile: ProfileView,
    /// The account's settings, kept in step while logged in.
    settings_sync: Option<SettingsSync>,
    /// The rail's Console key was pressed; the window toggles its console.
    pub console_pressed: bool,
    /// The connection ended; why.
    pub closed: Option<String>,
    /// The first-run welcome guide.
    pub guide: LobbyWelcomeGuide,
}

impl LobbyPanel {
    /// Connects and logs in with the stored key, or as a new account.
    #[must_use]
    pub fn connect(url: &str, key_file: Option<std::path::PathBuf>) -> Self {
        let key = key_file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|k| k.trim().to_owned())
            .filter(|k| !k.is_empty());
        Self {
            client: LobbyClient::connect(url),
            key,
            key_file,
            connected: false,
            name: String::new(),
            local_lobby_player_id: None,
            server_gametypes: Vec::new(),
            private_lobby_supported: false,
            live_matches: Vec::new(),
            online: 0,
            chat_log: VecDeque::new(),
            chat_draft: String::new(),
            pending_private_create: false,
            pending_quick_play: false,
            queued: None,
            room: None,
            room_from_queue: false,
            notice: String::new(),
            choice: None,
            settings: None,
            console_pressed: false,
            tab: Tab::Play,
            leaderboard: LeaderboardView::default(),
            demos: DemosView::new(demo_base_url(url)),
            profile: ProfileView::default(),
            settings_sync: None,
            closed: None,
            guide: LobbyWelcomeGuide::new(crate::config_dir()),
        }
    }

    /// Where to go, once the lobby says so. Disconnects from the lobby.
    pub fn take_choice(&mut self) -> Option<LobbyChoice> {
        let choice = self.choice.take()?;
        if let Some(sync) = &mut self.settings_sync {
            with_cvars(|c| sync.flush(&mut self.client, c));
        }
        self.client.disconnect();
        Some(choice)
    }

    #[must_use]
    pub fn in_room(&self) -> Option<&MatchLobbyView> {
        self.room.as_ref()
    }

    /// Reads what the lobby sent since the last frame.
    pub fn poll(&mut self) {
        for event in self.client.poll_events() {
            self.on_event(event);
        }
        if let Some(sync) = &mut self.settings_sync {
            with_cvars(|c| sync.tick(&mut self.client, c, std::time::Instant::now()));
        }
        self.demos.tick();
        // A recording is in: the menu gets out of the way.
        if let Some(path) = self.demos.ready.take() {
            self.choice = Some(LobbyChoice {
                server_url: String::new(),
                map: path.display().to_string(),
                admission_ticket: None,
                name: self.name.clone(),
                play: false,
                demo: Some(path),
            });
        }
    }

    fn on_event(&mut self, event: LobbyEvent) {
        match event {
            LobbyEvent::Connected => {
                self.connected = true;
                self.client.auth(self.key.as_deref());
            }
            LobbyEvent::Error(why) => {
                self.connected = false;
                self.closed = Some(why);
            }
            LobbyEvent::Disconnected { reason, code } => {
                self.connected = false;
                self.closed = Some(format!("{reason} ({code})"));
            }
            LobbyEvent::Message(message) => self.on_message(message),
        }
    }

    fn on_message(&mut self, message: LobbyIn) {
        if let Some(room) = &mut self.room {
            room.handle(&message, &self.client);
        }
        if let Some(sync) = &mut self.settings_sync {
            with_cvars(|c| sync.on_message(&message, &mut self.client, c));
            // A download may have brought other bindings
            // (`reloadBindingsFromStorage`).
            if matches!(&message, LobbyIn::Other { kind, .. } if kind == "settings:data") {
                shagger::input::input_system::with_input(
                    shagger::input::input_system::InputSystem::reload_bindings_from_storage,
                );
            }
        }
        if let LobbyIn::Other { kind, message } = &message {
            self.leaderboard.on_message(kind, message);
            self.demos.on_message(kind, message);
            self.profile.on_message(kind, message);
        }
        match message {
            LobbyIn::Welcome(w) => {
                self.local_lobby_player_id = Some(w.player_id.clone());
                self.name = w
                    .identity
                    .as_ref()
                    .map_or_else(|| w.player_id.clone(), |i| i.name.clone());
                if let (Some(new_key), Some(file)) = (&w.key, &self.key_file)
                    && let Err(e) = std::fs::write(file, new_key)
                {
                    self.notice = format!("Could not save the new account's key: {e}");
                }
                self.server_gametypes = w.gametypes.clone();
                self.private_lobby_supported = w
                    .features
                    .as_ref()
                    .and_then(|f| f.get("privateLobbies"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true);
                self.live_matches = w.live_matches.clone();
                self.online = w.online_players.len();
                self.chat_log = w.chat_history.iter().cloned().collect();
                self.trim_chat();
                self.guide.set_authenticated(true);
                let feature = |name: &str| {
                    w.features
                        .as_ref()
                        .and_then(|f| f.get(name))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                };
                self.demos.connected(feature("demoOwnership"), feature("featuredDemos"));
                // `startSettingsSync`.
                if let Some(dir) = crate::config_dir() {
                    self.settings_sync = with_cvars(|c| SettingsSync::start(&mut self.client, &w, c, &dir));
                }
            }
            LobbyIn::AuthError(e) => self.notice = format!("Login refused: {}", e.code),
            LobbyIn::MatchesUpdate(matches) => self.live_matches = matches,
            LobbyIn::Chat(c) => {
                self.chat_log.push_back(c);
                self.trim_chat();
            }
            LobbyIn::RoomState(lobby) => self.handle_lobby_created(&lobby),
            LobbyIn::Connect(LobbyConnect {
                server_url,
                map,
                admission_ticket,
                ..
            }) => {
                self.choice = Some(LobbyChoice {
                    server_url,
                    map,
                    admission_ticket,
                    name: self.name.clone(),
                    play: true,
                    demo: None,
                });
            }
            LobbyIn::LobbyError(e) => {
                self.pending_private_create = false;
                self.pending_quick_play = false;
                self.notice = e;
            }
            LobbyIn::Other { kind, .. } if kind == "queue:left" => self.queued = None,
            LobbyIn::Other { .. } => {}
        }
    }

    fn trim_chat(&mut self) {
        while self.chat_log.len() > CHAT_LOG_LIMIT {
            self.chat_log.pop_front();
        }
    }

    /// `handleLobbyCreated`.
    fn handle_lobby_created(&mut self, lobby: &serde_json::Value) {
        let id = lobby.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if self.room.as_ref().is_some_and(|r| r.lobby_id == id) {
            return;
        }
        let quick_play = self.pending_quick_play && lobby.get("visibility").and_then(|v| v.as_str()) == Some("private");
        self.pending_private_create = false;
        self.pending_quick_play = false;
        self.room_from_queue = self.queued.take().is_some();
        self.notice.clear();
        let fill = if quick_play { QUICK_PLAY_BOTS } else { 0 };
        let mut room = MatchLobbyView::new(
            lobby,
            self.local_lobby_player_id.as_deref(),
            fill,
            now_seed(),
            &self.client,
        );
        if self.room_from_queue && !room.local_ready {
            // The browser's queue lands in a room already matched; ready up.
            self.client.set_ready(&room.lobby_id, true);
            room.local_ready = true;
        }
        self.room = Some(room);
    }

    fn mode_ready(&self, id: &str) -> bool {
        self.server_gametypes.iter().any(|g| g.id == id)
    }

    /// Quick play needs the lobby, its private rooms and FFA, and no room open.
    #[must_use]
    pub fn quick_play_enabled(&self) -> bool {
        self.connected
            && self.private_lobby_supported
            && self.queued.is_none()
            && !self.pending_private_create
            && self.room.is_none()
            && self.mode_ready(QUICK_PLAY_MODE)
    }

    /// `switchTab`: shows a rail page, asking the lobby for its data.
    pub fn open_tab(&mut self, tab: Tab) {
        self.settings = None;
        if self.tab != tab && self.connected {
            match tab {
                Tab::Leaderboard => self.leaderboard.refresh(&mut self.client),
                Tab::Profile => self.profile.refresh(&mut self.client),
                Tab::Demos => self.demos.open(&mut self.client),
                Tab::Play => {}
            }
        }
        self.tab = tab;
    }

    /// A private FFA room that adds its bots, votes a map and readies by itself.
    pub fn start_quick_play(&mut self) {
        if self.quick_play_enabled() {
            self.show_private_lobby(QUICK_PLAY_MODE);
            self.pending_quick_play = true;
        }
    }

    fn show_private_lobby(&mut self, mode: &str) {
        self.pending_private_create = true;
        self.client.create_lobby(mode, true);
    }

    fn spectate(&mut self, m: &LiveMatchInfo) {
        if let Some(url) = &m.server_url {
            self.choice = Some(LobbyChoice {
                server_url: url.clone(),
                map: m.map.clone(),
                admission_ticket: None,
                name: self.name.clone(),
                play: false,
                demo: None,
            });
        }
    }

    /// Draws the page.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // "Go to keyboard setup" from the guide.
        if self.guide.ui(ui.ctx(), hsl(46.0, 100.0, 57.0)) {
            self.settings = Some(FpsSettingsView::open(SettingsPageId::Controls));
        }
        egui::Panel::left("lobby_rail")
            .exact_size(240.0)
            .show(ui, |ui| self.rail(ui));
        egui::Panel::right("lobby_chat")
            .exact_size(320.0)
            .show(ui, |ui| self.chat(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            if !self.notice.is_empty() {
                ui.colored_label(hsl(0.0, 80.0, 60.0), &self.notice);
            }
            if let Some(settings) = &mut self.settings {
                settings.ui(ui);
                return;
            }
            match self.tab {
                Tab::Leaderboard => {
                    self.leaderboard.ui(ui, &mut self.client);
                    return;
                }
                Tab::Profile => {
                    self.profile.ui(ui, &mut self.client, &self.name);
                    return;
                }
                Tab::Demos => {
                    self.demos.ui(ui, &mut self.client, self.connected);
                    return;
                }
                Tab::Play => {}
            }
            let left = match &mut self.room {
                Some(room) => room.ui(ui, &self.client) == Some(RoomAction::Leave),
                None => {
                    self.modes(ui);
                    ui.add_space(16.0);
                    self.matches(ui);
                    false
                }
            };
            if left {
                self.room = None;
            }
        });
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("WASHOI!")
                .family(display_family())
                .size(34.0)
                .color(hsl(46.0, 100.0, 57.0)),
        );
        ui.add_space(12.0);
        let quick = egui::Button::new(
            egui::RichText::new("QUICK PLAY\nFFA vs bots")
                .size(20.0)
                .color(rgba(washiki::ink())),
        )
        .fill(hsl(46.0, 100.0, 57.0))
        .min_size(egui::vec2(220.0, 64.0));
        if ui
            .add_enabled(self.quick_play_enabled(), quick)
            .on_hover_text("A private free-for-all against four bots, on a random map")
            .clicked()
        {
            self.start_quick_play();
        }
        ui.add_space(12.0);
        for (tab, name) in [
            (Tab::Play, "Play"),
            (Tab::Leaderboard, "Leaderboard"),
            (Tab::Demos, "Demos"),
            (Tab::Profile, "Profile"),
        ] {
            if ui.selectable_label(self.tab == tab, name).clicked() {
                self.open_tab(tab);
            }
        }
        ui.add_space(12.0);
        if let Some(settings) = &mut self.settings {
            ui.label(egui::RichText::new("Settings").strong());
            settings.page_nav(ui);
            if self.guide.suspended && ui.button("« Back to welcome").clicked() {
                self.settings = None;
                self.guide.resume();
            }
        } else {
            ui.label(format!("{} online", self.online));
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let open = self.settings.is_some();
                if ui.selectable_label(open, "Settings").clicked() {
                    self.settings = if open { None } else { Some(FpsSettingsView::default()) };
                }
                if open && ui.button("Back to play").clicked() {
                    self.settings = None;
                }
                if ui.button("Console").clicked() {
                    self.console_pressed = true;
                }
                if ui.button("?").on_hover_text("Welcome & tips").clicked() {
                    self.guide.open_help();
                }
            });
            let meta = if !self.connected {
                self.closed.as_deref().map_or("Offline", |_| "Disconnected")
            } else if self.room.is_some() {
                "In a room"
            } else {
                "In lobby"
            };
            ui.label(meta);
            ui.strong(&self.name);
        });
    }

    fn modes(&mut self, ui: &mut egui::Ui) {
        ui.heading("Play");
        let mut action: Option<(bool, &'static str)> = None;
        egui::Grid::new("lobby_modes")
            .num_columns(2)
            .spacing([16.0, 12.0])
            .show(ui, |ui| {
                for (i, mode) in GAME_MODES.iter().enumerate() {
                    let ready = self.connected && self.mode_ready(mode.id);
                    let (h, s, l) = mode.accent;
                    egui::Frame::group(ui.style())
                        .stroke(egui::Stroke::new(2.0, hsl(h, s, l)))
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(300.0);
                                ui.label(
                                    egui::RichText::new(mode.title)
                                        .family(display_family())
                                        .size(24.0)
                                        .color(hsl(h, s, l)),
                                );
                                ui.label(egui::RichText::new(mode.subtitle).strong());
                                ui.label(egui::RichText::new(mode.hook).color(rgba(washiki::cream_dim())));
                                ui.horizontal(|ui| {
                                    let queued = self.queued.as_deref() == Some(mode.id);
                                    let label = if queued { "Leave queue" } else { "Find match" };
                                    let can_queue = ready && (self.queued.is_none() || queued);
                                    if ui.add_enabled(can_queue, egui::Button::new(label)).clicked() {
                                        action = Some((false, mode.id));
                                    }
                                    let can_private = ready && self.private_lobby_supported && self.queued.is_none();
                                    if ui.add_enabled(can_private, egui::Button::new("Private room")).clicked() {
                                        action = Some((true, mode.id));
                                    }
                                });
                                if !ready {
                                    ui.weak("Not on this server");
                                }
                            });
                        });
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        match action {
            Some((true, mode)) => self.show_private_lobby(mode),
            Some((false, mode)) if self.queued.as_deref() == Some(mode) => {
                self.client.leave_queue();
                self.queued = None;
            }
            Some((false, mode)) => {
                self.client.join_queue(mode);
                self.queued = Some(mode.to_owned());
            }
            None => {}
        }
        if let Some(mode) = &self.queued {
            ui.label(format!("Searching for a {mode} match…"));
        }
    }

    fn matches(&mut self, ui: &mut egui::Ui) {
        ui.heading("Live matches");
        if self.live_matches.is_empty() {
            ui.weak("No matches right now.");
            return;
        }
        let mut watch = None;
        egui::Grid::new("lobby_matches").striped(true).show(ui, |ui| {
            for m in &self.live_matches {
                ui.label(&m.gametype_name);
                ui.label(if m.map.is_empty() { "?" } else { &m.map });
                ui.label(format!("{}/{}", m.player_count, m.max_players));
                ui.label(&m.state);
                let can = m.state == "playing" && m.server_url.is_some();
                if ui.add_enabled(can, egui::Button::new("Spectate")).clicked() {
                    watch = Some(m.clone());
                }
                ui.end_row();
            }
        });
        if let Some(m) = watch {
            self.spectate(&m);
        }
    }

    fn chat(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chat");
        let mut send = false;
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.chat_draft)
                    .hint_text("Say something")
                    .desired_width(f32::INFINITY),
            );
            send = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    for m in &self.chat_log {
                        ui.horizontal_wrapped(|ui| {
                            ui.weak(clock_time(m.ts));
                            ui.strong(&m.from);
                            ui.label(&m.text);
                        });
                    }
                });
            });
        });
        let text = self.chat_draft.trim().to_owned();
        if send && !text.is_empty() && self.connected {
            match &self.room {
                Some(room) => self.client.room_chat_send(&room.lobby_id, &text),
                None => self.client.chat_send(&text),
            }
            self.chat_draft.clear();
        }
    }
}
