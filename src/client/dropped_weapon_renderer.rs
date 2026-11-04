//! wash-fps `client/DroppedWeaponRenderer.ts`: weapons on the floor, from
//! the snapshot's `item_` entities.
//!
//! - Lifecycle: an `item_` entity in the snapshot is a weapon on the floor;
//!   gone from it, the weapon was picked up or expired.
//! - Motion: a weapon seen soon after its `weapon_drop` event replays the
//!   server's throw (`stepThrownItem`) through the same collision, spinning,
//!   rolling and tumbling while it flies, and comes to rest on its side. One
//!   seen without its throw (joined late) eases to the server's position.
//! - A demo (`useServerTransforms`) puts it where the server says, unturned.
//!
//! What this keeps is where each weapon is and how it is turned; the
//! renderer draws them with the world weapon models. Not ported: the PVS
//! check (the depth test hides what is behind walls), parked models (there
//! are no per-drop models to keep), the far-detail cut and the swap prompt.

use std::collections::HashMap;

use shagger::shared::collision::collision_world::CollisionWorld;
use shagger::shared::server::simulation::types::Vec3;

use crate::shared::items::item_throw::{DROPPED_WEAPON_BOUNCE, ITEM_STEP_SECONDS, ThrownItem, step_thrown_item};

/// Most a frame advances a throw: a stall catches up without a burst of steps.
const MAX_THROW_CATCHUP_SECONDS: f64 = 0.25;
/// How fast a landed weapon's tumble eases onto its side (per second).
const SETTLE_LEVEL_RATE: f64 = 6.0;
/// Vertical speed either side of a step that makes it a bounce (units/s).
const BOUNCE_SPEED: f64 = 20.0;
/// Spin kept through a bounce off the floor.
const BOUNCE_SPIN_KEPT: f64 = 0.5;
/// Spin kept per throw step while sliding along the floor.
const SLIDE_SPIN_KEPT: f64 = 0.96;
/// Roll per unit slid, so a sliding gun rolls with its speed (radians per unit).
const SLIDE_ROLL_PER_UNIT: f64 = 0.02;
/// `TOSS_FRESH_MS`: how long after its `weapon_drop` a weapon first seen is
/// still in the air.
const TOSS_FRESH_MS: f64 = 500.0;
/// A stashed throw whose entity never came is forgotten after this long.
const THROW_FORGET_MS: f64 = 5000.0;

/// `DroppedWeaponInfo`: an `item_` entity as the snapshot has it, Babylon axes.
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedWeaponInfo {
    pub position: [f64; 3],
    pub weapon_id: Option<String>,
    pub settled: bool,
}

/// `DropThrow`: where the weapon leaves the hand, and how fast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropThrow {
    pub start: [f64; 3],
    pub velocity: [f64; 3],
}

/// A small generator for the cosmetic spins (`Math.random` in the browser).
#[derive(Debug, Clone, Copy)]
struct Spins(u64);

impl Spins {
    fn new(seed: &str) -> Self {
        // FNV-1a, so each drop turns its own way.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in seed.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        Self(h | 1)
    }

    /// In 0 to 1.
    fn next(&mut self) -> f64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn sign(&mut self) -> f64 {
        if self.next() < 0.5 { -1.0 } else { 1.0 }
    }
}

/// `DroppedWeaponVisual`: one weapon on the floor.
#[derive(Debug, Clone)]
pub struct DroppedWeaponVisual {
    pub weapon_id: String,
    /// Where it is drawn, Babylon axes.
    pub position: [f64; 3],
    /// `mesh.rotation`: roll about the barrel (x), yaw (y), tumble (z).
    pub rotation: [f64; 3],
    server_position: [f64; 3],
    server_settled: bool,
    use_server_transforms: bool,
    throw: Option<ThrownItem>,
    throw_previous: [f64; 3],
    throw_elapsed: f64,
    spin: f64,
    roll_rate: f64,
    tumble_rate: f64,
    yaw: f64,
    roll: f64,
    tumble: f64,
    spins: Spins,
}

fn v3([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn lerp(from: [f64; 3], to: [f64; 3], t: f64) -> [f64; 3] {
    [
        from[0] + (to[0] - from[0]) * t,
        from[1] + (to[1] - from[1]) * t,
        from[2] + (to[2] - from[2]) * t,
    ]
}

impl DroppedWeaponVisual {
    fn new(
        entity_id: &str,
        weapon_id: &str,
        position: [f64; 3],
        thrown: Option<DropThrow>,
        use_server_transforms: bool,
    ) -> Self {
        let mut spins = Spins::new(entity_id);
        let spin = spins.sign() * (5.0 + spins.next() * 4.0);
        let roll_rate = spins.sign() * (6.0 + spins.next() * 5.0);
        let tumble_rate = spins.sign() * (4.0 + spins.next() * 4.0);
        let yaw = spins.next() * std::f64::consts::TAU;
        // A demo follows the server alone: no throw to replay.
        let throw = thrown.filter(|_| !use_server_transforms).map(|t| ThrownItem {
            position: v3(t.start),
            velocity: v3(t.velocity),
            settled: false,
            bounce: Some(DROPPED_WEAPON_BOUNCE),
        });
        let start = match (&throw, thrown) {
            (Some(_), Some(t)) => t.start,
            _ => position,
        };
        Self {
            weapon_id: weapon_id.to_owned(),
            position: start,
            rotation: [0.0; 3],
            server_position: position,
            server_settled: false,
            use_server_transforms,
            throw,
            throw_previous: start,
            throw_elapsed: 0.0,
            spin,
            roll_rate,
            tumble_rate,
            yaw,
            roll: 0.0,
            tumble: 0.0,
            spins,
        }
    }

    /// Whether it has come to rest: its throw settled, or the server said so.
    #[must_use]
    pub fn resting(&self) -> bool {
        self.throw.map_or(self.server_settled, |t| t.settled)
    }

    fn set_server_position(&mut self, at: [f64; 3], settled: bool) {
        self.server_position = at;
        self.server_settled = settled;
        if self.use_server_transforms {
            self.position = at;
        }
    }

    /// `reactToStep`: a bounce knocks spin out of it and jolts it a little;
    /// sliding on the floor rolls it with its speed while the spin dies.
    fn react_to_step(&mut self, falling_before: f64) {
        let Some(throw) = self.throw else { return };
        let velocity = throw.velocity;
        if falling_before < -BOUNCE_SPEED && velocity.y > BOUNCE_SPEED {
            self.spin *= BOUNCE_SPIN_KEPT;
            self.tumble_rate = self.tumble_rate * BOUNCE_SPIN_KEPT + (self.spins.next() - 0.5) * 4.0;
            self.roll_rate = self.roll_rate * BOUNCE_SPIN_KEPT + (self.spins.next() - 0.5) * 4.0;
        } else if velocity.y == 0.0 {
            let sliding = velocity.x.hypot(velocity.z);
            self.spin *= SLIDE_SPIN_KEPT;
            self.tumble_rate *= SLIDE_SPIN_KEPT;
            let sign = if self.roll_rate < 0.0 { -1.0 } else { 1.0 };
            self.roll_rate = self.roll_rate * SLIDE_SPIN_KEPT + sign * sliding * SLIDE_ROLL_PER_UNIT;
        }
    }

    /// `update`: step the throw, place and turn the weapon, `dt` seconds on.
    fn update(&mut self, dt: f64, collision: Option<&mut impl CollisionWorld>) {
        let dt = dt.clamp(0.0, MAX_THROW_CATCHUP_SECONDS);
        let mut between = 0.0;
        if let Some(collision) = collision
            && self.throw.is_some_and(|t| !t.settled)
        {
            self.throw_elapsed += dt;
            while self.throw_elapsed >= ITEM_STEP_SECONDS && self.throw.is_some_and(|t| !t.settled) {
                let Some(throw) = self.throw.as_mut() else { break };
                self.throw_previous = arr(throw.position);
                let falling = throw.velocity.y;
                step_thrown_item(throw, collision, ITEM_STEP_SECONDS);
                self.react_to_step(falling);
                self.throw_elapsed -= ITEM_STEP_SECONDS;
            }
            between = if self.throw.is_some_and(|t| t.settled) {
                1.0
            } else {
                self.throw_elapsed / ITEM_STEP_SECONDS
            };
        }
        if self.use_server_transforms {
            return;
        }
        match self.throw {
            // The server's path, drawn between its steps.
            Some(t) if !(t.settled && self.server_settled) => {
                self.position = lerp(self.throw_previous, arr(t.position), between);
            }
            // Both at rest: the start crossed the network rounded, so settle
            // gently onto the server's spot, where the pickup is.
            Some(_) => self.position = lerp(self.position, self.server_position, (dt * 4.0).min(1.0)),
            // Only the server's word: follow it without its snapshot rate's jumps.
            None => self.position = lerp(self.position, self.server_position, (dt * 20.0).min(1.0)),
        }
        // Spinning while it flies; at rest it lies flat on the nearer side,
        // at its last heading.
        if self.resting() {
            let ease = (dt * SETTLE_LEVEL_RATE).min(1.0);
            let pi = std::f64::consts::PI;
            let side = ((self.roll - pi / 2.0) / pi).round() * pi + pi / 2.0;
            let flat = (self.tumble / pi).round() * pi;
            self.roll += (side - self.roll) * ease;
            self.tumble += (flat - self.tumble) * ease;
        } else {
            self.yaw += self.spin * dt;
            self.roll += self.roll_rate * dt;
            self.tumble += self.tumble_rate * dt;
        }
        self.rotation = [self.roll, self.yaw, self.tumble];
    }
}

/// `DroppedWeaponRenderer`.
#[derive(Debug, Clone, Default)]
pub struct DroppedWeaponRenderer {
    weapons: HashMap<String, DroppedWeaponVisual>,
    /// Throws from `weapon_drop` events, by entity, with when they came (ms).
    pending_throws: HashMap<String, (DropThrow, f64)>,
    /// A demo: the server's positions as they are.
    pub use_server_transforms: bool,
}

impl DroppedWeaponRenderer {
    #[must_use]
    pub fn new(use_server_transforms: bool) -> Self {
        Self {
            use_server_transforms,
            ..Self::default()
        }
    }

    /// `stashThrow`: the throw a `weapon_drop` starts, replayed when its
    /// entity shows up.
    pub fn stash_throw(&mut self, entity_id: &str, thrown: DropThrow, now_ms: f64) {
        self.pending_throws.insert(entity_id.to_owned(), (thrown, now_ms));
    }

    /// `syncServerEntities`: spawn the new, move the known, drop the gone.
    pub fn sync_server_entities(&mut self, entities: &HashMap<String, DroppedWeaponInfo>, now_ms: f64) {
        self.weapons.retain(|id, _| entities.contains_key(id));
        self.pending_throws.retain(|_, (_, at)| now_ms - *at <= THROW_FORGET_MS);
        let mut ids: Vec<&String> = entities.keys().collect();
        ids.sort_unstable();
        for id in ids {
            let info = &entities[id];
            if let Some(existing) = self.weapons.get_mut(id) {
                existing.set_server_position(info.position, info.settled);
            } else if let Some(weapon_id) = &info.weapon_id {
                // A toss only while the drop is fresh: one first seen later
                // has landed, and thrown again from there it would hop.
                let thrown = self
                    .pending_throws
                    .remove(id)
                    .filter(|(_, at)| now_ms - at <= TOSS_FRESH_MS)
                    .map(|(t, _)| t);
                let mut visual =
                    DroppedWeaponVisual::new(id, weapon_id, info.position, thrown, self.use_server_transforms);
                visual.server_settled = info.settled;
                self.weapons.insert(id.clone(), visual);
            }
        }
    }

    /// `updateVisuals`: every weapon's frame.
    pub fn update(&mut self, dt: f64, mut collision: Option<&mut impl CollisionWorld>) {
        for visual in self.weapons.values_mut() {
            visual.update(dt, collision.as_deref_mut());
        }
    }

    /// The weapons on the floor, by entity id.
    pub fn visuals(&self) -> impl Iterator<Item = (&String, &DroppedWeaponVisual)> {
        self.weapons.iter()
    }

    /// `count`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.weapons.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.weapons.is_empty()
    }

    /// The map changed or the match ended: nothing on the floor.
    pub fn clear(&mut self) {
        self.weapons.clear();
        self.pending_throws.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::shared::bsp::brush_model_state::BrushModelState;
    use shagger::shared::bsp::lift_system::LiftSystem;
    use shagger::shared::collision::collision_world::{Aabb, StepSlideMoveResult, TraceResult};

    /// A floor at y = 0 and nothing else.
    struct Floor;

    impl CollisionWorld for Floor {
        fn lifts(&self) -> Option<&LiftSystem> {
            None
        }
        fn brush_models(&self) -> Option<&BrushModelState> {
            None
        }
        fn trace(&mut self, _start: Vec3, _end: Vec3, _bounds: Option<Aabb>, _mask: Option<i32>) -> TraceResult {
            TraceResult {
                fraction: 1.0,
                ..TraceResult::default()
            }
        }
        fn step_slide_move(&mut self, start: Vec3, velocity: Vec3, dt: f64, _bounds: Aabb) -> StepSlideMoveResult {
            let mut end = Vec3::new(
                start.x + velocity.x * dt,
                start.y + velocity.y * dt,
                start.z + velocity.z * dt,
            );
            let mut velocity = velocity;
            let grounded = end.y <= 4.0;
            if grounded {
                end.y = 4.0;
                velocity.y = velocity.y.max(0.0);
            }
            StepSlideMoveResult {
                end_pos: end,
                velocity,
                grounded,
            }
        }
        fn point_contents(&mut self, _point: Vec3) -> u32 {
            0
        }
        fn world_bounds(&self) -> Aabb {
            Aabb {
                min: Vec3::new(-1e4, -1e4, -1e4),
                max: Vec3::new(1e4, 1e4, 1e4),
            }
        }
    }

    fn info(at: [f64; 3], settled: bool) -> HashMap<String, DroppedWeaponInfo> {
        HashMap::from([(
            "item_1".to_owned(),
            DroppedWeaponInfo {
                position: at,
                weapon_id: Some("rlauncher".to_owned()),
                settled,
            },
        )])
    }

    #[test]
    fn a_fresh_drop_is_thrown_lands_and_lies_on_its_side() {
        let mut r = DroppedWeaponRenderer::new(false);
        r.stash_throw(
            "item_1",
            DropThrow {
                start: [0.0, 60.0, 0.0],
                velocity: [80.0, 120.0, 0.0],
            },
            0.0,
        );
        r.sync_server_entities(&info([0.0, 60.0, 0.0], false), 100.0);
        let (_, w) = r.visuals().next().unwrap();
        assert_eq!(w.position, [0.0, 60.0, 0.0]);
        let mut floor = Floor;
        for _ in 0..240 {
            r.update(1.0 / 60.0, Some(&mut floor));
        }
        let (_, w) = r.visuals().next().unwrap();
        assert!(w.resting());
        assert!(w.position[0] > 10.0, "thrown forward: {:?}", w.position);
        assert!((w.position[1] - 4.0).abs() < 1e-6);
        // On its side, flat: roll an odd multiple of pi/2, tumble of pi.
        let pi = std::f64::consts::PI;
        let side = (w.rotation[0] - pi / 2.0) / pi;
        assert!((side - side.round()).abs() < 1e-3, "roll {}", w.rotation[0]);
        let flat = w.rotation[2] / pi;
        assert!((flat - flat.round()).abs() < 1e-3, "tumble {}", w.rotation[2]);
    }

    #[test]
    fn a_late_weapon_follows_the_server_and_goes_with_its_entity() {
        let mut r = DroppedWeaponRenderer::new(false);
        // A throw seen long ago is not replayed.
        r.stash_throw(
            "item_1",
            DropThrow {
                start: [0.0, 60.0, 0.0],
                velocity: [80.0, 120.0, 0.0],
            },
            0.0,
        );
        r.sync_server_entities(&info([100.0, 4.0, 0.0], true), 2000.0);
        r.update(1.0 / 60.0, None::<&mut Floor>);
        let (_, w) = r.visuals().next().unwrap();
        assert_eq!(w.position, [100.0, 4.0, 0.0]);
        assert!(w.resting());
        r.sync_server_entities(&HashMap::new(), 2100.0);
        assert!(r.is_empty());
    }

    #[test]
    fn a_demo_puts_it_where_the_server_says() {
        let mut r = DroppedWeaponRenderer::new(true);
        r.sync_server_entities(&info([1.0, 2.0, 3.0], false), 0.0);
        r.sync_server_entities(&info([5.0, 6.0, 7.0], false), 50.0);
        r.update(1.0 / 60.0, None::<&mut Floor>);
        let (_, w) = r.visuals().next().unwrap();
        assert_eq!(w.position, [5.0, 6.0, 7.0]);
        assert_eq!(w.rotation, [0.0; 3]);
    }
}
