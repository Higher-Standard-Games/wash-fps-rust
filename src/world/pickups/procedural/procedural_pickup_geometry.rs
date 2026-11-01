//! wash-fps `world/pickups/procedural/ProceduralPickupGeometry.ts`: each
//! recipe's prototype geometry, built once and shared by every pickup of the
//! recipe.
//!
//! - Health: a smooth sphere shell around a chunky cross, whose up and down
//!   faces are capped by a separate mesh in their own colour.
//! - Armour: chest, shoulder and waist plates over a dark backer; the shard
//!   is a smaller broken plate.
//! - Ammo: a chamfered chassis, coloured inserts and type marks.
//! - Quad: a dark octagonal badge with a luminous Q on both faces.
//!
//! Which material each part takes is the visual's business.

use std::f64::consts::PI;

use shagger::babylon::meshes::mesh_builder::{create_box, create_cylinder, create_plane, create_sphere, create_torus};
use shagger::babylon::meshes::vertex_data::{VertexData, babylon_local_matrix};
use shagger::rendering::glam::{DMat4, DVec3};

use super::procedural_pickup_recipes::PROCEDURAL_GLOBAL_SCALE;
use super::types::ProceduralPickupRecipe;

// Base dimensions at recipe scale 1 (Quake units).
const CROSS_ARM: f64 = 6.0;
const CROSS_THICK: f64 = 2.0;
/// How far the caps sit proud of the bar faces (z-fight guard).
const CROSS_CAP_LIFT: f64 = 0.06;
const CORE_DIAM: f64 = 15.0;
const RING_DIAM: f64 = 21.0;
const RING_THICK: f64 = 0.9;
const ARMOR_HEIGHT: f64 = 20.0;
const ARMOR_WIDTH: f64 = 19.0;
const SHARD_SIZE_RATIO: f64 = 0.9;
const CRATE_W: f64 = 15.0;
const CRATE_H: f64 = 11.0;
const CRATE_D: f64 = 11.0;
/// Icon decal size, as a fraction of the face it sits on.
const CRATE_ICON_FILL: f64 = 0.62;
const QUAD_BADGE_DIAM: f64 = 20.0;
const QUAD_MARK_DIAM: f64 = 12.0;

fn scale_of(recipe: &ProceduralPickupRecipe) -> f64 {
    recipe.scale * PROCEDURAL_GLOBAL_SCALE
}

fn at(x: f64, y: f64, z: f64) -> DMat4 {
    DMat4::from_translation(DVec3::new(x, y, z))
}

/// `mergeOrThrow`: `Mesh.MergeMeshes` of parts placed by their matrices.
fn merge(parts: Vec<(VertexData, DMat4)>) -> VertexData {
    VertexData::merge_meshes(parts)
}

/// `bakePart`: a part placed, with the placement baked into its vertices.
fn bake_part(mut data: VertexData, position: DVec3, rotation: DVec3) -> (VertexData, DMat4) {
    data.bake(babylon_local_matrix(position, rotation, DVec3::ONE));
    (data, DMat4::IDENTITY)
}

/// `buildCrossProto`: three orthogonal bars.
#[must_use]
pub fn build_cross_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let len = CROSS_ARM * 2.0 * s;
    let t = CROSS_THICK * s;
    merge(vec![
        (create_box(len, t, t), DMat4::IDENTITY),
        (create_box(t, len, t), DMat4::IDENTITY),
        (create_box(t, t, len), DMat4::IDENTITY),
    ])
}

/// `buildCrossCapsProto`: wafer plates just proud of the bars' up and down
/// faces, in the tier colour.
#[must_use]
pub fn build_cross_caps_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let len = CROSS_ARM * 2.0 * s;
    let t = CROSS_THICK * s;
    let lift = CROSS_CAP_LIFT * s;
    let plate = t * 0.06;
    let mut caps = Vec::new();
    for sign in [1.0, -1.0] {
        caps.push((create_box(len, plate, t), at(0.0, sign * (t / 2.0 + lift), 0.0)));
        caps.push((create_box(t, plate, len), at(0.0, sign * (t / 2.0 + lift), 0.0)));
        caps.push((create_box(t, plate, t), at(0.0, sign * (len / 2.0 + lift), 0.0)));
    }
    merge(caps)
}

/// `buildCoreProto`: the smooth energy shell.
#[must_use]
pub fn build_core_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    create_sphere(CORE_DIAM * scale_of(recipe), 24)
}

/// `buildRingProto`: a torus in the XZ plane.
#[must_use]
pub fn build_ring_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    create_torus(RING_DIAM * s, RING_THICK * s, 28)
}

/// `extrudedPlate`: an XY outline extruded along Z into a flat-shaded plate.
#[must_use]
pub fn extruded_plate(outline: &[[f64; 2]], depth: f64, z: f64) -> VertexData {
    let count = outline.len();
    let front = z + depth / 2.0;
    let back = z - depth / 2.0;
    // Babylon keeps positions as numbers, then stores them as floats.
    let mut positions: Vec<f64> = vec![0.0, 0.0, front, 0.0, 0.0, back];
    for [x, y] in outline {
        positions.extend_from_slice(&[*x, *y, front]);
    }
    for [x, y] in outline {
        positions.extend_from_slice(&[*x, *y, back]);
    }
    let area: f64 = (0..count)
        .map(|i| {
            let [x, y] = outline[i];
            let [nx, ny] = outline[(i + 1) % count];
            x * ny - nx * y
        })
        .sum();
    let ccw = area > 0.0;
    let mut indices: Vec<u32> = Vec::new();
    for i in 0..count {
        let next = (i + 1) % count;
        let fi = (2 + i) as u32;
        let fn_ = (2 + next) as u32;
        let bi = (2 + count + i) as u32;
        let bn = (2 + count + next) as u32;
        if ccw {
            indices.extend_from_slice(&[0, fi, fn_, 1, bn, bi, fi, bi, bn, fi, bn, fn_]);
        } else {
            indices.extend_from_slice(&[0, fn_, fi, 1, bi, bn, fi, bn, bi, fi, fn_, bn]);
        }
    }
    let uvs: Vec<f32> = positions
        .chunks_exact(3)
        .flat_map(|p| [(p[0] / 32.0 + 0.5) as f32, (p[1] / 32.0 + 0.5) as f32])
        .collect();
    let mut data = VertexData {
        positions: positions.iter().map(|&v| v as f32).collect(),
        normals: Vec::new(),
        uvs,
        indices,
    };
    data.compute_normals();
    data.flat_shaded()
}

fn scaled_outline(width: f64, height: f64, points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    points.iter().map(|[x, y]| [x * width, y * height]).collect()
}

fn armor_scale(recipe: &ProceduralPickupRecipe) -> f64 {
    scale_of(recipe) * if recipe.kind == "shard" { SHARD_SIZE_RATIO } else { 1.0 }
}

/// `buildArmorFrameProto`: the dark backer between the plates.
#[must_use]
pub fn build_armor_frame_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = armor_scale(recipe);
    let w = ARMOR_WIDTH * s;
    let h = ARMOR_HEIGHT * s;
    let outline: &[[f64; 2]] = if recipe.kind == "shard" {
        &[
            [-0.42, 0.22],
            [-0.12, 0.38],
            [0.31, 0.19],
            [0.39, -0.13],
            [0.06, -0.37],
            [-0.3, -0.24],
            [-0.48, 0.02],
        ]
    } else {
        &[
            [-0.29, 0.46],
            [-0.46, 0.3],
            [-0.42, 0.02],
            [-0.29, -0.1],
            [-0.24, -0.47],
            [0.24, -0.47],
            [0.29, -0.1],
            [0.42, 0.02],
            [0.46, 0.3],
            [0.29, 0.46],
        ]
    };
    extruded_plate(&scaled_outline(w, h, outline), 5.0 * s, 0.0)
}

/// `buildArmorProto`: chest, shoulder and waist plates, front and back.
#[must_use]
pub fn build_armor_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = armor_scale(recipe);
    let w = ARMOR_WIDTH * s;
    let h = ARMOR_HEIGHT * s;
    let front_z = 3.1 * s;
    let panels: &[&[[f64; 2]]] = if recipe.kind == "shard" {
        &[&[
            [-0.35, 0.17],
            [-0.09, 0.32],
            [0.26, 0.15],
            [0.32, -0.1],
            [0.04, -0.3],
            [-0.24, -0.19],
            [-0.4, 0.02],
        ]]
    } else {
        &[
            &[
                [-0.35, 0.22],
                [-0.18, 0.36],
                [0.0, 0.25],
                [0.18, 0.36],
                [0.35, 0.22],
                [0.29, -0.09],
                [0.0, -0.24],
                [-0.29, -0.09],
            ],
            &[
                [-0.22, -0.18],
                [0.22, -0.18],
                [0.19, -0.39],
                [0.0, -0.48],
                [-0.19, -0.39],
            ],
            &[[-0.48, 0.27], [-0.31, 0.4], [-0.29, 0.1], [-0.42, 0.04]],
            &[[0.31, 0.4], [0.48, 0.27], [0.42, 0.04], [0.29, 0.1]],
        ]
    };
    let mut parts = Vec::new();
    for points in panels {
        let outline = scaled_outline(w, h, points);
        parts.push((extruded_plate(&outline, 1.35 * s, front_z), DMat4::IDENTITY));
        parts.push((extruded_plate(&outline, 1.35 * s, -front_z), DMat4::IDENTITY));
    }
    merge(parts)
}

/// `buildAmmoCrateProto`: the dark chamfered chassis and its handle.
#[must_use]
pub fn build_ammo_crate_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let (w, h, d) = (CRATE_W * s, CRATE_H * s, CRATE_D * s);
    let outline = [
        [-0.42, 0.5],
        [-0.5, 0.34],
        [-0.5, -0.34],
        [-0.42, -0.5],
        [0.42, -0.5],
        [0.5, -0.34],
        [0.5, 0.34],
        [0.42, 0.5],
    ];
    let body = extruded_plate(&scaled_outline(w, h, &outline), d, 0.0);
    let handle = create_box(w * 0.44, h * 0.12, d * 0.52);
    merge(vec![
        (body, DMat4::IDENTITY),
        bake_part(handle, DVec3::new(0.0, h * 0.55, 0.0), DVec3::ZERO),
    ])
}

/// `buildAmmoPanelProto`: front and back inserts and side rails.
#[must_use]
pub fn build_ammo_panel_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let (w, h, d) = (CRATE_W * s, CRATE_H * s, CRATE_D * s);
    let outline = [
        [-0.35, 0.36],
        [-0.41, 0.24],
        [-0.41, -0.24],
        [-0.35, -0.36],
        [0.35, -0.36],
        [0.41, -0.24],
        [0.41, 0.24],
        [0.35, 0.36],
    ];
    let mut parts = Vec::new();
    for sign in [-1.0, 1.0] {
        parts.push((
            extruded_plate(&scaled_outline(w, h, &outline), 0.7 * s, sign * (d / 2.0 + 0.46 * s)),
            DMat4::IDENTITY,
        ));
        let side = create_box(0.85 * s, h * 0.56, d * 0.58);
        parts.push(bake_part(
            side,
            DVec3::new(sign * (w / 2.0 + 0.18 * s), 0.0, 0.0),
            DVec3::ZERO,
        ));
    }
    merge(parts)
}

fn mark_box(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x0, y1], [x1, y1], [x1, y0]]
}

/// `buildAmmoMarkProto`: the type's geometric glyph on the front and back.
#[must_use]
pub fn build_ammo_mark_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let (w, h, d) = (CRATE_W * s, CRATE_H * s, CRATE_D * s);
    let mut marks: Vec<Vec<[f64; 2]>> = Vec::new();
    match recipe.kind {
        "bullets" => {
            for x in [-0.17, 0.0, 0.17] {
                marks.push(mark_box(x - 0.045, -0.22, x + 0.045, 0.22));
            }
        }
        "shells" => {
            for x in [-0.11, 0.11] {
                marks.push(mark_box(x - 0.075, -0.22, x + 0.075, 0.22));
            }
        }
        "grenades" => {
            marks.push(vec![
                [-0.2, 0.08],
                [-0.14, 0.23],
                [0.14, 0.23],
                [0.2, 0.08],
                [0.13, -0.2],
                [-0.13, -0.2],
            ]);
            marks.push(mark_box(-0.04, 0.22, 0.04, 0.32));
        }
        "rockets" => {
            marks.push(vec![
                [-0.08, -0.13],
                [-0.08, 0.13],
                [0.0, 0.31],
                [0.08, 0.13],
                [0.08, -0.13],
            ]);
            marks.push(vec![[-0.08, -0.04], [-0.24, -0.22], [-0.08, -0.18]]);
            marks.push(vec![[0.08, -0.04], [0.24, -0.22], [0.08, -0.18]]);
            marks.push(vec![[-0.05, -0.15], [0.0, -0.31], [0.05, -0.15]]);
        }
        "cells" => marks.push(vec![[0.0, 0.3], [0.25, 0.0], [0.0, -0.3], [-0.25, 0.0]]),
        "lasers" => marks.push(vec![
            [-0.02, 0.31],
            [0.2, 0.31],
            [0.035, 0.02],
            [0.19, 0.02],
            [-0.2, -0.31],
            [-0.04, -0.04],
            [-0.19, -0.04],
        ]),
        "bolts" => {
            marks.push(vec![[-0.26, 0.2], [-0.19, 0.28], [0.26, -0.2], [0.19, -0.28]]);
            marks.push(vec![[-0.26, -0.2], [-0.19, -0.28], [0.26, 0.2], [0.19, 0.28]]);
        }
        _ => marks.push(vec![
            [0.0, 0.31],
            [0.08, 0.08],
            [0.28, 0.0],
            [0.08, -0.08],
            [0.0, -0.31],
            [-0.08, -0.08],
            [-0.28, 0.0],
            [-0.08, 0.08],
        ]),
    }
    let mut parts = Vec::new();
    for sign in [-1.0, 1.0] {
        for mark in &marks {
            parts.push((
                extruded_plate(&scaled_outline(w, h, mark), 0.45 * s, sign * (d / 2.0 + 1.1 * s)),
                DMat4::IDENTITY,
            ));
        }
    }
    merge(parts)
}

/// `buildAmmoIconProto`: the icon decals on the crate's narrow sides.
#[must_use]
pub fn build_ammo_icon_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let (w, h) = (CRATE_W * s, CRATE_H * s);
    let size = w.min(h) * CRATE_ICON_FILL;
    let lift = 0.7 * s;
    let faces = [(w / 2.0 + lift, PI / 2.0), (-(w / 2.0 + lift), -PI / 2.0)];
    merge(
        faces
            .into_iter()
            .map(|(x, yaw)| {
                bake_part(
                    create_plane(size, size),
                    DVec3::new(x, 0.0, 0.0),
                    DVec3::new(0.0, yaw, 0.0),
                )
            })
            .collect(),
    )
}

/// `buildPowerupCoreProto`: the dark octagonal medallion.
#[must_use]
pub fn build_powerup_core_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let badge = create_cylinder(3.5 * s, QUAD_BADGE_DIAM * s, QUAD_BADGE_DIAM * s, 8);
    bake_part(badge, DVec3::ZERO, DVec3::new(PI / 2.0, 0.0, PI / 8.0)).0
}

/// `buildPowerupMarkProto`: a luminous Q and four corner sparks, both faces.
#[must_use]
pub fn build_powerup_mark_proto(recipe: &ProceduralPickupRecipe) -> VertexData {
    let s = scale_of(recipe);
    let mut parts = Vec::new();
    for face in [-1.0, 1.0] {
        let z = face * 2.3 * s;
        let ring = create_torus(QUAD_MARK_DIAM * s, 1.65 * s, 32);
        parts.push(bake_part(ring, DVec3::new(0.0, 0.0, z), DVec3::new(PI / 2.0, 0.0, 0.0)));
        let tail = create_box(1.8 * s, 5.2 * s, 1.2 * s);
        parts.push(bake_part(
            tail,
            DVec3::new(4.2 * s, -4.8 * s, z),
            DVec3::new(0.0, 0.0, -0.7),
        ));
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                let spark = create_box(1.4 * s, 1.4 * s, 1.1 * s);
                parts.push(bake_part(
                    spark,
                    DVec3::new(x * 6.9 * s, y * 6.9 * s, z),
                    DVec3::new(0.0, 0.0, PI / 4.0),
                ));
            }
        }
    }
    merge(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::pickups::procedural::procedural_pickup_recipes::get_procedural_pickup_recipe;

    #[test]
    fn extruded_plates_are_flat_shaded_prisms() {
        let square = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        let plate = extruded_plate(&square, 2.0, 0.0);
        // Four sides of a cap fan, front and back, and two per side wall.
        assert_eq!(plate.indices.len(), 4 * 12);
        assert_eq!(plate.vertex_count(), 48);
        // The first triangle is the front cap: it faces +Z.
        assert!((plate.normals[2].abs() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_cross_spans_its_arms() {
        let recipe = get_procedural_pickup_recipe("health_medium").unwrap();
        let cross = build_cross_proto(recipe);
        let max_x = cross.positions.chunks_exact(3).map(|p| p[0]).fold(f32::MIN, f32::max);
        let half = (CROSS_ARM * recipe.scale * PROCEDURAL_GLOBAL_SCALE) as f32;
        assert!((max_x - half).abs() < 1e-4);
        assert_eq!(cross.vertex_count(), 72);
    }
}
