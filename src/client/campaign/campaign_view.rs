//! wash-fps `client/campaign/CampaignView.ts` and `styles/Campaign.css`'s
//! page: the operations as a list down the page, each with its comic card
//! (clicked, it reads panel by panel) and a track node showing where it
//! stands (cleared, current or locked), and the way into the next one;
//! under them, the finale. Progress comes from the lobby
//! (`campaign:state`); until it answers, the copy this computer kept is
//! shown.
//!
//! The art (the cards, the squad and their street) is built into the game
//! (`assets/campaign`), decoded on a thread the first time the page or a
//! card is wanted. egui has no CSS filters, so the blurred and darkened
//! copies (a locked card, the street at the banner's sides, the card behind
//! the panel being read) are made once as the art decodes.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use shagger::lobby::types::{CampaignClear, CampaignMissionInfo, CampaignProgress};
use shagger::ui::components::lobby_twin::{
    elide, gradient_polygon, gradient_polygon_in, gradient_polygon_x, hsl, hsla, text,
};
use shagger::ui::components::raised_key::{KeyLook, KeyText, KeyTextLayout, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use super::campaign_story::{CampaignCard, FINALE_CARD, campaign_story};
use crate::client::campaign_board_pane::{
    CampaignBoard, CampaignBoardPane, CampaignBoardState, board_panel, read_board,
};
use crate::client::new_ui_api::request_failure;
use crate::client::washoi_style::display_family;

/// What the campaign board asks the lobby for, as a failure names it.
const BOARDS: &str = "the campaign boards";

/// `GAMETYPE_NAMES`: the game modes by their full names.
#[must_use]
pub fn gametype_name(id: &str) -> &str {
    match id {
        "amphi" => "Amphi",
        "duel" => "Duel",
        "ffa" => "Free For All",
        "snd" => "Search & Destroy",
        "tdm" => "Team Deathmatch",
        "survival" => "Survival",
        other => other,
    }
}

/// `MissionStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionStatus {
    Cleared,
    Current,
    Open,
    Locked,
}

/// `missionStatus`: where a mission stands: cleared, the one to play next,
/// open (unlocked, not cleared, not next: only after a mission is taken out
/// of order), or locked. Numbers past the written missions are locked.
#[must_use]
pub fn mission_status(number: u32, state: Option<&CampaignProgress>) -> MissionStatus {
    let Some(state) = state else {
        return if number == 1 {
            MissionStatus::Current
        } else {
            MissionStatus::Locked
        };
    };
    let cleared = |id: &str| state.clears.iter().any(|c| c.mission_id == id);
    let Some(mission) = state.missions.iter().find(|m| m.number == number) else {
        return MissionStatus::Locked;
    };
    if number > state.unlocked_through {
        return MissionStatus::Locked;
    }
    if cleared(&mission.id) {
        return MissionStatus::Cleared;
    }
    let first_open = state
        .missions
        .iter()
        .find(|m| m.number <= state.unlocked_through && !cleared(&m.id));
    if first_open.map(|m| m.number) == Some(number) {
        MissionStatus::Current
    } else {
        MissionStatus::Open
    }
}

/// The campaign's colours (`.campaign`'s tokens).
fn bg() -> Color32 {
    hsl(268.0, 52.0, 9.0)
}
fn pink() -> Color32 {
    hsl(322.0, 95.0, 62.0)
}
fn cyan() -> Color32 {
    hsl(182.0, 95.0, 52.0)
}
fn gold() -> Color32 {
    hsl(46.0, 100.0, 57.0)
}
fn dim() -> Color32 {
    hsl(250.0, 14.0, 30.0)
}
fn cream(a: f32) -> Color32 {
    hsla(45.0, 100.0, 95.0, a)
}
fn ink() -> Color32 {
    hsl(250.0, 45.0, 7.0)
}

fn body() -> FontFamily {
    FontFamily::Proportional
}

/// `INGRESS`: the campaign's opening, under the page title.
const INGRESS: &str = "Welcome to Neo-Kyoto, where walking is for targets, friction is illegal, and megacorporations broadcast street violence for prime-time ad revenue. You're an unranked runner looking to turn lethal slide-speed into cold crypto, starting with a back-alley Cartel tryout. Out-move the gangs, scrap the bots, and claim the arena.";

/// `INGRESS_SHORT`: the same in one line, for a screen too short for the
/// whole opening.
const INGRESS_SHORT: &str = "From a back-alley Cartel tryout to the top of the broadcast: climb the 8-stage Neo-Kyoto gauntlet, where speed is survival and friction is fatal.";

/// `FINALE_TITLE`.
const FINALE_TITLE: &str = "Campaign complete!";

fn screen_w(ui: &egui::Ui) -> f32 {
    ui.ctx().content_rect().width()
}

// ── The art ──

const CARDS: [&[u8]; 9] = [
    include_bytes!("../../../assets/campaign/card1.webp"),
    include_bytes!("../../../assets/campaign/card2.webp"),
    include_bytes!("../../../assets/campaign/card3.webp"),
    include_bytes!("../../../assets/campaign/card4.webp"),
    include_bytes!("../../../assets/campaign/card5.webp"),
    include_bytes!("../../../assets/campaign/card6.webp"),
    include_bytes!("../../../assets/campaign/card7.webp"),
    include_bytes!("../../../assets/campaign/card8.webp"),
    // The finale.
    include_bytes!("../../../assets/campaign/card9.webp"),
];
pub(crate) const SQUAD: &[u8] = include_bytes!("../../../assets/campaign/squad.webp");
pub(crate) const STREET: &[u8] = include_bytes!("../../../assets/campaign/street.webp");

/// One decoded picture, by its name: `card<n>`, `card<n>-locked`,
/// `card<n>-blur`, `squad`, `street`, `street-blur`.
type Decoded = (String, egui::ColorImage);

fn color_image(image: &image::RgbaImage) -> egui::ColorImage {
    let size = [image.width() as usize, image.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw())
}

/// A softened copy: scaled to `width`, blurred by `sigma`, its light times
/// `brightness`, its colour times `saturation`.
fn softened(image: &image::RgbaImage, width: u32, sigma: f32, brightness: f32, saturation: f32) -> image::RgbaImage {
    let height = (u64::from(image.height()) * u64::from(width) / u64::from(image.width().max(1))).max(1) as u32;
    let small = image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle);
    let mut out = image::imageops::blur(&small, sigma);
    for p in out.pixels_mut() {
        let [r, g, b, a] = p.0.map(f32::from);
        let grey = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let mix = |c: f32| ((grey + (c - grey) * saturation) * brightness).clamp(0.0, 255.0) as u8;
        p.0 = [mix(r), mix(g), mix(b), a as u8];
    }
    out
}

/// A set of pictures decoded together: the banner (`0`, the street and
/// the squad) or card `n` with its softened copies.
fn decode_group(group: u8, send: &std::sync::mpsc::Sender<Decoded>) {
    let decode = |bytes: &[u8]| image::load_from_memory(bytes).ok().map(|i| i.to_rgba8());
    if group == 0 {
        if let Some(street) = decode(STREET) {
            let _ = send.send(("street".into(), color_image(&street)));
            // `.campaign__backdrop`: blur(8px) brightness(0.45) saturate(0.8).
            let _ = send.send((
                "street-blur".into(),
                color_image(&softened(&street, 640, 4.0, 0.45, 0.8)),
            ));
        }
        if let Some(squad) = decode(SQUAD) {
            let _ = send.send(("squad".into(), color_image(&squad)));
        }
        return;
    }
    let Some(card) = CARDS.get(usize::from(group) - 1).and_then(|bytes| decode(bytes)) else {
        return;
    };
    // GPUs and egui take textures up to 2048 a side: the finale's card is
    // larger and is drawn at most about 1500 wide.
    let card = if card.width().max(card.height()) > 2048 {
        let (w, h) = (card.width(), card.height());
        let scale = 2048.0 / w.max(h) as f32;
        image::imageops::resize(
            &card,
            (w as f32 * scale) as u32,
            (h as f32 * scale) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        card
    };
    let _ = send.send((format!("card{group}"), color_image(&card)));
    // A locked card: grayscale(1) brightness(0.35) blur(3px).
    let _ = send.send((
        format!("card{group}-locked"),
        color_image(&softened(&card, 480, 1.5, 0.35, 0.0)),
    ));
    // The card behind the panel being read: blur(7px) brightness(0.38)
    // saturate(0.75).
    let _ = send.send((
        format!("card{group}-blur"),
        color_image(&softened(&card, 768, 3.5, 0.38, 0.75)),
    ));
}

/// Every picture, in turn.
fn decode_all(send: &std::sync::mpsc::Sender<Decoded>) {
    for group in 0..=CARDS.len() as u8 {
        decode_group(group, send);
    }
}

/// The campaign's pictures as egui textures, each group decoded on its
/// own thread the first time it is wanted.
pub struct CampaignArt {
    send: std::sync::mpsc::Sender<Decoded>,
    receiver: Receiver<Decoded>,
    requested: [bool; CARDS.len() + 1],
    textures: HashMap<String, egui::TextureHandle>,
    /// The context the textures were loaded into. Each screen's window
    /// brings its own, and the art outlives them (it is the lobby's): a
    /// texture from another context shows nothing there.
    ctx: Option<egui::Context>,
}

impl Default for CampaignArt {
    fn default() -> Self {
        let (send, receiver) = channel();
        Self {
            send,
            receiver,
            requested: [false; CARDS.len() + 1],
            textures: HashMap::new(),
            ctx: None,
        }
    }
}

impl CampaignArt {
    /// Asks for the banner (`0`) or card `group`, once.
    pub fn want(&mut self, group: u8) {
        let Some(asked) = self.requested.get_mut(usize::from(group)) else {
            return;
        };
        if std::mem::replace(asked, true) {
            return;
        }
        let send = self.send.clone();
        let spawned = std::thread::Builder::new()
            .name("campaign-art".into())
            .spawn(move || decode_group(group, &send));
        if spawned.is_err() {
            self.requested[usize::from(group)] = false;
        }
    }

    /// Takes what has decoded since the last frame. In a new context
    /// (back from a match) the pictures are decoded again for it.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if self.ctx.as_ref() != Some(ctx) {
            if self.ctx.is_some() {
                self.textures.clear();
                self.requested = [false; CARDS.len() + 1];
            }
            self.ctx = Some(ctx.clone());
        }
        while let Ok((name, image)) = self.receiver.try_recv() {
            let texture = ctx.load_texture(&name, image, egui::TextureOptions::LINEAR);
            self.textures.insert(name, texture);
            ctx.request_repaint();
        }
    }

    /// A picture by name, once decoded.
    #[must_use]
    pub fn texture(&self, name: &str) -> Option<&egui::TextureHandle> {
        self.textures.get(name)
    }
}

/// `object-fit: cover` at `focus` (fractions): the part of an `image`
/// sized picture that fills `frame` without stretching.
#[must_use]
pub fn cover_uv(image: [f32; 2], frame: egui::Vec2, focus: (f32, f32)) -> Rect {
    let scale = (frame.x / image[0]).max(frame.y / image[1]);
    let shown = vec2(frame.x / scale / image[0], frame.y / scale / image[1]);
    let min = pos2((1.0 - shown.x) * focus.0, (1.0 - shown.y) * focus.1);
    Rect::from_min_size(min, shown)
}

// ── The page ──

/// What the page asks of the panel.
#[derive(Debug, Clone, PartialEq)]
pub enum CampaignAction {
    /// Start (or replay) a mission.
    Start(CampaignMissionInfo),
    /// `readCard`: read a comic card panel by panel, starting nothing.
    Read(CampaignCard, &'static str),
    /// `openBoard`: the Leaderboard key, pressed with the board closed; the
    /// panel asks the lobby and answers with `board_loading` or
    /// `board_failed`.
    OpenBoard,
}

/// `CampaignView`.
#[derive(Default)]
pub struct CampaignView {
    state: Option<CampaignProgress>,
    /// Mission keys may be pressed: the lobby is there and no room is open.
    startable: bool,
    reason: String,
    /// The current mission the list was last brought to; other redraws keep
    /// the reader's place.
    shown_current: Option<String>,
    /// Bring the current mission into view on the next frame.
    scroll_to_current: bool,
    /// How far the list has scrolled, for the banner's drift.
    scroll: f32,
    /// A mission key pressed: "Starting…" until the page is redrawn
    /// otherwise.
    starting: Option<String>,
    pub art: CampaignArt,
    started: Option<std::time::Instant>,
    /// The finale as a finished campaign shows it, scrolled into view: for
    /// screenshots (`--page campaign/complete`).
    preview_finale: bool,
    /// The campaign's leaderboard, in place of the operations while it is
    /// open.
    board_open: bool,
    /// Something is over the board (a profile, the welcome, a comic card,
    /// the guide): its Escape is that one's, not the board's.
    board_covered: bool,
    board: Option<CampaignBoardState>,
    /// The request the board waits for, and when it went: a late answer for
    /// an earlier opening is dropped.
    board_request: Option<(String, std::time::Instant)>,
    board_pane: CampaignBoardPane,
    /// The account on this computer, for "You" on the boards.
    you: Option<String>,
}

impl CampaignView {
    /// `setState`: the campaign as the lobby (or this computer's copy) has
    /// it.
    pub fn set_state(&mut self, state: Option<CampaignProgress>) {
        self.state = state;
        let current = self.current_mission();
        if self.state.is_some() && current != self.shown_current {
            self.scroll_to_current = true;
        }
    }

    #[must_use]
    pub fn state(&self) -> Option<&CampaignProgress> {
        self.state.as_ref()
    }

    /// `setStartable`: whether a mission can start now, and why not when it
    /// cannot.
    pub fn set_startable(&mut self, startable: bool, reason: &str) {
        if startable && !self.startable {
            self.starting = None;
        }
        self.startable = startable;
        reason.clone_into(&mut self.reason);
    }

    /// The account on this computer, for "You" on the boards.
    pub fn set_account(&mut self, id: Option<String>) {
        self.you = id;
    }

    /// `openBoard`'s request went out as `request`.
    pub fn board_loading(&mut self, request: String) {
        self.board_open = true;
        self.board = Some(CampaignBoardState::Loading);
        self.board_request = Some((request, std::time::Instant::now()));
    }

    /// `openBoard` could not ask: the lobby is offline or runs no campaign.
    pub fn board_failed(&mut self, reason: &str) {
        self.board_open = true;
        self.board = Some(CampaignBoardState::Failed(reason.to_owned()));
        self.board_request = None;
    }

    /// `closeBoard`: back to the operations.
    pub fn close_board(&mut self) {
        self.board_open = false;
        self.board_request = None;
    }

    #[must_use]
    pub fn board_open(&self) -> bool {
        self.board_open
    }

    /// Whether something is over the page this frame, so Escape is not
    /// the board's (`stopPropagation` on the board's own keydown).
    pub fn set_covered(&mut self, covered: bool) {
        self.board_covered = covered;
    }

    /// The lobby's `campaign:board` or `campaign:board:error`.
    pub fn on_board_message(&mut self, kind: &str, message: &serde_json::Value) {
        let Some((request, _)) = &self.board_request else {
            return;
        };
        if message.get("requestId").and_then(serde_json::Value::as_str) != Some(request.as_str()) {
            return;
        }
        self.board_request = None;
        self.board = Some(match (kind, read_board(message)) {
            ("campaign:board", Some(board)) => CampaignBoardState::Ready(board),
            ("campaign:board", None) => {
                CampaignBoardState::Failed(request_failure("Campaign board: unreadable answer", BOARDS))
            }
            _ => {
                let reason = message
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unavailable");
                CampaignBoardState::Failed(request_failure(&format!("Campaign board: {reason}"), BOARDS))
            }
        });
    }

    /// The lobby went away while the board waited.
    pub fn board_disconnected(&mut self) {
        if self.board_request.take().is_some() {
            self.board = Some(CampaignBoardState::Failed(request_failure(
                "Lobby disconnected",
                BOARDS,
            )));
        }
    }

    /// For screenshots: the board open, as if the lobby had answered.
    pub fn seed_board(&mut self, board: CampaignBoard) {
        self.board_open = true;
        self.board_request = None;
        self.board = Some(CampaignBoardState::Ready(board));
    }

    /// `showCurrent`: bring the mission to play next into view.
    pub fn show_current(&mut self) {
        self.scroll_to_current = true;
    }

    fn cleared_count(&self) -> usize {
        let Some(state) = &self.state else { return 0 };
        state
            .missions
            .iter()
            .filter(|m| state.clears.iter().any(|c| c.mission_id == m.id))
            .count()
    }

    /// `renderCount`: the count on its dark plate, its left middle at
    /// `at`, and a segment for each operation: cyan once cleared, gold for
    /// the next.
    fn count_plate(&self, painter: &Painter, at: Pos2, length: u32) {
        let count = text(
            painter,
            &format!("{} / {length}", self.cleared_count()),
            28.0,
            display_family(),
            cyan(),
            0.0,
        );
        let label = text(painter, "CLEARED", 13.0, body(), cream(0.75), 13.0 * 0.24);
        let (pip_w, pip_gap) = (15.0, 5.0);
        let pips_w = length as f32 * (pip_w + pip_gap) - pip_gap;
        let gap = 14.0;
        let inner_h = count.size().y.max(label.size().y).max(14.0);
        let width = 20.0 + count.size().x + gap + label.size().x + gap + 4.0 + pips_w + 24.0;
        let rect = Rect::from_min_size(pos2(at.x, at.y - (inner_h + 26.0) / 2.0), vec2(width, inner_h + 26.0));
        plate_behind(painter, rect, PlateLook::Dark);
        let mid = rect.top() + 12.0 + inner_h / 2.0;
        let mut x = rect.left() + 20.0;
        // `text-shadow: 0 0 14px` cyan, as a soft halo.
        painter.rect_filled(
            Rect::from_min_size(pos2(x, mid - count.size().y / 2.0), count.size()).expand(4.0),
            8.0,
            hsla(182.0, 95.0, 52.0, 0.12),
        );
        let w = count.size().x;
        painter.galley(pos2(x, mid - count.size().y / 2.0), count, cyan());
        x += w + gap;
        let w = label.size().x;
        painter.galley(pos2(x, mid - label.size().y / 2.0), label, cream(0.75));
        x += w + gap + 4.0;
        let skew = (24f32).to_radians().tan() * 7.0;
        for number in 1..=length {
            let (fill, glow) = match mission_status(number, self.state.as_ref()) {
                MissionStatus::Cleared => (cyan(), Some(hsla(182.0, 95.0, 52.0, 0.3))),
                MissionStatus::Current | MissionStatus::Open => (gold(), Some(hsla(46.0, 100.0, 57.0, 0.3))),
                MissionStatus::Locked => (cream(0.14), None),
            };
            let quad = |grow: f32| {
                vec![
                    pos2(x + skew - grow, mid - 7.0 - grow),
                    pos2(x + pip_w + skew + grow, mid - 7.0 - grow),
                    pos2(x + pip_w - skew + grow, mid + 7.0 + grow),
                    pos2(x - skew - grow, mid + 7.0 + grow),
                ]
            };
            if let Some(glow) = glow {
                painter.add(Shape::convex_polygon(quad(3.0), glow, Stroke::NONE));
            }
            painter.add(Shape::convex_polygon(quad(0.0), fill, Stroke::NONE));
            x += pip_w + pip_gap;
        }
    }

    /// Shows the finale as a finished campaign would, in view: for
    /// screenshots.
    pub fn preview_finale(&mut self) {
        self.preview_finale = true;
        self.scroll_to_current = true;
    }

    /// Every operation cleared: the finale shows.
    fn complete(&self) -> bool {
        if self.preview_finale {
            return true;
        }
        let length = self.state.as_ref().map_or(0, |s| s.length);
        length > 0 && self.cleared_count() >= length as usize
    }

    fn current_mission(&self) -> Option<String> {
        let state = self.state.as_ref()?;
        state
            .missions
            .iter()
            .find(|m| mission_status(m.number, Some(state)) == MissionStatus::Current)
            .map(|m| m.id.clone())
    }

    /// Draws the page over `page` (the stage, edge to edge) with its
    /// content inside `content`.
    pub fn ui(&mut self, ui: &mut egui::Ui, page: Rect, content: Rect) -> Option<CampaignAction> {
        for group in 0..=CARDS.len() as u8 {
            self.art.want(group);
        }
        self.art.poll(ui.ctx());
        let time = self
            .started
            .get_or_insert_with(std::time::Instant::now)
            .elapsed()
            .as_secs_f32();
        let painter = ui.painter().with_clip_rect(page);
        let screen_h = ui.ctx().content_rect().height();
        // The campaign's own night, top to bottom.
        let night = [
            page.left_top(),
            page.right_top(),
            page.right_bottom(),
            page.left_bottom(),
        ];
        gradient_polygon(&painter, &night, &[(0.0, bg()), (1.0, hsl(262.0, 48.0, 7.0))]);

        let banner_h = (screen_h * 0.42).min(580.0);
        self.banner(&painter, page, banner_h, time);

        // The heading on the banner's fading foot: the title with the count
        // beside it, and the campaign's opening under both.
        let head_y = page.top() + banner_h - banner_h * 0.36;
        let title_size = (page.width() * 0.04).clamp(40.0, 72.0);
        let plate = campaign_plate(&painter, pos2(content.left(), head_y), "CAMPAIGN", title_size);
        let length = self
            .state
            .as_ref()
            .map_or(8, |s| if s.length == 0 { 8 } else { s.length });
        self.count_plate(&painter, pos2(plate.right() + 34.0, plate.center().y), length);
        // A short screen gets the one-line version, leaving the operations
        // room.
        let ingress = if screen_h <= 1000.0 { INGRESS_SHORT } else { INGRESS };
        let size = (screen_w(ui) * 0.009).clamp(14.0, 17.0);
        let box_ = caption_box(
            &painter,
            pos2(content.left(), plate.bottom() + 18.0),
            ingress,
            size,
            (content.width()).min(size * 0.6 * 72.0 + 36.0),
        );
        let mut top = box_.bottom() + 18.0;
        if let Some(a) = self.board_key(ui, content, box_.bottom()) {
            return Some(a);
        }
        if self.board_open {
            self.board_ui(
                ui,
                Rect::from_min_max(pos2(content.left(), box_.bottom() + 22.0), content.max),
            );
            return None;
        }
        if !self.startable && !self.reason.is_empty() {
            let notice = text(&painter, &self.reason, 14.0, body(), gold(), 0.0);
            let h = notice.size().y;
            painter.galley(pos2(content.left(), top), notice, gold());
            top += h + 14.0;
        }
        let list = Rect::from_min_max(
            pos2(content.left(), top),
            pos2(content.right() + 30.0, content.bottom()),
        );
        self.list(ui, list, time)
    }

    /// `.campaign__board-key`: the Leaderboard key at the head's far end,
    /// its foot level with the opening's; gold lined like the rail's
    /// Campaign key, pressed in while the board is open.
    fn board_key(&mut self, ui: &mut egui::Ui, content: Rect, bottom: f32) -> Option<CampaignAction> {
        let painter = ui.painter().clone();
        let size = (screen_w(ui) * 0.011).clamp(15.0, 21.0);
        let label = "LEADERBOARD";
        let galley = text(&painter, label, size, display_family(), ink(), size * 0.07);
        let h = (size * 3.0).max(52.0);
        let w = 32.0 + galley.size().x + 40.0;
        let rect = Rect::from_min_size(pos2(content.right() - 18.0 - w, bottom - h), vec2(w, h));
        let face = |top: f32, mid: f32, bottom: f32| KeyLook {
            face: vec![
                (0.0, hsl(52.0, 100.0, top)),
                (0.45, hsl(46.0, 100.0, mid)),
                (1.0, hsl(40.0, 98.0, bottom)),
            ],
            ..KeyLook::gold()
        };
        let mut key =
            RaisedKey::new("campaign-board-key", rect, face(66.0, 57.0, 47.0)).hover(face(74.0, 64.0, 54.0), -0.35);
        if self.board_open {
            key = key.rest_press(0.62).hover(face(74.0, 64.0, 54.0), 0.62);
        }
        let face = key.show(ui);
        // Scanlines over the gold, then the italic label on its pink shadow.
        let clip = painter.with_clip_rect(face.rect.intersect(painter.clip_rect()));
        let mut y = face.rect.top();
        while y < face.rect.bottom() {
            clip.rect_filled(
                Rect::from_min_max(pos2(face.rect.left(), y), pos2(face.rect.right(), y + 2.0)),
                0.0,
                hsla(40.0, 100.0, 30.0, 0.14),
            );
            y += 5.0;
        }
        let italic = |color: Color32| {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                label,
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(size, display_family()),
                    color,
                    italics: true,
                    extra_letter_spacing: size * 0.07,
                    ..Default::default()
                },
            );
            painter.layout_job(job)
        };
        let shadow = hsla(322.0, 95.0, 40.0, 0.7);
        let g = italic(face.text);
        let at = pos2(face.rect.left() + 32.0, face.rect.center().y - g.size().y / 2.0);
        painter.galley(at + vec2(0.06 * size, 0.07 * size), italic(shadow), shadow);
        painter.galley(at, g, face.text);
        if face.clicked {
            if self.board_open {
                self.close_board();
            } else {
                return Some(CampaignAction::OpenBoard);
            }
        }
        None
    }

    /// `.campaign__board`: the leaderboard in the operations' place, a
    /// close key in its corner; Escape closes it too. It scrolls.
    fn board_ui(&mut self, ui: &mut egui::Ui, rect: Rect) {
        // `requestCampaignBoard`'s ten seconds.
        if let Some((_, sent)) = &self.board_request {
            if sent.elapsed() > std::time::Duration::from_secs(10) {
                self.board_request = None;
                self.board = Some(CampaignBoardState::Failed(request_failure(
                    "Campaign board request timed out",
                    BOARDS,
                )));
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        // Taken, so nothing beneath closes on the same press.
        if !self.board_covered && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.close_board();
            return;
        }
        board_panel(ui.painter(), rect);
        let inner = rect.shrink2(vec2(26.0, 0.0));
        let inner = Rect::from_min_max(
            pos2(inner.left(), rect.top() + 26.0),
            pos2(inner.right(), rect.bottom() - 24.0),
        );
        let state = self.board.clone().unwrap_or(CampaignBoardState::Loading);
        let you = self.you.clone();
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("campaign-board")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let origin = ui.cursor().min;
                    let at = Rect::from_min_size(origin, vec2(inner.width() - 14.0, inner.height()));
                    let h = self.board_pane.paint(ui.painter(), at, &state, you.as_deref());
                    ui.allocate_rect(Rect::from_min_size(origin, vec2(at.width(), h)), egui::Sense::hover());
                });
        });
        // `campaign__board-close`: gold, pink when pointed at.
        let close = Rect::from_min_size(pos2(rect.right() - 34.0 - 54.0, rect.top() + 14.0), vec2(54.0, 54.0));
        let response = ui.interact(close, egui::Id::unique("campaign-board-close"), egui::Sense::click());
        let lit = response.hovered();
        let painter = ui.painter();
        let slant = |r: Rect| {
            vec![
                pos2(r.left() + 10.0, r.top()),
                r.right_top(),
                pos2(r.right() - 10.0, r.bottom()),
                r.left_bottom(),
            ]
        };
        painter.add(Shape::convex_polygon(
            slant(close.translate(vec2(4.0, 5.0))),
            hsl(250.0, 50.0, 3.0),
            Stroke::NONE,
        ));
        painter.add(Shape::convex_polygon(
            slant(close),
            if lit { pink() } else { gold() },
            Stroke::NONE,
        ));
        let fg = if lit { cream(1.0) } else { ink() };
        let g = text(painter, "×", 34.0, display_family(), fg, 0.0);
        painter.galley(close.center() - g.size() / 2.0, g, fg);
        if lit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            self.close_board();
        }
    }

    /// `.campaign__banner`: the squad on their street across the top,
    /// drifting as the list scrolls and wandering like a handheld camera.
    fn banner(&self, painter: &Painter, page: Rect, banner_h: f32, time: f32) {
        let rect = Rect::from_min_size(page.min, vec2(page.width(), banner_h));
        let clip = painter.with_clip_rect(rect.intersect(page));
        let drift = self.scroll.min(600.0);
        let (far, near) = handheld(time);
        let white = Color32::WHITE;
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        if let Some(t) = self.art.texture("street-blur") {
            // `inset: -40px -20px 0`, `background-position: center 30%`, cover.
            let r = Rect::from_min_max(
                pos2(rect.left() - 20.0 + far.x, rect.top() - 40.0 - drift * 0.02 + far.y),
                pos2(rect.right() + 20.0 + far.x, rect.bottom() + far.y),
            );
            let size = t.size_vec2();
            clip.image(t.id(), r, cover_uv([size.x, size.y], r.size(), (0.5, 0.3)), white);
        }
        let stage_h = banner_h * 1.7;
        let stage_w = stage_h * 16.0 / 9.0;
        let stage_top = rect.top() - stage_h * 0.01;
        let cx = rect.center().x;
        if let Some(t) = self.art.texture("street") {
            let r = Rect::from_min_size(
                pos2(cx - stage_w / 2.0 + far.x, stage_top - drift * 0.035 + far.y),
                vec2(stage_w, stage_h),
            );
            // `mask-image`: the street fades out at its sides, over 14 %.
            let edge = stage_w * 0.14;
            let strips = 12;
            let inner = Rect::from_min_max(pos2(r.left() + edge, r.top()), pos2(r.right() - edge, r.bottom()));
            image_part(&clip, t.id(), r, inner, white);
            for side in [0.0f32, 1.0] {
                for i in 0..strips {
                    let f0 = i as f32 / strips as f32;
                    let f1 = (i + 1) as f32 / strips as f32;
                    let (x0, x1) = if side == 0.0 {
                        (r.left() + edge * f0, r.left() + edge * f1)
                    } else {
                        (r.right() - edge * f1, r.right() - edge * f0)
                    };
                    let alpha = (f0 + f1) / 2.0;
                    let part = Rect::from_min_max(pos2(x0, r.top()), pos2(x1, r.bottom()));
                    image_part(&clip, t.id(), r, part, Color32::from_white_alpha((alpha * 255.0) as u8));
                }
            }
        }
        if let Some(t) = self.art.texture("squad") {
            let size = t.size_vec2();
            let h = stage_h;
            let w = h * size.x / size.y.max(1.0);
            let r = Rect::from_min_size(
                pos2(cx - w / 2.0 + near.x, stage_top - drift * 0.08 + near.y),
                vec2(w, h),
            );
            clip.image(t.id(), r, full, white);
        }
        // `.campaign__fade`: shade at the sides, where the stage meets the
        // softened backdrop.
        let side = |a: f32| bg().gamma_multiply(a);
        let w = rect.width();
        horizontal_band(&clip, rect, 0.0, 0.24 * w, side(0.6), side(0.0));
        horizontal_band(&clip, rect, 0.76 * w, w, side(0.0), side(0.6));
        // The banner's foot dissolves into the page beneath: opaque to 40 %,
        // half at 60 %, gone at 80 %.
        let y = |f: f32| rect.top() + banner_h * f;
        vertical_band(
            &clip,
            rect,
            y(0.4),
            y(0.6),
            bg().gamma_multiply(0.0),
            bg().gamma_multiply(0.5),
        );
        vertical_band(&clip, rect, y(0.6), y(0.8), bg().gamma_multiply(0.5), bg());
        clip.rect_filled(Rect::from_min_max(pos2(rect.left(), y(0.8)), rect.max), 0.0, bg());
    }

    /// `.campaign__list`: the operations, top to bottom, on a track.
    fn list(&mut self, ui: &mut egui::Ui, list: Rect, time: f32) -> Option<CampaignAction> {
        let state = self.state.clone();
        let length = state.as_ref().map_or(8, |s| if s.length == 0 { 8 } else { s.length });
        let mut action = None;
        let width = list.width() - 18.0 - 14.0;
        let wide = list.width() > 820.0;
        let rows: Vec<(u32, Option<CampaignMissionInfo>, MissionStatus)> = (1..=length)
            .map(|n| {
                let mission = state
                    .as_ref()
                    .and_then(|s| s.missions.iter().find(|m| m.number == n))
                    .cloned();
                (n, mission, mission_status(n, state.as_ref()))
            })
            .collect();
        let heights: Vec<f32> = rows
            .iter()
            .map(|(_, _, status)| row_height(width, wide, *status))
            .collect();
        let complete = self.complete();
        let finale_h = finale_height(width, wide, complete);
        let current_at = if self.preview_finale {
            Some(heights.iter().sum::<f32>() + list.height() / 2.0 - 20.0)
        } else {
            rows.iter()
                .position(|(_, _, s)| *s == MissionStatus::Current)
                .map(|i| heights[..i].iter().sum::<f32>() + heights[i] / 2.0)
        };
        let mut area = egui::ScrollArea::vertical()
            .id_salt("campaign-list")
            .auto_shrink([false, false]);
        if self.scroll_to_current
            && let Some(at) = current_at
        {
            self.scroll_to_current = false;
            self.shown_current = self.current_mission();
            area = area.vertical_scroll_offset((at - list.height() / 2.0).max(0.0));
        }
        let output = ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            area.show(ui, |ui| {
                let origin = ui.cursor().min;
                let mut y = origin.y + 6.0;
                let mut previous: Option<MissionStatus> = None;
                for ((number, mission, status), h) in rows.iter().zip(&heights) {
                    let row = Rect::from_min_size(pos2(origin.x, y), vec2(width, *h));
                    let first = *number == 1;
                    if let Some(a) = self.row(ui, row, wide, *number, mission.as_ref(), *status, previous, first) {
                        action = Some(a);
                    }
                    previous = Some(*status);
                    y += h;
                }
                let row = Rect::from_min_size(pos2(origin.x, y), vec2(width, finale_h));
                if let Some(a) = self.finale(ui, row, wide, complete, previous, time) {
                    action = Some(a);
                }
                y += finale_h;
                ui.allocate_rect(
                    Rect::from_min_max(origin, pos2(origin.x + width, y + 40.0)),
                    egui::Sense::hover(),
                );
            })
        });
        self.scroll = output.inner.state.clamped_offset().y;
        if let Some(CampaignAction::Start(m)) = &action {
            self.starting = Some(m.id.clone());
        }
        action
    }

    /// One `.campaign-card`: its node on the track, its card, and what it
    /// is with the way in.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        ui: &mut egui::Ui,
        row: Rect,
        wide: bool,
        number: u32,
        mission: Option<&CampaignMissionInfo>,
        status: MissionStatus,
        previous: Option<MissionStatus>,
        first: bool,
    ) -> Option<CampaignAction> {
        let painter = ui.painter().clone();
        let story = mission.and_then(|m| campaign_story(&m.id));
        let track_x = row.left() + 32.0;
        let body_h = row.height() - 26.0;
        let mid_y = row.top() + body_h / 2.0;
        // The track: the line into this node in the colour of the
        // operation before, and on to the next in its own.
        if !first {
            track_line(
                &painter,
                track_x,
                row.top(),
                mid_y - 13.0,
                previous.unwrap_or(status),
                true,
            );
        }
        // On to the next operation, or the finale under the last.
        track_line(&painter, track_x, mid_y - 13.0, row.bottom(), status, false);
        // The node.
        let (radius, fill, border, label_color) = match status {
            MissionStatus::Cleared => (17.0, cyan(), cyan(), ink()),
            MissionStatus::Current | MissionStatus::Open => (23.0, gold(), gold(), ink()),
            MissionStatus::Locked => (17.0, hsl(250.0, 40.0, 10.0), dim(), cream(0.6)),
        };
        if matches!(status, MissionStatus::Current | MissionStatus::Open) {
            for (r, a) in [(radius + 14.0, 0.08), (radius + 8.0, 0.16), (radius + 3.0, 0.3)] {
                painter.circle_filled(pos2(track_x, mid_y), r, hsla(46.0, 100.0, 57.0, a));
            }
        }
        painter.circle(pos2(track_x, mid_y), radius - 1.5, fill, Stroke::new(3.0, border));
        let label = match status {
            MissionStatus::Locked => String::new(),
            _ => format!("{number:02}"),
        };
        if status == MissionStatus::Cleared {
            // The browser's "✓", drawn: the fonts have no tick.
            let c = pos2(track_x, mid_y);
            painter.add(Shape::line(
                vec![c + vec2(-5.0, 0.5), c + vec2(-1.5, 4.0), c + vec2(5.5, -4.0)],
                Stroke::new(2.6, label_color),
            ));
        } else if !label.is_empty() {
            let g = text(&painter, &label, 13.0, display_family(), label_color, 0.0);
            let at = pos2(track_x - g.size().x / 2.0, mid_y - g.size().y / 2.0);
            painter.galley(at, g, label_color);
        }

        // The card: 46 % of the row beside its details, or the row's width
        // above them on a narrow screen.
        let art_left = row.left() + 64.0 + 26.0;
        let art_w = if wide {
            ((row.width() - 64.0) * 0.46).max(280.0)
        } else {
            row.right() - art_left
        };
        let art_h = art_w
            * if status == MissionStatus::Locked {
                7.0 / 32.0
            } else {
                9.0 / 16.0
            };
        let mut art = Rect::from_min_size(pos2(art_left, row.top()), vec2(art_w, art_h));
        let card = story.and_then(|s| s.card);
        // `makeReadable`: an open operation's card reads its story when
        // clicked.
        let mut action = None;
        if let (Some(card), Some(story), false) = (card, story, status == MissionStatus::Locked) {
            let response = ui.interact(art, egui::Id::unique(("campaign-read", number)), egui::Sense::click());
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn);
                art = art.translate(vec2(0.0, -3.0));
                painter.rect_filled(art.expand(10.0), 14.0, hsla(182.0, 95.0, 52.0, 0.08));
                painter.rect_filled(art.expand(4.0), 8.0, hsla(182.0, 95.0, 52.0, 0.14));
            }
            if response.clicked() {
                action = Some(CampaignAction::Read(card, story.title));
            }
        }
        self.card_art(&painter, art, number, card, status);

        let info = if wide {
            let left = art.right() + 26.0;
            Rect::from_min_max(pos2(left, row.top()), pos2(row.right(), row.top() + body_h))
        } else {
            Rect::from_min_max(
                pos2(art_left, art.bottom() + 14.0),
                pos2(row.right(), row.bottom() - 26.0),
            )
        };
        let start = self.info(ui, &painter, info, wide, number, mission, status);
        start.map(CampaignAction::Start).or(action)
    }

    /// `buildFinale`: under the last operation. Until every operation is
    /// cleared it is a classified strip, so the ending stays unseen; then
    /// its card, larger than the others and turning slowly the way the comic
    /// does before a match, under a CAMPAIGN COMPLETE plate.
    fn finale(
        &self,
        ui: &mut egui::Ui,
        row: Rect,
        wide: bool,
        complete: bool,
        previous: Option<MissionStatus>,
        time: f32,
    ) -> Option<CampaignAction> {
        let painter = ui.painter().clone();
        let track_x = row.left() + 32.0;
        let art_left = row.left() + 64.0 + 26.0;
        if !complete {
            let body_h = row.height() - 26.0;
            let mid_y = row.top() + body_h / 2.0;
            track_line(
                &painter,
                track_x,
                row.top(),
                mid_y - 13.0,
                previous.unwrap_or(MissionStatus::Locked),
                true,
            );
            painter.circle(
                pos2(track_x, mid_y),
                15.5,
                hsl(250.0, 40.0, 10.0),
                Stroke::new(3.0, dim()),
            );
            let art_w = if wide {
                ((row.width() - 64.0) * 0.46).max(280.0)
            } else {
                row.right() - art_left
            };
            let art = Rect::from_min_size(pos2(art_left, row.top()), vec2(art_w, art_w * 7.0 / 32.0));
            self.card_art(&painter, art, 0, None, MissionStatus::Locked);
            let info = if wide {
                Rect::from_min_max(
                    pos2(art.right() + 26.0, row.top()),
                    pos2(row.right(), row.top() + body_h),
                )
            } else {
                Rect::from_min_max(pos2(art_left, art.bottom() + 14.0), row.max)
            };
            let size = (screen_w(ui) * 0.014).clamp(16.0, 24.0);
            let name = text(&painter, "CLASSIFIED", size, display_family(), cream(0.45), 0.0);
            let hook = text(
                &painter,
                "Clear every operation to see how it ends.",
                14.0,
                body(),
                cream(0.45),
                0.0,
            );
            let total = name.size().y + 10.0 + hook.size().y;
            let y = if wide {
                info.center().y - total / 2.0
            } else {
                info.top()
            };
            let name_h = name.size().y;
            painter.galley(pos2(info.left(), y), name, cream(0.45));
            painter.galley(pos2(info.left(), y + name_h + 10.0), hook, cream(0.45));
            return None;
        }
        // `padding: 30px 0 60px`: the plate, then the card.
        let size = (screen_w(ui) * 0.034).clamp(32.0, 64.0);
        let plate = campaign_plate_in(
            &painter,
            pos2(art_left, row.top() + 30.0),
            &FINALE_TITLE.to_uppercase(),
            size,
            PlateLook::Deep,
        );
        let art_w = finale_art_width(row.width(), wide);
        let art = Rect::from_min_size(
            pos2(art_left, plate.bottom() + 22.0),
            vec2(art_w, art_w * FINALE_CARD.height / FINALE_CARD.width),
        );
        // The track runs into the finale's node, a star.
        let node = pos2(track_x, art.center().y);
        track_line(
            &painter,
            track_x,
            row.top(),
            node.y,
            previous.unwrap_or(MissionStatus::Cleared),
            true,
        );
        let r = 23.0 + 10.0;
        for (grow, a) in [(16.0, 0.12), (8.0, 0.25)] {
            painter.circle_filled(node, r + grow, hsla(46.0, 100.0, 57.0, a));
        }
        painter.circle(node, r - 1.5, gold(), Stroke::new(3.0, gold()));
        star(&painter, node, 11.0, ink());

        let response = ui.interact(art, egui::Id::unique("campaign-finale"), egui::Sense::click());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn);
        }
        // `campaign-finale-drift`: 14 s each way, eased; the turn in depth
        // (rotateX/Y) has no flat equivalent and is left out.
        let phase = (time / 14.0).rem_euclid(2.0);
        let f = if phase < 1.0 { phase } else { 2.0 - phase };
        let e = f * f * (3.0 - 2.0 * f);
        let key = |a: f32, b: f32, c: f32| {
            if e < 0.5 {
                a + (b - a) * e * 2.0
            } else {
                b + (c - b) * (e - 0.5) * 2.0
            }
        };
        let turn = key(-0.8, 0.2, 0.6).to_radians();
        let scale = key(1.0, 1.025, 1.045);
        let shift = vec2(key(0.0, 0.0, -8.0), key(0.0, 0.0, -5.0));
        let centre = art.center() + shift;
        let drift = Rect::from_center_size(centre, art.size() * scale);
        let corners: Vec<Pos2> = [
            drift.left_top(),
            drift.right_top(),
            drift.right_bottom(),
            drift.left_bottom(),
        ]
        .map(|p| rotate(p, centre, turn))
        .to_vec();
        // The glow: a soft gold and pink halo under the card.
        // `box-shadow`: the drop, the gold glow and the pink haze.
        for (offset, blur, colour) in [
            (22.0, 60.0, hsla(0.0, 0.0, 0.0, 0.6)),
            (0.0, 60.0, hsla(46.0, 100.0, 57.0, 0.3)),
            (0.0, 120.0, hsla(300.0, 90.0, 55.0, 0.18)),
        ] {
            painter.add(
                egui::epaint::Shadow {
                    offset: [0, offset as i8],
                    blur: blur as u8,
                    spread: 0,
                    color: colour,
                }
                .as_shape(drift, 0.0),
            );
        }
        painter.add(Shape::convex_polygon(
            corners.clone(),
            hsl(250.0, 12.0, 13.0),
            Stroke::NONE,
        ));
        if let Some(t) = self.art.texture("card9") {
            let mut mesh = egui::Mesh::with_texture(t.id());
            for (p, uv) in corners
                .iter()
                .zip([pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)])
            {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: *p,
                    uv,
                    color: Color32::WHITE,
                });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(mesh);
            // `campaign-finale-sheen`: light across the card's face, every
            // 9 s, still for the first 55 %.
            let t = (time / 9.0).fract();
            if t > 0.55 {
                let k = (t - 0.55) / 0.45;
                let k = k * k * (3.0 - 2.0 * k);
                let x = drift.left() + drift.width() * (-0.2 + 1.4 * k);
                let band = drift.width() * 0.12;
                let lean = drift.height() * (15f32).to_radians().tan();
                let clip = painter.with_clip_rect(drift.intersect(painter.clip_rect()));
                let light = hsla(45.0, 100.0, 95.0, 0.16);
                let clear = hsla(45.0, 100.0, 95.0, 0.0);
                let mut sheen = egui::Mesh::default();
                for (dx, c) in [(-band, clear), (0.0, light), (band, clear)] {
                    sheen.colored_vertex(pos2(x + dx + lean, drift.top()), c);
                    sheen.colored_vertex(pos2(x + dx, drift.bottom()), c);
                }
                for i in 0..2u32 {
                    let b = i * 2;
                    sheen.add_triangle(b, b + 1, b + 3);
                    sheen.add_triangle(b, b + 3, b + 2);
                }
                clip.add(sheen);
            }
        }
        painter.add(Shape::closed_line(corners, Stroke::new(4.0, gold())));
        response
            .clicked()
            .then_some(CampaignAction::Read(FINALE_CARD, FINALE_TITLE))
    }

    fn card_art(&self, painter: &Painter, art: Rect, number: u32, card: Option<CampaignCard>, status: MissionStatus) {
        let locked = status == MissionStatus::Locked;
        // The shadow, then the striped empty frame.
        if !locked {
            painter.rect_filled(
                art.translate(vec2(0.0, 10.0)).expand(4.0),
                4.0,
                hsla(0.0, 0.0, 0.0, 0.25),
            );
        }
        painter.rect_filled(art, 0.0, hsl(250.0, 12.0, 13.0));
        let art_clip = painter.with_clip_rect(art.intersect(painter.clip_rect()));
        let stripe = 12.0 * std::f32::consts::SQRT_2;
        let mut x = art.left() - art.height();
        let mut odd = false;
        while x < art.right() {
            if odd {
                art_clip.add(Shape::convex_polygon(
                    vec![
                        pos2(x, art.bottom()),
                        pos2(x + stripe, art.bottom()),
                        pos2(x + stripe + art.height(), art.top()),
                        pos2(x + art.height(), art.top()),
                    ],
                    hsl(250.0, 12.0, 15.0),
                    Stroke::NONE,
                ));
            }
            odd = !odd;
            x += stripe;
        }
        if let Some(card) = card {
            let name = if locked {
                format!("card{}-locked", card.number)
            } else {
                format!("card{}", card.number)
            };
            if let Some(t) = self.art.texture(&name) {
                let size = t.size_vec2();
                let uv = cover_uv([size.x, size.y], art.size(), card.thumb_focus);
                art_clip.image(t.id(), art, uv, Color32::WHITE);
            }
        }
        let stamp = if locked {
            Some("CLASSIFIED".to_owned())
        } else if card.is_none() {
            Some(format!("OPERATION {number:02}"))
        } else {
            None
        };
        if let Some(stamp) = stamp {
            let size = (art.width() * 0.035).clamp(14.0, 22.0);
            let g = text(painter, &stamp, size, display_family(), cream(0.55), size * 0.08);
            let r = Rect::from_center_size(art.center(), g.size() + vec2(28.0, 8.0));
            let turn = -6f32.to_radians();
            let corners =
                [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].map(|p| rotate(p, art.center(), turn));
            art_clip.add(Shape::closed_line(corners.to_vec(), Stroke::new(2.0, cream(0.5))));
            let mut shape = egui::epaint::TextShape::new(
                rotate(pos2(r.left() + 14.0, r.top() + 4.0), art.center(), turn),
                g,
                cream(0.55),
            );
            shape.angle = turn;
            art_clip.add(shape);
        }
        let border = match status {
            MissionStatus::Current => gold(),
            MissionStatus::Locked => hsla(250.0, 15.0, 60.0, 0.3),
            _ => hsla(182.0, 95.0, 52.0, 0.7),
        };
        painter.rect_stroke(art, 0.0, Stroke::new(3.0, border), egui::StrokeKind::Inside);
    }

    #[allow(clippy::too_many_arguments)]
    fn info(
        &self,
        ui: &mut egui::Ui,
        painter: &Painter,
        info: Rect,
        wide: bool,
        number: u32,
        mission: Option<&CampaignMissionInfo>,
        status: MissionStatus,
    ) -> Option<CampaignMissionInfo> {
        let screen_w = ui.ctx().content_rect().width();
        let (Some(mission), false) = (mission, status == MissionStatus::Locked) else {
            let size = (screen_w * 0.014).clamp(16.0, 24.0);
            let name = text(painter, "CLASSIFIED", size, display_family(), cream(0.45), 0.0);
            let hook = text(
                painter,
                &format!("Clear operation {:02} to unlock.", number.saturating_sub(1)),
                14.0,
                body(),
                cream(0.45),
                0.0,
            );
            let total = name.size().y + 10.0 + hook.size().y;
            let y = if wide {
                info.center().y - total / 2.0
            } else {
                info.top()
            };
            let name_h = name.size().y;
            painter.galley(pos2(info.left(), y), name, cream(0.45));
            painter.galley(pos2(info.left(), y + name_h + 10.0), hook, cream(0.45));
            return None;
        };
        let story = campaign_story(&mission.id);
        let title = story.map_or(mission.map_title.as_str(), |s| s.title).to_uppercase();
        let name_size = (screen_w * 0.02).clamp(22.0, 38.0);
        let name_text = elide(painter, &title, name_size, display_family(), info.width() - name_size);
        let name = text(painter, &name_text, name_size, display_family(), cream(1.0), 0.0);
        let name_plate_h = name.size().y + name_size * 0.26;
        let hook_size = (screen_w * 0.0095).clamp(15.0, 18.0);
        let hook = story.map(|s| {
            let mut job = egui::text::LayoutJob::simple(
                s.intro.to_owned(),
                FontId::new(hook_size, body()),
                cream(0.8),
                info.width().min(hook_size * 0.5 * 54.0),
            );
            job.wrap.break_anywhere = false;
            painter.fonts_mut(|f| f.layout_job(job))
        });
        let mode = text(painter, gametype_name(&mission.gametype), 16.0, body(), cream(1.0), 0.0);
        let clear = self
            .state
            .as_ref()
            .and_then(|s| s.clears.iter().find(|c| c.mission_id == mission.id));
        let key_h = 52.0;
        let mut total = name_plate_h + 4.0 + 10.0 + mode.size().y + 10.0 + 6.0 + key_h;
        if let Some(h) = &hook {
            total += h.size().y + 10.0;
        }
        let mut y = if wide {
            info.center().y - total / 2.0
        } else {
            info.top()
        };
        drop(name);
        let plate = campaign_plate_in(painter, pos2(info.left(), y), &name_text, name_size, PlateLook::Dark);
        y = plate.bottom() + 4.0 + 10.0;
        if let Some(hook) = hook {
            let h = hook.size().y;
            painter.galley(pos2(info.left(), y), hook, cream(0.8));
            y += h + 10.0;
        }
        // The one thing worth a bullet: which game mode the operation is.
        let mode_h = mode.size().y;
        painter.rect_filled(
            Rect::from_center_size(pos2(info.left() + 4.5, y + mode_h / 2.0), vec2(9.0, 9.0)),
            0.0,
            pink(),
        );
        painter.galley(pos2(info.left() + 19.0, y), mode, cream(1.0));
        y += mode_h + 10.0 + 6.0;

        let mut x = info.left();
        if let Some(clear) = clear {
            x += stats(painter, pos2(x, y + key_h / 2.0), clear) + 28.0;
        }
        let cleared = status == MissionStatus::Cleared;
        let starting = self.starting.as_deref() == Some(mission.id.as_str());
        let label = if starting {
            "Starting…"
        } else if cleared {
            "Replay"
        } else {
            "Play"
        };
        let key_w = (info.right() - x).clamp(120.0, 320.0);
        let rect = Rect::from_min_size(pos2(x, y), vec2(key_w, key_h));
        let look = if cleared {
            KeyLook::cream().sm()
        } else {
            KeyLook::gold().sm()
        };
        let face = RaisedKey::new(("campaign", mission.id.as_str()), rect, look.clone())
            .hover(look.hovered(), -0.35)
            .disabled(!self.startable)
            .show(ui);
        // The ▶ and ↻ before the label, drawn: the fonts have neither.
        let icon = !starting;
        KeyText::new(&label.to_uppercase(), "", display_family(), body())
            .size(17.0, 13.0)
            .pad(if icon { 46.0 } else { 22.0 }, 18.0)
            .layout(KeyTextLayout::Row)
            .paint(painter, &face);
        if icon {
            key_icon(
                painter,
                pos2(face.rect.left() + 30.0, face.rect.center().y),
                cleared,
                face.text,
            );
        }
        if face.hovered && !self.startable && !self.reason.is_empty() {
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::NULL.with("campaign-reason"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| ui.label(&self.reason));
        }
        (face.clicked && self.startable).then(|| mission.clone())
    }
}

/// The key's mark: ▶ to play, ↻ to replay, 14 px across at `centre`.
fn key_icon(painter: &Painter, centre: Pos2, replay: bool, color: Color32) {
    if replay {
        let r = 6.5;
        let points: Vec<Pos2> = (0..=20)
            .map(|i| {
                let a = (-60.0 + 280.0 * i as f32 / 20.0).to_radians();
                centre + vec2(a.cos(), a.sin()) * r
            })
            .collect();
        let end = *points.last().unwrap_or(&centre);
        painter.add(Shape::line(points, Stroke::new(2.4, color)));
        // The arrowhead at the arc's end, pointing on round.
        let a = 220f32.to_radians();
        let along = vec2(-a.sin(), a.cos());
        let out = vec2(a.cos(), a.sin());
        painter.add(Shape::convex_polygon(
            vec![
                end + along * 4.5,
                end + out * 4.0 - along * 1.5,
                end - out * 4.0 - along * 1.5,
            ],
            color,
            Stroke::NONE,
        ));
    } else {
        painter.add(Shape::convex_polygon(
            vec![
                centre + vec2(-5.0, -7.0),
                centre + vec2(8.0, 0.0),
                centre + vec2(-5.0, 7.0),
            ],
            color,
            Stroke::NONE,
        ));
    }
}

/// `buildStats`: the best a cleared operation has gone. Returns its width.
fn stats(painter: &Painter, at: Pos2, clear: &CampaignClear) -> f32 {
    let label = text(painter, "MOST FRAGS", 11.0, body(), cream(0.55), 11.0 * 0.18);
    let value = text(
        painter,
        &clear.best_frags.to_string(),
        22.0,
        display_family(),
        cream(1.0),
        0.0,
    );
    let total = label.size().y + 4.0 + value.size().y;
    let width = label.size().x.max(value.size().x);
    let top = at.y - total / 2.0;
    let label_h = label.size().y;
    painter.galley(pos2(at.x, top), label, cream(0.55));
    painter.galley(pos2(at.x, top + label_h + 4.0), value, cream(1.0));
    width
}

/// A row's height: its card (or a locked strip) and the 26 px under it,
/// or the card above its details on a narrow screen.
fn row_height(width: f32, wide: bool, status: MissionStatus) -> f32 {
    let art_w = if wide {
        ((width - 64.0) * 0.46).max(280.0)
    } else {
        width - 64.0 - 26.0
    };
    let art_h = art_w
        * if status == MissionStatus::Locked {
            7.0 / 32.0
        } else {
            9.0 / 16.0
        };
    // The story so far runs to five lines or so beside the card.
    let info_h = if status == MissionStatus::Locked { 60.0 } else { 300.0 };
    if wide {
        art_h.max(info_h) + 26.0
    } else {
        art_h + 14.0 + info_h + 26.0
    }
}

/// `.campaign-finale__art`: aligned with the cards above, half as wide
/// again (`min(100%, 1560px)`, at most 46 % × 1.6 of the row).
fn finale_art_width(width: f32, wide: bool) -> f32 {
    let avail = width - 64.0 - 26.0;
    if wide {
        avail.min(1560.0).min((width - 64.0) * 0.46 * 1.6)
    } else {
        avail
    }
}

/// The finale's height: a locked strip, or its plate and card.
fn finale_height(width: f32, wide: bool, complete: bool) -> f32 {
    if !complete {
        return row_height(width, wide, MissionStatus::Locked);
    }
    let art_w = finale_art_width(width, wide);
    // The plate at its largest, the gap, the card, and `60px` under it.
    30.0 + 64.0 * 1.4 + 22.0 + art_w * FINALE_CARD.height / FINALE_CARD.width + 60.0
}

/// `★`, drawn: the fonts lack it.
fn star(painter: &Painter, centre: Pos2, r: f32, colour: Color32) {
    let points: Vec<Pos2> = (0..10)
        .map(|i| {
            let a = (-90.0 + 36.0 * i as f32).to_radians();
            let radius = if i % 2 == 0 { r } else { r * 0.42 };
            centre + vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    // Not convex: a fan from the centre.
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, colour);
    for p in &points {
        mesh.colored_vertex(*p, colour);
    }
    for i in 0..10u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
    }
    painter.add(mesh);
}

/// One stretch of the track, coloured by `status` (the row's own, or the
/// one before for the line into a node).
fn track_line(painter: &Painter, x: f32, top: f32, bottom: f32, status: MissionStatus, into: bool) {
    if bottom <= top {
        return;
    }
    let rect = Rect::from_min_max(pos2(x - 2.0, top), pos2(x + 2.0, bottom));
    match status {
        MissionStatus::Cleared => {
            painter.rect_filled(rect.expand2(vec2(4.0, 0.0)), 4.0, hsla(182.0, 95.0, 52.0, 0.15));
            painter.rect_filled(rect, 0.0, cyan());
        }
        MissionStatus::Current | MissionStatus::Open if !into => {
            let corners = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ];
            gradient_polygon_in(painter, &corners, top, bottom, &[(0.0, gold()), (1.0, dim())]);
        }
        MissionStatus::Locked => {
            let mut y = top;
            while y < bottom {
                let end = (y + 10.0).min(bottom);
                painter.rect_filled(
                    Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), end)),
                    0.0,
                    dim(),
                );
                y += 18.0;
            }
        }
        _ => {
            painter.rect_filled(rect, 0.0, dim());
        }
    }
}

/// Which stamp a `.campaign-plate` wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlateLook {
    /// The game's yellow banner stamp: the page heading and the comic
    /// card's title.
    Gold,
    /// `--dark`: a purple a shade off the page's, for the operations'
    /// titles and the count.
    Dark,
    /// `--deep`: deep purple, for the finale.
    Deep,
}

impl PlateLook {
    fn ink(self) -> Color32 {
        match self {
            Self::Gold => ink(),
            Self::Dark => cream(1.0),
            Self::Deep => gold(),
        }
    }

    /// The printed offset copy under the words.
    fn text_shadow(self) -> Color32 {
        match self {
            Self::Gold => hsla(48.0, 100.0, 80.0, 0.55),
            Self::Dark => hsla(322.0, 95.0, 40.0, 0.6),
            Self::Deep => hsla(322.0, 95.0, 40.0, 0.7),
        }
    }

    fn stops(self) -> [(f32, Color32); 3] {
        match self {
            Self::Gold => [
                (0.0, hsl(52.0, 100.0, 66.0)),
                (0.45, hsl(46.0, 100.0, 57.0)),
                (1.0, hsl(40.0, 98.0, 47.0)),
            ],
            Self::Dark => [
                (0.0, hsl(278.0, 56.0, 27.0)),
                (0.48, hsl(270.0, 54.0, 20.0)),
                (1.0, hsl(262.0, 52.0, 15.0)),
            ],
            Self::Deep => [
                (0.0, hsl(284.0, 72.0, 30.0)),
                (0.45, hsl(274.0, 74.0, 20.0)),
                (1.0, hsl(264.0, 78.0, 12.0)),
            ],
        }
    }
}

/// The stamped plate itself over `rect`, with its printed shadow.
pub fn plate_behind(painter: &Painter, rect: Rect, look: PlateLook) {
    let skew = (6f32).to_radians().tan();
    let shape = |r: Rect| -> Vec<Pos2> {
        [(0.02, 0.06), (0.98, 0.0), (1.0, 0.84), (0.55, 1.0), (0.0, 0.9)]
            .iter()
            .map(|&(fx, fy)| {
                let p = pos2(r.left() + r.width() * fx, r.top() + r.height() * fy);
                // `skewX(-6deg)` about the plate's centre.
                pos2(p.x - (p.y - r.center().y) * skew, p.y)
            })
            .collect()
    };
    painter.add(Shape::convex_polygon(
        shape(rect.translate(vec2(6.0, 7.0))),
        hsl(250.0, 50.0, 3.0),
        Stroke::NONE,
    ));
    gradient_polygon_x(painter, &shape(rect), &look.stops());
    if look != PlateLook::Gold {
        // The scanlines of the purple stamps.
        let clip = painter.with_clip_rect(rect.shrink2(vec2(rect.height() * skew, 0.0)));
        let mut y = rect.top();
        while y < rect.bottom() {
            clip.rect_filled(
                Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), y + 2.0)),
                0.0,
                hsla(276.0, 70.0, 70.0, 0.07),
            );
            y += 5.0;
        }
    }
}

/// `.campaign-plate`: a stamped yellow plate behind `label`, with its
/// printed shadow, at `at` (its top left). Returns the plate.
pub fn campaign_plate(painter: &Painter, at: Pos2, label: &str, size: f32) -> Rect {
    campaign_plate_in(painter, at, label, size, PlateLook::Gold)
}

/// A `.campaign-plate` in `look`.
pub fn campaign_plate_in(painter: &Painter, at: Pos2, label: &str, size: f32, look: PlateLook) -> Rect {
    let g = text(painter, label, size, display_family(), look.ink(), 0.0);
    let pad = (0.4 * size, 0.55 * size, 0.1 * size, 0.16 * size);
    let rect = Rect::from_min_size(at, vec2(g.size().x + pad.0 + pad.1, g.size().y + pad.2 + pad.3));
    plate_behind(painter, rect, look);
    let shadow = text(painter, label, size, display_family(), look.text_shadow(), 0.0);
    let at = pos2(rect.left() + pad.0, rect.top() + pad.2);
    painter.galley(at + vec2(0.05 * size, 0.06 * size), shadow, look.text_shadow());
    painter.galley(at, g, look.ink());
    rect
}

/// `.campaign__ingress`: `words` in a cream narration box with typewriter
/// letters, like the comic cards' captions, tilted a little, `width` wide
/// at most from `at`. Returns its box, untilted.
fn caption_box(painter: &Painter, at: Pos2, words: &str, size: f32, width: f32) -> Rect {
    let colour = hsl(250.0, 40.0, 10.0);
    let mut job = egui::text::LayoutJob::simple(
        words.to_owned(),
        FontId::new(size, FontFamily::Monospace),
        colour,
        (width - 36.0).max(120.0),
    );
    job.wrap.break_anywhere = false;
    let g = painter.fonts_mut(|f| f.layout_job(job));
    let rect = Rect::from_min_size(at, g.size() + vec2(36.0, 25.0));
    let turn = (-0.6f32).to_radians();
    let corners = |r: Rect| -> Vec<Pos2> {
        [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
            .map(|p| rotate(p, rect.center(), turn))
            .to_vec()
    };
    painter.add(Shape::convex_polygon(
        corners(rect.translate(vec2(6.0, 7.0))),
        hsl(250.0, 50.0, 3.0),
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        corners(rect),
        hsl(44.0, 55.0, 90.0),
        Stroke::new(3.0, hsl(250.0, 50.0, 5.0)),
    ));
    let mut shape = egui::epaint::TextShape::new(rotate(rect.min + vec2(18.0, 12.0), rect.center(), turn), g, colour);
    shape.angle = turn;
    painter.add(shape);
    rect
}

/// The part `part` of a picture drawn whole over `whole`.
fn image_part(painter: &Painter, texture: egui::TextureId, whole: Rect, part: Rect, tint: Color32) {
    let uv = Rect::from_min_max(
        pos2(
            (part.left() - whole.left()) / whole.width(),
            (part.top() - whole.top()) / whole.height(),
        ),
        pos2(
            (part.right() - whole.left()) / whole.width(),
            (part.bottom() - whole.top()) / whole.height(),
        ),
    );
    painter.image(texture, part, uv, tint);
}

/// A band across `rect` from `x0` to `x1` (offsets from its left), its
/// colour going from `from` to `to`.
fn horizontal_band(painter: &Painter, rect: Rect, x0: f32, x1: f32, from: Color32, to: Color32) {
    let (l, r) = (rect.left() + x0, rect.left() + x1);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(pos2(l, rect.top()), from);
    mesh.colored_vertex(pos2(r, rect.top()), to);
    mesh.colored_vertex(pos2(r, rect.bottom()), to);
    mesh.colored_vertex(pos2(l, rect.bottom()), from);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// A band across `rect` from `y0` to `y1`, its colour going from `from` to
/// `to`.
fn vertical_band(painter: &Painter, rect: Rect, y0: f32, y1: f32, from: Color32, to: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(pos2(rect.left(), y0), from);
    mesh.colored_vertex(pos2(rect.right(), y0), from);
    mesh.colored_vertex(pos2(rect.right(), y1), to);
    mesh.colored_vertex(pos2(rect.left(), y1), to);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

fn rotate(p: Pos2, about: Pos2, angle: f32) -> Pos2 {
    let (s, c) = angle.sin_cos();
    let d = p - about;
    about + vec2(d.x * c - d.y * s, d.x * s + d.y * c)
}

/// `campaign-handheld-far` and `-near`: where the street and the squad
/// have wandered at `time` seconds, the squad a quarter as far.
pub(crate) fn handheld(time: f32) -> (egui::Vec2, egui::Vec2) {
    const KEYS: [(f32, f32, f32); 6] = [
        (0.0, 0.0, 0.0),
        (0.21, 16.0, -2.0),
        (0.43, 6.0, 2.0),
        (0.64, -18.0, -1.0),
        (0.83, -7.0, 2.0),
        (1.0, 0.0, 0.0),
    ];
    let t = (time / 17.0).fract();
    let at = KEYS.windows(2).find(|k| t <= k[1].0).unwrap_or(&KEYS[4..6]);
    let (a, b) = (at[0], at[1]);
    let f = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    // ease-in-out between keys.
    let e = f * f * (3.0 - 2.0 * f);
    let far = vec2(a.1 + (b.1 - a.1) * e, a.2 + (b.2 - a.2) * e);
    (far, far * 0.25)
}

/// The art for tests and screenshots, decoded on this thread.
#[must_use]
pub fn decode_now() -> Vec<(String, Arc<egui::ColorImage>)> {
    let (send, receive) = channel();
    decode_all(&send);
    drop(send);
    receive.iter().map(|(n, i)| (n, Arc::new(i))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(unlocked: u32, cleared: &[&str]) -> CampaignProgress {
        let missions = ["a", "b", "c"]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                serde_json::from_value(serde_json::json!({
                    "id": id, "number": i + 1, "map": "m", "gametype": "duel"
                }))
                .unwrap()
            })
            .collect();
        CampaignProgress {
            length: 8,
            missions,
            clears: cleared
                .iter()
                .map(|id| serde_json::from_value(serde_json::json!({"missionId": id, "bestFrags": 3})).unwrap())
                .collect(),
            unlocked_through: unlocked,
            completed_at: None,
        }
    }

    #[test]
    fn a_mission_stands_cleared_current_open_or_locked() {
        assert_eq!(mission_status(1, None), MissionStatus::Current);
        assert_eq!(mission_status(2, None), MissionStatus::Locked);
        let s = progress(2, &["a"]);
        assert_eq!(mission_status(1, Some(&s)), MissionStatus::Cleared);
        assert_eq!(mission_status(2, Some(&s)), MissionStatus::Current);
        assert_eq!(mission_status(3, Some(&s)), MissionStatus::Locked);
        // Past the written missions.
        assert_eq!(mission_status(5, Some(&s)), MissionStatus::Locked);
        // Taken out of order: the first not cleared is current, the next open.
        let s = progress(3, &["b"]);
        assert_eq!(mission_status(1, Some(&s)), MissionStatus::Current);
        assert_eq!(mission_status(3, Some(&s)), MissionStatus::Open);
    }

    #[test]
    fn a_cover_crop_keeps_the_focus() {
        let uv = cover_uv([1600.0, 900.0], vec2(320.0, 320.0), (0.78, 0.3));
        assert!((uv.height() - 1.0).abs() < 1e-6);
        assert!((uv.width() - 0.5625).abs() < 1e-6);
        assert!((uv.left() - 0.4375 * 0.78).abs() < 1e-6);
    }

    #[test]
    fn the_art_decodes() {
        let art = decode_now();
        assert_eq!(art.len(), 2 + 1 + 9 * 3);
        let card = art.iter().find(|(n, _)| n == "card1").unwrap();
        assert_eq!(card.1.size, [1536, 864]);
    }

    /// Takes card 1 into `ctx`, decoding it if it is not there yet.
    fn card_in(art: &mut CampaignArt, ctx: &egui::Context) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            art.want(1);
            art.poll(ctx);
            if art.texture("card1").is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn the_art_comes_back_in_a_new_context() {
        // The lobby's window after a match is a new egui context: a texture
        // from the old one would draw nothing in it.
        let mut art = CampaignArt::default();
        let (lobby, after_match) = (egui::Context::default(), egui::Context::default());
        assert!(card_in(&mut art, &lobby));
        art.poll(&after_match);
        assert!(art.texture("card1").is_none(), "the old context's texture is let go");
        assert!(card_in(&mut art, &after_match), "and the card is decoded again");
    }
}
