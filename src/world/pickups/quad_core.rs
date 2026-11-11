//! wash-fps `world/pickups/QuadCore.ts`: the quad's Q turns inside its ring,
//! glowing in its own colours.
//!
//! Babylon lifts the model's innermost part out as a mesh of its own under
//! the item, turned about its centre in the model's plane, with a copy of
//! the material that emits its albedo. Here the core is a primitive
//! appended to the model (the rest keeps every other triangle), drawn about
//! its centre, and its glow an added-light copy of it.

use shagger::loader::model::gltf_model::{GltfModel, ModelMaterial, ModelPrimitive};
use shagger::rendering::glam::{Mat4, Vec3};

use super::health_tint::innermost_part;

/// The core turns once every this many seconds, in the badge's own plane.
pub const SPIN_PERIOD: f64 = 3.0;
/// How brightly the core glows in its own colours: fullbright, a little
/// over for bloom.
pub const CORE_GLOW: f32 = 1.4;

/// `splitCore`'s parts.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitCore {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// The centre it turns about, in the model's own space.
    pub centre: [f32; 3],
    /// Every other triangle, for the rest of the model.
    pub rest: Vec<u32>,
}

/// The model's innermost part as a mesh of its own: its vertices renumbered
/// from 0 about its centre, and the centre it turns about. The rest of the
/// model keeps every other triangle.
#[must_use]
pub fn split_core(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u32],
) -> Option<SplitCore> {
    let triangles = innermost_part(positions, indices);
    if triangles.is_empty() || triangles.len() * 3 == indices.len() {
        return None;
    }
    let in_core: std::collections::HashSet<usize> = triangles.into_iter().collect();
    let mut remap = std::collections::HashMap::new();
    let mut out = SplitCore {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
        centre: [0.0; 3],
        rest: Vec::new(),
    };
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for t in (0..indices.len() - indices.len() % 3).step_by(3) {
        if !in_core.contains(&t) {
            out.rest.extend_from_slice(&indices[t..t + 3]);
            continue;
        }
        for &v in &indices[t..t + 3] {
            let at = *remap.entry(v).or_insert_with(|| {
                let v = v as usize;
                let p = positions[v];
                for a in 0..3 {
                    min[a] = min[a].min(p[a]);
                    max[a] = max[a].max(p[a]);
                }
                out.positions.push(p);
                if let Some(n) = normals.get(v) {
                    out.normals.push(*n);
                }
                if let Some(uv) = uvs.get(v) {
                    out.uvs.push(*uv);
                }
                out.positions.len() as u32 - 1
            });
            out.indices.push(at);
        }
    }
    out.centre = std::array::from_fn(|a| (min[a] + max[a]) / 2.0);
    // The core's vertices sit about its own centre, so it turns in place
    for p in &mut out.positions {
        for (x, c) in p.iter_mut().zip(out.centre) {
            *x -= c;
        }
    }
    Some(out)
}

/// A model with its first primitive's core split out (`addSpinningCore`).
pub struct SpinningCore {
    /// The model: the first primitive without its core, the core appended
    /// last.
    pub model: GltfModel,
    /// The core's primitive in [`Self::model`].
    pub primitive: usize,
    pub centre: Vec3,
    /// The core alone, its material emitting its albedo, to draw as added
    /// light at [`CORE_GLOW`].
    pub glow: GltfModel,
}

/// Splits the first primitive's innermost part out to turn on its own, or
/// None when the model has no separate core.
#[must_use]
pub fn add_spinning_core(model: &GltfModel) -> Option<SpinningCore> {
    let root = model.primitives.first()?;
    let split = split_core(&root.positions, &root.normals, &root.uvs, &root.indices)?;
    let mut out = model.clone();
    out.primitives[0].indices = split.rest;
    let core = ModelPrimitive {
        name: format!("{}_core", root.name),
        positions: split.positions,
        normals: split.normals,
        uvs: split.uvs,
        indices: split.indices,
        bounds_center: Vec3::ZERO,
        creation_order: model.primitives.len(),
        node_scale: Vec3::ONE,
        node_matrix: Mat4::IDENTITY,
        skin: None,
        ..root.clone()
    };
    out.primitives.push(core.clone());
    let primitive = out.primitives.len() - 1;
    // `glowingMaterial`: the source's albedo as its emissive texture.
    let materials: Vec<ModelMaterial> = model
        .materials
        .iter()
        .map(|m| ModelMaterial {
            name: format!("{}_core", m.name),
            base_color: [1.0; 4],
            metallic_roughness_image: None,
            normal_image: None,
            emissive_image: None,
            ..m.clone()
        })
        .collect();
    let glow = GltfModel {
        primitives: vec![ModelPrimitive {
            creation_order: 0,
            ..core
        }],
        materials,
        images: model.images.clone(),
        attachment_points: Default::default(),
        skeleton: Default::default(),
    };
    Some(SpinningCore {
        model: out,
        primitive,
        centre: Vec3::from(split.centre),
        glow,
    })
}

/// The core's turn at `seconds` (`sceneSpin`), every core together.
#[must_use]
pub fn core_angle(seconds: f64) -> f64 {
    (seconds * std::f64::consts::PI * 2.0) / SPIN_PERIOD
}

/// Where the core draws under its model's `root` matrix at `seconds`.
#[must_use]
pub fn core_matrix(root: Mat4, centre: Vec3, seconds: f64) -> Mat4 {
    root * Mat4::from_translation(centre) * Mat4::from_rotation_z(core_angle(seconds) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_splits_out_about_its_centre() {
        let quad = |s: f32, x: f32| [[x - s, -s, 0.0], [x + s, -s, 0.0], [x + s, s, 0.0], [x - s, s, 0.0]];
        let mut positions = quad(1.0, 0.0).to_vec();
        positions.extend(quad(0.25, 0.1));
        let normals = vec![[0.0, 0.0, 1.0]; 8];
        let uvs = vec![[0.5, 0.5]; 8];
        let indices = [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let split = split_core(&positions, &normals, &uvs, &indices).unwrap();
        assert_eq!(split.rest, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(split.indices, vec![0, 1, 2, 0, 2, 3]);
        assert!((split.centre[0] - 0.1).abs() < 1e-6);
        assert!((split.positions[0][0] + 0.25).abs() < 1e-6);
        assert_eq!(split.normals.len(), 4);
        // A model that is all core has nothing to split.
        assert!(split_core(&positions[..4], &normals[..4], &uvs[..4], &indices[..6]).is_none());
        assert!((core_angle(1.5) - std::f64::consts::PI).abs() < 1e-12);
    }
}
