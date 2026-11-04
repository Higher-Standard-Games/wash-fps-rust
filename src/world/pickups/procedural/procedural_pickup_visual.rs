//! wash-fps `world/pickups/procedural/ProceduralPickupVisual.ts`: one
//! pickup's sculpture, assembled from its recipe's prototypes and animated
//! from an elapsed clock.
//!
//! - Health: a Fresnel energy shell, near clear face-on and hot at the rim,
//!   flowing between colour and accent, around an accent cross whose up and
//!   down faces take the tier colour, with a ring carrying a scrolling
//!   gradient. High tiers turn the cross to face the camera.
//! - Armour: plates on a dark backer; the shard is tilted.
//! - Ammo: a dark crate with coloured inserts, marks and icon decals.
//! - Quad: a gold Q on a dark badge, rocking gently.
//!
//! The visual here is the parts and their poses; the draws are made from
//! them by [`super::procedural_pickup_factory`]. [`PickupVisualState`] is
//! the server's word on it (`setActive`, with `Pickup.setActiveState`
//! before it): hidden until told, the collect burst when taken and the
//! respawn assembly when back. Not ported: `setInView` (the depth test
//! hides what is behind walls).

use std::f64::consts::PI;

use shagger::babylon::meshes::vertex_data::babylon_local_matrix;
use shagger::rendering::glam::{DMat3, DMat4, DVec3, Vec3};

use super::procedural_pickup_asset_cache::{Proto, PrototypeSet};
use super::procedural_pickup_materials::ENERGY_BOOST;
use super::procedural_pickup_recipes::PROCEDURAL_GLOBAL_SCALE;
use super::procedural_pickup_transient::{ProceduralPickupTransient, TransientOptions};
use super::types::{ProceduralPickupFamily, ProceduralPickupRecipe, Rgb};

const TAU: f64 = PI * 2.0;
const DEG2RAD: f64 = PI / 180.0;

/// Rest height above the anchor, so the item floats clear of the floor.
pub const HOVER_LIFT: f64 = 30.0;
/// Amplifies each recipe's bob.
const BOB_GAIN: f64 = 2.6;
/// A livelier float than the recipe's frequency alone.
const BOB_SPEED: f64 = 1.2;
/// The shell's colour flow (cycles per second).
const COLOR_FLOW_SPEED: f64 = 0.22;
/// The ring gradient's travel (UV turns per second).
const RING_SCROLL_SPEED: f64 = 0.35;

/// Which material a part is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartMaterial {
    Structural,
    ArmorPlate,
    EmblemFill,
    AmmoIcon,
    /// `matEnergy`: core colour, pulsed.
    Energy,
    /// `matAccent`: accent colour, pulsed.
    Accent,
    /// `matSoft`: core colour at alpha 0.3, pulsed.
    Soft,
    /// The Fresnel shell, flowing and pulsed.
    Shell,
    /// The scrolling ring gradient.
    Ring,
}

/// The node a part hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartParent {
    Cosmetic,
    ArmorPivot,
    /// A ring's pivot, by ring.
    Ring(usize),
}

/// One prototype clone (`inst`) with its material and local transform.
#[derive(Debug, Clone, PartialEq)]
pub struct VisualPart {
    pub name: &'static str,
    pub proto: Proto,
    pub material: PartMaterial,
    pub parent: PartParent,
    /// Babylon Euler rotation.
    pub rotation: DVec3,
    pub scaling: f64,
    /// `BILLBOARDMODE_ALL`.
    pub billboard: bool,
}

/// `RingEl`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RingEl {
    speed: f64,
}

/// Where a visual stands at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct VisualPose {
    /// The cosmetic node's world matrix.
    pub cosmetic: DMat4,
    pub cosmetic_translation: DVec3,
    pub cosmetic_scale: f64,
    pub pulse: f64,
    pub shell_emissive: Vec3,
    pub ring_pivots: Vec<DMat4>,
    /// The ring texture's `uOffset`.
    pub ring_offset: f64,
}

/// `ProceduralPickupVisual`.
#[derive(Debug, Clone)]
pub struct ProceduralPickupVisual {
    pub recipe: ProceduralPickupRecipe,
    /// The anchor's (`root`) position, Babylon axes.
    pub position: DVec3,
    phase: f64,
    pub parts: Vec<VisualPart>,
    armor_pivot_rotation: DVec3,
    rings: Vec<RingEl>,
    flow_a: DVec3,
    flow_b: DVec3,
}

fn vec_of(c: Rgb) -> DVec3 {
    DVec3::new(c.r, c.g, c.b)
}

impl ProceduralPickupVisual {
    /// The constructor: `seed` is the pickup id, spreading the phases.
    #[must_use]
    pub fn new(recipe: &ProceduralPickupRecipe, set: &PrototypeSet, position: DVec3, seed: u32) -> Self {
        let mut visual = Self {
            recipe: recipe.clone(),
            position,
            phase: f64::from(seed % 997) * 0.618,
            parts: Vec::new(),
            armor_pivot_rotation: DVec3::ZERO,
            rings: Vec::new(),
            flow_a: vec_of(recipe.color),
            flow_b: vec_of(recipe.accent_color),
        };
        let mut inst = |proto: Proto, parent: PartParent, material: PartMaterial, name: &'static str| {
            set.geometry(proto).map(|_| {
                visual.parts.push(VisualPart {
                    name,
                    proto,
                    material,
                    parent,
                    rotation: DVec3::ZERO,
                    scaling: 1.0,
                    billboard: false,
                });
                visual.parts.len() - 1
            })
        };
        use PartMaterial as M;
        use PartParent::Cosmetic;
        let mut billboarded = Vec::new();
        let mut armor_rotation = DVec3::ZERO;
        match recipe.family {
            ProceduralPickupFamily::Health => {
                let cross = inst(Proto::Cross, Cosmetic, M::Accent, "cross");
                let caps = inst(Proto::CrossCaps, Cosmetic, M::EmblemFill, "cross_caps");
                if recipe.high_tier {
                    billboarded.extend(cross);
                    billboarded.extend(caps);
                }
                inst(Proto::Core, Cosmetic, M::Shell, "shell");
            }
            ProceduralPickupFamily::Ammo => {
                inst(Proto::AmmoCrate, Cosmetic, M::Structural, "ammo_crate");
                inst(Proto::AmmoPanel, Cosmetic, M::ArmorPlate, "ammo_panel");
                inst(Proto::AmmoMark, Cosmetic, M::Structural, "ammo_mark");
                if set.ammo_icon_mat.is_some() {
                    inst(Proto::AmmoIcon, Cosmetic, M::AmmoIcon, "ammo_icon");
                }
            }
            ProceduralPickupFamily::Powerup => {
                inst(Proto::PowerupCore, Cosmetic, M::Structural, "powerup_core");
                inst(Proto::PowerupMark, Cosmetic, M::Energy, "powerup_mark");
            }
            ProceduralPickupFamily::Armor => {
                inst(Proto::ArmorFrame, PartParent::ArmorPivot, M::Structural, "armor_frame");
                inst(Proto::Armor, PartParent::ArmorPivot, M::ArmorPlate, "armor");
                if recipe.kind == "shard" {
                    armor_rotation = DVec3::new(0.22, 0.0, 0.5);
                }
            }
        }
        let mut ring_parts = Vec::new();
        for i in 0..recipe.ring_count as usize {
            let material = if recipe.family == ProceduralPickupFamily::Health {
                M::Ring
            } else {
                M::Accent
            };
            ring_parts.push((i, inst(Proto::Ring, PartParent::Ring(i), material, "ring")));
        }
        for i in billboarded {
            visual.parts[i].scaling = 0.9;
            visual.parts[i].billboard = true;
        }
        visual.armor_pivot_rotation = armor_rotation;
        for (i, part) in ring_parts {
            if let Some(part) = part {
                visual.parts[part].rotation.x = 0.3 + i as f64 * 0.6;
            }
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            visual.rings.push(RingEl {
                speed: recipe.spin_speed * DEG2RAD * (0.9 + i as f64 * 0.4) * dir,
            });
        }
        visual
    }

    /// `update(dt, elapsed)` for a visible pickup in view: the pose at
    /// `elapsed` seconds.
    #[must_use]
    pub fn pose(&self, elapsed: f64) -> VisualPose {
        self.pose_assembling(elapsed, 1.0)
    }

    /// [`Self::pose`] at `assemble_scale` of its size (the respawn assembly).
    #[must_use]
    pub fn pose_assembling(&self, elapsed: f64, assemble_scale: f64) -> VisualPose {
        let t = elapsed + self.phase;
        let r = &self.recipe;
        let bob_w = t * r.bob_frequency * BOB_SPEED * TAU;
        let y = HOVER_LIFT + bob_w.sin() * r.bob_amplitude * BOB_GAIN;
        let x = if r.figure_eight {
            (bob_w * 2.0).sin() * r.bob_amplitude * 0.35
        } else {
            0.0
        };
        let yaw = if r.family == ProceduralPickupFamily::Powerup {
            0.28 * (t * 0.45 * TAU).sin()
        } else {
            t * r.spin_speed * DEG2RAD
        };
        let breathe = assemble_scale * (1.0 + 0.02 * (t * r.pulse_frequency * TAU).sin());
        let local = babylon_local_matrix(DVec3::new(x, y, 0.0), DVec3::new(0.0, yaw, 0.0), DVec3::splat(breathe));
        let cosmetic = DMat4::from_translation(self.position) * local;

        let pulse_w = t * r.pulse_frequency * TAU;
        let mut pulse = 0.82 + 0.18 * pulse_w.sin();
        if r.high_tier {
            pulse += 0.12 * (pulse_w * 2.0).sin().max(0.0);
        }
        if r.unstable {
            pulse += 0.06 * (t * 11.0).sin();
        }
        let pulse = pulse.max(0.5);
        let f = 0.5 + 0.5 * (t * COLOR_FLOW_SPEED * TAU).sin();
        let flow = (self.flow_a + (self.flow_b - self.flow_a) * f) * pulse;
        VisualPose {
            cosmetic,
            cosmetic_translation: self.position + DVec3::new(x, y, 0.0),
            cosmetic_scale: breathe,
            pulse,
            shell_emissive: flow.as_vec3(),
            ring_pivots: self
                .rings
                .iter()
                .map(|ring| cosmetic * DMat4::from_rotation_y(t * ring.speed))
                .collect(),
            ring_offset: (t * RING_SCROLL_SPEED) % 1.0,
        }
    }

    /// `createTransient`'s options: the collection burst for this recipe.
    #[must_use]
    pub fn transient_options(&self) -> TransientOptions {
        let eff = self.recipe.scale * PROCEDURAL_GLOBAL_SCALE;
        TransientOptions {
            duration: if self.recipe.high_tier { 0.46 } else { 0.34 },
            end_diameter: 32.0 * eff,
            style: self.recipe.collect_style,
            scale: eff,
            family: self.recipe.family,
        }
    }

    /// A part's world matrix at `pose`; `camera_rotation` turns camera axes
    /// into world axes (the inverse view rotation), for billboards.
    #[must_use]
    pub fn part_world(&self, part: &VisualPart, pose: &VisualPose, camera_rotation: DMat3) -> DMat4 {
        let local = babylon_local_matrix(DVec3::ZERO, part.rotation, DVec3::splat(part.scaling));
        if part.billboard {
            // A billboard keeps its parent's translation and scale, not its
            // rotation, then turns with the camera.
            let scale = DMat4::from_scale(DVec3::splat(pose.cosmetic_scale)) * local;
            return DMat4::from_translation(pose.cosmetic_translation)
                * DMat4::from_mat3(camera_rotation)
                * DMat4::from_mat3(DMat3::from_mat4(scale));
        }
        let parent = match part.parent {
            PartParent::Cosmetic => pose.cosmetic,
            PartParent::ArmorPivot => {
                pose.cosmetic * babylon_local_matrix(DVec3::ZERO, self.armor_pivot_rotation, DVec3::ONE)
            }
            PartParent::Ring(i) => pose.ring_pivots[i],
        };
        parent * local
    }

    /// The emissive colour a part's material shows at `pose`, for the
    /// materials that pulse.
    #[must_use]
    pub fn part_emissive(&self, part: &VisualPart, pose: &VisualPose) -> Option<Vec3> {
        let base = match part.material {
            PartMaterial::Energy | PartMaterial::Soft => vec_of(self.recipe.color) * ENERGY_BOOST,
            PartMaterial::Accent => vec_of(self.recipe.accent_color) * ENERGY_BOOST,
            PartMaterial::Shell => return Some(pose.shell_emissive),
            _ => return None,
        };
        Some((base * pose.pulse).as_vec3())
    }
}

/// A map pickup's visibility as the server tells it: `Pickup.isActive`
/// (`setActiveState`) and the visual's own `setActive`, the respawn
/// assembly and the collection burst.
#[derive(Debug, Clone, PartialEq)]
pub struct PickupVisualState {
    /// `Pickup.isActive`.
    is_active: bool,
    /// The visual's `prevActive`: `None` until its first authoritative value.
    prev_active: Option<bool>,
    /// Drawn (`visible`).
    pub visible: bool,
    assemble_age: f64,
    assemble_duration: f64,
    /// The collection burst, played again on each collection.
    pub transient: ProceduralPickupTransient,
}

impl PickupVisualState {
    /// `server_managed`: a map pickup in a match or demo starts hidden until
    /// the first active mask (fail-closed); a held view shows it.
    #[must_use]
    pub fn new(visual: &ProceduralPickupVisual, server_managed: bool) -> Self {
        Self {
            is_active: !server_managed,
            prev_active: None,
            visible: !server_managed,
            assemble_age: f64::INFINITY,
            assemble_duration: if visual.recipe.high_tier { 0.7 } else { 0.45 },
            transient: ProceduralPickupTransient::new(&visual.transient_options()),
        }
    }

    /// `Pickup.setActiveState` then `ProceduralPickupVisual.setActive`;
    /// `cosmetic` is where the item floats (local x and y), for the burst.
    pub fn set_active(&mut self, active: bool, cosmetic: [f64; 2]) {
        if active == self.is_active {
            return;
        }
        self.is_active = active;
        let Some(prev) = self.prev_active else {
            // First authoritative value: applied silently.
            self.prev_active = Some(active);
            self.visible = active;
            return;
        };
        if active == prev {
            return;
        }
        self.prev_active = Some(active);
        if active {
            self.visible = true;
            self.assemble_age = 0.0;
        } else {
            let y = if cosmetic[1] == 0.0 { HOVER_LIFT } else { cosmetic[1] };
            self.transient.play([cosmetic[0], y]);
            self.visible = false;
        }
    }

    /// `update`'s clocks: the burst runs while hidden; the assembly while
    /// shown. The item's size this frame (`assembleScale`).
    pub fn update(&mut self, dt: f64) -> f64 {
        self.transient.update(dt);
        if !self.visible {
            return 1.0;
        }
        self.assembly_scale_after(dt)
    }

    fn assembly_scale_after(&mut self, dt: f64) -> f64 {
        if self.assemble_age < self.assemble_duration {
            self.assemble_age += dt;
            let p = (self.assemble_age / self.assemble_duration).min(1.0);
            return 0.35 + 0.65 * (1.0 - (1.0 - p) * (1.0 - p));
        }
        1.0
    }
}

/// The inverse of a view matrix's rotation: camera axes to world axes.
#[must_use]
pub fn camera_rotation(view: DMat4) -> DMat3 {
    DMat3::from_mat4(view).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_hides_bursts_and_brings_back() {
        let recipe = get_procedural_pickup_recipe("armor_ra").unwrap();
        let set = build_prototype_set(recipe);
        let visual = ProceduralPickupVisual::new(recipe, &set, DVec3::ZERO, 3);
        let mut state = PickupVisualState::new(&visual, true);
        // Hidden until told; the first word (here: still out) is silent.
        assert!(!state.visible);
        state.set_active(false, [0.0, 30.0]);
        assert!(!state.visible && !state.transient.playing());
        // Its first appearance is silent too: `isActive` was false.
        state.set_active(true, [0.0, 30.0]);
        assert!(state.visible);
        assert_eq!(state.update(0.1), 1.0);
        // Taken: the burst plays while the item is hidden.
        state.set_active(false, [0.0, 34.0]);
        assert!(!state.visible && state.transient.playing());
        // Back: it assembles from 0.35 of its size.
        state.set_active(true, [0.0, 30.0]);
        let first = state.update(0.0);
        assert!((first - 0.35).abs() < 1e-12);
        let later = state.update(0.5);
        assert!(later > 0.9 && later < 1.0);
        assert_eq!(state.update(1.0), 1.0);
        // A held view shows it from the start.
        assert!(PickupVisualState::new(&visual, false).visible);
    }
    use crate::world::pickups::procedural::procedural_pickup_asset_cache::build_prototype_set;
    use crate::world::pickups::procedural::procedural_pickup_recipes::get_procedural_pickup_recipe;

    #[test]
    fn health_hovers_spins_and_pulses() {
        let recipe = get_procedural_pickup_recipe("health_mega").unwrap();
        let set = build_prototype_set(recipe);
        let visual = ProceduralPickupVisual::new(recipe, &set, DVec3::new(10.0, 20.0, 30.0), 0);
        let names: Vec<_> = visual.parts.iter().map(|p| p.name).collect();
        assert_eq!(names, ["cross", "cross_caps", "shell", "ring"]);
        assert!(visual.parts[0].billboard && visual.parts[1].billboard);
        let pose = visual.pose(0.0);
        // At t = 0 the bob and spin are at rest: straight above the anchor.
        assert!((pose.cosmetic_translation - DVec3::new(10.0, 50.0, 30.0)).length() < 1e-9);
        assert!((pose.pulse - 0.82).abs() < 1e-12);
        // The billboard faces the camera whatever the spin.
        let world = visual.part_world(&visual.parts[0], &visual.pose(1.3), DMat3::IDENTITY);
        let x = world.transform_vector3(DVec3::X);
        assert!(x.y.abs() < 1e-9 && x.z.abs() < 1e-9);
    }
}
