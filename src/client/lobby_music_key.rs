//! wash-fps `client/LobbyMusicKey.ts`: the lobby music key, a speaker in
//! the Play page's links bar that silences the menus' music, and only
//! that. It stays pressed down while the music is off, and the choice is
//! kept in `preferences.json` (the browser's local storage).
//!
//! The music itself plays in the lobby screen
//! ([`crate::client::game_events::GameAudio::start_menu_music`]), which
//! reads [`lobby_music_muted`] each frame.

use shagger::ui::components::lobby_twin::hsla;
use shagger::ui::egui_layer::egui;

use egui::{Color32, Pos2, Rect, Shape, Stroke, vec2};

use crate::client::lobby_preferences::{read_preference, write_preference};
use crate::client::lobby_welcome_dialog::dark_key_face;

pub const LOBBY_MUSIC_PREFERENCE: &str = "washoi.lobby.musicMuted";

/// `lobbyMusicMuted`: whether the player turned the lobby music off.
#[must_use]
pub fn lobby_music_muted() -> bool {
    read_preference(LOBBY_MUSIC_PREFERENCE).as_deref() == Some("true")
}

/// `setLobbyMusicMuted`: remember the choice. The key still works for this
/// visit when the file cannot be written.
pub fn set_lobby_music_muted(muted: bool) {
    write_preference(LOBBY_MUSIC_PREFERENCE, muted.then_some("true"));
}

/// `createLobbyMusicKey`: the 44 by 44 key at `r`, down while `muted`.
/// True when pressed; the caller flips and stores the choice.
pub fn lobby_music_key(ui: &mut egui::Ui, r: Rect, muted: bool) -> bool {
    let (clicked, face) = dark_key_face(ui, "lobby-music", r, muted);
    let ink = if muted {
        hsla(45.0, 100.0, 92.0, 0.6)
    } else {
        Color32::from_rgb(0xff, 0xf6, 0xdf)
    };
    speaker(ui.painter(), face, muted, ink);
    clicked
}

/// `SPEAKER` and `SPEAKER_MUTED`: the 24-unit icon drawn 20 px wide in the
/// middle of `face`.
fn speaker(painter: &egui::Painter, face: Rect, muted: bool, ink: Color32) {
    let scale = 20.0 / 24.0;
    let origin = face.center() - vec2(10.0, 10.0);
    let at = |x: f32, y: f32| origin + vec2(x, y) * scale;
    // `M4 9h4l5-4v14l-5-4H4z`, as a box and the cone beside it (egui fills
    // convex shapes only).
    painter.rect_filled(Rect::from_min_max(at(4.0, 9.0), at(8.0, 15.0)), 0.0, ink);
    painter.add(Shape::convex_polygon(
        vec![at(8.0, 9.0), at(13.0, 5.0), at(13.0, 19.0), at(8.0, 15.0)],
        ink,
        Stroke::NONE,
    ));
    let stroke = Stroke::new(2.0 * scale, ink);
    let round = |painter: &egui::Painter, points: Vec<Pos2>| {
        for p in [points[0], points[points.len() - 1]] {
            painter.circle_filled(p, scale, ink);
        }
        painter.add(Shape::line(points, stroke));
    };
    if muted {
        // `M16 9l6 6M22 9l-6 6`.
        round(painter, vec![at(16.0, 9.0), at(22.0, 15.0)]);
        round(painter, vec![at(22.0, 9.0), at(16.0, 15.0)]);
    } else {
        // `M16 8.5a5 5 0 0 1 0 7` and `M18.5 6a8.5 8.5 0 0 1 0 12`: arcs
        // about a centre left of their chords.
        for (x, half, radius) in [(16.0_f32, 3.5_f32, 5.0_f32), (18.5, 6.0, 8.5)] {
            let cx = x - (radius * radius - half * half).sqrt();
            let sweep = (half / radius).asin();
            let points = (0..=12)
                .map(|i| {
                    let a = -sweep + 2.0 * sweep * i as f32 / 12.0;
                    at(cx + radius * a.cos(), 12.0 + radius * a.sin())
                })
                .collect();
            round(painter, points);
        }
    }
}
