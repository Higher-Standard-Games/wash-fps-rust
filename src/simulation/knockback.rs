//! Knockback air drag (wash-fps `simulation/Knockback.ts`), shared by the
//! server tick and client prediction.

use shagger::shared::server::simulation::types::Vec3;

/// Steps after an impulse before drag begins (about 0.4 s at 125 Hz).
pub const KB_DRAG_DELAY_TICKS: f64 = 50.0;
/// Steps over which drag ramps up to full strength (about 0.6 s).
pub const KB_DRAG_RAMP_TICKS: f64 = 75.0;
const KB_HORIZ_DRAG: f64 = 0.96;
const KB_EXTRA_GRAVITY: f64 = 600.0;

/// `readKnockbackAge`: an integer age in the drag window, or none.
#[must_use]
pub fn read_knockback_age(value: f64) -> Option<f64> {
    (value.fract() == 0.0 && (0.0..=KB_DRAG_DELAY_TICKS + KB_DRAG_RAMP_TICKS).contains(&value)).then_some(value)
}

/// `applyKnockbackDrag`: one step of drag at `age`, before that step's
/// movement. None once a landing or the end of the window ends it.
pub fn apply_knockback_drag(velocity: &mut Vec3, grounded: bool, age: f64, dt: f64) -> Option<f64> {
    if grounded || read_knockback_age(age).is_none() {
        return None;
    }
    if age < KB_DRAG_DELAY_TICKS {
        return Some(age);
    }
    let ramp = (age - KB_DRAG_DELAY_TICKS) / KB_DRAG_RAMP_TICKS;
    let strength = ramp * ramp;
    let multiplier = 1.0 - strength * (1.0 - KB_HORIZ_DRAG);
    velocity.x *= multiplier;
    velocity.z *= multiplier;
    velocity.y -= KB_EXTRA_GRAVITY * strength * dt;
    Some(age)
}
