//! Port of wash-fps `server/systems/LiquidWorldEffects.ts`: what a liquid
//! does to you for being in it. Drowning, and the damage lava and slime
//! deal, as pure rules over a per-player timer (Q3's numbers).

use crate::liquid::liquid_types::WaterLevel;

/// Seconds of air a player has once their head goes under.
pub const AIR_SECONDS: f64 = 12.0;
/// Drowning damage climbs by this each second, up to the max.
pub const DROWN_DAMAGE_STEP: f64 = 2.0;
pub const DROWN_DAMAGE_MAX: f64 = 15.0;
/// Damage per second per water level.
pub const LAVA_DAMAGE_PER_LEVEL: f64 = 30.0;
pub const SLIME_DAMAGE_PER_LEVEL: f64 = 10.0;
/// How often liquid burns and drowning bites, in seconds.
pub const EFFECT_INTERVAL: f64 = 1.0;

/// `LiquidEffectState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidEffectState {
    /// Seconds of air left. Refilled whenever the head is clear.
    pub air: f64,
    pub drown_timer: f64,
    /// Escalating drowning damage; resets when the player breathes.
    pub drown_damage: f64,
    pub burn_timer: f64,
}

impl Default for LiquidEffectState {
    /// `createLiquidEffectState`.
    fn default() -> Self {
        Self {
            air: AIR_SECONDS,
            drown_timer: 0.0,
            drown_damage: 0.0,
            burn_timer: 0.0,
        }
    }
}

/// `LiquidEffectResult`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LiquidEffectResult {
    /// Damage from having no air; ignores armour.
    pub drown_damage: f64,
    pub burn_damage: f64,
    /// True on the tick air runs out, for the gasp.
    pub started_drowning: bool,
}

/// `tickLiquidEffects`: one player's liquid effects for `delta_time`.
pub fn tick_liquid_effects(
    state: &mut LiquidEffectState,
    level: WaterLevel,
    is_lava: bool,
    is_slime: bool,
    delta_time: f64,
) -> LiquidEffectResult {
    let burn_damage = tick_burn(state, level, is_lava, is_slime, delta_time);
    let (drown_damage, started_drowning) = tick_drowning(state, level, delta_time);
    LiquidEffectResult {
        drown_damage,
        burn_damage,
        started_drowning,
    }
}

fn tick_burn(state: &mut LiquidEffectState, level: WaterLevel, is_lava: bool, is_slime: bool, delta_time: f64) -> f64 {
    if level == 0 || (!is_lava && !is_slime) {
        // The next contact burns at once.
        state.burn_timer = 0.0;
        return 0.0;
    }
    state.burn_timer -= delta_time;
    if state.burn_timer > 0.0 {
        return 0.0;
    }
    state.burn_timer += EFFECT_INTERVAL;
    let level = f64::from(level);
    let mut damage = 0.0;
    if is_lava {
        damage += LAVA_DAMAGE_PER_LEVEL * level;
    }
    if is_slime {
        damage += SLIME_DAMAGE_PER_LEVEL * level;
    }
    damage
}

fn tick_drowning(state: &mut LiquidEffectState, level: WaterLevel, delta_time: f64) -> (f64, bool) {
    // Anything short of fully submerged is a breath.
    if level < 3 {
        state.air = AIR_SECONDS;
        state.drown_timer = 0.0;
        state.drown_damage = 0.0;
        return (0.0, false);
    }
    if state.air > 0.0 {
        state.air = (state.air - delta_time).max(0.0);
        if state.air > 0.0 {
            return (0.0, false);
        }
        // Air just ran out: the first hit lands on this tick.
        state.drown_timer = 0.0;
    }
    state.drown_timer -= delta_time;
    if state.drown_timer > 0.0 {
        return (0.0, false);
    }
    let started = state.drown_damage == 0.0;
    state.drown_timer += EFFECT_INTERVAL;
    state.drown_damage = DROWN_DAMAGE_MAX.min(state.drown_damage + DROWN_DAMAGE_STEP);
    (state.drown_damage, started)
}

/// `airFraction`.
#[must_use]
pub fn air_fraction(state: &LiquidEffectState) -> f64 {
    (state.air / AIR_SECONDS).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drowning_starts_after_twelve_seconds_and_escalates() {
        let mut state = LiquidEffectState::default();
        let mut damage = Vec::new();
        let mut started = 0;
        for _ in 0..(16 * 10) {
            let r = tick_liquid_effects(&mut state, 3, false, false, 0.1);
            if r.started_drowning {
                started += 1;
            }
            if r.drown_damage > 0.0 {
                damage.push(r.drown_damage);
            }
        }
        assert_eq!(started, 1);
        assert_eq!(damage[..3], [2.0, 4.0, 6.0]);
        assert_eq!(air_fraction(&state), 0.0);
        // A breath refills.
        tick_liquid_effects(&mut state, 2, false, false, 0.1);
        assert_eq!(air_fraction(&state), 1.0);
    }

    #[test]
    fn lava_and_slime_burn_by_depth_at_once() {
        let mut state = LiquidEffectState::default();
        let r = tick_liquid_effects(&mut state, 2, true, true, 0.016);
        assert_eq!(r.burn_damage, 2.0 * 30.0 + 2.0 * 10.0);
        assert_eq!(tick_liquid_effects(&mut state, 2, true, false, 0.5).burn_damage, 0.0);
        tick_liquid_effects(&mut state, 0, true, false, 0.016);
        assert_eq!(tick_liquid_effects(&mut state, 1, true, false, 0.016).burn_damage, 30.0);
    }
}
