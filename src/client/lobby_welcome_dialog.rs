//! wash-fps `client/LobbyWelcomeDialog.ts` and `styles/LobbyWelcomeDialog.css`:
//! the welcome guide, three steps on a gold card. The terms of service to
//! agree to, the keys as they are bound, and a tip of the day. Until the
//! terms are agreed the guide stays on its first step and cannot be closed.
//!
//! The card itself (the dimmed lobby behind it, the header, the footer and
//! its ink keys, a scrolling body) is here too: the credits dialog takes it
//! as the web one takes this file's CSS.

use std::sync::Arc;

use shagger::ui::components::lobby_twin::{clamp_css, cream, gradient_polygon, gradient_polygon_in, hsl, hsla, text};
use shagger::ui::components::raised_key::{KeyLook, KeyText, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use crate::client::credits::markdown_lite::{self, Block, Reading, render_markdown};
use crate::client::fps_actions::fps;
use crate::client::identity_frame::glow;
use crate::client::lobby_preferences::{read_preference, write_preference};
use crate::client::settings::controls_page::ControlsPage;
use crate::client::settings::settings_fields::body;
use crate::client::settings::settings_model::{SETTINGS_ACTIONS, codes_for, input_name};
use crate::client::washoi_style::display_family;
use crate::simulation::movement_profile::DEFAULT_FPS_MOVEMENT_CONFIG;

/// The terms, as the web build bundles them.
pub const TERMS: &str = include_str!("../../assets/docs/TERMS.md");
/// `WELCOME_PREFERENCE`.
pub const WELCOME_PREFERENCE: &str = "washoi.welcome.hideAutomatically";
/// `TERMS_PREFERENCE`: holds the terms' version the player agreed to.
pub const TERMS_PREFERENCE: &str = "washoi.terms.agreed";

/// `TERMS_VERSION`: the terms' "Last Updated" date; new terms ask again.
#[must_use]
pub fn terms_version() -> String {
    markdown_lite::last_updated(TERMS)
}

/// `termsAgreed`.
#[must_use]
pub fn terms_agreed() -> bool {
    read_preference(TERMS_PREFERENCE).is_some_and(|v| v == terms_version())
}

/// `welcomeIsHidden`: the guide opens by itself unless the player turned
/// that off, and never before they agreed to the terms.
#[must_use]
pub fn welcome_is_hidden() -> bool {
    terms_agreed() && read_preference(WELCOME_PREFERENCE).as_deref() == Some("true")
}

/// `--ink` at `a`.
fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 7.0, a)
}

/// `--gold-text`.
fn gold_text() -> Color32 {
    hsl(46.0, 100.0, 60.0)
}

// ── The card ────────────────────────────────────────────────────────────

/// `.lobby-guide-background` and `.lobby-guide`'s veil: the lobby behind,
/// darkened (`brightness(0.48)` under `#09050b70`; egui has no blur). Takes
/// the pointer, so nothing beneath answers; true when the veil itself was
/// clicked (the credits close on that).
pub fn veil(ui: &mut egui::Ui, screen: Rect, id: &str, card: Rect) -> bool {
    let response = ui.interact(screen, egui::Id::NULL.with(("guide-veil", id)), Sense::click());
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(5, 3, 6, 186));
    response.clicked() && response.interact_pointer_pos().is_some_and(|p| !card.contains(p))
}

/// The card's place: `min(max_w, 100%)` inside the gutters, centred; `fill`
/// takes the window's height (the guide's `--wizard` card, and a long read),
/// `min(max_h, ...)`.
#[must_use]
pub fn card_rect(screen: Rect, max_w: f32, max_h: f32) -> Rect {
    let gy = clamp_css(10.0, screen.height() * 0.025, 28.0);
    let gx = clamp_css(12.0, screen.width() * 0.03, 44.0);
    let w = max_w.min(screen.width() - 2.0 * gx);
    // Less the card's 10px base.
    let h = max_h.min(screen.height() - 2.0 * gy - 10.0);
    Rect::from_center_size(screen.center() - vec2(0.0, 5.0), vec2(w, h))
}

/// `.lobby-guide__card`: gold, lit from its upper left, a violet edge on its
/// left and a darker base under it. Returns the content's rect (inside the
/// padding).
#[must_use]
pub fn paint_card(painter: &Painter, card: Rect, screen_w: f32) -> Rect {
    // `0 24px 70px #0009`.
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 24],
            blur: 70,
            spread: 0,
            color: Color32::from_black_alpha(150),
        }
        .as_shape(card, 4),
    );
    // `0 10px 0 hsl(36 85% 28%)`.
    painter.rect_filled(card.translate(vec2(0.0, 10.0)), 4.0, hsl(36.0, 85.0, 28.0));
    let shape = [
        card.left_top(),
        card.right_top(),
        card.right_bottom(),
        card.left_bottom(),
    ];
    painter.rect_filled(card, 4.0, hsl(44.0, 98.0, 52.0));
    gradient_polygon(
        painter,
        &shape,
        &[(0.0, hsl(46.0, 100.0, 57.0)), (1.0, hsl(41.0, 96.0, 48.0))],
    );
    // `radial-gradient(120% 55% at 25% 0%, hsla(54, 100%, 78%, 0.55), transparent 62%)`.
    glow(
        &painter.with_clip_rect(card),
        pos2(card.left() + card.width() * 0.25, card.top()),
        vec2(card.width() * 1.2 * 0.62, card.height() * 0.55 * 0.62),
        hsla(54.0, 100.0, 78.0, 0.55),
        &shape,
    );
    // `inset 0 2px 0 #ffffff60` and `border-left: 6px solid var(--violet)`.
    painter.rect_filled(
        Rect::from_min_size(card.min, vec2(card.width(), 2.0)),
        0.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, 96),
    );
    painter.rect_filled(
        Rect::from_min_size(card.min, vec2(6.0, card.height())),
        egui::CornerRadius {
            nw: 4,
            sw: 4,
            ..Default::default()
        },
        hsl(276.0, 90.0, 64.0),
    );
    let pad = clamp_css(20.0, screen_w * 0.03, 38.0);
    Rect::from_min_max(card.min + vec2(6.0 + pad, pad), card.max - vec2(pad, pad))
}

/// `.lobby-guide__header`: the eyebrow and the title; returns the bottom
/// after the title's margin.
#[must_use]
pub fn header(painter: &Painter, inner: Rect, screen_w: f32, eyebrow: &str, title: &str, below: f32) -> f32 {
    let e = text(painter, eyebrow, 14.0, display_family(), ink(0.7), 14.0 * 0.17);
    let eh = e.size().y;
    painter.galley(inner.min, e, Color32::WHITE);
    let size = clamp_css(26.0, screen_w * 0.035, 48.0);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = inner.width();
    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: FontId::new(size, display_family()),
            color: ink(1.0),
            extra_letter_spacing: -0.04 * size,
            line_height: Some(size * 1.1),
            ..Default::default()
        },
    );
    let t = painter.layout_job(job);
    let top = inner.top() + eh + 10.0;
    let th = t.size().y;
    painter.galley(pos2(inner.left(), top), t, Color32::WHITE);
    top + th + below
}

/// `.lobby-guide__footer`'s height: its rule, padding and a key on its base.
pub const FOOTER_H: f32 = 2.0 + 20.0 + KEY_H + 6.0;
/// A footer key's face (`.raised-key`'s 60px).
const KEY_H: f32 = 60.0;

/// The footer's rule along `inner`'s foot; returns the keys' top.
#[must_use]
pub fn footer(painter: &Painter, inner: Rect) -> f32 {
    let top = inner.bottom() - FOOTER_H;
    painter.rect_filled(
        Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), 2.0)),
        0.0,
        ink(0.2),
    );
    top + 22.0
}

/// `raised-key--ink raised-key--inline`'s width for `label`.
#[must_use]
pub fn ink_key_width(painter: &Painter, label: &str, min_w: f32) -> f32 {
    let g = text(
        painter,
        &label.to_uppercase(),
        20.0,
        display_family(),
        Color32::WHITE,
        20.0 * 0.07,
    );
    (g.size().x + 26.0 + 22.0).max(min_w)
}

/// An ink key at `left`, `top`, `width` wide. Disabled, its face stays and
/// its lettering fades. True when pressed.
pub fn ink_key(ui: &mut egui::Ui, id: &str, left: f32, top: f32, width: f32, label: &str, disabled: bool) -> bool {
    let rect = Rect::from_min_size(pos2(left, top), vec2(width, KEY_H));
    let face = RaisedKey::new(("guide-key", id), rect, KeyLook::ink())
        .disabled(disabled)
        .show(ui);
    let painter = ui.painter().clone();
    let mut label_face = face.clone();
    if disabled {
        label_face.text = label_face.text.gamma_multiply(0.4);
    }
    KeyText::new(label, "", display_family(), body())
        .size(20.0, 13.0)
        .spacing(0.07)
        .pad(26.0, 22.0)
        .paint(&painter, &label_face);
    face.clicked && !disabled
}

/// A scrolling body in `rect`: `add` paints from the top it is given, `width`
/// wide, and returns its bottom; the scrollbar is the ink's
/// (`scrollbar-color`). `reset` scrolls back to the top.
pub fn scroll_body(
    ui: &mut egui::Ui,
    rect: Rect,
    id: &str,
    reset: bool,
    add: impl FnOnce(&mut egui::Ui, Pos2, f32) -> f32,
) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        let v = ui.visuals_mut();
        v.widgets.inactive.bg_fill = ink(0.4);
        v.widgets.hovered.bg_fill = ink(0.55);
        v.widgets.active.bg_fill = ink(0.7);
        v.extreme_bg_color = Color32::TRANSPARENT;
        let mut area = egui::ScrollArea::vertical()
            .id_salt(("guide-scroll", id))
            .auto_shrink([false, false]);
        if reset {
            area = area.vertical_scroll_offset(0.0);
        }
        area.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let top = ui.cursor().min;
            let width = ui.available_width() - 14.0;
            let bottom = add(ui, top, width);
            ui.allocate_rect(
                Rect::from_min_max(top, pos2(top.x + width, bottom.max(top.y))),
                Sense::hover(),
            );
        });
    });
}

/// `.lobby-guide-help` and `.lobby-guide-back`: the dark key of the links
/// bar, its label in the display face. True when pressed.
pub fn dark_key(ui: &mut egui::Ui, id: &str, r: Rect, label: &str, size: f32) -> bool {
    let (clicked, face) = dark_key_face(ui, id, r, false);
    let g = text(
        ui.painter(),
        label,
        size,
        display_family(),
        Color32::from_rgb(0xff, 0xf6, 0xdf),
        0.0,
    );
    ui.painter().galley(face.center() - g.size() / 2.0, g, Color32::WHITE);
    clicked
}

/// The dark key without its label: true when pressed, and the face to draw
/// the label on. `down` holds it sunk onto its base, darker (a toggle that
/// is on, as `.lobby-music-key[aria-pressed="true"]`).
pub fn dark_key_face(ui: &mut egui::Ui, id: &str, r: Rect, down: bool) -> (bool, Rect) {
    let response = ui.interact(
        r.expand2(vec2(0.0, 5.0)),
        egui::Id::NULL.with(("dark-key", id)),
        Sense::click(),
    );
    let painter = ui.painter().clone();
    let held = response.is_pointer_button_down_on() || down;
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let sink = if held { 4.0 } else { 0.0 };
    painter.rect_filled(
        r.translate(vec2(0.0, 9.0 - sink)).expand(1.0),
        4.0,
        Color32::from_black_alpha(if held { 40 } else { 64 }),
    );
    painter.rect_filled(r.translate(vec2(0.0, 5.0)), 4.0, Color32::from_rgb(0x0d, 0x0b, 0x16));
    let face = r.translate(vec2(0.0, sink));
    let (top, foot) = if down {
        (Color32::from_rgb(0x22, 0x1f, 0x31), Color32::from_rgb(0x12, 0x0f, 0x1c))
    } else if hovered {
        (Color32::from_rgb(0x57, 0x4b, 0x6d), Color32::from_rgb(0x28, 0x21, 0x37))
    } else {
        (Color32::from_rgb(0x39, 0x35, 0x4b), Color32::from_rgb(0x19, 0x16, 0x26))
    };
    painter.rect_filled(face, 4.0, foot);
    gradient_polygon(
        &painter,
        &[
            face.left_top() + vec2(2.0, 0.0),
            face.right_top() - vec2(2.0, 0.0),
            face.right_bottom() - vec2(2.0, 2.0),
            face.left_bottom() + vec2(2.0, -2.0),
        ],
        &[(0.0, top), (1.0, foot)],
    );
    painter.hline(
        (face.left() + 3.0)..=(face.right() - 3.0),
        face.top() + 0.5,
        Stroke::new(
            1.0,
            if down {
                Color32::from_black_alpha(136)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 53)
            },
        ),
    );
    (response.clicked(), face)
}

/// `« Back to welcome`'s size: padding 12 18 around 17px.
#[must_use]
pub fn back_key_size(painter: &Painter) -> egui::Vec2 {
    let g = text(painter, BACK_LABEL, 17.0, display_family(), Color32::WHITE, 0.0);
    g.size() + vec2(36.0 + 14.0, 24.0)
}

/// The back key's label; the chevrons are drawn beside it.
pub const BACK_LABEL: &str = "Back to welcome";

/// `.lobby-guide-back`, with its `«` drawn (the display face has none).
pub fn back_key(ui: &mut egui::Ui, at: Pos2) -> bool {
    let painter = ui.painter().clone();
    let r = Rect::from_min_size(at, back_key_size(&painter));
    let clicked = dark_key(ui, "back", r, "", 17.0);
    let g = text(
        &painter,
        BACK_LABEL,
        17.0,
        display_family(),
        Color32::from_rgb(0xff, 0xf6, 0xdf),
        0.0,
    );
    let x = r.left() + 18.0;
    let mid = r.center().y;
    let s = Stroke::new(2.2, Color32::from_rgb(0xff, 0xf6, 0xdf));
    for dx in [0.0, 6.0] {
        painter.add(Shape::line(
            vec![
                pos2(x + dx + 5.0, mid - 5.0),
                pos2(x + dx, mid),
                pos2(x + dx + 5.0, mid + 5.0),
            ],
            s,
        ));
    }
    painter.galley(pos2(x + 14.0 + 4.0, mid - g.size().y / 2.0), g, Color32::WHITE);
    clicked
}

// ── The guide ───────────────────────────────────────────────────────────

/// What the guide asks of the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideAnswer {
    /// `onClose`.
    Close,
    /// `onSetup`: to the keyboard setup, with a key back.
    Setup,
    /// A link in the terms, for the system's browser.
    Link,
}

/// The step names.
const STEPS: [&str; 3] = ["Terms of service", "Your keys", "Tip of the day"];

/// `LobbyWelcomeDialog`.
#[derive(Debug)]
pub struct LobbyWelcomeDialog {
    pub tab: usize,
    /// Until the terms are agreed to, the guide stays on its first page.
    agreed: bool,
    hide: bool,
    save_status: String,
    secondary_tip: usize,
    terms: Vec<Block>,
    /// The page changed: scroll back to its top.
    scroll_reset: bool,
    /// The link a click in the terms asked for.
    pub link: Option<String>,
}

impl LobbyWelcomeDialog {
    #[must_use]
    pub fn new(tab: usize, secondary_tip: usize) -> Self {
        let agreed = terms_agreed();
        Self {
            tab: if agreed { tab.min(2) } else { 0 },
            agreed,
            hide: welcome_is_hidden(),
            save_status: String::new(),
            secondary_tip,
            terms: render_markdown(TERMS, 2),
            scroll_reset: true,
            link: None,
        }
    }

    /// Whether the guide may close (Escape, Close).
    #[must_use]
    pub fn agreed(&self) -> bool {
        self.agreed
    }

    fn select(&mut self, tab: usize) {
        self.tab = tab;
        self.scroll_reset = true;
    }

    /// The agree box: `writePreference(TERMS_PREFERENCE, ...)`.
    pub fn set_agreed(&mut self, agreed: bool) {
        self.agreed = agreed;
        let saved = write_preference(TERMS_PREFERENCE, agreed.then(terms_version).as_deref());
        self.save_status = if saved || !agreed {
            String::new()
        } else {
            "Your agreement couldn't be saved. The terms will be asked again next time.".into()
        };
        let tab = self.tab;
        self.select(tab);
    }

    /// "Don't show this again".
    pub fn set_hidden(&mut self, hide: bool) {
        self.hide = hide;
        let saved = write_preference(WELCOME_PREFERENCE, Some(if hide { "true" } else { "false" }));
        self.save_status = if saved {
            String::new()
        } else if hide {
            "This choice couldn't be saved. The guide may appear next time.".into()
        } else {
            "This choice couldn't be saved. Automatic display may still be disabled next time.".into()
        };
    }

    /// Next, or Close on the last step.
    pub fn next_step(&mut self) -> Option<GuideAnswer> {
        if !self.agreed {
            return None;
        }
        if self.tab == 2 {
            return Some(GuideAnswer::Close);
        }
        let tab = self.tab + 1;
        self.select(tab);
        None
    }

    /// Opens `tab` (0 to 2), once the terms are agreed.
    pub fn show_step(&mut self, tab: usize) {
        if self.agreed {
            self.select(tab.min(2));
        }
    }

    /// Arrow keys between the steps, once the terms are agreed.
    pub fn step(&mut self, by: isize) {
        if self.agreed {
            let tab = (self.tab as isize + by).rem_euclid(3) as usize;
            self.select(tab);
        }
    }

    /// Draws the guide over `screen`; what it asks of the lobby.
    pub fn ui(&mut self, ui: &mut egui::Ui, screen: Rect) -> Option<GuideAnswer> {
        let w = screen.width();
        let card = card_rect(screen, 1400.0, 1080.0);
        // The guide cannot be closed from its veil.
        veil(ui, screen, "welcome", card);
        let painter = ui.painter().clone();
        let inner = paint_card(&painter, card, w);
        let narrow = w <= 820.0;
        let header_below = if narrow { 16.0 } else { 20.0 };
        let mut y = header(
            &painter,
            inner,
            w,
            "WASHOI / READY TO PLAY",
            "Welcome to Washiki Washoi!",
            header_below,
        );
        let mut answer = None;
        if let Some(t) = self.tabs(ui, inner, y, narrow) {
            self.select(t);
        }
        y += if narrow { 50.0 } else { 62.0 } + 20.0;

        let keys_top = footer(&painter, inner);
        let status_h = if self.save_status.is_empty() { 0.0 } else { 28.0 };
        let content = Rect::from_min_max(pos2(inner.left(), y), pos2(inner.right(), keys_top - 22.0 - status_h));
        if !self.save_status.is_empty() {
            let g = text(&painter, &self.save_status, 16.0, body(), ink(1.0), 0.0);
            painter.galley(pos2(inner.left(), content.bottom() + 4.0), g, Color32::WHITE);
        }
        let reset = std::mem::take(&mut self.scroll_reset);
        match self.tab {
            0 => self.terms_page(ui, content, w),
            1 => {
                scroll_body(ui, content, "keys", reset, |ui, top, width| {
                    keys_page(ui, top, width, w)
                });
            }
            _ => {
                let tip = self.secondary_tip;
                scroll_body(ui, content, "tips", reset, |ui, top, width| {
                    tips_page(ui, top, width, w, tip)
                });
            }
        }

        // The footer: Go to keyboard setup | I agree | Don't show this again, and Next.
        let label = if self.tab == 2 { "Close" } else { "Next »" };
        let next_w = ink_key_width(&painter, label, 125.0);
        // `margin: 0 calc(--key-dx + 2px) --key-d 0`: room for the side.
        let next_left = inner.right() - next_w - (6.0 * 0.75 + 2.0);
        if ink_key(ui, "next", next_left, keys_top, next_w, label, !self.agreed) {
            answer = self.next_step();
        }
        match self.tab {
            0 => {
                let agreed = self.agreed;
                if check_row(
                    ui,
                    "agree",
                    pos2(inner.left(), keys_top + KEY_H / 2.0),
                    agreed,
                    "I agree to the terms of service",
                    None,
                    true,
                ) {
                    self.set_agreed(!agreed);
                }
            }
            1 => {
                let label = "Go to keyboard setup";
                let sw = ink_key_width(&painter, label, 0.0);
                if ink_key(ui, "setup", inner.left(), keys_top, sw, label, false) {
                    answer = Some(GuideAnswer::Setup);
                }
            }
            _ => {
                let hide = self.hide;
                if check_row(
                    ui,
                    "hide",
                    pos2(inner.left(), keys_top + KEY_H / 2.0),
                    hide,
                    "Don't show this again",
                    Some("Reopen this guide with ? in the lobby."),
                    false,
                ) {
                    self.set_hidden(!hide);
                }
            }
        }
        if self.link.is_some() && answer.is_none() {
            answer = Some(GuideAnswer::Link);
        }
        answer
    }

    /// `.lobby-guide__tabs`: one chevron strip, each step pointing into the
    /// next; the step clicked, if it may be chosen.
    fn tabs(&self, ui: &mut egui::Ui, inner: Rect, top: f32, narrow: bool) -> Option<usize> {
        let painter = ui.painter().clone();
        let (c, h) = if narrow { (14.0, 50.0) } else { (24.0, 62.0) };
        let tw = (inner.width() + c) / 3.0;
        let w = ui.ctx().content_rect().width();
        let size = if narrow {
            12.0
        } else {
            clamp_css(15.0, w * 0.0125, 20.0)
        };
        let (dot, dot_size) = if narrow { (26.0, 14.0) } else { (34.0, 17.0) };
        let mut chosen = None;
        for (i, name) in STEPS.iter().enumerate() {
            let left = inner.left() + i as f32 * (tw - c / 2.0);
            let r = Rect::from_min_size(pos2(left, top), vec2(tw, h));
            // The convex parts: the body to the point, and the notch's two wedges.
            let mid = r.center().y;
            let body_pts = [
                pos2(if i == 0 { r.left() } else { r.left() + c }, r.top()),
                pos2(r.right() - c, r.top()),
                pos2(r.right(), mid),
                pos2(r.right() - c, r.bottom()),
                pos2(if i == 0 { r.left() } else { r.left() + c }, r.bottom()),
            ];
            let wedges: Vec<[Pos2; 3]> = if i == 0 {
                Vec::new()
            } else {
                vec![
                    [r.left_top(), pos2(r.left() + c, r.top()), pos2(r.left() + c, mid)],
                    [pos2(r.left() + c, mid), pos2(r.left() + c, r.bottom()), r.left_bottom()],
                ]
            };
            let disabled = i > 0 && !self.agreed;
            let response = ui.interact(
                r,
                egui::Id::NULL.with(("guide-tab", i)),
                if disabled { Sense::hover() } else { Sense::click() },
            );
            let selected = i == self.tab;
            let hovered = response.hovered() && !disabled && !selected;
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            } else if response.hovered() && disabled {
                ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
            }
            let stops: Vec<(f32, Color32)> = if selected {
                vec![
                    (0.0, hsl(250.0, 28.0, 21.0)),
                    (0.6, hsl(250.0, 40.0, 10.0)),
                    (1.0, hsl(252.0, 44.0, 6.0)),
                ]
            } else if hovered {
                vec![
                    (0.0, hsl(278.0, 92.0, 75.0)),
                    (0.55, hsl(273.0, 80.0, 61.0)),
                    (1.0, hsl(268.0, 72.0, 50.0)),
                ]
            } else if i < self.tab {
                vec![(0.0, hsl(44.0, 100.0, 55.0)), (1.0, hsl(40.0, 95.0, 46.0))]
            } else {
                vec![(0.0, hsl(51.0, 100.0, 76.0)), (1.0, hsl(46.0, 100.0, 62.0))]
            };
            // `filter: drop-shadow(0 5px 0 hsl(36 85% 30%))`, under the strip.
            let shade = [(0.0, hsl(36.0, 85.0, 30.0)), (1.0, hsl(36.0, 85.0, 30.0))];
            let down = |p: &Pos2| *p + vec2(0.0, 5.0);
            gradient_polygon(&painter, &body_pts.iter().map(down).collect::<Vec<_>>(), &shade);
            for wedge in &wedges {
                gradient_polygon(&painter, &wedge.iter().map(down).collect::<Vec<_>>(), &shade);
            }
            gradient_polygon_in(&painter, &body_pts, r.top(), r.bottom(), &stops);
            for wedge in &wedges {
                gradient_polygon_in(&painter, wedge, r.top(), r.bottom(), &stops);
            }
            if response.clicked() && !disabled {
                chosen = Some(i);
            }

            // The number in its badge, then the name: centred, 14 apart.
            let (text_c, badge_c, number_c) = if selected {
                (gold_text(), gold_text(), ink(1.0))
            } else if hovered {
                (cream(), hsl(258.0, 72.0, 19.0), cream())
            } else {
                (ink(1.0), ink(1.0), gold_text())
            };
            let fade = |c: Color32| if disabled { c.gamma_multiply(0.45) } else { c };
            let pad = c + if narrow { 6.0 } else { 14.0 };
            let gap = if narrow { 8.0 } else { 14.0 };
            let room = (tw - c / 2.0 - pad * 2.0 - dot - gap).max(10.0);
            let label = shagger::ui::components::lobby_twin::elide(
                &painter,
                &name.to_uppercase(),
                size,
                display_family(),
                room,
            );
            let g = text(&painter, &label, size, display_family(), fade(text_c), size * 0.07);
            let total = dot + gap + g.size().x;
            let x = r.left() + c / 2.0 + (tw - c / 2.0 - total) / 2.0;
            let centre = pos2(x + dot / 2.0, mid);
            painter.circle_filled(centre, dot / 2.0, fade(badge_c));
            let n = text(
                &painter,
                &(i + 1).to_string(),
                dot_size,
                display_family(),
                fade(number_c),
                0.0,
            );
            painter.galley(centre - n.size() / 2.0, n, Color32::WHITE);
            painter.galley(pos2(x + dot + gap, mid - g.size().y / 2.0), g, Color32::WHITE);
        }
        chosen
    }

    /// The terms page: its text above, the terms filling the rest.
    fn terms_page(&mut self, ui: &mut egui::Ui, content: Rect, screen_w: f32) {
        let painter = ui.painter().clone();
        let size = clamp_css(19.0, screen_w * 0.0175, 23.0);
        let intro = "Welcome to Washoi, an arena shooter where movement makes the difference. Before your first match, please read our terms: everything played on our servers is public.";
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = content.width().min(880.0);
        job.append(
            intro,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(size, FontFamily::Proportional),
                color: ink(0.92),
                line_height: Some(size * 1.55),
                ..Default::default()
            },
        );
        let g = painter.layout_job(job);
        let top = content.top() + 4.0;
        let h = g.size().y;
        painter.galley(pos2(content.left() + 5.0, top), g, Color32::WHITE);
        // `.lobby-guide__terms`: a scrolling inset, the credits' reading style.
        let inset = Rect::from_min_max(
            pos2(content.left(), top + h + 14.0 + 8.0),
            pos2(content.right(), content.bottom() - 4.0),
        );
        painter.rect_filled(inset, 3.0, ink(0.1));
        painter.rect_stroke(inset, 3.0, Stroke::new(2.0, ink(0.22)), egui::StrokeKind::Inside);
        let body_r = Rect::from_min_max(inset.min + vec2(30.0, 6.0), inset.max - vec2(8.0, 4.0));
        let reset = self.scroll_reset;
        let blocks = &self.terms;
        let mut link = None;
        scroll_body(ui, body_r, "terms", reset, |ui, top, width| {
            let (clicked, bottom) =
                markdown_lite::paint(ui, blocks, top.x, top.y, width - 8.0, Reading::Terms, screen_w);
            link = clicked;
            bottom + 24.0
        });
        if link.is_some() {
            self.link = link;
        }
    }
}

/// `.lobby-guide__preference`: a 24px box and its words (and a small line
/// under them). `bold` is the agreement's weight. True when clicked.
fn check_row(
    ui: &mut egui::Ui,
    id: &str,
    at: Pos2,
    checked: bool,
    label: &str,
    small: Option<&str>,
    bold: bool,
) -> bool {
    let painter = ui.painter().clone();
    let g = text(&painter, label, 20.0, body(), ink(if bold { 1.0 } else { 0.92 }), 0.0);
    let s = small.map(|s| text(&painter, s, 16.0, body(), ink(0.78), 0.0));
    let text_h = g.size().y + s.as_ref().map_or(0.0, |s| s.size().y + 4.0);
    let text_w = g.size().x.max(s.as_ref().map_or(0.0, |s| s.size().x));
    let b = Rect::from_min_size(pos2(at.x, at.y - 12.0), vec2(24.0, 24.0));
    let r = Rect::from_min_max(
        pos2(at.x, at.y - text_h / 2.0),
        pos2(at.x + 24.0 + 14.0 + text_w, at.y + text_h / 2.0),
    )
    .union(b);
    let response = ui.interact(r, egui::Id::NULL.with(("guide-check", id)), Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // `accent-color: var(--ink)`.
    if checked {
        painter.rect_filled(b, 4.0, ink(1.0));
        painter.add(Shape::line(
            vec![
                b.min + vec2(5.5, 12.5),
                b.min + vec2(10.0, 17.0),
                b.min + vec2(18.5, 7.0),
            ],
            Stroke::new(2.8, hsl(46.0, 100.0, 70.0)),
        ));
    } else {
        painter.rect_filled(b, 4.0, hsla(45.0, 100.0, 97.0, 0.92));
        painter.rect_stroke(b, 4.0, Stroke::new(2.0, ink(0.75)), egui::StrokeKind::Inside);
    }
    let x = b.right() + 14.0;
    let gy = at.y - text_h / 2.0;
    let gh = g.size().y;
    painter.galley(pos2(x, gy), g, Color32::WHITE);
    if let Some(s) = s {
        painter.galley(pos2(x, gy + gh + 4.0), s, Color32::WHITE);
    }
    response.clicked()
}

/// `.lobby-guide__panel h2` and its `p`: returns the bottom.
fn panel_head(painter: &Painter, left: f32, top: f32, width: f32, screen_w: f32, title: &str, muted: &str) -> f32 {
    let size = clamp_css(24.0, screen_w * 0.023, 34.0);
    let t = wrapped_ink(
        painter,
        title,
        FontId::new(size, display_family()),
        ink(1.0),
        width.min(820.0),
        size * 1.3,
    );
    let mut y = top + 6.0;
    let th = t.size().y;
    painter.galley(pos2(left, y), t, Color32::WHITE);
    y += th + 8.0;
    if !muted.is_empty() {
        let ps = clamp_css(19.0, screen_w * 0.0175, 23.0);
        let p = wrapped_ink(
            painter,
            muted,
            FontId::new(ps, FontFamily::Proportional),
            ink(0.78),
            width.min(880.0),
            ps * 1.55,
        );
        let ph = p.size().y;
        painter.galley(pos2(left, y + 6.0), p, Color32::WHITE);
        y += 6.0 + ph + 14.0;
    }
    y
}

fn wrapped_ink(painter: &Painter, s: &str, font: FontId, color: Color32, width: f32, line_h: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width.max(1.0);
    job.append(
        s,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            line_height: Some(line_h),
            ..Default::default()
        },
    );
    painter.layout_job(job)
}

/// "Your keys": the board and the mouse as bound.
fn keys_page(ui: &mut egui::Ui, top: Pos2, width: f32, screen_w: f32) -> f32 {
    let painter = ui.painter().clone();
    let y = panel_head(
        &painter,
        top.x + 5.0,
        top.y + 6.0,
        width - 10.0,
        screen_w,
        "Your controls, ready to go.",
        "You can change these in keyboard setup.",
    );
    let rect = Rect::from_min_size(pos2(top.x + 5.0, y), vec2(width - 10.0, 10.0));
    ControlsPage::preview(ui, rect, screen_w) + 10.0
}

/// `bindings`: an action's keys as keycaps, mouse buttons last.
fn binding_labels(action: &str) -> Vec<String> {
    let codes = shagger::input::input_system::with_input(|i| codes_for(action, i));
    let mut keys: Vec<&String> = codes.iter().filter(|c| !c.contains(':')).collect();
    keys.extend(codes.iter().filter(|c| c.contains(':')));
    let mut labels: Vec<String> = Vec::new();
    for code in keys {
        let name = input_name(code);
        if !labels.contains(&name) {
            labels.push(name);
        }
    }
    labels
}

/// A piece of a tip: words, or an action's keycaps.
enum Bit<'a> {
    Words(&'a str),
    Keys(&'a str),
}

/// "Tip of the day": two lessons and one of two extras.
fn tips_page(ui: &mut egui::Ui, top: Pos2, width: f32, screen_w: f32, secondary: usize) -> f32 {
    let painter = ui.painter().clone();
    let left = top.x + 5.0;
    let width = width - 10.0;
    let mut y = panel_head(&painter, left, top.y + 6.0, width, screen_w, "Did you know?", "");
    let slide = format!(
        "Crouchsliding is essential. Build at least {} u/s, then press ",
        DEFAULT_FPS_MOVEMENT_CONFIG.slide_min_speed
    );
    let lessons: [(&str, Vec<Bit<'_>>, bool); 3] = [
        (
            "Keep your momentum",
            vec![
                Bit::Words(&slide),
                Bit::Keys(fps::CROUCH),
                Bit::Words(" to slide and carry your speed through the arena."),
            ],
            false,
        ),
        (
            "Reach a little higher",
            vec![
                Bit::Words("Can't quite reach that ledge? While airborne beside a wall, hold "),
                Bit::Keys(fps::JUMP),
                Bit::Words(" to charge a wall jump, then release to push off."),
            ],
            false,
        ),
        if secondary == 0 {
            (
                "Land softer",
                vec![
                    Bit::Words("Hold "),
                    Bit::Keys(fps::CROUCH),
                    Bit::Words(" before landing from a high fall to soften the impact."),
                ],
                true,
            )
        } else {
            (
                "Chain your movement",
                vec![
                    Bit::Words("Jump out of a crouchslide with "),
                    Bit::Keys(fps::JUMP),
                    Bit::Words(" to turn the slide into another boosted jump."),
                ],
                true,
            )
        },
    ];
    for (title, bits, extra) in &lessons {
        y += 12.0;
        y = lesson(&painter, left, y, width, screen_w, title, bits, *extra) + 12.0;
    }
    let g = text(
        &painter,
        "Tips describe standard movement rules.",
        16.0,
        body(),
        ink(0.78),
        0.0,
    );
    let gh = g.size().y;
    painter.galley(pos2(left, y), g, Color32::WHITE);
    y + gh
}

/// `.lobby-guide__lesson`: a faint band with a violet bar (ink for the
/// extra), its title and its text with keycaps in it. Returns the bottom.
#[allow(clippy::too_many_arguments)]
fn lesson(
    painter: &Painter,
    left: f32,
    top: f32,
    width: f32,
    screen_w: f32,
    title: &str,
    bits: &[Bit<'_>],
    extra: bool,
) -> f32 {
    let size = clamp_css(19.0, screen_w * 0.0175, 23.0);
    let line_h = size * 1.55;
    // Laid out first, painted over the band once its height is known.
    let band = painter.add(Shape::Noop);
    let bar = painter.add(Shape::Noop);
    let x0 = left + 4.0 + 20.0;
    let max_x = left + width - 20.0;
    let t = text(painter, title, 20.0, display_family(), ink(1.0), 0.0);
    let mut y = top + 15.0;
    let th = t.size().y;
    painter.galley(pos2(x0, y), t, Color32::WHITE);
    y += th + 4.0 + 6.0;
    // Words wrap word by word; a keycap is one unbreakable piece.
    let mut x = x0;
    let font = FontId::new(size, FontFamily::Proportional);
    let space = painter
        .layout_no_wrap(" ".into(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let place = |w: f32, x: &mut f32, y: &mut f32| {
        if *x > x0 && *x + w > max_x {
            *x = x0;
            *y += line_h;
        }
        let at = pos2(*x, *y);
        *x += w;
        at
    };
    for bit in bits {
        match bit {
            Bit::Words(words) => {
                for (i, word) in words.split(' ').enumerate() {
                    if i > 0 {
                        x += space;
                    }
                    if word.is_empty() {
                        continue;
                    }
                    let g = painter.layout_no_wrap(word.to_owned(), font.clone(), ink(0.92));
                    let at = place(g.size().x, &mut x, &mut y);
                    painter.galley(pos2(at.x, at.y + (line_h - g.size().y) / 2.0), g, Color32::WHITE);
                }
            }
            Bit::Keys(action) => {
                let labels = binding_labels(action);
                if labels.is_empty() {
                    let s = format!(
                        "{} is unbound — open keyboard setup",
                        if *action == fps::JUMP { "Jump" } else { "Crouch" }
                    );
                    let g = painter.layout_no_wrap(s, font.clone(), ink(0.92));
                    let at = place(g.size().x, &mut x, &mut y);
                    painter.galley(pos2(at.x, at.y + (line_h - g.size().y) / 2.0), g, Color32::WHITE);
                }
                for (i, label) in labels.iter().enumerate() {
                    if i > 0 {
                        let g = painter.layout_no_wrap(" / ".into(), font.clone(), ink(0.92));
                        let at = place(g.size().x, &mut x, &mut y);
                        painter.galley(pos2(at.x, at.y + (line_h - g.size().y) / 2.0), g, Color32::WHITE);
                    }
                    let at = place(keycap_width(painter, label) + 4.0, &mut x, &mut y);
                    keycap(painter, pos2(at.x + 2.0, at.y + (line_h - 28.0) / 2.0), label);
                }
            }
        }
    }
    y += line_h + 14.0;
    let r = Rect::from_min_max(pos2(left, top), pos2(left + width, y));
    painter.set(
        band,
        Shape::rect_filled(
            r,
            3.0,
            if extra {
                hsla(48.0, 100.0, 85.0, 0.35)
            } else {
                ink(0.08)
            },
        ),
    );
    painter.set(
        bar,
        Shape::rect_filled(
            Rect::from_min_size(r.min, vec2(4.0, r.height())),
            egui::CornerRadius {
                nw: 3,
                sw: 3,
                ..Default::default()
            },
            if extra { ink(1.0) } else { hsl(276.0, 70.0, 40.0) },
        ),
    );
    y
}

fn keycap_width(painter: &Painter, label: &str) -> f32 {
    text(painter, label, 16.0, display_family(), Color32::WHITE, 0.0)
        .size()
        .x
        + 18.0
}

/// `.lobby-guide__keycap`: a little ink key with gold lettering.
fn keycap(painter: &Painter, at: Pos2, label: &str) {
    let w = keycap_width(painter, label);
    let r = Rect::from_min_size(at, vec2(w, 25.0));
    painter.rect_filled(r.translate(vec2(0.0, 3.0)), 3.0, hsl(250.0, 50.0, 4.0));
    gradient_polygon(
        painter,
        &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()],
        &[(0.0, hsl(250.0, 35.0, 18.0)), (1.0, hsl(250.0, 45.0, 9.0))],
    );
    let g = text(painter, label, 16.0, display_family(), gold_text(), 0.0);
    painter.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
}

/// The actions the guide's hidden summary lists, as the web's screen
/// reader text: `Name: keys`.
#[must_use]
pub fn binding_summary() -> Vec<String> {
    SETTINGS_ACTIONS
        .iter()
        .filter(|a| a.group != crate::client::settings::settings_model::ActionGroupId::Other)
        .map(|a| {
            let labels = binding_labels(a.id);
            format!(
                "{}: {}",
                a.name,
                if labels.is_empty() {
                    "Unbound".into()
                } else {
                    labels.join(" / ")
                }
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terms_carry_their_version() {
        assert_eq!(terms_version(), "October 22, 2026");
    }

    #[test]
    fn the_steps_wait_for_the_terms() {
        let mut d = LobbyWelcomeDialog {
            tab: 0,
            agreed: false,
            hide: false,
            save_status: String::new(),
            secondary_tip: 0,
            terms: Vec::new(),
            scroll_reset: false,
            link: None,
        };
        assert_eq!(d.next_step(), None);
        assert_eq!(d.tab, 0);
        d.step(1);
        assert_eq!(d.tab, 0);
        d.agreed = true;
        assert_eq!(d.next_step(), None);
        assert_eq!(d.tab, 1);
        d.step(1);
        assert_eq!(d.next_step(), Some(GuideAnswer::Close));
        d.step(1);
        assert_eq!(d.tab, 0);
    }
}
