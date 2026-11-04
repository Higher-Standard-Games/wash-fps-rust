//! wash-fps `world/pickups/procedural/ProceduralPickupTransient.ts`: the
//! short burst of pieces, shaped and coloured for the item, that a pickup
//! plays each time it is collected.
//!
//! Six fragments (crosses' arms, armour plates, crate panels or the quad's
//! triangles) fly out, tumble, shrink and fade, around a flash that shrinks
//! faster. The pieces are built once with the pickup's model
//! ([`TransientShapes`]) and posed here from the burst's age.

use std::f64::consts::PI;

use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::babylon::meshes::ico_sphere_builder::create_ico_sphere;
use shagger::babylon::meshes::mesh_builder::{create_box, create_cylinder};
use shagger::babylon::meshes::vertex_data::{VertexData, babylon_local_matrix};
use shagger::rendering::glam::{DMat4, DVec3, Vec3};

use super::types::{CollectStyle, ProceduralPickupFamily, Rgb};

/// `SHAPES`: each family's six fragment offsets, in burst units.
fn shapes(family: ProceduralPickupFamily) -> [[f64; 2]; 6] {
    match family {
        ProceduralPickupFamily::Health => [
            [-1.0, 0.0],
            [1.0, 0.0],
            [0.0, -1.0],
            [0.0, 1.0],
            [-0.45, 0.45],
            [0.45, -0.45],
        ],
        ProceduralPickupFamily::Armor => [
            [-0.8, 0.65],
            [0.8, 0.65],
            [0.0, 0.2],
            [-0.5, -0.65],
            [0.5, -0.65],
            [0.0, -0.9],
        ],
        ProceduralPickupFamily::Ammo => [
            [-0.7, 0.5],
            [0.7, 0.5],
            [-0.7, -0.5],
            [0.7, -0.5],
            [0.0, 0.9],
            [0.0, -0.9],
        ],
        ProceduralPickupFamily::Powerup => {
            std::array::from_fn(|i| [((i as f64) * PI / 3.0).cos(), ((i as f64) * PI / 3.0).sin()])
        }
    }
}

/// `TransientOptions`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransientOptions {
    pub duration: f64,
    pub end_diameter: f64,
    pub style: CollectStyle,
    pub scale: f64,
    pub family: ProceduralPickupFamily,
}

/// Which of the burst's three materials a piece takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientMaterial {
    Bright,
    Dark,
    Flash,
}

/// The burst's pieces as geometry: six fragments, then the flash.
#[derive(Debug, Clone)]
pub struct TransientShapes {
    pub pieces: Vec<(VertexData, TransientMaterial)>,
}

/// `material(name, tint)`: self-lit, blended, two-sided, no depth writes.
#[must_use]
pub fn transient_material(tint: Vec3) -> StandardMaterialParams {
    StandardMaterialParams {
        disable_lighting: true,
        diffuse_color: Vec3::ZERO,
        specular_color: Vec3::ZERO,
        emissive_color: tint,
        alpha: 1.0,
        alpha_blend: true,
        back_face_culling: false,
        disable_depth_write: true,
        ..StandardMaterialParams::default()
    }
}

fn lerp_white(c: Rgb, t: f64) -> Vec3 {
    Vec3::new(
        (c.r + (1.0 - c.r) * t) as f32,
        (c.g + (1.0 - c.g) * t) as f32,
        (c.b + (1.0 - c.b) * t) as f32,
    )
}

/// The three materials' emissive tints for a pickup of `color`: bright,
/// dark, flash.
#[must_use]
pub fn transient_tints(color: Rgb) -> [Vec3; 3] {
    [
        lerp_white(color, 0.4),
        Vec3::new(color.r as f32, color.g as f32, color.b as f32) * 0.42,
        lerp_white(color, 0.7),
    ]
}

/// The constructor's meshes.
#[must_use]
pub fn build_transient_shapes(opts: &TransientOptions) -> TransientShapes {
    let s = opts.scale;
    let mut pieces = Vec::new();
    for i in 0..6 {
        let data = if opts.family == ProceduralPickupFamily::Powerup {
            create_cylinder(2.8 * s, 0.0, 2.7 * s, 3)
        } else {
            let armor = opts.family == ProceduralPickupFamily::Armor;
            let ammo = opts.family == ProceduralPickupFamily::Ammo;
            let width = if armor {
                if i < 2 { 4.2 } else { 3.2 }
            } else if ammo {
                3.7
            } else if i < 4 {
                2.1
            } else {
                1.5
            };
            let height = if armor {
                3.5
            } else if ammo {
                2.8
            } else if i < 4 {
                3.5
            } else {
                1.5
            };
            let depth = if ammo { 1.6 } else { 1.1 };
            create_box(width * s, height * s, depth * s)
        };
        let material = if i % 3 == 0 {
            TransientMaterial::Dark
        } else {
            TransientMaterial::Bright
        };
        pieces.push((data, material));
    }
    pieces.push((create_ico_sphere(2.3 * s, 1), TransientMaterial::Flash));
    TransientShapes { pieces }
}

/// `Fragment`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fragment {
    offset: DVec3,
    start: DVec3,
    travel: DVec3,
    tumble: DVec3,
    base_rotation_z: f64,
}

/// One piece's place this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransientPiece {
    /// Its index in [`TransientShapes::pieces`].
    pub index: usize,
    /// Local to the pickup's anchor.
    pub local: DMat4,
    pub alpha: f32,
}

/// `ProceduralPickupTransient`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralPickupTransient {
    fragments: Vec<Fragment>,
    flash_position: DVec3,
    duration: f64,
    age: f64,
    /// Between bursts: nothing shown, nothing to advance.
    idle: bool,
}

impl ProceduralPickupTransient {
    /// Built parked: the first collection plays it.
    #[must_use]
    pub fn new(opts: &TransientOptions) -> Self {
        let layout = shapes(opts.family);
        let spread = opts.end_diameter
            * if opts.style == CollectStyle::Shatter {
                0.33
            } else {
                0.25
            };
        let n = layout.len() as f64;
        let fragments = layout
            .iter()
            .enumerate()
            .map(|(i, [x, y])| {
                let fi = i as f64;
                let base_rotation_z = if opts.family == ProceduralPickupFamily::Powerup {
                    fi * PI / 3.0
                } else {
                    0.0
                };
                let angle = (fi / n) * PI * 2.0;
                let odd = if i % 2 == 1 { 1.0 } else { -1.0 };
                Fragment {
                    offset: DVec3::new(x * 2.5 * opts.scale, y * 2.5 * opts.scale, 0.0),
                    start: DVec3::ZERO,
                    travel: DVec3::new(
                        angle.cos() * spread,
                        (0.4 + (i % 3) as f64 * 0.18) * spread,
                        angle.sin() * spread,
                    ),
                    tumble: DVec3::new(odd * 1.4, ((i % 3) as f64 - 1.0) * 1.8, -odd * 1.5),
                    base_rotation_z,
                }
            })
            .collect();
        Self {
            fragments,
            flash_position: DVec3::ZERO,
            duration: opts.duration,
            age: 0.0,
            idle: true,
        }
    }

    /// `play`: start a burst from `center` (local to the anchor: the
    /// animated item above it).
    pub fn play(&mut self, center: [f64; 2]) {
        self.age = 0.0;
        self.idle = false;
        for fragment in &mut self.fragments {
            fragment.start = DVec3::new(center[0] + fragment.offset.x, center[1] + fragment.offset.y, 0.0);
        }
        self.flash_position = DVec3::new(center[0], center[1], 0.0);
    }

    /// `update`: advance the burst; false once it has finished (or none runs).
    pub fn update(&mut self, dt: f64) -> bool {
        if self.idle {
            return false;
        }
        self.age += dt;
        if self.age >= self.duration {
            self.idle = true;
            return false;
        }
        true
    }

    /// Whether a burst is showing.
    #[must_use]
    pub fn playing(&self) -> bool {
        !self.idle
    }

    /// Every piece's pose at the burst's age; none between bursts.
    #[must_use]
    pub fn pieces(&self) -> Vec<TransientPiece> {
        if self.idle {
            return Vec::new();
        }
        let p = (self.age / self.duration).min(1.0);
        let ease = 1.0 - (1.0 - p) * (1.0 - p);
        let fragment_alpha = ((1.0 - p) * (1.0 - p)) as f32;
        let mut out: Vec<TransientPiece> = self
            .fragments
            .iter()
            .enumerate()
            .map(|(index, f)| TransientPiece {
                index,
                local: babylon_local_matrix(
                    f.start + f.travel * ease,
                    DVec3::new(
                        f.tumble.x * ease,
                        f.tumble.y * ease,
                        f.base_rotation_z + f.tumble.z * ease,
                    ),
                    DVec3::splat((1.0 - p * 0.65).max(0.01)),
                ),
                alpha: fragment_alpha,
            })
            .collect();
        out.push(TransientPiece {
            index: self.fragments.len(),
            local: babylon_local_matrix(
                self.flash_position,
                DVec3::ZERO,
                DVec3::splat((1.0 - p * 2.5).max(0.01)),
            ),
            alpha: (1.0 - p * 4.0).max(0.0) as f32,
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> TransientOptions {
        TransientOptions {
            duration: 0.34,
            end_diameter: 32.0,
            style: CollectStyle::Collapse,
            scale: 1.0,
            family: ProceduralPickupFamily::Health,
        }
    }

    #[test]
    fn a_burst_flies_out_fades_and_parks() {
        let mut t = ProceduralPickupTransient::new(&opts());
        assert!(t.pieces().is_empty(), "built parked");
        t.play([0.0, 30.0]);
        let start = t.pieces();
        assert_eq!(start.len(), 7);
        assert_eq!(start[0].alpha, 1.0);
        // The first fragment starts at its offset over the centre.
        let at = start[0].local.transform_point3(DVec3::ZERO);
        assert!((at - DVec3::new(-2.5, 30.0, 0.0)).length() < 1e-9);
        assert!(t.update(0.17));
        let mid = t.pieces();
        assert!(mid[0].alpha < 0.3 && mid[0].alpha > 0.2);
        // The flash is gone by a quarter of the way.
        assert_eq!(mid[6].alpha, 0.0);
        let moved = mid[0].local.transform_point3(DVec3::ZERO);
        assert!(moved.y > 30.0, "fragments rise: {moved}");
        assert!(!t.update(0.2));
        assert!(t.pieces().is_empty());
    }

    #[test]
    fn shapes_have_six_fragments_and_a_flash() {
        for family in [
            ProceduralPickupFamily::Health,
            ProceduralPickupFamily::Armor,
            ProceduralPickupFamily::Ammo,
            ProceduralPickupFamily::Powerup,
        ] {
            let shapes = build_transient_shapes(&TransientOptions { family, ..opts() });
            assert_eq!(shapes.pieces.len(), 7);
            assert_eq!(shapes.pieces[0].1, TransientMaterial::Dark);
            assert_eq!(shapes.pieces[6].1, TransientMaterial::Flash);
            assert!(shapes.pieces.iter().all(|(d, _)| !d.indices.is_empty()));
        }
    }
}
