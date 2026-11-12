//! Ported from wash-fps `client/hud/HintCoach.ts`.
//!
//! HintCoach decides when a hint for a new player is worth showing.
//!
//! It watches the local player's movement, their state (health, armor, the
//! weapon in hand, water, the bomb) and a few events (shots, fall damage, an
//! emptied clip), and offers a hint when the player is in the situation the
//! hint is about. A different hint replaces the one on screen at once; the
//! same hint shows at most once a minute, and a few only once.
//!
//! Times are milliseconds (`performance.now()`), as in the TypeScript.

use std::collections::HashMap;

use crate::simulation::movement_config::{LedgeGrabPhase, WallJumpPhase};
use crate::simulation::movement_profile::DEFAULT_FPS_MOVEMENT_CONFIG;

/// `HintId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HintId {
    Slide,
    Aim,
    Walljump,
    Bunnyhop,
    Slidespeed,
    Strafe,
    Ledge,
    Fall,
    Health,
    Armor,
    Shock,
    Air,
    Switch,
    Blade,
    Rocketjump,
    Grenade,
    Reload,
    Scoreboard,
    Plant,
    Defuse,
}

/// The local player's movement, from a predicted movement step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MovementSample {
    pub grounded: bool,
    pub sliding: bool,
    /// Horizontal speed, units/s
    pub horizontal_speed: f64,
    /// Vertical speed, units/s, up positive
    pub vertical_speed: f64,
    pub wall_jump: WallJumpPhase,
    pub ledge_grab: LedgeGrabPhase,
    /// Forward held, and either strafe key held
    pub forward: bool,
    pub strafing: bool,
}

/// The local player's state, from the HUD.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSample {
    pub health: f64,
    pub armor: f64,
    /// Milliseconds since the live match began, or `None` in warmup
    pub live_for_ms: Option<f64>,
    /// The weapon in hand, and whether a gun other than the blade is carried
    pub weapon: String,
    pub carries_gun: bool,
    pub grounded: bool,
    /// 0 dry, 1 wading, 2 swimming, 3 under
    pub water_level: u8,
    /// Air left, 0-1
    pub air: f64,
    /// Carrying the bomb; defending a planted bomb
    pub carries_bomb: bool,
    pub defending_planted_bomb: bool,
}

/// Horizontal speed at which crouching starts a slide.
pub const SLIDE_SPEED: f64 = DEFAULT_FPS_MOVEMENT_CONFIG.slide_min_speed;
/// How long a player runs that fast without sliding before the slide hint (ms).
const FAST_RUN_MS: f64 = 300.0;
/// Jumps within this window make a hop chain (ms).
const HOP_WINDOW_MS: f64 = 2000.0;
/// Three jumps within this window are jumping in a row: time to try a wall (ms).
const JUMP_RUN_MS: f64 = 3000.0;
/// Upward speed that makes leaving the ground a jump rather than a step off (units/s).
const JUMP_SPEED: f64 = 150.0;
/// How long fire is held from the hip before the aim hint (ms).
const HIP_HOLD_MS: f64 = 1000.0;
/// Health under this, for `HURT_MS`, is hurt enough to look for health.
const HURT_HEALTH: f64 = 50.0;
const HURT_MS: f64 = 3000.0;
/// Time in a live match without armor before the armor hint (ms).
const NO_ARMOR_MS: f64 = 15_000.0;
/// Air left under this is running out.
const LOW_AIR: f64 = 0.5;
/// Weapons that shock their holder in water, and the depth they do it at.
const SHOCK_WEAPONS: [&str; 2] = ["voltaic", "electrobolt"];
const SHOCK_WATER_LEVEL: u8 = 2;
/// The blade held this long with a gun carried: time to switch (ms).
const BLADE_WITH_GUN_MS: f64 = 8000.0;
/// The blade held this long without a charged blast (ms).
const BLADE_UNCHARGED_MS: f64 = 12_000.0;
/// The rocket launcher held on the ground this long, in all (ms).
const ROCKET_GROUND_MS: f64 = 10_000.0;
/// Into a live match this long, the scoreboard hint (ms).
const SCOREBOARD_AFTER_MS: f64 = 20_000.0;
/// Carrying the bomb this long (ms).
const CARRY_BOMB_MS: f64 = 3000.0;
/// How long a hint stays on screen before it fades (ms).
pub const HINT_SHOW_MS: f64 = 4_500.0;
/// Least time before the same hint shows again (ms).
pub const HINT_REPEAT_MS: f64 = 60_000.0;
/// Hints that show only once in a session.
const ONCE: [HintId; 2] = [HintId::Scoreboard, HintId::Grenade];

/// `HintCoach`.
pub struct HintCoach {
    last_shown: HashMap<HintId, f64>,
    previous: Option<MovementSample>,
    fast_since: Option<f64>,
    jumps: Vec<f64>,
    /// Whether the current time in the air has been forward-only (no strafe)
    air_forward_only: bool,
    last_air_forward_only: bool,
    hip_hold_since: Option<f64>,
    /// When each held condition began, by name
    since: HashMap<&'static str, f64>,
    rocket_ground_ms: f64,
    last_status_at: Option<f64>,
    last_charge_at: f64,
    blade_since: Option<f64>,
    random: Box<dyn FnMut() -> f64>,
}

impl std::fmt::Debug for HintCoach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HintCoach")
            .field("last_shown", &self.last_shown)
            .finish_non_exhaustive()
    }
}

impl HintCoach {
    /// `new HintCoach(random)`.
    pub fn new(random: impl FnMut() -> f64 + 'static) -> Self {
        Self {
            last_shown: HashMap::new(),
            previous: None,
            fast_since: None,
            jumps: Vec::new(),
            air_forward_only: false,
            last_air_forward_only: false,
            hip_hold_since: None,
            since: HashMap::new(),
            rocket_ground_ms: 0.0,
            last_status_at: None,
            last_charge_at: f64::NEG_INFINITY,
            blade_since: None,
            random: Box::new(random),
        }
    }

    /// Take one movement sample; returns a hint to show now, if one is due.
    pub fn observe(&mut self, sample: &MovementSample, now: f64) -> Option<HintId> {
        let before = self.previous.replace(*sample);
        if before.is_some_and(|b| b.grounded) && !sample.grounded && sample.vertical_speed > JUMP_SPEED {
            self.jumps.push(now);
            self.last_air_forward_only = self.air_forward_only;
            self.air_forward_only = sample.forward;
        }
        if !sample.grounded && (sample.strafing || !sample.forward) {
            self.air_forward_only = false;
        }
        self.jumps.retain(|&at| now - at <= JUMP_RUN_MS);

        if sample.ledge_grab == LedgeGrabPhase::Grabbing
            && before.is_none_or(|b| b.ledge_grab != LedgeGrabPhase::Grabbing)
            && self.offer(HintId::Ledge, now)
        {
            return Some(HintId::Ledge);
        }

        // Running fast enough to slide, on the ground, not sliding
        if sample.grounded && !sample.sliding && sample.horizontal_speed >= SLIDE_SPEED {
            self.fast_since.get_or_insert(now);
        } else {
            self.fast_since = None;
        }

        if self.fast_since.is_some_and(|since| now - since >= FAST_RUN_MS) && self.offer(HintId::Slide, now) {
            return Some(HintId::Slide);
        }
        if (sample.wall_jump == WallJumpPhase::Available || self.jumps.len() >= 3) && self.offer(HintId::Walljump, now)
        {
            return Some(HintId::Walljump);
        }
        if self.jumps.iter().filter(|&&at| now - at <= HOP_WINDOW_MS).count() >= 2 {
            // Hopping with forward alone gains nothing: strafing is the lesson
            if self.last_air_forward_only && self.offer(HintId::Strafe, now) {
                return Some(HintId::Strafe);
            }
            let pick = if (self.random)() < 0.5 {
                HintId::Bunnyhop
            } else {
                HintId::Slidespeed
            };
            let other = if pick == HintId::Bunnyhop {
                HintId::Slidespeed
            } else {
                HintId::Bunnyhop
            };
            if self.offer(pick, now) {
                return Some(pick);
            }
            if self.offer(other, now) {
                return Some(other);
            }
        }
        None
    }

    /// Take the player's state from the HUD; returns a hint to show now, if one is due.
    pub fn status(&mut self, s: &StatusSample, now: f64) -> Option<HintId> {
        let dt = self.last_status_at.map_or(0.0, |last| (now - last).min(1000.0));
        self.last_status_at = Some(now);

        if s.weapon == "rlauncher" && s.grounded {
            self.rocket_ground_ms += dt;
        }
        if s.weapon == "blade" {
            self.blade_since.get_or_insert(now);
        } else {
            self.blade_since = None;
        }

        let hurt = self.held("hurt", s.health > 0.0 && s.health < HURT_HEALTH, now, HURT_MS);
        let unarmored = self.held("unarmored", s.live_for_ms.is_some() && s.armor <= 0.0, now, NO_ARMOR_MS);
        let bomb = self.held("bomb", s.carries_bomb, now, CARRY_BOMB_MS);
        let blade_with_gun = self.held(
            "blade-with-gun",
            s.weapon == "blade" && s.carries_gun,
            now,
            BLADE_WITH_GUN_MS,
        );
        let candidates = [
            (
                HintId::Shock,
                SHOCK_WEAPONS.contains(&s.weapon.as_str()) && s.water_level >= SHOCK_WATER_LEVEL,
            ),
            (HintId::Air, s.air < LOW_AIR),
            (HintId::Health, hurt),
            (HintId::Armor, unarmored),
            (HintId::Plant, bomb),
            (HintId::Defuse, s.defending_planted_bomb),
            (HintId::Switch, blade_with_gun),
            (
                HintId::Blade,
                self.blade_since
                    .is_some_and(|since| now - since.max(self.last_charge_at) >= BLADE_UNCHARGED_MS),
            ),
            (HintId::Rocketjump, self.rocket_ground_ms >= ROCKET_GROUND_MS),
            (HintId::Grenade, s.weapon == "glauncher"),
            (
                HintId::Scoreboard,
                s.live_for_ms.is_some_and(|ms| ms >= SCOREBOARD_AFTER_MS),
            ),
        ];
        for (id, due) in candidates {
            if !due || !self.offer(id, now) {
                continue;
            }
            if id == HintId::Rocketjump {
                self.rocket_ground_ms = 0.0;
            }
            return Some(id);
        }
        None
    }

    /// The local player fired; returns the aim hint for a hip shot with a weapon that can aim.
    pub fn shot(&mut self, now: f64, from_hip: bool, can_aim: bool) -> Option<HintId> {
        (from_hip && can_aim && self.offer(HintId::Aim, now)).then_some(HintId::Aim)
    }

    /// Whether the local player is holding fire from the hip with a weapon that
    /// can aim; returns the aim hint once they have held it a second.
    pub fn holding_fire(&mut self, now: f64, from_hip: bool) -> Option<HintId> {
        if !from_hip {
            self.hip_hold_since = None;
            return None;
        }
        let since = *self.hip_hold_since.get_or_insert(now);
        (now - since >= HIP_HOLD_MS && self.offer(HintId::Aim, now)).then_some(HintId::Aim)
    }

    /// The local player took fall damage; returns the hint to land crouched, if it is due.
    pub fn fell(&mut self, now: f64) -> Option<HintId> {
        self.offer(HintId::Fall, now).then_some(HintId::Fall)
    }

    /// The local player's clip ran dry with ammo in reserve; returns the reload hint, if due.
    pub fn clip_emptied(&mut self, now: f64) -> Option<HintId> {
        self.offer(HintId::Reload, now).then_some(HintId::Reload)
    }

    /// The local player charged a blast: the blade hint waits again.
    pub fn charged(&mut self, now: f64) {
        self.last_charge_at = now;
    }

    /// Whether a condition has held for at least `ms`.
    fn held(&mut self, name: &'static str, holding: bool, now: f64, ms: f64) -> bool {
        if !holding {
            self.since.remove(name);
            return false;
        }
        now - *self.since.entry(name).or_insert(now) >= ms
    }

    /// Show this hint now unless it has shown lately (or, for a few, at all).
    fn offer(&mut self, id: HintId, now: f64) -> bool {
        if let Some(&last) = self.last_shown.get(&id)
            && (ONCE.contains(&id) || now - last < HINT_REPEAT_MS)
        {
            return false;
        }
        self.last_shown.insert(id, now);
        true
    }
}

/// Ported from wash-fps `client/hud/__tests__/HintCoach.test.ts`.
#[cfg(test)]
mod tests {
    use super::*;

    const STILL: MovementSample = MovementSample {
        grounded: true,
        sliding: false,
        horizontal_speed: 0.0,
        vertical_speed: 0.0,
        wall_jump: WallJumpPhase::None,
        ledge_grab: LedgeGrabPhase::None,
        forward: false,
        strafing: false,
    };

    fn running() -> MovementSample {
        MovementSample {
            horizontal_speed: SLIDE_SPEED + 20.0,
            ..STILL
        }
    }

    fn coach() -> HintCoach {
        HintCoach::new(|| 0.5)
    }

    fn fixed(value: f64) -> HintCoach {
        HintCoach::new(move || value)
    }

    /// Feed samples every 16 ms, as movement steps arrive; returns the hints offered.
    fn run(c: &mut HintCoach, sample: MovementSample, from: f64, ms: f64) -> Vec<HintId> {
        let mut hints = Vec::new();
        let mut t = from;
        while t < from + ms {
            if let Some(hint) = c.observe(&sample, t) {
                hints.push(hint);
            }
            t += 16.0;
        }
        hints
    }

    fn jump_at(c: &mut HintCoach, t: f64) -> Option<HintId> {
        c.observe(&STILL, t);
        c.observe(
            &MovementSample {
                grounded: false,
                vertical_speed: 320.0,
                ..STILL
            },
            t + 16.0,
        )
    }

    #[test]
    fn offers_the_slide_hint_once_the_player_runs_fast_enough_to_slide_for_a_moment() {
        let mut c = coach();
        assert_eq!(run(&mut c, running(), 0.0, 250.0), vec![]);
        assert_eq!(run(&mut c, running(), 250.0, 150.0), vec![HintId::Slide]);
    }

    #[test]
    fn offers_no_slide_hint_to_a_player_who_is_sliding_or_too_slow_to_slide() {
        let mut c = coach();
        let sliding = MovementSample {
            sliding: true,
            ..running()
        };
        assert_eq!(run(&mut c, sliding, 0.0, 2000.0), vec![]);
        let slow = MovementSample {
            horizontal_speed: SLIDE_SPEED - 1.0,
            ..running()
        };
        assert_eq!(run(&mut c, slow, 2000.0, 2000.0), vec![]);
    }

    #[test]
    fn offers_the_walljump_hint_in_the_air_beside_a_wall() {
        let sample = MovementSample {
            grounded: false,
            wall_jump: WallJumpPhase::Available,
            ..STILL
        };
        assert_eq!(coach().observe(&sample, 0.0), Some(HintId::Walljump));
    }

    #[test]
    fn offers_a_hop_hint_after_two_jumps_within_two_seconds() {
        for (random, expected) in [(0.2, HintId::Bunnyhop), (0.8, HintId::Slidespeed)] {
            let mut c = fixed(random);
            assert_eq!(jump_at(&mut c, 0.0), None);
            assert_eq!(jump_at(&mut c, 900.0), Some(expected));
        }
    }

    #[test]
    fn offers_the_walljump_hint_on_a_third_jump_in_a_row() {
        let mut c = fixed(0.2);
        assert_eq!(jump_at(&mut c, 0.0), None);
        assert_eq!(jump_at(&mut c, 900.0), Some(HintId::Bunnyhop));
        assert_eq!(jump_at(&mut c, 1800.0), Some(HintId::Walljump));
    }

    #[test]
    fn offers_the_aim_hint_on_a_hip_shot_with_a_weapon_that_can_aim() {
        let mut c = coach();
        assert_eq!(c.shot(0.0, true, false), None);
        assert_eq!(c.shot(0.0, false, true), None);
        assert_eq!(c.shot(0.0, true, true), Some(HintId::Aim));
    }

    #[test]
    fn offers_the_aim_hint_once_fire_is_held_from_the_hip_for_a_second() {
        let mut c = coach();
        assert_eq!(c.holding_fire(0.0, true), None);
        assert_eq!(c.holding_fire(900.0, true), None);
        // Letting go, or aiming, starts it over
        assert_eq!(c.holding_fire(950.0, false), None);
        assert_eq!(c.holding_fire(1000.0, true), None);
        assert_eq!(c.holding_fire(1999.0, true), None);
        assert_eq!(c.holding_fire(2000.0, true), Some(HintId::Aim));
    }

    #[test]
    fn offers_the_landing_hint_on_fall_damage() {
        assert_eq!(coach().fell(0.0), Some(HintId::Fall));
    }

    #[test]
    fn shows_a_different_hint_at_once_and_the_same_hint_at_most_once_a_minute() {
        let mut c = coach();
        assert_eq!(c.fell(0.0), Some(HintId::Fall));
        assert_eq!(c.shot(100.0, true, true), Some(HintId::Aim));
        assert_eq!(c.fell(HINT_REPEAT_MS - 1.0), None);
        assert_eq!(c.fell(HINT_REPEAT_MS), Some(HintId::Fall));
    }

    #[test]
    fn offers_the_strafe_hint_to_a_player_hopping_with_forward_alone() {
        let hop = |c: &mut HintCoach, t: f64, strafing: bool| {
            let air = |vertical_speed: f64| MovementSample {
                grounded: false,
                vertical_speed,
                forward: true,
                strafing,
                ..STILL
            };
            c.observe(&MovementSample { forward: true, ..STILL }, t)
                .or_else(|| c.observe(&air(320.0), t + 16.0))
                .or_else(|| c.observe(&air(0.0), t + 300.0))
        };
        let mut forward_only = fixed(0.2);
        hop(&mut forward_only, 0.0, false);
        assert_eq!(hop(&mut forward_only, 900.0, false), Some(HintId::Strafe));
        let mut strafer = fixed(0.2);
        hop(&mut strafer, 0.0, true);
        assert_eq!(hop(&mut strafer, 900.0, true), Some(HintId::Bunnyhop));
    }

    #[test]
    fn offers_the_ledge_hint_on_grabbing_a_ledge() {
        let mut c = coach();
        c.observe(
            &MovementSample {
                grounded: false,
                ..STILL
            },
            0.0,
        );
        let grab = MovementSample {
            grounded: false,
            ledge_grab: LedgeGrabPhase::Grabbing,
            ..STILL
        };
        assert_eq!(c.observe(&grab, 16.0), Some(HintId::Ledge));
    }

    #[test]
    fn offers_the_reload_hint_when_the_clip_runs_dry() {
        assert_eq!(coach().clip_emptied(0.0), Some(HintId::Reload));
    }

    mod status_hints {
        use super::*;

        fn fine() -> StatusSample {
            StatusSample {
                health: 100.0,
                armor: 50.0,
                live_for_ms: None,
                weapon: "machinegun".into(),
                carries_gun: true,
                grounded: true,
                water_level: 0,
                air: 1.0,
                carries_bomb: false,
                defending_planted_bomb: false,
            }
        }

        /// Status every 50 ms, as the HUD ticks; returns the hints offered.
        fn tick(c: &mut HintCoach, s: &StatusSample, from: f64, ms: f64) -> Vec<HintId> {
            let mut hints = Vec::new();
            let mut t = from;
            while t <= from + ms {
                if let Some(hint) = c.status(s, t) {
                    hints.push(hint);
                }
                t += 50.0;
            }
            hints
        }

        #[test]
        fn says_nothing_to_a_player_who_is_fine() {
            assert_eq!(tick(&mut coach(), &fine(), 0.0, 30_000.0), vec![]);
        }

        #[test]
        fn points_a_hurt_player_to_health_after_a_few_seconds() {
            let mut c = coach();
            let hurt = StatusSample { health: 40.0, ..fine() };
            assert_eq!(tick(&mut c, &hurt, 0.0, 2900.0), vec![]);
            assert_eq!(tick(&mut c, &hurt, 2950.0, 100.0), vec![HintId::Health]);
        }

        #[test]
        fn points_to_armor_after_15_s_without_any_in_a_live_match_never_in_warmup() {
            let bare = StatusSample { armor: 0.0, ..fine() };
            assert_eq!(tick(&mut coach(), &bare, 0.0, 30_000.0), vec![]);
            let mut c = coach();
            let live = StatusSample {
                live_for_ms: Some(0.0),
                ..bare.clone()
            };
            assert_eq!(tick(&mut c, &live, 0.0, 14_900.0), vec![]);
            let later = StatusSample {
                live_for_ms: Some(15_000.0),
                ..bare
            };
            assert_eq!(tick(&mut c, &later, 14_950.0, 100.0), vec![HintId::Armor]);
        }

        #[test]
        fn warns_about_the_shock_and_the_air_at_once() {
            let shock = StatusSample {
                weapon: "voltaic".into(),
                water_level: 2,
                ..fine()
            };
            assert_eq!(tick(&mut coach(), &shock, 0.0, 0.0), vec![HintId::Shock]);
            let under = StatusSample {
                water_level: 3,
                ..fine()
            };
            assert_eq!(tick(&mut coach(), &under, 0.0, 0.0), vec![]);
            let air = StatusSample { air: 0.4, ..fine() };
            assert_eq!(tick(&mut coach(), &air, 0.0, 0.0), vec![HintId::Air]);
        }

        #[test]
        fn suggests_switching_from_the_blade_and_the_blast_to_one_who_never_charges() {
            let blade = StatusSample {
                weapon: "blade".into(),
                ..fine()
            };
            assert_eq!(tick(&mut coach(), &blade, 0.0, 8000.0), vec![HintId::Switch]);
            let mut c = coach();
            let bare = StatusSample {
                carries_gun: false,
                ..blade
            };
            tick(&mut c, &bare, 0.0, 6000.0);
            c.charged(6000.0);
            assert_eq!(tick(&mut c, &bare, 6050.0, 11_900.0), vec![]);
            assert_eq!(tick(&mut c, &bare, 18_000.0, 100.0), vec![HintId::Blade]);
        }

        #[test]
        fn suggests_rocket_jumping_after_ten_seconds_on_the_ground_in_all() {
            let mut c = coach();
            let rl = StatusSample {
                weapon: "rlauncher".into(),
                ..fine()
            };
            assert_eq!(tick(&mut c, &rl, 0.0, 6000.0), vec![]);
            let air = StatusSample {
                grounded: false,
                ..rl.clone()
            };
            assert_eq!(tick(&mut c, &air, 6050.0, 5000.0), vec![]);
            assert_eq!(tick(&mut c, &rl, 11_100.0, 4100.0), vec![HintId::Rocketjump]);
        }

        #[test]
        fn tells_about_bouncing_grenades_and_the_scoreboard_once_only() {
            let mut c = coach();
            let gl = StatusSample {
                weapon: "glauncher".into(),
                ..fine()
            };
            assert_eq!(tick(&mut c, &gl, 0.0, 0.0), vec![HintId::Grenade]);
            assert_eq!(tick(&mut c, &gl, HINT_REPEAT_MS * 2.0, 0.0), vec![]);
            let at = |ms: f64| StatusSample {
                live_for_ms: Some(ms),
                ..fine()
            };
            assert_eq!(tick(&mut c, &at(19_000.0), 0.0, 0.0), vec![]);
            assert_eq!(tick(&mut c, &at(20_000.0), 100.0, 0.0), vec![HintId::Scoreboard]);
            assert_eq!(tick(&mut c, &at(200_000.0), HINT_REPEAT_MS * 3.0, 0.0), vec![]);
        }

        #[test]
        fn tells_the_bomb_carrier_how_to_arm_and_a_defender_how_to_defuse() {
            let carrier = StatusSample {
                carries_bomb: true,
                ..fine()
            };
            assert_eq!(tick(&mut coach(), &carrier, 0.0, 3000.0), vec![HintId::Plant]);
            let defender = StatusSample {
                defending_planted_bomb: true,
                ..fine()
            };
            assert_eq!(tick(&mut coach(), &defender, 0.0, 0.0), vec![HintId::Defuse]);
        }
    }
}
