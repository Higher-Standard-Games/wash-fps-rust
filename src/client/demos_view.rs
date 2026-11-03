//! wash-fps `client/DemosView.ts`: this week's featured matches, the latest
//! recordings, and yours, filtered by mode, map and a search, six to a page.
//! Everything shown is in the recording's header (map, mode, who played, how
//! long, when, how big); a header has no score, so a card has none either.
//!
//! Play downloads the recording ([`super::demo_download`]) and hands it to
//! the demo player. Not yet: the highlight reels (milestone 12) and the
//! level shots.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::ui::egui_layer::egui;

use crate::client::demo_download::{DemoDownload, DemoDownloadState};
use crate::client::lobby_panel::GAME_MODES;

/// `DEMOS_PER_PAGE`: three across and two down.
pub const DEMOS_PER_PAGE: usize = 6;
/// `DEMO_LIST_LIMIT`: how many recordings the lobby sends at once.
pub const DEMO_LIST_LIMIT: u32 = 64;
/// `DEMO_ARCHIVE_TIMEOUT_MS`.
pub const DEMO_ARCHIVE_TIMEOUT: Duration = Duration::from_secs(10);
const FEATURED_LIMIT: u32 = 6;
/// The narrowest card before the grid drops a column.
const CARD_MIN_WIDTH: f32 = 220.0;

/// `reelLengths`: the reel lengths a demo can make, at most a third of it.
#[must_use]
pub fn reel_lengths(duration_ms: f64) -> Vec<u32> {
    [1, 2, 3]
        .into_iter()
        .filter(|m| duration_ms >= f64::from(*m) * 3.0 * 60_000.0)
        .collect()
}

/// `DemoInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoInfo {
    pub private: bool,
    pub filename: String,
    pub map: String,
    pub map_display_name: String,
    pub gametype: String,
    pub players: Vec<String>,
    pub duration_ms: f64,
    pub date: f64,
    pub size_mb: f64,
}

impl DemoInfo {
    #[must_use]
    pub fn from_json(v: &Value) -> Option<Self> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let filename = v.get("filename")?.as_str()?.to_owned();
        Some(Self {
            private: v.get("private").and_then(Value::as_bool).unwrap_or(false),
            filename,
            map: text("map"),
            map_display_name: text("mapDisplayName"),
            gametype: text("gametype"),
            players: v
                .get("players")
                .and_then(Value::as_array)
                .map(|p| p.iter().filter_map(|n| n.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            duration_ms: v.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0),
            date: v.get("date").and_then(Value::as_f64).unwrap_or(0.0),
            size_mb: v.get("sizeMB").and_then(Value::as_f64).unwrap_or(0.0),
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
}

/// `demoMode`: deathmatch recordings are free for all.
#[must_use]
pub fn demo_mode(demo: &DemoInfo) -> &str {
    if demo.gametype.eq_ignore_ascii_case("deathmatch") {
        "ffa"
    } else {
        &demo.gametype
    }
}

/// `clock`.
#[must_use]
pub fn clock(ms: f64) -> String {
    let seconds = (ms / 1000.0).floor() as i64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// `ago`: short for a card, longer for the featured match. Older than a
/// week is the day and month.
#[must_use]
pub fn ago(date: f64, short: bool, now: f64) -> String {
    let minutes = (now - date) / 60_000.0;
    if minutes < 60.0 {
        return format!(
            "{}{}",
            minutes.round().max(1.0),
            if short { "m ago" } else { " min ago" }
        );
    }
    if minutes < 24.0 * 60.0 {
        return format!("{}{}", (minutes / 60.0).round(), if short { "h ago" } else { " h ago" });
    }
    let days = (minutes / (24.0 * 60.0)).floor();
    if short && days < 7.0 {
        return format!("{days}d ago");
    }
    if days == 1.0 {
        return "Yesterday".to_owned();
    }
    if days < 7.0 {
        return format!("{days} days ago");
    }
    // `toLocaleDateString(undefined, { day: "numeric", month: "short" })`.
    let day = (date / 86_400_000.0).floor() as i64;
    let (_, month, d) = civil_from_days(day);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{d} {}", MONTHS[(month - 1) as usize])
}

/// Days since 1970 to a UTC date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `pageKeys`: all pages when few, else the ends and the neighbours.
#[must_use]
pub fn page_keys(page: usize, pages: usize) -> Vec<usize> {
    if pages <= 7 {
        return (1..=pages).collect();
    }
    let mut keys: Vec<usize> = [1, 2, page.saturating_sub(1), page, page + 1, pages - 1, pages]
        .into_iter()
        .filter(|n| *n >= 1 && *n <= pages)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// `Tab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoTab {
    Featured,
    Latest,
    Yours,
}

/// `TABS`.
const TABS: [(DemoTab, &str, &str); 3] = [
    (DemoTab::Featured, "Featured", "This week's best"),
    (DemoTab::Latest, "Latest", "Every recorded match"),
    (DemoTab::Yours, "Yours", "Matches you played"),
];

/// A featured pick: the demo, and why it was picked.
#[derive(Debug, Clone)]
pub struct FeaturedDemo {
    pub demo: DemoInfo,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
enum FeaturedState {
    Idle,
    Loading,
    Ready,
    Failed(String),
}

/// `DemosView`.
#[derive(Debug)]
pub struct DemosView {
    pub tab: DemoTab,
    /// `"all"` or a mode id.
    mode: &'static str,
    map: Option<String>,
    query: String,
    page: usize,
    archive: Vec<DemoInfo>,
    archive_yours: bool,
    archive_request: Option<(String, Instant)>,
    archive_arrived: bool,
    archive_failed: bool,
    ownership: bool,
    featured_supported: bool,
    featured: Vec<FeaturedDemo>,
    featured_state: FeaturedState,
    featured_request: Option<String>,
    /// The recording being fetched.
    pub download: Option<DemoDownload>,
    /// A recording ready to play, for the menu to take.
    pub ready: Option<std::path::PathBuf>,
    /// Where recordings are fetched from.
    pub base: String,
}

impl DemosView {
    #[must_use]
    pub fn new(base: String) -> Self {
        Self {
            tab: DemoTab::Featured,
            mode: "all",
            map: None,
            query: String::new(),
            page: 1,
            archive: Vec::new(),
            archive_yours: false,
            archive_request: None,
            archive_arrived: false,
            archive_failed: false,
            ownership: false,
            featured_supported: false,
            featured: Vec::new(),
            featured_state: FeaturedState::Idle,
            featured_request: None,
            download: None,
            ready: None,
            base,
        }
    }

    /// `connected`: what the lobby's `welcome` says it can do.
    pub fn connected(&mut self, ownership: bool, featured: bool) {
        self.ownership = ownership;
        self.featured_supported = featured;
        if !ownership && self.tab == DemoTab::Yours {
            self.tab = DemoTab::Latest;
        }
    }

    /// The page opened: fetch what it shows.
    pub fn open(&mut self, client: &mut LobbyClient) {
        self.request_archive(client, false);
        if self.tab == DemoTab::Featured {
            self.request_featured(client);
        }
    }

    fn request_archive(&mut self, client: &mut LobbyClient, force: bool) {
        let yours = self.tab == DemoTab::Yours;
        let mut force = force;
        if yours != self.archive_yours {
            self.archive_yours = yours;
            self.archive.clear();
            self.archive_arrived = false;
            force = true;
        }
        if (self.archive_request.is_some() || self.archive_arrived) && !force {
            return;
        }
        self.archive_failed = false;
        let id = client.request_demo_list(DEMO_LIST_LIMIT, yours);
        self.archive_request = Some((id, Instant::now() + DEMO_ARCHIVE_TIMEOUT));
    }

    fn request_featured(&mut self, client: &mut LobbyClient) {
        if !self.featured_supported || matches!(self.featured_state, FeaturedState::Loading | FeaturedState::Ready) {
            return;
        }
        self.featured_state = FeaturedState::Loading;
        let map = self.map.clone().unwrap_or_else(|| "all".to_owned());
        self.featured_request = Some(client.request_featured_demos(self.mode, &map, FEATURED_LIMIT));
    }

    fn refetch_featured(&mut self, client: &mut LobbyClient) {
        self.featured_state = FeaturedState::Idle;
        self.featured.clear();
        self.featured_request = None;
        if self.tab == DemoTab::Featured {
            self.request_featured(client);
        }
    }

    /// `receiveList` and the featured answers.
    pub fn on_message(&mut self, kind: &str, message: &Value) {
        let id = message.get("requestId").and_then(Value::as_str);
        match kind {
            "demos:list_response" => {
                let Some((wanted, _)) = &self.archive_request else {
                    return;
                };
                if id.is_some_and(|i| i != wanted) {
                    return;
                }
                self.archive_request = None;
                self.archive_arrived = true;
                self.archive = message
                    .get("demos")
                    .and_then(Value::as_array)
                    .map(|d| d.iter().filter_map(DemoInfo::from_json).collect())
                    .unwrap_or_default();
            }
            "featured:page" if id.is_some() && id == self.featured_request.as_deref() => {
                self.featured_request = None;
                self.featured_state = FeaturedState::Ready;
                self.featured = message
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|e| {
                        e.iter()
                            .filter_map(|f| {
                                Some(FeaturedDemo {
                                    demo: DemoInfo::from_json(f.get("demo")?)?,
                                    reason: f.get("reason").and_then(Value::as_str).unwrap_or("").to_owned(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            "featured:error" if id.is_some() && id == self.featured_request.as_deref() => {
                self.featured_request = None;
                let reason = message.get("reason").and_then(Value::as_str).unwrap_or("unavailable");
                self.featured_state = FeaturedState::Failed(format!("Featured demos: {reason}"));
            }
            _ => {}
        }
    }

    /// The archive's deadline and the download's progress.
    pub fn tick(&mut self) {
        if let Some((_, deadline)) = &self.archive_request
            && Instant::now() >= *deadline
        {
            self.archive_request = None;
            self.archive_failed = true;
        }
        if let Some(download) = &mut self.download {
            download.poll();
            if let DemoDownloadState::Ready(path) = &download.state {
                self.ready = Some(path.clone());
                self.download = None;
            }
        }
    }

    /// The Latest tab, fetched.
    pub fn show_latest(&mut self, client: &mut LobbyClient) {
        if self.tab != DemoTab::Latest {
            self.tab = DemoTab::Latest;
            self.page = 1;
            self.request_archive(client, false);
        }
    }

    /// Play on the newest recording shown; false until one is listed.
    pub fn play_newest(&mut self) -> bool {
        let Some(demo) = self.shown_list().first().map(|d| (*d).clone()) else {
            return false;
        };
        self.play(&demo);
        true
    }

    fn in_mode(&self) -> impl Iterator<Item = &DemoInfo> {
        self.archive
            .iter()
            .filter(|d| self.mode == "all" || demo_mode(d) == self.mode)
    }

    /// `shownList`.
    fn shown_list(&self) -> Vec<&DemoInfo> {
        let query = self.query.trim().to_lowercase();
        let mut list: Vec<&DemoInfo> = self
            .in_mode()
            .filter(|d| self.map.as_ref().is_none_or(|m| *m == d.map))
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

    fn rule_text(&self) -> String {
        match self.tab {
            DemoTab::Latest => format!("Newest first · the latest {DEMO_LIST_LIMIT} recordings"),
            DemoTab::Yours => "Matches you played on a verified seat, newest first".to_owned(),
            DemoTab::Featured if self.featured_state != FeaturedState::Ready || self.featured.is_empty() => {
                "Recordings from the last 7 days".to_owned()
            }
            DemoTab::Featured if self.featured.iter().any(|f| f.reason.contains("top 10")) => {
                "This week's matches with players from the 30-day top 10".to_owned()
            }
            DemoTab::Featured => "No top-10 matches this week · the latest multiplayer matches instead".to_owned(),
        }
    }

    fn mode_title(&self, demo: &DemoInfo) -> String {
        let id = demo_mode(demo);
        GAME_MODES
            .iter()
            .find(|m| m.id == id)
            .map_or_else(|| id.to_uppercase(), |m| m.title.to_owned())
    }

    fn play(&mut self, demo: &DemoInfo) {
        if self.download.is_some() {
            return;
        }
        self.download = Some(DemoDownload::start(&self.base, &demo.filename));
    }

    /// A card: who, where, how long, when, how big, and Play.
    fn card(&mut self, ui: &mut egui::Ui, demo: &DemoInfo, featured: bool, now: f64, width: f32) {
        egui::Frame::group(ui.style()).inner_margin(10.0).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.strong(self.mode_title(demo));
                    if featured {
                        ui.label("Featured");
                    }
                    if demo.private {
                        ui.weak("Private");
                    }
                });
                ui.label(egui::RichText::new(demo.map_name()).size(22.0).strong());
                let who = if demo.players.len() == 2 {
                    format!("{} VS {}", demo.players[0], demo.players[1])
                } else {
                    format!("{} players · {}", demo.players.len(), demo.players.join(", "))
                };
                ui.label(who);
                ui.weak(format!(
                    "{} · {} · {} MB",
                    clock(demo.duration_ms),
                    ago(demo.date, true, now),
                    demo.size_mb
                ));
                ui.horizontal(|ui| {
                    let busy = self.download.is_some();
                    if ui.add_enabled(!busy, egui::Button::new("Play")).clicked() {
                        self.play(demo);
                    }
                    let reels = reel_lengths(demo.duration_ms);
                    let tip = if reels.is_empty() {
                        "Too short for a highlight reel"
                    } else {
                        "Highlight reels come with the demo player's reel cut"
                    };
                    ui.add_enabled(false, egui::Button::new("Highlights"))
                        .on_disabled_hover_text(tip);
                });
            });
        });
    }

    fn cards(&mut self, ui: &mut egui::Ui, demos: &[DemoInfo], featured: bool, now: f64) {
        // Three across when the page is wide enough, as the browser's grid;
        // each card's frame adds its margin (10) and stroke (1) on both sides.
        let gap = ui.spacing().item_spacing.x;
        let available = ui.available_width();
        let columns = (((available + gap) / (CARD_MIN_WIDTH + 22.0 + gap)) as usize).clamp(1, 3);
        let width = ((available - gap * (columns - 1) as f32) / columns as f32 - 22.0).max(CARD_MIN_WIDTH);
        for row in demos.chunks(columns) {
            ui.horizontal(|ui| {
                for demo in row {
                    self.card(ui, demo, featured, now, width);
                }
            });
        }
    }

    /// The page; `connected` is the lobby's state.
    pub fn ui(&mut self, ui: &mut egui::Ui, client: &mut LobbyClient, connected: bool) {
        let now = now_ms();
        let map_label = self.map.as_ref().map(|m| {
            self.archive
                .iter()
                .find(|d| d.map == *m)
                .map_or(m.clone(), |d| d.map_name().to_owned())
        });
        let mode_label = if self.mode == "all" {
            "All modes".to_owned()
        } else {
            GAME_MODES
                .iter()
                .find(|m| m.id == self.mode)
                .map_or("", |m| m.title)
                .to_owned()
        };
        let kicker: Vec<String> = ["Demos".to_owned(), mode_label].into_iter().chain(map_label).collect();
        ui.weak(kicker.join(" · "));
        ui.horizontal(|ui| {
            for (tab, name, sub) in TABS {
                let enabled = tab != DemoTab::Yours || self.ownership;
                let r = ui
                    .add_enabled(enabled, egui::Button::selectable(self.tab == tab, name))
                    .on_hover_text(sub)
                    .on_disabled_hover_text("This lobby server cannot prove which recordings are yours.");
                if r.clicked() && self.tab != tab {
                    self.tab = tab;
                    self.page = 1;
                    self.request_archive(client, false);
                    if tab == DemoTab::Featured {
                        self.request_featured(client);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            let mut mode = self.mode;
            ui.selectable_value(&mut mode, "all", "All");
            for m in &GAME_MODES {
                ui.selectable_value(&mut mode, m.id, m.title);
            }
            if mode != self.mode {
                self.mode = mode;
                if self.map.as_ref().is_some_and(|m| !self.in_mode().any(|d| d.map == *m)) {
                    self.map = None;
                }
                self.page = 1;
                self.refetch_featured(client);
            }
            ui.separator();
            if ui
                .add(egui::TextEdit::singleline(&mut self.query).hint_text("Search maps and players"))
                .changed()
            {
                self.page = 1;
            }
        });
        // The rail's maps: each with how many recordings it has.
        let mut maps: Vec<(String, String, usize)> = Vec::new();
        for d in self.in_mode() {
            match maps.iter_mut().find(|m| m.0 == d.map) {
                Some(m) => m.2 += 1,
                None => maps.push((d.map.clone(), d.map_name().to_owned(), 1)),
            }
        }
        if !maps.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.weak(format!(
                    "{} {}",
                    maps.len(),
                    if maps.len() == 1 { "map" } else { "maps" }
                ));
                let mut map = self.map.clone();
                ui.selectable_value(&mut map, None, "Every map");
                for (id, name, count) in &maps {
                    ui.selectable_value(&mut map, Some(id.clone()), format!("{name} ({count})"));
                }
                if map != self.map {
                    self.map = map;
                    self.page = 1;
                    self.refetch_featured(client);
                }
            });
        }
        ui.weak(self.rule_text());
        self.transfer(ui);
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.tab == DemoTab::Featured {
                self.featured_pane(ui, client, connected, now);
            } else {
                self.grid_pane(ui, client, connected, now);
            }
        });
    }

    /// The download's line: loading, or why it failed with Try again.
    fn transfer(&mut self, ui: &mut egui::Ui) {
        let Some(download) = &mut self.download else { return };
        let mut cancel = false;
        let mut retry = None;
        ui.horizontal(|ui| match &download.state {
            DemoDownloadState::Loading { received, total } => {
                let text = match total {
                    Some(t) if *t > 0 => format!(
                        "Loading recording… {:.1} / {:.1} MB",
                        *received as f64 / 1e6,
                        *t as f64 / 1e6
                    ),
                    _ => "Loading recording…".to_owned(),
                };
                ui.label(text);
                cancel = ui.button("Cancel").clicked();
            }
            DemoDownloadState::Failed(e) => {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                if ui.button("Try again").clicked() {
                    retry = Some(download.filename.clone());
                }
                cancel = ui.button("Close").clicked();
            }
            DemoDownloadState::Ready(_) => {}
        });
        if cancel {
            download.cancel();
            self.download = None;
        } else if let Some(file) = retry {
            self.download = Some(DemoDownload::start(&self.base, &file));
        }
    }

    fn featured_pane(&mut self, ui: &mut egui::Ui, client: &mut LobbyClient, connected: bool, now: f64) {
        if self.featured_supported && self.featured_state == FeaturedState::Idle {
            self.request_featured(client);
        }
        let empty = if !connected {
            Some("Connect to the lobby to see this week's picks.".to_owned())
        } else if !self.featured_supported {
            Some("Featured picks are unavailable on this lobby server. Latest has every match.".to_owned())
        } else {
            match &self.featured_state {
                FeaturedState::Failed(e) => Some(e.clone()),
                FeaturedState::Ready if self.featured.is_empty() => {
                    let place = self.map.as_ref().map(|m| format!(" on {m}")).unwrap_or_default();
                    Some(format!(
                        "No multiplayer matches{place} this week yet. Latest has every match."
                    ))
                }
                FeaturedState::Ready => None,
                _ => Some("Loading this week's picks…".to_owned()),
            }
        };
        if let Some(text) = empty {
            ui.weak(text);
            return;
        }
        ui.weak(format!("{} picks", self.featured.len()));
        let demos: Vec<DemoInfo> = self.featured.iter().map(|f| f.demo.clone()).collect();
        self.cards(ui, &demos, true, now);
    }

    fn grid_pane(&mut self, ui: &mut egui::Ui, client: &mut LobbyClient, connected: bool, now: f64) {
        let list: Vec<DemoInfo> = self.shown_list().into_iter().cloned().collect();
        if list.is_empty() {
            let empty = if !connected {
                "Lobby server offline. Connect to browse recordings.".to_owned()
            } else if self.archive_failed {
                "The archive did not answer. Try again.".to_owned()
            } else if !self.archive_arrived {
                "Loading the archive…".to_owned()
            } else if !self.query.trim().is_empty() {
                format!("No demos with “{}” in them", self.query.trim())
            } else if !self.archive.is_empty() {
                "No demos for this mode and map.".to_owned()
            } else if self.tab == DemoTab::Yours {
                "No recordings of yours yet. A match you play lands here once it ends.".to_owned()
            } else {
                "No recorded demos yet. Play a match.".to_owned()
            };
            ui.weak(empty);
            if self.archive_failed && connected && ui.button("Try again").clicked() {
                self.request_archive(client, true);
            }
            return;
        }
        let pages = list.len().div_ceil(DEMOS_PER_PAGE);
        self.page = self.page.clamp(1, pages);
        let start = (self.page - 1) * DEMOS_PER_PAGE;
        let shown = &list[start..(start + DEMOS_PER_PAGE).min(list.len())];
        self.cards(ui, shown, false, now);
        if pages > 1 {
            ui.horizontal(|ui| {
                if ui.add_enabled(self.page > 1, egui::Button::new("« Prev")).clicked() {
                    self.page -= 1;
                }
                let keys = page_keys(self.page, pages);
                for (i, n) in keys.iter().enumerate() {
                    if i > 0 && n - keys[i - 1] > 1 {
                        ui.weak("…");
                    }
                    if ui.selectable_label(*n == self.page, n.to_string()).clicked() {
                        self.page = *n;
                    }
                }
                if ui.add_enabled(self.page < pages, egui::Button::new("Next »")).clicked() {
                    self.page += 1;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helpers_match_the_typescript() {
        assert_eq!(reel_lengths(2.0 * 60_000.0), Vec::<u32>::new());
        assert_eq!(reel_lengths(7.0 * 60_000.0), [1, 2]);
        assert_eq!(clock(125_000.0), "2:05");
        let now = 1_793_700_000_000.0;
        assert_eq!(ago(now - 30.0 * 60_000.0, true, now), "30m ago");
        assert_eq!(ago(now - 3.0 * 3_600_000.0, false, now), "3 h ago");
        assert_eq!(ago(now - 86_400_000.0 * 1.5, false, now), "Yesterday");
        assert_eq!(ago(now - 86_400_000.0 * 3.0, true, now), "3d ago");
        // 2026-10-03.
        assert_eq!(
            ago(1_791_000_000_000.0, true, 1_791_000_000_000.0 + 86_400_000.0 * 30.0),
            "3 Oct"
        );
        assert_eq!(page_keys(1, 5), [1, 2, 3, 4, 5]);
        assert_eq!(page_keys(5, 10), [1, 2, 4, 5, 6, 9, 10]);
        let demo = DemoInfo::from_json(&serde_json::json!({
            "filename": "a.shagdemo", "map": "washoi_dm2", "mapDisplayName": "", "gametype": "deathmatch",
            "players": ["x", "y"], "durationMs": 60000, "date": 0, "sizeMB": 1.5
        }))
        .unwrap();
        assert_eq!(demo_mode(&demo), "ffa");
        assert_eq!(demo.map_name(), "washoi_dm2");
    }
}
