//! Port of wash-fps `liquid/client/LiquidInteractionSystem.ts`: bodies
//! crossing a liquid surface, the splash, and the ring it leaves.
//!
//! It feeds on the water level already replicated for every player:
//! replicate causes, derive effects. Water and slime ripple on the surface
//! itself (the water system's disturbances); a ring disc remains for lava
//! and for surfaces the water system does not draw. The clock is the
//! caller's (`now` in seconds) where the TypeScript reads `performance.now`.
//!
//! With crowns on, a water splash throws spray (`SplashDroplets`) and the
//! harder landings ripple where they fall back.
//!
//! The discs are drawn by `liquid_ring_renderer`.

use std::collections::HashMap;

use shagger::rendering::glam::DVec3;
use shagger::shared::collision::collision_world::MASK_LIQUID;
use shagger::shared::js::{js_cos, js_hypot, js_sin};

use crate::client::fps_liquid_audio::{HIT_WATER, LiquidCrossingSound, enter_sound, exit_sound, splash_volume};
use crate::liquid::liquid_types::{LiquidKind, WaterLevel};
use crate::liquid::liquid_volume_sampler::liquid_kind_at;

use super::splash_droplets::{DropletSpawn, SplashDroplets};

/// How many rings can be alive at once. Beyond this the oldest is recycled.
pub const RING_POOL_SIZE: usize = 12;
/// Seconds a ring takes to expand and fade.
pub const RING_LIFETIME: f64 = 0.5;
const RING_START_RADIUS: f64 = 4.0;
pub const RING_END_RADIUS: f64 = 48.0;

/// How far above the body to start looking for the surface, and how far down.
const SURFACE_SEARCH_ABOVE: f64 = 8.0;
const SURFACE_SEARCH_STEP: f64 = 4.0;
const SURFACE_SEARCH_STEPS: u32 = 24;

/// Lift off the surface so the ring does not fight the water for the pixel.
const RING_SURFACE_LIFT: f64 = 0.5;

/// How hard a droplet must land to ring the water, in units per second.
const REENTRY_RING_SPEED: f64 = 180.0;
/// Re-entry ripples are faint: a crown's aftermath, not another splash.
const REENTRY_RING_STRENGTH: f64 = 0.25;

/// Directions probed to find the pool's edge.
const EDGE_PROBE_DIRECTIONS: u32 = 8;
const EDGE_PROBE_STEPS: u32 = 6;

/// `RING_COLORS`.
#[must_use]
pub fn ring_color(kind: LiquidKind) -> [f32; 3] {
    match kind {
        LiquidKind::Water => [0.75, 0.9, 1.0],
        LiquidKind::Slime => [0.6, 0.95, 0.35],
        LiquidKind::Lava => [1.0, 0.75, 0.35],
    }
}

/// `PointContentsSource`: the contents at a Babylon-space point.
pub type PointContents<'a> = &'a dyn Fn(DVec3) -> u32;

/// The water system's ripples (`waterDisturbancesFor(scene)` with its
/// `resolveSurface` set): `add` snaps to a drawn pool and says whether one
/// took it.
pub trait WaterRipples {
    fn add(&mut self, position: DVec3, strength: f64, radius: f64, direction: Option<(f64, f64)>) -> bool;
}

impl WaterRipples for shagger::rendering::water::liquid_surface_system::LiquidSurfaceSystem {
    fn add(&mut self, position: DVec3, strength: f64, radius: f64, direction: Option<(f64, f64)>) -> bool {
        self.add_disturbance(position, strength, radius, direction)
    }
}

/// A ring disc on the surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub position: DVec3,
    pub kind: LiquidKind,
    pub alpha: f64,
    /// The disc's radius this frame.
    pub scale: f64,
    /// Seconds since spawn, or at least [`RING_LIFETIME`] when free.
    pub age: f64,
    /// Where this ring stops, clamped to its pool.
    pub max_radius: f64,
}

#[derive(Debug, Clone, Copy)]
struct TrackedBody {
    level: WaterLevel,
    x: f64,
    z: f64,
    last_wake: f64,
    wake_distance: f64,
    wake_x: f64,
    wake_z: f64,
}

/// `LiquidInteractionSystem`.
#[derive(Debug, Default)]
pub struct LiquidInteractionSystem {
    rings: Vec<Ring>,
    bodies: HashMap<String, TrackedBody>,
    next_ring: usize,
    last_spawned: Option<usize>,
    last_spawned_kind: Option<LiquidKind>,
    projectiles: HashMap<String, bool>,
    sounds: Vec<LiquidCrossingSound>,
    /// The crowns' spray; none until [`LiquidInteractionSystem::enable_crowns`].
    droplets: Option<SplashDroplets>,
    /// Splash counter, the seed of each crown's spread.
    splash_seed: u32,
    reentry_cooldown: f64,
}

impl LiquidInteractionSystem {
    /// `enableCrowns`: splashes throw spray (from `r_water_quality` 2 up).
    pub fn enable_crowns(&mut self) {
        self.droplets.get_or_insert_with(SplashDroplets::default);
    }

    /// `disableCrowns`: back to plain ripples.
    pub fn disable_crowns(&mut self) {
        self.droplets = None;
        self.reentry_cooldown = 0.0;
    }

    /// `crownsEnabled`.
    #[must_use]
    pub fn crowns_enabled(&self) -> bool {
        self.droplets.is_some()
    }

    /// The spray to draw this frame, none without crowns.
    #[must_use]
    pub fn droplets(&self) -> Option<&SplashDroplets> {
        self.droplets.as_ref()
    }

    /// `notify`: a body's water level this frame; only a change splashes,
    /// and moving through the surface leaves a wake.
    #[allow(clippy::too_many_arguments)]
    pub fn notify(
        &mut self,
        entity_id: &str,
        level: WaterLevel,
        kind: LiquidKind,
        position: DVec3,
        vertical_speed: f64,
        local: bool,
        now: f64,
        collision: Option<PointContents<'_>>,
        mut ripples: Option<&mut dyn WaterRipples>,
    ) {
        // A body's first report is a baseline, not a crossing.
        let Some(tracked) = self.bodies.get_mut(entity_id) else {
            self.bodies.insert(
                entity_id.to_owned(),
                TrackedBody {
                    level,
                    x: position.x,
                    z: position.z,
                    last_wake: now,
                    wake_distance: 0.0,
                    wake_x: 0.0,
                    wake_z: 0.0,
                },
            );
            return;
        };
        let previous = tracked.level;
        let (mx, mz) = (position.x - tracked.x, position.z - tracked.z);
        let distance = js_hypot(&[mx, mz]);
        tracked.level = level;
        tracked.x = position.x;
        tracked.z = position.z;

        // A wake for continuous surface movement; not for teleports,
        // stationary bodies or swimmers under the surface.
        if previous > 0 && level > 0 && level < 3 && distance < 96.0 {
            tracked.wake_distance += distance;
            tracked.wake_x += mx;
            tracked.wake_z += mz;
            let elapsed = now - tracked.last_wake;
            if elapsed >= 0.12 {
                let speed = tracked.wake_distance / elapsed;
                if js_hypot(&[tracked.wake_x, tracked.wake_z]) > 0.5 && speed > 20.0 {
                    let swimming = level == 2;
                    let strength = if swimming {
                        0.5f64.min(speed / 500.0)
                    } else {
                        0.28f64.min(speed / 900.0)
                    };
                    let radius = if swimming {
                        88f64.min(48.0 + speed * 0.12)
                    } else {
                        52f64.min(24.0 + speed * 0.08)
                    };
                    if let Some(ripples) = ripples.as_deref_mut() {
                        ripples.add(position, strength, radius, Some((tracked.wake_x, tracked.wake_z)));
                    }
                }
                tracked.last_wake = now;
                tracked.wake_distance = 0.0;
                tracked.wake_x = 0.0;
                tracked.wake_z = 0.0;
            }
        } else {
            tracked.last_wake = now;
            tracked.wake_distance = 0.0;
            tracked.wake_x = 0.0;
            tracked.wake_z = 0.0;
        }

        if previous == level {
            return;
        }
        let entered = previous == 0 && level > 0;
        let exited = previous > 0 && level == 0;
        if !entered && !exited {
            return;
        }
        // The liquid from the surface itself: only the level is replicated.
        let surface = surface_near(collision, position, kind);
        let splash_kind = surface.map_or(kind, |s| s.1);
        let volume = splash_volume(vertical_speed);
        if volume > 0.0 {
            self.sounds.push(LiquidCrossingSound {
                path: if entered {
                    enter_sound(splash_kind)
                } else {
                    exit_sound(splash_kind)
                },
                volume,
                position,
                local,
            });
        }
        // A ring only for a body that broke the surface.
        if volume > 0.0
            && let Some((y, _)) = surface
        {
            let strength = volume * if entered { 1.0 } else { 0.55 };
            self.spawn_ring(position, y, splash_kind, strength, collision, ripples);
        }
    }

    /// `notifyProjectile`: splashes when a projectile enters a liquid,
    /// which is non-solid, so no impact ever fires.
    pub fn notify_projectile(
        &mut self,
        id: &str,
        position: DVec3,
        speed: f64,
        collision: Option<PointContents<'_>>,
        ripples: Option<&mut dyn WaterRipples>,
    ) {
        let Some(contents) = collision else { return };
        let wet = contents(position) & MASK_LIQUID != 0;
        let previous = self.projectiles.insert(id.to_owned(), wet);
        // First sighting is a baseline; leaving is not an impact.
        if previous.is_none() || previous == Some(wet) || !wet {
            return;
        }
        let strength = 1f64.min(0.35 + speed / 3000.0);
        let surface = surface_near(collision, position, LiquidKind::Water);
        self.sounds.push(LiquidCrossingSound {
            path: HIT_WATER,
            volume: strength,
            position,
            local: false,
        });
        if let Some((y, kind)) = surface {
            self.spawn_ring(position, y, kind, strength, collision, ripples);
        }
    }

    /// `forgetProjectile`.
    pub fn forget_projectile(&mut self, id: &str) {
        self.projectiles.remove(id);
    }

    /// The projectiles being tracked.
    pub fn projectile_ids(&self) -> impl Iterator<Item = &str> {
        self.projectiles.keys().map(String::as_str)
    }

    /// `forget`: the body's next report is a baseline again.
    pub fn forget(&mut self, entity_id: &str) {
        self.bodies.remove(entity_id);
    }

    /// The bodies being tracked.
    pub fn body_ids(&self) -> impl Iterator<Item = &str> {
        self.bodies.keys().map(String::as_str)
    }

    /// The crossings to play since the last call.
    pub fn take_sounds(&mut self) -> Vec<LiquidCrossingSound> {
        std::mem::take(&mut self.sounds)
    }

    fn spawn_ring(
        &mut self,
        position: DVec3,
        surface_y: f64,
        kind: LiquidKind,
        strength: f64,
        collision: Option<PointContents<'_>>,
        ripples: Option<&mut dyn WaterRipples>,
    ) {
        // A ripple belongs to the surface, not to the swimmer.
        let at = DVec3::new(position.x, surface_y, position.z);
        // Water only: slime and lava are thick and throw no spray.
        if kind == LiquidKind::Water
            && let Some(droplets) = &mut self.droplets
        {
            self.splash_seed += 1;
            droplets.spawn(
                DropletSpawn {
                    x: at.x,
                    z: at.z,
                    surface_y,
                    strength,
                },
                f64::from(self.splash_seed),
            );
        }
        self.place_ring(at, kind, strength, 48.0 + 64.0 * strength.min(1.0), collision, ripples);
    }

    /// `placeRing`: water and slime ripple on the surface; lava, and pools
    /// the water system does not draw, keep the disc.
    fn place_ring(
        &mut self,
        at: DVec3,
        kind: LiquidKind,
        strength: f64,
        radius: f64,
        collision: Option<PointContents<'_>>,
        ripples: Option<&mut dyn WaterRipples>,
    ) {
        if kind != LiquidKind::Lava
            && let Some(ripples) = ripples
            && ripples.add(at, strength, radius, None)
        {
            return;
        }
        let index = self.acquire_ring();
        let max_radius = edge_limited_radius(collision, at);
        let ring = &mut self.rings[index];
        ring.position = DVec3::new(at.x, at.y + RING_SURFACE_LIFT, at.z);
        ring.kind = kind;
        ring.alpha = 0.55 * strength;
        ring.max_radius = max_radius;
        ring.scale = RING_START_RADIUS;
        ring.age = 0.0;
        self.last_spawned = Some(index);
        self.last_spawned_kind = Some(kind);
    }

    fn acquire_ring(&mut self) -> usize {
        if let Some(free) = self.rings.iter().position(|r| r.age >= RING_LIFETIME) {
            return free;
        }
        if self.rings.len() < RING_POOL_SIZE {
            self.rings.push(Ring {
                position: DVec3::ZERO,
                kind: LiquidKind::Water,
                alpha: 0.0,
                scale: RING_START_RADIUS,
                age: RING_LIFETIME,
                max_radius: RING_END_RADIUS,
            });
            return self.rings.len() - 1;
        }
        let recycled = self.next_ring % self.rings.len();
        self.next_ring += 1;
        recycled
    }

    /// The frame callback: rings expand fast and settle, and fade; the
    /// crown flies, and the harder landings ripple where they fall.
    pub fn update(
        &mut self,
        dt: f64,
        collision: Option<PointContents<'_>>,
        mut ripples: Option<&mut dyn WaterRipples>,
    ) {
        let dt = dt.min(0.1);
        if let Some(droplets) = &mut self.droplets {
            let reentries = droplets.update(dt);
            self.reentry_cooldown = (self.reentry_cooldown - dt).max(0.0);
            for hit in reentries {
                if hit.speed < REENTRY_RING_SPEED || self.reentry_cooldown > 0.0 {
                    continue;
                }
                self.place_ring(
                    DVec3::new(hit.x, hit.y, hit.z),
                    LiquidKind::Water,
                    REENTRY_RING_STRENGTH,
                    24.0,
                    collision,
                    ripples.as_mut().map(|r| &mut **r as &mut dyn WaterRipples),
                );
                self.reentry_cooldown = 0.12;
            }
        }
        for ring in &mut self.rings {
            if ring.age >= RING_LIFETIME {
                continue;
            }
            ring.age += dt;
            let t = (ring.age / RING_LIFETIME).min(1.0);
            let eased = 1.0 - (1.0 - t) * (1.0 - t);
            ring.scale = RING_START_RADIUS + (ring.max_radius - RING_START_RADIUS) * eased;
            ring.alpha = 0f64.max(ring.alpha * (1.0 - dt / RING_LIFETIME) - dt * 0.15);
        }
    }

    /// The rings alive this frame.
    pub fn rings(&self) -> impl Iterator<Item = &Ring> {
        self.rings.iter().filter(|r| r.age < RING_LIFETIME)
    }

    /// `activeRings`.
    #[must_use]
    pub fn active_rings(&self) -> usize {
        self.rings().count()
    }

    /// `lastRingY`.
    #[must_use]
    pub fn last_ring_y(&self) -> Option<f64> {
        self.last_spawned.map(|i| self.rings[i].position.y)
    }

    /// `lastRingKind`.
    #[must_use]
    pub fn last_ring_kind(&self) -> Option<LiquidKind> {
        self.last_spawned_kind
    }

    /// `lastRingMaxRadius`.
    #[must_use]
    pub fn last_ring_max_radius(&self) -> Option<f64> {
        self.last_spawned.map(|i| self.rings[i].max_radius)
    }
}

/// `surfaceNear`: the first height where liquid begins, walking down a
/// column through the body, with its kind; none when no surface is near.
/// Without a collision world, the body's height and the caller's guess.
fn surface_near(
    collision: Option<PointContents<'_>>,
    position: DVec3,
    fallback: LiquidKind,
) -> Option<(f64, LiquidKind)> {
    let Some(contents) = collision else {
        return Some((position.y, fallback));
    };
    for i in 0..=SURFACE_SEARCH_STEPS {
        let y = position.y + SURFACE_SEARCH_ABOVE - f64::from(i) * SURFACE_SEARCH_STEP;
        if let Some(kind) = liquid_kind_at(contents(DVec3::new(position.x, y, position.z)) & MASK_LIQUID) {
            return Some((y, kind));
        }
    }
    None
}

/// `edgeLimitedRadius`: how far a ring may spread before it runs out of
/// pool, walking out through the liquid in eight directions.
fn edge_limited_radius(collision: Option<PointContents<'_>>, position: DVec3) -> f64 {
    let Some(contents) = collision else {
        return RING_END_RADIUS;
    };
    let mut limit = RING_END_RADIUS;
    let step = RING_END_RADIUS / f64::from(EDGE_PROBE_STEPS);
    for d in 0..EDGE_PROBE_DIRECTIONS {
        let angle = (f64::from(d) / f64::from(EDGE_PROBE_DIRECTIONS)) * std::f64::consts::PI * 2.0;
        let (dx, dz) = (js_cos(angle), js_sin(angle));
        for s in 1..=EDGE_PROBE_STEPS {
            let distance = step * f64::from(s);
            // Just below the surface: exactly at it is the boundary case.
            let probe = DVec3::new(position.x + dx * distance, position.y - 2.0, position.z + dz * distance);
            if contents(probe) & MASK_LIQUID == 0 {
                limit = limit.min(RING_START_RADIUS.max(distance - step));
                break;
            }
        }
    }
    limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::shared::collision::collision_world::contents;

    /// Fast enough that `splash_volume` is comfortably above its gate.
    const DIVING: f64 = -400.0;
    const W: LiquidKind = LiquidKind::Water;

    fn at(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// Position, strength, radius and direction.
    type Impulse = (DVec3, f64, f64, Option<(f64, f64)>);

    /// `waterDisturbancesFor(scene)` with a `resolveSurface`.
    #[derive(Default)]
    struct Waves {
        impulses: Vec<Impulse>,
        surface: Option<f64>,
    }

    impl WaterRipples for Waves {
        fn add(&mut self, position: DVec3, strength: f64, radius: f64, direction: Option<(f64, f64)>) -> bool {
            let Some(y) = self.surface else { return false };
            self.impulses.push((
                DVec3::new(position.x, y, position.z),
                strength.min(1.0),
                radius.min(128.0),
                direction,
            ));
            true
        }
    }

    fn pool(p: DVec3) -> u32 {
        if p.y <= 56.0 && p.x.abs() <= 200.0 && p.z.abs() <= 200.0 {
            contents::WATER
        } else {
            0
        }
    }

    fn lava_pit(p: DVec3) -> u32 {
        if p.y > 56.0 {
            0
        } else if p.x > 500.0 {
            contents::WATER
        } else {
            contents::LAVA
        }
    }

    #[test]
    fn snaps_a_splash_to_the_drawn_surface_without_a_disc() {
        let mut system = LiquidInteractionSystem::default();
        let mut waves = Waves {
            surface: Some(56.0),
            ..Waves::default()
        };
        let water = |p: DVec3| if p.y < 56.0 { contents::WATER } else { 0 };
        for level in [0, 2] {
            system.notify(
                "player",
                level,
                W,
                at(0.0, 72.0, 0.0),
                DIVING,
                true,
                0.0,
                Some(&water),
                Some(&mut waves),
            );
        }
        assert_eq!(waves.impulses.len(), 1);
        assert_eq!(waves.impulses[0].0.y, 56.0);
        assert!((waves.impulses[0].2 - (48.0 + 64.0 * splash_volume(DIVING))).abs() < 1e-9);
        assert_eq!(system.active_rings(), 0);
        // No drawn pool takes it: the disc stays.
        let mut none = Waves::default();
        for level in [0, 2] {
            system.notify(
                "other",
                level,
                W,
                at(0.0, 0.0, 0.0),
                DIVING,
                false,
                0.0,
                None,
                Some(&mut none),
            );
        }
        assert_eq!(system.active_rings(), 1);
    }

    #[test]
    fn swimming_wakes_are_broader_and_submerged_motion_is_quiet() {
        let mut system = LiquidInteractionSystem::default();
        let mut waves = Waves {
            surface: Some(56.0),
            ..Waves::default()
        };
        let mut clock = 0.0;
        for level in [1, 2, 3] {
            let id = level.to_string();
            system.notify(
                &id,
                level,
                W,
                at(0.0, 0.0, 0.0),
                0.0,
                true,
                clock,
                None,
                Some(&mut waves),
            );
            clock += 0.15;
            system.notify(
                &id,
                level,
                W,
                at(30.0, 0.0, 0.0),
                0.0,
                true,
                clock,
                None,
                Some(&mut waves),
            );
        }
        assert_eq!(waves.impulses.len(), 2);
        assert!(waves.impulses[1].2 > waves.impulses[0].2);
        assert!(waves.impulses[1].1 > waves.impulses[0].1);
        // Jitter that cancels out leaves nothing.
        let mut quiet = Waves {
            surface: Some(56.0),
            ..Waves::default()
        };
        for (t, x) in [(0.0, 0.0), (0.06, 4.0), (0.15, 0.0)] {
            system.notify("jitter", 2, W, at(x, 0.0, 0.0), 0.0, true, t, None, Some(&mut quiet));
        }
        assert!(quiet.impulses.is_empty());
    }

    #[test]
    fn wakes_are_comparable_at_any_frame_rate() {
        let mut system = LiquidInteractionSystem::default();
        let mut waves = Waves {
            surface: Some(56.0),
            ..Waves::default()
        };
        let mut clock = 0.0;
        for fps in [60u32, 1000] {
            let count = waves.impulses.len();
            let start = clock;
            let id = fps.to_string();
            system.notify(&id, 2, W, at(0.0, 0.0, 0.0), 0.0, true, clock, None, Some(&mut waves));
            for frame in 1..=fps / 2 {
                clock = start + f64::from(frame) / f64::from(fps);
                let x = f64::from(frame) * 200.0 / f64::from(fps);
                system.notify(&id, 2, W, at(x, 0.0, 0.0), 0.0, true, clock, None, Some(&mut waves));
            }
            assert!(waves.impulses.len() - count >= 3);
            let last = waves.impulses.last().unwrap();
            assert!((last.1 - 0.4).abs() < 0.01);
            let (dx, dz) = last.3.unwrap();
            assert!((dx / js_hypot(&[dx, dz]) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn suppresses_underwater_stationary_teleport_and_outside_pool() {
        let mut system = LiquidInteractionSystem::default();
        let mut waves = Waves {
            surface: Some(56.0),
            ..Waves::default()
        };
        for (level, distance) in [(3, 30.0), (2, 0.0), (2, 200.0)] {
            let id = format!("{level}:{distance}");
            system.notify(
                &id,
                level,
                W,
                at(0.0, 0.0, 0.0),
                0.0,
                false,
                0.0,
                None,
                Some(&mut waves),
            );
            system.notify(
                &id,
                level,
                W,
                at(distance, 0.0, 0.0),
                0.0,
                false,
                0.15,
                None,
                Some(&mut waves),
            );
        }
        assert!(waves.impulses.is_empty());
    }

    #[test]
    fn transitions() {
        let mut system = LiquidInteractionSystem::default();
        let o = at(0.0, 0.0, 0.0);
        let notify = |s: &mut LiquidInteractionSystem, id: &str, level, speed| {
            s.notify(id, level, W, o, speed, false, 0.0, None, None);
        };
        // Once on entering, not every frame after.
        notify(&mut system, "p1", 0, DIVING);
        notify(&mut system, "p1", 2, DIVING);
        for _ in 0..30 {
            notify(&mut system, "p1", 2, DIVING);
        }
        assert_eq!(system.active_rings(), 1);
        // Again on leaving; depth changes under the surface are nothing.
        notify(&mut system, "p1", 3, DIVING);
        notify(&mut system, "p1", 1, DIVING);
        assert_eq!(system.active_rings(), 1);
        notify(&mut system, "p1", 0, DIVING);
        assert_eq!(system.active_rings(), 2);
        // Starting in the water is a baseline; a forgotten id starts over.
        notify(&mut system, "p2", 3, DIVING);
        assert_eq!(system.active_rings(), 2);
        system.forget("p1");
        notify(&mut system, "p1", 2, DIVING);
        assert_eq!(system.active_rings(), 2);
        // A gentle crossing leaves no ring and no sound.
        let sounds = system.take_sounds().len();
        notify(&mut system, "p3", 0, 0.0);
        notify(&mut system, "p3", 2, 5.0);
        assert_eq!(system.active_rings(), 2);
        assert_eq!(sounds, 2);
        assert!(system.take_sounds().is_empty());
    }

    #[test]
    fn rings_belong_to_the_surface() {
        let mut system = LiquidInteractionSystem::default();
        let pool: PointContents<'_> = &pool;
        system.notify("p1", 2, W, at(0.0, 40.0, 0.0), DIVING, true, 0.0, Some(pool), None);
        system.notify("p1", 0, W, at(0.0, 80.0, 0.0), DIVING, true, 0.0, Some(pool), None);
        assert_eq!(system.active_rings(), 1);
        let y = system.last_ring_y().unwrap();
        assert!(y > 50.0 && y < 60.0);
        // Nowhere near a surface: nothing.
        system.notify(
            "far",
            2,
            W,
            at(5000.0, 5000.0, 5000.0),
            DIVING,
            true,
            0.0,
            Some(pool),
            None,
        );
        system.notify(
            "far",
            0,
            W,
            at(5000.0, 5000.0, 5000.0),
            DIVING,
            true,
            0.0,
            Some(pool),
            None,
        );
        assert_eq!(system.active_rings(), 1);
        // Near the bank the ring stops short; in open water it does not.
        system.notify("bank", 0, W, at(190.0, 50.0, 0.0), DIVING, true, 0.0, Some(pool), None);
        system.notify("bank", 2, W, at(190.0, 50.0, 0.0), DIVING, true, 0.0, Some(pool), None);
        assert!(system.last_ring_max_radius().unwrap() < RING_END_RADIUS);
        system.notify("open", 0, W, at(0.0, 50.0, 0.0), DIVING, true, 0.0, Some(pool), None);
        system.notify("open", 2, W, at(0.0, 50.0, 0.0), DIVING, true, 0.0, Some(pool), None);
        assert_eq!(system.last_ring_max_radius(), Some(RING_END_RADIUS));
    }

    #[test]
    fn the_liquid_comes_from_the_surface() {
        let mut system = LiquidInteractionSystem::default();
        let pit: PointContents<'_> = &lava_pit;
        system.notify("p1", 0, W, at(0.0, 50.0, 0.0), DIVING, false, 0.0, Some(pit), None);
        system.notify("p1", 2, W, at(0.0, 50.0, 0.0), DIVING, false, 0.0, Some(pit), None);
        assert_eq!(system.last_ring_kind(), Some(LiquidKind::Lava));
        system.notify("p2", 0, W, at(600.0, 50.0, 0.0), DIVING, false, 0.0, Some(pit), None);
        system.notify("p2", 2, W, at(600.0, 50.0, 0.0), DIVING, false, 0.0, Some(pit), None);
        assert_eq!(system.last_ring_kind(), Some(LiquidKind::Water));
        // No map: the caller's guess.
        let slime = LiquidKind::Slime;
        system.notify("p3", 0, slime, at(0.0, 50.0, 0.0), DIVING, false, 0.0, None, None);
        system.notify("p3", 2, slime, at(0.0, 50.0, 0.0), DIVING, false, 0.0, None, None);
        assert_eq!(system.last_ring_kind(), Some(slime));
        // A rocket into lava: a lava splash, even with the water system up.
        let mut waves = Waves {
            surface: Some(56.0),
            ..Waves::default()
        };
        system.notify_projectile("proj_1", at(0.0, 80.0, 0.0), 1400.0, Some(pit), Some(&mut waves));
        system.notify_projectile("proj_1", at(0.0, 40.0, 0.0), 1400.0, Some(pit), Some(&mut waves));
        assert_eq!(system.last_ring_kind(), Some(LiquidKind::Lava));
        assert!(waves.impulses.is_empty());
    }

    #[test]
    fn crowns_spray_water_not_lava_and_ring_where_they_land() {
        let mut system = LiquidInteractionSystem::default();
        system.enable_crowns();
        let pit: PointContents<'_> = &lava_pit;
        system.notify("p1", 0, W, at(0.0, 50.0, 0.0), DIVING, false, 0.0, Some(pit), None);
        system.notify("p1", 2, W, at(0.0, 50.0, 0.0), DIVING, false, 0.0, Some(pit), None);
        assert_eq!(system.droplets().unwrap().alive_count(), 0);
        let mut waves = Waves {
            surface: Some(56.0),
            ..Waves::default()
        };
        system.notify(
            "p2",
            0,
            W,
            at(600.0, 50.0, 0.0),
            DIVING,
            false,
            0.0,
            Some(pit),
            Some(&mut waves),
        );
        system.notify(
            "p2",
            2,
            W,
            at(600.0, 50.0, 0.0),
            DIVING,
            false,
            0.0,
            Some(pit),
            Some(&mut waves),
        );
        assert!(system.droplets().unwrap().alive_count() > 0);
        assert_eq!(waves.impulses.len(), 1);
        for _ in 0..120 {
            system.update(1.0 / 60.0, Some(pit), Some(&mut waves));
        }
        assert_eq!(system.droplets().unwrap().alive_count(), 0);
        // Re-entries ring faintly, rate-limited.
        assert!(waves.impulses.len() > 1);
        assert!(
            waves.impulses[1..]
                .iter()
                .all(|i| i.1 == REENTRY_RING_STRENGTH && i.2 == 24.0)
        );
    }

    #[test]
    fn projectiles_splash_once_on_the_way_in() {
        let mut system = LiquidInteractionSystem::default();
        let pool: PointContents<'_> = &pool;
        let shot = |s: &mut LiquidInteractionSystem, id: &str, y| {
            s.notify_projectile(id, at(0.0, y, 0.0), 1400.0, Some(pool), None);
        };
        shot(&mut system, "proj_1", 80.0);
        shot(&mut system, "proj_1", 40.0);
        for _ in 0..20 {
            shot(&mut system, "proj_1", 20.0);
        }
        shot(&mut system, "proj_1", 80.0);
        assert_eq!(system.active_rings(), 1);
        // Spawned under, or never wet: nothing.
        shot(&mut system, "proj_2", 40.0);
        shot(&mut system, "proj_3", 400.0);
        shot(&mut system, "proj_3", 380.0);
        assert_eq!(system.active_rings(), 1);
        // A recycled id starts clean.
        system.forget_projectile("proj_1");
        shot(&mut system, "proj_1", 40.0);
        assert_eq!(system.active_rings(), 1);
        // Rings fade out.
        for _ in 0..6 {
            system.update(0.1, None, None);
        }
        assert_eq!(system.active_rings(), 0);
    }
}
