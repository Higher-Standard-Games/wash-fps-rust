//! wash-fps `client/IdentityFrame.ts` and the frame in `styles/Identity.css`:
//! the frame the Welcome and Profile screens share. The gold panel, the rail
//! widened to hold one question at a time, beside a dark stage with its light
//! and backdrop word. It opens by widening out of the rail and closes by
//! narrowing back onto it.
//!
//! The building blocks of a screen are here too (`identityScreen`, the action
//! keys, the warning, the one key that matters and the rows of small keys
//! under it), so both screens set them alike.

use std::time::Instant;

use shagger::ui::components::lobby_twin::{
    RAIL_SKEW, clamp_css, gradient_polygon, hsl, hsla, paint_backdrop, paint_light_in, text,
};
use shagger::ui::components::outline_text::paint_outline_text;
use shagger::ui::components::raised_key::{KeyFace, KeyLook, KeyText, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::client::settings::settings_fields::body;
use crate::client::washoi_style::{DISPLAY_FONT, display_family};

/// `MOVE_MS`: the panel's move, and when a closing frame goes.
const MOVE: f32 = 0.72;
/// `transition: flex-basis 0.7s cubic-bezier(0.7, 0, 0.2, 1)`.
const GROW: f32 = 0.7;

fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 8.0, a)
}

/// CSS `cubic-bezier(x1, y1, x2, y2)` at `t`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let curve = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s).powi(2) + 3.0 * b * s * s * (1.0 - s) + s.powi(3);
    // Find s whose x is t, by bisection: x is monotonic in s.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if curve(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    curve(y1, y2, (lo + hi) / 2.0)
}

/// The frame's state: when it opened, and when it started closing.
#[derive(Debug, Default)]
pub struct IdentityFrame {
    opened: Option<Instant>,
    closing: Option<Instant>,
}

/// Where a screen goes, as this frame laid it out.
#[derive(Debug, Clone, Copy)]
pub struct FrameLayout {
    /// The panel, its slanted edge included.
    pub panel: Rect,
    /// `welcome__inner`: up to 640 wide, centred in the panel's padding.
    pub column: Rect,
    /// The dark side, from under the panel's slant to the right.
    pub stage: Rect,
    /// `max-height: 820px`: a smaller logo, tighter screens and keys.
    pub short: bool,
    /// The screens' and the stage's opacity, fading in as the panel grows.
    pub inner_alpha: f32,
    pub stage_alpha: f32,
}

impl IdentityFrame {
    /// `widenFromRail`: open, starting at the rail's width.
    pub fn open(&mut self) {
        self.opened = Some(Instant::now());
        self.closing = None;
    }

    /// `narrowOntoRail`: narrow back onto the rail, then go.
    pub fn close(&mut self) {
        if self.opened.is_some() && self.closing.is_none() {
            self.closing = Some(Instant::now());
        }
    }

    /// Shown at all, opening, open or closing.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.opened.is_some()
    }

    #[must_use]
    pub fn closing(&self) -> bool {
        self.closing.is_some()
    }

    /// Open and still: nothing of the lobby shows through.
    #[must_use]
    pub fn covers(&self) -> bool {
        self.closing.is_none() && self.opened.is_some_and(|at| at.elapsed().as_secs_f32() >= GROW)
    }

    /// True once, when a closing frame has gone.
    pub fn finished(&mut self) -> bool {
        if self.closing.is_some_and(|at| at.elapsed().as_secs_f32() >= MOVE) {
            self.opened = None;
            self.closing = None;
            return true;
        }
        false
    }

    /// The panel's share of the width, the screens' and the stage's
    /// opacity, and the whole overlay's.
    fn motion(&self) -> (f32, f32, f32, f32) {
        let ease = |t: f32| cubic_bezier(0.7, 0.0, 0.2, 1.0, t.clamp(0.0, 1.0));
        let linear = |t: f32| t.clamp(0.0, 1.0);
        if let Some(at) = self.closing {
            let t = at.elapsed().as_secs_f32();
            // `.welcome--leaving`: the panel narrows, the rest fades, and the
            // overlay goes after 0.4s over 0.25s.
            return (
                1.0 - ease(t / GROW),
                1.0 - linear(t / 0.25),
                1.0 - linear(t / 0.4),
                1.0 - linear((t - 0.4) / 0.25),
            );
        }
        let t = self.opened.map_or(1.0, |at| at.elapsed().as_secs_f32());
        (ease(t / GROW), linear(t / 0.25), linear(t / 0.4), 1.0)
    }

    /// Draws the frame over `screen`: the stage with its light, ghost word
    /// and whatever `stage` puts on it, then the panel. The panel's screens
    /// are the caller's, in the returned column, drawn with
    /// [`FrameLayout::inner_alpha`]. `small_ghost` is the Profile's (a name
    /// can be long).
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        ui: &mut egui::Ui,
        screen: Rect,
        time: f64,
        ghost: &str,
        small_ghost: bool,
        accent: Color32,
        stage: impl FnOnce(&mut egui::Ui, Rect),
    ) -> FrameLayout {
        let (grow, inner_alpha, stage_alpha, alpha) = self.motion();
        ui.set_opacity(alpha);
        let (w, h) = (screen.width(), screen.height());
        let short = w > 700.0 && h <= 820.0;
        // `flex: 0 0 60%` (78% under 1100px), from the rail's width.
        let rail = clamp_css(300.0, w * 0.26, 520.0);
        let full = w * if w <= 1100.0 { 0.78 } else { 0.6 };
        let panel_w = rail + (full - rail) * grow;
        let panel = Rect::from_min_size(screen.min, vec2(panel_w, h));
        let stage_r = Rect::from_min_max(pos2(panel.right() - RAIL_SKEW, screen.top()), screen.max);

        let painter = ui.painter().clone();
        paint_backdrop(&painter, screen);
        ui.scope(|ui| {
            ui.set_opacity(alpha * stage_alpha);
            ui.set_clip_rect(stage_r);
            let painter = ui.painter().clone();
            paint_light_in(&painter, stage_r, time, accent);
            // `.lobby__ghost` on the stage: off the bottom left.
            let size = if small_ghost {
                clamp_css(110.0, w * 0.12, 240.0)
            } else {
                clamp_css(150.0, w * 0.17, 340.0)
            };
            let at = pos2(stage_r.left() + stage_r.width() * 0.08, stage_r.bottom() + h * 0.05);
            paint_outline_text(
                &painter,
                DISPLAY_FONT,
                &ghost.to_uppercase(),
                size,
                -0.04 * size,
                at,
                Stroke::new(2.0, hsla(182.0, 95.0, 52.0, 0.26)),
            );
            stage(ui, stage_r);
        });

        // `welcome__panel`: gold, its right edge slanted and lit violet.
        let shape = [
            panel.left_top(),
            panel.right_top(),
            pos2(panel.right() - RAIL_SKEW, panel.bottom()),
            panel.left_bottom(),
        ];
        gradient_polygon(
            &painter,
            &shape,
            &[(0.0, hsl(46.0, 100.0, 57.0)), (1.0, hsl(41.0, 96.0, 48.0))],
        );
        // `radial-gradient(120% 55% at 25% 0%, ...)`, kept inside the panel.
        glow(
            &painter.with_clip_rect(panel),
            pos2(panel.left() + panel_w * 0.25, panel.top()),
            vec2(panel_w * 1.2 * 0.62, h * 0.55 * 0.62),
            hsla(54.0, 100.0, 78.0, 0.55),
            &shape,
        );
        kanji(&painter, panel, w);
        gradient_polygon(
            &painter,
            &[
                pos2(panel.right() - 8.0, panel.top()),
                panel.right_top(),
                pos2(panel.right() - RAIL_SKEW, panel.bottom()),
                pos2(panel.right() - RAIL_SKEW - 8.0, panel.bottom()),
            ],
            &[(0.0, hsl(276.0, 90.0, 64.0)), (1.0, hsl(262.0, 75.0, 42.0))],
        );

        // Padding: 36 (24 short) top and bottom, 60 left, skew + 56 right.
        let pad_y = if short { 24.0 } else { 36.0 };
        let room = panel_w - 60.0 - (RAIL_SKEW + 56.0);
        let column_w = room.clamp(0.0, 640.0);
        let left = panel.left() + 60.0 + (room - column_w).max(0.0) / 2.0;
        let column = Rect::from_min_max(
            pos2(left, panel.top() + pad_y),
            pos2(left + column_w, panel.bottom() - pad_y),
        );
        FrameLayout {
            panel,
            column,
            stage: stage_r,
            short,
            inner_alpha,
            stage_alpha,
        }
    }
}

/// A soft ellipse of `color` fading out at `radius`, within the convex `clip`.
fn glow(painter: &Painter, centre: Pos2, radius: egui::Vec2, color: Color32, clip: &[Pos2]) {
    const SEGMENTS: u32 = 48;
    const RINGS: u32 = 8;
    let inside = |p: Pos2| {
        // Left of every edge, the shape being clockwise on screen.
        clip.iter().zip(clip.iter().cycle().skip(1)).all(|(a, b)| {
            let (e, v) = (*b - *a, p - *a);
            e.x * v.y - e.y * v.x >= 0.0
        })
    };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, color);
    for ring in 1..=RINGS {
        let r = ring as f32 / RINGS as f32;
        let fall = 1.0 - r;
        for s in 0..SEGMENTS {
            let a = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let p = centre + vec2(a.cos() * radius.x * r, a.sin() * radius.y * r);
            let c = if inside(p) {
                color.gamma_multiply(fall)
            } else {
                Color32::TRANSPARENT
            };
            mesh.colored_vertex(p, c);
        }
    }
    for s in 0..SEGMENTS {
        mesh.add_triangle(0, 1 + s, 1 + (s + 1) % SEGMENTS);
    }
    for ring in 0..RINGS - 1 {
        let a = 1 + ring * SEGMENTS;
        let b = a + SEGMENTS;
        for s in 0..SEGMENTS {
            let n = (s + 1) % SEGMENTS;
            mesh.add_triangle(a + s, b + s, b + n);
            mesh.add_triangle(a + s, b + n, a + n);
        }
    }
    painter.add(mesh);
}

/// `welcome__kanji`: 名 faint in the panel's lower right, drawn in strokes
/// (the game's faces have no kanji).
fn kanji(painter: &Painter, panel: Rect, screen_w: f32) {
    let size = clamp_css(220.0, screen_w * 0.24, 420.0);
    let origin = pos2(
        panel.right() - panel.width() * 0.14 - size * 0.98,
        panel.bottom() - panel.height() * 0.06 - size * 0.9,
    );
    let at = |x: f32, y: f32| origin + vec2(x * size, y * size);
    // `hsla(30, 90%, 22%, 0.09)` laid on the gold where each stroke is, so
    // strokes that cross don't darken twice.
    let ink_on_gold = |y: f32| {
        let t = ((y - panel.top()) / panel.height()).clamp(0.0, 1.0);
        let gold = hsl(46.0, 100.0, 57.0).lerp_to_gamma(hsl(41.0, 96.0, 48.0), t);
        gold.lerp_to_gamma(hsl(30.0, 90.0, 22.0), 0.09)
    };
    // 夕 over 口: the sweep, the fold, the dot, then the box on its legs.
    let strokes: [&[(f32, f32)]; 6] = [
        &[(0.45, 0.02), (0.03, 0.73)],
        &[(0.29, 0.17), (0.84, 0.17), (0.38, 0.58)],
        &[(0.22, 0.25), (0.56, 0.44)],
        &[(0.375, 0.58), (0.375, 0.99)],
        &[(0.375, 0.58), (0.84, 0.58), (0.84, 0.99)],
        &[(0.375, 0.9), (0.84, 0.9)],
    ];
    for points in strokes {
        let line: Vec<Pos2> = points.iter().map(|&(x, y)| at(x, y)).collect();
        let mid = line.iter().map(|p| p.y).sum::<f32>() / line.len() as f32;
        painter.add(Shape::line(line, Stroke::new(size * 0.078, ink_on_gold(mid))));
    }
}

/// A run of the lead: plain, or bold (`<b>`, in full ink).
pub type Lead<'a> = &'a [(&'a str, bool)];

/// `identityScreen`: a kicker over a big title, and a line under it.
/// Returns the bottom of the last part.
#[allow(clippy::too_many_arguments)]
pub fn screen_head(
    painter: &Painter,
    left: f32,
    top: f32,
    width: f32,
    short: bool,
    kicker: &str,
    title: &str,
    lead: Lead<'_>,
) -> f32 {
    let gap = gap(short);
    let screen_w = painter.ctx().content_rect().width();
    let k = text(painter, &kicker.to_uppercase(), 13.0, body(), ink(0.62), 13.0 * 0.32);
    let mut y = top;
    let kh = k.size().y;
    painter.galley(pos2(left, y), k, Color32::WHITE);
    y += kh + gap;
    let size = if short {
        clamp_css(32.0, screen_w * 0.032, 52.0)
    } else {
        clamp_css(38.0, screen_w * 0.038, 66.0)
    };
    let t = text(painter, title, size, display_family(), ink(1.0), size * 0.01);
    // `line-height: 0.95`: the box is tighter than the face's own line.
    let th = size * 0.95;
    let ty = y - (t.size().y - th) / 2.0;
    painter.galley_with_override_text_color(pos2(left + 4.0, ty + 5.0), t.clone(), hsla(30.0, 90.0, 32.0, 0.3));
    painter.galley(pos2(left, ty), t, Color32::WHITE);
    y += th;
    if !lead.is_empty() {
        let g = lead_galley(painter, lead, width.min(548.0));
        y += gap;
        let lh = g.size().y;
        painter.galley(pos2(left, y), g, Color32::WHITE);
        y += lh;
    }
    y
}

/// `welcome__lead`: 20px, the bold runs in full ink.
fn lead_galley(painter: &Painter, lead: Lead<'_>, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width.max(1.0);
    for (run, bold) in lead {
        job.append(
            run,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(20.0, FontFamily::Proportional),
                color: if *bold { ink(1.0) } else { ink(0.78) },
                line_height: Some(26.0),
                ..Default::default()
            },
        );
    }
    painter.layout_job(job)
}

/// `.welcome__screen`'s gap.
#[must_use]
pub fn gap(short: bool) -> f32 {
    if short { 12.0 } else { 16.0 }
}

/// A key's lettering, uppercase in the display face.
fn label_galley(painter: &Painter, label: &str, size: f32, em: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    text(painter, &label.to_uppercase(), size, display_family(), color, size * em)
}

/// `welcome__actions`: cream keys in a row that wraps, 54 tall, 16 apart.
/// `labels` may differ from the ids for a moment (`Copied ✓`). Returns the
/// key pressed and the bottom.
pub fn action_keys(
    ui: &mut egui::Ui,
    id: &str,
    left: f32,
    top: f32,
    width: f32,
    labels: &[&str],
) -> (Option<usize>, f32) {
    keys_in_row(
        ui,
        id,
        left,
        top,
        width,
        labels,
        KeyLook::cream(),
        54.0,
        15.0,
        0.1,
        (26.0, 24.0),
    )
}

#[allow(clippy::too_many_arguments)]
fn keys_in_row(
    ui: &mut egui::Ui,
    id: &str,
    left: f32,
    top: f32,
    width: f32,
    labels: &[&str],
    look: KeyLook,
    h: f32,
    size: f32,
    em: f32,
    pad: (f32, f32),
) -> (Option<usize>, f32) {
    let painter = ui.painter().clone();
    let gap = 16.0;
    // `raised-key--inline`: the side's reach to the right, and the depth below.
    let (reach, below) = (look.depth * 0.75 + 2.0, look.depth);
    let (mut x, mut y) = (left, top);
    let mut pressed = None;
    for (i, label) in labels.iter().enumerate() {
        let lw = label_galley(&painter, label, size, em, Color32::WHITE).size().x;
        let kw = lw + pad.0 + pad.1;
        if x > left && x + kw + reach > left + width {
            x = left;
            y += h + below + gap;
        }
        let rect = Rect::from_min_size(pos2(x, y), vec2(kw, h));
        let face = RaisedKey::new((id, i), rect, look.clone()).show(ui);
        KeyText::new(label, "", display_family(), body())
            .size(size, 13.0)
            .spacing(em)
            .pad(pad.0, pad.1)
            .paint(&painter, &face);
        if face.clicked {
            pressed = Some(i);
        }
        x += kw + reach + gap;
    }
    (pressed, y + h + below)
}

/// `.identity-warn`: a bold start and the rest, on a faint ink band with an
/// ink bar at its left. Returns its bottom.
pub fn warn(painter: &Painter, left: f32, top: f32, width: f32, bold: &str, rest: &str) -> f32 {
    let r = warn_box(painter, left, top, width, bold, rest, 0.0);
    r.bottom()
}

/// [`warn`] with room for a box at its start (`identity-agree`).
fn warn_box(painter: &Painter, left: f32, top: f32, width: f32, bold: &str, rest: &str, indent: f32) -> Rect {
    let w = width.min(546.0);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (w - 18.0 - 16.0 - 5.0 - indent).max(1.0);
    let format = |color| egui::TextFormat {
        font_id: FontId::new(16.0, FontFamily::Proportional),
        color,
        line_height: Some(21.6),
        ..Default::default()
    };
    job.append(bold, 0.0, format(ink(1.0)));
    job.append(rest, 0.0, format(ink(0.82)));
    let g = painter.layout_job(job);
    let r = Rect::from_min_size(pos2(left, top), vec2(w, g.size().y + 24.0));
    painter.rect_filled(r, 0.0, ink(0.08));
    painter.rect_filled(Rect::from_min_size(r.min, vec2(5.0, r.height())), 0.0, ink(1.0));
    painter.galley(pos2(left + 5.0 + 18.0 + indent, top + 12.0), g, Color32::WHITE);
    r
}

/// `identity-agree`: the warning with a box to tick. Returns whether it
/// was clicked and its bottom.
#[allow(clippy::too_many_arguments)]
pub fn agree_box(
    ui: &mut egui::Ui,
    id: &str,
    left: f32,
    top: f32,
    width: f32,
    checked: bool,
    bold: &str,
    rest: &str,
) -> (bool, f32) {
    let painter = ui.painter().clone();
    let r = warn_box(&painter, left, top, width, bold, rest, 28.0);
    let response = ui.interact(r, egui::Id::NULL.with(("agree", id)), egui::Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // An 18px box, inked when ticked (`accent-color: var(--ink)`).
    let b = Rect::from_min_size(pos2(left + 5.0 + 18.0, top + 14.0), vec2(18.0, 18.0));
    if checked {
        painter.rect_filled(b, 3.0, ink(1.0));
        painter.add(Shape::line(
            vec![b.min + vec2(4.0, 9.5), b.min + vec2(7.5, 13.0), b.min + vec2(14.0, 5.0)],
            Stroke::new(2.4, hsl(46.0, 100.0, 60.0)),
        ));
    } else {
        painter.rect_filled(b, 3.0, hsla(45.0, 100.0, 97.0, 0.9));
        painter.rect_stroke(b, 3.0, Stroke::new(1.5, ink(0.7)), egui::StrokeKind::Inside);
    }
    (response.clicked(), r.bottom())
}

/// `raised-key--go`: the one key that matters, ink at rest and violet under
/// the pointer, its hint under the label. `arrow` adds the label's `→`.
/// Returns whether it was pressed and its bottom.
#[allow(clippy::too_many_arguments)]
pub fn go_key(
    ui: &mut egui::Ui,
    id: &str,
    left: f32,
    top: f32,
    short: bool,
    label: &str,
    hint: &str,
    arrow: bool,
    disabled: bool,
) -> (bool, f32) {
    let painter = ui.painter().clone();
    let look = KeyLook {
        cut: 0.04,
        ..KeyLook::ink().depth(10.2)
    };
    let l = label_galley(&painter, label, 32.0, 0.12, Color32::WHITE);
    let arrow_w = if arrow { 32.0 * 0.12 + 30.0 } else { 0.0 };
    let hint_g = text(
        &painter,
        &hint.to_uppercase(),
        13.0,
        body(),
        Color32::WHITE,
        13.0 * 0.12,
    );
    let w = (l.size().x + arrow_w).max(hint_g.size().x) + 96.0;
    let h = if short { 74.0 } else { 92.0 };
    // `.welcome__go { margin-top: 8px }`.
    let rect = Rect::from_min_size(pos2(left, top + 8.0), vec2(w.max(260.0), h));
    let face: KeyFace = RaisedKey::new(("go", id), rect, look.clone())
        .disabled(disabled)
        .show(ui);
    // `:disabled`: the face stays; the lettering goes to a third.
    let (text_c, sub_c) = if disabled {
        (look.text.gamma_multiply(0.35), look.sub.gamma_multiply(0.35))
    } else {
        (face.text, face.sub)
    };
    let l = label_galley(&painter, label, 32.0, 0.12, text_c);
    let hint_g = text(&painter, &hint.to_uppercase(), 13.0, body(), sub_c, 13.0 * 0.12);
    let total = l.size().y + 8.0 + hint_g.size().y;
    let top_y = face.rect.center().y - total / 2.0;
    let line_w = l.size().x + arrow_w;
    let lx = face.rect.center().x - line_w / 2.0;
    let lh = l.size().y;
    painter.galley(pos2(lx, top_y), l.clone(), text_c);
    if arrow {
        // `→`, drawn: the display face has none.
        let y = top_y + lh * 0.55;
        let x0 = lx + l.size().x + 14.0;
        let x1 = x0 + 18.0;
        let s = Stroke::new(2.6, text_c);
        painter.add(Shape::line_segment([pos2(x0, y), pos2(x1, y)], s));
        painter.add(Shape::line(
            vec![pos2(x1 - 6.0, y - 5.0), pos2(x1, y), pos2(x1 - 6.0, y + 5.0)],
            s,
        ));
    }
    painter.galley(
        pos2(face.rect.center().x - hint_g.size().x / 2.0, top_y + lh + 8.0),
        hint_g,
        sub_c,
    );
    (face.clicked, rect.bottom() + look.depth)
}

/// `welcome__alt`: a question, then small cream keys to where it leads.
/// Returns the key pressed and the bottom.
pub fn alt_row(
    ui: &mut egui::Ui,
    id: &str,
    left: f32,
    top: f32,
    width: f32,
    question: &str,
    labels: &[&str],
) -> (Option<usize>, f32) {
    let painter = ui.painter().clone();
    let q = text(&painter, question, 16.0, body(), ink(0.7), 0.0);
    // `margin-top: 4px`; the keys are 46 tall and the row is centred on them.
    let top = top + 4.0;
    let qw = q.size().x;
    painter.galley(pos2(left, top + 23.0 - q.size().y / 2.0), q, Color32::WHITE);
    keys_in_row(
        ui,
        id,
        left + qw + 16.0,
        top,
        width - qw - 16.0,
        labels,
        KeyLook::cream().sm(),
        46.0,
        14.0,
        0.07,
        (20.0, 16.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_eases_as_the_browser_does() {
        assert!((cubic_bezier(0.7, 0.0, 0.2, 1.0, 0.0)).abs() < 1e-4);
        assert!((cubic_bezier(0.7, 0.0, 0.2, 1.0, 1.0) - 1.0).abs() < 1e-4);
        // Slow out of the rail, then quick: a third of the way in, it has
        // barely moved.
        assert!(cubic_bezier(0.7, 0.0, 0.2, 1.0, 0.3) < 0.15);
        assert!((cubic_bezier(0.25, 0.25, 0.75, 0.75, 0.4) - 0.4).abs() < 1e-3);
    }

    #[test]
    fn a_frame_opens_and_goes_after_its_move() {
        let mut frame = IdentityFrame::default();
        assert!(!frame.is_open());
        frame.open();
        assert!(frame.is_open() && !frame.covers() && !frame.finished());
        frame.close();
        assert!(frame.closing());
        frame.closing = Some(Instant::now() - std::time::Duration::from_secs(1));
        assert!(frame.finished());
        assert!(!frame.is_open());
    }
}
