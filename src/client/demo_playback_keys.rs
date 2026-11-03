//! Port of wash-fps `client/DemoPlaybackKeys.ts`: the keyboard over demo
//! playback and highlight reels, what a key does apart from doing it, so the
//! mapping can be read (and tested) in one place.
//!
//!   Space        play or pause
//!   ← / →        5 seconds back or on (a reel: the previous or next shot)
//!   , / .        one frame back or on, paused
//!   ↑ / ↓        faster or slower
//!   P / N        the previous or next frag
//!   Tab          the next player's eyes (Shift+Tab the previous)
//!   1 – 9        that player's eyes, in the player list's order
//!
//! Keys are DOM `code` names (`shagger::input::key_codes::dom_code`).

/// `DemoKeyAction`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DemoKeyAction {
    TogglePause,
    Seek { seconds: f64 },
    Shot { step: i8 },
    Frame { step: i8 },
    Speed { step: i8 },
    Frag { step: i8 },
    Player { step: i8 },
    PlayerAt { index: usize },
}

/// `DemoKeyContext`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DemoKeyContext {
    /// A highlight reel rather than a whole demo: only pause and shots apply.
    pub reel: bool,
    /// The free-flying camera has the movement keys (Space, arrows).
    pub free_camera: bool,
}

/// The modifier keys held with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
}

/// `DEMO_SEEK_SECONDS`: how far ← and → move a demo.
pub const DEMO_SEEK_SECONDS: f64 = 5.0;

/// `demoKeyAction`.
#[must_use]
pub fn demo_key_action(code: &str, modifiers: Modifiers, context: DemoKeyContext) -> Option<DemoKeyAction> {
    if modifiers.alt || modifiers.ctrl || modifiers.meta {
        return None;
    }
    let movement = code == "Space" || code.starts_with("Arrow");
    if movement && context.free_camera {
        return None;
    }
    if code == "Space" {
        return Some(DemoKeyAction::TogglePause);
    }
    if context.reel {
        return match code {
            "ArrowLeft" => Some(DemoKeyAction::Shot { step: -1 }),
            "ArrowRight" => Some(DemoKeyAction::Shot { step: 1 }),
            _ => None,
        };
    }
    match code {
        "ArrowLeft" => {
            return Some(DemoKeyAction::Seek {
                seconds: -DEMO_SEEK_SECONDS,
            });
        }
        "ArrowRight" => {
            return Some(DemoKeyAction::Seek {
                seconds: DEMO_SEEK_SECONDS,
            });
        }
        "Comma" => return Some(DemoKeyAction::Frame { step: -1 }),
        "Period" => return Some(DemoKeyAction::Frame { step: 1 }),
        "ArrowUp" => return Some(DemoKeyAction::Speed { step: 1 }),
        "ArrowDown" => return Some(DemoKeyAction::Speed { step: -1 }),
        "KeyN" => return Some(DemoKeyAction::Frag { step: 1 }),
        "KeyP" => return Some(DemoKeyAction::Frag { step: -1 }),
        "Tab" => {
            return Some(DemoKeyAction::Player {
                step: if modifiers.shift { -1 } else { 1 },
            });
        }
        _ => {}
    }
    let digit = code.strip_prefix("Digit")?.parse::<usize>().ok()?;
    (1..=9)
        .contains(&digit)
        .then(|| DemoKeyAction::PlayerAt { index: digit - 1 })
}

/// `fragSeekTick`: where a frag key lands, a couple of seconds before the
/// next (or previous) frag so it is seen coming. `from` is the playhead; a
/// frag just jumped to counts as passed, so pressing again moves on.
#[must_use]
pub fn frag_seek_tick(frag_ticks: &[f64], from: f64, step: i8, tick_rate: f64) -> Option<f64> {
    let lead_in = 2.0 * tick_rate;
    let mut starts: Vec<f64> = frag_ticks.iter().map(|t| (t - lead_in).max(0.0)).collect();
    starts.sort_by(f64::total_cmp);
    let slack = 0.5 * tick_rate;
    if step > 0 {
        starts.into_iter().find(|&t| t > from + slack)
    } else {
        starts.into_iter().rev().find(|&t| t < from - slack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: &str) -> Option<DemoKeyAction> {
        demo_key_action(code, Modifiers::default(), DemoKeyContext::default())
    }

    #[test]
    fn a_demo_maps_its_keys() {
        assert_eq!(key("Space"), Some(DemoKeyAction::TogglePause));
        assert_eq!(key("ArrowLeft"), Some(DemoKeyAction::Seek { seconds: -5.0 }));
        assert_eq!(key("Period"), Some(DemoKeyAction::Frame { step: 1 }));
        assert_eq!(key("ArrowDown"), Some(DemoKeyAction::Speed { step: -1 }));
        assert_eq!(key("KeyP"), Some(DemoKeyAction::Frag { step: -1 }));
        assert_eq!(key("Digit3"), Some(DemoKeyAction::PlayerAt { index: 2 }));
        assert_eq!(key("Digit0"), None);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert_eq!(
            demo_key_action("Tab", shift, DemoKeyContext::default()),
            Some(DemoKeyAction::Player { step: -1 })
        );
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert_eq!(demo_key_action("Space", ctrl, DemoKeyContext::default()), None);
    }

    #[test]
    fn a_reel_and_the_free_camera_take_fewer() {
        let reel = DemoKeyContext {
            reel: true,
            free_camera: false,
        };
        assert_eq!(
            demo_key_action("ArrowRight", Modifiers::default(), reel),
            Some(DemoKeyAction::Shot { step: 1 })
        );
        assert_eq!(demo_key_action("KeyN", Modifiers::default(), reel), None);
        let free = DemoKeyContext {
            reel: false,
            free_camera: true,
        };
        assert_eq!(demo_key_action("Space", Modifiers::default(), free), None);
        assert_eq!(
            demo_key_action("KeyN", Modifiers::default(), free),
            Some(DemoKeyAction::Frag { step: 1 })
        );
    }

    #[test]
    fn frag_keys_land_before_the_frag_and_move_on() {
        let frags = [400.0, 100.0, 1000.0];
        // 60 Hz: two seconds of lead-in, half a second of slack.
        assert_eq!(frag_seek_tick(&frags, 0.0, 1, 60.0), Some(280.0));
        assert_eq!(frag_seek_tick(&frags, 280.0, 1, 60.0), Some(880.0));
        assert_eq!(frag_seek_tick(&frags, 880.0, -1, 60.0), Some(280.0));
        assert_eq!(frag_seek_tick(&frags, 900.0, 1, 60.0), None);
        assert_eq!(frag_seek_tick(&frags, 10.0, -1, 60.0), None);
    }
}
