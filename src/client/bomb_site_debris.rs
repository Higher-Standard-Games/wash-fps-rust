//! Port of wash-fps `client/BombSiteDebris.ts`: cosmetic, bounded model
//! fragments thrown from a `func_explosive` when the bomb disables it. The
//! persistent layout (which brushes are gone) comes from the snapshot; this
//! only animates the moment it changes.
//!
//! The TypeScript clones a preloaded model per fragment; here a fragment
//! names its template (an index into the definition's `debris` paths) and
//! the renderer draws that model at [`Fragment::world`].

use std::collections::BTreeSet;

use shagger::rendering::glam::{DVec3, EulerRot, Mat4, Quat};
use shagger::shared::bsp::brush_model_state::BrushModelDefinition;
use shagger::shared::math_random::MathRandom;

/// Fragments per disabled model, and alive at once.
pub const FRAGMENTS_PER_MODEL: usize = 12;
pub const MAX_FRAGMENTS: usize = 36;
/// Seconds a fragment lives.
pub const FRAGMENT_LIFETIME: f64 = 4.0;
const GRAVITY: f64 = 600.0;
/// `velocity -= normal * 1.45 * dot`, then `* 0.65`.
const BOUNCE_REFLECT: f64 = 1.45;
const BOUNCE_DAMPING: f64 = 0.65;
/// The step is clamped, so a hitch does not throw fragments through walls.
const MAX_STEP: f64 = 0.05;

/// One tumbling fragment.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    /// The debris model it is, by path.
    pub path: String,
    pub position: DVec3,
    /// Babylon's `rotation` (Euler, yaw about Y applied first).
    pub rotation: DVec3,
    pub velocity: DVec3,
    pub spin: DVec3,
    pub age: f64,
}

impl Fragment {
    /// The fragment's node matrix, as Babylon composes `position` and
    /// `rotation`.
    #[must_use]
    pub fn world(&self) -> Mat4 {
        Mat4::from_rotation_translation(
            Quat::from_euler(
                EulerRot::YXZ,
                self.rotation.y as f32,
                self.rotation.x as f32,
                self.rotation.z as f32,
            ),
            self.position.as_vec3(),
        )
    }
}

/// A sweep through the map (`trace` with a point and the solid mask): the
/// end position and, if one was hit, the surface normal; `None` when the
/// segment is clear.
pub type DebrisTrace<'a> = &'a mut dyn FnMut(DVec3, DVec3) -> Option<(DVec3, Option<DVec3>)>;

/// `BombSiteDebris`.
#[derive(Debug, Clone)]
pub struct BombSiteDebris {
    fragments: Vec<Fragment>,
    /// The disabled models last seen; `None` until the first snapshot of a
    /// round (or map), which only establishes state.
    previous: Option<BTreeSet<usize>>,
    round: Option<u32>,
    /// `Math.random`'s stand-in.
    random: MathRandom,
}

impl Default for BombSiteDebris {
    fn default() -> Self {
        Self {
            fragments: Vec::new(),
            previous: None,
            round: None,
            random: MathRandom::new(0x2f6b_95d1_c3a7_0e49),
        }
    }
}

impl BombSiteDebris {
    fn random(&mut self) -> f64 {
        self.random.next_f64()
    }

    /// `update`: `round` and `disabled` from the match state, the map's
    /// `definitions`, `available` says whether a debris model loaded (the
    /// TypeScript's preloaded templates), `trace` bounces the fragments.
    pub fn update(
        &mut self,
        dt: f64,
        round: u32,
        disabled: &[usize],
        definitions: &[BrushModelDefinition],
        available: &dyn Fn(&str) -> bool,
        mut trace: Option<DebrisTrace<'_>>,
    ) {
        if self.round != Some(round) {
            self.fragments.clear();
            self.previous = None;
            self.round = Some(round);
        }
        if definitions.is_empty() {
            self.previous = None;
            return;
        }
        let current: BTreeSet<usize> = disabled.iter().copied().collect();
        if self.previous.as_ref() != Some(&current) {
            if let Some(previous) = &self.previous {
                let newly: Vec<&BrushModelDefinition> = definitions
                    .iter()
                    .filter(|d| current.contains(&d.model_index) && !previous.contains(&d.model_index))
                    .collect();
                for definition in newly {
                    self.emit(definition, available);
                }
            }
            // The first snapshot establishes state: joining after an
            // explosion emits nothing.
            self.previous = Some(current);
        }
        if self.fragments.is_empty() {
            return;
        }
        let step = dt.clamp(0.0, MAX_STEP);
        self.fragments.retain_mut(|fragment| {
            fragment.age += step;
            if fragment.age > FRAGMENT_LIFETIME {
                return false;
            }
            fragment.velocity.y -= GRAVITY * step;
            let from = fragment.position;
            let to = from + fragment.velocity * step;
            match trace.as_mut().and_then(|t| t(from, to)) {
                Some((end, normal)) => {
                    fragment.position = end;
                    if let Some(normal) = normal {
                        fragment.position += normal;
                        let dot = fragment.velocity.dot(normal);
                        fragment.velocity -= normal * (BOUNCE_REFLECT * dot);
                        fragment.velocity *= BOUNCE_DAMPING;
                    }
                }
                None => fragment.position = to,
            }
            fragment.rotation += fragment.spin * step;
            true
        });
    }

    /// `emit`: up to twelve fragments from the middle half of the model's
    /// box, alternating its debris models, never past `MAX_FRAGMENTS`.
    fn emit(&mut self, definition: &BrushModelDefinition, available: &dyn Fn(&str) -> bool) {
        let paths: Vec<&String> = definition.debris.iter().filter(|p| available(p)).collect();
        if paths.is_empty() {
            return;
        }
        let (min, max) = (definition.min, definition.max);
        for i in 0..FRAGMENTS_PER_MODEL {
            if self.fragments.len() >= MAX_FRAGMENTS {
                break;
            }
            let position = DVec3::new(
                min.x + (0.25 + self.random() * 0.5) * (max.x - min.x),
                min.y + (0.25 + self.random() * 0.5) * (max.y - min.y),
                min.z + (0.25 + self.random() * 0.5) * (max.z - min.z),
            );
            let velocity = DVec3::new(
                (self.random() - 0.5) * 450.0,
                180.0 + self.random() * 300.0,
                (self.random() - 0.5) * 450.0,
            );
            let spin = DVec3::new(self.random() * 6.0, self.random() * 6.0, self.random() * 6.0);
            self.fragments.push(Fragment {
                path: paths[i % paths.len()].clone(),
                position,
                rotation: DVec3::ZERO,
                velocity,
                spin,
                age: 0.0,
            });
        }
    }

    pub fn fragments(&self) -> &[Fragment] {
        &self.fragments
    }

    pub fn clear(&mut self) {
        self.fragments.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::shared::server::simulation::types::Vec3;

    fn definition(index: usize) -> BrushModelDefinition {
        BrushModelDefinition {
            model_index: index,
            targetname: "a".into(),
            delay: 0.6,
            initially_enabled: true,
            origin: Vec3::new(0.0, 0.0, 0.0),
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(100.0, 100.0, 100.0),
            debris: vec!["models/objects/debris/debris4.md3".into()],
        }
    }

    #[test]
    fn emits_only_on_a_change_after_the_first_snapshot() {
        let defs = [definition(5), definition(6)];
        let mut debris = BombSiteDebris::default();
        // Joining after the detonation: state only.
        debris.update(0.016, 1, &[5], &defs, &|_| true, None);
        assert!(debris.fragments().is_empty());
        debris.update(0.016, 1, &[5, 6], &defs, &|_| true, None);
        assert_eq!(debris.fragments().len(), FRAGMENTS_PER_MODEL);
        let f = &debris.fragments()[0];
        assert!(f.position.x >= 25.0 && f.position.x <= 75.0);
        // The same list again emits nothing.
        debris.update(0.016, 1, &[5, 6], &defs, &|_| true, None);
        assert_eq!(debris.fragments().len(), FRAGMENTS_PER_MODEL);
        // A new round clears them and starts over.
        debris.update(0.016, 2, &[], &defs, &|_| true, None);
        assert!(debris.fragments().is_empty());
    }

    #[test]
    fn caps_fragments_and_skips_missing_models() {
        let defs: Vec<_> = (1..=5).map(definition).collect();
        let mut debris = BombSiteDebris::default();
        debris.update(0.016, 1, &[], &defs, &|_| false, None);
        debris.update(0.016, 1, &[1, 2, 3, 4, 5], &defs, &|_| false, None);
        assert!(debris.fragments().is_empty());
        let mut debris = BombSiteDebris::default();
        debris.update(0.016, 1, &[], &defs, &|_| true, None);
        debris.update(0.016, 1, &[1, 2, 3, 4, 5], &defs, &|_| true, None);
        assert_eq!(debris.fragments().len(), MAX_FRAGMENTS);
    }

    #[test]
    fn falls_bounces_and_expires() {
        let defs = [definition(5)];
        let mut debris = BombSiteDebris::default();
        debris.update(0.016, 1, &[], &defs, &|_| true, None);
        debris.update(0.0, 1, &[5], &defs, &|_| true, None);
        let floor = |from: DVec3, to: DVec3| -> Option<(DVec3, Option<DVec3>)> {
            (to.y < 0.0).then(|| {
                let t = from.y / (from.y - to.y);
                (from + (to - from) * t, Some(DVec3::Y))
            })
        };
        let mut floor = floor;
        for _ in 0..60 {
            debris.update(0.05, 1, &[5], &defs, &|_| true, Some(&mut floor));
        }
        assert!(debris.fragments().iter().all(|f| f.position.y >= 0.0));
        for _ in 0..30 {
            debris.update(0.05, 1, &[5], &defs, &|_| true, Some(&mut floor));
        }
        assert!(debris.fragments().is_empty());
    }
}
