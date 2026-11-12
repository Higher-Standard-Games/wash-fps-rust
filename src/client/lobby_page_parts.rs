//! wash-fps `styles/LobbyPanel.css`, "Shared by the Leaderboard and Demos
//! pages": a page's head (kicker, title and the modes in their own colours),
//! its boards as keys, the rule under them and the find field; and the
//! rail's labelled list of cream keys (`raised-key--room`), which the rooms,
//! your ranks and the demos' maps share.

use shagger::ui::components::lobby_twin::{RAIL_SKEW, clamp_css, cream, elide, gradient_polygon, hsl, hsla, ink};
use shagger::ui::components::raised_key::{KeyFace, KeyLook, KeyText, KeyTextLayout, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Painter, Rect, Shape, Stroke, pos2, vec2};

use crate::client::settings::settings_fields::{body, line};
use crate::client::washoi_style::display_family;

/// A mode as a page lists it (`RankedMode`): its title and mark.
#[derive(Debug, Clone, Copy)]
pub struct ModeChip<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub mark: Color32,
}

/// `lobby__head-row`'s heading: the kicker in the accent, the title under
/// it with its drop shadow. Returns the space it took.
pub fn page_head(painter: &Painter, left: f32, top: f32, kicker: &str, title: &str, accent: Color32) -> Rect {
    let kicker = line(painter, &kicker.to_uppercase(), 12.0, body(), accent, 0.32);
    let kh = kicker.size().y;
    painter.galley(pos2(left, top), kicker, accent);
    let size = clamp_css(36.0, painter.ctx().content_rect().width() * 0.034, 60.0);
    let title = line(painter, title, size, display_family(), cream(), 0.0);
    let ty = top + kh + 8.0;
    let size = title.size();
    painter.galley_with_override_text_color(pos2(left + 5.0, ty + 6.0), title.clone(), hsl(250.0, 50.0, 3.0));
    painter.galley(pos2(left, ty), title, cream());
    Rect::from_min_max(pos2(left, top), pos2(left + size.x, ty + size.y))
}

/// `lobby__modes`: a key per mode, right-aligned to `right` and standing on
/// `bottom`. The picked one is lit in its mark. Returns the click and the
/// row's left edge.
pub fn mode_keys(
    ui: &mut egui::Ui,
    id: &str,
    right: f32,
    bottom: f32,
    modes: &[ModeChip<'_>],
    on: usize,
) -> (Option<usize>, f32) {
    let (w, h, gap) = (100.0, 46.0, 10.0);
    let left = right - modes.len() as f32 * (w + gap) + gap;
    let mut clicked = None;
    for (i, m) in modes.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(left + i as f32 * (w + gap), bottom - 6.0 - h), vec2(w, h));
        if flat_key(ui, (id, m.id), rect, m.title, Some(m.mark), i == on, 14.0) {
            clicked = Some(i);
        }
    }
    (clicked, left)
}

/// `lobby__mode-key` and `lb-period button`: a flat key on a dark slab. At
/// rest it is slate, a hover lifts it violet, and the picked one sinks in
/// `mark` (gold without one).
pub fn flat_key(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    label: &str,
    mark: Option<Color32>,
    on: bool,
    size: f32,
) -> bool {
    let response = ui.interact(rect, egui::Id::NULL.with(id), egui::Sense::click());
    let hovered = response.hovered() && !on;
    let painter = ui.painter();
    let (dy, top, bottom, text_c) = if on {
        let (hi, lo) = match mark {
            Some(m) => (mix_white(m, 0.38), m),
            None => (hsl(51.0, 100.0, 73.0), hsl(44.0, 100.0, 55.0)),
        };
        (3.0, hi, lo, ink())
    } else if hovered {
        (-2.0, hsl(278.0, 92.0, 72.0), hsl(268.0, 72.0, 52.0), cream())
    } else {
        (
            0.0,
            hsl(250.0, 24.0, 25.0),
            hsl(250.0, 30.0, 17.0),
            hsla(45.0, 100.0, 95.0, 0.78),
        )
    };
    let r = rect.translate(vec2(0.0, dy));
    if on {
        let below = mark.map_or(hsl(30.0, 88.0, 28.0), |m| m.gamma_multiply(0.6));
        painter.rect_filled(r.translate(vec2(0.0, 1.0)), 2.0, below);
    } else {
        painter.rect_filled(
            r.translate(vec2(0.0, 8.0)).expand(2.0),
            4.0,
            hsla(250.0, 70.0, 2.0, 0.25),
        );
        painter.rect_filled(r.translate(vec2(0.0, 4.0)), 2.0, hsl(250.0, 40.0, 8.0));
    }
    gradient_polygon(
        painter,
        &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()],
        &[(0.0, top), (1.0, bottom)],
    );
    if on {
        painter.hline(
            r.x_range(),
            r.top() + 1.0,
            Stroke::new(2.0, Color32::from_white_alpha(60)),
        );
    } else if let Some(m) = mark {
        painter.rect_filled(
            Rect::from_min_max(pos2(r.left(), r.bottom() - 4.0), r.right_bottom()),
            0.0,
            m,
        );
    }
    let g = line(painter, &label.to_uppercase(), size, display_family(), text_c, 0.08);
    painter.galley(
        r.center() - g.size() / 2.0 - vec2(0.0, if mark.is_some() && !on { 2.0 } else { 0.0 }),
        g,
        text_c,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

fn mix_white(c: Color32, white: f32) -> Color32 {
    let m = |v: u8| (f32::from(v) * (1.0 - white) + 255.0 * white) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// `raised-key--tab`: slate, gold when open.
#[must_use]
pub fn tab_look(open: bool) -> KeyLook {
    if open {
        KeyLook {
            sub: hsla(250.0, 45.0, 8.0, 0.66),
            depth: 6.6,
            ..KeyLook::gold()
        }
    } else {
        KeyLook {
            face: vec![(0.0, hsl(250.0, 26.0, 25.0)), (1.0, hsl(250.0, 30.0, 16.0))],
            side: [hsl(250.0, 36.0, 11.0), hsl(250.0, 40.0, 6.0)],
            text: cream(),
            sub: hsla(45.0, 100.0, 95.0, 0.58),
            shadow: hsla(250.0, 70.0, 2.0, 0.6),
            depth: 6.6,
            ..KeyLook::gold()
        }
    }
}

/// `lobby__boards`: the page's boards as three keys across `rect`'s width,
/// name over sub. Returns the one clicked and the row's bottom (with the
/// keys' depth).
pub fn board_keys(
    ui: &mut egui::Ui,
    id: &str,
    left: f32,
    top: f32,
    width: f32,
    boards: &[(&str, &str)],
    open: usize,
) -> (Option<usize>, f32) {
    let gap = 22.0;
    let h = if ui.ctx().content_rect().height() <= 960.0 {
        62.0
    } else {
        78.0
    };
    let w = (width - gap * (boards.len() as f32 - 1.0)) / boards.len() as f32;
    let name_size = clamp_css(16.0, ui.ctx().content_rect().width() * 0.0125, 22.0);
    let mut clicked = None;
    for (i, (name, sub)) in boards.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(left + i as f32 * (w + gap), top), vec2(w, h));
        let is_open = i == open;
        let mut key = RaisedKey::new((id, *name), rect, tab_look(is_open));
        if is_open {
            key = key.rest_press(0.45).hover(tab_look(true), 0.45);
        }
        let face = key.show(ui);
        KeyText::new(&name.to_uppercase(), &sub.to_uppercase(), display_family(), body())
            .size(name_size, 12.0)
            .spacing(0.08)
            .pad(26.0, 24.0)
            .layout(KeyTextLayout::Stacked)
            .paint(ui.painter(), &face);
        if face.clicked {
            clicked = Some(i);
        }
    }
    (clicked, top + h + 6.6 + 4.0)
}

/// `lobby__rule`: the small print under the boards. Returns its bottom.
pub fn rule(painter: &Painter, left: f32, top: f32, s: &str) -> f32 {
    let c = hsla(45.0, 100.0, 95.0, 0.5);
    let g = line(painter, &s.to_uppercase(), 12.0, body(), c, 0.18);
    let h = g.size().y;
    painter.galley(pos2(left, top), g, c);
    top + h
}

/// `lobby__rail-label`: a label at each end of the rail's middle.
/// Returns where the list under it starts.
pub fn rail_label(painter: &Painter, mid: Rect, rail_w: f32, left: &str, right: &str) -> f32 {
    let sub = hsla(250.0, 45.0, 8.0, 0.62);
    let y = mid.top() + 6.0;
    let l = line(painter, &left.to_uppercase(), 11.0, body(), sub, 0.26);
    painter.galley(pos2(mid.left() + 24.0, y), l, sub);
    let r = line(painter, &right.to_uppercase(), 11.0, body(), sub, 0.26);
    let x = mid.left() + rail_w - RAIL_SKEW - 26.0 - r.size().x;
    painter.galley(pos2(x, y), r, sub);
    y + 11.0 + 12.0
}

/// One line of a rail list (`raised-key--room`): the mark, the name, a
/// detail and what stands at the right end.
pub struct RailKey<'a> {
    pub id: (&'a str, &'a str),
    pub mark: Color32,
    pub name: &'a str,
    pub detail: &'a str,
    pub right: &'a str,
    /// `aria-pressed`: the mark gets a ring.
    pub pressed: bool,
    pub name_width: f32,
}

impl RailKey<'_> {
    pub fn show(&self, ui: &mut egui::Ui, rect: Rect) -> KeyFace {
        let face = RaisedKey::new(self.id, rect, KeyLook::cream().sm()).show(ui);
        let painter = ui.painter();
        let r = face.rect;
        let cy = r.center().y;
        let mut x = r.left() + 20.0;
        let mark = [
            pos2(x + 2.2, cy - 5.0),
            pos2(x + 10.0, cy - 5.0),
            pos2(x + 7.8, cy + 5.0),
            pos2(x, cy + 5.0),
        ];
        painter.add(Shape::convex_polygon(mark.to_vec(), self.mark, Stroke::NONE));
        if self.pressed {
            let ring: Vec<_> = [(-3.0, -3.0), (3.0, -3.0), (3.0, 3.0), (-3.0, 3.0)]
                .iter()
                .zip(mark)
                .map(|(&(dx, dy), p)| p + vec2(dx, dy))
                .collect();
            painter.add(Shape::closed_line(ring, Stroke::new(2.0, ink())));
        }
        x += 20.0;
        let name = line(
            painter,
            &self.name.to_uppercase(),
            14.0,
            display_family(),
            face.text,
            0.07,
        );
        painter.galley(pos2(x, cy - name.size().y / 2.0), name, face.text);
        x += self.name_width;
        let right = line(painter, self.right, 13.0, body(), face.sub, 0.12);
        let right_x = r.right() - 16.0 - right.size().x;
        painter.galley(pos2(right_x, cy - right.size().y / 2.0), right, face.sub);
        let room = right_x - 10.0 - x;
        if room > 20.0 && !self.detail.is_empty() {
            let shown = elide(painter, self.detail, 13.0, body(), room);
            let g = line(painter, &shown, 13.0, body(), face.sub, 0.06);
            painter.galley(pos2(x, cy - g.size().y / 2.0), g, face.sub);
        }
        face
    }
}

/// `lb-status` and the like: a line of body text at 70%, wrapped.
/// Returns its bottom.
pub fn status(painter: &Painter, left: f32, top: f32, width: f32, s: &str) -> f32 {
    let c = hsla(45.0, 100.0, 95.0, 0.7);
    let g = crate::client::settings::settings_fields::wrapped(painter, s, egui::FontId::new(16.0, body()), c, width);
    let h = g.size().y;
    painter.galley(pos2(left, top), g, c);
    top + h
}
