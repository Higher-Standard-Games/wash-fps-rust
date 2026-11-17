//! wash-fps `client/campaign/CampaignCutscene.ts` and `Campaign.css`'s
//! `.campaign-cut`: the comic card before a campaign mission. The screen
//! fades to black, then each framed panel of the card in turn fills the
//! screen, drifting slowly, with the rest of the card blurred behind it. A
//! panel moves on after `PANEL_MS`, or at a click or key press. After the
//! last panel the screen stays black, with a status line, until the match
//! connects.
//!
//! The page's CSS transitions become times kept here: the cutscene is
//! drawn each frame from how long each phase has run. The card's tilt in
//! depth (`rotateX`/`rotateY`) and the blur at full speed between panels
//! have no egui counterpart and are left out; the push and turn are kept.

use std::time::{Duration, Instant};

use shagger::ui::components::lobby_twin::{gradient_polygon_x, hsl, hsla, text};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Mesh, Pos2, Rect, Shape, Stroke, pos2, vec2};

use super::campaign_story::{CampaignCard, CardPanel};
use super::campaign_view::{CampaignArt, campaign_plate};
use crate::client::washoi_style::display_family;

/// How long a panel holds, once it has settled on screen, before the next
/// one.
pub const PANEL_MS: u64 = 5000;
const FADE_MS: u64 = 450;
/// The first panel fading in; its time to read starts after.
pub const ART_FADE_MS: u64 = 700;
/// The move from one panel to the next.
const MOVE_MS: u64 = 825;
/// How much of the screen a panel may fill.
const FILL_W: f32 = 0.9;
const FILL_H: f32 = 0.84;

/// `PanelView`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelView {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}

/// `panelView`: scale and offset that centre `panel` on a
/// `width`×`height` screen, as large as fits.
#[must_use]
pub fn panel_view(panel: CardPanel, width: f32, height: f32) -> PanelView {
    let scale = (width * FILL_W / panel.w).min(height * FILL_H / panel.h);
    PanelView {
        scale,
        x: width / 2.0 - (panel.x + panel.w / 2.0) * scale,
        y: height / 2.0 - (panel.y + panel.h / 2.0) * scale,
    }
}

/// CSS `cubic-bezier(x1, y1, x2, y2)` at `t`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let curve = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    // Solve x(s) = t by bisection; it is monotonic for 0 ≤ x1, x2 ≤ 1.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if curve(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    curve(y1, y2, (lo + hi) / 2.0)
}

/// `MOVE_EASE`: slow off the mark, fast through the middle, a soft
/// landing.
fn move_ease(t: f32) -> f32 {
    cubic_bezier(0.76, 0.0, 0.24, 1.0, t)
}

fn ease_in_out(t: f32) -> f32 {
    cubic_bezier(0.42, 0.0, 0.58, 1.0, t)
}

fn ease(t: f32) -> f32 {
    cubic_bezier(0.25, 0.1, 0.25, 1.0, t)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Opening,
    Panels,
    Waiting,
    Error,
    Closed,
}

/// What the cutscene asks of the lobby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutsceneEvent {
    /// The last panel has been read: the mission may start.
    PanelsDone,
    /// "Back to campaign" after a failure.
    Back,
}

/// `CampaignCutscene`.
pub struct CampaignCutscene {
    card: CampaignCard,
    title: String,
    phase: Phase,
    opened: Instant,
    index: Option<usize>,
    /// When the current panel was shown, and the one it moved from.
    shown: Instant,
    from: Option<CardPanel>,
    /// When the art came on (the first panel).
    art_on: Option<Instant>,
    /// When the art went (the panels were read, or it failed).
    art_off: Option<Instant>,
    waiting_since: Option<Instant>,
    closing: Option<Instant>,
    status: String,
    panels_done: bool,
}

impl CampaignCutscene {
    /// A card to play: `card` absent (a retry, or a mission without art)
    /// goes straight to the black screen.
    #[must_use]
    pub fn new(card: Option<CampaignCard>, title: &str) -> Self {
        let now = Instant::now();
        let card = card.unwrap_or(CampaignCard {
            number: 0,
            width: 1536.0,
            height: 864.0,
            panels: &[],
            thumb_focus: (0.5, 0.5),
        });
        Self {
            card,
            title: title.to_uppercase(),
            phase: Phase::Opening,
            opened: now,
            index: None,
            shown: now,
            from: None,
            art_on: None,
            art_off: None,
            waiting_since: None,
            closing: None,
            status: String::new(),
            panels_done: false,
        }
    }

    /// `setStatus`: the words under the black screen while the mission is
    /// being set up.
    pub fn set_status(&mut self, status: &str) {
        status.clone_into(&mut self.status);
    }

    /// `fail`: something went wrong: say what, and offer the way back.
    pub fn fail(&mut self, message: &str) {
        if self.phase == Phase::Closed {
            return;
        }
        self.phase = Phase::Error;
        self.art_off.get_or_insert_with(Instant::now);
        message.clone_into(&mut self.status);
    }

    /// `close`: the match is connecting: fade the black away.
    pub fn close(&mut self) {
        if self.phase != Phase::Closed {
            self.phase = Phase::Closed;
            self.closing = Some(Instant::now());
        }
    }

    /// `isOpen`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.phase != Phase::Closed
    }

    /// Faded out after closing: the lobby may drop it.
    #[must_use]
    pub fn is_gone(&self) -> bool {
        self.closing
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(FADE_MS))
    }

    /// Whether the last panel has been read.
    #[must_use]
    pub fn panels_read(&self) -> bool {
        self.panels_done
    }

    fn dwell(&self, first: bool) -> Duration {
        Duration::from_millis(if first { ART_FADE_MS } else { MOVE_MS } + PANEL_MS)
    }

    fn show(&mut self, index: usize) {
        self.from = self.index.map(|i| self.card.panels[i]);
        if self.index.is_none() {
            self.art_on = Some(Instant::now());
        }
        self.index = Some(index);
        self.shown = Instant::now();
    }

    fn next(&mut self) -> Option<CutsceneEvent> {
        if self.phase != Phase::Panels {
            return None;
        }
        let next = self.index.map_or(0, |i| i + 1);
        if next < self.card.panels.len() {
            self.show(next);
            None
        } else {
            self.finish_panels()
        }
    }

    fn finish_panels(&mut self) -> Option<CutsceneEvent> {
        if !matches!(self.phase, Phase::Panels | Phase::Opening) {
            return None;
        }
        self.phase = Phase::Waiting;
        self.waiting_since = Some(Instant::now());
        if self.art_on.is_some() {
            self.art_off = Some(Instant::now());
        }
        self.panels_done = true;
        Some(CutsceneEvent::PanelsDone)
    }

    /// Moves the card on with time and input, and draws it over everything.
    pub fn ui(&mut self, ctx: &egui::Context, art: &mut CampaignArt) -> Option<CutsceneEvent> {
        if !self.card.panels.is_empty() {
            art.want(self.card.number);
        }
        art.poll(ctx);
        ctx.request_repaint();
        let mut event = None;
        let texture_name = format!("card{}", self.card.number);
        let ready = self.card.panels.is_empty()
            || (art.texture(&texture_name).is_some()
                && art.texture(&format!("card{}-blur", self.card.number)).is_some());
        // `play`: the fade to black, and the card once it has decoded.
        if self.phase == Phase::Opening && self.opened.elapsed() >= Duration::from_millis(FADE_MS) && ready {
            if self.card.panels.is_empty() {
                event = self.finish_panels();
            } else {
                self.phase = Phase::Panels;
                self.show(0);
            }
        }
        if self.phase == Phase::Panels && self.shown.elapsed() >= self.dwell(self.from.is_none()) {
            event = event.or(self.next());
        }

        let screen = ctx.content_rect();
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("campaign-cut"));
        let painter = ctx.layer_painter(layer);
        let fade_in = (self.opened.elapsed().as_secs_f32() / (FADE_MS as f32 / 1000.0)).min(1.0);
        let fade_out = self.closing.map_or(1.0, |at| {
            1.0 - (at.elapsed().as_secs_f32() / (FADE_MS as f32 / 1000.0)).min(1.0)
        });
        let opacity = ease(fade_in) * ease(fade_out);
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha((opacity * 255.0) as u8));
        // The whole screen is the cutscene's: nothing under it is clicked.
        if self.phase != Phase::Closed {
            egui::Area::new(egui::Id::unique("campaign-cut-input"))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .show(ctx, |ui| {
                    let response = ui.allocate_rect(screen, egui::Sense::click());
                    if response.clicked() && self.phase != Phase::Error {
                        event = event.or(self.next());
                    }
                });
        }
        event = event.or(self.keys(ctx));

        let art_alpha = self.art_alpha() * opacity;
        if art_alpha > 0.0
            && let (Some(index), Some(sharp), Some(blurred)) = (
                self.index,
                art.texture(&texture_name),
                art.texture(&format!("card{}-blur", self.card.number)),
            )
        {
            self.paint_card(&painter, screen, index, sharp.id(), blurred.id(), art_alpha);
        }
        self.paint_vignette(&painter, screen, opacity);
        let chrome = self.chrome_alpha() * opacity;
        if chrome > 0.0 {
            self.paint_chrome(&painter, screen, chrome);
        }
        event = event.or(self.paint_status(ctx, &painter, screen, opacity));
        event
    }

    /// `onKey`: any key moves the card on, Esc skips it; the console key
    /// and the modifiers pass.
    fn keys(&mut self, ctx: &egui::Context) -> Option<CutsceneEvent> {
        if !matches!(self.phase, Phase::Opening | Phase::Panels | Phase::Waiting) {
            return None;
        }
        let pressed: Vec<egui::Key> = ctx.input_mut(|i| {
            let mut keys = Vec::new();
            i.events.retain(|e| match e {
                egui::Event::Key {
                    key, pressed, repeat, ..
                } if *key != egui::Key::Backtick => {
                    if *pressed && !*repeat {
                        keys.push(*key);
                    }
                    false
                }
                egui::Event::Text(_) => false,
                _ => true,
            });
            keys
        });
        let mut event = None;
        for key in pressed {
            event = event.or(if key == egui::Key::Escape {
                self.finish_panels()
            } else {
                self.next()
            });
        }
        event
    }

    /// The art's opacity: fading in over `ART_FADE_MS` with the first
    /// panel, out as the panels end.
    fn art_alpha(&self) -> f32 {
        let Some(on) = self.art_on else { return 0.0 };
        let fade = ART_FADE_MS as f32 / 1000.0;
        let mut a = ease((on.elapsed().as_secs_f32() / fade).min(1.0));
        if let Some(off) = self.art_off {
            a *= 1.0 - ease((off.elapsed().as_secs_f32() / fade).min(1.0));
        }
        a
    }

    /// The title and the panel count: in 300 ms after the art, over 600.
    fn chrome_alpha(&self) -> f32 {
        let Some(on) = self.art_on else { return 0.0 };
        let t = |at: Instant| ((at.elapsed().as_secs_f32() - 0.3) / 0.6).clamp(0.0, 1.0);
        let mut a = ease(t(on));
        if let Some(off) = self.art_off {
            a *= 1.0 - ease(t(off));
        }
        a
    }

    fn paint_card(
        &self,
        painter: &egui::Painter,
        screen: Rect,
        index: usize,
        sharp: egui::TextureId,
        blurred: egui::TextureId,
        alpha: f32,
    ) {
        let to = self.card.panels[index];
        // `place`: the stage and the sharp panel's clip ease from the last
        // panel to this one.
        let elapsed = self.shown.elapsed().as_secs_f32();
        let moving = self.from.map(|from| {
            let t = move_ease(elapsed / (MOVE_MS as f32 / 1000.0));
            (from, t)
        });
        let panel = match moving {
            Some((from, t)) => CardPanel {
                x: from.x + (to.x - from.x) * t,
                y: from.y + (to.y - from.y) * t,
                w: from.w + (to.w - from.w) * t,
                h: from.h + (to.h - from.h) * t,
            },
            None => to,
        };
        let view_of = |p: CardPanel| panel_view(p, screen.width(), screen.height());
        let view = match moving {
            Some((from, t)) => {
                let (a, b) = (view_of(from), view_of(to));
                PanelView {
                    scale: a.scale + (b.scale - a.scale) * t,
                    x: a.x + (b.x - a.x) * t,
                    y: a.y + (b.y - a.y) * t,
                }
            }
            None => view_of(to),
        };
        // `startDrift`: a slow push and turn about the panel's centre, a
        // different way on each one.
        let way = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
        let span = self.dwell(self.from.is_none()).as_secs_f32() + MOVE_MS as f32 / 1000.0;
        let d = ease_in_out((elapsed / span).min(1.0));
        let drift_scale = 1.0 + 0.045 * d;
        let turn = ((-0.7 + 1.2 * d) * way).to_radians();
        let shift = vec2(-7.0 * way * d, -5.0 * d);
        let centre = pos2(to.x + to.w / 2.0, to.y + to.h / 2.0);
        let (sin, cos) = turn.sin_cos();
        let to_screen = |p: Pos2| -> Pos2 {
            // Card pixels → drifted card pixels → screen.
            let r = p - centre;
            let r = vec2(r.x * cos - r.y * sin, r.x * sin + r.y * cos) * drift_scale;
            let q = centre + r + shift;
            pos2(
                screen.left() + view.x + q.x * view.scale,
                screen.top() + view.y + q.y * view.scale,
            )
        };
        let tint = Color32::from_white_alpha((alpha * 255.0) as u8);
        let (w, h) = (self.card.width, self.card.height);
        let quad = |texture: egui::TextureId, r: Rect| {
            let mut mesh = Mesh::with_texture(texture);
            let uv = |p: Pos2| pos2(p.x / w, p.y / h);
            for p in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()] {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: to_screen(p),
                    uv: uv(p),
                    color: tint,
                });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            mesh
        };
        painter.add(quad(blurred, Rect::from_min_size(Pos2::ZERO, vec2(w, h))));
        // `drop-shadow(0 0 24px rgba(0,0,0,0.9))` round the sharp panel.
        let clip = Rect::from_min_size(pos2(panel.x, panel.y), vec2(panel.w, panel.h));
        for (grow, a) in [(24.0, 0.15), (14.0, 0.25), (6.0, 0.35)] {
            let g = grow / view.scale;
            let r = clip.expand(g);
            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].map(to_screen);
            painter.add(Shape::convex_polygon(
                corners.to_vec(),
                Color32::from_black_alpha((a * alpha * 255.0) as u8),
                Stroke::NONE,
            ));
        }
        painter.add(quad(sharp, clip));
        // `rush`: speed lines streak the way the view travels.
        if let Some((from, _)) = moving {
            let t = (elapsed / (MOVE_MS as f32 / 1000.0)).min(1.0);
            if t < 1.0 {
                let dx = to.x + to.w / 2.0 - (from.x + from.w / 2.0);
                let dy = to.y + to.h / 2.0 - (from.y + from.h / 2.0);
                self.paint_speed(painter, screen, dy.atan2(dx), t, alpha);
            }
        }
    }

    /// `.campaign-cut__speed`: uneven streaks along the travel, sweeping
    /// back past the view, clear in the middle where the panel lands.
    fn paint_speed(&self, painter: &egui::Painter, screen: Rect, angle: f32, t: f32, alpha: f32) {
        let opacity = if t < 0.45 {
            ease_in_out(t / 0.45)
        } else {
            1.0 - ease_in_out((t - 0.45) / 0.55)
        };
        let opacity = opacity * alpha;
        if opacity <= 0.0 {
            return;
        }
        let e = ease_in_out(t);
        let reach = screen.width().max(screen.height()) * 2.0;
        let sweep = (0.08 - 0.22 * e) * reach;
        let stretch = 1.0 + 0.35 * e;
        let along = vec2(angle.cos(), angle.sin());
        let across = vec2(-along.y, along.x);
        let centre = screen.center();
        // One period of the repeating gradients: (offset, width, colour).
        let lines: [(f32, f32, Color32); 3] = [
            (9.0, 2.0, hsla(45.0, 100.0, 95.0, 0.8)),
            (23.0, 3.0, hsla(45.0, 100.0, 95.0, 0.5)),
            (31.0, 2.0, hsla(45.0, 100.0, 95.0, 0.7)),
        ];
        let half = reach / 2.0;
        let hole = vec2(screen.width() * 0.34, screen.height() * 0.30);
        let clear = |p: Pos2| {
            let d = p - centre;
            let r = ((d.x / hole.x).powi(2) + (d.y / hole.y).powi(2)).sqrt();
            ((r - 0.4) / 0.6).clamp(0.0, 1.0)
        };
        let mut offset = -half;
        let mut row = 0u32;
        while offset < half {
            let mut streaks: Vec<(f32, f32, Color32)> = lines.iter().map(|&(o, w, c)| (offset + o, w, c)).collect();
            if row.is_multiple_of(2) {
                streaks.push((offset + 61.0 % 47.0, 3.0, hsla(46.0, 100.0, 70.0, 0.65)));
            }
            for (o, w, color) in streaks {
                // Each streak in pieces along its length, faded by the mask.
                let pieces = 32;
                for k in 0..pieces {
                    let s0 = -half + reach * k as f32 / pieces as f32;
                    let s1 = s0 + reach / pieces as f32;
                    let mid = centre + across * (o + w / 2.0) + along * ((s0 + s1) / 2.0 * stretch + sweep);
                    let m = clear(mid);
                    if m <= 0.01 {
                        continue;
                    }
                    let a = centre + across * o + along * (s0 * stretch + sweep);
                    let b = centre + across * o + along * (s1 * stretch + sweep);
                    let c = color.gamma_multiply(opacity * m);
                    painter.add(Shape::convex_polygon(
                        vec![a, b, b + across * w, a + across * w],
                        c,
                        Stroke::NONE,
                    ));
                }
            }
            offset += 47.0;
            row += 1;
        }
    }

    /// `.campaign-cut__vignette`: dark at the corners, clear from the
    /// middle out to 55 %.
    fn paint_vignette(&self, painter: &egui::Painter, screen: Rect, opacity: f32) {
        if self.art_on.is_none() || opacity <= 0.0 {
            return;
        }
        let mut mesh = Mesh::default();
        let c = screen.center();
        let rx = screen.width() / 2.0 * std::f32::consts::SQRT_2;
        let ry = screen.height() / 2.0 * std::f32::consts::SQRT_2;
        let rings = [(0.55, 0.0), (0.7, 0.2), (0.85, 0.45), (1.0, 0.75)];
        let steps = 48;
        for &(r, a) in &rings {
            for k in 0..steps {
                let t = k as f32 / steps as f32 * std::f32::consts::TAU;
                mesh.colored_vertex(
                    pos2(c.x + rx * r * t.cos(), c.y + ry * r * t.sin()),
                    Color32::from_black_alpha((a * opacity * 255.0) as u8),
                );
            }
        }
        for ring in 0..rings.len() as u32 - 1 {
            for k in 0..steps {
                let a = ring * steps + k;
                let b = ring * steps + (k + 1) % steps;
                mesh.add_triangle(a, b, b + steps);
                mesh.add_triangle(a, b + steps, a + steps);
            }
        }
        painter.with_clip_rect(screen).add(mesh);
    }

    /// The mission's name on the yellow plate, and which panel of how many.
    fn paint_chrome(&self, painter: &egui::Painter, screen: Rect, alpha: f32) {
        let layer = painter.clone().with_layer_id(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("campaign-cut-chrome"),
        ));
        let layer = {
            let mut l = layer;
            l.set_opacity(alpha);
            l
        };
        let title_size = (screen.width() * 0.022).clamp(22.0, 38.0);
        campaign_plate(&layer, screen.min + vec2(34.0, 26.0), &self.title, title_size);

        let count = self.card.panels.len();
        let index = self.index.unwrap_or(0);
        let pip_w = 92.0;
        let pip_h = 18.0;
        let pips_w = count as f32 * pip_w + count.saturating_sub(1) as f32 * 10.0;
        let right = screen.right() - 34.0;
        let mid_y = screen.bottom() - 26.0 - 20.0;
        let skew = 24f32.to_radians().tan() * pip_h / 2.0;
        let fill_t = (self.shown.elapsed().as_secs_f32() / self.dwell(self.from.is_none()).as_secs_f32()).min(1.0);
        for i in 0..count {
            let left = right - pips_w + i as f32 * (pip_w + 10.0);
            let r = Rect::from_min_size(pos2(left, mid_y - pip_h / 2.0), vec2(pip_w, pip_h));
            let shape = |r: Rect| {
                vec![
                    pos2(r.left() + skew, r.top()),
                    pos2(r.right() + skew, r.top()),
                    pos2(r.right() - skew, r.bottom()),
                    pos2(r.left() - skew, r.bottom()),
                ]
            };
            layer.add(Shape::convex_polygon(
                shape(r),
                hsla(45.0, 100.0, 95.0, 0.16),
                Stroke::NONE,
            ));
            let fill = if i < index {
                1.0
            } else if i == index {
                fill_t
            } else {
                0.0
            };
            if fill > 0.0 {
                let f = Rect::from_min_size(r.min, vec2(pip_w * fill, pip_h));
                gradient_polygon_x(
                    &layer,
                    &shape(f),
                    &[(0.0, hsl(40.0, 98.0, 47.0)), (1.0, hsl(52.0, 100.0, 66.0))],
                );
            }
        }
        let label = format!("{} / {count}", index + 1);
        let g = text(&layer, &label, 15.0, display_family(), Color32::BLACK, 15.0 * 0.06);
        let plate_w = g.size().x + 0.95 * 15.0 + 6.0;
        let plate_h = g.size().y + 0.26 * 15.0 + 2.0;
        campaign_plate(
            &layer,
            pos2(right - pips_w - 18.0 - plate_w, mid_y - plate_h / 2.0),
            &label,
            15.0,
        );
    }

    /// `.campaign-cut__status` under the black screen, and "Back to
    /// campaign" after a failure.
    fn paint_status(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        screen: Rect,
        opacity: f32,
    ) -> Option<CutsceneEvent> {
        let error = self.phase == Phase::Error;
        let shown = match (self.phase, self.waiting_since) {
            (Phase::Error, _) => 1.0,
            (_, Some(at)) => ease(((at.elapsed().as_secs_f32() - 0.4) / 0.5).clamp(0.0, 1.0)),
            _ => 0.0,
        } * opacity;
        if shown > 0.0 && !self.status.is_empty() {
            let (size, spacing, label, color) = if error {
                (18.0, 0.12, self.status.clone(), hsl(46.0, 100.0, 60.0))
            } else {
                // `campaign-cut-pulse`: 1.6 s each way.
                let t = self.waiting_since.map_or(0.0, |at| at.elapsed().as_secs_f32()) / 1.6;
                let swing = if (t as u32).is_multiple_of(2) {
                    t.fract()
                } else {
                    1.0 - t.fract()
                };
                let a = 0.35 + 0.45 * ease_in_out(swing);
                (14.0, 0.3, self.status.to_uppercase(), hsla(45.0, 100.0, 95.0, a))
            };
            let g = text(
                painter,
                &label,
                size,
                egui::FontFamily::Proportional,
                color,
                size * spacing,
            );
            let at = screen.center() - g.size() / 2.0;
            painter.galley(at, g, color.gamma_multiply(shown));
        }
        if !error {
            return None;
        }
        let g = text(
            painter,
            "BACK TO CAMPAIGN",
            14.0,
            egui::FontFamily::Proportional,
            Color32::WHITE,
            14.0 * 0.2,
        );
        let size = g.size() + vec2(44.0, 20.0);
        let rect = Rect::from_min_size(pos2(screen.center().x - size.x / 2.0, screen.center().y + 48.0), size);
        let mut back = None;
        egui::Area::new(egui::Id::unique("campaign-cut-back"))
            .order(egui::Order::Tooltip)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(rect, egui::Sense::click());
                let hot = response.hovered() || response.has_focus();
                let color = if hot {
                    hsl(46.0, 100.0, 57.0)
                } else {
                    hsla(45.0, 100.0, 95.0, 1.0)
                };
                let border = if hot {
                    hsl(46.0, 100.0, 57.0)
                } else {
                    hsla(45.0, 100.0, 95.0, 0.7)
                };
                let p = ui.painter();
                p.rect_stroke(rect, 0.0, Stroke::new(2.0, border), egui::StrokeKind::Inside);
                let text_at = rect.center() - g.size() / 2.0;
                p.galley(text_at, g, color);
                if response.clicked() {
                    back = Some(CutsceneEvent::Back);
                }
            });
        if back.is_some() {
            self.close();
        }
        back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_fills_as_much_of_the_screen_as_fits_and_sits_in_the_middle() {
        let panel = CardPanel {
            x: 14.0,
            y: 14.0,
            w: 707.0,
            h: 836.0,
        };
        let view = panel_view(panel, 1920.0, 1080.0);
        assert!((view.scale - 1080.0 * 0.84 / 836.0).abs() < 1e-5);
        let centre_x = view.x + (panel.x + panel.w / 2.0) * view.scale;
        let centre_y = view.y + (panel.y + panel.h / 2.0) * view.scale;
        assert!((centre_x - 960.0).abs() < 1e-3);
        assert!((centre_y - 540.0).abs() < 1e-3);
    }

    #[test]
    fn the_move_eases_slowly_off_and_softly_in() {
        assert!(move_ease(0.0).abs() < 1e-4);
        assert!((move_ease(1.0) - 1.0).abs() < 1e-4);
        assert!((move_ease(0.5) - 0.5).abs() < 1e-3);
        assert!(move_ease(0.15) < 0.05);
    }

    #[test]
    fn a_card_without_panels_goes_straight_to_black() {
        let mut cut = CampaignCutscene::new(None, "x");
        assert_eq!(cut.finish_panels(), Some(CutsceneEvent::PanelsDone));
        assert!(cut.panels_read());
        // Once.
        assert_eq!(cut.finish_panels(), None);
        cut.fail("no");
        assert!(cut.is_open());
        cut.close();
        assert!(!cut.is_open());
    }
}
