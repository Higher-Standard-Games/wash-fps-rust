//! wash-fps `client/DroppedWeaponRenderer.ts`: dropped weapons drawn where
//! the server has them.
//!
//! - Lifecycle: an `item_` entity in the snapshot is a weapon on the floor;
//!   gone from it, the weapon was picked up or expired.
//! - Motion: the server throws each drop as a Havok body and replicates its
//!   position (the centre of the gun's box) and rotation; drawn here between
//!   snapshots on the server's tick clock, so a paused demo holds still.
//! - A recording from before rotations were sent (demo v1 to v8) lies on its
//!   side.
//!
//! See SHAG's networking guide, "9b. Dropped Weapons". Positions and
//! quaternions stay in the game's (Babylon's) axes, as the renderer draws
//! them, so the server's quaternion is used as it is.
//!
//! What this keeps is where each weapon is and how it is turned; the
//! renderer draws them with the world weapon models. Not ported: the PVS
//! check (the map renderer does it), parked models (there are no per-drop
//! models to keep) and the far-detail cut. The swap prompt is
//! `hud::widgets::pickup_prompt`.

use std::collections::HashMap;

use shagger::rendering::glam::{DQuat, DVec3};

use crate::shared::items::item_throw::ITEM_STEP_SECONDS;
use crate::shared::items::weapon_boxes::{WeaponBox, weapon_box};

/// `DroppedWeaponInfo`: an `item_` entity as the snapshot has it, Babylon axes.
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedWeaponInfo {
    /// Centre of its box when it has a rotation, else where it is.
    pub position: [f64; 3],
    pub weapon_id: Option<String>,
    pub settled: bool,
    /// How it lies (x, y, z, w), from the server; `None` in recordings made
    /// before rotations were sent.
    pub rotation: Option<[f64; 4]>,
}

/// `TICK_RATE`: server ticks per second, what snapshot ticks count.
const TICK_RATE: f64 = 1.0 / ITEM_STEP_SECONDS;
/// `MAX_SAMPLES`: pose samples kept per weapon.
const MAX_SAMPLES: usize = 6;
/// `MAX_LAG_INTERVALS`: most the render tick trails the newest snapshot
/// before it catches up (snapshot intervals).
const MAX_LAG_INTERVALS: f64 = 3.0;
/// Most one frame advances the render tick (seconds).
const MAX_FRAME_SECONDS: f64 = 0.25;

/// `lyingRotation`: on its side (a quarter roll about the barrel), turned to
/// a heading of its own from a hash of the entity id.
fn lying_rotation(entity_id: &str) -> DQuat {
    // `(hash * 31 + charCodeAt(i)) | 0` over UTF-16 units.
    let hash = entity_id
        .encode_utf16()
        .fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)));
    // `(hash >>> 0) % 628`
    let yaw = f64::from(hash.cast_unsigned() % 628) / 100.0;
    // `RotationYawPitchRoll(yaw, 0, 0).multiply(RotationAxis(Right, PI / 2))`
    DQuat::from_rotation_y(yaw) * DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)
}

/// `PoseSample`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PoseSample {
    tick: f64,
    position: DVec3,
    rotation: Option<DQuat>,
}

/// `DroppedWeaponVisual`: one weapon on the floor.
#[derive(Debug, Clone)]
pub struct DroppedWeaponVisual {
    pub weapon_id: String,
    /// Where it is drawn this frame (`drawnPosition`): the centre of its box,
    /// Babylon axes. The pickup range and the PVS check measure from here.
    pub position: [f64; 3],
    /// How it is turned this frame (`drawnRotation`).
    pub rotation: DQuat,
    settled: bool,
    weapon_box: WeaponBox,
    /// Recent server poses, oldest first.
    samples: Vec<PoseSample>,
    lying: DQuat,
}

impl DroppedWeaponVisual {
    fn new(entity_id: &str, weapon_id: &str, info: &DroppedWeaponInfo, tick: f64) -> Self {
        let mut visual = Self {
            weapon_id: weapon_id.to_owned(),
            position: info.position,
            rotation: DQuat::IDENTITY,
            settled: info.settled,
            weapon_box: weapon_box(weapon_id),
            samples: Vec::with_capacity(MAX_SAMPLES + 1),
            lying: lying_rotation(entity_id),
        };
        visual.add_sample(tick, info);
        visual.draw_at(tick);
        visual
    }

    /// Whether the server says it has come to rest.
    #[must_use]
    pub fn resting(&self) -> bool {
        self.settled
    }

    /// `applyPose`: where the model's origin sits, its box centre less the
    /// turned centre offset. The model is drawn there, turned by
    /// [`Self::rotation`], as its raw GLB meshes times the weapon's scale.
    #[must_use]
    pub fn model_origin(&self) -> [f64; 3] {
        let offset = self.rotation * DVec3::from_array(self.weapon_box.centre);
        (DVec3::from_array(self.position) - offset).to_array()
    }

    /// `addSample`: a server pose at a snapshot tick. A tick earlier than the
    /// newest (a demo seek back) starts the samples again from it.
    fn add_sample(&mut self, tick: f64, info: &DroppedWeaponInfo) {
        self.settled = info.settled;
        if let Some(last) = self.samples.last() {
            if tick == last.tick {
                return;
            }
            if tick < last.tick {
                self.samples.clear();
            }
        }
        let rotation = info.rotation.map(|[x, y, z, w]| DQuat::from_xyzw(x, y, z, w));
        let mut position = DVec3::from_array(info.position);
        // A recording without rotations has the point it was stepped as,
        // just off the floor: the box lying on its side sits half its
        // thickness up.
        if rotation.is_none() {
            position.y += self.weapon_box.size[2] / 2.0 - 4.0;
        }
        self.samples.push(PoseSample {
            tick,
            position,
            rotation,
        });
        if self.samples.len() > MAX_SAMPLES {
            self.samples.remove(0);
        }
    }

    /// `drawAt`: the pose at this (fractional) server tick.
    fn draw_at(&mut self, tick: f64) {
        let samples = &self.samples;
        if samples.is_empty() {
            return;
        }
        let to = samples
            .iter()
            .position(|sample| sample.tick >= tick)
            .unwrap_or(samples.len() - 1);
        let from = to.saturating_sub(1);
        let a = samples[from];
        let b = samples[to];
        let t = if b.tick > a.tick {
            ((tick - a.tick) / (b.tick - a.tick)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        // `Vector3.LerpToRef`: start + (end − start) × amount.
        self.position = (a.position + (b.position - a.position) * t).to_array();
        let ra = a.rotation.unwrap_or(self.lying);
        let rb = b.rotation.unwrap_or(self.lying);
        // `Quaternion.SlerpToRef`, which also takes the shorter way round.
        self.rotation = ra.slerp(rb, t);
    }
}

/// `DroppedWeaponRenderer`.
#[derive(Debug, Clone)]
pub struct DroppedWeaponRenderer {
    weapons: HashMap<String, DroppedWeaponVisual>,
    /// The newest snapshot tick (`latestTick`), −1 before the first.
    latest_tick: f64,
    /// The server tick drawn this frame (`renderTick`, fractional).
    render_tick: f64,
    /// Ticks between snapshots, smoothed (`snapshotTicks`): how far behind
    /// the newest the render tick runs.
    snapshot_ticks: f64,
}

impl Default for DroppedWeaponRenderer {
    fn default() -> Self {
        Self {
            weapons: HashMap::new(),
            latest_tick: -1.0,
            render_tick: 0.0,
            snapshot_ticks: 1.0,
        }
    }
}

impl DroppedWeaponRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `syncServerEntities`: a snapshot's `item_` entities at its `tick`.
    ///
    /// - New entity: spawn a visual at its pose.
    /// - Existing entity: a new pose sample at this snapshot's tick.
    /// - Missing entity: gone (the server sends every dropped weapon
    ///   regardless of PVS, so missing means picked up or expired).
    pub fn sync_server_entities(&mut self, entities: &HashMap<String, DroppedWeaponInfo>, tick: u32) {
        let snapshot_tick = f64::from(tick);
        if snapshot_tick != self.latest_tick {
            if self.latest_tick < 0.0 || snapshot_tick < self.latest_tick {
                // First snapshot, or a demo seek back: start drawing from here.
                self.render_tick = snapshot_tick;
            } else {
                self.snapshot_ticks = self.snapshot_ticks * 0.8 + (snapshot_tick - self.latest_tick) * 0.2;
            }
            self.latest_tick = snapshot_tick;
        }
        self.weapons.retain(|id, _| entities.contains_key(id));
        for (id, info) in entities {
            if let Some(existing) = self.weapons.get_mut(id) {
                existing.add_sample(snapshot_tick, info);
            } else if let Some(weapon_id) = &info.weapon_id {
                self.weapons
                    .insert(id.clone(), DroppedWeaponVisual::new(id, weapon_id, info, snapshot_tick));
            }
        }
    }

    /// `updateVisuals`: advance the render tick `dt` seconds and place every
    /// weapon at it. It runs up to the newest snapshot and no further, so
    /// between snapshots a weapon moves from the last pose to the newest,
    /// and with no new snapshots (a paused demo) it holds still.
    pub fn update(&mut self, dt: f64) {
        let dt = dt.clamp(0.0, MAX_FRAME_SECONDS);
        self.render_tick += dt * TICK_RATE;
        let lag = self.latest_tick - self.render_tick;
        if lag > MAX_LAG_INTERVALS * self.snapshot_ticks {
            self.render_tick = self.latest_tick - self.snapshot_ticks;
        }
        if self.render_tick > self.latest_tick {
            self.render_tick = self.latest_tick;
        }
        for visual in self.weapons.values_mut() {
            visual.draw_at(self.render_tick);
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

    /// The map changed or the match ended: nothing on the floor, and the
    /// next snapshot starts the clock again.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(at: [f64; 3], rotation: Option<[f64; 4]>) -> HashMap<String, DroppedWeaponInfo> {
        HashMap::from([(
            "item_1".to_owned(),
            DroppedWeaponInfo {
                position: at,
                weapon_id: Some("rlauncher".to_owned()),
                settled: false,
                rotation,
            },
        )])
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    const TURNED: [f64; 4] = [
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
    ];

    #[test]
    fn the_model_origin_is_the_box_centre_less_the_turned_centre() {
        let mut r = DroppedWeaponRenderer::new();
        r.sync_server_entities(&info([100.0, 50.0, 0.0], Some([0.0, 0.0, 0.0, 1.0])), 10);
        let (_, w) = r.visuals().next().unwrap();
        // rlauncher: centre (29, 6.9, 0).
        assert!(close(w.model_origin(), [71.0, 43.1, 0.0]));
        // A quarter turn about +y takes +x to −z: the centre's 29 along the
        // barrel now lies along −z.
        r.sync_server_entities(&info([100.0, 50.0, 0.0], Some(TURNED)), 11);
        r.update(1.0);
        let (_, w) = r.visuals().next().unwrap();
        assert!(close(w.model_origin(), [100.0, 43.1, 29.0]), "{:?}", w.model_origin());
    }

    #[test]
    fn it_moves_between_snapshots_and_never_past_the_newest() {
        let mut r = DroppedWeaponRenderer::new();
        r.sync_server_entities(&info([0.0, 0.0, 0.0], Some([0.0, 0.0, 0.0, 1.0])), 100);
        r.sync_server_entities(&info([10.0, 0.0, 0.0], Some(TURNED)), 104);
        // Half way: 2 ticks of 4.
        r.update(2.0 / 125.0);
        let (_, w) = r.visuals().next().unwrap();
        assert!(close(w.position, [5.0, 0.0, 0.0]), "{:?}", w.position);
        let half = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_4);
        assert!(w.rotation.abs_diff_eq(half, 1e-9), "{:?}", w.rotation);
        // No new snapshot (a paused demo): it holds at the newest.
        for _ in 0..10 {
            r.update(1.0 / 60.0);
        }
        let (_, w) = r.visuals().next().unwrap();
        assert!(close(w.position, [10.0, 0.0, 0.0]));
        let held = (w.position, w.rotation);
        r.update(1.0);
        let (_, w) = r.visuals().next().unwrap();
        assert_eq!((w.position, w.rotation), held);
    }

    #[test]
    fn a_seek_back_starts_the_samples_again() {
        let mut r = DroppedWeaponRenderer::new();
        r.sync_server_entities(&info([0.0, 0.0, 0.0], Some([0.0, 0.0, 0.0, 1.0])), 500);
        r.sync_server_entities(&info([10.0, 0.0, 0.0], Some([0.0, 0.0, 0.0, 1.0])), 504);
        r.update(1.0);
        r.sync_server_entities(&info([-20.0, 0.0, 0.0], Some([0.0, 0.0, 0.0, 1.0])), 100);
        r.update(1.0 / 60.0);
        let (_, w) = r.visuals().next().unwrap();
        assert!(close(w.position, [-20.0, 0.0, 0.0]), "{:?}", w.position);
    }

    #[test]
    fn a_recording_without_rotations_lies_on_its_side() {
        let mut r = DroppedWeaponRenderer::new();
        r.sync_server_entities(&info([0.0, 4.0, 0.0], None), 1);
        let (_, w) = r.visuals().next().unwrap();
        // Lifted half the box's z size less 4: rlauncher's 14 → 7 up.
        assert!(close(w.position, [0.0, 7.0, 0.0]));
        // The barrel stays level and the box's z (its thickness) is upright.
        let barrel = w.rotation * DVec3::X;
        assert!(barrel.y.abs() < 1e-9, "{barrel:?}");
        let thickness = w.rotation * DVec3::Z;
        assert!((thickness.y.abs() - 1.0).abs() < 1e-9, "{thickness:?}");
        // The heading is the browser's: hash("item_1") % 628 / 100.
        let hash = "item_1"
            .encode_utf16()
            .fold(0i32, |h, u| h.wrapping_mul(31).wrapping_add(i32::from(u)));
        let yaw = f64::from(hash.cast_unsigned() % 628) / 100.0;
        assert!((barrel.z - -yaw.sin()).abs() < 1e-9 && (barrel.x - yaw.cos()).abs() < 1e-9);
    }

    #[test]
    fn a_weapon_goes_with_its_entity() {
        let mut r = DroppedWeaponRenderer::new();
        r.sync_server_entities(&info([0.0, 0.0, 0.0], None), 1);
        assert_eq!(r.len(), 1);
        r.sync_server_entities(&HashMap::new(), 2);
        assert!(r.is_empty());
    }
}
