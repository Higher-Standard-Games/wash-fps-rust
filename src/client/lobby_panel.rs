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
//! The Profile key widens the rail into the account's gold panel
//! (`client/profile_view.rs`) over everything, as the browser does.
//!
//! Not yet: deep links and the drop-in from a spectated match.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use shagger::core::cvar::with_cvars;
use shagger::lobby::lobby_client::{CLIENT_DISCONNECT_REASON, LobbyClient, LobbyEvent};
use shagger::lobby::player_identity::normalize_key;
use shagger::lobby::settings_sync::SettingsSync;
use shagger::lobby::types::{AuthError, CampaignMissionInfo, GametypeInfo, LiveMatchInfo, LobbyConnect, LobbyIn};
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

use crate::client::campaign::campaign_cutscene::{CampaignCutscene, CutsceneEvent};
use crate::client::campaign::campaign_music::CAMPAIGN_MENU_PLAYLIST;
use crate::client::campaign::campaign_session::{
    cache_campaign, cache_mission_looks, cached_campaign, remember_campaign_match, take_campaign_retry,
    take_campaign_return,
};
use crate::client::campaign::campaign_story::campaign_story;
use crate::client::campaign::campaign_view::{CampaignAction, CampaignView};
use crate::client::credits::credits_dialog::{CreditsAnswer, CreditsDialog};
use crate::client::demos_view::DemosView;
use crate::client::leaderboard_view::LeaderboardView;
use crate::client::levelshots::{Levelshots, site_base};
use crate::client::lobby_full_notice::{NoticeKind, lobby_full_notice, offline_retry_delay_sec, retry_delay_sec};
use crate::client::lobby_music_key::{lobby_music_key, lobby_music_muted, set_lobby_music_muted};
use crate::client::lobby_rooms::{PlayKeysInput, RoomAction, describe_room, play_keys, sort_rooms};
use crate::client::lobby_welcome_dialog::{back_key, back_key_size, dark_key};
use crate::client::lobby_welcome_guide::{GuideEvent, LobbyWelcomeGuide};
use crate::client::profile_view::{Account, ProfileAction, ProfileView, Screen as ProfileScreen};
use crate::client::room_chat_view::RoomChatView;
use crate::client::settings::fps_settings_view::FpsSettingsView;
use crate::client::settings::settings_model::SETTINGS_PAGES;
use crate::client::settings::settings_model::SettingsPageId;
use crate::client::washoi_style::{DISPLAY_FONT, display_family};
use crate::client::welcome_overlay::WelcomeOverlay;

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
/// The cap the chat's online pips scale to when the lobby names none.
const FALLBACK_LOBBY_CAPACITY: usize = 64;

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
    /// The links row's Quit.
    Quit,
    /// A page for the system's browser, such as the devlog.
    OpenUrl(String),
}

/// The team's devlog, opened from the Play page's links bar (`DEVLOG_URL`).
pub const DEVLOG_URL: &str = "https://washoi.net/devlog";

/// The rail's pages (`switchTab`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Play,
    Campaign,
    Leaderboard,
    Demos,
    Settings,
}

/// The keys under the player card: the Profile over the lobby, the
/// Settings page, and the console dropped over everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UtilityKey {
    Profile,
    Settings,
    Console,
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
        "banned" => "This player is banned from Washoi's servers.".into(),
        other => format!("Login refused: {other}"),
    }
}

/// `banNoticeText`: the refusal, the reason, how long and how to appeal.
fn ban_notice_text(e: &AuthError, now_ms: f64) -> String {
    let mut parts = vec![login_failure_text("banned")];
    if let Some(reason) = e.reason.as_deref().filter(|r| !r.is_empty()) {
        parts.push(format!("Reason: {reason}"));
    }
    parts.push(match e.until {
        Some(until) => {
            let hours = ((until - now_ms) / 3_600_000.0).ceil().max(1.0);
            if hours > 48.0 {
                format!("Ends in {} days.", (hours / 24.0).ceil())
            } else {
                format!("Ends in {hours} hours.")
            }
        }
        None => "Until it is lifted.".into(),
    });
    if let Some(appeal) = e.appeal.as_deref().filter(|a| !a.is_empty()) {
        parts.push(format!("To appeal: {appeal}"));
    }
    parts.join(" ")
}

/// `runDeepLinkJoin`'s reading of a link: `?lobby=` first, else
/// `/lobby/<id>` in the path, else the bare id; `?invite=` beside it.
fn parse_lobby_link(link: &str) -> Option<(String, Option<String>)> {
    let link = link.trim();
    let valid = |id: &str| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let (path, query) = link.split_once('?').unwrap_or((link, ""));
    let query = query.split('#').next().unwrap_or("");
    let param = |name: &str| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_owned())
            .filter(|v| !v.is_empty())
    };
    let from_path = path
        .split_once("/lobby/")
        .map(|(_, rest)| rest.split(['/', '#']).next().unwrap_or("").to_owned());
    let id = param("lobby")
        .or(from_path)
        .or_else(|| (!link.contains('/') && !link.contains('?')).then(|| link.to_owned()))?;
    valid(&id).then(|| (id, param("invite")))
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
/// It is the account's only credential, so on Unix only its owner may read it.
fn save_key(file: &Path, key: &str) -> std::io::Result<()> {
    shagger::core::atomic_file::write_atomic_private(file, key.as_bytes())
}

/// The account as [`ProfileView`] reads it, borrowing the panel's fields
/// one by one so the view and the client can be borrowed beside it.
macro_rules! account {
    ($panel:expr) => {
        Account {
            name: &$panel.name,
            key: $panel.key.as_deref(),
            key_unsaved: $panel.key_unsaved,
            id: $panel.account_id.as_deref(),
            connected: $panel.connected,
            in_room: $panel.room.is_some(),
            modes: &$panel.server_gametypes,
        }
    };
}

/// `LobbyPanel`.
pub struct LobbyPanel {
    pub client: LobbyClient,
    url: String,
    key: Option<String>,
    /// Where the account's key lives.
    key_file: Option<PathBuf>,
    /// The key file exists but could not be read: a new account's key must
    /// not replace it.
    key_unreadable: bool,
    /// The key in use is not on disk (`playerKeyIsSessionOnly`).
    key_unsaved: bool,
    /// A key to log in with on the connection being opened, from the
    /// Profile's "Use another key" (`switchFpsAccount`).
    switch_key: Option<String>,
    /// The switch is the Welcome's "Use my key", not the Profile's.
    switch_from_welcome: bool,
    /// The account was deleted: the next login makes a new one.
    starting_over: bool,
    /// Failed logins in a row, for the offline backoff (`loginFailures`).
    login_failures: u32,
    /// When to connect again, after a drop or a full lobby.
    retry_at: Option<Instant>,
    /// The lobby is full and closes the connection; its wait stands.
    lobby_full: bool,
    /// The lobby refused this player as banned: no retries until a new key logs in.
    banned: bool,
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
    /// The most players the lobby takes, which the chat's online pips are
    /// scaled to (`lobbyCapacity`).
    lobby_capacity: usize,
    chat: VecDeque<ChatLine>,
    chat_draft: String,
    pending_private_create: bool,
    /// The private room being created is quick play's.
    pending_quick_play: bool,
    /// The room Quick play opened, while the player is in it: Quick play
    /// leads back to it (`quickPlayLobbyId`).
    quick_play_lobby_id: Option<String>,
    /// A quick play room being rejoined after a reconnect: it is Quick
    /// play's again once back (`rejoiningQuickPlay`).
    rejoining_quick_play: Option<String>,
    queue_pending: bool,
    room: Option<MatchLobbyView>,
    /// The room's own chat, in the rail while the room is open.
    room_chat: Option<RoomChatView>,
    /// The settings screen, made the first time it is opened.
    settings: Option<FpsSettingsView>,
    /// The Leaderboard page.
    pub leaderboard: LeaderboardView,
    /// The Demos page.
    pub demos: DemosView,
    /// The Profile, over everything while open.
    pub profile: ProfileView,
    /// A new player's name or key, over everything while open.
    pub welcome: WelcomeOverlay,
    /// The welcome guide and its `?` key.
    pub guide: LobbyWelcomeGuide,
    /// The credits, while open.
    pub credits: Option<CreditsDialog>,
    /// A page for the system's browser, asked for from a dialog.
    open_url: Option<String>,
    /// The Console key was pressed: the window toggles its console.
    pub console_toggle: bool,
    /// Whether the window's console is down (the key shows pressed).
    pub console_open: bool,
    /// The lobby music key's state (`lobbyMusicMuted`), which the screen
    /// applies to the menu music.
    pub music_muted: bool,
    /// `LobbyFullNotice`: shown while the lobby turns this player away or
    /// is out of reach, counting down to `retry_at`.
    full_notice: Option<NoticeKind>,
    /// `runDeepLinkJoin`'s room and invite, from `--join`: joined once,
    /// after login and the welcome guide.
    deep_link: Option<(String, Option<String>)>,
    /// `--page update`: the notice as if a newer release were out.
    shown_update: Option<crate::update_check::Update>,
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
    /// The Campaign page.
    pub campaign: CampaignView,
    /// A mission to play again at once (`campaignRetry`): started once the
    /// lobby has sent the campaign and is on screen.
    campaign_retry: Option<String>,
    /// The campaign's missions and how far it is unlocked, as the lobby
    /// last sent them, for a retry waiting to start.
    campaign_missions: Option<(Vec<CampaignMissionInfo>, u32)>,
    /// The lobby runs the campaign (welcome's `campaign` feature).
    campaign_supported: bool,
    /// The comic card over everything while a mission starts.
    pub cutscene: Option<CampaignCutscene>,
    /// The mission being started, until its match connects or fails.
    starting_mission: Option<CampaignMissionInfo>,
    /// The page drawn last frame, for the page's music.
    campaign_shown: bool,
}

impl LobbyPanel {
    /// Connects and logs in with the stored key, or as a new account.
    #[must_use]
    pub fn connect(url: &str, key_file: Option<PathBuf>) -> Self {
        // A key saved by an older build may be readable by every user.
        if let Some(file) = &key_file {
            shagger::core::atomic_file::restrict_to_owner(file);
        }
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
            key_unsaved: key_unreadable,
            switch_key: None,
            switch_from_welcome: false,
            starting_over: false,
            login_failures: 0,
            retry_at: None,
            lobby_full: false,
            banned: false,
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
            lobby_capacity: FALLBACK_LOBBY_CAPACITY,
            chat: VecDeque::new(),
            chat_draft: String::new(),
            pending_private_create: false,
            pending_quick_play: false,
            quick_play_lobby_id: None,
            rejoining_quick_play: None,
            queue_pending: false,
            room: None,
            room_chat: None,
            settings: None,
            leaderboard: LeaderboardView::new(),
            demos: DemosView::new(url),
            profile: ProfileView::new(),
            welcome: WelcomeOverlay::new(),
            guide: LobbyWelcomeGuide::default(),
            credits: None,
            open_url: None,
            console_toggle: false,
            console_open: false,
            music_muted: lobby_music_muted(),
            full_notice: None,
            deep_link: None,
            shown_update: None,
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
            campaign: CampaignView::default(),
            campaign_retry: None,
            campaign_missions: None,
            campaign_supported: false,
            cutscene: None,
            starting_mission: None,
            campaign_shown: false,
        }
    }

    /// Where to go, once the lobby says so. The connection stays open.
    pub fn take_choice(&mut self) -> Option<LobbyChoice> {
        self.choice.take()
    }

    /// Back from a match: why it broke off, if it did. A match that reached
    /// its post-game comes back to Play, out of its room, as the browser's
    /// `returnToLobby` does; one left before then comes back to the room.
    pub fn after_match(&mut self, error: Option<String>, completed: bool) {
        if let Some(why) = error {
            self.notice = why;
        }
        // Back in the lobby: what a campaign match forced (its models, its
        // crew in green, its song) goes. The TypeScript reloads the page.
        crate::client::campaign::campaign_session::apply_campaign_looks(None);
        if completed {
            if let Some(room) = &self.room {
                self.client.leave_lobby(&room.lobby_id);
            }
            self.show_play_stage();
            self.room_to_rejoin = None;
        }
        // Back from a campaign match: the campaign is where the player was,
        // and a lost mission asked to be played again starts at once.
        if take_campaign_return().is_some() {
            self.tab = Tab::Campaign;
            self.campaign.show_current();
            // A campaign match is its own room: done, it is left.
            if let Some(room) = &self.room {
                self.client.leave_lobby(&room.lobby_id);
            }
            self.show_play_stage();
            self.room_to_rejoin = None;
        }
        self.campaign_retry = take_campaign_retry();
        if self.connected && self.campaign_supported {
            self.client.request_campaign();
        }
        self.retry_campaign_mission();
    }

    /// The menus' playlist on every page but the Campaign's, which plays
    /// its own opener (`playPageMusic`).
    #[must_use]
    pub fn page_playlist(&self) -> &'static str {
        if self.tab == Tab::Campaign {
            CAMPAIGN_MENU_PLAYLIST
        } else {
            crate::client::game_events::MENU_PLAYLIST
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
            // After a deletion, as the browser's reload: a new player.
            if std::mem::take(&mut self.starting_over) {
                self.profile.close();
            }
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
            room.tick(&self.client);
        }
        self.profile.tick();
        self.welcome.tick();
    }

    fn on_event(&mut self, event: LobbyEvent) {
        match event {
            LobbyEvent::Connected => {
                self.connected = true;
                let key = self.switch_key.as_deref().or(self.key.as_deref());
                self.client.auth(key);
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
            // `roomToRejoin.quickPlay`: once back, it is Quick play's again.
            self.rejoining_quick_play =
                (self.quick_play_lobby_id.as_deref() == Some(room.lobby_id.as_str())).then(|| room.lobby_id.clone());
            self.room_to_rejoin = Some((room.lobby_id, self.room_invite.take(), self.account_id.clone()));
        }
        self.quick_play_lobby_id = None;
        self.queue_pending = false;
        self.pending_private_create = false;
        self.pending_quick_play = false;
        self.private_lobby_supported = false;
        self.campaign_supported = false;
        self.fail_campaign_start("Lost the lobby server.");
        self.campaign.board_disconnected();
        self.server_gametypes.clear();
        self.live_matches.clear();
        self.online.clear();
        self.settings_sync = None;
        self.leaderboard.bind(false, false, None);
        self.demos.disconnected();
        if self.switch_key.take().is_some() {
            self.switch_answered(Err("The lobby didn't answer. Try again.".into()));
        }
        self.profile.bind(false, false, &account!(self), &mut self.client);
        self.welcome.disconnected();
        if !deliberate && !self.lobby_full && !self.banned && self.retry_at.is_none() {
            self.login_failures += 1;
            let seconds = offline_retry_delay_sec(self.login_failures, random01());
            self.full_notice = Some(NoticeKind::Offline);
            self.retry_at = Some(Instant::now() + Duration::from_secs(seconds));
        }
        self.lobby_full = false;
    }

    /// `performLobbyLogin`'s answer to a refusal.
    fn login_refused(&mut self, e: &AuthError) {
        // A key pasted on the Profile: say why, and log back in as before.
        if self.switch_key.take().is_some() {
            let why = if e.code == "banned" {
                ban_notice_text(e, now_ms())
            } else {
                login_failure_text(&e.code)
            };
            self.switch_answered(Err(why));
            if e.code != "lobby_full" {
                self.client.auth(self.key.as_deref());
                return;
            }
        }
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
        // A ban closes the connection; say why and stay offline.
        if e.code == "banned" {
            self.banned = true;
            self.full_notice = None;
            self.retry_at = None;
            self.notice = ban_notice_text(e, now_ms());
            return;
        }
        // A full lobby waits as long as it asks (and closes the connection);
        // any other refusal tries again after the outage backoff.
        let seconds = if e.code == "lobby_full" {
            self.lobby_full = true;
            self.full_notice = Some(NoticeKind::Full);
            retry_delay_sec(e.retry_after_sec, random01())
        } else {
            self.login_failures += 1;
            self.full_notice = Some(NoticeKind::Offline);
            self.notice = login_failure_text(&e.code);
            offline_retry_delay_sec(self.login_failures, random01())
        };
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
                self.key_unsaved = true;
            } else {
                self.key_unsaved = false;
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
                // `restoreAccount`: the pasted key is this computer's now.
                let switched = self.switch_key.take();
                if let Some(key) = &switched {
                    self.key_unreadable = false;
                    self.keep_new_key(key);
                }
                self.login_failures = 0;
                self.banned = false;
                self.retry_at = None;
                self.full_notice = None;
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
                self.campaign_supported = feature("campaign");
                self.campaign.set_account(self.account_id.clone());
                if let Some(id) = &self.account_id
                    && let Some(cached) = cached_campaign(id)
                {
                    self.campaign.set_state(Some(cached));
                }
                if self.campaign_supported {
                    self.client.request_campaign();
                }
                self.profile.bind(
                    feature("playerStats"),
                    feature("leaderboard") && feature("skillRatings"),
                    &account!(self),
                    &mut self.client,
                );
                if switched.is_some() {
                    self.switch_answered(Ok(self.name.clone()));
                } else if w.key.is_some() {
                    // `accountIsNew`: ask the new player who they are.
                    self.welcome.open(&account!(self));
                }
                // `welcomeGuide.setAuthenticated(true)`.
                self.guide.set_authenticated(true);
                self.demos.connected(
                    feature("demoOwnership"),
                    feature("featuredDemos"),
                    self.tab == Tab::Demos,
                    &mut self.client,
                );
                self.queue_pending = false;
                self.notice.clear();
                self.online = w.online_players.iter().map(|p| p.id.clone()).collect();
                self.lobby_capacity = w
                    .max_players
                    .filter(|&m| m > 0)
                    .map_or(FALLBACK_LOBBY_CAPACITY, |m| m as usize);
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
                match self.room_to_rejoin.take() {
                    Some((lobby_id, invite, account)) if account == self.account_id => {
                        self.client.join_lobby(&lobby_id, invite.as_deref());
                    }
                    _ => self.rejoining_quick_play = None,
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
            LobbyIn::CampaignState(state) => {
                if let Some(id) = &self.account_id {
                    cache_campaign(id, &state);
                }
                // A campaign demo is drawn as its mission was, from these.
                cache_mission_looks(&state.missions);
                self.campaign_missions = Some((state.missions.clone(), state.unlocked_through));
                self.campaign.set_state(Some(*state));
                self.retry_campaign_mission();
            }
            LobbyIn::Connect(LobbyConnect {
                server_url,
                map,
                admission_ticket,
                campaign,
                ..
            }) => {
                // A campaign match's mission and models go with it to the game.
                let is_campaign = campaign.is_some();
                remember_campaign_match(campaign.map(|c| *c));
                if is_campaign && let Some(cut) = &mut self.cutscene {
                    cut.close();
                    self.starting_mission = None;
                }
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
                self.fail_campaign_start(&e);
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
                self.profile.on_message(kind, message, &account!(self));
                self.welcome.on_message(kind, message);
            }
            "identity:error" => {
                self.profile.on_message(kind, message, &account!(self));
                self.welcome.on_message(kind, message);
            }
            // `forgetDeletedAccount`: drop the key here, so nothing logs back
            // in, and start over as a new player once the lobby has closed.
            "identity:deleted" => {
                self.profile.on_message(kind, message, &account!(self));
                self.key = None;
                if let Some(file) = &self.key_file {
                    for f in [file.clone(), beside(file, "unknown")] {
                        if let Err(e) = std::fs::remove_file(&f)
                            && e.kind() != std::io::ErrorKind::NotFound
                        {
                            println!("lobby: could not remove {}: {e}", f.display());
                        }
                    }
                }
                self.key_unreadable = false;
                self.starting_over = true;
                self.retry_at = Some(Instant::now() + Duration::from_secs(2));
            }
            "stats:summary" | "stats:error" => self.profile.on_message(kind, message, &account!(self)),
            "lobby:kicked" => {
                if self.room.as_ref().map(|r| r.lobby_id.clone()) == field("lobbyId") {
                    let reason = field("reason").unwrap_or_default();
                    self.fail_campaign_start(&format!("Left the mission: {reason}"));
                    self.show_play_stage();
                    self.system_line(format!("Left room: {reason}"));
                }
            }
            // The lobby is the rematch screen: the room stays.
            "match:ended" => {
                self.demos.match_ended();
                self.system_line("Match ended — ready up to rematch");
            }
            "queue:left" => self.queue_pending = false,
            "leaderboard:page" | "leaderboard:error" => {
                self.leaderboard.on_message(kind, message);
                self.profile.on_message(kind, message, &account!(self));
            }
            "demos:list_response" | "featured:page" | "featured:error" => self.demos.on_message(kind, message),
            "campaign:board" | "campaign:board:error" => self.campaign.on_board_message(kind, message),
            // `recoverSeat`'s answer.
            "match:seat_reissued" => {
                let Some((request, match_id)) = self.seat_request.take() else {
                    return;
                };
                if field("requestId").as_deref() != Some(request.as_str()) {
                    self.seat_request = Some((request, match_id));
                    return;
                }
                // Back from the match, Rejoin reads as itself again.
                if let Some(room) = &mut self.room {
                    room.seat_reissued();
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
        let campaign_room = lobby.get("campaign").is_some_and(|c| !c.is_null());
        let quick_play = self.pending_quick_play && private && !campaign_room;
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
        let rejoined_quick_play = self.rejoining_quick_play.take().is_some_and(|r| r == id);
        self.quick_play_lobby_id = (quick_play || rejoined_quick_play).then(|| id.to_owned());
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
        // A campaign room stays on the Campaign page (its music too) behind
        // the comic card; `handleLobbyCreated` changes no tab.
        if !campaign_room {
            self.tab = Tab::Play;
        }
        // A campaign room opens behind the comic card.
        if self.cutscene.is_some() && campaign_room {
            self.ready_for_campaign();
        }
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
        self.quick_play_lobby_id = None;
        self.room_chat = None;
        self.seat_request = None;
        self.room_invite = None;
    }

    fn mode_ready(&self, id: &str) -> bool {
        self.server_gametypes.iter().any(|g| g.id == id)
    }

    /// The open room is the one Quick play opened.
    fn in_quick_play(&self) -> bool {
        self.room
            .as_ref()
            .is_some_and(|r| self.quick_play_lobby_id.as_deref() == Some(r.lobby_id.as_str()))
    }

    /// `refreshQuickPlayKey`: the lobby, its private rooms and FFA, and no
    /// other room open. In the room it opened, it leads back there.
    #[must_use]
    pub fn quick_play_enabled(&self) -> bool {
        self.connected
            && (self.in_quick_play()
                || (self.private_lobby_supported
                    && !self.queue_pending
                    && self.room.is_none()
                    && self.mode_ready(QUICK_PLAY_MODE)))
    }

    /// `startQuickPlay`: a private FFA that adds its bots, votes a map and
    /// readies up by itself.
    pub fn start_quick_play(&mut self) {
        // Already in the quick play room: back to it, bots and seat as they were.
        if self.in_quick_play() {
            self.tab = Tab::Play;
            return;
        }
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

    fn mode(&self) -> &'static GameMode {
        &GAME_MODES[self.selected_mode.min(GAME_MODES.len() - 1)]
    }

    /// The colours the page takes: the Leaderboard's and Demos' own pick.
    fn page_accent(&self) -> Accent {
        match self.tab {
            Tab::Leaderboard => self.leaderboard.mode().accent(),
            Tab::Demos => self.demos.accent(),
            _ => self.mode().accent(),
        }
    }

    /// Draws the lobby over the whole of `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui, style: &TwinStyle) -> Option<PanelAction> {
        let screen = ui.max_rect();
        let time = self.started.elapsed().as_secs_f64();
        let painter = ui.painter().clone();
        let accent = self.page_accent();
        paint_backdrop(&painter, screen);
        paint_light_in(&painter, screen, time, accent.face);
        self.levelshots.poll(ui.ctx());
        let mut action = None;
        // The Profile over everything: the lobby shows only while it moves.
        if self.profile.covers() || self.welcome.covers() {
            self.overlays_ui(ui, screen, time, accent.face, style);
            ui.ctx().request_repaint();
            return None;
        }
        let layout = Layout::new(screen);
        if self.shown_tab != Some(self.tab) {
            if self.shown_tab == Some(Tab::Demos) {
                self.demos.deactivate();
            }
            self.shown_tab = Some(self.tab);
            match self.tab {
                Tab::Leaderboard => self.leaderboard.refresh(&mut self.client),
                Tab::Demos => self.demos.show(&mut self.client),
                _ => {}
            }
        }
        self.refresh_campaign();
        if (self.tab == Tab::Campaign) != self.campaign_shown {
            self.campaign_shown = self.tab == Tab::Campaign;
            if self.campaign_shown {
                if self.campaign_supported && self.connected {
                    self.client.request_campaign();
                }
                self.campaign.show_current();
            }
        }
        self.stage(ui, &layout, style, &mut action);
        self.rail(ui, &layout, style);
        // Above the links bar (its keys, 44 tall on a 5 px base, 17 px up).
        if let Some(kind) = self.full_notice
            && lobby_full_notice(ui, screen, screen.max.y - 17.0 - 49.0, kind, self.retry_at)
        {
            self.retry_at = Some(Instant::now());
        }
        self.overlays_ui(ui, screen, time, accent.face, style);
        self.dialogs_ui(ui, screen);
        self.run_deep_link_join();
        self.update_notice(ui, screen);
        if let Some(url) = self.open_url.take() {
            action = Some(PanelAction::OpenUrl(url));
        }
        self.cutscene_ui(ui.ctx());
        // Keep the light drifting.
        ui.ctx().request_repaint();
        action
    }
}

impl LobbyPanel {
    /// Whether the welcome guide may be over the lobby (`canOpen`).
    /// A shared room link to join once the lobby lets this player in:
    /// `https://…/lobby/<id>`, `…?lobby=<id>&invite=<token>` or the bare id.
    pub fn set_deep_link(&mut self, link: &str) {
        self.deep_link = parse_lobby_link(link);
        if self.deep_link.is_none() {
            println!("lobby: no room in the link {link}");
        }
    }

    /// `runDeepLinkJoin`: once, when logged in and the guide is done.
    fn run_deep_link_join(&mut self) {
        if self.deep_link.is_none()
            || !self.connected
            || self.account_id.is_none()
            || self.welcome.is_open()
            || !self.guide.ready_for_join()
        {
            return;
        }
        if let Some((lobby, invite)) = self.deep_link.take() {
            println!("lobby: deep link, joining room {lobby}");
            self.client.join_lobby(&lobby, invite.as_deref());
        }
    }

    fn guide_can_open(&self) -> bool {
        !self.welcome.is_open() && !self.profile.is_open() && self.full_notice != Some(NoticeKind::Full)
    }

    /// wash-fps `native/ShellControls.ts`'s `installUpdateNotice`: a newer
    /// client is out. A notice in the upper right offers the download in
    /// the system's browser, or Later, which holds for this run.
    fn update_notice(&mut self, ui: &mut egui::Ui, screen: Rect) {
        let Some(update) = self.shown_update.clone().or_else(crate::update_check::available) else {
            return;
        };
        let painter = ui.painter().clone();
        let say = match &update.url {
            Some(_) => format!("Washoi {} is out.", update.version),
            None => format!("Washoi {} is out; get it from washoi.net.", update.version),
        };
        let g = line(&painter, &say, 15.0, body(), hsl(43.0, 72.0, 89.0), 0.0);
        let key_g = |label: &str| line(&painter, label, 13.0, display_family(), hsl(43.0, 100.0, 94.0), 0.12);
        let keys: Vec<(&str, Arc<egui::Galley>)> = update
            .url
            .as_ref()
            .map(|_| ("DOWNLOAD", key_g("DOWNLOAD")))
            .into_iter()
            .chain(std::iter::once(("LATER", key_g("LATER"))))
            .collect();
        // Padding 10 14, items 10 apart; the keys stand on a 5px base.
        let keys_w: f32 = keys.iter().map(|(_, k)| k.size().x + 32.0 + 10.0).sum();
        let w = 14.0 + g.size().x + keys_w + 14.0;
        let h = 10.0 + 44.0 + 5.0 + 10.0;
        let r = Rect::from_min_size(pos2(screen.right() - 24.0 - w, screen.top() + 24.0), vec2(w, h));
        ui.interact(r, egui::Id::NULL.with("update-notice"), egui::Sense::hover());
        painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(16, 12, 32, 235));
        let mid = r.top() + 10.0 + 22.0;
        let gh = g.size().y;
        painter.galley(pos2(r.left() + 14.0, mid - gh / 2.0), g, cream());
        let mut x = r.left() + 14.0 + w - 28.0 - keys_w + 10.0;
        let mut done = false;
        for (id, label) in keys {
            let kw = label.size().x + 32.0;
            let kr = Rect::from_min_size(pos2(x, r.top() + 10.0), vec2(kw, 44.0));
            x += kw + 10.0;
            if link_key(ui, &painter, kr, label, id) {
                if id == "DOWNLOAD"
                    && let Some(url) = &update.url
                {
                    self.open_url = Some(url.clone());
                }
                done = true;
            }
        }
        if done {
            crate::update_check::dismiss();
            self.shown_update = None;
        }
    }

    /// The welcome guide and the credits, over everything.
    fn dialogs_ui(&mut self, ui: &mut egui::Ui, screen: Rect) {
        let can_open = self.guide_can_open();
        self.guide.refresh(can_open);
        if self.guide.is_open() {
            // The guide has the lobby's attention: the credits wait.
            self.credits = None;
            match self.guide.ui(ui, screen) {
                Some(GuideEvent::OpenControls) => {
                    self.tab = Tab::Settings;
                    self.settings_view().show_page(SettingsPageId::Controls);
                }
                Some(GuideEvent::Open(url)) => self.open_url = Some(url),
                None => {}
            }
            return;
        }
        if let Some(credits) = &mut self.credits {
            match credits.ui(ui, screen) {
                Some(CreditsAnswer::Close) => self.credits = None,
                Some(CreditsAnswer::Open(url)) => self.open_url = Some(url),
                None => {}
            }
        }
    }

    /// The Profile and the Welcome, whichever is open, over the lobby.
    fn overlays_ui(&mut self, ui: &mut egui::Ui, screen: Rect, time: f64, accent: Color32, style: &TwinStyle) {
        if self.profile.is_open() {
            let account = account!(self);
            let answer = self
                .profile
                .ui(ui, screen, time, accent, style, &account, &mut self.client);
            if let Some(ProfileAction::Switch(key)) = answer {
                self.switch_from_welcome = false;
                self.start_switch(&key);
            }
        }
        if self.welcome.is_open() {
            let account = account!(self);
            let answer = self
                .welcome
                .ui(ui, screen, time, accent, style, &account, &mut self.client);
            if let Some(ProfileAction::Switch(key)) = answer {
                self.switch_from_welcome = true;
                self.start_switch(&key);
            }
        }
    }
}

mod campaign_tab;
mod panel_ui;

use panel_ui::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::identity_fields::Tone;

    #[test]
    fn a_ban_notice_gives_the_reason_the_end_and_the_appeal() {
        let mut e: AuthError = serde_json::from_value(serde_json::json!({
            "type": "auth:error", "code": "banned", "reason": "Aimbot", "until": 3.0 * 86_400_000.0, "appeal": "write to us"
        }))
        .unwrap();
        assert_eq!(
            ban_notice_text(&e, 0.0),
            "This player is banned from Washoi's servers. Reason: Aimbot Ends in 3 days. To appeal: write to us"
        );
        e.until = None;
        e.reason = None;
        assert!(ban_notice_text(&e, 0.0).contains("Until it is lifted."));
    }

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
    fn room_links_read_as_the_browser_reads_them() {
        let room = |id: &str, invite: Option<&str>| Some((id.to_owned(), invite.map(str::to_owned)));
        assert_eq!(
            parse_lobby_link("https://eu.washoi.net/lobby/abc_1-2"),
            room("abc_1-2", None)
        );
        assert_eq!(
            parse_lobby_link("https://eu.washoi.net/client/?lobby=r9&invite=t0k"),
            room("r9", Some("t0k"))
        );
        assert_eq!(parse_lobby_link("https://x/lobby/a?invite=b"), room("a", Some("b")));
        assert_eq!(parse_lobby_link(" r42 "), room("r42", None));
        assert_eq!(parse_lobby_link("https://eu.washoi.net/"), None);
        assert_eq!(parse_lobby_link("https://x/lobby/a%20b"), None);
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
    fn a_finished_match_comes_back_to_play_and_one_left_early_to_its_room() {
        let mut panel = offline_panel();
        let room = serde_json::json!({
            "id": "r1", "gametype": "duel", "visibility": "private",
            "config": {"maxPlayers": 2}, "players": [], "maps": []
        });
        panel.handle_lobby_created(&room);
        panel.after_match(None, false);
        assert!(panel.in_room());
        panel.after_match(None, true);
        assert!(!panel.in_room());
        assert_eq!(panel.tab, Tab::Play);
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
            campaign: None,
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
            let mut action = Some(PanelAction::Quit);
            let mut output = ctx.run_ui(input, |ui| {
                action = panel.ui(ui, &TwinStyle::default());
            });
            output.textures_delta.clear();
            assert_eq!(action, None);
            assert!(!output.shapes.is_empty());
        }
    }

    /// Polls `panel` until `done` holds, for up to `secs`.
    fn wait_for(panel: &mut LobbyPanel, secs: u64, done: impl Fn(&LobbyPanel) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            panel.poll();
            if done(panel) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// The Profile's account flows against a real lobby: rename, switch to
    /// another player and back, delete. `WASHOI_TEST_LOBBY=ws://localhost:9000
    /// cargo test -- --ignored profile_flows` with a scratch lobby.
    #[test]
    #[ignore = "needs a lobby"]
    fn profile_flows_against_a_lobby() {
        let url = std::env::var("WASHOI_TEST_LOBBY").expect("WASHOI_TEST_LOBBY");
        let dir = std::env::temp_dir().join(format!("washoi-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("key");
        let mut panel = LobbyPanel::connect(&url, Some(file.clone()));
        assert!(wait_for(&mut panel, 10, |p| p.account_id.is_some()), "login");
        let first_key = panel.key.clone().unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), first_key);
        panel.open_profile(ProfileScreen::Name);

        // Rename.
        let new_name = format!("Prof{}", std::process::id() % 100_000);
        panel
            .profile
            .type_into(ProfileScreen::Name, &new_name, &account!(panel));
        panel.profile.rename(&account!(panel), &mut panel.client);
        assert!(wait_for(&mut panel, 10, |p| p.name == new_name), "renamed");
        assert_eq!(
            panel.profile.said(ProfileScreen::Name),
            (format!("You're {new_name} now").as_str(), Tone::Ok)
        );

        // A second player, to switch to.
        let mut other = LobbyPanel::connect(&url, Some(dir.join("other")));
        assert!(wait_for(&mut other, 10, |p| p.account_id.is_some()), "second login");
        let (other_key, other_id) = (other.key.clone().unwrap(), other.account_id.clone());
        other.leave();

        // A key nobody has is refused, and the player stays.
        let first_id = panel.account_id.clone();
        panel
            .profile
            .type_into(ProfileScreen::Switch, "0000-0000-0000-0000-0000-0000", &account!(panel));
        let go = panel.profile.switch(&account!(panel)).expect("switch");
        let ProfileAction::Switch(key) = go;
        panel.start_switch(&key);
        assert!(
            wait_for(&mut panel, 10, |p| p.profile.said(ProfileScreen::Switch).1 == Tone::Bad
                && p.account_id.is_some()),
            "refused"
        );
        assert_eq!(panel.account_id, first_id);
        assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), first_key);

        // The other key logs in and becomes this computer's.
        panel.profile.type_into(
            ProfileScreen::Switch,
            &format_key_for_test(&other_key),
            &account!(panel),
        );
        let ProfileAction::Switch(key) = panel.profile.switch(&account!(panel)).expect("switch");
        panel.start_switch(&key);
        assert!(wait_for(&mut panel, 10, |p| p.account_id == other_id), "switched");
        assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), other_key);
        assert_eq!(panel.profile.said(ProfileScreen::Switch).1, Tone::Ok);

        // Delete it: the key file goes, and a new player starts.
        panel
            .profile
            .open(ProfileScreen::Delete, &account!(panel), &mut panel.client);
        panel
            .profile
            .type_into(ProfileScreen::Delete, &other_key, &account!(panel));
        panel.profile.delete(&account!(panel), &mut panel.client);
        assert!(wait_for(&mut panel, 10, |p| p.starting_over), "deleted");
        assert_eq!(panel.profile.said(ProfileScreen::Delete).1, Tone::Ok);
        assert!(!file.exists());
        assert!(
            wait_for(&mut panel, 20, |p| p.account_id.is_some() && p.account_id != other_id),
            "a new player"
        );
        assert!(file.exists());
        panel.leave();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A new player's Welcome against a real lobby: the name, then on
    /// another computer "Use my key" with the first player's key.
    #[test]
    #[ignore = "needs a lobby"]
    fn welcome_flows_against_a_lobby() {
        let url = std::env::var("WASHOI_TEST_LOBBY").expect("WASHOI_TEST_LOBBY");
        let dir = std::env::temp_dir().join(format!("washoi-welcome-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut panel = LobbyPanel::connect(&url, Some(dir.join("key")));
        assert!(wait_for(&mut panel, 10, |p| p.account_id.is_some()), "login");
        assert!(panel.welcome.showing(), "a new player is welcomed");
        let name = format!("Wel{}", std::process::id() % 100_000);
        panel.welcome.type_into(false, &name, &account!(panel));
        panel.welcome.take_name(&mut panel.client);
        assert!(wait_for(&mut panel, 10, |p| p.name == name), "named");
        assert!(!panel.welcome.showing());
        let (key, id) = (panel.key.clone().unwrap(), panel.account_id.clone());
        panel.leave();

        let file = dir.join("second");
        let mut second = LobbyPanel::connect(&url, Some(file.clone()));
        assert!(wait_for(&mut second, 10, |p| p.account_id.is_some()), "second login");
        assert!(second.welcome.showing());
        second.welcome.type_into(true, &key, &account!(second));
        let ProfileAction::Switch(pasted) = second.welcome.use_key().expect("a whole key");
        second.switch_from_welcome = true;
        second.start_switch(&pasted);
        assert!(wait_for(&mut second, 10, |p| p.account_id == id), "restored");
        assert_eq!(second.name, name);
        assert_eq!(
            second.welcome.said(true),
            (format!("Welcome back, {name}").as_str(), Tone::Ok)
        );
        assert!(!second.welcome.showing() || wait_for(&mut second, 3, |p| !p.welcome.showing()));
        assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), key);
        second.leave();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn format_key_for_test(key: &str) -> String {
        shagger::lobby::player_identity::format_key(key)
    }
}
