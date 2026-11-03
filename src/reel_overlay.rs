//! A highlight reel's picture over the scene, after wash-fps
//! `client/DemoViewerHUD.ts` in its reel mode: the veil of each transition,
//! the title card over the opening black, each shot's card as it starts,
//! the closing card, and the reel's timeline with its shots marked.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, vec2};
use shagger::ui::egui_layer::egui;
use washoi::client::cinematic_demo_player::{CinematicDemoPlayer, ReelState};
use washoi::client::demo_playback::DemoPlayback;
use washoi::client::reel_cards::{ShotCard, shot_card};

/// How long a shot's card stays up, then fades over the last half second.
const CARD_SECONDS: f64 = 3.2;
const GOLD: Color32 = Color32::from_rgb(0xf5, 0xc5, 0x18);
const CREAM: Color32 = Color32::from_rgb(0xf6, 0xec, 0xd4);

/// What the overlay remembers between frames.
#[derive(Default)]
pub struct ReelOverlay {
    card: Option<(ShotCard, f64)>,
    clock: f64,
}

impl ReelOverlay {
    /// Takes the shots that started, ages the card.
    pub fn update(&mut self, reel: &mut CinematicDemoPlayer, dt: f64) {
        self.clock += dt;
        // The first card waits for the title to lift.
        if reel.is_showing_title()
            && let Some((_, at)) = &mut self.card
        {
            *at = self.clock;
        }
        for index in reel.take_segment_changes() {
            let Some(r) = reel.reel() else { continue };
            if let Some(segment) = r.segments.get(index) {
                // A scene's card shows once, on its first shot.
                let first = index == 0 || r.segments[index - 1].scene != segment.scene;
                if first {
                    self.card = Some((shot_card(segment, r.total_scenes), self.clock));
                }
            }
        }
    }

    pub fn ui(&self, ctx: &egui::Context, reel: &CinematicDemoPlayer, demo: &DemoPlayback) {
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("reel")));
        let veil = reel.veil(demo);
        if veil.opacity > 0.0 {
            let a = (veil.opacity.clamp(0.0, 1.0) * 255.0) as u8;
            let color = if veil.white {
                Color32::from_rgba_unmultiplied(255, 255, 255, a)
            } else {
                Color32::from_rgba_unmultiplied(0, 0, 0, a)
            };
            painter.rect_filled(screen, 0.0, color);
        }
        let center = screen.center();
        if reel.is_showing_title()
            && let Some(title) = reel.title()
        {
            painter.text(
                center - vec2(0.0, 60.0),
                Align2::CENTER_CENTER,
                "HIGHLIGHTS",
                FontId::proportional(22.0),
                GOLD,
            );
            let line = title
                .sides
                .iter()
                .map(|s| match &s.label {
                    Some(label) => format!("{label}: {}", s.names.join(", ")),
                    None => s.names.join(", "),
                })
                .collect::<Vec<_>>()
                .join("  vs  ");
            let more = if title.more > 0 {
                format!("  and {} more", title.more)
            } else {
                String::new()
            };
            painter.text(
                center,
                Align2::CENTER_CENTER,
                format!("{line}{more}"),
                FontId::proportional(34.0),
                CREAM,
            );
            painter.text(
                center + vec2(0.0, 48.0),
                Align2::CENTER_CENTER,
                format!("{} on {}", demo.header.gametype.to_uppercase(), demo.header.map),
                FontId::proportional(18.0),
                CREAM.gamma_multiply(0.7),
            );
        }
        if reel.state() == ReelState::Ended
            && let Some(summary) = reel.summary()
        {
            let mut y = center.y - 70.0;
            let mut line = |text: String, size: f32, color: Color32| {
                painter.text(
                    Pos2::new(center.x, y),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(size),
                    color,
                );
                y += size + 14.0;
            };
            line("THAT'S THE REEL".to_owned(), 22.0, GOLD);
            if let Some((name, frags)) = &summary.top_fragger {
                line(format!("Top fragger: {name}, {frags} frags"), 28.0, CREAM);
            }
            if let Some((title, player)) = &summary.best {
                line(format!("Best moment: {title} by {player}"), 22.0, CREAM);
            }
            line(
                format!(
                    "{} frags, {} tricks  ·  R to watch it again",
                    summary.frags, summary.tricks
                ),
                18.0,
                CREAM.gamma_multiply(0.7),
            );
        }
        if let Some((card, at)) = &self.card
            && reel.state() != ReelState::Ended
            && !reel.is_showing_title()
        {
            let age = self.clock - at;
            if age < CARD_SECONDS {
                let fade = ((CARD_SECONDS - age) / 0.5).min(1.0) as f32;
                egui::Area::new(egui::Id::unique("reel-card"))
                    .anchor(Align2::LEFT_BOTTOM, vec2(32.0, -56.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        ui.set_opacity(fade);
                        egui::Frame::new()
                            .fill(Color32::from_black_alpha(170))
                            .inner_margin(14.0)
                            .corner_radius(6.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(&card.kicker).color(GOLD).size(15.0));
                                ui.label(RichText::new(&card.title).color(CREAM).size(30.0).strong());
                                ui.label(RichText::new(&card.player).color(CREAM).size(20.0));
                                let mut rest = card.tags.join(" · ");
                                if !card.detail.is_empty() {
                                    rest = if rest.is_empty() {
                                        card.detail.clone()
                                    } else {
                                        format!("{} · {rest}", card.detail)
                                    };
                                }
                                if !rest.is_empty() {
                                    ui.label(RichText::new(rest).color(CREAM.gamma_multiply(0.75)).size(16.0));
                                }
                            });
                    });
            }
        }
        // The timeline: the reel's progress with each shot's start marked.
        let info = reel.reel_info(demo);
        if info.total_segments > 0 {
            let bar = Rect::from_min_max(
                Pos2::new(screen.left() + 32.0, screen.bottom() - 24.0),
                Pos2::new(screen.right() - 32.0, screen.bottom() - 20.0),
            );
            painter.rect_filled(bar, 2.0, Color32::from_white_alpha(40));
            let done = Rect::from_min_max(
                bar.min,
                Pos2::new(bar.left() + bar.width() * info.reel_progress as f32, bar.bottom()),
            );
            painter.rect_filled(done, 2.0, GOLD);
            for start in reel.shot_starts().into_iter().skip(1) {
                let x = bar.left() + bar.width() * start as f32;
                painter.line_segment(
                    [Pos2::new(x, bar.top() - 3.0), Pos2::new(x, bar.bottom() + 3.0)],
                    Stroke::new(1.0, CREAM.gamma_multiply(0.6)),
                );
            }
        }
    }
}
