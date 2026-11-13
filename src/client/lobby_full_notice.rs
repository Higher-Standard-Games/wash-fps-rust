//! wash-fps `client/LobbyFullNotice.ts`: what a player turned away at the
//! lobby's player cap sees: the servers are full, a countdown to the next
//! try, and a key to try now. Also the lobby offline, with the same
//! countdown to the next reconnect.
//!
//! It sits over a corner of the lobby rather than covering it, so
//! settings, demos and the rest of the menu still work while the player
//! waits. The panel keeps the countdown (its `retry_at`); this draws it.

use std::time::Instant;

use shagger::shared::js::js_round;
use shagger::ui::components::lobby_twin::{cream, hsl, hsla, text};
use shagger::ui::components::raised_key::{KeyLook, KeyText, KeyTextLayout, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, FontId, Rect, Shape, Stroke, pos2, vec2};

use crate::client::washoi_style::display_family;

/// Waits are spread this far either side of the lobby's, so turned-away
/// players don't all return at once.
const RETRY_JITTER_SEC: f64 = 10.0;
const MIN_WAIT_SEC: f64 = 5.0;
/// When the lobby gives no wait.
const DEFAULT_WAIT_SEC: u32 = 60;

/// `retryDelaySec`: the lobby's wait, give or take `RETRY_JITTER_SEC`.
#[must_use]
pub fn retry_delay_sec(retry_after_sec: Option<u32>, random: f64) -> u64 {
    let base = retry_after_sec.filter(|&s| s > 0).unwrap_or(DEFAULT_WAIT_SEC);
    let jitter = js_round((random * 2.0 - 1.0) * RETRY_JITTER_SEC);
    (f64::from(base) + jitter).max(MIN_WAIT_SEC) as u64
}

/// `offlineRetryDelaySec`: 2, 5, 10, then 30 seconds, each give or take a
/// fifth, so short outages recover quickly and longer ones back off
/// without every client returning at once.
#[must_use]
pub fn offline_retry_delay_sec(attempt: u32, random: f64) -> u64 {
    const WAITS: [f64; 4] = [2.0, 5.0, 10.0, 30.0];
    let base = WAITS[(attempt.max(1) as usize - 1).min(WAITS.len() - 1)];
    js_round(base * (0.8 + random * 0.4)).max(1.0) as u64
}

/// Which notice: `wait` (the servers are full) or `waitOffline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Full,
    Offline,
}

/// `.lobby-full`: the notice in the lower right, above the links bar
/// (`links_top`). `retry_at` is the next try; `None` once it is under way
/// (`tryNow`: "Trying…", "Connecting…"). True when Try now is pressed.
pub fn lobby_full_notice(
    ui: &mut egui::Ui,
    screen: Rect,
    links_top: f32,
    kind: NoticeKind,
    retry_at: Option<Instant>,
) -> bool {
    let painter = ui.painter().clone();
    let (title, say) = match kind {
        NoticeKind::Full => (
            "SERVERS ARE FULL",
            "The maximum number of players are online. Please try again shortly.",
        ),
        NoticeKind::Offline => (
            "LOBBY OFFLINE",
            "We will reconnect automatically. You can also try now.",
        ),
    };
    // `width: min(380px, 100vw - 32px)`, padding 18 22.
    let narrow = screen.width() <= 640.0;
    let edge = if narrow { 16.0 } else { 24.0 };
    let w = 380.0_f32.min(screen.width() - 32.0);
    let inner = w - 44.0;
    let title_g = text(&painter, title, 20.0, display_family(), hsl(46.0, 100.0, 60.0), 0.06);
    let text_g = painter.layout(
        say.to_owned(),
        FontId::new(16.0, FontFamily::Proportional),
        cream(),
        inner,
    );
    let countdown = match retry_at {
        Some(at) => format!(
            "Retrying in {} s…",
            at.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64
        ),
        None => "Connecting…".to_owned(),
    };
    let count_g = text(
        &painter,
        &countdown,
        16.0,
        FontFamily::Proportional,
        hsla(45.0, 100.0, 95.0, 0.72),
        0.0,
    );
    let key_label = if retry_at.is_some() { "TRY NOW" } else { "TRYING…" };
    let key_g = text(&painter, key_label, 13.0, display_family(), Color32::WHITE, 0.08);
    let key_size = vec2(key_g.size().x + 32.0, 40.0);
    let h = 18.0 + title_g.size().y + 6.0 + text_g.size().y + 14.0 + key_size.y + 3.6 + 18.0;
    let r = Rect::from_min_size(pos2(screen.right() - edge - w, links_top - edge - h), vec2(w, h));
    ui.interact(r, egui::Id::NULL.with("lobby-full"), egui::Sense::hover());
    // The card, its upper right corner cut 18 px.
    let cut = |r: Rect| {
        vec![
            r.left_top(),
            pos2(r.right() - 18.0, r.top()),
            pos2(r.right(), r.top() + 18.0),
            r.right_bottom(),
            r.left_bottom(),
        ]
    };
    painter.rect_filled(
        r.translate(vec2(0.0, 18.0)).expand(6.0),
        12.0,
        Color32::from_black_alpha(70),
    );
    shagger::ui::components::lobby_twin::gradient_polygon(
        &painter,
        &cut(r),
        &[
            (0.0, hsla(250.0, 40.0, 13.0, 0.94)),
            (1.0, hsla(250.0, 44.0, 8.0, 0.96)),
        ],
    );
    let mut outline = cut(r);
    outline.push(r.left_top());
    painter.add(Shape::line(outline, Stroke::new(1.5, hsla(46.0, 100.0, 60.0, 0.55))));
    let mut y = r.top() + 18.0;
    let title_h = title_g.size().y;
    painter.galley(pos2(r.left() + 22.0, y), title_g, Color32::WHITE);
    y += title_h + 6.0;
    let text_h = text_g.size().y;
    painter.galley(pos2(r.left() + 22.0, y), text_g, cream());
    y += text_h + 14.0;
    let key_rect = Rect::from_min_size(pos2(r.right() - 22.0 - key_size.x, y), key_size);
    let face = RaisedKey::new("lobby-full-try", key_rect, KeyLook::cream().sm())
        .disabled(retry_at.is_none())
        .show(ui);
    KeyText::new(key_label, "", display_family(), FontFamily::Proportional)
        .size(13.0, 13.0)
        .spacing(0.08)
        .pad(4.0, 4.0)
        .layout(KeyTextLayout::Centered)
        .paint(&painter, &face);
    let ch = count_g.size().y;
    painter.galley(
        pos2(r.left() + 22.0, face.rect.center().y - ch / 2.0),
        count_g,
        Color32::WHITE,
    );
    face.clicked && retry_at.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_spread_about_the_lobbys() {
        assert_eq!(offline_retry_delay_sec(1, 0.5), 2);
        assert_eq!(offline_retry_delay_sec(4, 0.5), 30);
        assert_eq!(offline_retry_delay_sec(9, 0.0), 24);
        assert_eq!(retry_delay_sec(Some(30), 0.5), 30);
        assert_eq!(retry_delay_sec(None, 1.0), 70);
        assert_eq!(retry_delay_sec(Some(1), 0.0), 5);
    }
}
