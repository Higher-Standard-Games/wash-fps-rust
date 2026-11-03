//! The region picker before the data sync: washoi.net's `/play` screen
//! (`src/components/play/RegionLauncher.tsx`, `RegionMap.tsx` and
//! `region-map.css`) drawn with egui.
//!
//! The world as a dot matrix, a pin and a card per region with its address,
//! whether it is live and its ping, the nearest one preselected, and a Launch
//! key. A region the player picks is remembered and wins next time, as the
//! site's `washoi.region` does. The list itself is read from the site
//! ([`washoi::regions`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};
use washoi::client::hud::washiki;
use washoi::client::lobby_panel::{display_family, hsl, install_style};
use washoi::regions::{self, Region, world_dots};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use egui::{Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};

use crate::window::{OpenWindow, next_frame, open_window, present, run_on_event_loop};

const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.035,
    g: 0.03,
    b: 0.07,
    a: 1.0,
};
/// Dot size inside its cell, square (`DOT`).
const DOT: f32 = 0.52;
/// Radius in cells of the lit land around a pin (`GLOW_RADIUS`).
const GLOW_RADIUS: f32 = 11.0;
/// `region-map-ping`: a ring grows from 0.6 to 4.2 times over 2.4 s.
const RING_PERIOD: f32 = 2.4;

/// `PingState`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ping {
    Measuring,
    Ms(f64),
    NoReply,
}

/// `pingText`.
fn ping_text(ping: Option<Ping>) -> String {
    match ping {
        Some(Ping::Ms(ms)) => format!("{ms} ms"),
        Some(Ping::Measuring) => "…".into(),
        Some(Ping::NoReply) => "No reply".into(),
        None => "— ms".into(),
    }
}

fn rgba(c: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3]))
}

fn hsla(h: f32, s: f32, l: f32, a: f32) -> Color32 {
    rgba(washiki::hsla(h, s, l, a))
}

fn cyan() -> Color32 {
    hsl(185.0, 98.0, 62.0)
}

fn gold() -> Color32 {
    hsl(42.0, 96.0, 60.0)
}

/// The picker's state, shared by the window and the screenshot.
struct Picker {
    config_dir: Option<PathBuf>,
    regions: Arc<Mutex<Vec<Region>>>,
    /// By lobby address, so a list read later does not ping again.
    pings: Arc<Mutex<HashMap<String, Ping>>>,
    /// The region picked here or on an earlier start.
    picked: Option<String>,
    started: Instant,
    cancel: Arc<AtomicBool>,
}

/// Measures each live region of `list` not measured yet, a thread each.
fn ping_all(list: &[Region], pings: &Arc<Mutex<HashMap<String, Ping>>>) {
    for url in list.iter().filter_map(|r| r.lobby_url.clone()) {
        {
            let mut known = pings.lock().expect("pings");
            if known.contains_key(&url) {
                continue;
            }
            known.insert(url.clone(), Ping::Measuring);
        }
        let pings = pings.clone();
        std::thread::spawn(move || {
            let ping = regions::ping(&url).map_or(Ping::NoReply, Ping::Ms);
            pings.lock().expect("pings").insert(url, ping);
        });
    }
}

impl Picker {
    /// Shows the list read last time at once, then the site's when it answers.
    fn start(config_dir: Option<PathBuf>, offline: bool) -> Self {
        let first = regions::cached(config_dir.as_deref()).unwrap_or_else(regions::builtin);
        let picked = regions::saved_choice(config_dir.as_deref());
        let picker = Self {
            regions: Arc::new(Mutex::new(first.clone())),
            pings: Arc::new(Mutex::new(HashMap::new())),
            picked,
            started: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            config_dir,
        };
        if !offline {
            ping_all(&first, &picker.pings);
            let (list, pings, cancel, dir) = (
                picker.regions.clone(),
                picker.pings.clone(),
                picker.cancel.clone(),
                picker.config_dir.clone(),
            );
            std::thread::spawn(move || {
                let read = regions::load(dir.as_deref(), &cancel);
                ping_all(&read, &pings);
                *list.lock().expect("regions") = read;
            });
        }
        picker
    }

    fn ping_of(&self, region: &Region) -> Option<Ping> {
        let url = region.lobby_url.as_ref()?;
        self.pings.lock().expect("pings").get(url).copied()
    }

    /// `recommended` and `selected`.
    fn recommended_and_selected(&self, list: &[Region]) -> (Option<String>, Option<String>) {
        let recommended = regions::nearest(list, |id| {
            let r = list.iter().find(|r| r.id == id)?;
            match self.ping_of(r)? {
                Ping::Ms(ms) => Some(ms),
                _ => None,
            }
        })
        .map(|r| r.id.clone());
        let selected = self
            .picked
            .clone()
            .filter(|id| list.iter().any(|r| &r.id == id))
            .or_else(|| recommended.clone())
            .or_else(|| list.first().map(|r| r.id.clone()));
        (recommended, selected)
    }

    /// `select`: remembered at once, as the site stores it.
    fn select(&mut self, id: &str) {
        self.picked = Some(id.to_owned());
        regions::save_choice(self.config_dir.as_deref(), id);
    }

    /// Draws the screen; the region to launch when the player launches one.
    fn ui(&mut self, root: &mut egui::Ui) -> Option<Region> {
        let ctx = root.ctx().clone();
        let list = self.regions.lock().expect("regions").clone();
        let (recommended, selected) = self.recommended_and_selected(&list);
        let time = self.started.elapsed().as_secs_f32();
        let mut launch = false;
        let mut pick = None;
        // Keys: arrows move between regions, Enter launches.
        let (step, enter) = ctx.input(|i| {
            let step = i32::from(i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown))
                - i32::from(i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp));
            (step, i.key_pressed(egui::Key::Enter))
        });
        if step != 0 && !list.is_empty() {
            let at = list.iter().position(|r| Some(&r.id) == selected.as_ref()).unwrap_or(0);
            let next = (at as i32 + step).rem_euclid(list.len() as i32) as usize;
            pick = Some(list[next].id.clone());
        }
        launch |= enter;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(hsl(252.0, 32.0, 6.0)))
            .show(root, |ui| {
                let full = ui.max_rect().shrink2(vec2(24.0, 20.0));
                // Launch and the line under it take the bottom.
                let bottom = 118.0;
                let slab = Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - bottom));
                self.slab(
                    ui,
                    slab,
                    &list,
                    selected.as_deref(),
                    recommended.as_deref(),
                    time,
                    &mut pick,
                );
                let selected_region = list.iter().find(|r| Some(&r.id) == selected.as_ref());
                let under = Rect::from_min_max(pos2(full.min.x, slab.max.y + 24.0), full.max);
                launch |= launch_key(ui, under, selected_region, list.iter().any(Region::live));
            });
        if let Some(id) = pick {
            self.select(&id);
        }
        // Keep the rings and the link moving.
        ctx.request_repaint();
        let (_, selected) = self.recommended_and_selected(&list);
        let chosen = list.into_iter().find(|r| Some(&r.id) == selected.as_ref())?;
        (launch && chosen.live()).then(|| {
            self.select(&chosen.id);
            chosen
        })
    }

    /// The `Slab`: its title bar, then the map.
    #[allow(clippy::too_many_arguments)]
    fn slab(
        &self,
        ui: &mut egui::Ui,
        slab: Rect,
        list: &[Region],
        selected: Option<&str>,
        recommended: Option<&str>,
        time: f32,
        pick: &mut Option<String>,
    ) {
        let painter = ui.painter().clone();
        let hairline = hsla(180.0, 100.0, 50.0, 0.28);
        // `variant="drop"`: a hard shadow down and right.
        painter.rect_filled(slab.translate(vec2(6.0, 8.0)), 0.0, hsla(250.0, 45.0, 4.0, 0.6));
        painter.rect_filled(slab, 0.0, hsl(252.0, 32.0, 9.0));
        painter.rect_stroke(slab, 0.0, Stroke::new(1.0, hairline), egui::StrokeKind::Inside);
        let bar = Rect::from_min_size(slab.min, vec2(slab.width(), 40.0));
        painter.hline(bar.x_range(), bar.max.y, Stroke::new(1.0, hairline));
        let dim = rgba(washiki::cream_dim());
        let caption = |parts: &[(&str, Color32)]| {
            let mut job = egui::text::LayoutJob::default();
            for (text, color) in parts {
                job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(10.5, display_family()),
                        color: *color,
                        extra_letter_spacing: 2.5,
                        ..Default::default()
                    },
                );
            }
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let left = caption(&[("// ", cyan()), ("SELECT REGION", dim)]);
        painter.galley(pos2(bar.min.x + 24.0, bar.center().y - left.size().y / 2.0), left, dim);
        let right = caption(&[
            ("CLICK A SERVER ", dim),
            ("·", gold()),
            (" NEAREST IS PRESELECTED", dim),
        ]);
        let at = pos2(bar.max.x - 24.0 - right.size().x, bar.center().y - right.size().y / 2.0);
        painter.galley(at, right, dim);

        // `px-6 pb-6 pt-14`, the map as wide as fits.
        let room = Rect::from_min_max(
            pos2(slab.min.x + 24.0, bar.max.y + 56.0),
            pos2(slab.max.x - 24.0, slab.max.y - 24.0),
        );
        let (cols, rows) = world_dots::size();
        let cell = (room.width() / cols as f32).min(room.height() / rows as f32);
        let size = vec2(cols as f32, rows as f32) * cell;
        let map = Rect::from_center_size(pos2(room.center().x, room.min.y + size.y / 2.0), size);
        self.map(ui, map, cell, list, selected, recommended, time, pick);
    }

    /// `RegionMap`.
    #[allow(clippy::too_many_arguments)]
    fn map(
        &self,
        ui: &mut egui::Ui,
        map: Rect,
        cell: f32,
        list: &[Region],
        selected: Option<&str>,
        recommended: Option<&str>,
        time: f32,
        pick: &mut Option<String>,
    ) {
        let painter = ui.painter_at(ui.max_rect());
        let at = |x: f32, y: f32| map.min + vec2(x, y) * cell;
        let pins: Vec<(&Region, Pos2, (f32, f32))> = list
            .iter()
            .map(|r| {
                let (x, y) = world_dots::to_grid(r.lon, r.lat);
                (r, at(x as f32, y as f32), (x as f32, y as f32))
            })
            .collect();

        // The land, and the lit patch around each pin in three bands.
        let mut land = egui::Mesh::default();
        let mut glow = egui::Mesh::default();
        let land_color = hsla(185.0, 70.0, 60.0, 0.3);
        for (y, row) in world_dots::rows().enumerate() {
            for (x, c) in row.iter().enumerate() {
                if *c != b'1' {
                    continue;
                }
                let (fx, fy) = (x as f32, y as f32);
                let dot = Rect::from_min_size(
                    at(fx + (1.0 - DOT) / 2.0, fy + (1.0 - DOT) / 2.0),
                    Vec2::splat(DOT * cell),
                );
                land.add_colored_rect(dot, land_color);
                for (r, _, (px, py)) in &pins {
                    let dist = (fx + 0.5 - px).hypot(fy + 0.5 - py);
                    if dist > GLOW_RADIUS {
                        continue;
                    }
                    let band = ((dist / GLOW_RADIUS) * 3.0).floor().min(2.0) as usize;
                    let on = Some(r.id.as_str()) == selected;
                    let opacity = [1.0, 0.6, 0.28][band] * if on { 1.0 } else { 0.55 };
                    let color = if on { hsl(42.0, 96.0, 58.0) } else { cyan() };
                    glow.add_colored_rect(dot, color.gamma_multiply(opacity));
                }
            }
        }
        painter.add(land);
        painter.add(glow);

        // The link: a shallow arc, its dashes flowing (`region-map-flow`).
        if let [(_, a, (ax, ay)), (_, b, (bx, by)), ..] = pins[..] {
            let control = at((ax + bx) / 2.0, ay.min(by) - (ax - bx).abs() * 0.18);
            let points: Vec<Pos2> = (0..=64)
                .map(|i| {
                    let t = i as f32 / 64.0;
                    let u = 1.0 - t;
                    pos2(
                        u * u * a.x + 2.0 * u * t * control.x + t * t * b.x,
                        u * u * a.y + 2.0 * u * t * control.y + t * t * b.y,
                    )
                })
                .collect();
            let offset = (time / 1.6).fract() * 2.2 * cell;
            painter.extend(egui::Shape::dashed_line_with_offset(
                &points,
                Stroke::new(0.35 * cell, hsla(185.0, 98.0, 62.0, 0.45)),
                &[1.2 * cell],
                &[1.0 * cell],
                offset,
            ));
        }

        // The pins: two rings a beat apart, and the core.
        for (r, pin, _) in &pins {
            let on = Some(r.id.as_str()) == selected;
            let color = if on { gold() } else { cyan() };
            for delay in [0.0, RING_PERIOD / 2.0] {
                let t = ((time - delay).max(0.0) / RING_PERIOD).fract();
                if time < delay {
                    continue;
                }
                // `ease-out`, near enough.
                let eased = 1.0 - (1.0 - t).powi(2);
                let scale = 0.6 + (4.2 - 0.6) * eased;
                let alpha = 0.9 * (1.0 - eased);
                painter.circle_stroke(
                    *pin,
                    1.6 * scale * cell,
                    Stroke::new(0.3 * cell, color.gamma_multiply(alpha)),
                );
            }
            painter.circle(*pin, 0.9 * cell, color, Stroke::new(0.35 * cell, hsl(250.0, 45.0, 6.0)));
        }

        // The labels, which are the controls.
        for (r, pin, _) in &pins {
            let on = Some(r.id.as_str()) == selected;
            let nearest = Some(r.id.as_str()) == recommended;
            if card(ui, r, *pin, on, nearest, self.ping_of(r)) {
                *pick = Some(r.id.clone());
            }
        }
    }
}

/// A convex polygon filled top to bottom from `top` to `bottom`.
fn gradient_polygon(painter: &egui::Painter, points: &[Pos2], stops: &[(f32, Color32)]) {
    let (y0, y1) = points
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    let color_at = |y: f32| {
        let t = ((y - y0) / (y1 - y0).max(1.0)).clamp(0.0, 1.0);
        let i = stops.iter().rposition(|(at, _)| *at <= t).unwrap_or(0);
        let (a, ca) = stops[i];
        let Some(&(b, cb)) = stops.get(i + 1) else { return ca };
        ca.lerp_to_gamma(cb, (t - a) / (b - a))
    };
    // Slice into bands at each stop so the middle stop shows.
    let mut mesh = egui::Mesh::default();
    let cuts: Vec<f32> = stops.iter().map(|(at, _)| y0 + at * (y1 - y0)).collect();
    for pair in cuts.windows(2) {
        let (top, bottom) = (pair[0], pair[1]);
        let clipped = clip_band(points, top, bottom);
        if clipped.len() < 3 {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for p in &clipped {
            mesh.colored_vertex(*p, color_at(p.y));
        }
        for i in 1..clipped.len() as u32 - 1 {
            mesh.add_triangle(base, base + i, base + i + 1);
        }
    }
    painter.add(mesh);
}

/// The part of a convex polygon between two heights.
fn clip_band(points: &[Pos2], top: f32, bottom: f32) -> Vec<Pos2> {
    let clip = |poly: &[Pos2], keep: &dyn Fn(Pos2) -> bool, edge: f32| {
        let mut out = Vec::new();
        for (i, &p) in poly.iter().enumerate() {
            let q = poly[(i + 1) % poly.len()];
            if keep(p) {
                out.push(p);
            }
            if keep(p) != keep(q) {
                let t = (edge - p.y) / (q.y - p.y);
                out.push(p + (q - p) * t);
            }
        }
        out
    };
    let below_top = clip(points, &|p| p.y >= top, top);
    clip(&below_top, &|p| p.y <= bottom, bottom)
}

fn text(
    ui: &egui::Ui,
    s: &str,
    size: f32,
    family: egui::FontFamily,
    color: Color32,
    spacing: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        s,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(size, family),
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    ui.fonts_mut(|f| f.layout_job(job))
}

/// `region-map__label`: the code block and the details beside it, up and to
/// the right of the pin (or below it); true when clicked.
fn card(ui: &mut egui::Ui, r: &Region, pin: Pos2, on: bool, nearest: bool, ping: Option<Ping>) -> bool {
    let cream = rgba(washiki::cream());
    let dim = rgba(washiki::cream_dim());
    let display = display_family;
    let body = || egui::FontFamily::Proportional;
    let code = text(ui, &r.code, 26.0, display(), hsl(250.0, 45.0, 8.0), -0.5);
    let name = text(ui, &r.name.to_uppercase(), 13.0, display(), cream, 1.0);
    let host = text(ui, &r.host, 14.0, body(), hsl(185.0, 98.0, 68.0), 0.5);
    let live = r.live();
    let mut stats = vec![if live {
        text(ui, "ONLINE", 10.0, display(), hsl(150.0, 80.0, 60.0), 1.6)
    } else {
        text(ui, "NOT LIVE", 10.0, display(), hsl(320.0, 90.0, 70.0), 1.6)
    }];
    stats.push(text(ui, &ping_text(ping).to_uppercase(), 10.0, display(), dim, 1.6));
    if nearest {
        stats.push(text(ui, "NEAREST", 10.0, display(), gold(), 1.6));
    }
    let stats_width: f32 = stats.iter().map(|g| g.size().x).sum::<f32>() + 10.0 * (stats.len() - 1) as f32;
    let meta_inner = vec2(
        name.size().x.max(host.size().x).max(stats_width),
        name.size().y + host.size().y + stats[0].size().y + 4.0,
    );
    let meta_size = meta_inner + vec2(26.0, 13.0);
    let code_size = vec2(code.size().x + 26.0, meta_size.y);
    let size = vec2(code_size.x + meta_size.x, meta_size.y);
    let id = ui.scope_id().with(("region", &r.id));
    let place = |lift: f32| {
        let offset = if r.label_below {
            vec2(10.0, 10.0 - lift)
        } else {
            vec2(10.0, -size.y - 8.0 - lift)
        };
        Rect::from_min_size(pin + offset, size)
    };
    let response = ui.interact(place(0.0), id, egui::Sense::click());
    let rect = place(if response.hovered() { 2.0 } else { 0.0 });
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter_at(ui.max_rect());
    // `drop-shadow(3px 4px 0 …)`.
    let shadow = hsla(250.0, 45.0, 4.0, 0.6);
    let code_rect = Rect::from_min_size(rect.min, code_size);
    let meta_rect = Rect::from_min_size(pos2(code_rect.max.x, rect.min.y), meta_size);
    let code_poly = |r: Rect| {
        vec![
            pos2(r.min.x + r.width() * 0.1, r.min.y),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
        ]
    };
    let meta_poly = |r: Rect| {
        vec![
            r.left_top(),
            r.right_top(),
            pos2(r.min.x + r.width() * 0.94, r.max.y),
            r.left_bottom(),
        ]
    };
    for poly in [code_poly(code_rect), meta_poly(meta_rect)] {
        painter.add(egui::Shape::convex_polygon(
            poly.iter().map(|p| *p + vec2(3.0, 4.0)).collect(),
            shadow,
            Stroke::NONE,
        ));
    }
    let code_stops: Vec<(f32, Color32)> = if on {
        vec![
            (0.0, hsl(51.0, 100.0, 73.0)),
            (0.55, hsl(46.0, 100.0, 58.0)),
            (1.0, hsl(42.0, 100.0, 50.0)),
        ]
    } else {
        vec![(0.0, hsl(185.0, 98.0, 72.0)), (1.0, hsl(185.0, 90.0, 50.0))]
    };
    gradient_polygon(&painter, &code_poly(code_rect), &code_stops);
    let meta = meta_poly(meta_rect);
    painter.add(egui::Shape::convex_polygon(
        meta.clone(),
        hsla(252.0, 29.0, 9.0, 0.94),
        Stroke::NONE,
    ));
    let border = if on {
        hsla(42.0, 96.0, 60.0, 0.7)
    } else {
        hsla(180.0, 100.0, 50.0, 0.28)
    };
    // Every side but the left, which meets the code block.
    painter.line(vec![meta[0], meta[1], meta[2], meta[3]], Stroke::new(1.0, border));
    painter.galley(
        pos2(code_rect.min.x + 14.0, code_rect.center().y - code.size().y / 2.0),
        code,
        cream,
    );
    let mut y = meta_rect.min.y + 6.0;
    let x = meta_rect.min.x + 10.0;
    for line in [name, host] {
        let h = line.size().y;
        painter.galley(pos2(x, y), line, cream);
        y += h + 2.0;
    }
    let mut sx = x;
    for stat in stats {
        let w = stat.size().x;
        painter.galley(pos2(sx, y), stat, cream);
        sx += w + 10.0;
    }
    response.clicked()
}

/// The `Key variant="cta"` and the line under it; true when pressed.
fn launch_key(ui: &mut egui::Ui, under: Rect, selected: Option<&Region>, any_live: bool) -> bool {
    let Some(r) = selected else { return false };
    let width = under.width().min(576.0);
    let key = Rect::from_min_size(pos2(under.center().x - width / 2.0, under.min.y), vec2(width, 56.0));
    let response = ui.interact(key, ui.scope_id().with("launch"), egui::Sense::click());
    let painter = ui.painter_at(ui.max_rect());
    let lift = if response.hovered() && r.live() { 2.0 } else { 0.0 };
    let face = key.translate(vec2(0.0, -lift));
    painter.rect_filled(key.translate(vec2(4.0, 5.0)), 0.0, hsla(250.0, 45.0, 4.0, 0.7));
    let stops = [
        (0.0, hsl(51.0, 100.0, 73.0)),
        (0.55, hsl(46.0, 100.0, 58.0)),
        (1.0, hsl(42.0, 100.0, 50.0)),
    ];
    let stops: Vec<(f32, Color32)> = if r.live() {
        stops.to_vec()
    } else {
        // `<Key>` desaturates when disabled.
        stops
            .iter()
            .map(|(at, c)| {
                let grey = ((u16::from(c.r()) + u16::from(c.g()) + u16::from(c.b())) / 3) as u8;
                (*at, Color32::from_gray(grey).gamma_multiply(0.6))
            })
            .collect()
    };
    gradient_polygon(
        &painter,
        &[
            face.left_top(),
            face.right_top(),
            face.right_bottom(),
            face.left_bottom(),
        ],
        &stops,
    );
    if response.hovered() && r.live() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let label = text(
        ui,
        &format!("LAUNCH · {}", r.code),
        20.0,
        display_family(),
        rgba(washiki::ink()),
        2.0,
    );
    painter.galley(face.center() - label.size() / 2.0, label, Color32::BLACK);
    let cream = rgba(washiki::cream());
    let dim = rgba(washiki::cream_dim());
    let status = if !any_live {
        "no servers are up yet."
    } else if r.live() {
        "ready to connect."
    } else {
        "not live yet. Pick another region to play now."
    };
    let mut job = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat {
        font_id: egui::FontId::proportional(18.0),
        color,
        ..Default::default()
    };
    job.append(&r.name, 0.0, format(cream));
    job.append(&r.host, 6.0, format(cyan()));
    job.append(&format!(" - {status}"), 0.0, format(dim));
    let line = ui.fonts_mut(|f| f.layout_job(job));
    painter.galley(
        pos2(under.center().x - line.size().x / 2.0, key.max.y + 14.0),
        line,
        cream,
    );
    response.clicked() && r.live()
}

struct PickerWindow {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layer: EguiLayer,
}

struct PickerApp {
    picker: Picker,
    open: Option<PickerWindow>,
    chosen: Option<Region>,
    quit: bool,
    error: Option<String>,
}

impl PickerApp {
    fn redraw(&mut self) {
        let Some(m) = &mut self.open else { return };
        let frame = match next_frame(&m.gpu, &m.window, &mut m.surface, &m.config) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(why) => {
                self.error = Some(why);
                return;
            }
        };
        let view = frame
            .texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let picker = &mut self.picker;
        let mut chosen = None;
        let ui = m.layer.run(&m.window, |ui| chosen = picker.ui(ui));
        let mut encoder = m.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("region picker"),
        });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("region picker clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(BACKGROUND),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        m.layer
            .paint(&m.gpu, &mut encoder, &view, [m.config.width, m.config.height], ui);
        m.gpu.queue.submit([encoder.finish()]);
        present(&m.gpu, &m.window, &mut m.surface, &m.config, frame);
        if chosen.is_some() {
            self.chosen = chosen;
        }
    }
}

impl ApplicationHandler for PickerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() {
            return;
        }
        match open_window(event_loop) {
            Ok(OpenWindow {
                window,
                gpu,
                surface,
                config,
            }) => {
                window.set_title("Washoi");
                let layer = EguiLayer::new(&gpu, &window, config.format);
                install_style(&layer.ctx);
                self.open = Some(PickerWindow {
                    window,
                    gpu,
                    surface,
                    config,
                    layer,
                });
            }
            Err(why) => {
                self.error = Some(why);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(m) = &mut self.open {
            let _ = m.layer.on_window_event(&m.window, &event);
        }
        let escape = matches!(&event, WindowEvent::KeyboardInput { event: key, .. }
            if key.state == ElementState::Pressed && key.physical_key == PhysicalKey::Code(KeyCode::Escape));
        match event {
            WindowEvent::CloseRequested => {
                self.quit = true;
                event_loop.exit();
            }
            _ if escape => {
                self.quit = true;
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(m) = &mut self.open {
                    m.config.width = size.width.max(1);
                    m.config.height = size.height.max(1);
                    m.surface.configure(&m.gpu.device, &m.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.chosen.is_some() || self.error.is_some() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(m) = &self.open {
            m.window.request_redraw();
        }
    }
}

/// The screen until the player launches a region (`Some`) or closes the
/// window (`None`). `offline` shows the list kept from last time, unpinged.
///
/// # Errors
/// No window could open.
pub fn run(config_dir: Option<&Path>, offline: bool) -> Result<Option<Region>, String> {
    let mut app = PickerApp {
        picker: Picker::start(config_dir.map(Path::to_path_buf), offline),
        open: None,
        chosen: None,
        quit: false,
        error: None,
    };
    let ran = run_on_event_loop(&mut app);
    app.picker.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(m) = &app.open {
        crate::window::remember_placement(&m.window);
    }
    app.open = None;
    ran?;
    if let Some(why) = app.error {
        return Err(why);
    }
    Ok(if app.quit { None } else { app.chosen })
}

/// The screen after `seconds`, saved to `path` without a window: for
/// checking its look.
///
/// # Errors
/// No GPU, or the image could not be written.
pub fn screenshot(config_dir: Option<&Path>, path: &Path, size: (u32, u32), seconds: f64) -> Result<(), String> {
    let (width, height) = size;
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
    install_style(&layer.ctx);
    let mut picker = Picker::start(config_dir.map(Path::to_path_buf), false);
    let input = || egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32))),
        ..Default::default()
    };
    let started = Instant::now();
    let mut pixels = Vec::new();
    while started.elapsed() < Duration::from_secs_f64(seconds) || pixels.is_empty() {
        let frame = layer.run_input(input(), |ui| {
            picker.ui(ui);
        });
        pixels = render_encoded_to_rgba(&gpu, width, height, BACKGROUND, |encoder, view| {
            layer.paint(&gpu, encoder, view, [width, height], frame);
        });
        std::thread::sleep(Duration::from_millis(33));
    }
    picker.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    for r in picker.regions.lock().expect("regions").iter() {
        println!("region: {} {} {}", r.id, r.host, ping_text(picker.ping_of(r)));
    }
    image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    println!("region: saved {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_band_of_a_square_is_the_rows_between() {
        let square = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)];
        let band = clip_band(&square, 2.0, 5.0);
        let ys: Vec<f32> = band.iter().map(|p| p.y).collect();
        assert!(ys.iter().all(|y| (2.0..=5.0).contains(y)));
        assert_eq!(band.len(), 4);
    }

    #[test]
    fn a_pick_wins_over_the_nearest() {
        let dir = std::env::temp_dir().join(format!("washoi-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut picker = Picker::start(Some(dir.clone()), true);
        let list = regions::builtin();
        let us = list[1].lobby_url.clone().unwrap();
        picker.pings.lock().unwrap().insert(us, Ping::Ms(20.0));
        assert_eq!(
            picker.recommended_and_selected(&list),
            (Some("us".into()), Some("us".into()))
        );
        picker.select("eu");
        assert_eq!(picker.recommended_and_selected(&list).1.as_deref(), Some("eu"));
        // And it is remembered for the next start.
        assert_eq!(Picker::start(Some(dir.clone()), true).picked.as_deref(), Some("eu"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
