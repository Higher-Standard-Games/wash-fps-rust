//! A highlight reel's picture over the scene, after wash-fps
//! `client/DemoViewerHUD.ts` in its reel mode: the veil of each transition,
//! the comic effects over the picture (`ReelEffects`: speed lines, freeze
//! frames, frag counters and each scene's tag), the title card over the
//! opening black, the closing card, and the reel's timeline with its shots
//! marked.

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, vec2};
use shagger::ui::egui_layer::egui;
use washoi::client::cinematic_demo_player::{CinematicDemoPlayer, ReelState};
use washoi::client::demo_playback::DemoPlayback;
use washoi::client::reel_effects::draw_reel_effects;

const GOLD: Color32 = Color32::from_rgb(0xf5, 0xc5, 0x18);
const CREAM: Color32 = Color32::from_rgb(0xf6, 0xec, 0xd4);

/// What the overlay remembers between frames.
#[derive(Default)]
pub struct ReelOverlay;

impl ReelOverlay {
    /// Takes the shots that started; the effects come from the player each frame.
    pub fn update(&mut self, reel: &mut CinematicDemoPlayer, _dt: f64) {
        reel.take_segment_changes();
    }

    pub fn ui(&self, ctx: &egui::Context, reel: &CinematicDemoPlayer, demo: &DemoPlayback) {
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("reel")));
        // The comic effects under the veil, as the HUD stacks them.
        let effects = reel.effects(demo);
        if effects.any() {
            draw_reel_effects(&painter, &effects, screen);
        }
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
