//! A whole demo's playback controls, after wash-fps `client/DemoViewerHUD.ts`
//! in its demo mode: a timeline with the demo's frags and deaths marked
//! (coloured from the followed player's side: their frags, their deaths,
//! everyone else's), play or pause, the time, the speed, the status and
//! whose eyes the view is behind. While playback runs untouched the bar
//! fades away; a key, the mouse or a pause brings it back. A click on the
//! timeline seeks there; on a mark, to a little before it.
//!
//! The TypeScript builds it from DOM elements with the HUD's raised keys;
//! here it is one egui area in the same colours, and the gold exit plate
//! (`.dvh-exit`) in the top left corner, which fades with it. Minimizing
//! the bar is left out.
//!
//! The demo's two ends, as `DemoService.presentDemoEnds` shows them: the
//! players' names over its first seconds of play, and at its last frame a
//! fade to black under an end card (`demoEndCard`) with Replay and Exit.

// The bar's widgets sense clicks but never take keyboard focus
// (`Sense::CLICK`, not `Sense::click()`): a focused button would also take
// Space, toggling the pause back that the demo keys just toggled.
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, vec2};
use shagger::ui::egui_layer::egui;
use washoi::client::demo_playback::{DemoDeath, DemoPlayback};
use washoi::client::reel_cards::{DemoFinal, EndCard, ReelTitle, demo_end_card, reel_title};

/// `IDLE_AFTER_MS`: how long the controls stay up after the last touch.
const IDLE_AFTER: f64 = 2.5;
/// How long they take to fade.
const FADE: f64 = 0.4;
/// `TIMELINE_LEAD_SECONDS`: a mark seeks this far before its frag.
const TIMELINE_LEAD_SECONDS: f64 = 2.0;
const GOLD: Color32 = Color32::from_rgb(0xf5, 0xc5, 0x18);
const CREAM: Color32 = Color32::from_rgb(0xf6, 0xec, 0xd4);
const INK: Color32 = Color32::from_rgba_premultiplied(22, 19, 34, 242);
const EDGE: Color32 = Color32::from_rgba_premultiplied(15, 34, 38, 41);
const RED: Color32 = Color32::from_rgb(0xe5, 0x48, 0x3c);
/// `DEMO_TITLE_MS`: how long the names stay up over the first seconds of play.
const DEMO_TITLE_SECONDS: f64 = 3.0;
/// `DEMO_END_FADE_MS`: how long a finished demo takes to fade to black.
const DEMO_END_FADE_SECONDS: f64 = 1.0;

/// What a click on the bar asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DemoOverlayAction {
    TogglePause,
    /// Seek to this point of the timeline (0 to 1).
    Seek(f64),
    Speed(i8),
    /// The end card's Replay: the demo again from its first frame.
    Replay,
    /// The exit plate or the end card's Exit: leave playback, back to the
    /// lobby (Escape).
    Exit,
}

/// `DemoTimelineEvent`'s kind, from the followed player's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    Frag,
    Death,
    Other,
}

/// `paintTimelineEvents`' colouring of one death.
#[must_use]
pub fn mark_kind(death: &DemoDeath, followed: Option<&str>) -> MarkKind {
    match followed {
        Some(id) if death.killer_id.as_deref() == Some(id) => MarkKind::Frag,
        Some(id) if death.victim_id == id => MarkKind::Death,
        _ => MarkKind::Other,
    }
}

/// `formatTime`: m:ss.
#[must_use]
pub fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// The bar's state between frames.
pub struct DemoOverlay {
    deaths: Vec<DemoDeath>,
    /// Seconds since the last touch.
    idle: f64,
    /// The players' names, from the header.
    title: ReelTitle,
    /// Seconds of wall clock since playback opened.
    clock: f64,
    /// When the demo reached its end, on `clock`, with its card; none while it plays.
    ended: Option<(f64, EndCard)>,
}

impl DemoOverlay {
    /// `scanTimelineEvents`: the demo's deaths, read once.
    #[must_use]
    pub fn new(demo: &DemoPlayback) -> Self {
        Self {
            deaths: demo.scan_deaths(),
            idle: 0.0,
            title: reel_title(&demo.header.players),
            clock: 0.0,
            ended: None,
        }
    }

    /// `presentDemoEnds`' bookkeeping: at the demo's last frame its card is
    /// made from `final_` (the scores as they stand, the frags counted
    /// here), and it goes again as soon as playback moves on.
    pub fn present_ends(&mut self, demo: &DemoPlayback, final_: impl FnOnce() -> DemoFinal) {
        if !demo.ended() {
            self.ended = None;
            return;
        }
        if self.ended.is_none() {
            let mut final_ = final_();
            final_.frags = self.deaths.iter().filter(|d| d.killer_id.is_some()).count() as u32;
            let (first, last) = demo.tick_span();
            final_.duration_ms = (last - first) / demo.tick_rate() * 1000.0;
            self.ended = Some((self.clock, demo_end_card(&final_)));
        }
    }

    /// The names over the opening and the end card over its fade to black;
    /// returns the end card's Replay or Exit.
    pub fn ends_ui(&self, ctx: &egui::Context, demo: &DemoPlayback) -> Option<DemoOverlayAction> {
        let screen = ctx.content_rect();
        let center = screen.center();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::unique("demo-ends"),
        ));
        let Some((at, card)) = &self.ended else {
            let (first, _) = demo.tick_span();
            let elapsed = (demo.playhead_tick() - first) / demo.tick_rate();
            if elapsed < DEMO_TITLE_SECONDS {
                // Fades over its last half second, as the card's transition.
                let fade = ((DEMO_TITLE_SECONDS - elapsed) / 0.5).clamp(0.0, 1.0) as f32;
                let line = self
                    .title
                    .sides
                    .iter()
                    .map(|s| match &s.label {
                        Some(label) => format!("{label}: {}", s.names.join(", ")),
                        None => s.names.join(", "),
                    })
                    .collect::<Vec<_>>()
                    .join("  vs  ");
                let more = if self.title.more > 0 {
                    format!("  and {} more", self.title.more)
                } else {
                    String::new()
                };
                // Wrapped to the screen: a free-for-all names up to eight.
                let mut job = egui::text::LayoutJob::simple(
                    format!("{line}{more}"),
                    FontId::proportional(30.0),
                    CREAM.gamma_multiply(fade),
                    screen.width() - 96.0,
                );
                job.halign = egui::Align::Center;
                let names = painter.layout_job(job);
                let top = screen.top() + screen.height() * 0.22;
                let height = names.size().y;
                painter.galley(Pos2::new(center.x, top), names, CREAM);
                painter.text(
                    Pos2::new(center.x, top + height + 16.0),
                    Align2::CENTER_CENTER,
                    format!("{} on {}", demo.header.gametype.to_uppercase(), demo.header.map),
                    FontId::proportional(18.0),
                    CREAM.gamma_multiply(0.7 * fade),
                );
            }
            return None;
        };
        let veil = ((self.clock - at) / DEMO_END_FADE_SECONDS).clamp(0.0, 1.0);
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha((veil * 255.0) as u8));
        // The keys fade in after the veil.
        let shown = (veil * 255.0) as u8;
        let fade = |c: Color32| c.gamma_multiply(f32::from(shown) / 255.0);
        let mut y = center.y - 24.0 * card.chips.len() as f32 - 20.0;
        for (label, value) in &card.chips {
            painter.text(
                Pos2::new(center.x - 8.0, y),
                Align2::RIGHT_CENTER,
                label.to_uppercase(),
                FontId::proportional(16.0),
                fade(GOLD),
            );
            painter.text(
                Pos2::new(center.x + 8.0, y),
                Align2::LEFT_CENTER,
                value,
                FontId::proportional(26.0),
                fade(CREAM),
            );
            y += 44.0;
        }
        painter.text(
            Pos2::new(center.x, y),
            Align2::CENTER_CENTER,
            &card.counts,
            FontId::proportional(17.0),
            fade(CREAM.gamma_multiply(0.7)),
        );
        if veil < 1.0 {
            return None;
        }
        let mut action = None;
        egui::Area::new(egui::Id::unique("demo-end-keys"))
            .fade_in(false)
            .fixed_pos(Pos2::new(center.x - 130.0, y + 36.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (label, choice) in [("Replay", DemoOverlayAction::Replay), ("Exit", DemoOverlayAction::Exit)] {
                        let key = egui::Button::new(egui::RichText::new(label).size(20.0).color(INK).strong())
                            .fill(GOLD)
                            .min_size(vec2(120.0, 40.0))
                            .sense(Sense::CLICK);
                        if ui.add(key).clicked() {
                            action = Some(choice);
                        }
                    }
                });
            });
        action
    }

    /// The ticks of the demo's frags (deaths with a killer), for the frag
    /// keys.
    #[must_use]
    pub fn frag_ticks(&self) -> Vec<f64> {
        self.deaths
            .iter()
            .filter(|d| d.killer_id.is_some())
            .map(|d| d.tick)
            .collect()
    }

    /// A key or the mouse moved: show the bar.
    pub fn wake(&mut self) {
        self.idle = 0.0;
    }

    /// Ages the bar; a paused demo keeps it up.
    pub fn update(&mut self, dt: f64, paused: bool) {
        self.clock += dt;
        self.idle = if paused { 0.0 } else { self.idle + dt };
    }

    fn opacity(&self) -> f32 {
        (1.0 - ((self.idle - IDLE_AFTER) / FADE).clamp(0.0, 1.0)) as f32
    }

    /// Draws the bar; returns what a click asked for.
    pub fn ui(
        &self,
        ctx: &egui::Context,
        demo: &DemoPlayback,
        followed: Option<(&str, &str)>,
    ) -> Option<DemoOverlayAction> {
        let opacity = self.opacity();
        if opacity <= 0.0 {
            return None;
        }
        let screen = ctx.content_rect();
        let height = 64.0;
        let bar = Rect::from_min_max(
            Pos2::new(screen.left() + 24.0, screen.bottom() - height - 16.0),
            Pos2::new(screen.right() - 24.0, screen.bottom() - 16.0),
        );
        let mut action = exit_plate(ctx, screen.min, opacity);
        egui::Area::new(egui::Id::unique("demo-viewer-hud"))
            .fixed_pos(bar.min)
            .fade_in(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(bar.size(), Sense::hover());
                let painter = ui.painter();
                let fade = |c: Color32| c.gamma_multiply(opacity);
                // `hsla(250, 30%, 11%, 0.95)` with its cyan edge.
                painter.rect_filled(rect, 2.0, fade(INK));
                painter.rect_stroke(rect, 2.0, Stroke::new(1.0, fade(EDGE)), egui::StrokeKind::Inside);
                // Row one: the timeline.
                let line = Rect::from_min_max(
                    Pos2::new(rect.left() + 16.0, rect.top() + 14.0),
                    Pos2::new(rect.right() - 16.0, rect.top() + 20.0),
                );
                let hit = ui.interact(
                    line.expand2(vec2(0.0, 8.0)),
                    ui.scope_id().with("timeline"),
                    Sense::CLICK,
                );
                let (first, last) = demo.tick_span();
                let progress = demo.progress_of_tick(demo.playhead_tick()) as f32;
                painter.rect_filled(line, 3.0, fade(Color32::from_white_alpha(45)));
                painter.rect_filled(
                    Rect::from_min_max(
                        line.min,
                        Pos2::new(line.left() + line.width() * progress, line.bottom()),
                    ),
                    3.0,
                    fade(GOLD),
                );
                let x_of = |p: f64| line.left() + line.width() * p as f32;
                let followed_id = followed.map(|(id, _)| id);
                let mut hovered_mark = None;
                for death in &self.deaths {
                    let colour = match mark_kind(death, followed_id) {
                        MarkKind::Frag => GOLD,
                        MarkKind::Death => RED,
                        MarkKind::Other => CREAM.gamma_multiply(0.55),
                    };
                    let x = x_of(death.progress);
                    painter.line_segment(
                        [Pos2::new(x, line.top() - 4.0), Pos2::new(x, line.bottom() + 4.0)],
                        Stroke::new(2.0, fade(colour)),
                    );
                    if let Some(pointer) = hit.hover_pos()
                        && (pointer.x - x).abs() <= 3.0
                    {
                        hovered_mark = Some(death);
                    }
                }
                painter.circle_filled(Pos2::new(x_of(f64::from(progress)), line.center().y), 6.0, fade(CREAM));
                if let Some(death) = hovered_mark {
                    let label = match &death.killer_name {
                        Some(killer) => format!("{killer} fragged {}", death.victim_name),
                        None => format!("{} died", death.victim_name),
                    };
                    painter.text(
                        Pos2::new(x_of(death.progress), line.top() - 10.0),
                        Align2::CENTER_BOTTOM,
                        label,
                        FontId::proportional(13.0),
                        fade(CREAM),
                    );
                }
                if hit.clicked()
                    && let Some(pointer) = hit.interact_pointer_pos()
                {
                    action = Some(DemoOverlayAction::Seek(match hovered_mark {
                        Some(death) => demo.progress_of_tick(death.tick - TIMELINE_LEAD_SECONDS * demo.tick_rate()),
                        None => f64::from(((pointer.x - line.left()) / line.width()).clamp(0.0, 1.0)),
                    }));
                }
                // Row two: the keys and the read-outs.
                let row = rect.top() + 44.0;
                let play = Rect::from_center_size(Pos2::new(rect.left() + 34.0, row), vec2(40.0, 24.0));
                let play_hit = ui.interact(play, ui.scope_id().with("play"), Sense::CLICK);
                painter.rect_filled(
                    play,
                    4.0,
                    fade(Color32::from_white_alpha(if play_hit.hovered() { 60 } else { 30 })),
                );
                // The glyphs are drawn: the fonts have no pause sign.
                let c = play.center();
                if demo.paused() {
                    painter.add(egui::Shape::convex_polygon(
                        vec![c + vec2(-5.0, -7.0), c + vec2(7.0, 0.0), c + vec2(-5.0, 7.0)],
                        fade(CREAM),
                        Stroke::NONE,
                    ));
                } else {
                    for dx in [-4.0, 4.0] {
                        painter.rect_filled(
                            Rect::from_center_size(c + vec2(dx, 0.0), vec2(4.0, 14.0)),
                            1.0,
                            fade(CREAM),
                        );
                    }
                }
                if play_hit.clicked() {
                    action = Some(DemoOverlayAction::TogglePause);
                }
                let duration = (last - first) / demo.tick_rate();
                let now = (demo.playhead_tick() - first) / demo.tick_rate();
                painter.text(
                    Pos2::new(play.right() + 14.0, row),
                    Align2::LEFT_CENTER,
                    format!("{} / {}", format_time(now.min(duration)), format_time(duration)),
                    FontId::monospace(16.0),
                    fade(CREAM),
                );
                let status = if demo.ended() {
                    "Ended"
                } else if demo.paused() {
                    "Paused"
                } else {
                    "Playing"
                };
                painter.text(
                    Pos2::new(play.right() + 150.0, row),
                    Align2::LEFT_CENTER,
                    status,
                    FontId::proportional(14.0),
                    fade(CREAM.gamma_multiply(0.75)),
                );
                if let Some((_, name)) = followed {
                    painter.text(
                        Pos2::new(rect.center().x, row),
                        Align2::CENTER_CENTER,
                        format!("Watching {name}"),
                        FontId::proportional(15.0),
                        fade(GOLD),
                    );
                }
                // The speed, with a key either side.
                let speed_text = format!("{}x", demo.speed());
                let right = rect.right() - 16.0;
                for (step, label, x) in [(-1i8, "−", right - 92.0), (1, "+", right - 12.0)] {
                    let key = Rect::from_center_size(Pos2::new(x, row), vec2(24.0, 24.0));
                    let key_hit = ui.interact(key, ui.scope_id().with(("speed", step)), Sense::CLICK);
                    painter.rect_filled(
                        key,
                        4.0,
                        fade(Color32::from_white_alpha(if key_hit.hovered() { 60 } else { 30 })),
                    );
                    painter.text(
                        key.center(),
                        Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(16.0),
                        fade(CREAM),
                    );
                    if key_hit.clicked() {
                        action = Some(DemoOverlayAction::Speed(step));
                    }
                }
                painter.text(
                    Pos2::new(right - 52.0, row),
                    Align2::CENTER_CENTER,
                    speed_text,
                    FontId::monospace(14.0),
                    fade(CREAM),
                );
            });
        action
    }
}

/// `.dvh-exit`: a gold plate cut away at its lower right, with a ✕ in
/// ink, in the corner at `at`; its click leaves playback.
fn exit_plate(ctx: &egui::Context, at: Pos2, opacity: f32) -> Option<DemoOverlayAction> {
    let size = vec2(52.0, 34.0);
    let mut action = None;
    egui::Area::new(egui::Id::unique("demo-viewer-exit"))
        .fixed_pos(at)
        .fade_in(false)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let (rect, hit) = ui.allocate_exact_size(size, Sense::CLICK);
            let hit = hit.on_hover_text("Leave playback (Esc)");
            let fade = |c: Color32| {
                // `filter: brightness(1.08)` on hover.
                let c = if hit.hovered() {
                    let up = |v: u8| (f32::from(v) * 1.08).min(255.0) as u8;
                    Color32::from_rgb(up(c.r()), up(c.g()), up(c.b()))
                } else {
                    c
                };
                c.gamma_multiply(opacity)
            };
            // `clip-path: polygon(0 0, 100% 0, calc(100% - 10px) 100%, 0 100%)`,
            // `linear-gradient(240deg, #ffc52c, #efa600)`: light at the top
            // right, deeper at the bottom left.
            let light = fade(Color32::from_rgb(0xff, 0xc5, 0x2c));
            let deep = fade(Color32::from_rgb(0xef, 0xa6, 0x00));
            let corners = [
                (rect.left_top(), deep.lerp_to_gamma(light, 0.4)),
                (rect.right_top(), light),
                (
                    Pos2::new(rect.right() - 10.0, rect.bottom()),
                    deep.lerp_to_gamma(light, 0.5),
                ),
                (rect.left_bottom(), deep),
            ];
            let mut mesh = egui::Mesh::default();
            for (pos, color) in corners {
                mesh.colored_vertex(pos, color);
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            let painter = ui.painter();
            painter.add(mesh);
            // `box-shadow: inset 0 -3px 0 #cd8600`.
            let lip = rect.bottom() - 3.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(rect.left(), lip),
                    Pos2::new(rect.right() - 10.0 + 10.0 * 3.0 / size.y, lip),
                    Pos2::new(rect.right() - 10.0, rect.bottom()),
                    rect.left_bottom(),
                ],
                fade(Color32::from_rgb(0xcd, 0x86, 0x00)),
                Stroke::NONE,
            ));
            // The ✕, drawn: padding 14 left, 22 right; 8 top, 9 bottom.
            let mark = Rect::from_min_max(
                Pos2::new(rect.left() + 14.0, rect.top() + 8.0),
                Pos2::new(rect.right() - 22.0, rect.bottom() - 9.0),
            );
            let mark = Rect::from_center_size(mark.center(), vec2(11.0, 11.0));
            let ink = Stroke::new(2.6, fade(Color32::from_rgb(0x17, 0x13, 0x26)));
            painter.line_segment([mark.left_top(), mark.right_bottom()], ink);
            painter.line_segment([mark.right_top(), mark.left_bottom()], ink);
            if hit.clicked() {
                action = Some(DemoOverlayAction::Exit);
            }
        });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn death(victim: &str, killer: Option<&str>) -> DemoDeath {
        DemoDeath {
            tick: 0.0,
            progress: 0.0,
            victim_id: victim.to_owned(),
            killer_id: killer.map(str::to_owned),
            victim_name: victim.to_owned(),
            killer_name: killer.map(str::to_owned),
        }
    }

    #[test]
    fn marks_take_the_followed_side() {
        let d = death("b", Some("a"));
        assert_eq!(mark_kind(&d, Some("a")), MarkKind::Frag);
        assert_eq!(mark_kind(&d, Some("b")), MarkKind::Death);
        assert_eq!(mark_kind(&d, Some("c")), MarkKind::Other);
        assert_eq!(mark_kind(&d, None), MarkKind::Other);
        assert_eq!(mark_kind(&death("a", None), Some("a")), MarkKind::Death);
    }

    #[test]
    fn times_read_as_minutes_and_seconds() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(61.9), "1:01");
        assert_eq!(format_time(600.0), "10:00");
    }
}
