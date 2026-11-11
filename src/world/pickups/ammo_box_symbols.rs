//! wash-fps `world/pickups/AmmoBoxSymbols.ts`: an ammo box shows its
//! weapon's symbol on each upright side.
//!
//! Babylon parents four unlit quads to the box model, just off its sides,
//! so they turn, bob and hide with it. Here they are one model of four
//! quads in the box's own space, drawn under the box's matrix.

use shagger::babylon::materials::standard_material::{StandardMaterialParams, TextureUse};
use shagger::loader::model::gltf_model::{AlphaMode, GltfModel, ModelMaterial, ModelPrimitive, Skeleton};
use shagger::loader::texture::DecodedTexture;
use shagger::rendering::glam::{Mat4, Vec3};

use super::blue_glow::png_image;

/// The weapon symbol each ammo type shows, from `washoi_gfx.pk3`.
const AMMO_SYMBOLS: [(&str, &str); 7] = [
    ("ammo_bullets", "machinegun"),
    ("ammo_shells", "riot"),
    ("ammo_grenades", "grenade"),
    ("ammo_rockets", "rocket"),
    ("ammo_plasmagun", "plasma"),
    ("ammo_lasers", "laser"),
    ("ammo_bolts", "electro"),
];

/// The symbol's width as a share of the side it sits on.
const SYMBOL_SIZE: f32 = 0.7;
/// How far out from the side the symbol floats, in the model's own units,
/// so it never flickers into it.
const SYMBOL_LIFT: f32 = 0.004;

/// The weapon symbol for an ammo id, or None for anything that is not ammo.
#[must_use]
pub fn ammo_symbol_path(ammo_id: &str) -> Option<String> {
    AMMO_SYMBOLS
        .iter()
        .find(|(id, _)| *id == ammo_id)
        .map(|(_, symbol)| format!("gfx/simpleitems/weapon/{symbol}"))
}

/// `createSymbolMaterial`: unlit, its texture as diffuse and emissive, alpha
/// blended by the texture's alpha.
#[must_use]
pub fn symbol_material() -> StandardMaterialParams {
    StandardMaterialParams {
        disable_lighting: true,
        diffuse_color: Vec3::ZERO,
        specular_color: Vec3::ZERO,
        emissive_color: Vec3::ONE,
        alpha_blend: true,
        texture: TextureUse {
            diffuse: true,
            alpha_from_diffuse: true,
            emissive: true,
            opacity_rgb: false,
        },
        ..StandardMaterialParams::default()
    }
}

/// The four sides' quads, as corner positions (bottom left, bottom right,
/// top right, top left as seen from outside), for a box with these bounds.
#[must_use]
pub fn symbol_quads(min: Vec3, max: Vec3) -> [[Vec3; 4]; 4] {
    let center_y = (min.y + max.y) / 2.0;
    let mid_x = (min.x + max.x) / 2.0;
    let mid_z = (min.z + max.z) / 2.0;
    let width = max.x - min.x;
    let depth = max.z - min.z;
    // Each side: its centre, the way it faces, and how wide it is.
    let sides = [
        (Vec3::new(max.x + SYMBOL_LIFT, center_y, mid_z), Vec3::X, depth),
        (Vec3::new(min.x - SYMBOL_LIFT, center_y, mid_z), Vec3::NEG_X, depth),
        (Vec3::new(mid_x, center_y, max.z + SYMBOL_LIFT), Vec3::Z, width),
        (Vec3::new(mid_x, center_y, min.z - SYMBOL_LIFT), Vec3::NEG_Z, width),
    ];
    sides.map(|(centre, facing, across)| {
        let half = across * SYMBOL_SIZE / 2.0;
        // Babylon's plane turned to face the side: its +X runs this way,
        // to the right as seen from outside in Babylon's left-handed space.
        let right = facing.cross(Vec3::Y) * half;
        let up = Vec3::Y * half;
        [
            centre - right - up,
            centre + right - up,
            centre + right + up,
            centre - right + up,
        ]
    })
}

/// The symbols of one ammo type for a box model: four quads in the box's
/// first primitive's space, textured with the symbol.
#[must_use]
pub fn ammo_box_symbols(box_model: &GltfModel, symbol: &DecodedTexture) -> Option<GltfModel> {
    let root = box_model.primitives.first()?;
    let (min, max) = root.positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))),
    );
    if !min.is_finite() || !max.is_finite() {
        return None;
    }
    let image = png_image("ammo_symbol", &symbol.pixels, symbol.width, symbol.height)?;
    let primitives = symbol_quads(min, max)
        .iter()
        .enumerate()
        .map(|(i, corners)| {
            let normal = (corners[3] - corners[0]).cross(corners[1] - corners[0]).normalize();
            ModelPrimitive {
                name: format!("{}_symbol_{i}", root.name),
                node: 0,
                node_scale: Vec3::ONE,
                node_matrix: Mat4::IDENTITY,
                positions: corners.iter().map(|c| c.to_array()).collect(),
                normals: vec![normal.to_array(); 4],
                uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                // Clockwise seen from outside: Babylon's front faces.
                indices: vec![0, 2, 1, 0, 3, 2],
                material: Some(0),
                bounds_center: (corners[0] + corners[2]) / 2.0,
                creation_order: i,
                skin: None,
            }
        })
        .collect();
    Some(GltfModel {
        primitives,
        materials: vec![ModelMaterial {
            name: format!("ammo_symbol_{}", symbol.path),
            base_color: [1.0; 4],
            base_color_image: Some(0),
            metallic: 0.0,
            roughness: 1.0,
            metallic_roughness_image: None,
            normal_image: None,
            emissive: [0.0; 3],
            emissive_image: None,
            alpha_mode: AlphaMode::Blend,
            alpha_cutoff: 0.5,
            double_sided: false,
        }],
        images: vec![image],
        attachment_points: Default::default(),
        skeleton: Skeleton::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ammo_type_has_its_weapon_symbol() {
        assert_eq!(
            ammo_symbol_path("ammo_plasmagun").as_deref(),
            Some("gfx/simpleitems/weapon/plasma")
        );
        assert_eq!(ammo_symbol_path("health_small"), None);
    }

    #[test]
    fn the_symbols_sit_just_off_each_side() {
        let quads = symbol_quads(Vec3::splat(-0.5), Vec3::splat(0.5));
        // The +X side: off the face, 0.7 of its width, upright.
        let side = quads[0];
        assert!(side.iter().all(|c| (c.x - 0.504).abs() < 1e-6));
        assert!(((side[1] - side[0]).length() - 0.7).abs() < 1e-6);
        assert!(((side[3] - side[0]).y - 0.7).abs() < 1e-6);
        // Its right runs along +Z, as Babylon's plane turned by -PI/2.
        assert!(side[1].z > side[0].z);
        for quad in quads {
            let centre = (quad[0] + quad[2]) / 2.0;
            assert!((centre.length() - 0.504).abs() < 1e-6);
        }
    }
}
