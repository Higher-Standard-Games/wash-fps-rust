//! wash-fps `client/LeaderboardView.ts` and `styles/Leaderboard.css`, with
//! `NewUiApi.ts`'s `getLeaderboard` checks.
//!
//! The leaderboard: a podium and the rest of the top ten for the last 30
//! days or all time, or the whole top 100 with a search; your own rank in
//! every mode in the rail. Every row comes from the lobby's ranking query.
//! Nothing is shown the lobby does not know: it keeps wins, losses and
//! draws, so there is no K/D.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::ui::components::lobby_twin::{
    clamp_css, cream, elide, gradient_polygon, horizontal_gradient, hsl, hsla, ink, violet,
};
use shagger::ui::components::raised_key::{KeyLook, KeyText, KeyTextLayout, RaisedKey};
use shagger::ui::components::text_field::TextField;
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontId, Painter, Rect, Shape, Stroke, pos2, vec2};

use crate::client::lobby_page_parts::{
    ModeChip, RailKey, board_keys, flat_key, mode_keys, page_head, rail_label, rule, status,
};
use crate::client::lobby_panel::{GAME_MODES, GameMode};
use crate::client::settings::settings_fields::{body, cut_panel, gold_text, line};
use crate::client::washoi_style::display_family;

/// `RATING_POLICY.version` and `.displayScale`.
const POLICY_VERSION: &str = "shag-trueskill-v2";
const DISPLAY_SCALE: f64 = 40.0;
/// `requestLeaderboard`'s default wait.
const TIMEOUT: Duration = Duration::from_secs(10);

const UNAVAILABLE: &str = "Rankings are unavailable on this lobby server.";
const OFFLINE: &str = "The lobby server is offline. Rankings come back with it.";

/// A row of the board (`LeaderboardEntry`, as `getLeaderboard` lets it through).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub rank: u32,
    pub player_id: String,
    pub name: String,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub rating: i64,
    pub conservative_score: f64,
    pub provisional: bool,
    pub rated_matches: u32,
    pub bot: bool,
}

impl Entry {
    fn played(&self) -> u32 {
        self.wins + self.losses + self.draws
    }

    fn rating_text(&self) -> String {
        format!(
            "{}{}",
            self.rating,
            if self.provisional { " · Provisional" } else { "" }
        )
    }
}

const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

/// `Number.isSafeInteger`.
fn safe_int(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.fract() == 0.0 && v.abs() <= MAX_SAFE)
}

/// A row, when `valid`: a whole rating, a finite score, a provisional flag
/// and at least one rated match.
fn read_entry(v: &Value) -> Option<Entry> {
    let num = |k: &str| v.get(k).and_then(Value::as_f64);
    let count = |k: &str| v.get(k).and_then(Value::as_u64).map(|n| n as u32);
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let rating = safe_int(num("rating"))?;
    let rated = safe_int(num("ratedMatches")).filter(|&n| n > 0.0)?;
    Some(Entry {
        rank: count("rank")?,
        player_id: text("playerId")?,
        name: text("name")?,
        wins: count("wins")?,
        losses: count("losses")?,
        draws: count("draws")?,
        rating: rating as i64,
        conservative_score: num("conservativeScore").filter(|v| v.is_finite())?,
        provisional: v.get("provisional").and_then(Value::as_bool)?,
        rated_matches: rated as u32,
        bot: v.get("bot").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// `LeaderboardPage`.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub total: u32,
    pub local_player_entry: Option<Entry>,
    /// `localProcessing.excluded`.
    pub excluded: u32,
}

/// `getLeaderboard`'s checks on a `leaderboard:page`: the Overall pool,
/// TrueSkill under this policy, and every row whole.
pub fn read_page(message: &Value) -> Result<Page, String> {
    const WRONG: &str = "This lobby server did not return the requested ranking.";
    let scope = message.get("scope").and_then(Value::as_str);
    let system = message.get("system").and_then(Value::as_str);
    let policy = message.get("policyVersion").and_then(Value::as_str);
    if scope != Some("overall") || system != Some("trueskill") || policy != Some(POLICY_VERSION) {
        return Err(WRONG.into());
    }
    let entries = message
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(WRONG)?
        .iter()
        .map(read_entry)
        .collect::<Option<Vec<_>>>()
        .ok_or(WRONG)?;
    let local_player_entry = match message.get("localPlayerEntry") {
        None | Some(Value::Null) => None,
        Some(v) => Some(read_entry(v).ok_or(WRONG)?),
    };
    Ok(Page {
        entries,
        total: message.get("total").and_then(Value::as_u64).unwrap_or(0) as u32,
        local_player_entry,
        excluded: message
            .pointer("/localProcessing/excluded")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Period {
    Month,
    All,
}

impl Period {
    fn id(self) -> &'static str {
        match self {
            Self::Month => "month",
            Self::All => "all",
        }
    }

    /// `PERIOD_NAME`.
    fn name(self) -> &'static str {
        match self {
            Self::Month => "30 days",
            Self::All => "All time",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Month,
    All,
    List,
}

/// `TABS`.
const TABS: [(Tab, &str, &str); 3] = [
    (Tab::Month, "Last 30 days", "Rolling · never resets"),
    (Tab::All, "All time", "Since launch"),
    (Tab::List, "Top 100", "The full list"),
];

#[derive(Debug, Clone)]
enum Loaded {
    Loading { request: String, sent: Instant },
    Ready(Page),
    Failed(String),
}

/// `AVATAR_COLORS`, as HSL.
const AVATAR_COLORS: [(f32, f32, f32); 8] = [
    (182.0, 95.0, 62.0),
    (322.0, 95.0, 70.0),
    (125.0, 70.0, 62.0),
    (276.0, 90.0, 76.0),
    (28.0, 100.0, 64.0),
    (200.0, 90.0, 68.0),
    (350.0, 90.0, 70.0),
    (52.0, 100.0, 64.0),
];

/// `avatarColor`: a colour per name, the same in every client.
#[must_use]
pub fn avatar_color(name: &str) -> Color32 {
    let mut hash: u32 = 0;
    for ch in name.chars() {
        let mut unit = [0u16; 2];
        let first = ch.encode_utf16(&mut unit)[0];
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(first));
    }
    let (h, s, l) = AVATAR_COLORS[hash as usize % AVATAR_COLORS.len()];
    hsl(h, s, l)
}

/// `nextStep`: a measured skill gap, never a prediction of how many wins
/// it takes.
#[must_use]
pub fn next_step(entries: &[Entry], you: &Entry, limit: usize) -> String {
    if you.rank == 1 {
        return "On top. Hold it".into();
    }
    let target = if you.rank as usize <= limit {
        entries.iter().rev().find(|e| e.rank < you.rank)
    } else {
        entries.get(limit - 1)
    };
    let Some(target) = target else {
        return String::new();
    };
    let gap = ((target.conservative_score - you.conservative_score) * DISPLAY_SCALE)
        .ceil()
        .max(0.0) as i64;
    format!(
        "{gap} rating {} behind #{}",
        if gap == 1 { "point" } else { "points" },
        target.rank
    )
}

#[derive(Debug)]
pub struct LeaderboardView {
    mode: usize,
    tab: Tab,
    list_period: Period,
    query: String,
    boards: HashMap<(usize, Period), Loaded>,
    ranks: HashMap<(usize, Period), Loaded>,
    connected: bool,
    supported: bool,
    account_id: Option<String>,
    /// Jump to me was pressed: scroll to your row and flash it.
    jump: bool,
    flash_since: Option<f64>,
}

impl Default for LeaderboardView {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaderboardView {
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: 0,
            tab: Tab::Month,
            list_period: Period::Month,
            query: String::new(),
            boards: HashMap::new(),
            ranks: HashMap::new(),
            connected: false,
            supported: false,
            account_id: None,
            jump: false,
            flash_since: None,
        }
    }

    /// `bindLeaderboard`: the connection, whether the lobby ranks
    /// (`features.leaderboard` and `features.skillRatings`), and who is
    /// logged in. Forgets everything the last connection said.
    pub fn bind(&mut self, connected: bool, supported: bool, account_id: Option<String>) {
        self.connected = connected;
        self.supported = supported;
        self.account_id = account_id;
        self.clear();
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.boards.clear();
        self.ranks.clear();
    }

    /// `refresh`: the page came into view; ask again.
    pub fn refresh(&mut self, client: &mut LobbyClient) {
        self.clear();
        self.load(client);
    }

    fn availability(&self) -> Result<(), &'static str> {
        if !self.connected {
            Err(OFFLINE)
        } else if !self.supported {
            Err(UNAVAILABLE)
        } else {
            Ok(())
        }
    }

    fn period(&self) -> Period {
        match self.tab {
            Tab::Month => Period::Month,
            Tab::All => Period::All,
            Tab::List => self.list_period,
        }
    }

    /// `load`: the board and ranks missing from the selected ranking's cache.
    fn load(&mut self, client: &mut LobbyClient) {
        if self.availability().is_err() {
            return;
        }
        let period = self.period();
        let key = (self.mode, period);
        if !self.boards.contains_key(&key) {
            let request = client.request_leaderboard(GAME_MODES[self.mode].id, period.id(), 0, 100, None);
            self.boards.insert(
                key,
                Loaded::Loading {
                    request,
                    sent: Instant::now(),
                },
            );
        }
        for (i, mode) in GAME_MODES.iter().enumerate() {
            let key = (i, period);
            // A full board already carries your row.
            if !self.ranks.contains_key(&key) && !self.boards.contains_key(&key) {
                let request = client.request_leaderboard(mode.id, period.id(), 0, 1, None);
                self.ranks.insert(
                    key,
                    Loaded::Loading {
                        request,
                        sent: Instant::now(),
                    },
                );
            }
        }
    }

    /// Takes `leaderboard:page` and `leaderboard:error` for a request of ours.
    pub fn on_message(&mut self, kind: &str, message: &Value) {
        let Some(id) = message.get("requestId").and_then(Value::as_str) else {
            return;
        };
        let answer = match kind {
            "leaderboard:page" => read_page(message).map_or_else(Loaded::Failed, Loaded::Ready),
            "leaderboard:error" => Loaded::Failed(format!(
                "Leaderboard: {}",
                message.get("reason").and_then(Value::as_str).unwrap_or("error")
            )),
            _ => return,
        };
        for loaded in self.boards.values_mut().chain(self.ranks.values_mut()) {
            if matches!(loaded, Loaded::Loading { request, .. } if request == id) {
                *loaded = answer;
                return;
            }
        }
    }

    /// Opens a board by its id (`month`, `all` or `list`).
    pub fn open_board(&mut self, board: &str) {
        self.tab = match board {
            "all" => Tab::All,
            "list" => Tab::List,
            _ => Tab::Month,
        };
    }

    /// For screenshots: a page as if the lobby had sent it.
    pub fn seed(&mut self, mode: &str, period: &str, page: Page) {
        let Some(m) = GAME_MODES.iter().position(|g| g.id == mode) else {
            return;
        };
        let period = if period == "all" { Period::All } else { Period::Month };
        self.connected = true;
        self.supported = true;
        self.account_id = page.local_player_entry.as_ref().map(|e| e.player_id.clone());
        self.boards.insert((m, period), Loaded::Ready(page));
    }

    fn time_out(&mut self) {
        for loaded in self.boards.values_mut().chain(self.ranks.values_mut()) {
            if matches!(loaded, Loaded::Loading { sent, .. } if sent.elapsed() > TIMEOUT) {
                *loaded = Loaded::Failed("Leaderboard request timed out".into());
            }
        }
    }

    fn set_mode(&mut self, mode: usize, client: &mut LobbyClient) {
        self.mode = mode;
        self.load(client);
    }

    /// Your row in a mode and period, from a full board or a rank-only reply.
    fn yours(&self, mode: usize, period: Period) -> (Option<&Loaded>, Option<&Entry>) {
        let key = (mode, period);
        let loaded = self.boards.get(&key).or_else(|| self.ranks.get(&key));
        let you = match loaded {
            Some(Loaded::Ready(page)) => page
                .local_player_entry
                .as_ref()
                .filter(|e| self.account_id.as_deref() == Some(e.player_id.as_str())),
            _ => None,
        };
        (loaded, you)
    }

    /// The mode shown, whose colours the page takes.
    #[must_use]
    pub fn mode(&self) -> &'static GameMode {
        &GAME_MODES[self.mode]
    }

    /// The stage: head, boards, rule, status and the podium or the list.
    pub fn ui(&mut self, ui: &mut egui::Ui, rect: Rect, client: &mut LobbyClient) {
        self.time_out();
        let painter = ui.painter().clone();
        let mode = *self.mode();
        let accent = mode.accent().face;
        let tab = TABS.iter().find(|t| t.0 == self.tab).unwrap_or(&TABS[0]);
        let head = page_head(
            &painter,
            rect.left(),
            rect.top(),
            &format!("Leaderboard · {}", mode.title),
            tab.1,
            accent,
        );
        let chips: Vec<ModeChip<'_>> = GAME_MODES
            .iter()
            .map(|m| ModeChip {
                id: m.id,
                title: m.title,
                mark: hsl(m.mark.0, m.mark.1, m.mark.2),
            })
            .collect();
        // `lb-filters`: beside the title when there is room, else under it.
        let keys_w = chips.len() as f32 * 110.0 - 10.0;
        let beside = rect.right() - keys_w - 24.0 >= head.right();
        let keys_bottom = if beside {
            head.bottom()
        } else {
            head.bottom() + 16.0 + 52.0
        };
        let (picked, _) = mode_keys(ui, "lb-mode", rect.right(), keys_bottom, &chips, self.mode);
        if let Some(m) = picked {
            self.set_mode(m, client);
        }
        let mut y = keys_bottom.max(head.bottom()) + 18.0;
        let boards: Vec<(&str, &str)> = TABS.iter().map(|t| (t.1, t.2)).collect();
        let open = TABS.iter().position(|t| t.0 == self.tab).unwrap_or(0);
        let (picked, bottom) = board_keys(ui, "lb-board", rect.left(), y, rect.width(), &boards, open);
        if let Some(i) = picked {
            self.tab = TABS[i].0;
            self.load(client);
        }
        y = bottom + 18.0 - 10.6;

        // `renderBoard`: rows from a lobby that can no longer rank are not
        // shown as if it could.
        let period = self.period();
        let availability = self.availability();
        let loaded = self.boards.get(&(self.mode, period)).cloned();
        let page = match (&availability, &loaded) {
            (Ok(()), Some(Loaded::Ready(page))) => Some(page.clone()),
            _ => None,
        };
        let ranked = page
            .as_ref()
            .filter(|p| p.total > 0)
            .map(|p| format!(" · {} ranked", p.total));
        y = rule(
            &painter,
            rect.left(),
            y,
            &format!(
                "Career skill rating · provisional ratings include uncertainty{}",
                ranked.unwrap_or_default()
            ),
        ) + 18.0;
        let mut message = match (&availability, &loaded, &page) {
            (Err(why), _, _) => (*why).to_owned(),
            (_, Some(Loaded::Failed(why)), _) => why.clone(),
            (_, _, None) => "Loading rankings…".to_owned(),
            (_, _, Some(p)) if p.total == 0 => format!(
                "No ranked {} matches {}.",
                mode.title,
                if period == Period::Month {
                    "in the last 30 days"
                } else {
                    "yet"
                }
            ),
            _ => String::new(),
        };
        if let Some(p) = page.as_ref().filter(|p| p.excluded > 0) {
            if !message.is_empty() {
                message.push(' ');
            }
            message.push_str(&format!(
                "{} of your matches did not qualify for skill rating.",
                p.excluded
            ));
        }
        if !message.is_empty() {
            y = status(&painter, rect.left(), y, rect.width(), &message) + 18.0;
        }

        let entries = page.as_ref().map(|p| p.entries.clone()).unwrap_or_default();
        let you = page.as_ref().and(self.yours(self.mode, period).1.cloned());
        let pane = Rect::from_min_max(pos2(rect.left(), y), rect.max);
        if self.tab == Tab::List {
            self.list(
                ui,
                pane,
                &entries,
                you.as_ref(),
                page.as_ref().is_some_and(|p| p.total > 0),
                client,
            );
        } else {
            self.podium(ui, pane, &entries, you.as_ref());
        }
    }

    /// `renderPodium`: the top three, then the rest of the top ten, and you
    /// under them when you are further down.
    fn podium(&mut self, ui: &mut egui::Ui, pane: Rect, entries: &[Entry], you: Option<&Entry>) {
        let painter = ui.painter().clone();
        let mut y = pane.top();
        let top: Vec<&Entry> = entries.iter().take(3).collect();
        if !top.is_empty() {
            y = self.plinths(&painter, pane, &top) + 18.0;
        }
        let rest: Vec<Entry> = entries.iter().skip(3).take(7).cloned().collect();
        let pinned = you.filter(|y| y.rank > 10);
        if rest.is_empty() && pinned.is_none() {
            return;
        }
        let pin = pinned.map(|you| (you.clone(), next_step(entries, you, 10)));
        let board = Rect::from_min_max(pos2(pane.left(), y), pane.max);
        self.board(ui, board, &rest, pin, false, "lb-top");
    }

    /// `lb-podium`: second, first and third, first standing tallest.
    /// Returns the bottom.
    fn plinths(&self, painter: &Painter, pane: Rect, top: &[&Entry]) -> f32 {
        let screen = painter.ctx().content_rect();
        let short = screen.height() <= 960.0;
        let tight = screen.width() <= 1200.0;
        let gap = if tight { 18.0 } else { 28.0 };
        let inner = pane.width() - 12.0;
        let unit = (inner - 2.0 * gap) / 3.12;
        let widths = [unit, unit * 1.12, unit];
        let order: Vec<&Entry> = [top.get(1), top.first(), top.get(2)]
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let faces: Vec<(f32, f32, Vec<Line>)> = order
            .iter()
            .map(|e| {
                let col = if e.rank == 1 {
                    1
                } else if e.rank == 2 {
                    0
                } else {
                    2
                };
                let w = widths[col];
                let lines = self.plinth_lines(painter, e, w, tight);
                (col as f32, w, lines)
            })
            .collect();
        let height_of = |e: &Entry, lines: &[Line]| {
            let pad = if short { 14.0 + 16.0 } else { 20.0 + 22.0 };
            let content: f32 = lines.iter().map(|l| l.height).sum::<f32>() + 4.0 * (lines.len() as f32 - 1.0);
            let min = match (e.rank, short) {
                (1, false) => 204.0,
                (1, true) => 164.0,
                (_, false) => 158.0,
                (_, true) => 128.0,
            };
            (content + pad).max(min)
        };
        let tallest = order
            .iter()
            .zip(&faces)
            .map(|(e, f)| height_of(e, &f.2))
            .fold(0.0, f32::max);
        let base = pane.top() + 4.0 + tallest;
        for (e, (col, w, lines)) in order.iter().zip(faces) {
            let x = pane.left() + (0..col as usize).map(|c| widths[c] + gap).sum::<f32>();
            let h = height_of(e, &lines);
            let r = Rect::from_min_size(pos2(x, base - h), vec2(w, h));
            self.plinth(painter, r, e, &lines, short, tight);
        }
        base + 12.0
    }

    fn plinth_lines(&self, painter: &Painter, e: &Entry, width: f32, tight: bool) -> Vec<Line> {
        let vw = painter.ctx().content_rect().width();
        let text_w = width - if tight { 22.0 + 18.0 } else { 34.0 + 30.0 } - rank_size(e.rank, vw, tight) * 0.7 - 20.0;
        let ink_c = ink();
        let mut lines = Vec::new();
        let is_you = self.account_id.as_deref() == Some(e.player_id.as_str());
        if e.rank == 1 {
            let tag = if self.tab == Tab::Month { "Leading" } else { "Champion" };
            lines.push(Line::tag(painter, tag, false));
        }
        if is_you {
            lines.push(Line::tag(painter, "You", false));
        }
        if e.bot {
            lines.push(Line::tag(painter, "Bot", true));
        }
        let name_size = if tight { 17.0 } else { clamp_css(20.0, vw * 0.017, 30.0) };
        let name = elide(painter, &e.name, name_size, display_family(), text_w);
        lines.push(Line::text(line(
            painter,
            &name,
            name_size,
            display_family(),
            ink_c,
            0.02,
        )));
        let wins_size = if tight { 22.0 } else { clamp_css(24.0, vw * 0.02, 34.0) };
        let rating = line(painter, &e.rating.to_string(), wins_size, display_family(), ink_c, 0.0);
        let small = line(painter, "RATING", 14.0, body(), ink_c.gamma_multiply(0.7), 0.14);
        lines.push(Line {
            height: rating.size().y,
            kind: LineKind::Rating(rating, small),
        });
        let count = e.played();
        let meta = format!(
            "{}{} rated · {count} {} · {} W / {} L / {} D",
            if e.provisional { "Provisional · " } else { "" },
            e.rated_matches,
            if count == 1 { "match" } else { "matches" },
            e.wins,
            e.losses,
            e.draws
        );
        let meta_c = hsla(250.0, 45.0, 8.0, 0.66);
        let size = if tight { 12.0 } else { 14.0 };
        let g = spaced_wrap(painter, &meta, size, meta_c, 0.06, text_w);
        lines.push(Line::text(g));
        lines
    }

    fn plinth(&self, painter: &Painter, r: Rect, e: &Entry, lines: &[Line], short: bool, tight: bool) {
        let skew = |r: Rect| {
            let c = r.width() * 0.03;
            vec![
                pos2(r.left() + c, r.top()),
                r.right_top(),
                pos2(r.right() - c, r.bottom()),
                r.left_bottom(),
            ]
        };
        let (face, side): (Vec<(f32, Color32)>, [Color32; 2]) = match e.rank {
            1 => (
                vec![
                    (0.0, hsl(51.0, 100.0, 74.0)),
                    (0.55, hsl(46.0, 100.0, 58.0)),
                    (1.0, hsl(42.0, 100.0, 50.0)),
                ],
                [hsl(36.0, 92.0, 40.0), hsl(28.0, 88.0, 25.0)],
            ),
            2 => (
                vec![
                    (0.0, hsl(220.0, 30.0, 96.0)),
                    (0.6, hsl(222.0, 16.0, 80.0)),
                    (1.0, hsl(224.0, 12.0, 68.0)),
                ],
                [hsl(225.0, 12.0, 46.0), hsl(228.0, 14.0, 30.0)],
            ),
            _ => (
                vec![
                    (0.0, hsl(30.0, 90.0, 74.0)),
                    (0.6, hsl(24.0, 72.0, 56.0)),
                    (1.0, hsl(20.0, 66.0, 46.0)),
                ],
                [hsl(18.0, 62.0, 32.0), hsl(14.0, 60.0, 20.0)],
            ),
        };
        // The drop shadow, then the side, then the face.
        painter.add(Shape::convex_polygon(
            skew(r.translate(vec2(14.0, 18.0)).expand(4.0)),
            hsla(250.0, 70.0, 2.0, 0.3),
            Stroke::NONE,
        ));
        gradient_polygon(
            painter,
            &skew(r.translate(vec2(9.0, 11.0))),
            &[(0.0, side[0]), (1.0, side[1])],
        );
        let face_pts = skew(r);
        gradient_polygon(painter, &face_pts, &face);
        painter.line_segment(
            [face_pts[0], face_pts[1]],
            Stroke::new(2.0, Color32::from_white_alpha(153)),
        );
        if self.account_id.as_deref() == Some(e.player_id.as_str()) {
            let inset = skew(r.shrink(2.0));
            painter.add(Shape::closed_line(inset, Stroke::new(4.0, ink())));
        }
        let (pad_l, pad_r) = if tight { (22.0, 18.0) } else { (34.0, 30.0) };
        let _ = pad_r;
        let vw = painter.ctx().content_rect().width();
        let size = rank_size(e.rank, vw, tight);
        let rank = line(painter, &e.rank.to_string(), size, display_family(), ink(), -0.04);
        let rank_w = rank.size().x;
        painter.galley(pos2(r.left() + pad_l, r.center().y - rank.size().y / 2.0), rank, ink());
        let x = r.left() + pad_l + rank_w + if tight { 12.0 } else { 20.0 };
        let total: f32 = lines.iter().map(|l| l.height).sum::<f32>() + 4.0 * (lines.len() as f32 - 1.0);
        let _ = short;
        let mut y = r.center().y - total / 2.0;
        for l in lines {
            l.paint(painter, pos2(x, y));
            y += l.height + 4.0;
        }
    }

    /// `lb-board`: the head row, the rows (scrolling) and you pinned under
    /// them. `full` fills `rect`; otherwise the board is as tall as its rows.
    fn board(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        rows: &[Entry],
        pin: Option<(Entry, String)>,
        full: bool,
        id: &str,
    ) {
        let short = ui.ctx().content_rect().height() <= 960.0;
        let row_h = if short { 38.0 } else { 44.0 };
        let head_h = 36.0;
        let pin_h = if pin.is_some() { row_h + 1.5 } else { 0.0 };
        let natural = head_h + rows.len() as f32 * row_h + pin_h;
        let h = if full {
            rect.height()
        } else {
            natural.min(rect.height())
        };
        if h < head_h + 4.0 {
            return;
        }
        let board = Rect::from_min_size(rect.min, vec2(rect.width(), h));
        let painter = ui.painter().clone();
        cut_panel(
            &painter,
            board,
            20.0,
            &[(0.0, hsla(250.0, 40.0, 11.0, 0.8)), (1.0, hsla(250.0, 44.0, 7.0, 0.88))],
            Some(Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.28))),
        );
        let cols = Columns::new(board.width(), ui.ctx().content_rect().width());
        let head = Rect::from_min_size(board.min, vec2(board.width(), head_h));
        self.head_row(&painter, head, &cols);
        let list = Rect::from_min_max(
            pos2(board.left(), head.bottom()),
            pos2(board.right(), board.bottom() - pin_h),
        );
        let now = ui.input(|i| i.time);
        let mut jump = std::mem::take(&mut self.jump);
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for e in rows {
                        let (r, _) = ui.allocate_exact_size(vec2(list.width(), row_h), egui::Sense::hover());
                        let is_you = self.is_you(e);
                        if is_you && jump {
                            ui.scroll_to_rect(r, Some(egui::Align::Center));
                            self.flash_since = Some(now);
                            jump = false;
                        }
                        let flash = if is_you { self.flash(now) } else { 0.0 };
                        self.row(ui.painter(), r, e, None, &cols, flash, true);
                    }
                    if rows.is_empty() && !self.query.trim().is_empty() {
                        let c = hsla(45.0, 100.0, 95.0, 0.5);
                        let g = line(
                            ui.painter(),
                            &format!("Nobody in the top 100 goes by “{}”", self.query.trim()),
                            16.0,
                            body(),
                            c,
                            0.0,
                        );
                        let (r, _) =
                            ui.allocate_exact_size(vec2(list.width(), g.size().y + 52.0), egui::Sense::hover());
                        ui.painter().galley(pos2(r.left() + 22.0, r.top() + 26.0), g, c);
                    }
                });
        });
        if self.flash_since.is_some_and(|t| now - t < 1.4) {
            ui.ctx().request_repaint();
        }
        if let Some((you, note)) = pin {
            let r = Rect::from_min_max(pos2(board.left(), board.bottom() - row_h), board.max);
            painter.hline(
                board.x_range(),
                r.top() - 0.75,
                Stroke::new(1.5, hsla(46.0, 100.0, 60.0, 0.4)),
            );
            self.row(&painter, r, &you, Some(&note), &cols, 0.0, false);
        }
    }

    fn is_you(&self, e: &Entry) -> bool {
        self.account_id.as_deref() == Some(e.player_id.as_str())
    }

    /// `lb-row-flash`: full for the first 30% of 1.4 s, then fading.
    fn flash(&self, now: f64) -> f32 {
        let Some(since) = self.flash_since else {
            return 0.0;
        };
        let t = ((now - since) / 1.4) as f32;
        if !(0.0..1.0).contains(&t) {
            0.0
        } else if t < 0.3 {
            1.0
        } else {
            1.0 - (t - 0.3) / 0.7
        }
    }

    fn head_row(&self, painter: &Painter, r: Rect, cols: &Columns) {
        let c = hsla(45.0, 100.0, 95.0, 0.45);
        let accent = self.mode().accent().face;
        let cy = r.center().y;
        let label = |s: &str, x: f32, right: bool, color: Color32| {
            let g = line(painter, &s.to_uppercase(), 11.0, body(), color, 0.2);
            let at = if right { x - g.size().x } else { x };
            painter.galley(pos2(at, cy - g.size().y / 2.0), g, color);
        };
        label("Rank", cols.rank_x(r), false, c);
        label("Player", cols.who_x(r), false, c);
        for (i, name) in ["Rating", "W / L / D", "Matches", "Rated"].into_iter().enumerate() {
            label(name, cols.num_right(r, i), true, if i == 0 { accent } else { c });
        }
        painter.hline(
            r.x_range(),
            r.bottom() - 0.5,
            Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.25)),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn row(&self, painter: &Painter, r: Rect, e: &Entry, note: Option<&str>, cols: &Columns, flash: f32, rule: bool) {
        let is_you = self.is_you(e);
        if is_you {
            horizontal_gradient(
                painter,
                r,
                &[
                    (0.0, hsla(46.0, 100.0, 60.0, 0.24)),
                    (0.7, hsla(46.0, 100.0, 60.0, 0.05)),
                    (1.0, hsla(46.0, 100.0, 60.0, 0.05)),
                ],
            );
            painter.rect_filled(Rect::from_min_size(r.min, vec2(5.0, r.height())), 0.0, gold_text());
        }
        if flash > 0.0 {
            painter.rect_filled(r, 0.0, hsla(46.0, 100.0, 60.0, 0.55 * flash));
        }
        if rule {
            painter.hline(
                r.x_range(),
                r.bottom() - 0.5,
                Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.1)),
            );
        }
        let cy = r.center().y;
        let rank_c = match e.rank {
            1 => hsl(48.0, 100.0, 62.0),
            2 => hsl(220.0, 25.0, 84.0),
            3 => hsl(26.0, 85.0, 64.0),
            _ => cream(),
        };
        let g = line(painter, &format!("#{}", e.rank), 19.0, display_family(), rank_c, 0.0);
        painter.galley(pos2(cols.rank_x(r), cy - g.size().y / 2.0), g, rank_c);
        // `lb-row__who`: avatar, name and the badges.
        let mut x = cols.who_x(r);
        let who_right = cols.num_right(r, 0) - cols.num_w - 8.0;
        let av = Rect::from_min_size(pos2(x, cy - 14.0), vec2(28.0, 28.0));
        let base = if is_you { gold_text() } else { avatar_color(&e.name) };
        let corners = [
            pos2(av.left() + av.width() * 0.14, av.top()),
            pos2(av.right(), av.top() + av.height() * 0.08),
            pos2(av.left() + av.width() * 0.88, av.bottom()),
            pos2(av.left(), av.top() + av.height() * 0.9),
        ];
        let mixed = lerp(violet(), base, 0.4);
        gradient_polygon(painter, &corners, &[(0.0, base), (1.0, mixed)]);
        let initial: String = e.name.chars().take(1).flat_map(char::to_uppercase).collect();
        let g = line(painter, &initial, 13.0, display_family(), ink(), 0.0);
        painter.galley(av.center() - g.size() / 2.0, g, ink());
        x = av.right() + 12.0;
        let room = (who_right - x).max(20.0);
        let name = elide(painter, &e.name, 16.0, display_family(), room * 0.6);
        let g = line(painter, &name, 16.0, display_family(), cream(), 0.03);
        let name_w = g.size().x;
        painter.galley(pos2(x, cy - g.size().y / 2.0), g, cream());
        x += name_w + 12.0;
        let badge = |x: &mut f32, s: &str, bg: Color32| {
            let g = line(painter, &s.to_uppercase(), 10.0, display_family(), ink(), 0.12);
            let b = Rect::from_min_size(pos2(*x, cy - 8.0), vec2(g.size().x + 14.0, 16.0));
            let c = b.width() * 0.1;
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(b.left() + c, b.top()),
                    b.right_top(),
                    pos2(b.right() - c, b.bottom()),
                    b.left_bottom(),
                ],
                bg,
                Stroke::NONE,
            ));
            painter.galley(b.center() - g.size() / 2.0, g, ink());
            *x = b.right() + 8.0;
        };
        if e.bot {
            badge(&mut x, "Bot", hsl(186.0, 90.0, 46.0));
        }
        if is_you {
            badge(&mut x, "You", gold_text());
        }
        let mut notes = Vec::new();
        if e.provisional {
            notes.push("Provisional");
        }
        if let Some(n) = note.filter(|n| !n.is_empty()) {
            notes.push(n);
        }
        for n in notes {
            let room = who_right - x;
            if room < 30.0 {
                break;
            }
            let shown = elide(painter, &n.to_uppercase(), 13.0, body(), room);
            let g = line(painter, &shown, 13.0, body(), gold_text(), 0.08);
            let w = g.size().x;
            painter.galley(pos2(x, cy - g.size().y / 2.0), g, gold_text());
            x += w + 8.0;
        }
        let soft = hsla(45.0, 100.0, 95.0, 0.88);
        let nums = [
            (e.rating.to_string(), cream()),
            (format!("{} / {} / {}", e.wins, e.losses, e.draws), soft),
            (e.played().to_string(), soft),
            (e.rated_matches.to_string(), soft),
        ];
        for (i, (s, c)) in nums.into_iter().enumerate() {
            let g = line(painter, &s, 17.0, body(), c, 0.0);
            painter.galley(pos2(cols.num_right(r, i) - g.size().x, cy - g.size().y / 2.0), g, c);
        }
    }

    /// `renderList`: the toolbar, then the whole top 100 (filtered by the
    /// search), and you pinned under it.
    fn list(
        &mut self,
        ui: &mut egui::Ui,
        pane: Rect,
        entries: &[Entry],
        you: Option<&Entry>,
        ranked: bool,
        client: &mut LobbyClient,
    ) {
        let painter = ui.painter().clone();
        let top = pane.top();
        let field = Rect::from_min_size(pos2(pane.left(), top), vec2(360.0_f32.min(pane.width() * 0.4), 46.0));
        TextField::new("lb-find", field, &mut self.query, FontId::new(16.0, body()))
            .placeholder("Find a player…")
            .enabled(ranked)
            .show(ui);
        let mut x = field.right() + 18.0;
        let in_list = you.is_some_and(|y| entries.iter().any(|e| e.player_id == y.player_id));
        if in_list {
            let label = "Jump to me";
            let g = line(&painter, &label.to_uppercase(), 13.0, display_family(), ink(), 0.1);
            let r = Rect::from_min_size(pos2(x, top + 1.0), vec2(g.size().x + 42.0, 40.0));
            let face = RaisedKey::new("lb-jump", r, KeyLook::cream().sm()).show(ui);
            KeyText::new(&label.to_uppercase(), "", display_family(), body())
                .size(13.0, 12.0)
                .spacing(0.1)
                .pad(4.0, 4.0)
                .layout(KeyTextLayout::Centered)
                .paint(&painter, &face);
            if face.clicked {
                self.query.clear();
                self.jump = true;
            }
            x = r.right() + 18.0;
        }
        let _ = x;
        // `lb-period`, at the right.
        let mut right = pane.right();
        for period in [Period::All, Period::Month] {
            let g = line(
                &painter,
                &period.name().to_uppercase(),
                12.0,
                display_family(),
                cream(),
                0.08,
            );
            let r = Rect::from_min_size(
                pos2(right - g.size().x - 32.0, top + 2.0),
                vec2(g.size().x + 32.0, 40.0),
            );
            right = r.left() - 8.0;
            if flat_key(
                ui,
                ("lb-period", period.id()),
                r,
                period.name(),
                None,
                period == self.list_period,
                12.0,
            ) {
                self.list_period = period;
                self.load(client);
            }
        }
        if !ranked {
            return;
        }
        let query = self.query.trim().to_lowercase();
        let shown: Vec<Entry> = if query.is_empty() {
            entries.to_vec()
        } else {
            entries
                .iter()
                .filter(|e| e.name.to_lowercase().contains(&query))
                .cloned()
                .collect()
        };
        let pin = you.map(|you| (you.clone(), next_step(entries, you, 100)));
        let board = Rect::from_min_max(pos2(pane.left(), top + 46.0 + 18.0), pane.max);
        self.board(ui, board, &shown, pin, true, "lb-list");
    }

    /// `renderRail`: your rank in each mode, in the rail's middle.
    pub fn rail(&mut self, ui: &mut egui::Ui, mid: Rect, rail_w: f32, client: &mut LobbyClient) {
        self.time_out();
        let painter = ui.painter().clone();
        let period = self.period();
        let top = rail_label(&painter, mid, rail_w, "Your ranks", period.name());
        let list = Rect::from_min_max(
            pos2(mid.left() + 22.0, top + 8.0),
            pos2(
                mid.left() + rail_w - shagger::ui::components::lobby_twin::RAIL_SKEW - 26.0,
                mid.bottom(),
            ),
        );
        if let Err(why) = self.availability() {
            let sub = hsla(250.0, 45.0, 8.0, 0.62);
            let g = crate::client::settings::settings_fields::wrapped(
                &painter,
                why,
                FontId::new(15.0, body()),
                sub,
                list.width().min(240.0),
            );
            painter.galley(list.min, g, sub);
            return;
        }
        let screen = ui.ctx().content_rect();
        let key_h = if screen.height() <= 960.0 { 40.0 } else { 46.0 };
        let show_record = screen.width() > 1200.0;
        let mut picked = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("lb-ranks")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, mode) in GAME_MODES.iter().enumerate() {
                        let (loaded, you) = self.yours(i, period);
                        let standing = match (loaded, you) {
                            (None | Some(Loaded::Loading { .. }), _) => "…".to_owned(),
                            (_, Some(y)) => format!("#{}", y.rank),
                            _ => "—".to_owned(),
                        };
                        let record = match (loaded, you) {
                            (Some(Loaded::Failed(_)), _) => "Unavailable".to_owned(),
                            (_, Some(y)) => y.rating_text(),
                            (Some(Loaded::Ready(_)), None) => "Unranked".to_owned(),
                            _ => String::new(),
                        };
                        let (slot, _) =
                            ui.allocate_exact_size(vec2(list.width() - 4.0, key_h + 9.6), egui::Sense::hover());
                        let rect = Rect::from_min_size(slot.min, vec2(slot.width() - 4.0, key_h));
                        let face = RailKey {
                            id: ("lb-rank", mode.id),
                            mark: hsl(mode.mark.0, mode.mark.1, mode.mark.2),
                            name: mode.title,
                            detail: if show_record { &record } else { "" },
                            right: &standing,
                            pressed: i == self.mode,
                            name_width: 76.0,
                        }
                        .show(ui, rect);
                        if face.clicked {
                            picked = Some(i);
                        }
                    }
                });
        });
        if let Some(m) = picked {
            self.set_mode(m, client);
        }
    }
}

fn rank_size(rank: u32, vw: f32, tight: bool) -> f32 {
    if tight {
        clamp_css(40.0, vw * 0.05, 60.0)
    } else if rank == 1 {
        clamp_css(76.0, vw * 0.066, 128.0)
    } else {
        clamp_css(60.0, vw * 0.052, 100.0)
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Body text with letter spacing, wrapped at `width` on spaces.
fn spaced_wrap(
    painter: &Painter,
    s: &str,
    size: f32,
    color: Color32,
    em: f32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let font = FontId::new(size, body());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        s,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: size * em,
            line_height: Some(size * 1.2),
            ..Default::default()
        },
    );
    job.wrap.max_width = width.max(1.0);
    painter.layout_job(job)
}

/// The grid of `lb-row`: rank, who, then four numbers at the right.
struct Columns {
    rank_w: f32,
    num_w: f32,
}

impl Columns {
    fn new(width: f32, vw: f32) -> Self {
        let narrow = vw <= 1500.0;
        let (rank_w, min, max) = if narrow {
            (64.0, 56.0, 84.0)
        } else {
            (76.0, 70.0, 110.0)
        };
        let free = width - 18.0 - 22.0 - rank_w - 160.0;
        Self {
            rank_w,
            num_w: (free / 4.0).clamp(min, max),
        }
    }

    fn rank_x(&self, r: Rect) -> f32 {
        r.left() + 18.0
    }

    fn who_x(&self, r: Rect) -> f32 {
        r.left() + 18.0 + self.rank_w
    }

    /// The right edge of number column `i` (0 is Rating).
    fn num_right(&self, r: Rect, i: usize) -> f32 {
        r.right() - 22.0 - (3 - i) as f32 * self.num_w
    }
}

enum LineKind {
    Tag(std::sync::Arc<egui::Galley>, bool),
    Text(std::sync::Arc<egui::Galley>),
    Rating(std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>),
}

/// One line of a plinth's face, measured.
struct Line {
    height: f32,
    kind: LineKind,
}

impl Line {
    fn tag(painter: &Painter, s: &str, bot: bool) -> Self {
        let c = if bot { ink() } else { gold_text() };
        let g = line(painter, &s.to_uppercase(), 10.0, display_family(), c, 0.16);
        Self {
            height: g.size().y + 8.0,
            kind: LineKind::Tag(g, bot),
        }
    }

    fn text(g: std::sync::Arc<egui::Galley>) -> Self {
        Self {
            height: g.size().y,
            kind: LineKind::Text(g),
        }
    }

    fn paint(&self, painter: &Painter, at: egui::Pos2) {
        match &self.kind {
            LineKind::Tag(g, bot) => {
                let r = Rect::from_min_size(at, vec2(g.size().x + 18.0, self.height));
                let c = r.width() * 0.08;
                let bg = if *bot { hsl(186.0, 90.0, 46.0) } else { ink() };
                painter.add(Shape::convex_polygon(
                    vec![
                        pos2(r.left() + c, r.top()),
                        r.right_top(),
                        pos2(r.right() - c, r.bottom()),
                        r.left_bottom(),
                    ],
                    bg,
                    Stroke::NONE,
                ));
                let fg = if *bot { ink() } else { gold_text() };
                painter.galley(r.center() - g.size() / 2.0, g.clone(), fg);
            }
            LineKind::Text(g) => painter.galley(at, g.clone(), ink()),
            LineKind::Rating(big, small) => {
                painter.galley(at, big.clone(), ink());
                let x = at.x + big.size().x + 6.0;
                let y = at.y + big.size().y - small.size().y - 1.0;
                painter.galley(pos2(x, y), small.clone(), ink());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(rank: u32, score: f64) -> Entry {
        Entry {
            rank,
            player_id: format!("p{rank}"),
            name: format!("P{rank}"),
            wins: 1,
            losses: 0,
            draws: 0,
            rating: 1000,
            conservative_score: score,
            provisional: false,
            rated_matches: 1,
            bot: false,
        }
    }

    #[test]
    fn the_next_step_is_a_measured_gap() {
        let entries: Vec<Entry> = (1..=12).map(|r| entry(r, 20.0 - f64::from(r))).collect();
        assert_eq!(next_step(&entries, &entries[0], 10), "On top. Hold it");
        // In the list: the one just above.
        assert_eq!(next_step(&entries, &entries[4], 10), "40 rating points behind #4");
        // Below the shown ten: the tenth.
        let you = entry(14, 20.0 - 14.0 + 0.01);
        assert_eq!(next_step(&entries, &you, 10), "160 rating points behind #10");
        let close = entry(11, 20.0 - 10.0 - 0.01);
        assert_eq!(next_step(&entries, &close, 10), "1 rating point behind #10");
    }

    #[test]
    fn avatar_colours_are_the_browsers() {
        // hash("Reaper") % 8 and hash("Eclipse") % 8, from the TypeScript.
        let pick = |name: &str| {
            let mut hash: u32 = 0;
            for c in name.encode_utf16() {
                hash = hash.wrapping_mul(31).wrapping_add(u32::from(c));
            }
            hash as usize % 8
        };
        for name in ["Reaper", "Eclipse", "Nova", "Ünï"] {
            let (h, s, l) = AVATAR_COLORS[pick(name)];
            assert_eq!(avatar_color(name), hsl(h, s, l));
        }
    }

    #[test]
    fn a_page_is_read_only_when_whole() {
        let row = json!({"rank":1,"playerId":"7","name":"A","wins":3,"losses":1,"draws":0,
            "rating":1100,"conservativeScore":2.5,"provisional":false,"ratedMatches":4});
        let page = json!({"type":"leaderboard:page","requestId":"lb","scope":"overall","system":"trueskill",
            "policyVersion":POLICY_VERSION,"entries":[row],"total":1,"localPlayerEntry":row,
            "localProcessing":{"excluded":2}});
        let read = read_page(&page).unwrap();
        assert_eq!(read.entries.len(), 1);
        assert_eq!(read.excluded, 2);
        assert_eq!(read.local_player_entry.as_ref().map(|e| e.rating), Some(1100));
        let mut public = page.clone();
        public["scope"] = json!("public");
        assert!(read_page(&public).is_err());
        let mut unrated = page;
        unrated["entries"][0]["ratedMatches"] = json!(0);
        assert!(read_page(&unrated).is_err());
    }

    #[test]
    fn an_answer_lands_on_its_request() {
        let mut view = LeaderboardView::new();
        view.connected = true;
        view.supported = true;
        view.boards.insert(
            (0, Period::Month),
            Loaded::Loading {
                request: "a".into(),
                sent: Instant::now(),
            },
        );
        view.on_message("leaderboard:error", &json!({"requestId":"b","reason":"rate_limited"}));
        assert!(matches!(view.boards[&(0, Period::Month)], Loaded::Loading { .. }));
        view.on_message("leaderboard:error", &json!({"requestId":"a","reason":"rate_limited"}));
        assert!(matches!(&view.boards[&(0, Period::Month)], Loaded::Failed(why) if why == "Leaderboard: rate_limited"));
    }
}
