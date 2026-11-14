//! wash-fps `client/settings/SettingsFields.ts` and the shared controls of
//! `FPSSettingsView.css`: the card, a field's head, the slider, the row of
//! keys to pick one of, colour swatches, the drop-down and the frame-cost
//! pips. Each control draws itself into the `Ui`'s next free space at the
//! width it is given, and says what the player picked; the pages read and
//! write the cvars.

use std::sync::Arc;

use shagger::ui::components::lobby_twin::{cream, gradient_polygon, hsl, hsla, ink, text, violet};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::client::washoi_style::display_family;

/// `--body`.
#[must_use]
pub fn body() -> FontFamily {
    FontFamily::Proportional
}

/// `--gold-text`.
#[must_use]
pub fn gold_text() -> Color32 {
    hsl(46.0, 100.0, 60.0)
}

/// `text` with letter spacing in em.
#[must_use]
pub fn line(painter: &Painter, s: &str, size: f32, family: FontFamily, color: Color32, em: f32) -> Arc<egui::Galley> {
    text(painter, s, size, family, color, size * em)
}

/// Text wrapped at `width`.
#[must_use]
pub fn wrapped(painter: &Painter, s: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(s.to_owned(), font, color, width.max(1.0));
    job.wrap.break_anywhere = false;
    painter.layout_job(job)
}

/// The cut-cornered slab (`clip-path` off its top right).
pub fn cut_panel(painter: &Painter, r: Rect, cut: f32, stops: &[(f32, Color32)], border: Option<Stroke>) {
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

/// `formatPercent`.
#[must_use]
pub fn format_percent(v: f64) -> String {
    format!("{}%", shagger::shared::js::js_round(v * 100.0))
}

/// A number as the page shows it: two decimals at most, no trailing zeros.
#[must_use]
pub fn plain(v: f64) -> String {
    shagger::shared::js::js_number_to_string(shagger::shared::js::js_round(v * 100.0) / 100.0)
}

/// `.fps-set-card`: a dark slab with a violet edge, its title, and an aside.
/// `add` draws the body at the card's inner width.
pub fn card<R>(ui: &mut egui::Ui, title: &str, note: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let width = ui.available_width();
    let top = ui.cursor().min;
    let bg = ui.painter().add(Shape::Noop);
    let (pad_x, pad_top, pad_bottom) = (22.0, 16.0, 20.0);
    let inner = Rect::from_min_size(top + vec2(pad_x, pad_top), vec2(width - 2.0 * pad_x, f32::INFINITY));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let painter = child.painter().clone();
    let title_g = line(&painter, &title.to_uppercase(), 15.0, display_family(), cream(), 0.16);
    let th = title_g.size().y;
    painter.galley(inner.min, title_g, cream());
    if !note.is_empty() {
        let note_g = line(
            &painter,
            &note.to_uppercase(),
            12.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.45),
            0.14,
        );
        let at = pos2(inner.right() - note_g.size().x, inner.top() + th - note_g.size().y);
        painter.galley(at, note_g, cream());
    }
    child.add_space(th + 14.0);
    let r = add(&mut child);
    let bottom = child.min_rect().bottom() + pad_bottom;
    let rect = Rect::from_min_max(top, pos2(top.x + width, bottom));
    let shape = [
        rect.left_top(),
        pos2(rect.right() - 20.0, rect.top()),
        pos2(rect.right(), rect.top() + 20.0),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let fill = mesh_polygon(
        &shape,
        &[
            (0.0, hsla(250.0, 40.0, 11.0, 0.78)),
            (1.0, hsla(250.0, 44.0, 7.0, 0.86)),
        ],
    );
    ui.painter().set(
        bg,
        Shape::Vec(vec![
            fill,
            Shape::closed_line(shape.to_vec(), Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.28))),
        ]),
    );
    ui.allocate_rect(rect, egui::Sense::hover());
    r
}

/// A vertical gradient over a convex polygon, as one shape (for a slot
/// painted under what is drawn after it).
pub fn mesh_polygon(points: &[Pos2], stops: &[(f32, Color32)]) -> Shape {
    let top = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let bottom = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
    let at = |y: f32| {
        let t = if bottom > top { (y - top) / (bottom - top) } else { 0.0 };
        let mut c = stops[0].1;
        for w in stops.windows(2) {
            let (a, b) = (w[0], w[1]);
            if t >= a.0 && t <= b.0 {
                let k = if b.0 > a.0 { (t - a.0) / (b.0 - a.0) } else { 0.0 };
                c = a.1.lerp_to_gamma(b.1, k);
            } else if t > b.0 {
                c = b.1;
            }
        }
        c
    };
    let mut mesh = egui::Mesh::default();
    for p in points {
        mesh.colored_vertex(*p, at(p.y));
    }
    for i in 1..points.len().saturating_sub(1) {
        mesh.add_triangle(0, i as u32, i as u32 + 1);
    }
    // One physical pixel at the usual scale; the shape has no painter here.
    shagger::ui::components::lobby_twin::feather_convex(&mut mesh, points, |p| at(p.y), 1.0);
    Shape::mesh(mesh)
}

/// What a field's head shows besides its name.
#[derive(Debug, Clone, Default)]
pub struct Head<'a> {
    pub name: &'a str,
    /// `fps-set-field__val`, and whether it is the default (`· default`).
    pub value: Option<(String, bool)>,
    /// `fps-set-field__tag`: when a setting takes hold, such as "Next map".
    pub tag: Option<&'a str>,
    /// Frame cost pips (1 to 3).
    pub cost: Option<u8>,
}

impl<'a> Head<'a> {
    #[must_use]
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn value(mut self, value: String, is_default: bool) -> Self {
        self.value = Some((value, is_default));
        self
    }

    #[must_use]
    pub fn tag(mut self, tag: Option<&'a str>) -> Self {
        self.tag = tag;
        self
    }

    #[must_use]
    pub fn cost(mut self, cost: Option<u8>) -> Self {
        self.cost = cost;
        self
    }
}

/// `.fps-set-field__top`: the name, with its tag and cost, and the value at
/// the right. Returns its height.
fn head(ui: &mut egui::Ui, h: &Head<'_>, width: f32, alpha: f32) -> f32 {
    let painter = ui.painter().clone();
    let at = ui.cursor().min;
    let name = line(
        &painter,
        &h.name.to_uppercase(),
        15.0,
        body(),
        hsla(45.0, 100.0, 95.0, 0.82 * alpha),
        0.14,
    );
    let mut height = name.size().y;
    let name_w = name.size().x;
    let mut x = at.x + name_w + 9.0;
    if let Some(tag) = h.tag {
        let g = line(&painter, &tag.to_uppercase(), 10.0, body(), ink(), 0.14);
        let r = Rect::from_min_size(pos2(x, at.y + (height - 15.0) / 2.0), g.size() + vec2(12.0, 5.0));
        painter.rect_filled(r, 2.0, hsl(276.0, 90.0, 72.0).gamma_multiply(alpha));
        painter.galley(r.min + vec2(6.0, 3.0), g, ink());
        x = r.right() + 9.0;
    }
    if let Some(pips) = h.cost {
        cost_pips(&painter, pos2(x, at.y + height / 2.0 - 4.0), pips, alpha);
    }
    painter.galley(at, name, cream());
    if let Some((value, is_default)) = &h.value {
        let v = line(
            &painter,
            value,
            20.0,
            display_family(),
            cream().gamma_multiply(alpha),
            0.0,
        );
        let mut right = at.x + width;
        if *is_default {
            let d = line(
                &painter,
                " · DEFAULT",
                11.0,
                body(),
                hsla(45.0, 100.0, 95.0, 0.38 * alpha),
                0.14,
            );
            right -= d.size().x;
            painter.galley(pos2(right, at.y + v.size().y - d.size().y - 2.0), d, cream());
        }
        let vh = v.size().y;
        painter.galley(pos2(right - v.size().x, at.y + height - vh + 2.0), v, cream());
        height = height.max(vh - 2.0);
    }
    height
}

/// `.fps-set-cost`: three slanted pips, `lit` of them coloured by how dear.
pub fn cost_pips(painter: &Painter, at: Pos2, lit: u8, alpha: f32) {
    let color = match lit {
        1 => hsl(96.0, 85.0, 55.0),
        2 => hsl(40.0, 100.0, 58.0),
        _ => hsl(356.0, 90.0, 62.0),
    };
    for i in 0..3u8 {
        let x = at.x + f32::from(i) * 10.0;
        let fill = if i < lit { color } else { hsla(45.0, 100.0, 95.0, 0.14) };
        painter.add(Shape::convex_polygon(
            vec![
                pos2(x + 2.0, at.y),
                pos2(x + 8.0, at.y),
                pos2(x + 6.0, at.y + 8.0),
                pos2(x, at.y + 8.0),
            ],
            fill.gamma_multiply(alpha),
            Stroke::NONE,
        ));
    }
}

/// `.fps-set-field__help`, wrapped at `width`; returns its height.
fn help(ui: &mut egui::Ui, s: &str, width: f32, alpha: f32) -> f32 {
    let painter = ui.painter().clone();
    let g = wrapped(
        &painter,
        s,
        FontId::new(13.0, body()),
        hsla(45.0, 100.0, 95.0, 0.45 * alpha),
        width,
    );
    let h = g.size().y;
    painter.galley(ui.cursor().min, g, cream());
    h
}

/// The field's frame: head, control, help; a disabled field is dimmed
/// (`aria-disabled`, opacity 0.35) and takes no input.
fn field<R>(
    ui: &mut egui::Ui,
    h: &Head<'_>,
    help_text: Option<&str>,
    enabled: bool,
    control: impl FnOnce(&mut egui::Ui, f32, f32) -> R,
) -> R {
    let width = ui.available_width();
    let alpha = if enabled { 1.0 } else { 0.35 };
    let top = ui.cursor().min;
    let hh = if h.name.is_empty() {
        -9.0
    } else {
        head(ui, h, width, alpha)
    };
    let control_top = pos2(top.x, top.y + hh + 9.0);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(control_top, vec2(width, f32::INFINITY)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if !enabled {
        child.disable();
    }
    let r = control(&mut child, width, alpha);
    let mut bottom = child.min_rect().bottom();
    if let Some(s) = help_text {
        let mut at = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(
            pos2(top.x, bottom + 9.0),
            vec2(width, f32::INFINITY),
        )));
        let hh = help(&mut at, s, width, alpha);
        bottom += 9.0 + hh;
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + width, bottom)),
        egui::Sense::hover(),
    );
    r
}

/// A slider's range, step and how its value reads.
#[derive(Clone, Copy)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub format: fn(f64) -> String,
}

impl Range {
    #[must_use]
    pub const fn new(min: f64, max: f64, step: f64, format: fn(f64) -> String) -> Self {
        Self { min, max, step, format }
    }

    fn snap(&self, v: f64) -> f64 {
        let n = ((v - self.min) / self.step).round();
        let snapped = self.min + n * self.step;
        // Keep the step's decimals, so 0.1 + 0.2 reads as 0.3.
        let decimals = self.step.to_string().split('.').nth(1).map_or(0, str::len) as i32;
        let k = 10f64.powi(decimals);
        ((snapped * k).round() / k).clamp(self.min, self.max)
    }
}

/// `slider(...)`: the name and value over a chunky track with a key for a
/// thumb. Returns the new value while the player drags or clicks it.
pub fn slider(
    ui: &mut egui::Ui,
    name: &str,
    value: f64,
    range: Range,
    default: Option<f64>,
    help_text: Option<&str>,
    enabled: bool,
) -> Option<f64> {
    let is_default = default.is_some_and(|d| (d - value).abs() < 1e-9);
    let h = Head::new(name).value((range.format)(value), is_default);
    field(ui, &h, help_text, enabled, |ui, width, alpha| {
        slider_track(ui, name, value, range, width, alpha)
    })
}

/// The bare track, at `width` (`.fps-set-range`).
pub fn slider_track(ui: &mut egui::Ui, id: &str, value: f64, range: Range, width: f32, alpha: f32) -> Option<f64> {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), egui::Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let _ = id;
    let span = (range.max - range.min).max(1e-9);
    let thumb_w = 20.0;
    let usable = (rect.width() - thumb_w).max(1.0);
    let mut out = None;
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let t = f64::from(((p.x - rect.left() - thumb_w / 2.0) / usable).clamp(0.0, 1.0));
        let v = range.snap(range.min + t * span);
        if (v - value).abs() > 1e-12 {
            out = Some(v);
        }
    }
    if response.has_focus() {
        let (left, right) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if left || right {
            let v = range.snap(value + if right { range.step } else { -range.step });
            if (v - value).abs() > 1e-12 {
                out = Some(v);
            }
        }
    }
    let shown = out.unwrap_or(value);
    let t = (((shown - range.min) / span).clamp(0.0, 1.0)) as f32;
    let painter = ui.painter();
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - 5.0), vec2(rect.width(), 10.0));
    let split = rect.left() + thumb_w / 2.0 + usable * t;
    painter.rect_filled(track, 2.0, hsla(250.0, 30.0, 24.0, 0.9 * alpha));
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(split, track.max.y)),
        2.0,
        gold_text().gamma_multiply(alpha),
    );
    painter.hline(
        (track.left() + 1.0)..=(track.right() - 1.0),
        track.top() + 1.0,
        Stroke::new(2.0, hsla(0.0, 0.0, 0.0, 0.35 * alpha)),
    );
    let lit = response.hovered() || response.dragged() || response.has_focus();
    let lift = if lit { -1.0 } else { 0.0 };
    let thumb = Rect::from_min_size(
        pos2(split - thumb_w / 2.0, rect.center().y - 14.0 + lift),
        vec2(thumb_w, 28.0),
    );
    painter.rect_filled(
        thumb.translate(vec2(0.0, 6.0)).expand(1.0),
        3.0,
        hsla(0.0, 0.0, 0.0, 0.3 * alpha),
    );
    let (top, foot, base) = if lit {
        (hsl(278.0, 92.0, 78.0), hsl(268.0, 72.0, 54.0), hsl(258.0, 72.0, 22.0))
    } else {
        (hsl(51.0, 100.0, 80.0), hsl(44.0, 100.0, 55.0), hsl(30.0, 88.0, 28.0))
    };
    painter.rect_filled(thumb.translate(vec2(0.0, 4.0)), 2.0, base.gamma_multiply(alpha));
    gradient_polygon(
        painter,
        &[
            thumb.left_top(),
            thumb.right_top(),
            thumb.right_bottom(),
            thumb.left_bottom(),
        ],
        &[(0.0, top.gamma_multiply(alpha)), (1.0, foot.gamma_multiply(alpha))],
    );
    out
}

/// One key of a segmented row (`.fps-set-seg__btn`); `on` is held down.
fn seg_key(ui: &mut egui::Ui, rect: Rect, label: &str, on: bool, alpha: f32) -> bool {
    let response = ui.interact(
        rect.expand2(vec2(0.0, 2.0)),
        ui.scope_id().with(("seg", label, rect.min.x as i32, rect.min.y as i32)),
        egui::Sense::click(),
    );
    let hovered = response.hovered() && ui.is_enabled();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    let (face, stops, base_h, text_color) = if on {
        (
            rect.translate(vec2(0.0, 3.0)),
            [(0.0, hsl(51.0, 100.0, 73.0)), (1.0, hsl(44.0, 100.0, 55.0))],
            1.0,
            ink(),
        )
    } else if hovered {
        (
            rect.translate(vec2(0.0, -1.0)),
            [(0.0, hsl(278.0, 92.0, 72.0)), (1.0, hsl(268.0, 72.0, 52.0))],
            5.0,
            cream(),
        )
    } else {
        (
            rect,
            [(0.0, hsl(250.0, 24.0, 25.0)), (1.0, hsl(250.0, 30.0, 17.0))],
            4.0,
            hsla(45.0, 100.0, 95.0, 0.72),
        )
    };
    let base_color = if on {
        hsl(30.0, 88.0, 28.0)
    } else if hovered {
        hsl(258.0, 72.0, 20.0)
    } else {
        hsl(250.0, 40.0, 8.0)
    };
    painter.rect_filled(face.translate(vec2(0.0, base_h)), 2.0, base_color.gamma_multiply(alpha));
    let stops = stops.map(|(t, c)| (t, c.gamma_multiply(alpha)));
    gradient_polygon(
        painter,
        &[
            face.left_top(),
            face.right_top(),
            face.right_bottom(),
            face.left_bottom(),
        ],
        &stops,
    );
    let bevel = if on { 0.4 } else { 0.1 };
    painter.hline(
        (face.left() + 1.0)..=(face.right() - 1.0),
        face.top() + 0.75,
        Stroke::new(1.5, hsla(0.0, 0.0, 100.0, bevel * alpha)),
    );
    let g = line(
        painter,
        &label.to_uppercase(),
        13.0,
        display_family(),
        text_color.gamma_multiply(alpha),
        0.08,
    );
    painter.galley(face.center() - g.size() / 2.0, g, cream());
    response.clicked() && ui.is_enabled()
}

/// The keys of a segmented row laid out at `width`: each as wide as its
/// label and the rest shared out (`flex: 1 0 auto`), wrapping when they do
/// not fit. Returns the picked index.
pub fn seg_row(ui: &mut egui::Ui, labels: &[&str], current: Option<usize>, width: f32, alpha: f32) -> Option<usize> {
    let painter = ui.painter().clone();
    let gap = 8.0;
    let natural: Vec<f32> = labels
        .iter()
        .map(|l| {
            line(&painter, &l.to_uppercase(), 13.0, display_family(), cream(), 0.08)
                .size()
                .x
                + 24.0
        })
        .collect();
    // Rows, as flex-wrap fills them.
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0.0;
    for (i, w) in natural.iter().enumerate() {
        let last = rows.last_mut().expect("a row");
        let need = if last.is_empty() { *w } else { used + gap + w };
        if !last.is_empty() && need > width {
            rows.push(vec![i]);
            used = *w;
        } else {
            last.push(i);
            used = need;
        }
    }
    let mut picked = None;
    let top = ui.cursor().min;
    let mut y = top.y;
    for row in &rows {
        let sum: f32 = row.iter().map(|&i| natural[i]).sum::<f32>() + gap * (row.len() as f32 - 1.0);
        let extra = ((width - sum) / row.len() as f32).max(0.0);
        let mut x = top.x;
        for &i in row {
            let w = natural[i] + extra;
            let r = Rect::from_min_size(pos2(x, y), vec2(w, 40.0));
            if seg_key(ui, r, labels[i], current == Some(i), alpha) && current != Some(i) {
                picked = Some(i);
            }
            x += w + gap;
        }
        y += 40.0 + 4.0 + gap;
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + width, y - gap)),
        egui::Sense::hover(),
    );
    picked
}

/// `seg(...)`: a named row of keys, one held down.
pub fn seg(
    ui: &mut egui::Ui,
    h: Head<'_>,
    labels: &[&str],
    current: Option<usize>,
    help_text: Option<&str>,
    enabled: bool,
) -> Option<usize> {
    field(ui, &h, help_text, enabled, |ui, width, alpha| {
        seg_row(ui, labels, current, width, alpha)
    })
}

/// The ✓ on a picked swatch or menu entry, painted.
pub fn check_mark(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let s = size / 18.0;
    painter.add(Shape::line(
        vec![
            center + vec2(-6.0 * s, 0.0),
            center + vec2(-2.0 * s, 4.5 * s),
            center + vec2(6.5 * s, -5.0 * s),
        ],
        Stroke::new(2.4 * s, color),
    ));
}

/// `swatches(...)`: colour keys; `blocked` is one another field holds.
pub fn swatches(
    ui: &mut egui::Ui,
    name: &str,
    colors: &[Color32],
    current: Option<usize>,
    blocked: Option<usize>,
    help_text: Option<&str>,
    enabled: bool,
) -> Option<usize> {
    let h = Head::new(name);
    field(ui, &h, help_text, enabled, |ui, width, alpha| {
        let top = ui.cursor().min;
        let (size, gap) = (38.0, 9.0);
        let per_row = (((width + gap) / (size + gap)).floor() as usize).max(1);
        let mut picked = None;
        for (i, color) in colors.iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let r = Rect::from_min_size(
                pos2(
                    top.x + col as f32 * (size + gap),
                    top.y + row as f32 * (size + gap + 4.0),
                ),
                vec2(size, size),
            );
            let response = ui.interact(
                r,
                ui.scope_id().with(("sw", name, i, top.x as i32, top.y as i32)),
                egui::Sense::click(),
            );
            let is_blocked = blocked == Some(i) && current != Some(i);
            let on = current == Some(i);
            let hovered = response.hovered() && !is_blocked && ui.is_enabled();
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let a = alpha * if is_blocked { 0.22 } else { 1.0 };
            let face = if on {
                r.translate(vec2(0.0, 3.0))
            } else if hovered {
                r.translate(vec2(0.0, -2.0))
            } else {
                r
            };
            let painter = ui.painter();
            if !is_blocked {
                // `0 0 14px -2px var(--sw)`: a soft glow.
                for k in 1..=3 {
                    let k = k as f32;
                    painter.rect_filled(face.expand(k * 2.0), 3.0 + k * 2.0, color.gamma_multiply(0.07 * a));
                }
            }
            let base = Color32::from_rgb(
                (f32::from(color.r()) * 0.4 + 6.0) as u8,
                (f32::from(color.g()) * 0.4 + 5.0) as u8,
                (f32::from(color.b()) * 0.4 + 15.0) as u8,
            );
            if !is_blocked {
                painter.rect_filled(
                    face.translate(vec2(0.0, if on { 1.0 } else { 4.0 })),
                    3.0,
                    base.gamma_multiply(a),
                );
            }
            if on {
                painter.rect_filled(face.expand(3.0), 5.0, cream().gamma_multiply(alpha));
            }
            painter.rect_filled(face, 3.0, color.gamma_multiply(a));
            if on {
                check_mark(painter, face.center(), 18.0, ink());
            }
            if response.clicked() && !is_blocked && !on && ui.is_enabled() {
                picked = Some(i);
            }
        }
        let rows = colors.len().div_ceil(per_row);
        ui.allocate_rect(
            Rect::from_min_size(top, vec2(width, rows as f32 * (size + gap + 4.0) - gap)),
            egui::Sense::hover(),
        );
        picked
    })
}

/// `selectMenu(...)`: a field showing the current choice that opens its list
/// right under it. Returns the picked index.
pub fn select_menu(ui: &mut egui::Ui, id: &str, labels: &[&str], current: usize, width: f32) -> Option<usize> {
    let menu_id = ui.scope_id().with(("menu", id));
    let (rect, response) = ui.allocate_exact_size(vec2(width, 42.0), egui::Sense::click());
    let mut open: bool = ui.data(|d| d.get_temp(menu_id).unwrap_or(false));
    if response.clicked() {
        open = !open;
    }
    let hovered = response.hovered() && ui.is_enabled();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, Color32::from_rgb(0x25, 0x20, 0x39));
    let border = if hovered || open {
        violet()
    } else {
        Color32::from_rgb(0x63, 0x56, 0x75)
    };
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    let label = labels.get(current).copied().unwrap_or_default();
    let g = line(painter, label, 17.0, body(), cream(), 0.0);
    painter.galley(pos2(rect.left() + 11.0, rect.center().y - g.size().y / 2.0), g, cream());
    let c = pos2(rect.right() - 16.0, rect.center().y);
    let (a, b) = if open { (-1.0, 3.0) } else { (1.0, -3.0) };
    painter.add(Shape::line(
        vec![c + vec2(-4.5, b), c + vec2(0.0, b + 4.5 * a), c + vec2(4.5, b)],
        Stroke::new(2.0, cream().gamma_multiply(0.8)),
    ));
    let mut picked = None;
    if open {
        let area = egui::Area::new(menu_id.with("list"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(rect.left(), rect.bottom() + 6.0))
            .show(ui.ctx(), |ui| {
                let painter = ui.painter().clone();
                let row_h = 37.0;
                let list = Rect::from_min_size(ui.cursor().min, vec2(width, row_h * labels.len() as f32 + 10.0));
                painter.rect_filled(
                    list.translate(vec2(0.0, 12.0)).expand(6.0),
                    8.0,
                    hsla(250.0, 80.0, 2.0, 0.45),
                );
                gradient_polygon(
                    &painter,
                    &[
                        list.left_top(),
                        list.right_top(),
                        list.right_bottom(),
                        list.left_bottom(),
                    ],
                    &[
                        (0.0, hsla(250.0, 40.0, 13.0, 0.98)),
                        (1.0, hsla(250.0, 44.0, 8.0, 0.98)),
                    ],
                );
                painter.rect_stroke(
                    list,
                    2.0,
                    Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.55)),
                    egui::StrokeKind::Inside,
                );
                for (i, label) in labels.iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(list.left() + 5.0, list.top() + 5.0 + i as f32 * row_h),
                        vec2(width - 10.0, row_h),
                    );
                    let response = ui.interact(r, menu_id.with(("opt", i)), egui::Sense::click());
                    let color = if response.hovered() {
                        painter.rect_filled(r, 0.0, gold_text());
                        ink()
                    } else {
                        cream()
                    };
                    if i == current {
                        check_mark(&painter, pos2(r.left() + 13.0, r.center().y), 14.0, color);
                    }
                    let g = line(&painter, label, 17.0, body(), color, 0.0);
                    painter.galley(pos2(r.left() + 28.0, r.center().y - g.size().y / 2.0), g, color);
                    if response.clicked() {
                        picked = Some(i);
                    }
                }
                ui.allocate_rect(list, egui::Sense::hover());
            });
        let outside = ui.input(|i| i.pointer.any_pressed())
            && !response.hovered()
            && !area.response.hovered()
            && !area.response.contains_pointer();
        if picked.is_some() || outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(menu_id, open));
    picked.filter(|&i| i != current)
}

/// `.fps-set-notice`: a line about something that changed, with an Undo.
/// Returns true when Undo is pressed.
pub fn notice(ui: &mut egui::Ui, s: &str) -> bool {
    let width = ui.available_width();
    let painter = ui.painter().clone();
    let undo = line(&painter, "UNDO", 12.0, display_family(), ink(), 0.08);
    let text_w = width - 24.0 - 12.0 - undo.size().x - 24.0;
    let g = wrapped(&painter, s, FontId::new(14.0, body()), cream(), text_w);
    let h = g.size().y.max(26.0) + 20.0;
    let top = ui.cursor().min + vec2(0.0, 2.0);
    let r = Rect::from_min_size(top, vec2(width, h));
    painter.rect_filled(r, 3.0, hsla(276.0, 90.0, 64.0, 0.14));
    painter.rect_filled(Rect::from_min_size(r.min, vec2(3.0, h)), 0.0, hsl(276.0, 90.0, 72.0));
    painter.galley(pos2(r.left() + 12.0, r.center().y - g.size().y / 2.0), g, cream());
    let key = Rect::from_min_size(
        pos2(r.right() - 12.0 - undo.size().x - 24.0, r.center().y - 13.0),
        vec2(undo.size().x + 24.0, 26.0),
    );
    let response = ui.interact(key, ui.scope_id().with(("undo", s)), egui::Sense::click());
    let fill = if response.hovered() {
        hsl(46.0, 100.0, 68.0)
    } else {
        gold_text()
    };
    painter.rect_filled(key, 3.0, fill);
    painter.galley(key.center() - undo.size() / 2.0, undo, ink());
    ui.allocate_rect(
        Rect::from_min_max(r.min - vec2(0.0, 2.0), r.max + vec2(0.0, 6.0)),
        egui::Sense::hover(),
    );
    response.clicked()
}

/// Columns side by side at `fractions` of the width with `gap` between,
/// each drawn by `add(ui, index)`; the row is as tall as the tallest.
pub fn columns(ui: &mut egui::Ui, fractions: &[f32], gap: f32, mut add: impl FnMut(&mut egui::Ui, usize)) {
    let width = ui.available_width();
    let total: f32 = fractions.iter().sum();
    let free = width - gap * (fractions.len() as f32 - 1.0);
    let top = ui.cursor().min;
    let mut x = top.x;
    let mut bottom = top.y;
    for (i, f) in fractions.iter().enumerate() {
        let w = free * f / total;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(pos2(x, top.y), vec2(w, f32::INFINITY)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_width(w);
        add(&mut child, i);
        bottom = bottom.max(child.min_rect().bottom());
        x += w + gap;
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + width, bottom)),
        egui::Sense::hover(),
    );
}

/// A grid of fields, `per_row` across (`.fps-set-field-grid`: 18px down,
/// 26px across).
pub fn field_grid(ui: &mut egui::Ui, count: usize, per_row: usize, mut add: impl FnMut(&mut egui::Ui, usize)) {
    let mut i = 0;
    while i < count {
        let n = per_row.min(count - i);
        let fractions = vec![1.0; per_row];
        columns(ui, &fractions, 26.0, |ui, k| {
            if k < n {
                add(ui, i + k);
            }
        });
        i += n;
        if i < count {
            ui.add_space(18.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slider_snaps_to_its_step() {
        let r = Range::new(0.1, 1.0, 0.05, format_percent);
        assert!((r.snap(0.52) - 0.5).abs() < 1e-12);
        assert!((r.snap(2.0) - 1.0).abs() < 1e-12);
        assert_eq!(format_percent(0.5), "50%");
        assert_eq!(plain(1.7000000001), "1.7");
    }
}
