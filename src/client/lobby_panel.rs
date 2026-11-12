//! wash-fps `client/LobbyPanel.ts` and `styles/LobbyPanel.css`: the lobby,
//! a gold rail beside one stage (docs/research/NATIVE_LOBBY.md).
//!
//! The rail holds the brand, Quick play and the pages, the rooms being
//! played or formed, the player card and the Profile, Settings and Console
//! keys. The Play stage holds the four modes, the picked mode's hook with
//! Find and Private lobby, and the lobby chat. A room the lobby opens (from
//! the queue, a private room or quick play) takes the stage's body.
//!
//! The lobby connection lives as long as the panel, through every match, as
//! the browser's page does: the room, the chat and the queue are there when
//! the player comes back. When the lobby hands out a game server the panel
//! holds a [`LobbyChoice`] for the window to take.
//!
//! The logic is the browser's, and the retired native lobby's (`1a60807`)
//! for the key file and the retries. Drawn with egui on shagger's
//! `RaisedKey` and rail at the CSS's sizes (CSS pixels are egui points).
//!
//! Not yet (NATIVE_LOBBY.md 5): the Demos, Profile and Console pages,
//! which say so and offer the web lobby; the welcome dialog and guide,
//! credits, music, deep links and the drop-in from a spectated match.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use shagger::core::cvar::with_cvars;
use shagger::lobby::lobby_client::{CLIENT_DISCONNECT_REASON, LobbyClient, LobbyEvent};
use shagger::lobby::settings_sync::SettingsSync;
use shagger::lobby::types::{AuthError, GametypeInfo, LiveMatchInfo, LobbyConnect, LobbyIn};
use shagger::shared::js::js_round;
use shagger::ui::components::lobby_twin::{
    RAIL_SKEW, TwinRail, TwinStyle, clamp_css, cream, elide, gradient_polygon, horizontal_gradient, hsl, hsla,
    paint_backdrop, paint_brand, paint_light_in, paint_rail, text, violet,
};
use shagger::ui::components::match_lobby_view::{MatchLobbyView, RoomAction as ViewAction, RoomStyle};
use shagger::ui::components::outline_text::paint_outline_text;
use shagger::ui::components::raised_key::{Accent, KeyFace, KeyLook, KeyText, KeyTextLayout, RaisedKey};
use shagger::ui::components::text_field::TextField;
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::client::leaderboard_view::LeaderboardView;
use crate::client::levelshots::{Levelshots, site_base};
use crate::client::lobby_rooms::{PlayKeysInput, RoomAction, describe_room, play_keys, room_count_label, sort_rooms};
use crate::client::room_chat_view::RoomChatView;
use crate::client::settings::fps_settings_view::FpsSettingsView;
use crate::client::settings::settings_model::SETTINGS_PAGES;
use crate::client::washoi_style::{DISPLAY_FONT, display_family};

/// `GameMode`: a mode's tile on the Play stage.
#[derive(Debug, Clone, Copy)]
pub struct GameMode {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub hook: &'static str,
    /// Face, highlight, shade and the two side tones of the mode's keys, as HSL.
    pub accent: [(f32, f32, f32); 5],
    /// The mode's mark in the rail's room list, dark enough to read on cream.
    pub mark: (f32, f32, f32),
}

impl GameMode {
    /// The mode's `--accent` properties.
    #[must_use]
    pub fn accent(&self) -> Accent {
        let c = |(h, s, l): (f32, f32, f32)| hsl(h, s, l);
        Accent {
            face: c(self.accent[0]),
            hi: c(self.accent[1]),
            lo: c(self.accent[2]),
            side_hi: c(self.accent[3]),
            side_lo: c(self.accent[4]),
        }
    }
}

/// `GAME_MODES`.
pub const GAME_MODES: [GameMode; 4] = [
    GameMode {
        id: "duel",
        title: "Duel",
        subtitle: "1 vs 1",
        hook: "Timing, aim, and nerve. Nothing extra.",
        accent: [
            (182.0, 95.0, 52.0),
            (182.0, 100.0, 74.0),
            (188.0, 90.0, 40.0),
            (192.0, 85.0, 28.0),
            (198.0, 80.0, 17.0),
        ],
        mark: (186.0, 95.0, 42.0),
    },
    GameMode {
        id: "amphi",
        title: "Amphi",
        subtitle: "Aim arena",
        hook: "Tiny arena, short rounds. Pick a kit, prove your aim.",
        accent: [
            (100.0, 85.0, 52.0),
            (98.0, 95.0, 72.0),
            (108.0, 80.0, 40.0),
            (115.0, 70.0, 26.0),
            (125.0, 65.0, 16.0),
        ],
        mark: (104.0, 80.0, 38.0),
    },
    GameMode {
        id: "ffa",
        title: "FFA",
        subtitle: "Free for all",
        hook: "Spawn hot, scrap hard, steal the lead.",
        accent: [
            (46.0, 100.0, 57.0),
            (51.0, 100.0, 73.0),
            (41.0, 100.0, 48.0),
            (34.0, 90.0, 36.0),
            (28.0, 85.0, 23.0),
        ],
        mark: (38.0, 100.0, 44.0),
    },
    GameMode {
        id: "snd",
        title: "S&D",
        subtitle: "Search & destroy",
        hook: "Plant it, or make sure they never do.",
        accent: [
            (322.0, 95.0, 62.0),
            (322.0, 100.0, 77.0),
            (318.0, 85.0, 50.0),
            (312.0, 70.0, 33.0),
            (306.0, 70.0, 21.0),
        ],
        mark: (322.0, 90.0, 52.0),
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
    pub demo: Option<PathBuf>,
    /// Its highlight reel instead, about this many minutes long.
    pub reel: Option<f64>,
    /// The side this player took in the room (`alpha` or `bravo`), for the
    /// game server to honour on join; `None` in a mode without teams.
    pub team: Option<String>,
}

/// What the panel asks of the window, besides a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelAction {
    /// Back to the web lobby (`ui_lobby web`), for a page not built here yet.
    WebLobby,
    /// The links row's Quit.
    Quit,
    /// A page for the system's browser, such as the devlog.
    OpenUrl(&'static str),
}

/// The team's devlog, opened from the Play page's links bar (`DEVLOG_URL`).
pub const DEVLOG_URL: &str = "https://washoi.net/devlog";

/// The rail's pages (`switchTab`), and the keys under the player card that
/// open a page here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Play,
    Leaderboard,
    Demos,
    Settings,
    Profile,
    Console,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Self::Play => "Play",
            Self::Leaderboard => "Leaderboard",
            Self::Demos => "Demos",
            Self::Settings => "Settings",
            Self::Profile => "Profile",
            Self::Console => "Console",
        }
    }
}

/// A line in the lobby chat.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatLine {
    pub from: String,
    pub text: String,
    /// Milliseconds since the epoch.
    pub ts: f64,
    /// The lobby's own note (`lobby-chat__msg--sys`).
    pub system: bool,
}

/// `clockTime`: hours and minutes on the player's clock.
#[must_use]
pub fn clock_time(ts: f64) -> String {
    jiff::Timestamp::from_millisecond(ts as i64)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
        .map_or_else(|_| "--:--".into(), |z| format!("{:02}:{:02}", z.hour(), z.minute()))
}

/// `RoomChatView.ts`'s `nameColor`: a hue per name, the same in every client.
#[must_use]
pub fn name_color(name: &str) -> Color32 {
    const HUES: [f32; 10] = [182.0, 322.0, 125.0, 276.0, 28.0, 200.0, 350.0, 104.0, 240.0, 160.0];
    let mut hash: u32 = 0;
    // `for (const char of name)`: each code point, by its first UTF-16 unit.
    for c in name.chars() {
        let mut units = [0u16; 2];
        let first = c.encode_utf16(&mut units)[0];
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(first));
    }
    hsl(HUES[hash as usize % HUES.len()], 92.0, 68.0)
}

/// `loginFailureText`.
fn login_failure_text(code: &str) -> String {
    match code {
        "unknown_key" => "No player has that key. Check it, or keep the player you have.".into(),
        "rate_limited" => "Too many tries. Wait a minute, then try again.".into(),
        "lobby_full" => "Servers are full. Your current player key is unchanged. Try again shortly.".into(),
        "already_authenticated" => "This connection is already logged in. Try again.".into(),
        other => format!("Login refused: {other}"),
    }
}

/// `LobbyFullNotice.retryDelaySec`: the lobby's wait, give or take ten seconds.
fn retry_delay_sec(retry_after_sec: Option<u32>, random: f64) -> u64 {
    let base = retry_after_sec.filter(|&s| s > 0).unwrap_or(60);
    let jitter = js_round((random * 2.0 - 1.0) * 10.0);
    (f64::from(base) + jitter).max(5.0) as u64
}

/// `offlineRetryDelaySec`: 2, 5, 10, then 30 seconds, each give or take a fifth.
fn offline_retry_delay_sec(attempt: u32, random: f64) -> u64 {
    const WAITS: [f64; 4] = [2.0, 5.0, 10.0, 30.0];
    let base = WAITS[(attempt.max(1) as usize - 1).min(WAITS.len() - 1)];
    js_round(base * (0.8 + random * 0.4)).max(1.0) as u64
}

fn now_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

fn random01() -> f64 {
    (now_seed() % 1_000_000) as f64 / 1_000_000.0
}

/// `file` with `.suffix` added to its whole name (`lobby-key.unknown`).
fn beside(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    file.with_file_name(name)
}

/// Writes the key through a file beside it, flushed before the rename: an
/// empty key file after a power cut would make a new account.
fn save_key(file: &Path, key: &str) -> std::io::Result<()> {
    shagger::core::atomic_file::write_atomic(file, key.as_bytes())
}

/// `LobbyPanel`.
pub struct LobbyPanel {
    pub client: LobbyClient,
    url: String,
    key: Option<String>,
    /// Where the account's key lives (the web lobby's too).
    key_file: Option<PathBuf>,
    /// The key file exists but could not be read: a new account's key must
    /// not replace it.
    key_unreadable: bool,
    /// Failed logins in a row, for the offline backoff (`loginFailures`).
    login_failures: u32,
    /// When to connect again, after a drop or a full lobby.
    retry_at: Option<Instant>,
    /// The lobby is full and closes the connection; its wait stands.
    lobby_full: bool,
    /// The room, its invite and the account, when the connection dropped
    /// in a room (`roomToRejoin`).
    room_to_rejoin: Option<(String, Option<String>, Option<String>)>,
    /// The open room's invite, from its share link (`currentInviteToken`).
    room_invite: Option<String>,
    /// The logged-in account (`currentIdentity().id`).
    account_id: Option<String>,
    connected: bool,
    /// The account's name.
    pub name: String,
    local_lobby_player_id: Option<String>,
    server_gametypes: Vec<GametypeInfo>,
    private_lobby_supported: bool,
    live_matches: Vec<LiveMatchInfo>,
    /// `livePlayers`, by id.
    online: Vec<String>,
    chat: VecDeque<ChatLine>,
    chat_draft: String,
    pending_private_create: bool,
    /// The private room being created is quick play's.
    pending_quick_play: bool,
    queue_pending: bool,
    room: Option<MatchLobbyView>,
    /// The room's own chat, in the rail while the room is open.
    room_chat: Option<RoomChatView>,
    /// The settings screen, made the first time it is opened.
    settings: Option<FpsSettingsView>,
    /// The Leaderboard page.
    pub leaderboard: LeaderboardView,
    /// The page drawn last frame: a page coming into view asks again.
    shown_tab: Option<Tab>,
    /// The maps' levelshots, for the room's vote cards.
    levelshots: Levelshots,
    /// The site the game is served from, which invites open.
    site: String,
    /// A seat asked back for a running match: its request and the match.
    seat_request: Option<(String, String)>,
    /// The map of the room's last match, for a rejoin.
    last_map: Option<String>,
    /// `playNoticeEl`.
    notice: String,
    choice: Option<LobbyChoice>,
    pub tab: Tab,
    selected_mode: usize,
    settings_sync: Option<SettingsSync>,
    started: Instant,
}

impl LobbyPanel {
    /// Connects and logs in with the stored key, or as a new account.
    #[must_use]
    pub fn connect(url: &str, key_file: Option<PathBuf>) -> Self {
        let (key, key_unreadable) = match key_file.as_ref().map(std::fs::read_to_string) {
            Some(Ok(k)) => (Some(k.trim().to_owned()).filter(|k| !k.is_empty()), false),
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
                println!("lobby: the player key could not be read: {e}");
                (None, true)
            }
            _ => (None, false),
        };
        Self {
            client: LobbyClient::connect(url),
            url: url.to_owned(),
            key,
            key_file,
            key_unreadable,
            login_failures: 0,
            retry_at: None,
            lobby_full: false,
            room_to_rejoin: None,
            room_invite: None,
            account_id: None,
            connected: false,
            name: String::new(),
            local_lobby_player_id: None,
            server_gametypes: Vec::new(),
            private_lobby_supported: false,
            live_matches: Vec::new(),
            online: Vec::new(),
            chat: VecDeque::new(),
            chat_draft: String::new(),
            pending_private_create: false,
            pending_quick_play: false,
            queue_pending: false,
            room: None,
            room_chat: None,
            settings: None,
            leaderboard: LeaderboardView::new(),
            shown_tab: None,
            levelshots: Levelshots::new(url),
            site: site_base(url),
            seat_request: None,
            last_map: None,
            notice: String::new(),
            choice: None,
            tab: Tab::Play,
            selected_mode: 0,
            settings_sync: None,
            started: Instant::now(),
        }
    }

    /// Where to go, once the lobby says so. The connection stays open.
    pub fn take_choice(&mut self) -> Option<LobbyChoice> {
        self.choice.take()
    }

    /// Back from a match: why it broke off, if it did.
    pub fn after_match(&mut self, error: Option<String>) {
        if let Some(why) = error {
            self.notice = why;
        }
    }

    /// The way out of the game (`pagehide`): uploads settings not yet
    /// uploaded, waits up to a second and a half for the lobby to confirm
    /// them, then disconnects.
    pub fn leave(&mut self) {
        if self.connected
            && let Some(sync) = &mut self.settings_sync
        {
            with_cvars(|c| sync.flush(&mut self.client, c));
            let deadline = Instant::now() + Duration::from_millis(1500);
            while sync.awaiting_save()
                && let Some(left) = deadline.checked_duration_since(Instant::now())
            {
                match self.client.next_event(left) {
                    Some(LobbyEvent::Message(message)) => {
                        with_cvars(|c| sync.on_message(&message, &mut self.client, c));
                    }
                    Some(LobbyEvent::Connected) => {}
                    Some(LobbyEvent::Disconnected { .. } | LobbyEvent::Error(_)) | None => break,
                }
            }
        }
        self.client.disconnect();
    }

    #[must_use]
    pub fn in_room(&self) -> bool {
        self.room.is_some()
    }

    #[must_use]
    pub fn connected(&self) -> bool {
        self.connected
    }

    /// Reads what the lobby sent since the last frame.
    pub fn poll(&mut self) {
        if self.retry_at.is_some_and(|at| Instant::now() >= at) {
            self.retry_at = None;
            self.notice.clear();
            if self.connected {
                // A refused login on an open connection logs in again on it.
                self.client.auth(self.key.as_deref());
            } else {
                self.client = LobbyClient::connect(&self.url);
            }
        }
        for event in self.client.poll_events() {
            self.on_event(event);
        }
        if let Some(sync) = &mut self.settings_sync {
            with_cvars(|c| sync.tick(&mut self.client, c, Instant::now()));
        }
        if let Some(room) = &mut self.room {
            room.tick();
        }
    }

    fn on_event(&mut self, event: LobbyEvent) {
        match event {
            LobbyEvent::Connected => {
                self.connected = true;
                self.client.auth(self.key.as_deref());
            }
            LobbyEvent::Error(why) => {
                println!("lobby: {why}");
                self.connection_lost(false);
            }
            LobbyEvent::Disconnected { reason, code } => {
                println!("lobby: disconnected: {reason} ({code})");
                self.connection_lost(reason == CLIENT_DISCONNECT_REASON);
            }
            LobbyEvent::Message(message) => self.on_message(message),
        }
    }

    /// `_disconnected`: forget what the lobby said, keep the room to rejoin,
    /// and try again after the outage backoff, unless the close was this
    /// client's own or a full lobby already set its wait.
    fn connection_lost(&mut self, deliberate: bool) {
        self.connected = false;
        self.room_chat = None;
        self.seat_request = None;
        if let Some(room) = self.room.take() {
            self.room_to_rejoin = Some((room.lobby_id, self.room_invite.take(), self.account_id.clone()));
        }
        self.queue_pending = false;
        self.pending_private_create = false;
        self.pending_quick_play = false;
        self.private_lobby_supported = false;
        self.server_gametypes.clear();
        self.live_matches.clear();
        self.online.clear();
        self.settings_sync = None;
        self.leaderboard.bind(false, false, None);
        if !deliberate && !self.lobby_full && self.retry_at.is_none() {
            self.login_failures += 1;
            let seconds = offline_retry_delay_sec(self.login_failures, random01());
            self.notice = format!("Lobby server unavailable. Trying again in {seconds}s.");
            self.retry_at = Some(Instant::now() + Duration::from_secs(seconds));
        }
        self.lobby_full = false;
    }

    /// `performLobbyLogin`'s answer to a refusal.
    fn login_refused(&mut self, e: &AuthError) {
        // The lobby does not know the stored key (its accounts were reset):
        // set the key aside and make a new account.
        if e.code == "unknown_key"
            && let Some(key) = self.key.clone()
        {
            let aside = self.key_file.as_ref().map(|f| beside(f, "unknown"));
            match aside.map(|aside| save_key(&aside, &key)) {
                Some(Err(why)) => {
                    println!("lobby: could not set the unknown key aside: {why}");
                    self.notice = format!("{} The key could not be set aside: {why}", login_failure_text(&e.code));
                }
                _ => {
                    self.key = None;
                    self.client.auth(None);
                }
            }
            return;
        }
        // A full lobby waits as long as it asks (and closes the connection);
        // any other refusal tries again after the outage backoff.
        let seconds = if e.code == "lobby_full" {
            self.lobby_full = true;
            retry_delay_sec(e.retry_after_sec, random01())
        } else {
            self.login_failures += 1;
            offline_retry_delay_sec(self.login_failures, random01())
        };
        self.notice = format!("{} Trying again in {seconds}s.", login_failure_text(&e.code));
        self.retry_at = Some(Instant::now() + Duration::from_secs(seconds));
    }

    fn system_line(&mut self, text: impl Into<String>) {
        self.push_chat(ChatLine {
            from: String::new(),
            text: text.into(),
            ts: now_ms(),
            system: true,
        });
    }

    fn push_chat(&mut self, line: ChatLine) {
        self.chat.push_back(line);
        while self.chat.len() > CHAT_LOG_LIMIT {
            self.chat.pop_front();
        }
    }

    /// The new account's key, kept for this run whatever the disk says, so a
    /// reconnect logs back in rather than making another account.
    fn keep_new_key(&mut self, new_key: &str) {
        self.key = Some(new_key.to_owned());
        if let Some(file) = &self.key_file {
            if self.key_unreadable {
                self.notice = format!(
                    "{} could not be read, so this new account's key is not saved",
                    file.display()
                );
            } else if let Err(e) = save_key(file, new_key) {
                self.notice = format!("Could not save the new account's key: {e}");
            }
        }
    }

    fn on_message(&mut self, message: LobbyIn) {
        if let Some(chat) = &mut self.room_chat {
            chat.handle(&message);
        }
        if let Some(room) = &mut self.room {
            room.handle(&message, &self.client);
        }
        if let Some(sync) = &mut self.settings_sync {
            with_cvars(|c| sync.on_message(&message, &mut self.client, c));
            // A download may have brought other bindings (`reloadBindingsFromStorage`).
            if matches!(&message, LobbyIn::Other { kind, .. } if kind == "settings:data") {
                shagger::input::input_system::with_input(
                    shagger::input::input_system::InputSystem::reload_bindings_from_storage,
                );
            }
        }
        match message {
            LobbyIn::Welcome(w) => {
                self.local_lobby_player_id = Some(w.player_id.clone());
                self.name = w
                    .identity
                    .as_ref()
                    .map_or_else(|| w.player_id.clone(), |i| i.name.clone());
                if let Some(new_key) = &w.key {
                    self.keep_new_key(new_key);
                }
                self.login_failures = 0;
                self.retry_at = None;
                self.account_id = w.identity.as_ref().map(|i| i.id.clone());
                self.server_gametypes.clone_from(&w.gametypes);
                self.private_lobby_supported = w
                    .features
                    .as_ref()
                    .and_then(|f| f.get("privateLobbies"))
                    .and_then(Value::as_bool)
                    == Some(true);
                let feature =
                    |name: &str| w.features.as_ref().and_then(|f| f.get(name)).and_then(Value::as_bool) == Some(true);
                self.leaderboard.bind(
                    true,
                    feature("leaderboard") && feature("skillRatings"),
                    self.account_id.clone(),
                );
                if self.tab == Tab::Leaderboard {
                    self.leaderboard.refresh(&mut self.client);
                }
                self.queue_pending = false;
                self.notice.clear();
                self.online = w.online_players.iter().map(|p| p.id.clone()).collect();
                self.live_matches.clone_from(&w.live_matches);
                // Chat history, in place of whatever an earlier connection left.
                self.chat.clear();
                for c in &w.chat_history {
                    self.push_chat(ChatLine {
                        from: c.from.clone(),
                        text: c.text.clone(),
                        ts: c.ts,
                        system: false,
                    });
                }
                // `restoreRoom`: the same account, with the room's invite.
                if let Some((lobby_id, invite, account)) = self.room_to_rejoin.take()
                    && account == self.account_id
                {
                    self.client.join_lobby(&lobby_id, invite.as_deref());
                }
                // `startSettingsSync`, beside the bindings, which `--cvars` can move.
                let dir = shagger::input::input_system::with_input(|i| i.storage().map(Path::to_path_buf))
                    .or_else(crate::config_dir);
                if let Some(dir) = dir {
                    self.settings_sync = with_cvars(|c| SettingsSync::start(&mut self.client, &w, c, &dir));
                }
            }
            LobbyIn::AuthError(e) => self.login_refused(&e),
            LobbyIn::MatchesUpdate(matches) => {
                let before: Vec<(String, String)> = self
                    .live_matches
                    .iter()
                    .map(|m| (m.id.clone(), m.state.clone()))
                    .collect();
                for m in &matches {
                    let was_waiting = before.iter().any(|(id, state)| *id == m.id && state == "waiting");
                    if m.state == "playing" && was_waiting {
                        self.announce_match(m);
                    }
                }
                self.live_matches = matches;
            }
            LobbyIn::Chat(c) => self.push_chat(ChatLine {
                from: c.from,
                text: c.text,
                ts: c.ts,
                system: false,
            }),
            LobbyIn::RoomState(lobby) => self.handle_lobby_created(&lobby),
            LobbyIn::Connect(LobbyConnect {
                server_url,
                map,
                admission_ticket,
                ..
            }) => {
                // `MatchLobbyView`'s handoff: the local player's side.
                let team = self
                    .room
                    .as_ref()
                    .and_then(|room| room.players.iter().find(|p| p.is_local))
                    .map(|p| p.team.clone())
                    .filter(|team| !team.is_empty() && team != "ffa");
                self.last_map = Some(map.clone());
                self.choice = Some(LobbyChoice {
                    server_url,
                    map,
                    admission_ticket,
                    name: self.name.clone(),
                    play: true,
                    demo: None,
                    reel: None,
                    team,
                });
            }
            LobbyIn::LobbyError(e) => {
                println!("lobby: {e}");
                self.queue_pending = false;
                self.pending_private_create = false;
                self.pending_quick_play = false;
                self.notice = e;
            }
            LobbyIn::Other { kind, message } => self.on_other(&kind, &message),
        }
    }

    fn on_other(&mut self, kind: &str, message: &Value) {
        let field = |name: &str| message.get(name).and_then(Value::as_str).map(str::to_owned);
        match kind {
            "player:join" => {
                if let Some(id) = message.pointer("/player/id").and_then(Value::as_str)
                    && !self.online.iter().any(|p| p == id)
                {
                    self.online.push(id.to_owned());
                }
            }
            "player:leave" => {
                if let Some(id) = field("playerId") {
                    self.online.retain(|p| *p != id);
                }
            }
            // This account's new name, from here or another window logged in to it.
            "identity:renamed" => {
                if let Some(name) = message.pointer("/identity/name").and_then(Value::as_str) {
                    name.clone_into(&mut self.name);
                }
            }
            "lobby:kicked" => {
                if self.room.as_ref().map(|r| r.lobby_id.clone()) == field("lobbyId") {
                    self.show_play_stage();
                    let reason = field("reason").unwrap_or_default();
                    self.system_line(format!("Left room: {reason}"));
                }
            }
            // The lobby is the rematch screen: the room stays.
            "match:ended" => self.system_line("Match ended — ready up to rematch"),
            "queue:left" => self.queue_pending = false,
            "leaderboard:page" | "leaderboard:error" => self.leaderboard.on_message(kind, message),
            // `recoverSeat`'s answer.
            "match:seat_reissued" => {
                let Some((request, match_id)) = self.seat_request.take() else {
                    return;
                };
                if field("requestId").as_deref() != Some(request.as_str()) {
                    self.seat_request = Some((request, match_id));
                    return;
                }
                let team = self
                    .room
                    .as_ref()
                    .and_then(|room| room.players.iter().find(|p| p.is_local))
                    .map(|p| p.team.clone())
                    .filter(|team| !team.is_empty() && team != "ffa");
                self.choice = Some(LobbyChoice {
                    server_url: field("serverUrl").unwrap_or_default(),
                    map: self.last_map.clone().unwrap_or_default(),
                    admission_ticket: field("admissionTicket"),
                    name: self.name.clone(),
                    play: true,
                    demo: None,
                    reel: None,
                    team,
                });
            }
            "match:seat_reissue_error" if self.seat_request.as_ref().map(|(r, _)| r.clone()) == field("requestId") => {
                self.seat_request = None;
                if let Some(room) = &mut self.room {
                    room.seat_unavailable();
                }
            }
            _ => {}
        }
    }

    /// "Duel on Proving Grounds started · Reaper vs Eclipse".
    fn announce_match(&mut self, m: &LiveMatchInfo) {
        let mode = GAME_MODES.iter().find(|g| g.id == m.gametype).map_or_else(
            || {
                if m.gametype_name.is_empty() {
                    m.gametype.clone()
                } else {
                    m.gametype_name.clone()
                }
            },
            |g| g.title.to_owned(),
        );
        let place = if m.map_display_name.is_empty() {
            &m.map
        } else {
            &m.map_display_name
        };
        let place = if place.is_empty() {
            String::new()
        } else {
            format!(" on {place}")
        };
        let who = if m.players.len() == 2 {
            m.players.join(" vs ")
        } else {
            let count = if m.players.is_empty() {
                m.player_count as usize
            } else {
                m.players.len()
            };
            format!("{count} players")
        };
        self.system_line(format!("{mode}{place} started · {who}"));
    }

    /// `handleLobbyCreated`.
    fn handle_lobby_created(&mut self, lobby: &Value) {
        let id = lobby.get("id").and_then(Value::as_str).unwrap_or_default();
        if self.room.as_ref().is_some_and(|r| r.lobby_id == id) {
            return;
        }
        let private = lobby.get("visibility").and_then(Value::as_str) == Some("private");
        let quick_play = self.pending_quick_play && private;
        // `currentInviteToken`: a private room's share link carries it.
        let invite = private
            .then(|| lobby.get("shareUrl").and_then(Value::as_str))
            .flatten()
            .and_then(|url| url.split_once("invite=").map(|(_, rest)| rest))
            .map(|rest| rest.split(['&', '#']).next().unwrap_or_default().to_owned())
            .filter(|t| !t.is_empty());
        if self.pending_private_create && (!private || invite.is_none()) {
            self.pending_private_create = false;
            self.pending_quick_play = false;
            self.queue_pending = false;
            self.client.leave_lobby(id);
            "The server did not provide a private invite room.".clone_into(&mut self.notice);
            return;
        }
        self.pending_private_create = false;
        self.pending_quick_play = false;
        self.queue_pending = false;
        self.notice.clear();
        self.room_invite = invite;
        let fill = if quick_play { QUICK_PLAY_BOTS } else { 0 };
        let room = MatchLobbyView::new(
            lobby,
            self.local_lobby_player_id.as_deref(),
            fill,
            now_seed(),
            &self.client,
        );
        // The stage takes the room's colour.
        if let Some(at) = GAME_MODES.iter().position(|m| m.id == room.gametype) {
            self.selected_mode = at;
        }
        self.levelshots.want(room.maps.iter().map(|m| m.name.as_str()));
        self.room_chat = Some(RoomChatView::new(lobby));
        self.room = Some(room);
        self.tab = Tab::Play;
    }

    /// Opens a room from its state, as `lobby:created` does: for drawing a
    /// room without a lobby (`washoi menu --room`).
    pub fn show_room_state(&mut self, lobby: &Value) {
        if self.local_lobby_player_id.is_none() {
            self.local_lobby_player_id = lobby
                .pointer("/players/0/id")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        self.handle_lobby_created(lobby);
    }

    /// `showPlayStage`: the room is gone.
    fn show_play_stage(&mut self) {
        self.room = None;
        self.room_chat = None;
        self.seat_request = None;
        self.room_invite = None;
    }

    fn mode_ready(&self, id: &str) -> bool {
        self.server_gametypes.iter().any(|g| g.id == id)
    }

    /// `refreshQuickPlayKey`: the lobby, its private rooms and FFA, and no room open.
    #[must_use]
    pub fn quick_play_enabled(&self) -> bool {
        self.connected
            && self.private_lobby_supported
            && !self.queue_pending
            && self.room.is_none()
            && self.mode_ready(QUICK_PLAY_MODE)
    }

    /// `startQuickPlay`: a private FFA that adds its bots, votes a map and
    /// readies up by itself.
    pub fn start_quick_play(&mut self) {
        if self.room.is_some() {
            return;
        }
        self.tab = Tab::Play;
        if self.show_private_lobby(QUICK_PLAY_MODE) {
            self.pending_quick_play = true;
        }
    }

    /// `showMatchLobby`: the queue for a mode; the room arrives as `lobby:created`.
    fn show_match_lobby(&mut self, mode: &str) {
        if !self.connected || !self.mode_ready(mode) {
            return;
        }
        self.queue_pending = true;
        "Requesting a room...".clone_into(&mut self.notice);
        self.client.join_queue(mode);
    }

    /// `showPrivateLobby`: false when it cannot be asked for right now.
    fn show_private_lobby(&mut self, mode: &str) -> bool {
        if !self.connected || !self.private_lobby_supported || self.queue_pending || !self.mode_ready(mode) {
            return false;
        }
        self.queue_pending = true;
        self.pending_private_create = true;
        "Creating a private room...".clone_into(&mut self.notice);
        self.client.create_lobby(mode, true);
        true
    }

    /// A room's row: join it, watch its match, or drop into it.
    fn open_room(&mut self, m: &LiveMatchInfo) {
        match describe_room(m).action {
            RoomAction::Join => self.client.join_lobby(&m.id, None),
            RoomAction::Watch => {
                if let Some(url) = &m.server_url {
                    self.choice = Some(LobbyChoice {
                        server_url: url.clone(),
                        map: m.map.clone(),
                        admission_ticket: None,
                        name: self.name.clone(),
                        play: false,
                        demo: None,
                        reel: None,
                        team: None,
                    });
                }
            }
            RoomAction::Full => {}
        }
    }

    fn send_chat(&mut self) {
        let text = self.chat_draft.trim().to_owned();
        if text.is_empty() || !self.connected {
            return;
        }
        self.client.chat_send(&text);
        self.chat_draft.clear();
    }

    /// `refreshPlayerCard`'s second line.
    fn player_meta(&self) -> &'static str {
        if !self.connected {
            if self.lobby_full { "Servers full" } else { "Offline" }
        } else if self.room.is_some() {
            "In a room"
        } else {
            "In lobby"
        }
    }

    fn mode(&self) -> &'static GameMode {
        &GAME_MODES[self.selected_mode.min(GAME_MODES.len() - 1)]
    }

    /// The mode whose colours the page takes: the Leaderboard's own pick.
    fn page_mode(&self) -> &'static GameMode {
        match self.tab {
            Tab::Leaderboard => self.leaderboard.mode(),
            _ => self.mode(),
        }
    }

    /// Draws the lobby over the whole of `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui, style: &TwinStyle) -> Option<PanelAction> {
        let screen = ui.max_rect();
        let time = self.started.elapsed().as_secs_f64();
        let painter = ui.painter().clone();
        let accent = self.page_mode().accent();
        paint_backdrop(&painter, screen);
        paint_light_in(&painter, screen, time, accent.face);
        self.levelshots.poll(ui.ctx());
        let mut action = None;
        let layout = Layout::new(screen);
        if self.shown_tab != Some(self.tab) {
            self.shown_tab = Some(self.tab);
            if self.tab == Tab::Leaderboard {
                self.leaderboard.refresh(&mut self.client);
            }
        }
        self.stage(ui, &layout, style, &mut action);
        self.rail(ui, &layout, style);
        // Keep the light drifting.
        ui.ctx().request_repaint();
        action
    }
}

/// The CSS's sizes for one window size.
struct Layout {
    screen: Rect,
    w: f32,
    h: f32,
    rail_w: f32,
    /// The rail's content: `22px` from its left, `skew + 18px` short of its right.
    rail_left: f32,
    rail_right: f32,
    /// `max-height: 820px` and `680px`.
    short: bool,
    shorter: bool,
    /// `max-width: 1500px`: the rail's hints give way.
    narrow: bool,
    /// `max-width: 1100px`: two tiles a row, the chat under the mode.
    narrower: bool,
}

impl Layout {
    fn new(screen: Rect) -> Self {
        let (w, h) = (screen.width(), screen.height());
        let rail_w = TwinRail::width(w);
        Self {
            screen,
            w,
            h,
            rail_w,
            rail_left: screen.min.x + 22.0,
            rail_right: screen.min.x + rail_w - RAIL_SKEW - 18.0,
            short: w > 700.0 && h <= 820.0,
            shorter: w > 700.0 && h <= 680.0,
            narrow: w <= 1500.0,
            narrower: w <= 1100.0,
        }
    }
}

fn body() -> FontFamily {
    FontFamily::Proportional
}

/// `text` with letter spacing in em.
fn line(painter: &Painter, s: &str, size: f32, family: FontFamily, color: Color32, em: f32) -> Arc<egui::Galley> {
    text(painter, s, size, family, color, size * em)
}

/// Text wrapped at `width`.
fn wrapped(painter: &Painter, s: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(s.to_owned(), font, color, width.max(1.0));
    job.wrap.break_anywhere = false;
    painter.layout_job(job)
}

/// The cut-cornered panel of the chat (`clip-path` off its top right).
fn cut_panel(painter: &Painter, r: Rect, cut: f32, stops: &[(f32, Color32)], border: Option<Stroke>) {
    let shape = [
        r.left_top(),
        pos2(r.right() - cut, r.top()),
        pos2(r.right(), r.top() + cut),
        r.right_bottom(),
        r.left_bottom(),
    ];
    gradient_polygon(painter, &shape, stops);
    if let Some(stroke) = border {
        painter.add(Shape::closed_line(shape.to_vec(), stroke));
    }
}

impl LobbyPanel {
    /// Opens a page by name, as `settings` or `settings/video`: for
    /// screenshots and the in-match Settings key.
    pub fn open_page(&mut self, page: &str) {
        let mut parts = page.split('/');
        let (tab, sub, section) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        self.tab = match tab {
            "leaderboard" => Tab::Leaderboard,
            "demos" => Tab::Demos,
            "settings" => Tab::Settings,
            "profile" => Tab::Profile,
            "console" => Tab::Console,
            _ => Tab::Play,
        };
        if self.tab == Tab::Settings {
            let view = self.settings.get_or_insert_with(FpsSettingsView::default);
            if let Some(p) = SETTINGS_PAGES.iter().find(|p| p.title.eq_ignore_ascii_case(sub)) {
                view.show_page(p.id);
            }
            view.video.open_section(section);
        }
        if self.tab == Tab::Leaderboard {
            self.leaderboard.open_board(sub);
        }
    }

    /// The settings screen, open on `page`.
    pub fn settings_view(&mut self) -> &mut FpsSettingsView {
        self.settings.get_or_insert_with(FpsSettingsView::default)
    }

    /// Escape: closes what the page has open. True when it did.
    pub fn back(&mut self) -> bool {
        self.tab == Tab::Settings && self.settings.as_mut().is_some_and(FpsSettingsView::back)
    }

    /// The rail and everything on it.
    fn rail(&mut self, ui: &mut egui::Ui, l: &Layout, style: &TwinStyle) {
        let painter = ui.painter().clone();
        let (top, bottom) = (l.screen.min.y, l.screen.max.y);
        paint_rail(&painter, l.screen.min.x, top, bottom, l.rail_w, RAIL_SKEW, 1.0);
        let pad_top = if l.shorter { 14.0 } else { 22.0 };
        let logo_max = if l.shorter {
            Some(l.h * 0.11)
        } else if l.short {
            Some(l.h * 0.13)
        } else {
            None
        };
        let mut y = paint_brand(
            &painter,
            l.screen.min.x,
            l.rail_w,
            top + pad_top,
            logo_max,
            l.w,
            style,
            1.0,
        );

        // `lobby__rail-nav`: Quick play, then the pages.
        let key_h = if l.shorter {
            46.0
        } else if l.short {
            54.0
        } else {
            66.0
        };
        let label_size = clamp_css(15.0, l.w * 0.011, 21.0);
        let nav_w = l.rail_right - l.rail_left;
        let play_hint = if self.room.is_some() { "In a room" } else { "4 modes" };
        let keys: [(&str, &str, Option<Tab>); 4] = [
            ("Quick play", "FFA vs bots", None),
            ("Play", play_hint, Some(Tab::Play)),
            ("Leaderboard", "Top players", Some(Tab::Leaderboard)),
            ("Demos", "Replays", Some(Tab::Demos)),
        ];
        for (label, hint, tab) in keys {
            let rect = Rect::from_min_size(pos2(l.rail_left, y), vec2(nav_w, key_h));
            let gold = KeyLook::gold().flat();
            let key = match tab {
                // Quick play: pink, so a new player finds the way into a match.
                None => {
                    let pink = KeyLook {
                        face: vec![
                            (0.0, hsl(322.0, 100.0, 77.0)),
                            (0.55, hsl(322.0, 95.0, 62.0)),
                            (1.0, hsl(318.0, 85.0, 50.0)),
                        ],
                        side: [hsl(312.0, 70.0, 33.0), hsl(306.0, 70.0, 21.0)],
                        shadow: hsla(306.0, 70.0, 14.0, 0.42),
                        ..gold.clone()
                    };
                    let lit = KeyLook {
                        face: vec![
                            (0.0, hsl(322.0, 100.0, 84.0)),
                            (0.55, hsl(322.0, 100.0, 70.0)),
                            (1.0, hsl(318.0, 90.0, 57.0)),
                        ],
                        ..pink.clone()
                    };
                    RaisedKey::new(label, rect, pink)
                        .hover(lit, -0.35)
                        .disabled(!self.quick_play_enabled())
                }
                // The rail keeps its gold under the pointer; only the lift says so.
                Some(tab) => RaisedKey::new(label, rect, gold.clone())
                    .hover(gold, -0.35)
                    .current(self.tab == tab),
            };
            let face = key.show(ui);
            let hint = if l.narrow { "" } else { hint };
            KeyText::new(label, hint, display_family(), body())
                .size(label_size, 13.0)
                .pad(28.0, 24.0)
                .paint(&painter, &face);
            if face.clicked {
                match tab {
                    None => self.start_quick_play(),
                    Some(tab) => self.tab = tab,
                }
            }
            y += key_h + 6.0 + 8.0;
        }

        // The foot: the player card over the utility keys.
        let util_h = 40.0;
        let util_top = bottom - 18.0 - 6.0 - 3.6 - util_h;
        let card_h = 62.0;
        let card_top = util_top - 18.0 - card_h;
        self.player_card(
            &painter,
            Rect::from_min_size(pos2(l.rail_left, card_top), vec2(nav_w, card_h)),
        );
        let gap = 10.0;
        let util_w = (nav_w - 2.0 * gap) / 3.0;
        for (i, (label, tab)) in [
            ("Profile", Tab::Profile),
            ("Settings", Tab::Settings),
            ("Console", Tab::Console),
        ]
        .into_iter()
        .enumerate()
        {
            let x = l.rail_left + i as f32 * (util_w + gap);
            let rect = Rect::from_min_size(pos2(x, util_top), vec2(util_w, util_h));
            let face = RaisedKey::new(label, rect, KeyLook::gold().sm().flat())
                .current(self.tab == tab)
                .show(ui);
            let (size, em) = if l.narrow { (10.0, 0.06) } else { (11.0, 0.12) };
            KeyText::new(label, "", display_family(), body())
                .size(size, 13.0)
                .spacing(em)
                .pad(4.0, 4.0)
                .layout(KeyTextLayout::Centered)
                .paint(&painter, &face);
            if face.clicked {
                self.tab = tab;
            }
        }

        // `lobby__rail-mid--rooms`: what is left between.
        let mid = Rect::from_min_max(pos2(l.screen.min.x, y), pos2(l.screen.min.x + l.rail_w, card_top - 4.0));
        // On Settings, the page keys take the middle; in a room, the lobby
        // list gives way to the room's chat.
        if self.tab == Tab::Settings {
            self.settings_nav(ui, l, mid);
        } else if self.tab == Tab::Leaderboard {
            self.leaderboard.rail(ui, mid, l.rail_w, &mut self.client);
        } else if self.room_chat.is_some() {
            self.room_chat_panel(ui, l, mid);
        } else if mid.height() > 40.0 {
            self.rooms(ui, l, mid);
        }
    }

    fn player_card(&self, painter: &Painter, r: Rect) {
        let cut = r.width() * 0.03;
        painter.add(Shape::convex_polygon(
            vec![
                pos2(r.left() + cut, r.top()),
                r.right_top(),
                pos2(r.right() - cut, r.bottom()),
                r.left_bottom(),
            ],
            hsla(250.0, 45.0, 7.0, 0.92),
            Stroke::NONE,
        ));
        // `lobby__rail-avatar`: cyan to violet, a skewed square.
        let a = Rect::from_min_size(pos2(r.left() + 16.0, r.center().y - 19.0), vec2(38.0, 38.0));
        let corners = [
            pos2(a.left() + a.width() * 0.14, a.top()),
            pos2(a.right(), a.top() + a.height() * 0.08),
            pos2(a.left() + a.width() * 0.88, a.bottom()),
            pos2(a.left(), a.top() + a.height() * 0.9),
        ];
        gradient_polygon(painter, &corners, &[(0.0, hsl(182.0, 95.0, 55.0)), (1.0, violet())]);
        let text_left = a.right() + 12.0;
        let width = r.right() - 16.0 - text_left;
        let display = display_family();
        let name = if self.name.is_empty() { "Player" } else { &self.name };
        let name = elide(painter, name, 14.0, display.clone(), width);
        let name = line(painter, &name, 14.0, display, cream(), 0.05);
        let meta = line(
            painter,
            self.player_meta(),
            12.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.55),
            0.08,
        );
        let total = name.size().y + 3.0 + meta.size().y;
        let y = r.center().y - total / 2.0;
        let name_h = name.size().y;
        painter.galley(pos2(text_left, y), name, cream());
        painter.galley(pos2(text_left, y + name_h + 3.0), meta, cream());
    }

    /// `.lobby__rail .fps-set-pages`: the settings' page keys in the rail.
    fn settings_nav(&mut self, ui: &mut egui::Ui, l: &Layout, mid: Rect) {
        let painter = ui.painter().clone();
        let sub = hsla(250.0, 45.0, 8.0, 0.62);
        let heading = line(&painter, "SETTINGS", 11.0, body(), sub, 0.26);
        painter.galley(pos2(mid.left() + 24.0, mid.top() + 6.0), heading, sub);
        let list = Rect::from_min_max(
            pos2(mid.left() + 22.0, mid.top() + 6.0 + 11.0 + 14.0),
            pos2(mid.left() + l.rail_w - RAIL_SKEW - 12.0 - 14.0, mid.bottom()),
        );
        let key_h = if l.h <= 900.0 { 44.0 } else { 58.0 };
        let label_size = clamp_css(14.0, l.w * 0.01, 18.0);
        let view = self.settings.get_or_insert_with(FpsSettingsView::default);
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("settings-pages")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let top = ui.cursor().min;
                    let r = Rect::from_min_size(top, vec2(list.width() - 4.0, 1.0));
                    let used = view.page_nav(ui, r, key_h, label_size, !l.narrow);
                    ui.allocate_rect(Rect::from_min_size(top, vec2(r.width(), used)), egui::Sense::hover());
                });
        });
    }

    /// The rooms list in the rail's middle.
    fn rooms(&mut self, ui: &mut egui::Ui, l: &Layout, mid: Rect) {
        let painter = ui.painter().clone();
        let sub = hsla(250.0, 45.0, 8.0, 0.62);
        let label_y = mid.top() + 6.0;
        let heading = line(&painter, "ROOMS", 11.0, body(), sub, 0.26);
        painter.galley(pos2(mid.left() + 24.0, label_y), heading, sub);
        let matches = sort_rooms(&self.live_matches);
        let count = line(
            &painter,
            &room_count_label(&matches).to_uppercase(),
            11.0,
            body(),
            sub,
            0.26,
        );
        let count_x = mid.left() + l.rail_w - RAIL_SKEW - 26.0 - count.size().x;
        painter.galley(pos2(count_x, label_y), count, sub);
        let list = Rect::from_min_max(
            pos2(mid.left() + 22.0, label_y + 11.0 + 12.0),
            pos2(mid.left() + l.rail_w - RAIL_SKEW - 12.0, mid.bottom()),
        );
        if matches.is_empty() {
            let empty = if self.connected {
                "No rooms yet. Pick a mode to start one."
            } else if self.lobby_full {
                "Servers are full."
            } else {
                "Lobby server offline."
            };
            let g = wrapped(
                &painter,
                empty,
                FontId::proportional(15.0),
                sub,
                list.width().min(220.0),
            );
            painter.galley(pos2(list.left(), list.top() + 4.0), g, sub);
            return;
        }
        let mut open = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("lobby-rooms")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for m in &matches {
                        let (slot, _) =
                            ui.allocate_exact_size(vec2(list.width() - 14.0, 46.0 + 9.6), egui::Sense::hover());
                        let rect = Rect::from_min_size(slot.min, vec2(slot.width() - 4.0, 46.0));
                        if room_row(ui, rect, m).clicked {
                            open = Some(m.clone());
                        }
                    }
                });
        });
        if let Some(m) = open {
            self.open_room(&m);
        }
    }

    /// The stage: the Play page, a room, or a page not built yet.
    fn stage(&mut self, ui: &mut egui::Ui, l: &Layout, style: &TwinStyle, action: &mut Option<PanelAction>) {
        let painter = ui.painter().clone();
        let mode = *self.mode();
        let accent = self.page_mode().accent();
        let page_left = l.screen.min.x + l.rail_w - RAIL_SKEW;
        let page_w = l.screen.max.x - page_left;
        // `lobby__ghost`: the mode's name, huge and outlined, off the bottom
        // left. It shows behind a room too, in the room's colour.
        let ghost = match self.tab {
            Tab::Play => Some(mode.title.to_uppercase()),
            Tab::Settings => Some(self.settings_view().ghost().to_owned()),
            Tab::Leaderboard => Some("TOP".to_owned()),
            _ => None,
        };
        if let Some(word) = ghost {
            let size = clamp_css(200.0, l.w * 0.25, 440.0);
            let at = pos2(page_left + RAIL_SKEW - page_w * 0.01, l.screen.max.y + l.h * 0.04);
            let clip = painter.with_clip_rect(Rect::from_min_max(pos2(page_left, l.screen.min.y), l.screen.max));
            paint_outline_text(
                &clip,
                DISPLAY_FONT,
                &word,
                size,
                -0.04 * size,
                at,
                Stroke::new(2.0, accent.face.gamma_multiply(0.26)),
            );
        }
        // `lobby__links` along the foot, and the page above it.
        let links_h = 44.0 + 5.0 + 12.0;
        let content = Rect::from_min_max(
            pos2(l.screen.min.x + l.rail_w + 28.0, l.screen.min.y + 30.0),
            pos2(l.screen.max.x - 52.0, l.screen.max.y - links_h - 20.0),
        );
        if let Some(a) = self.links(ui, l, style) {
            *action = Some(a);
        }
        match self.tab {
            Tab::Play => self.play_page(ui, l, content, &mode),
            Tab::Settings => self.settings_view().ui(ui, content),
            Tab::Leaderboard => self.leaderboard.ui(ui, content, &mut self.client),
            other => {
                if not_yet_page(ui, content, other, &accent) {
                    *action = Some(PanelAction::WebLobby);
                }
            }
        }
    }

    /// `lobby__links`: the version and the devlog on the Play page, and Quit
    /// (the shell's), right to left along the foot.
    fn links(&mut self, ui: &mut egui::Ui, l: &Layout, style: &TwinStyle) -> Option<PanelAction> {
        let painter = ui.painter().clone();
        let bottom = l.screen.max.y - 12.0 - 5.0;
        let mut right = l.screen.max.x - 52.0;
        let mut action = None;
        let key = |ui: &mut egui::Ui, right: &mut f32, label: &str| {
            let g = line(&painter, label, 13.0, display_family(), hsl(43.0, 100.0, 94.0), 0.12);
            let r = Rect::from_min_max(pos2(*right - g.size().x - 32.0, bottom - 44.0), pos2(*right, bottom));
            *right = r.left() - 12.0;
            link_key(ui, &painter, r, g, label)
        };
        if key(ui, &mut right, "QUIT") {
            action = Some(PanelAction::Quit);
        }
        // On the Play page only: a room keeps the foot clear.
        if self.tab == Tab::Play && self.room.is_none() {
            if key(ui, &mut right, "DEVLOG") {
                action = Some(PanelAction::OpenUrl(DEVLOG_URL));
            }
            // The version, so a screenshot for a bug report carries it.
            let version = line(
                &painter,
                &format!("client {}", style.version.trim_start_matches('v')),
                15.0,
                display_family(),
                hsla(45.0, 100.0, 92.0, 0.7),
                0.16,
            );
            let at = pos2(right - version.size().x, bottom - 22.0 - version.size().y / 2.0);
            painter.galley_with_override_text_color(at + vec2(0.0, 2.0), version.clone(), hsla(0.0, 0.0, 0.0, 0.5));
            painter.galley(at, version, cream());
        }
        action
    }

    /// Kicker and title (`lobby__heading`); returns its bottom.
    fn heading(painter: &Painter, l: &Layout, at: Pos2, kicker: &str, title: &str, accent: Color32) -> f32 {
        let kicker = line(painter, &kicker.to_uppercase(), 12.0, body(), accent, 0.32);
        let kicker_h = kicker.size().y;
        painter.galley(at, kicker, accent);
        let size = clamp_css(40.0, l.w * 0.044, 74.0);
        let title = line(painter, title, size, display_family(), cream(), 0.0);
        let ty = at.y + kicker_h + 8.0;
        painter.galley_with_override_text_color(pos2(at.x + 5.0, ty + 6.0), title.clone(), hsl(250.0, 50.0, 3.0));
        let th = title.size().y;
        painter.galley(pos2(at.x, ty), title, cream());
        ty + th
    }

    fn play_page(&mut self, ui: &mut egui::Ui, l: &Layout, content: Rect, mode: &GameMode) {
        let painter = ui.painter().clone();
        let accent = mode.accent();
        if self.room.is_some() {
            self.room_page(ui, l, content, &accent);
            return;
        }
        let bottom = Self::heading(&painter, l, content.min, "Play", "Pick your fight", accent.face);

        // `lobby__play-tiles`: a column per mode, two a row when narrow.
        let tile_h = clamp_css(130.0, l.h * 0.18, 176.0);
        let per_row = if l.narrower { 2 } else { 4 };
        let gap = 24.0;
        let tile_w = (content.width() - gap * (per_row as f32 - 1.0)) / per_row as f32;
        let mut y = bottom + 22.0 + 10.0;
        for (i, m) in GAME_MODES.iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let rect = Rect::from_min_size(
                pos2(
                    content.left() + col as f32 * (tile_w + gap),
                    y + row as f32 * (tile_h + 9.0 + 8.0 + gap * 0.5),
                ),
                vec2(tile_w, tile_h),
            );
            if self.mode_tile(ui, rect, i, m) {
                self.selected_mode = i;
            }
        }
        let rows = GAME_MODES.len().div_ceil(per_row) as f32;
        y += rows * (tile_h + 9.0 + 8.0) + (rows - 1.0) * gap * 0.5 + 22.0;

        // `lobby__play-lower`: the mode on the left, the chat on the right.
        let lower = Rect::from_min_max(pos2(content.left(), y), content.max);
        let (detail, chat) = if l.narrower {
            let split = lower.top() + 200.0_f32.min(lower.height() * 0.5);
            (
                Rect::from_min_max(lower.min, pos2(lower.right(), split)),
                Rect::from_min_max(pos2(lower.left(), split + 16.0), lower.max),
            )
        } else {
            let left_w = (lower.width() - 42.0) / 2.12;
            (
                Rect::from_min_max(lower.min, pos2(lower.left() + left_w, lower.bottom())),
                Rect::from_min_max(pos2(lower.left() + left_w + 42.0, lower.top()), lower.max),
            )
        };
        self.detail(ui, detail, mode);
        if chat.height() > 120.0 {
            self.chat_panel(ui, chat);
        }
    }

    /// A room: the whole page is `MatchLobbyView`'s.
    fn room_page(&mut self, ui: &mut egui::Ui, l: &Layout, content: Rect, accent: &Accent) {
        let Some(room) = &mut self.room else { return };
        let shots = &self.levelshots;
        let map_shot = |map: &str| shots.texture(map);
        let style = RoomStyle {
            accent: *accent,
            display: display_family(),
            body: body(),
            window_width: l.w,
            short: l.short,
            narrow: l.narrow,
            narrower: l.narrower,
            site: &self.site,
            online: self.connected,
            map_shot: &map_shot,
        };
        match room.ui(ui, content, &self.client, &style) {
            Some(ViewAction::Leave) => {
                self.show_play_stage();
                self.room_to_rejoin = None;
            }
            Some(ViewAction::Rejoin(match_id)) => {
                let request = self.client.request_seat_reissue(&match_id);
                self.seat_request = Some((request, match_id));
            }
            None => {}
        }
    }

    /// A mode's tile (`raised-key--mode`); true when picked.
    fn mode_tile(&self, ui: &mut egui::Ui, rect: Rect, index: usize, m: &GameMode) -> bool {
        let accent = m.accent();
        let on = index == self.selected_mode;
        let rest = KeyLook {
            face: vec![
                (0.0, hsl(45.0, 100.0, 97.0)),
                (0.62, hsl(42.0, 58.0, 89.0)),
                (1.0, hsl(40.0, 40.0, 80.0)),
            ],
            side: [hsl(36.0, 24.0, 56.0), hsl(30.0, 20.0, 38.0)],
            shadow: hsla(250.0, 70.0, 2.0, 0.6),
            depth: 9.0,
            cut: 0.022,
            ..KeyLook::gold()
        };
        let picked = KeyLook {
            face: vec![(0.0, accent.hi), (0.58, accent.face), (1.0, accent.lo)],
            side: [accent.side_hi, accent.side_lo],
            sub: hsla(250.0, 45.0, 8.0, 0.66),
            ..rest.clone()
        };
        let key = if on {
            RaisedKey::new(("mode", m.id), rect, picked.clone())
                .rest_press(-0.5)
                .hover(picked, -0.75)
                .active_face(vec![(0.0, accent.face), (1.0, accent.lo)])
        } else {
            RaisedKey::new(("mode", m.id), rect, rest)
        };
        let face = key.show(ui);
        let painter = ui.painter();
        // `lobby__mode-band`: the mode's colour down the left edge.
        let r = face.rect;
        let cut = rect.width() * 0.022;
        let band = if on { accent.side_hi } else { accent.face };
        painter.add(Shape::convex_polygon(
            vec![
                pos2(r.left() + cut, r.top()),
                pos2(r.left() + cut + 16.0, r.top()),
                pos2(r.left() + 16.0, r.bottom()),
                r.left_bottom(),
            ],
            band,
            Stroke::NONE,
        ));
        let inner = Rect::from_min_max(r.min + vec2(40.0, 18.0), r.max - vec2(26.0, 22.0));
        let number = line(
            painter,
            &format!("{:02}", index + 1),
            13.0,
            display_family(),
            face.sub,
            0.14,
        );
        painter.galley(inner.left_top(), number, face.sub);
        let available = self.mode_ready(m.id);
        if self.connected && !available {
            let soon = line(painter, "SOON", 13.0, display_family(), face.sub, 0.14);
            painter.galley(pos2(inner.right() - soon.size().x, inner.top()), soon, face.sub);
        }
        let name_size = clamp_css(28.0, ui.ctx().content_rect().width() * 0.026, 46.0);
        let name = line(
            painter,
            &m.title.to_uppercase(),
            name_size,
            display_family(),
            face.text,
            0.02,
        );
        let subtitle = line(painter, &m.subtitle.to_uppercase(), 14.0, body(), face.sub, 0.18);
        let sub_y = inner.bottom() - subtitle.size().y;
        let name_y = sub_y - 8.0 - name.size().y;
        painter.galley(pos2(inner.left(), name_y), name, face.text);
        painter.galley(pos2(inner.left(), sub_y), subtitle, face.sub);
        face.clicked
    }

    /// `lobby__play-detail`: the hook, Find and Private lobby, the notice.
    fn detail(&mut self, ui: &mut egui::Ui, r: Rect, mode: &GameMode) {
        let painter = ui.painter().clone();
        let tagline = wrapped(
            &painter,
            mode.hook,
            FontId::proportional(24.0),
            hsla(45.0, 100.0, 95.0, 0.9),
            r.width().min(520.0),
        );
        let mut y = r.top();
        let tag_h = tagline.size().y;
        painter.galley(pos2(r.left(), y), tagline, cream());
        y += tag_h + 16.0 + 8.0;
        let keys = play_keys(PlayKeysInput {
            mode_title: mode.title,
            available: self.mode_ready(mode.id),
            connected: self.connected,
            queue_pending: self.queue_pending,
            private_lobby_supported: self.private_lobby_supported,
            lobby_full: self.lobby_full,
        });
        // The arcade key: its label's width, padded.
        let label = line(
            &painter,
            &keys.find_label.to_uppercase(),
            30.0,
            display_family(),
            cream(),
            0.1,
        );
        let find_w = (label.size().x + 126.0).min(r.width());
        let find = Rect::from_min_size(pos2(r.left(), y), vec2(find_w, 90.0));
        let face = RaisedKey::new("find", find, KeyLook::cta(&mode.accent()))
            .disabled(keys.find_disabled)
            .show(ui);
        KeyText::new(&keys.find_label, "", display_family(), body())
            .size(30.0, 13.0)
            .spacing(0.1)
            .pad(60.0, 66.0)
            .layout(KeyTextLayout::Centered)
            .paint(&painter, &face);
        if face.clicked {
            self.show_match_lobby(mode.id);
        }
        // Private lobby, beside it when it fits, else under it.
        let private_label = "Private lobby";
        let hint = "Invite only or bot matches";
        let lw = line(
            &painter,
            &private_label.to_uppercase(),
            16.0,
            display_family(),
            cream(),
            0.1,
        )
        .size()
        .x;
        let hw = line(&painter, &hint.to_uppercase(), 13.0, body(), cream(), 0.12)
            .size()
            .x;
        let private_w = lw.max(hw) + 62.0;
        let beside = find.right() + 9.6 * 0.75 + 30.0;
        let at = if beside + private_w <= r.right() {
            pos2(beside, find.center().y - 32.0)
        } else {
            pos2(r.left(), find.bottom() + 9.6 + 12.0)
        };
        let private = Rect::from_min_size(at, vec2(private_w, 64.0));
        let face = RaisedKey::new("private", private, KeyLook::cream().depth(6.6))
            .disabled(keys.private_disabled)
            .show(ui);
        KeyText::new(private_label, hint, display_family(), body())
            .size(16.0, 13.0)
            .spacing(0.1)
            .pad(32.0, 30.0)
            .layout(KeyTextLayout::Stacked)
            .paint(&painter, &face);
        if face.clicked {
            self.show_private_lobby(mode.id);
        }
        let notice_y = private.bottom().max(find.bottom()) + 9.6 + 16.0;
        if !self.notice.is_empty() {
            let n = wrapped(
                &painter,
                &self.notice,
                FontId::proportional(16.0),
                hsl(46.0, 100.0, 80.0),
                r.width(),
            );
            painter.galley(pos2(r.left(), notice_y), n, cream());
        }
    }

    /// `lobby-chat`: the lobby's chat beside the mode.
    fn chat_panel(&mut self, ui: &mut egui::Ui, r: Rect) {
        let count = if self.connected {
            format!("{} ONLINE", self.online.len())
        } else {
            "OFFLINE".into()
        };
        let send = chat_box(
            ui,
            r,
            ChatBox {
                id: "lobby-chat",
                title: "LOBBY",
                count: &count,
                dot: true,
                lines: &self.chat,
                draft: &mut self.chat_draft,
                placeholder: if self.connected {
                    "Say something…"
                } else {
                    "Lobby offline"
                },
                enabled: self.connected,
                empty: if self.connected {
                    "Nobody has said anything yet. Say hello."
                } else {
                    "Chat opens when the lobby server is back."
                },
                mine: &self.name,
                rail: false,
            },
        );
        if send {
            self.send_chat();
        }
    }

    /// The room's chat in the rail's middle (`lobby__rail--room .lobby-chat`).
    fn room_chat_panel(&mut self, ui: &mut egui::Ui, l: &Layout, mid: Rect) {
        let Some(chat) = &mut self.room_chat else { return };
        let r = Rect::from_min_max(
            pos2(mid.left() + 22.0, mid.top() + 4.0),
            pos2(mid.left() + l.rail_w - RAIL_SKEW - 18.0, mid.bottom() - 16.0),
        );
        if r.height() < 90.0 {
            return;
        }
        let enabled = self.connected && chat.supported;
        let count = chat.here().to_uppercase();
        let placeholder = chat.placeholder(self.connected);
        let empty = chat.empty_text();
        let send = chat_box(
            ui,
            r,
            ChatBox {
                id: "room-chat",
                title: "ROOM CHAT",
                count: &count,
                dot: false,
                lines: &chat.lines,
                draft: &mut chat.draft,
                placeholder,
                enabled,
                empty,
                mine: &self.name,
                rail: true,
            },
        );
        if send {
            chat.send(&self.client, self.connected);
        }
    }
}

/// What a chat box shows: the lobby's on the Play page, a room's in the rail.
struct ChatBox<'a> {
    id: &'static str,
    title: &'a str,
    count: &'a str,
    /// The pink online dot beside the count.
    dot: bool,
    lines: &'a VecDeque<ChatLine>,
    draft: &'a mut String,
    placeholder: &'a str,
    enabled: bool,
    empty: &'a str,
    /// This player's name, whose lines are lit.
    mine: &'a str,
    /// In the rail: a solid slab, tighter, its cut corner smaller.
    rail: bool,
}

/// `lobby-chat`: head, log and form. True when the player sends a line.
fn chat_box(ui: &mut egui::Ui, r: Rect, c: ChatBox<'_>) -> bool {
    let painter = ui.painter().clone();
    let rule = Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.2));
    if c.rail {
        cut_panel(
            &painter,
            r,
            18.0,
            &[(0.0, hsla(250.0, 45.0, 7.0, 0.94)), (1.0, hsla(250.0, 45.0, 7.0, 0.94))],
            None,
        );
    } else {
        cut_panel(
            &painter,
            r,
            22.0,
            &[
                (0.0, hsla(250.0, 40.0, 11.0, 0.72)),
                (1.0, hsla(250.0, 44.0, 7.0, 0.82)),
            ],
            Some(Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.32))),
        );
    }
    let inner = if c.rail {
        Rect::from_min_max(r.min + vec2(16.0, 14.0), r.max - vec2(16.0, 14.0))
    } else {
        Rect::from_min_max(r.min + vec2(22.0, 16.0), r.max - vec2(20.0, 16.0))
    };
    // `lobby-chat__head`.
    let title = line(&painter, c.title, 14.0, display_family(), cream(), 0.16);
    let head_h = title.size().y;
    painter.galley(inner.left_top(), title, cream());
    let count = line(&painter, c.count, 12.0, body(), hsla(45.0, 100.0, 95.0, 0.55), 0.14);
    let cx = inner.right() - count.size().x;
    let cy = inner.top() + head_h / 2.0;
    painter.galley(pos2(cx, cy - count.size().y / 2.0), count, cream());
    if c.dot {
        let pink = hsl(322.0, 100.0, 58.0);
        painter.circle_filled(pos2(cx - 12.0, cy), 7.0, pink.gamma_multiply(0.18));
        painter.circle_filled(pos2(cx - 12.0, cy), 4.0, pink);
    }
    let head_bottom = inner.top() + head_h + 12.0;
    painter.hline(inner.x_range(), head_bottom, rule);

    // `lobby-chat__form`, at the foot.
    let form_top = inner.bottom() - 44.0 - 3.6;
    painter.hline(inner.x_range(), form_top - 12.0, rule);
    let send_label = line(&painter, "SEND", 13.0, display_family(), cream(), 0.12);
    let pad = if c.rail { (16.0, 14.0) } else { (24.0, 22.0) };
    let send_w = send_label.size().x + pad.0 + pad.1;
    let send = Rect::from_min_size(pos2(inner.right() - send_w - 4.0, form_top), vec2(send_w, 44.0));
    let gap = if c.rail { 10.0 } else { 14.0 };
    let field = Rect::from_min_max(pos2(inner.left(), form_top), pos2(send.left() - gap, form_top + 44.0));
    let typed = TextField::new(c.id, field, c.draft, FontId::proportional(18.0))
        .placeholder(c.placeholder)
        .enabled(c.enabled)
        .show(ui);
    let face = RaisedKey::new((c.id, "send"), send, KeyLook::gold().sm())
        .disabled(!c.enabled)
        .show(ui);
    KeyText::new("Send", "", display_family(), body())
        .size(13.0, 13.0)
        .spacing(0.12)
        .pad(pad.0, pad.1)
        .layout(KeyTextLayout::Centered)
        .paint(&painter, &face);
    let sent = c.enabled && (typed.submitted || face.clicked);

    // `lobby-chat__log`: follows new lines while at the bottom.
    let log = Rect::from_min_max(
        pos2(inner.left(), head_bottom + 6.0),
        pos2(inner.right(), form_top - 18.0),
    );
    if log.height() < 20.0 {
        return sent;
    }
    if c.lines.is_empty() {
        let size = if c.rail { 16.0 } else { 17.0 };
        let alpha = if c.rail { 0.58 } else { 0.5 };
        let g = wrapped(
            &painter,
            c.empty,
            FontId::proportional(size),
            hsla(45.0, 100.0, 95.0, alpha),
            log.width(),
        );
        painter.galley(pos2(log.left(), log.top() + 14.0), g, cream());
        return sent;
    }
    let time_w = if c.rail { 42.0 } else { 50.0 };
    let size = if c.rail { 16.0 } else { 18.0 };
    ui.scope_builder(egui::UiBuilder::new().max_rect(log), |ui| {
        egui::ScrollArea::vertical()
            .id_salt((c.id, "log"))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for chat in c.lines {
                    chat_row(ui, chat, c.mine, log.width() - 12.0, time_w, size);
                }
            });
    });
    sent
}

/// `lobby-credits-key`: a dark key on a 5px base, sinking onto it when held.
fn link_key(ui: &mut egui::Ui, painter: &Painter, r: Rect, label: Arc<egui::Galley>, id: &str) -> bool {
    let response = ui.interact(
        r.expand2(vec2(0.0, 5.0)),
        ui.scope_id().with(("link", id)),
        egui::Sense::click(),
    );
    let held = response.is_pointer_button_down_on();
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let sink = if held { 4.0 } else { 0.0 };
    let base = r.translate(vec2(0.0, 5.0));
    painter.rect_filled(
        base.translate(vec2(0.0, 4.0 - sink * 0.8)).expand(1.0),
        4.0,
        hsla(0.0, 0.0, 0.0, 0.25),
    );
    painter.rect_filled(base, 4.0, Color32::from_rgb(0x0d, 0x0b, 0x16));
    let face = r.translate(vec2(0.0, sink));
    let (top, foot) = if hovered {
        (Color32::from_rgb(0x57, 0x4b, 0x6d), Color32::from_rgb(0x28, 0x21, 0x37))
    } else {
        (Color32::from_rgb(0x39, 0x35, 0x4b), Color32::from_rgb(0x19, 0x16, 0x26))
    };
    painter.add(egui::epaint::RectShape::filled(face, 4.0, foot));
    gradient_polygon(
        painter,
        &[
            face.left_top() + vec2(2.0, 0.0),
            face.right_top() - vec2(2.0, 0.0),
            face.right_bottom() - vec2(2.0, 0.0),
            face.left_bottom() + vec2(2.0, 0.0),
        ],
        &[(0.0, top), (1.0, foot)],
    );
    painter.hline(
        (face.left() + 3.0)..=(face.right() - 3.0),
        face.top() + 0.5,
        Stroke::new(1.0, hsla(0.0, 0.0, 100.0, 0.2)),
    );
    let size = label.size();
    painter.galley(face.center() - size / 2.0, label, cream());
    response.clicked()
}

/// One room in the rail (`raised-key--sm raised-key--cream raised-key--room`).
fn room_row(ui: &mut egui::Ui, rect: Rect, m: &LiveMatchInfo) -> KeyFace {
    let row = describe_room(m);
    let mode = GAME_MODES.iter().find(|g| g.id == m.gametype);
    let face = RaisedKey::new(("room", m.id.as_str()), rect, KeyLook::cream().sm())
        // A full room is still worth reading, so it stays lit.
        .disabled(row.disabled)
        .show(ui);
    let painter = ui.painter();
    let r = face.rect;
    let (text_c, sub) = if row.disabled {
        (face.text.gamma_multiply(1.4), face.sub.gamma_multiply(1.4))
    } else {
        (face.text, face.sub)
    };
    let mut x = r.left() + 20.0;
    let cy = r.center().y;
    // `lobby__room-mark`, pulsing while the match runs.
    let (h, s, l) = mode.map_or((250.0, 20.0, 40.0), |g| g.mark);
    let pulse = if m.state == "playing" {
        let t = ui.ctx().input(|i| i.time) as f32;
        0.65 + 0.35 * (t * std::f32::consts::TAU / 1.6).cos()
    } else {
        1.0
    };
    painter.add(Shape::convex_polygon(
        vec![
            pos2(x + 2.2, cy - 5.0),
            pos2(x + 10.0, cy - 5.0),
            pos2(x + 7.8, cy + 5.0),
            pos2(x, cy + 5.0),
        ],
        hsl(h, s, l).gamma_multiply(pulse),
        Stroke::NONE,
    ));
    x += 10.0 + 10.0;
    let title = mode.map_or_else(
        || {
            if m.gametype_name.is_empty() {
                m.gametype.clone()
            } else {
                m.gametype_name.clone()
            }
        },
        |g| g.title.to_owned(),
    );
    let name = line(painter, &title.to_uppercase(), 14.0, display_family(), text_c, 0.07);
    painter.galley(pos2(x, cy - name.size().y / 2.0), name, text_c);
    x += 66.0 + 10.0;
    // From the right: the badge, then the seats.
    let badge_text = line(
        painter,
        &row.action.label().to_uppercase(),
        10.0,
        display_family(),
        Color32::WHITE,
        0.1,
    );
    let badge_w = (badge_text.size().x + 20.0).max(54.0);
    let badge = Rect::from_min_size(pos2(r.right() - 16.0 - badge_w, cy - 9.0), vec2(badge_w, 18.0));
    let (badge_fg, badge_bg) = match row.action {
        RoomAction::Join => (hsl(46.0, 100.0, 60.0), hsl(250.0, 45.0, 7.0)),
        RoomAction::Watch if face.hovered => (hsla(45.0, 100.0, 96.0, 0.85), Color32::from_white_alpha(41)),
        RoomAction::Watch => (hsl(322.0, 85.0, 38.0), hsla(322.0, 90.0, 52.0, 0.16)),
        RoomAction::Full => (hsla(250.0, 45.0, 8.0, 0.45), hsla(250.0, 45.0, 8.0, 0.1)),
    };
    let bc = badge.width() * 0.08;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(badge.left() + bc, badge.top()),
            badge.right_top(),
            pos2(badge.right() - bc, badge.bottom()),
            badge.left_bottom(),
        ],
        badge_bg,
        Stroke::NONE,
    ));
    let badge_label = line(
        painter,
        &row.action.label().to_uppercase(),
        10.0,
        display_family(),
        badge_fg,
        0.1,
    );
    painter.galley(badge.center() - badge_label.size() / 2.0, badge_label, badge_fg);
    let seats = line(
        painter,
        &format!("{} / {}", m.player_count, m.max_players),
        13.0,
        body(),
        sub,
        0.12,
    );
    let seats_x = badge.left() - 10.0 - seats.size().x.max(42.0);
    painter.galley(pos2(seats_x, cy - seats.size().y / 2.0), seats, sub);
    // The map, while it is being played, in what is left.
    if m.state == "playing" {
        let room = seats_x - 10.0 - x;
        if room > 20.0 {
            let shown = elide(painter, &row.where_, 13.0, body(), room);
            let map = line(painter, &shown, 13.0, body(), sub, 0.06);
            painter.galley(pos2(x, cy - map.size().y / 2.0), map, sub);
        }
    }
    face
}

/// One chat line: time, who, what (`lobby-chat__msg`).
fn chat_row(ui: &mut egui::Ui, chat: &ChatLine, mine: &str, width: f32, time_w: f32, size: f32) {
    let painter = ui.painter().clone();
    let time = line(
        &painter,
        &clock_time(chat.ts),
        12.0,
        body(),
        hsla(45.0, 100.0, 95.0, 0.3),
        0.08,
    );
    let text_x = time_w + 12.0;
    let sys = size - 2.0;
    let (who, body_text, color) = if chat.system || chat.from.is_empty() {
        let mut job = egui::text::LayoutJob::default();
        // The diamond is drawn: the body face has no `◆`.
        job.append(
            &chat.text,
            sys + 6.0,
            egui::TextFormat {
                font_id: FontId::proportional(sys),
                color: hsla(45.0, 100.0, 95.0, 0.45),
                italics: true,
                ..Default::default()
            },
        );
        job.wrap.max_width = (width - text_x).max(10.0);
        (None, painter.layout_job(job), cream())
    } else {
        let me = chat.from == mine;
        let who_color = if me {
            hsl(46.0, 100.0, 60.0)
        } else {
            name_color(&chat.from)
        };
        let who = line(&painter, &chat.from, 14.0, display_family(), who_color, 0.04);
        let left = text_x + who.size().x + 12.0;
        let said = wrapped(
            &painter,
            &chat.text,
            FontId::proportional(size),
            hsla(45.0, 100.0, 95.0, 0.9),
            width - left,
        );
        (Some((who, me)), said, cream())
    };
    let height = body_text.size().y.max(18.0) + 12.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    let y = rect.top() + 6.0;
    let baseline = |g: &egui::Galley| y + body_text.rows.first().map_or(0.0, |r| r.max_y()) - g.size().y;
    if who.as_ref().is_some_and(|(_, me)| *me) {
        horizontal_gradient(
            &painter,
            rect,
            &[(0.0, hsla(46.0, 100.0, 60.0, 0.1)), (0.7, Color32::TRANSPARENT)],
        );
    }
    let tx = rect.left() + 4.0;
    let time_y = baseline(&time);
    painter.galley(pos2(tx, time_y), time, cream());
    let mut x = rect.left() + 4.0 + text_x;
    if let Some((who, _)) = who {
        let who_y = baseline(&who);
        let w = who.size().x;
        painter.galley(pos2(x, who_y), who, cream());
        x += w + 12.0;
    }
    if chat.system || chat.from.is_empty() {
        let d = sys * 0.3;
        let c = pos2(
            x + d + 1.0,
            y + body_text.rows.first().map_or(sys, |r| r.max_y()) * 0.55,
        );
        painter.add(Shape::convex_polygon(
            vec![c - vec2(0.0, d), c + vec2(d, 0.0), c + vec2(0.0, d), c - vec2(d, 0.0)],
            hsl(276.0, 90.0, 72.0),
            Stroke::NONE,
        ));
    }
    painter.galley(pos2(x, y), body_text, color);
}

/// A page the native lobby does not have yet: says so, and offers the web
/// lobby, which has it. True when the player asks for the web lobby.
fn not_yet_page(ui: &mut egui::Ui, content: Rect, tab: Tab, accent: &Accent) -> bool {
    let painter = ui.painter().clone();
    let kicker = line(&painter, &tab.title().to_uppercase(), 12.0, body(), accent.face, 0.32);
    let kh = kicker.size().y;
    painter.galley(content.min, kicker, accent.face);
    let title = line(
        &painter,
        "Not here yet",
        clamp_css(36.0, content.width() * 0.04, 60.0),
        display_family(),
        cream(),
        0.0,
    );
    let ty = content.top() + kh + 8.0;
    painter.galley_with_override_text_color(
        pos2(content.left() + 5.0, ty + 6.0),
        title.clone(),
        hsl(250.0, 50.0, 3.0),
    );
    let th = title.size().y;
    painter.galley(pos2(content.left(), ty), title, cream());
    let why = wrapped(
        &painter,
        &format!(
            "{} comes to the native lobby in a later step. The web lobby has it now; \
             the game switches back to it until you start with --lobby native again.",
            tab.title()
        ),
        FontId::proportional(22.0),
        hsla(45.0, 100.0, 95.0, 0.85),
        content.width().min(620.0),
    );
    let wy = ty + th + 22.0;
    let wh = why.size().y;
    painter.galley(pos2(content.left(), wy), why, cream());
    let label = "Open the web lobby";
    let lw = line(&painter, &label.to_uppercase(), 16.0, display_family(), cream(), 0.1)
        .size()
        .x;
    let key = Rect::from_min_size(pos2(content.left(), wy + wh + 28.0), vec2(lw + 62.0, 60.0));
    let face = RaisedKey::new("web-lobby", key, KeyLook::cream()).show(ui);
    KeyText::new(label, "", display_family(), body())
        .size(16.0, 13.0)
        .spacing(0.1)
        .pad(32.0, 30.0)
        .paint(&painter, &face);
    face.clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_take_the_browser_s_colours() {
        // `nameColor("Reaper")`: the hash, mod 10, picks the hue.
        let mut hash: u32 = 0;
        for c in "Reaper".chars() {
            hash = hash.wrapping_mul(31).wrapping_add(c as u32);
        }
        let hues = [182.0, 322.0, 125.0, 276.0, 28.0, 200.0, 350.0, 104.0, 240.0, 160.0];
        assert_eq!(name_color("Reaper"), hsl(hues[hash as usize % 10], 92.0, 68.0));
        // Past the BMP, the first UTF-16 unit counts, as `charCodeAt(0)`.
        assert_eq!(name_color("😀"), hsl(hues[0xD83D % 10], 92.0, 68.0));
    }

    #[test]
    fn retry_waits_match_the_browsers() {
        assert_eq!(offline_retry_delay_sec(1, 0.5), 2);
        assert_eq!(offline_retry_delay_sec(4, 0.5), 30);
        assert_eq!(offline_retry_delay_sec(9, 0.0), 24);
        assert_eq!(retry_delay_sec(Some(30), 0.5), 30);
        assert_eq!(retry_delay_sec(None, 1.0), 70);
        assert_eq!(retry_delay_sec(Some(1), 0.0), 5);
    }

    #[test]
    fn a_key_beside_keeps_the_whole_name() {
        assert_eq!(
            beside(Path::new("/c/lobby-key"), "unknown"),
            PathBuf::from("/c/lobby-key.unknown")
        );
        assert_eq!(
            beside(Path::new("/c/key.txt"), "unknown"),
            PathBuf::from("/c/key.txt.unknown")
        );
    }

    #[test]
    fn the_clock_reads_hours_and_minutes() {
        let shown = clock_time(1_700_000_000_000.0);
        assert_eq!(shown.len(), 5);
        assert_eq!(&shown[2..3], ":");
    }

    fn offline_panel() -> LobbyPanel {
        // Nothing listens there: the connection fails, as offline.
        LobbyPanel::connect("ws://127.0.0.1:9", None)
    }

    #[test]
    fn a_room_from_the_lobby_opens_on_the_play_page_in_its_colour() {
        let mut panel = offline_panel();
        panel.tab = Tab::Demos;
        let room = serde_json::json!({
            "id": "r1", "gametype": "snd", "visibility": "public",
            "config": {"maxPlayers": 8}, "players": [], "maps": []
        });
        panel.handle_lobby_created(&room);
        assert!(panel.in_room());
        assert_eq!(panel.tab, Tab::Play);
        assert_eq!(panel.mode().id, "snd");
        // The same room again changes nothing.
        panel.handle_lobby_created(&room);
        panel.on_other(
            "lobby:kicked",
            &serde_json::json!({"lobbyId": "r1", "reason": "Kicked"}),
        );
        assert!(!panel.in_room());
        assert_eq!(panel.chat.back().map(|c| c.text.as_str()), Some("Left room: Kicked"));
    }

    #[test]
    fn a_private_room_without_its_invite_is_left() {
        let mut panel = offline_panel();
        panel.pending_private_create = true;
        panel.queue_pending = true;
        panel.handle_lobby_created(&serde_json::json!({
            "id": "r2", "gametype": "ffa", "visibility": "private", "shareUrl": "/lobby/r2",
            "config": {"maxPlayers": 8}, "players": [], "maps": []
        }));
        assert!(!panel.in_room());
        assert!(!panel.queue_pending);
        assert_eq!(panel.notice, "The server did not provide a private invite room.");
        panel.pending_private_create = true;
        panel.handle_lobby_created(&serde_json::json!({
            "id": "r3", "gametype": "ffa", "visibility": "private", "shareUrl": "/lobby/r3?invite=abc&x=1",
            "config": {"maxPlayers": 8}, "players": [], "maps": []
        }));
        assert!(panel.in_room());
        assert_eq!(panel.room_invite.as_deref(), Some("abc"));
    }

    #[test]
    fn the_handoff_names_the_player_and_their_side() {
        let mut panel = offline_panel();
        panel.name = "Reaper".into();
        panel.local_lobby_player_id = Some("p1".into());
        panel.handle_lobby_created(&serde_json::json!({
            "id": "r4", "gametype": "snd", "visibility": "public", "config": {"maxPlayers": 8, "hasTeams": true},
            "players": [{"id": "p1", "name": "Reaper", "team": "alpha", "ready": true}], "maps": []
        }));
        panel.on_message(LobbyIn::Connect(LobbyConnect {
            match_id: Some("m".into()),
            server_url: "wss://game".into(),
            map: "dm2".into(),
            gametype: "snd".into(),
            admission_ticket: Some("t".into()),
        }));
        let choice = panel.take_choice().expect("a choice");
        assert_eq!(
            (choice.name.as_str(), choice.play, choice.team.as_deref()),
            ("Reaper", true, Some("alpha"))
        );
        // The room stays for the rematch.
        assert!(panel.in_room());
    }

    #[test]
    fn a_match_that_starts_is_announced_once() {
        let mut panel = offline_panel();
        let mut m = LiveMatchInfo {
            id: "r5".into(),
            gametype: "duel".into(),
            gametype_name: "Duel".into(),
            map: "q3dm17".into(),
            map_display_name: "The Longest Yard".into(),
            players: vec!["Reaper".into(), "Eclipse".into()],
            player_count: 2,
            max_players: 2,
            state: "waiting".into(),
            started_at: 0.0,
            server_url: None,
        };
        panel.on_message(LobbyIn::MatchesUpdate(vec![m.clone()]));
        assert!(panel.chat.is_empty());
        m.state = "playing".into();
        panel.on_message(LobbyIn::MatchesUpdate(vec![m.clone()]));
        panel.on_message(LobbyIn::MatchesUpdate(vec![m]));
        let lines: Vec<_> = panel.chat.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(lines, ["Duel on The Longest Yard started · Reaper vs Eclipse"]);
    }

    #[test]
    fn the_lobby_draws_without_a_connection() {
        let mut panel = offline_panel();
        let ctx = egui::Context::default();
        crate::client::washoi_style::install_style(&ctx);
        for size in [(1280.0, 720.0), (1920.0, 1080.0), (1024.0, 640.0)] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(size.0, size.1))),
                ..Default::default()
            };
            let mut action = Some(PanelAction::WebLobby);
            let mut output = ctx.run_ui(input, |ui| {
                action = panel.ui(ui, &TwinStyle::default());
            });
            output.textures_delta.clear();
            assert_eq!(action, None);
            assert!(!output.shapes.is_empty());
        }
    }
}
