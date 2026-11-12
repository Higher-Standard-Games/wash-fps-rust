//! Ported from wash-fps `client/MoveStick.ts`.
//!
//! A move stick's reading, as movement takes it.
//!
//! The stick gives any angle. Movement takes a world direction, which is
//! normalized, so a half-pushed stick still runs at full speed, and the four
//! move keys, which decide running (pure forward only) and animation. The keys
//! split the circle into eight equal sectors, as pressing one or two keys does:
//! each key covers its own direction 67.5° either side, and two keys overlap on
//! the diagonals.

use shagger::shared::js::{js_hypot, js_sin};

/// `MoveAxis`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MoveAxis {
    /// Forward is positive, back negative.
    pub forward: f64,
    /// Right is positive, left negative.
    pub right: f64,
}

/// `MoveKeys`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveKeys {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
}

/// Below this fraction of its travel the stick reads as centred.
pub const STICK_DEAD_ZONE: f64 = 0.12;

/// sin 22.5°: the edge between a straight sector and a diagonal one.
fn key_threshold() -> f64 {
    js_sin(std::f64::consts::PI / 8.0)
}

/// `stickDirection`: the stick's direction with length 1, or `None` while
/// it is inside the dead zone.
#[must_use]
pub fn stick_direction(axis: Option<MoveAxis>) -> Option<MoveAxis> {
    let axis = axis?;
    let length = js_hypot(&[axis.forward, axis.right]);
    if length < STICK_DEAD_ZONE {
        return None;
    }
    Some(MoveAxis {
        forward: axis.forward / length,
        right: axis.right / length,
    })
}

/// `stickKeys`: the move keys the stick holds; all released inside the dead
/// zone.
#[must_use]
pub fn stick_keys(axis: Option<MoveAxis>) -> MoveKeys {
    let Some(dir) = stick_direction(axis) else {
        return MoveKeys::default();
    };
    let t = key_threshold();
    MoveKeys {
        forward: dir.forward > t,
        backward: dir.forward < -t,
        left: dir.right < -t,
        right: dir.right > t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(forward: f64, right: f64) -> Option<MoveAxis> {
        Some(MoveAxis { forward, right })
    }

    #[test]
    fn a_half_pushed_stick_still_points_at_full_length() {
        let d = stick_direction(axis(0.3, 0.4)).unwrap();
        assert!((d.forward - 0.6).abs() < 1e-12 && (d.right - 0.8).abs() < 1e-12);
        assert_eq!(stick_direction(axis(0.05, 0.05)), None);
        assert_eq!(stick_direction(None), None);
    }

    #[test]
    fn the_keys_split_the_circle_into_eight_sectors() {
        assert_eq!(
            stick_keys(axis(1.0, 0.0)),
            MoveKeys {
                forward: true,
                ..MoveKeys::default()
            }
        );
        assert_eq!(
            stick_keys(axis(1.0, 1.0)),
            MoveKeys {
                forward: true,
                right: true,
                ..MoveKeys::default()
            }
        );
        // 20° off forward is still forward only.
        let a = 20f64.to_radians();
        assert_eq!(
            stick_keys(axis(a.cos(), -a.sin())),
            MoveKeys {
                forward: true,
                ..MoveKeys::default()
            }
        );
        assert_eq!(stick_keys(axis(0.0, 0.0)), MoveKeys::default());
    }
}
