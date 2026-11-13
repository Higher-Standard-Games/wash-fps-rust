//! wash-fps `client/DemosView.ts`, `ReelLengths.ts` and `styles/Demos.css`.
//!
//! Demos: this week's featured matches, the latest recordings, and yours,
//! with the maps in the rail. Everything shown is in the recording's header
//! (map, mode, who played, how long, when, how big). A header has no score,
//! team or rank, so a card has none either.
//!
//! A recording is downloaded here ([`DemoDownload`]) and handed to the
//! window as a [`DemoChoice`], as the web lobby's `demo:play` is. The
//! browser's video export is left out.

use std::time::{Duration, Instant};

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::shared::js::js_round;
use shagger::ui::components::lobby_twin::{clamp_css, cream, elide, gradient_polygon, hsl, hsla, ink, violet};
use shagger::ui::components::raised_key::{Accent, KeyFace, KeyLook, RaisedKey};
use shagger::ui::components::text_field::TextField;
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::client::demo_download::{DemoDownload, DemoDownloadState};
use crate::client::levelshots::Levelshots;
use crate::client::lobby_page_parts::{
    ModeChip, RailKey, board_keys, flat_key, mode_keys, page_head, rail_label, rule,
};
use crate::client::lobby_panel::{GAME_MODES, GameMode};
use crate::client::settings::settings_fields::{body, gold_text, line, wrapped};
use crate::client::washoi_style::display_family;

/// `DEMOS_PER_PAGE`: three across and two down.
pub const DEMOS_PER_PAGE: usize = 6;
/// `DEMO_LIST_LIMIT`: how many recordings the lobby sends at once.
pub const DEMO_LIST_LIMIT: u32 = 64;
/// `DEMO_ARCHIVE_TIMEOUT_MS`, and `requestFeaturedDemos`' wait.
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(10);
const FEATURED_LIMIT: u32 = 6;
/// `REEL_MINUTES`.
pub const REEL_MINUTES: [u32; 3] = [1, 2, 3];

/// `reelLengths`: highlight targets use at most a third of the recording.
#[must_use]
pub fn reel_lengths(duration_ms: f64) -> Vec<u32> {
    REEL_MINUTES
        .into_iter()
        .filter(|&m| duration_ms >= f64::from(m) * 3.0 * 60_000.0)
        .collect()
}

/// `pageKeys`: all the page numbers when few, else the ends and the
/// neighbours.
#[must_use]
pub fn page_keys(page: usize, pages: usize) -> Vec<usize> {
    if pages <= 7 {
        return (1..=pages).collect();
    }
    let mut keys: Vec<usize> = [1, 2, page.saturating_sub(1), page, page + 1, pages - 1, pages]
        .into_iter()
        .filter(|&n| n >= 1 && n <= pages)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// `ago`: short for a card and the pick list, longer for the featured match.
#[must_use]
pub fn ago(date: f64, short: bool, now: f64) -> String {
    let minutes = (now - date) / 60_000.0;
    if minutes < 60.0 {
        return format!("{}{} ago", js_round(minutes).max(1.0), if short { "m" } else { " min" });
    }
    if minutes < 24.0 * 60.0 {
        return format!("{}{} ago", js_round(minutes / 60.0), if short { "h" } else { " h" });
    }
    let days = (minutes / (24.0 * 60.0)).floor();
    if short && days < 7.0 {
        return format!("{days}d ago");
    }
    if days == 1.0 {
        return "Yesterday".into();
    }
    if days < 7.0 {
        return format!("{days} days ago");
    }
    jiff::Timestamp::from_millisecond(date as i64)
        .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
        .map_or_else(|_| String::new(), |z| z.strftime("%-d %b").to_string())
}

/// `clock`: a duration as `m:ss`.
fn clock(ms: f64) -> String {
    let seconds = (ms / 1000.0).floor() as i64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// `DemoInfo`: a recording's header, as the archive lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoInfo {
    pub filename: String,
    pub map: String,
    pub map_display_name: String,
    pub gametype: String,
    pub players: Vec<String>,
    pub duration_ms: f64,
    pub date: f64,
    pub size_mb: f64,
    pub levelshot: Option<String>,
}

impl DemoInfo {
    /// Reads one from the lobby's JSON.
    #[must_use]
    pub fn read(v: &Value) -> Option<Self> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let num = |k: &str| v.get(k).and_then(Value::as_f64);
        Some(Self {
            filename: text("filename")?,
            map: text("map")?,
            map_display_name: text("mapDisplayName").unwrap_or_default(),
            gametype: text("gametype").unwrap_or_default(),
            players: v
                .get("players")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default(),
            duration_ms: num("durationMs").unwrap_or(0.0),
            date: num("date").unwrap_or(0.0),
            size_mb: num("sizeMB").unwrap_or(0.0),
            levelshot: text("levelshot"),
        })
    }

    /// `mapName`.
    #[must_use]
    pub fn map_name(&self) -> &str {
        if self.map_display_name.is_empty() {
            &self.map
        } else {
            &self.map_display_name
        }
    }

    /// `demoMode`: deathmatch recordings are free for all.
    #[must_use]
    pub fn mode(&self) -> &str {
        if self.gametype == "deathmatch" {
            "ffa"
        } else {
            &self.gametype
        }
    }

    /// The map whose levelshot the card shows: the one the lobby names.
    fn shot_map(&self) -> String {
        self.levelshot
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .and_then(|f| f.strip_suffix(".jpg"))
            .filter(|m| !m.is_empty() && !m.contains('%'))
            .unwrap_or(&self.map)
            .to_owned()
    }

    fn size_text(&self) -> String {
        let mb = js_round(self.size_mb * 10.0) / 10.0;
        format!("{mb} MB")
    }
}

/// What the Demos page hands the window: a recording, downloaded.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoChoice {
    pub path: std::path::PathBuf,
    /// A highlight reel about this many minutes long instead.
    pub reel: Option<f64>,
}

/// A mode as this page offers it: All, then the four.
#[derive(Debug, Clone, Copy)]
struct PageMode {
    id: &'static str,
    title: &'static str,
    accent: Accent,
    mark: Color32,
}

fn page_modes() -> Vec<PageMode> {
    let all = PageMode {
        id: "all",
        title: "All",
        accent: Accent {
            face: hsl(276.0, 90.0, 64.0),
            hi: hsl(278.0, 95.0, 80.0),
            lo: hsl(268.0, 75.0, 54.0),
            side_hi: hsl(262.0, 70.0, 36.0),
            side_lo: hsl(256.0, 70.0, 22.0),
        },
        mark: hsl(276.0, 90.0, 72.0),
    };
    let mode = |m: &'static GameMode| PageMode {
        id: m.id,
        title: m.title,
        accent: m.accent(),
        mark: hsl(m.mark.0, m.mark.1, m.mark.2),
    };
    std::iter::once(all).chain(GAME_MODES.iter().map(mode)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Featured,
    Latest,
    Yours,
}

/// `TABS`.
const TABS: [(Tab, &str, &str); 3] = [
    (Tab::Featured, "Featured", "This week's best"),
    (Tab::Latest, "Latest", "Every recorded match"),
    (Tab::Yours, "Yours", "Matches you played"),
];

#[derive(Debug, Clone)]
enum Featured {
    Idle,
    Loading { request: String, sent: Instant },
    Ready(Vec<(DemoInfo, String)>),
    Failed(String),
}

/// A recording being fetched for playing (`dm-transfer`).
#[derive(Debug)]
struct Transfer {
    demo: DemoInfo,
    reel: Option<f64>,
    download: Option<DemoDownload>,
    /// Why it failed: the bar offers Try again.
    failed: Option<String>,
}

/// The glyphs the browser writes as text, drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Icon {
    Play,
    Bolt,
    Star,
}

fn paint_icon(painter: &Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let s = size / 2.0;
    let pts: Vec<Pos2> = match icon {
        Icon::Play => vec![
            center + vec2(-s * 0.7, -s * 0.8),
            center + vec2(s * 0.8, 0.0),
            center + vec2(-s * 0.7, s * 0.8),
        ],
        Icon::Bolt => {
            // Two triangles: the bolt's upper and lower strokes.
            let upper = vec![
                center + vec2(s * 0.35, -s),
                center + vec2(-s * 0.55, s * 0.15),
                center + vec2(s * 0.05, s * 0.15),
            ];
            let lower = vec![
                center + vec2(-s * 0.05, -s * 0.15),
                center + vec2(s * 0.55, -s * 0.15),
                center + vec2(-s * 0.35, s),
            ];
            painter.add(Shape::convex_polygon(upper, color, Stroke::NONE));
            painter.add(Shape::convex_polygon(lower, color, Stroke::NONE));
            return;
        }
        Icon::Star => {
            for k in 0..5 {
                let a = |i: f32| -std::f32::consts::FRAC_PI_2 + i * std::f32::consts::TAU / 5.0;
                let k = k as f32;
                let tip = center + vec2(a(k).cos(), a(k).sin()) * s;
                let l = center + vec2(a(k - 0.5).cos(), a(k - 0.5).sin()) * s * 0.42;
                let r = center + vec2(a(k + 0.5).cos(), a(k + 0.5).sin()) * s * 0.42;
                painter.add(Shape::convex_polygon(vec![l, tip, r, center], color, Stroke::NONE));
            }
            return;
        }
    };
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// A key's label with a glyph before it, centred on the face.
fn icon_label(painter: &Painter, face: &KeyFace, icon: Option<Icon>, label: &str, size: f32, em: f32) {
    let g = line(painter, &label.to_uppercase(), size, display_family(), face.text, em);
    let icon_w = if icon.is_some() { size * 0.8 + size * 0.5 } else { 0.0 };
    let w = icon_w + g.size().x;
    let x = face.rect.center().x - w / 2.0;
    let cy = face.rect.center().y;
    if let Some(icon) = icon {
        paint_icon(painter, icon, pos2(x + size * 0.4, cy), size * 0.8, face.text);
    }
    painter.galley(pos2(x + icon_w, cy - g.size().y / 2.0), g, face.text);
}

/// A slanted strip (`clip-path: polygon(a% 0, 100% 0, b% 100%, 0 100%)`).
fn slant(r: Rect, left_top: f32, right_bottom: f32) -> Vec<Pos2> {
    vec![
        pos2(r.left() + left_top, r.top()),
        r.right_top(),
        pos2(r.right() - right_bottom, r.bottom()),
        r.left_bottom(),
    ]
}

/// A label on a slanted chip; returns its width.
fn chip(painter: &Painter, at: Pos2, s: &str, size: f32, fg: Color32, bg: Color32, pad: (f32, f32)) -> f32 {
    let g = line(painter, &s.to_uppercase(), size, display_family(), fg, 0.12);
    let r = Rect::from_min_size(at, vec2(g.size().x + 2.0 * pad.0, g.size().y + 2.0 * pad.1));
    let c = r.width() * 0.08;
    painter.add(Shape::convex_polygon(slant(r, c, c), bg, Stroke::NONE));
    painter.galley(r.center() - g.size() / 2.0, g, fg);
    r.width()
}

#[derive(Debug)]
pub struct DemosView {
    tab: Tab,
    mode: usize,
    map: Option<String>,
    query: String,
    page: usize,
    archive: Vec<DemoInfo>,
    archive_yours: bool,
    archive_request: Option<(String, Instant)>,
    archive_arrived: bool,
    archive_failed: bool,
    ownership: bool,
    connected: bool,
    featured_supported: bool,
    featured: Featured,
    hero_file: Option<String>,
    reel_minutes: u32,
    transfer: Option<Transfer>,
    /// The reel menu, for this demo, under the key at this rect.
    reel_menu: Option<(DemoInfo, Rect, f64)>,
    /// Open the next card's reel menu as it is drawn.
    reel_on_next: bool,
    base: String,
}

impl DemosView {
    #[must_use]
    pub fn new(lobby_url: &str) -> Self {
        Self {
            tab: Tab::Featured,
            mode: 0,
            map: None,
            query: String::new(),
            page: 1,
            archive: Vec::new(),
            archive_yours: false,
            archive_request: None,
            archive_arrived: false,
            archive_failed: false,
            ownership: false,
            connected: false,
            featured_supported: false,
            featured: Featured::Idle,
            hero_file: None,
            reel_minutes: 2,
            transfer: None,
            reel_menu: None,
            reel_on_next: false,
            base: crate::client::demo_download::demo_base_url(lobby_url),
        }
    }

    // ── What the lobby says ──

    /// `show`: the page came into view; fetch what it shows.
    pub fn show(&mut self, client: &mut LobbyClient) {
        self.request_archive(client, true);
        self.request_featured(client);
    }

    /// `connected`: a lobby said hello, and what it can do.
    pub fn connected(&mut self, ownership: bool, featured: bool, visible: bool, client: &mut LobbyClient) {
        self.deactivate();
        self.archive_request = None;
        self.archive.clear();
        self.archive_arrived = false;
        self.archive_failed = false;
        self.connected = true;
        self.ownership = ownership;
        self.featured_supported = featured;
        if !ownership {
            if self.tab == Tab::Yours {
                self.tab = Tab::Latest;
            }
            self.archive_yours = false;
        }
        self.featured = Featured::Idle;
        if visible {
            self.show(client);
        }
    }

    /// `disconnected`.
    pub fn disconnected(&mut self) {
        self.deactivate();
        self.archive_request = None;
        self.archive_failed = false;
        self.ownership = false;
        self.connected = false;
        self.featured_supported = false;
        if self.tab == Tab::Yours {
            self.tab = Tab::Latest;
        }
        self.archive_yours = false;
        self.archive.clear();
        self.archive_arrived = false;
        self.featured = Featured::Idle;
    }

    /// `matchEnded`: the featured picks may have changed.
    pub fn match_ended(&mut self) {
        if !matches!(self.featured, Featured::Loading { .. }) {
            self.featured = Featured::Idle;
        }
    }

    /// `demos:list_response`, `featured:page` and `featured:error`.
    pub fn on_message(&mut self, kind: &str, message: &Value) {
        let id = message.get("requestId").and_then(Value::as_str);
        match kind {
            "demos:list_response" => {
                let scope = message.get("scope").and_then(Value::as_str).unwrap_or("latest");
                let ours = self.archive_request.as_ref().map(|(r, _)| r.as_str());
                if ours.is_none() || ours != id || (scope == "yours") != self.archive_yours {
                    return;
                }
                self.archive_request = None;
                self.archive_failed = false;
                self.archive = message
                    .get("demos")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(DemoInfo::read).collect())
                    .unwrap_or_default();
                self.archive_arrived = true;
            }
            "featured:page" | "featured:error" => {
                let Featured::Loading { request, .. } = &self.featured else {
                    return;
                };
                if Some(request.as_str()) != id {
                    return;
                }
                self.featured = if kind == "featured:page" {
                    Featured::Ready(
                        message
                            .get("entries")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(|e| {
                                        let demo = DemoInfo::read(e.get("demo")?)?;
                                        let reason = e.get("reason").and_then(Value::as_str).unwrap_or("");
                                        Some((demo, reason.to_owned()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    )
                } else {
                    let reason = message.get("reason").and_then(Value::as_str).unwrap_or("error");
                    Featured::Failed(format!("Featured demos: {reason}"))
                };
            }
            _ => {}
        }
    }

    /// For screenshots: the archive and the picks as if the lobby had sent them.
    pub fn seed(&mut self, demos: Vec<DemoInfo>, featured: Vec<(DemoInfo, String)>) {
        self.connected = true;
        self.ownership = true;
        self.featured_supported = true;
        self.archive = demos;
        self.archive_arrived = true;
        self.archive_request = None;
        self.featured = Featured::Ready(featured);
    }

    /// Opens a board by its id (`featured`, `latest` or `yours`).
    /// With `reel`, the first card's reel menu opens too (for screenshots).
    pub fn open_board(&mut self, board: &str, reel: bool) {
        self.reel_on_next = reel;
        self.tab = match board {
            "latest" => Tab::Latest,
            "yours" => Tab::Yours,
            _ => Tab::Featured,
        };
    }

    /// `deactivate`: leaving the page cancels its download.
    pub fn deactivate(&mut self) {
        if let Some(mut t) = self.transfer.take()
            && let Some(d) = &mut t.download
        {
            d.cancel();
        }
        self.reel_menu = None;
    }

    /// Esc: the reel menu closes first. True when something closed.
    pub fn back(&mut self) -> bool {
        self.reel_menu.take().is_some()
    }

    /// The picked mode's colours, which the page takes.
    #[must_use]
    pub fn accent(&self) -> Accent {
        page_modes()[self.mode].accent
    }

    // ── Asking ──

    fn request_archive(&mut self, client: &mut LobbyClient, force: bool) {
        if !self.connected {
            return;
        }
        let yours = match self.tab {
            Tab::Yours => true,
            Tab::Latest => false,
            Tab::Featured => self.archive_yours,
        };
        let mut force = force;
        if yours != self.archive_yours {
            self.archive_yours = yours;
            self.archive.clear();
            self.archive_arrived = false;
            force = true;
        }
        if self.archive_request.is_some() && !force {
            return;
        }
        self.archive_failed = false;
        let request = client.request_demo_list(DEMO_LIST_LIMIT, yours);
        self.archive_request = Some((request, Instant::now()));
    }

    fn request_featured(&mut self, client: &mut LobbyClient) {
        if !self.connected || !self.featured_supported {
            return;
        }
        if matches!(self.featured, Featured::Loading { .. } | Featured::Ready(_)) {
            return;
        }
        let modes = page_modes();
        let request = client.request_featured_demos(
            modes[self.mode].id,
            self.map.as_deref().unwrap_or("all"),
            FEATURED_LIMIT,
        );
        self.featured = Featured::Loading {
            request,
            sent: Instant::now(),
        };
    }

    fn time_out(&mut self) {
        if self
            .archive_request
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > ARCHIVE_TIMEOUT)
        {
            self.archive_request = None;
            self.archive_failed = true;
        }
        if matches!(&self.featured, Featured::Loading { sent, .. } if sent.elapsed() > ARCHIVE_TIMEOUT) {
            self.featured = Featured::Failed("Featured demos request timed out".into());
        }
    }

    // ── Choosing ──

    fn set_mode(&mut self, mode: usize, client: &mut LobbyClient) {
        self.mode = mode;
        if let Some(map) = &self.map
            && !self.in_mode().iter().any(|d| &d.map == map)
        {
            self.map = None;
        }
        self.page = 1;
        self.hero_file = None;
        self.refetch_featured(client);
    }

    fn set_map(&mut self, map: Option<String>, client: &mut LobbyClient) {
        self.map = map;
        self.page = 1;
        self.hero_file = None;
        self.refetch_featured(client);
    }

    fn refetch_featured(&mut self, client: &mut LobbyClient) {
        self.featured = Featured::Idle;
        self.request_featured(client);
    }

    fn set_tab(&mut self, tab: Tab, client: &mut LobbyClient) {
        if tab == Tab::Yours && !self.ownership {
            return;
        }
        if tab != self.tab {
            self.page = 1;
        }
        self.tab = tab;
        self.request_archive(client, false);
        if tab == Tab::Featured {
            self.request_featured(client);
        }
    }

    fn mode_of(&self, demo: &DemoInfo) -> PageMode {
        let modes = page_modes();
        modes.iter().find(|m| m.id == demo.mode()).copied().unwrap_or(modes[0])
    }

    fn in_mode(&self) -> Vec<DemoInfo> {
        let id = page_modes()[self.mode].id;
        self.archive
            .iter()
            .filter(|d| id == "all" || d.mode() == id)
            .cloned()
            .collect()
    }

    fn shown_list(&self) -> Vec<DemoInfo> {
        let query = self.query.trim().to_lowercase();
        let mut list: Vec<DemoInfo> = self
            .in_mode()
            .into_iter()
            .filter(|d| self.map.as_ref().is_none_or(|m| &d.map == m))
            .filter(|d| {
                query.is_empty()
                    || format!("{} {} {}", d.map, d.map_name(), d.players.join(" "))
                        .to_lowercase()
                        .contains(&query)
            })
            .collect();
        list.sort_by(|a, b| b.date.total_cmp(&a.date));
        list
    }

    // ── Watching ──

    fn play(&mut self, demo: &DemoInfo, reel: Option<u32>) {
        if self.transfer.as_ref().is_some_and(|t| t.failed.is_none()) {
            return;
        }
        self.reel_menu = None;
        self.transfer = Some(Transfer {
            demo: demo.clone(),
            reel: reel.map(f64::from),
            download: Some(DemoDownload::start(&self.base, &demo.filename)),
            failed: None,
        });
    }

    /// The download, moved on: the recording once it is in.
    fn poll_transfer(&mut self) -> Option<DemoChoice> {
        let t = self.transfer.as_mut()?;
        let d = t.download.as_mut()?;
        d.poll();
        match &d.state {
            DemoDownloadState::Loading { .. } => None,
            DemoDownloadState::Failed(why) => {
                t.failed = Some(why.clone());
                t.download = None;
                None
            }
            DemoDownloadState::Ready(path) => {
                let choice = DemoChoice {
                    path: path.clone(),
                    reel: t.reel,
                };
                self.transfer = None;
                Some(choice)
            }
        }
    }

    fn busy(&self) -> bool {
        self.transfer.as_ref().is_some_and(|t| t.failed.is_none())
    }

    // ── Drawing ──

    /// The stage. Returns a recording to play once it is downloaded.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        client: &mut LobbyClient,
        shots: &mut Levelshots,
    ) -> Option<DemoChoice> {
        self.time_out();
        let choice = self.poll_transfer();
        if self.transfer.is_some() {
            ui.ctx().request_repaint();
        }
        let painter = ui.painter().clone();
        let modes = page_modes();
        let mode = modes[self.mode];
        let tab = TABS.iter().find(|t| t.0 == self.tab).unwrap_or(&TABS[0]);
        let map_label = self.map.as_ref().map(|m| {
            self.archive
                .iter()
                .find(|d| &d.map == m)
                .map_or_else(|| m.clone(), |d| d.map_name().to_owned())
        });
        let kicker = ["Demos", if mode.id == "all" { "All modes" } else { mode.title }]
            .into_iter()
            .map(str::to_owned)
            .chain(map_label)
            .collect::<Vec<_>>()
            .join(" · ");
        let head = page_head(&painter, rect.left(), rect.top(), &kicker, tab.1, mode.accent.face);
        let chips: Vec<ModeChip<'_>> = modes
            .iter()
            .map(|m| ModeChip {
                id: m.id,
                title: m.title,
                mark: m.mark,
            })
            .collect();
        let keys_w = chips.len() as f32 * 110.0 - 10.0;
        let beside = rect.right() - keys_w - 24.0 >= head.right();
        let keys_bottom = if beside {
            head.bottom()
        } else {
            head.bottom() + 16.0 + 52.0
        };
        if let (Some(m), _) = mode_keys(ui, "dm-mode", rect.right(), keys_bottom, &chips, self.mode) {
            self.set_mode(m, client);
        }
        let mut y = keys_bottom.max(head.bottom()) + 18.0;
        let boards: Vec<(&str, &str)> = TABS.iter().map(|t| (t.1, t.2)).collect();
        let open = TABS.iter().position(|t| t.0 == self.tab).unwrap_or(0);
        let (picked, bottom) = board_keys(ui, "dm-board", rect.left(), y, rect.width(), &boards, open);
        if let Some(i) = picked {
            self.set_tab(TABS[i].0, client);
        }
        y = bottom + 18.0 - 10.6;
        y = rule(&painter, rect.left(), y, &self.rule_text()) + 18.0;
        if self.transfer.is_some() {
            y = self.transfer_bar(ui, Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 48.0)));
            y += 18.0;
        }
        let pane = Rect::from_min_max(pos2(rect.left(), y), rect.max);
        if self.tab == Tab::Featured {
            self.featured_pane(ui, pane, shots);
        } else {
            self.grid_pane(ui, pane, client, shots);
        }
        self.reel_sheet(ui);
        choice
    }

    fn rule_text(&self) -> String {
        match (&self.tab, &self.featured) {
            (Tab::Latest, _) => format!("Newest first · the latest {DEMO_LIST_LIMIT} recordings"),
            (Tab::Yours, _) => "Matches you played on a verified seat, newest first".into(),
            (_, Featured::Ready(f)) if !f.is_empty() => {
                if f.iter().any(|(_, reason)| reason.contains("top 10")) {
                    "This week's matches with players from the 30-day top 10".into()
                } else {
                    "No top-10 matches this week · the latest multiplayer matches instead".into()
                }
            }
            _ => "Recordings from the last 7 days".into(),
        }
    }

    /// `dm-transfer`: what is loading, or why it failed, and its key.
    fn transfer_bar(&mut self, ui: &mut egui::Ui, r: Rect) -> f32 {
        let painter = ui.painter().clone();
        painter.rect_filled(r, 0.0, hsl(254.0, 31.0, 18.0));
        let Some(t) = &self.transfer else {
            return r.bottom();
        };
        let (text, key) = match &t.failed {
            Some(why) => (why.clone(), "Try again"),
            None => {
                let progress = match t.download.as_ref().map(|d| &d.state) {
                    Some(DemoDownloadState::Loading {
                        received,
                        total: Some(total),
                    }) if *total > 0 => format!(" {}%", received * 100 / total),
                    _ => String::new(),
                };
                (format!("Loading {} recording…{progress}", t.demo.map_name()), "Cancel")
            }
        };
        let g = line(&painter, &key.to_uppercase(), 12.0, display_family(), ink(), 0.1);
        let kr = Rect::from_min_size(
            pos2(r.right() - 18.0 - g.size().x - 36.0, r.center().y - 17.0),
            vec2(g.size().x + 36.0, 32.0),
        );
        let body_g = wrapped(
            &painter,
            &text,
            FontId::new(16.0, body()),
            cream(),
            kr.left() - 16.0 - r.left() - 18.0,
        );
        painter.galley(
            pos2(r.left() + 18.0, r.center().y - body_g.size().y / 2.0),
            body_g,
            cream(),
        );
        let face = RaisedKey::new("dm-transfer", kr, KeyLook::cream().sm()).show(ui);
        icon_label(&painter, &face, None, key, 12.0, 0.1);
        if face.clicked {
            if t.failed.is_some() {
                let (demo, reel) = (t.demo.clone(), t.reel);
                self.transfer = None;
                self.play(&demo, reel.map(|m| m as u32));
            } else {
                self.deactivate();
            }
        }
        r.bottom()
    }

    /// A demo's slab: the side in its mode's colours and the dark face.
    /// Returns the face's corners.
    fn slab(painter: &Painter, r: Rect, cut: f32, mode: Option<&PageMode>) -> Vec<Pos2> {
        let (hi, lo) = mode.map_or((hsl(250.0, 36.0, 14.0), hsl(250.0, 40.0, 8.0)), |m| {
            (m.accent.side_hi, m.accent.side_lo)
        });
        let c = r.width() * cut;
        painter.add(Shape::convex_polygon(
            slant(r.translate(vec2(12.0, 16.0)).expand(3.0), c, c),
            hsla(250.0, 70.0, 2.0, 0.3),
            Stroke::NONE,
        ));
        gradient_polygon(
            painter,
            &slant(r.translate(vec2(8.0, 10.0)), c, c),
            &[(0.0, hi), (1.0, lo)],
        );
        let face = slant(r, c, c);
        gradient_polygon(
            painter,
            &face,
            &[(0.0, hsl(250.0, 26.0, 19.0)), (1.0, hsl(250.0, 32.0, 11.0))],
        );
        face
    }

    /// `dm-shot`: the levelshot (or its mode's gradient) under a dark wash,
    /// in the top `height` of a slab whose face is `face`.
    fn shot(
        painter: &Painter,
        face: &[Pos2],
        height: f32,
        demo: &DemoInfo,
        mode: &PageMode,
        shots: &mut Levelshots,
    ) -> Rect {
        let top = face[0].y;
        let full_h = face[3].y - top;
        let t = (height / full_h).clamp(0.0, 1.0);
        let bl = face[0].lerp(face[3], t);
        let br = face[1].lerp(face[2], t);
        let quad = [face[0], face[1], br, bl];
        let bounds = Rect::from_min_max(pos2(bl.x, face[0].y), pos2(face[1].x, bl.y));
        let map = demo.shot_map();
        shots.want([map.as_str()]);
        if let Some((tex, size)) = shots.texture_sized(&map) {
            // `background-size: cover`.
            let scale = (bounds.width() / size.x).max(bounds.height() / size.y);
            let shown = size * scale;
            let img = Rect::from_center_size(bounds.center(), shown);
            let uv = |p: Pos2| pos2((p.x - img.left()) / img.width(), (p.y - img.top()) / img.height());
            let mut mesh = egui::Mesh::with_texture(tex);
            for p in quad {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p,
                    uv: uv(p),
                    color: Color32::WHITE,
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3]);
            painter.add(Shape::mesh(mesh));
        } else {
            let tint = lerp(hsl(250.0, 30.0, 22.0), mode.mark, 0.3);
            gradient_polygon(painter, &quad, &[(0.0, tint), (1.0, hsl(250.0, 36.0, 10.0))]);
        }
        // `dm-shot::after`.
        let at = |f: f32| (face[0].lerp(bl, f), face[1].lerp(br, f));
        let band = |a: f32, b: f32, ca: Color32, cb: Color32| {
            let (p0, p1) = at(a);
            let (p3, p2) = at(b);
            gradient_polygon(painter, &[p0, p1, p2, p3], &[(0.0, ca), (1.0, cb)]);
        };
        band(0.0, 0.26, hsla(250.0, 50.0, 4.0, 0.4), Color32::TRANSPARENT);
        band(0.4, 1.0, Color32::TRANSPARENT, hsla(250.0, 50.0, 4.0, 0.88));
        bounds
    }

    /// `dm-shot__flags` and `__len`: the mode, extra chips and the map
    /// along the top, the length at the right. Returns the flags' bottom.
    #[allow(clippy::too_many_arguments)]
    fn flags(&self, painter: &Painter, shot: Rect, demo: &DemoInfo, hero: bool, featured: bool) -> f32 {
        let mode = self.mode_of(demo);
        let (inset, top, size, pad) = if hero {
            (30.0, 20.0, 12.0, (14.0, 7.0))
        } else {
            (22.0, 14.0, 11.0, (12.0, 6.0))
        };
        let len = line(
            painter,
            &clock(demo.duration_ms),
            if hero { 15.0 } else { 13.0 },
            display_family(),
            cream(),
            0.06,
        );
        let lp = if hero { (14.0, 8.0) } else { (12.0, 6.0) };
        let lr = Rect::from_min_size(
            pos2(
                shot.right() - (if hero { 28.0 } else { 18.0 }) - len.size().x - 2.0 * lp.0,
                shot.top() + top,
            ),
            vec2(len.size().x + 2.0 * lp.0, len.size().y + 2.0 * lp.1),
        );
        let c = lr.width() * 0.08;
        painter.add(Shape::convex_polygon(
            slant(lr, c, c),
            hsla(250.0, 50.0, 4.0, 0.72),
            Stroke::NONE,
        ));
        painter.galley(lr.center() - len.size() / 2.0, len, cream());
        let mut x = shot.left() + inset;
        let y = shot.top() + top;
        let label = if demo.players.len() > 2 {
            format!("{} · {}", mode.title, demo.players.len())
        } else {
            mode.title.to_owned()
        };
        x += chip(painter, pos2(x, y), &label, size, ink(), mode.mark, pad) + 8.0;
        if featured {
            let g = line(painter, "FEATURED", size, display_family(), gold_text(), 0.12);
            let r = Rect::from_min_size(
                pos2(x, y),
                vec2(g.size().x + size + 6.0 + 2.0 * pad.0, g.size().y + 2.0 * pad.1),
            );
            let c = r.width() * 0.08;
            painter.add(Shape::convex_polygon(slant(r, c, c), ink(), Stroke::NONE));
            paint_icon(
                painter,
                Icon::Star,
                pos2(r.left() + pad.0 + size * 0.45, r.center().y),
                size * 0.9,
                gold_text(),
            );
            painter.galley(
                pos2(r.left() + pad.0 + size + 6.0, r.center().y - g.size().y / 2.0),
                g,
                gold_text(),
            );
            x = r.right() + 8.0;
        }
        let map_size = if hero {
            clamp_css(18.0, painter.ctx().content_rect().width() * 0.016, 28.0)
        } else {
            clamp_css(14.0, painter.ctx().content_rect().width() * 0.0105, 19.0)
        };
        let room = lr.left() - 12.0 - (x + 4.0);
        let chip_h = size + 2.0 * pad.1;
        if room > 30.0 {
            let shown = elide(
                painter,
                &demo.map_name().to_uppercase(),
                map_size,
                display_family(),
                room,
            );
            let shadow = line(painter, &shown, map_size, display_family(), hsl(250.0, 50.0, 3.0), 0.05);
            let g = line(painter, &shown, map_size, display_family(), cream(), 0.05);
            let at = pos2(x + 4.0, y + chip_h / 2.0 + 1.0 - g.size().y / 2.0);
            painter.galley(at + vec2(2.0, 3.0), shadow, cream());
            painter.galley(at, g, cream());
        }
        y + chip_h.max(lr.height())
    }

    /// `renderFeatured`: the featured match beside the week's picks.
    fn featured_pane(&mut self, ui: &mut egui::Ui, pane: Rect, shots: &mut Levelshots) {
        let painter = ui.painter().clone();
        let vw = ui.ctx().content_rect().width();
        let (picks_w, gap) = if vw <= 1500.0 {
            (290.0, 28.0)
        } else {
            (clamp_css(300.0, vw * 0.23, 400.0), 38.0)
        };
        let area = Rect::from_min_max(pane.min, pos2(pane.right() - 10.0, pane.bottom() - 12.0));
        let hero_r = Rect::from_min_max(area.min, pos2(area.right() - picks_w - gap, area.bottom()));
        let picks_r = Rect::from_min_max(pos2(hero_r.right() + gap, area.top()), area.max);

        let empty = if !self.connected {
            Some("Connect to the lobby to see this week's picks.".to_owned())
        } else if !self.featured_supported {
            Some("Featured picks are unavailable on this lobby server. Latest has every match.".to_owned())
        } else {
            match &self.featured {
                Featured::Failed(why) => Some(why.clone()),
                Featured::Ready(f) if f.is_empty() => {
                    let modes = page_modes();
                    let mode = modes[self.mode];
                    let place = if self.map.is_some() {
                        " on this map".to_owned()
                    } else if mode.id != "all" {
                        format!(" in {}", mode.title)
                    } else {
                        String::new()
                    };
                    Some(format!(
                        "No multiplayer matches{place} this week yet. Latest has every match."
                    ))
                }
                Featured::Ready(_) => None,
                _ => Some("Loading this week's picks…".to_owned()),
            }
        };
        let picks: Vec<(DemoInfo, String)> = match &self.featured {
            Featured::Ready(f) => f.clone(),
            _ => Vec::new(),
        };
        // `dm-picks__label`.
        let label_c = hsla(45.0, 100.0, 95.0, 0.5);
        let g = line(&painter, "THIS WEEK'S BEST", 12.0, body(), label_c, 0.18);
        painter.galley(pos2(picks_r.left(), picks_r.top() + 2.0), g, label_c);
        if !picks.is_empty() {
            let g = line(&painter, &format!("{} PICKS", picks.len()), 12.0, body(), label_c, 0.18);
            painter.galley(pos2(picks_r.right() - g.size().x, picks_r.top() + 2.0), g, label_c);
        }
        if let Some(empty) = empty {
            let face = Self::slab(&painter, hero_r, 0.011, None);
            painter.line_segment([face[0], face[1]], Stroke::new(2.0, Color32::from_white_alpha(102)));
            let c = hsla(45.0, 100.0, 95.0, 0.55);
            let g = wrapped(&painter, &empty, FontId::new(18.0, body()), c, hero_r.width() - 80.0);
            painter.galley(hero_r.center() - g.size() / 2.0, g, c);
            return;
        }
        let current = picks
            .iter()
            .find(|(d, _)| Some(&d.filename) == self.hero_file.as_ref())
            .unwrap_or(&picks[0])
            .0
            .clone();
        self.hero_file = Some(current.filename.clone());
        self.hero(ui, hero_r, &current, shots);

        // `dm-picks__list`.
        let list = Rect::from_min_max(pos2(picks_r.left(), picks_r.top() + 2.0 + 12.0 + 14.0), picks_r.max);
        let mut picked = None;
        let busy = self.busy();
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("dm-picks")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (demo, reason) in &picks {
                        let (slot, _) =
                            ui.allocate_exact_size(vec2(list.width() - 14.0, 84.0 + 5.4 + 12.0), egui::Sense::hover());
                        let r = Rect::from_min_size(slot.min + vec2(2.0, 6.0), vec2(slot.width() - 8.0, 84.0));
                        let on = Some(&demo.filename) == self.hero_file.as_ref();
                        let _ = reason;
                        if self.pick(ui, r, demo, on, busy) {
                            picked = Some(demo.filename.clone());
                        }
                    }
                });
        });
        if let Some(file) = picked {
            self.hero_file = Some(file);
        }
        let _ = shots;
    }

    /// `raised-key--pick`: a channel in the list, its shot and who played.
    fn pick(&self, ui: &mut egui::Ui, r: Rect, demo: &DemoInfo, on: bool, _busy: bool) -> bool {
        let look = KeyLook {
            depth: 5.4,
            ..KeyLook::cream()
        };
        let face = RaisedKey::new(("dm-pick", demo.filename.as_str()), r, look)
            .current(on)
            .show(ui);
        let painter = ui.painter();
        let mode = self.mode_of(demo);
        let fr = face.rect;
        let vw = ui.ctx().content_rect().width();
        let shot_w = clamp_css(84.0, vw * 0.064, 118.0).min(fr.height() * 1.6 - 20.0);
        let sr = Rect::from_min_size(pos2(fr.left() + 12.0, fr.top() + 10.0), vec2(shot_w, shot_w / 1.6));
        let c = sr.width() * 0.07;
        let tint = lerp(hsl(250.0, 30.0, 20.0), mode.mark, 0.4);
        gradient_polygon(painter, &slant(sr, c, c), &[(0.0, tint), (1.0, hsl(250.0, 36.0, 12.0))]);
        let tx = sr.right() + 14.0;
        let tw = fr.right() - 18.0 - tx;
        let size = clamp_css(13.0, vw * 0.0095, 16.0);
        let mut y = fr.top() + 12.0;
        let mut name_line = |prefix: Option<&str>, name: &str| {
            let mut x = tx;
            if let Some(p) = prefix {
                let g = line(painter, p, 10.0, display_family(), face.sub, 0.0);
                painter.galley(pos2(x, y + size * 0.55 - g.size().y / 2.0), g.clone(), face.sub);
                x += g.size().x + 6.0;
            }
            let shown = elide(painter, name, size, display_family(), tx + tw - x);
            let g = line(painter, &shown, size, display_family(), face.text, 0.02);
            let h = g.size().y;
            painter.galley(pos2(x, y), g, face.text);
            y += h + 6.0;
        };
        if demo.players.len() == 2 {
            name_line(None, &demo.players[0]);
            name_line(Some("VS"), &demo.players[1]);
        } else {
            name_line(None, demo.players.first().map_or(demo.map_name(), String::as_str));
            if demo.players.len() > 1 {
                name_line(None, &format!("+{} more", demo.players.len() - 1));
            }
        }
        // `dm-pick__meta`: the mark, mode and map, how long ago.
        let my = fr.bottom() - 12.0 - 6.0;
        painter.add(Shape::convex_polygon(
            slant(Rect::from_min_size(pos2(tx, my - 5.0), vec2(10.0, 10.0)), 2.2, 2.2),
            mode.mark,
            Stroke::NONE,
        ));
        let when = line(
            painter,
            &ago(demo.date, true, now_ms()).to_uppercase(),
            12.0,
            body(),
            face.sub,
            0.1,
        );
        let wx = tx + tw - when.size().x;
        painter.galley(pos2(wx, my - when.size().y / 2.0), when, face.sub);
        let what = format!("{} · {}", mode.title, demo.map_name()).to_uppercase();
        let shown = elide(painter, &what, 12.0, body(), wx - 8.0 - (tx + 18.0));
        let g = line(painter, &shown, 12.0, body(), face.sub, 0.1);
        painter.galley(pos2(tx + 18.0, my - g.size().y / 2.0), g, face.sub);
        face.clicked
    }

    /// `dm-hero`: the featured match, its shot, who played, and its keys.
    fn hero(&mut self, ui: &mut egui::Ui, r: Rect, demo: &DemoInfo, shots: &mut Levelshots) {
        let painter = ui.painter().clone();
        let vw = ui.ctx().content_rect().width();
        let short = ui.ctx().content_rect().height() <= 960.0;
        let mode = self.mode_of(demo);
        let face = Self::slab(&painter, r, 0.011, Some(&mode));
        // `dm-hero__foot` wraps: when its keys leave no room for when, that
        // goes under them.
        let watch_size = clamp_css(16.0, vw * 0.0125, 22.0);
        let ago_size = clamp_css(16.0, vw * 0.013, 22.0);
        let sub = hsla(45.0, 100.0, 95.0, 0.58);
        let ag = line(
            &painter,
            &ago(demo.date, false, now_ms()).to_uppercase(),
            ago_size,
            display_family(),
            cream(),
            0.05,
        );
        let when = jiff::Timestamp::from_millisecond(demo.date as i64)
            .map(|t| t.to_zoned(jiff::tz::TimeZone::system()))
            .map_or_else(|_| String::new(), |z| z.strftime("%a %H:%M").to_string());
        let wg2 = line(
            &painter,
            &format!("{when} · {}", demo.size_text()).to_uppercase(),
            14.0,
            body(),
            sub,
            0.12,
        );
        let keys_w = {
            let w = |s: &str, size: f32| line(&painter, s, size, display_family(), ink(), 0.1).size().x;
            34.0 + w("WATCH", watch_size)
                + watch_size * 1.3
                + 78.0
                + 7.8
                + 28.0
                + w("HIGHLIGHTS", 14.0)
                + 68.0
                + 18.0
                + 3.0 * 56.0
        };
        let right = r.right() - 28.0 - r.width() * 0.011;
        let where_w = ag.size().x.max(wg2.size().x).max(150.0);
        let where_h = ag.size().y + 9.0 + wg2.size().y;
        let wrap = r.left() + keys_w + 26.0 + where_w > right;
        let foot_h =
            if short { 14.0 } else { 20.0 } + 62.0 + 7.8 + 10.0 + 6.0 + if wrap { where_h + 14.0 } else { 0.0 };
        let shot = Self::shot(&painter, &face, r.height() - foot_h, demo, &mode, shots);
        painter.line_segment([face[0], face[1]], Stroke::new(2.0, Color32::from_white_alpha(102)));
        self.flags(&painter, shot, demo, true, true);
        // `dm-vs`: who played, big, across the bottom of the shot.
        let vs_left = shot.left() + 34.0;
        let vs_right = shot.right() - 38.0;
        let bottom = shot.bottom() - 26.0;
        if demo.players.len() == 2 {
            let size = clamp_css(26.0, vw * 0.025, 48.0);
            let badge_size = clamp_css(16.0, vw * 0.013, 22.0);
            let badge = line(&painter, "VS", badge_size, display_family(), ink(), 0.04);
            let bw = badge.size().x + 30.0;
            let side_w = (vs_right - vs_left - bw - 56.0) / 2.0;
            for (i, name) in demo.players.iter().enumerate() {
                let shown = elide(&painter, name, size, display_family(), side_w);
                let g = line(&painter, &shown, size, display_family(), cream(), 0.01);
                let x = if i == 0 { vs_left } else { vs_right - g.size().x };
                let at = pos2(x, bottom - g.size().y);
                painter.galley_with_override_text_color(at + vec2(4.0, 5.0), g.clone(), hsl(250.0, 50.0, 3.0));
                painter.galley(at, g, cream());
            }
            let br = Rect::from_center_size(
                pos2(
                    (vs_left + vs_right) / 2.0,
                    bottom - 6.0 - (badge.size().y + 18.0) / 2.0 - 4.0,
                ),
                vec2(bw, badge.size().y + 18.0),
            );
            let c = br.width() * 0.1;
            painter.add(Shape::convex_polygon(slant(br, c, c), mode.mark, Stroke::NONE));
            painter.galley(br.center() - badge.size() / 2.0, badge, ink());
        } else {
            let size = clamp_css(18.0, vw * 0.015, 28.0);
            let mut rows: Vec<Vec<std::sync::Arc<egui::Galley>>> = vec![Vec::new()];
            let mut w = 0.0;
            for name in &demo.players {
                let g = line(&painter, name, size, display_family(), cream(), 0.0);
                if w + g.size().x > vs_right - vs_left && !rows.last().is_some_and(Vec::is_empty) {
                    rows.push(Vec::new());
                    w = 0.0;
                }
                w += g.size().x + 22.0;
                if let Some(r) = rows.last_mut() {
                    r.push(g);
                }
            }
            let row_h = size * 1.1 + 6.0;
            let mut y = bottom - rows.len() as f32 * row_h;
            let count_y = y - 12.0 - 27.0;
            chip(
                &painter,
                pos2(vs_left, count_y),
                &format!("{} players", demo.players.len()),
                13.0,
                ink(),
                mode.mark,
                (14.0, 7.0),
            );
            for row in rows {
                let mut x = vs_left;
                for g in row {
                    let gw = g.size().x;
                    painter.galley_with_override_text_color(pos2(x + 3.0, y + 4.0), g.clone(), hsl(250.0, 50.0, 3.0));
                    painter.galley(pos2(x, y), g, cream());
                    x += gw + 22.0;
                }
                y += row_h;
            }
        }

        // `dm-hero__foot`: Watch, Highlights with its lengths, and when.
        let busy = self.busy();
        let fy = shot.bottom() + if short { 14.0 } else { 20.0 };
        let wg = line(&painter, "WATCH", watch_size, display_family(), ink(), 0.1);
        let wr = Rect::from_min_size(
            pos2(r.left() + 34.0, fy),
            vec2(wg.size().x + watch_size * 1.3 + 78.0, 62.0),
        );
        let look = KeyLook {
            depth: 7.8,
            cut: 0.05,
            ..KeyLook::gold()
        };
        let face = RaisedKey::new(("dm-hero-watch", demo.filename.as_str()), wr, look)
            .disabled(busy)
            .show(ui);
        let loading = self
            .transfer
            .as_ref()
            .is_some_and(|t| t.failed.is_none() && t.demo.filename == demo.filename && t.reel.is_none());
        icon_label(
            &painter,
            &face,
            (!loading).then_some(Icon::Play),
            if loading { "Loading…" } else { "Watch" },
            watch_size,
            0.1,
        );
        if face.clicked {
            self.play(demo, None);
        }
        let allowed = reel_lengths(demo.duration_ms);
        let chosen = if allowed.contains(&self.reel_minutes) {
            Some(self.reel_minutes)
        } else {
            allowed.last().copied()
        };
        let hg = line(&painter, "HIGHLIGHTS", 14.0, display_family(), ink(), 0.1);
        let hr = Rect::from_min_size(
            pos2(wr.right() + 7.8 + 2.0 + 26.0, fy + 5.0),
            vec2(hg.size().x + 14.0 + 46.0 + 8.0, 52.0),
        );
        let face = RaisedKey::new(("dm-hero-reel", demo.filename.as_str()), hr, KeyLook::cream())
            .disabled(busy || allowed.is_empty())
            .show(ui);
        icon_label(&painter, &face, Some(Icon::Bolt), "Highlights", 14.0, 0.1);
        if face.clicked
            && let Some(m) = chosen
        {
            self.play(demo, Some(m));
        }
        let mut lx = hr.right() + 6.0 + 12.0;
        for minutes in REEL_MINUTES {
            let label = format!("{minutes} min");
            let g = line(&painter, &label.to_uppercase(), 12.0, display_family(), cream(), 0.06);
            let kr = Rect::from_min_size(pos2(lx, fy + 5.0 + 7.0), vec2((g.size().x + 20.0).max(50.0), 38.0));
            lx = kr.right() + 6.0;
            if allowed.contains(&minutes) {
                if flat_key(ui, ("dm-len", minutes), kr, &label, None, Some(minutes) == chosen, 12.0) {
                    self.reel_minutes = minutes;
                }
            } else {
                ui.scope(|ui| {
                    ui.set_opacity(0.35);
                    flat_key(ui, ("dm-len", minutes), kr, &label, None, false, 12.0);
                });
            }
        }
        // `dm-hero__where`, at the right or under the keys.
        let top = if wrap {
            fy + 62.0 + 7.8 + 14.0
        } else {
            fy + 31.0 - where_h / 2.0
        };
        let _ = lx;
        let ah = ag.size().y;
        painter.galley(pos2(right - ag.size().x, top), ag, cream());
        painter.galley(pos2(right - wg2.size().x, top + ah + 9.0), wg2, sub);
    }

    /// `renderGrid`: the toolbar, a page of cards, and the pager.
    fn grid_pane(&mut self, ui: &mut egui::Ui, pane: Rect, client: &mut LobbyClient, shots: &mut Levelshots) {
        let painter = ui.painter().clone();
        let list = self.shown_list();
        let pages = list.len().div_ceil(DEMOS_PER_PAGE).max(1);
        self.page = self.page.clamp(1, pages);
        let shown: Vec<DemoInfo> = list
            .iter()
            .skip((self.page - 1) * DEMOS_PER_PAGE)
            .take(DEMOS_PER_PAGE)
            .cloned()
            .collect();
        // `dm-toolbar`: the search and the count.
        let field = Rect::from_min_size(pane.min, vec2(360.0_f32.min(pane.width() * 0.4), 46.0));
        let before = self.query.clone();
        TextField::new("dm-find", field, &mut self.query, FontId::new(16.0, body()))
            .placeholder("Find a player or map…")
            .enabled(!self.archive.is_empty())
            .show(ui);
        if self.query != before {
            self.page = 1;
        }
        let sub = hsla(45.0, 100.0, 95.0, 0.55);
        let tail = format!(
            "{}{}",
            if list.len() == 1 { "demo" } else { "demos" },
            if pages > 1 {
                format!(" · page {} of {pages}", self.page)
            } else {
                String::new()
            }
        )
        .to_uppercase();
        let tg = line(&painter, &tail, 13.0, body(), sub, 0.18);
        let ng = line(&painter, &list.len().to_string(), 15.0, display_family(), cream(), 0.04);
        let cy = field.center().y;
        let tx = pane.right() - tg.size().x;
        painter.galley(pos2(tx, cy - tg.size().y / 2.0), tg, sub);
        painter.galley(pos2(tx - 4.0 - ng.size().x, cy - ng.size().y / 2.0), ng, cream());

        let pager_h = if pages > 1 { 44.0 + 3.6 + 22.0 } else { 0.0 };
        let grid = Rect::from_min_max(
            pos2(pane.left(), field.bottom() + 22.0),
            pos2(pane.right() - 10.0, pane.bottom() - pager_h - 12.0),
        );
        let vw = ui.ctx().content_rect().width();
        let (gy, gx) = if vw <= 1500.0 { (24.0, 26.0) } else { (30.0, 34.0) };
        if shown.is_empty() {
            let empty = if !self.connected {
                "Lobby server offline. Connect to browse recordings.".to_owned()
            } else if self.archive_failed {
                "The archive did not answer. Try again.".to_owned()
            } else if !self.archive_arrived {
                "Loading the archive…".to_owned()
            } else if !self.query.trim().is_empty() {
                format!("No demos with “{}” in them", self.query.trim())
            } else if !self.archive.is_empty() {
                "No demos for this mode and map.".to_owned()
            } else if self.tab == Tab::Yours {
                "No recordings of yours yet. A match you play lands here once it ends.".to_owned()
            } else {
                "No recorded demos yet. Play a match.".to_owned()
            };
            let g = wrapped(&painter, &empty, FontId::new(18.0, body()), sub, grid.width() * 0.7);
            let gh = g.size();
            painter.galley(grid.center() - gh / 2.0, g, sub);
            if self.archive_failed && self.connected {
                let kr = Rect::from_center_size(grid.center() + vec2(0.0, gh.y / 2.0 + 40.0), vec2(160.0, 44.0));
                let face = RaisedKey::new("dm-retry", kr, KeyLook::cream().sm()).show(ui);
                icon_label(&painter, &face, None, "Try again", 13.0, 0.1);
                if face.clicked {
                    self.request_archive(client, true);
                }
            }
        } else {
            let cw = (grid.width() - 2.0 * gx) / 3.0;
            let ch = (grid.height() - gy) / 2.0;
            for (i, demo) in shown.iter().enumerate() {
                let (col, row) = (i % 3, i / 3);
                let r = Rect::from_min_size(
                    pos2(
                        grid.left() + col as f32 * (cw + gx),
                        grid.top() + row as f32 * (ch + gy),
                    ),
                    vec2(cw, ch),
                );
                self.card(ui, r, demo, shots);
            }
        }
        if pages > 1 {
            self.pager(ui, pos2(pane.center().x, pane.bottom() - 44.0 - 3.6 - 12.0), pages);
        }
    }

    /// `dm-card`.
    fn card(&mut self, ui: &mut egui::Ui, r: Rect, demo: &DemoInfo, shots: &mut Levelshots) {
        let hovered = ui.rect_contains_pointer(r);
        let r = if hovered { r.translate(vec2(-2.0, -3.0)) } else { r };
        let painter = ui.painter().clone();
        let mode = self.mode_of(demo);
        let short = ui.ctx().content_rect().height() <= 960.0;
        let foot_h = 13.0 + 42.0 + 3.6 + 5.0 + 4.0;
        let face = Self::slab(&painter, r, 0.018, Some(&mode));
        let shot = Self::shot(&painter, &face, r.height() - foot_h, demo, &mode, shots);
        painter.line_segment([face[0], face[1]], Stroke::new(2.0, Color32::from_white_alpha(102)));
        let flags_bottom = self.flags(&painter, shot, demo, false, false);
        // `dm-mu`: who played.
        let vw = ui.ctx().content_rect().width();
        let size = clamp_css(13.0, vw * 0.0095, 16.0);
        let mut row_h: f32 = if short { 28.0 } else { 32.0 };
        let left = shot.left() + 18.0;
        let right = shot.right() - 22.0;
        let names: Vec<String> = if demo.players.len() == 2 {
            demo.players.clone()
        } else if demo.players.is_empty() {
            vec![format!("{} match", demo.map_name())]
        } else {
            let mut n: Vec<String> = demo.players.iter().take(2).cloned().collect();
            if demo.players.len() > 2 {
                n.push(format!("+{} more", demo.players.len() - 2));
            }
            n
        };
        // The rows give way to the flags rather than cover them.
        let room = shot.bottom() - 14.0 - flags_bottom - 8.0;
        row_h = row_h.min(room / names.len() as f32 - 4.0).max(18.0);
        let vs = demo.players.len() == 2;
        let rows_left = if vs { left + 42.0 + 6.0 } else { left };
        let mut y = shot.bottom() - 14.0 - names.len() as f32 * (row_h + 4.0) + 4.0;
        if vs {
            let vr = Rect::from_min_size(pos2(left, y), vec2(42.0, 2.0 * row_h + 4.0));
            painter.add(Shape::convex_polygon(
                vec![
                    vr.left_top(),
                    vr.right_top(),
                    pos2(vr.right() - 7.0, vr.bottom()),
                    vr.left_bottom(),
                ],
                mode.mark,
                Stroke::NONE,
            ));
            let g = line(&painter, "VS", 14.0, display_family(), ink(), 0.02);
            painter.galley(vr.center() - g.size() / 2.0, g, ink());
        }
        for name in &names {
            let rr = Rect::from_min_max(pos2(rows_left, y), pos2(right, y + row_h));
            painter.add(Shape::convex_polygon(
                vec![
                    rr.left_top(),
                    rr.right_top(),
                    pos2(rr.right() - 8.0, rr.bottom()),
                    rr.left_bottom(),
                ],
                hsla(250.0, 50.0, 5.0, 0.74),
                Stroke::NONE,
            ));
            let shown = elide(&painter, name, size, display_family(), rr.width() - 28.0);
            let g = line(&painter, &shown, size, display_family(), cream(), 0.03);
            painter.galley(pos2(rr.left() + 12.0, rr.center().y - g.size().y / 2.0), g, cream());
            y += row_h + 4.0;
        }
        // `dm-card__foot`: Watch, Highlights, and when.
        let busy = self.busy();
        let fy = shot.bottom() + 13.0;
        // The bolt alone on a narrow card (`dm-long`).
        let narrow = vw <= 1500.0 || r.width() < 420.0;
        let wg = line(&painter, "WATCH", 12.0, display_family(), ink(), 0.1);
        let wr = Rect::from_min_size(pos2(r.left() + 22.0, fy), vec2(wg.size().x + 12.0 * 1.3 + 34.0, 42.0));
        let face = RaisedKey::new(("dm-watch", demo.filename.as_str()), wr, KeyLook::gold().sm())
            .disabled(busy)
            .show(ui);
        let loading = self
            .transfer
            .as_ref()
            .is_some_and(|t| t.failed.is_none() && t.demo.filename == demo.filename);
        icon_label(
            &painter,
            &face,
            (!loading).then_some(Icon::Play),
            if loading { "Loading…" } else { "Watch" },
            12.0,
            0.1,
        );
        if face.clicked {
            self.play(demo, None);
        }
        let allowed = reel_lengths(demo.duration_ms);
        let label = if narrow { "" } else { "Highlights" };
        let hg = line(&painter, &label.to_uppercase(), 12.0, display_family(), ink(), 0.1);
        let hr = Rect::from_min_size(
            pos2(wr.right() + 2.7 + 2.0 + 12.0, fy),
            vec2(
                hg.size().x + 12.0 + 34.0 + if label.is_empty() { 0.0 } else { 6.0 },
                42.0,
            ),
        );
        let face = RaisedKey::new(("dm-reel", demo.filename.as_str()), hr, KeyLook::cream().sm())
            .disabled(busy || allowed.is_empty())
            .show(ui);
        let bolt = if label.is_empty() { 18.0 } else { 12.0 };
        icon_label(&painter, &face, Some(Icon::Bolt), label, bolt, 0.1);
        if face.clicked || std::mem::take(&mut self.reel_on_next) {
            self.reel_menu = Some((demo.clone(), face.rect, ui.input(|i| i.time)));
        }
        // `dm-card__where`.
        let room = r.right() - 14.0 - r.width() * 0.018 - (hr.right() + 12.0);
        if room > 30.0 {
            let ago_t = elide(
                &painter,
                &ago(demo.date, true, now_ms()).to_uppercase(),
                17.0,
                body(),
                room,
            );
            let ag = line(&painter, &ago_t, 17.0, body(), cream(), 0.08);
            let sub = hsla(45.0, 100.0, 95.0, 0.55);
            let size_t = elide(&painter, &demo.size_text().to_uppercase(), 13.0, body(), room);
            let sg = line(&painter, &size_t, 13.0, body(), sub, 0.1);
            let right = r.right() - 14.0 - r.width() * 0.018;
            let top = fy + 21.0 - (ag.size().y + 7.0 + sg.size().y) / 2.0;
            let ah = ag.size().y;
            painter.galley(pos2(right - ag.size().x, top), ag, cream());
            painter.galley(pos2(right - sg.size().x, top + ah + 7.0), sg, sub);
        }
    }

    /// `dm-pager`: Newer, the page numbers, Older; centred on `center_top`.
    fn pager(&mut self, ui: &mut egui::Ui, center_top: Pos2, pages: usize) {
        let painter = ui.painter().clone();
        let numbers = page_keys(self.page, pages);
        enum Item {
            Newer,
            Gap,
            Page(usize),
            Older,
        }
        let mut items = vec![Item::Newer];
        for (i, &n) in numbers.iter().enumerate() {
            if i > 0 && n - numbers[i - 1] > 1 {
                items.push(Item::Gap);
            }
            items.push(Item::Page(n));
        }
        items.push(Item::Older);
        let width = |item: &Item| match item {
            Item::Newer | Item::Older => 106.0,
            Item::Gap => 18.0,
            Item::Page(_) => 48.0,
        };
        let gap = 12.0 + 2.7 + 2.0;
        let total: f32 = items.iter().map(width).sum::<f32>() + gap * (items.len() as f32 - 1.0);
        let mut x = center_top.x - total / 2.0;
        let mut go = None;
        for item in &items {
            let w = width(item);
            let r = Rect::from_min_size(pos2(x, center_top.y), vec2(w, 44.0));
            x += w + gap;
            match item {
                Item::Gap => {
                    let c = hsla(45.0, 100.0, 95.0, 0.45);
                    let g = line(&painter, "…", 16.0, display_family(), c, 0.0);
                    painter.galley(r.center() - g.size() / 2.0, g, c);
                }
                Item::Page(n) => {
                    let face = RaisedKey::new(("dm-page", *n), r, KeyLook::cream().sm())
                        .current(*n == self.page)
                        .show(ui);
                    icon_label(&painter, &face, None, &n.to_string(), 14.0, 0.06);
                    if face.clicked {
                        go = Some(*n);
                    }
                }
                Item::Newer | Item::Older => {
                    let newer = matches!(item, Item::Newer);
                    let disabled = if newer { self.page == 1 } else { self.page == pages };
                    let face = RaisedKey::new(("dm-pager", newer), r, KeyLook::gold().sm())
                        .disabled(disabled)
                        .show(ui);
                    let label = if newer { "Newer" } else { "Older" };
                    let g = line(&painter, &label.to_uppercase(), 14.0, display_family(), face.text, 0.06);
                    let cw = 10.0;
                    let total = g.size().x + cw;
                    let left = face.rect.center().x - total / 2.0;
                    let cy = face.rect.center().y;
                    let (gx, chev_x) = if newer {
                        (left + cw, left + 3.0)
                    } else {
                        (left, left + g.size().x + 7.0)
                    };
                    painter.galley(pos2(gx, cy - g.size().y / 2.0), g, face.text);
                    let d = if newer { -1.0 } else { 1.0 };
                    painter.add(Shape::line(
                        vec![
                            pos2(chev_x - d * 2.0, cy - 4.0),
                            pos2(chev_x + d * 2.0, cy),
                            pos2(chev_x - d * 2.0, cy + 4.0),
                        ],
                        Stroke::new(1.8, face.text),
                    ));
                    if face.clicked {
                        go = Some(if newer { self.page - 1 } else { self.page + 1 });
                    }
                }
            }
        }
        if let Some(p) = go {
            self.page = p;
        }
    }

    /// `dm-reel`: the reel menu over a card, how long a cut of its best moments.
    fn reel_sheet(&mut self, ui: &mut egui::Ui) {
        let Some((demo, key, opened)) = self.reel_menu.clone() else {
            return;
        };
        let screen = ui.ctx().content_rect();
        let w = 440.0_f32.min(screen.width() * 0.92);
        let h = 18.0 + 36.0 + 6.0 + 18.0 + 16.0 + 64.0 + 16.0 + 18.0 + 20.0;
        let left = (key.center().x - w / 2.0).clamp(screen.left() + 16.0, screen.right() - w - 16.0);
        let above = key.top() - h - 18.0;
        let top = if above < 12.0 { key.bottom() + 18.0 } else { above };
        let r = Rect::from_min_size(pos2(left, top), vec2(w, h));
        let mut close = false;
        let mut play = None;
        egui::Area::new(egui::Id::NULL.with("dm-reel-sheet"))
            .order(egui::Order::Foreground)
            .fixed_pos(r.min)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(r.size());
                let painter = ui.painter().clone();
                painter.rect_filled(
                    r.translate(vec2(0.0, 30.0)).expand(18.0),
                    30.0,
                    hsla(250.0, 80.0, 2.0, 0.35),
                );
                let shape = vec![
                    r.left_top(),
                    pos2(r.right() - 26.0, r.top()),
                    pos2(r.right(), r.top() + 26.0),
                    r.right_bottom(),
                    r.left_bottom(),
                ];
                gradient_polygon(
                    &painter,
                    &shape,
                    &[(0.0, hsl(48.0, 100.0, 60.0)), (1.0, hsl(42.0, 97.0, 50.0))],
                );
                let stripe = Rect::from_min_size(r.min, vec2(8.0, r.height()));
                gradient_polygon(
                    &painter,
                    &[
                        stripe.left_top(),
                        stripe.right_top(),
                        stripe.right_bottom(),
                        stripe.left_bottom(),
                    ],
                    &[(0.0, violet()), (1.0, hsl(262.0, 75.0, 42.0))],
                );
                let vw = screen.width();
                let qs = clamp_css(18.0, vw * 0.013, 22.0);
                let x = r.left() + 30.0;
                let mut y = r.top() + 18.0;
                paint_icon(&painter, Icon::Bolt, pos2(x + qs * 0.4, y + 18.0), qs * 0.8, ink());
                let q = line(&painter, "HIGHLIGHT REEL", qs, display_family(), ink(), 0.04);
                painter.galley(pos2(x + qs * 1.2, y + 18.0 - q.size().y / 2.0), q, ink());
                let cr = Rect::from_min_size(pos2(r.right() - 22.0 - 36.0, y), vec2(36.0, 36.0));
                let resp = ui.interact(cr, egui::Id::NULL.with("dm-reel-close"), egui::Sense::click());
                let (bg, fg) = if resp.hovered() {
                    (ink(), gold_text())
                } else {
                    (hsla(250.0, 45.0, 8.0, 0.12), ink())
                };
                painter.rect_filled(cr, 3.0, bg);
                let c = cr.center();
                for d in [vec2(5.0, 5.0), vec2(5.0, -5.0)] {
                    painter.line_segment([c - d, c + d], Stroke::new(2.4, fg));
                }
                close |= resp.clicked();
                y += 36.0 + 6.0;
                let who = if demo.players.len() == 2 {
                    format!("{} vs {}", demo.players[0], demo.players[1])
                } else {
                    format!("{} players", demo.players.len())
                };
                let sub_c = hsla(250.0, 45.0, 8.0, 0.66);
                let sub = elide(
                    &painter,
                    &format!("{who} · {}", demo.map_name()).to_uppercase(),
                    15.0,
                    body(),
                    r.right() - 22.0 - x,
                );
                let g = line(&painter, &sub, 15.0, body(), sub_c, 0.06);
                painter.galley(pos2(x, y), g, sub_c);
                y += 18.0 + 16.0;
                let allowed = reel_lengths(demo.duration_ms);
                let ow = (r.right() - 22.0 - x - 24.0) / 3.0;
                for (i, minutes) in REEL_MINUTES.into_iter().enumerate() {
                    let or = Rect::from_min_size(pos2(x + i as f32 * (ow + 12.0), y), vec2(ow, 58.0));
                    let look = KeyLook {
                        depth: 5.0,
                        cut: 0.0,
                        ..KeyLook::cream()
                    };
                    let face = RaisedKey::new(("dm-reel-opt", minutes), or, look)
                        .disabled(!allowed.contains(&minutes))
                        .alpha(if allowed.contains(&minutes) { 1.0 } else { 0.35 })
                        .show(ui);
                    let b = line(&painter, &minutes.to_string(), 30.0, display_family(), face.text, 0.0);
                    let s = line(&painter, "MIN", 14.0, body(), face.text, 0.14);
                    let tw = b.size().x + 6.0 + s.size().x;
                    let bx = face.rect.center().x - tw / 2.0;
                    let base = face.rect.center().y + b.size().y * 0.35;
                    let bw = b.size().x;
                    painter.galley(pos2(bx, base - b.size().y * 0.85), b, face.text);
                    painter.galley(pos2(bx + bw + 6.0, base - s.size().y * 0.85), s, face.text);
                    if face.clicked {
                        play = Some(minutes);
                    }
                }
                y += 64.0 + 16.0;
                let hint_c = hsla(250.0, 45.0, 8.0, 0.62);
                let g = line(
                    &painter,
                    "KILLS, SPREES AND CLOSE CALLS, CUT TOGETHER",
                    14.0,
                    body(),
                    hint_c,
                    0.06,
                );
                painter.galley(pos2(x, y), g, hint_c);
            });
        // A press anywhere else closes it (`onDocumentDown`), not the one that opened it.
        let (pressed, at, now) = ui.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos(), i.time));
        if pressed && now > opened + 0.05 && at.is_some_and(|p| !r.contains(p)) {
            close = true;
        }
        if let Some(m) = play {
            self.reel_menu = None;
            self.play(&demo, Some(m));
        } else if close {
            self.reel_menu = None;
        }
    }

    /// `renderMaps`: the maps with how many recordings each, in the rail.
    pub fn rail(&mut self, ui: &mut egui::Ui, mid: Rect, rail_w: f32, client: &mut LobbyClient) {
        let painter = ui.painter().clone();
        let in_mode = self.in_mode();
        let mut maps: Vec<&DemoInfo> = Vec::new();
        for d in &in_mode {
            if !maps.iter().any(|m| m.map == d.map) {
                maps.push(d);
            }
        }
        maps.sort_by_key(|d| d.map_name().to_lowercase());
        let count = if maps.is_empty() {
            String::new()
        } else {
            format!("{} {}", maps.len(), if maps.len() == 1 { "map" } else { "maps" })
        };
        let top = rail_label(&painter, mid, rail_w, "Maps", &count);
        let list = Rect::from_min_max(
            pos2(mid.left() + 22.0, top + 8.0),
            pos2(
                mid.left() + rail_w - shagger::ui::components::lobby_twin::RAIL_SKEW - 26.0,
                mid.bottom(),
            ),
        );
        if !self.connected || (!self.archive_arrived && maps.is_empty()) {
            let why = if !self.connected {
                "Lobby server offline."
            } else if self.archive_failed {
                "Archive unavailable. Use Try again."
            } else {
                "Loading the archive…"
            };
            let sub = hsla(250.0, 45.0, 8.0, 0.62);
            let g = wrapped(&painter, why, FontId::new(15.0, body()), sub, list.width().min(240.0));
            painter.galley(list.min, g, sub);
            return;
        }
        let modes = page_modes();
        let key_h = if ui.ctx().content_rect().height() <= 960.0 {
            40.0
        } else {
            46.0
        };
        let mut rows: Vec<(Option<String>, String, usize, Color32)> =
            vec![(None, "All maps".into(), in_mode.len(), modes[self.mode].mark)];
        for d in &maps {
            let n = in_mode.iter().filter(|o| o.map == d.map).count();
            rows.push((Some(d.map.clone()), d.map_name().to_owned(), n, self.mode_of(d).mark));
        }
        let mut picked = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("dm-maps")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (id, label, n, mark) in &rows {
                        let (slot, _) =
                            ui.allocate_exact_size(vec2(list.width() - 4.0, key_h + 9.6), egui::Sense::hover());
                        let rect = Rect::from_min_size(slot.min, vec2(slot.width() - 4.0, key_h));
                        let face = RailKey {
                            id: ("dm-map", id.as_deref().unwrap_or("")),
                            mark: *mark,
                            name: label,
                            detail: "",
                            right: &n.to_string(),
                            pressed: self.map == *id,
                            name_width: 0.0,
                        }
                        .show(ui, rect);
                        if face.clicked {
                            picked = Some(id.clone());
                        }
                    }
                });
        });
        if let Some(id) = picked {
            let next = if self.map == id { None } else { id };
            self.set_map(next, client);
        }
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reel_lengths_take_a_third_at_most() {
        assert!(reel_lengths(2.9 * 60_000.0).is_empty());
        assert_eq!(reel_lengths(3.0 * 60_000.0), vec![1]);
        assert_eq!(reel_lengths(8.0 * 60_000.0), vec![1, 2]);
        assert_eq!(reel_lengths(9.5 * 60_000.0), vec![1, 2, 3]);
    }

    #[test]
    fn page_keys_keep_the_ends_and_the_neighbours() {
        assert_eq!(page_keys(1, 3), vec![1, 2, 3]);
        assert_eq!(page_keys(5, 10), vec![1, 2, 4, 5, 6, 9, 10]);
        assert_eq!(page_keys(1, 10), vec![1, 2, 9, 10]);
    }

    #[test]
    fn ago_reads_as_the_browser_writes_it() {
        let now = 1_800_000_000_000.0;
        let min = 60_000.0;
        assert_eq!(ago(now - 0.2 * min, true, now), "1m ago");
        assert_eq!(ago(now - 24.0 * min, false, now), "24 min ago");
        assert_eq!(ago(now - 90.0 * min, true, now), "2h ago");
        assert_eq!(ago(now - 26.0 * 60.0 * min, false, now), "Yesterday");
        assert_eq!(ago(now - 3.0 * 24.0 * 60.0 * min, true, now), "3d ago");
        assert_eq!(ago(now - 3.0 * 24.0 * 60.0 * min, false, now), "3 days ago");
    }

    #[test]
    fn a_list_lands_only_on_its_request() {
        let mut view = DemosView::new("ws://localhost:9000");
        view.connected = true;
        view.archive_request = Some(("d1".into(), Instant::now()));
        let demo = json!({"filename":"a.shagdemo","map":"dm1","mapDisplayName":"Dusk","gametype":"deathmatch",
            "players":["A","B"],"durationMs":600000,"date":1,"sizeMB":2.5});
        view.on_message(
            "demos:list_response",
            &json!({"requestId":"d0","scope":"latest","demos":[demo]}),
        );
        assert!(!view.archive_arrived);
        view.on_message(
            "demos:list_response",
            &json!({"requestId":"d1","scope":"latest","demos":[demo]}),
        );
        assert!(view.archive_arrived);
        assert_eq!(view.archive[0].mode(), "ffa");
        assert_eq!(view.archive[0].map_name(), "Dusk");
        assert_eq!(view.archive[0].size_text(), "2.5 MB");
    }

    #[test]
    fn the_page_draws_every_tab() {
        let ctx = egui::Context::default();
        crate::client::washoi_style::install_style(&ctx);
        let mut client = LobbyClient::connect("ws://127.0.0.1:9");
        let mut shots = Levelshots::new("ws://127.0.0.1:9");
        let mut view = DemosView::new("ws://127.0.0.1:9");
        let demo = |players: &[&str]| DemoInfo {
            filename: format!("{}.shagdemo", players.len()),
            map: "dm1".into(),
            map_display_name: "Dusk".into(),
            gametype: "duel".into(),
            players: players.iter().map(|s| (*s).to_owned()).collect(),
            duration_ms: 600_000.0,
            date: now_ms() - 60_000.0,
            size_mb: 3.0,
            levelshot: None,
        };
        let demos = vec![demo(&["A", "B"]), demo(&["A", "B", "C"])];
        view.seed(
            demos.clone(),
            demos.iter().map(|d| (d.clone(), String::new())).collect(),
        );
        for board in ["featured", "latest"] {
            view.open_board(board, false);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 900.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let r = Rect::from_min_max(pos2(400.0, 30.0), pos2(1550.0, 840.0));
                assert!(view.ui(ui, r, &mut client, &mut shots).is_none());
                view.rail(
                    ui,
                    Rect::from_min_max(pos2(0.0, 450.0), pos2(400.0, 740.0)),
                    400.0,
                    &mut client,
                );
            });
            output.textures_delta.clear();
            assert!(output.shapes.len() > 20);
        }
        client.disconnect();
    }
}
