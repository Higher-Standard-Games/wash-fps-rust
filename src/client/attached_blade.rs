//! wash-fps `client/AttachedBlade.ts`: the blade in a player's right hand,
//! laid along the fingers, or standing up from the fist in the blade
//! stance.
//!
//! The right-handed branch, as [`super::attached_gun`]: the blade's held
//! frame is the browser's with X reversed, which keeps its length, its thin
//! axis and its guard where they were.

use shagger::animation::player_model_fit::BladeStance;
use shagger::babylon::maths::math_glam::{from_euler_angles, rotation_from_axes};
use shagger::loader::model::gltf_model::GltfModel;
use shagger::rendering::glam::{Mat4, Quat, Vec3};

/// The packaged export's handle centre, in model units along X.
pub const BLADE_GRIP_OFFSET: f32 = 0.45;

/// A blade laid along the fingers (+Y), in its grip's units before scale
/// (`BladeShape`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BladeShape {
    /// The normal of its flat faces: +X or +Z.
    pub thin: Vec3,
    /// How far along +Y a fist holds it: three quarters of the way from the
    /// pommel to the guard.
    pub hold_at: f32,
}

impl Default for BladeShape {
    fn default() -> Self {
        Self {
            thin: Vec3::Z,
            hold_at: 0.0,
        }
    }
}

/// `alongFingers`: `FromEulerAngles(PI, 0, -PI/2)` with the model's X kept.
#[must_use]
pub fn along_fingers() -> Quat {
    from_euler_angles(std::f32::consts::PI, 0.0, -std::f32::consts::FRAC_PI_2)
}

/// The grip offset's matrix (`blade_grip_offset`).
#[must_use]
pub fn blade_offset() -> Mat4 {
    Mat4::from_translation(Vec3::new(BLADE_GRIP_OFFSET, 0.0, 0.0))
}

/// `measureBladeShape`: the thin axis and the fist's place, with the grip
/// at the origin. The guard is the widest point in the lowest third.
#[must_use]
pub fn measure_blade_shape(model: &GltfModel) -> BladeShape {
    let held = Mat4::from_quat(along_fingers()) * blade_offset();
    let points: Vec<Vec3> = model
        .primitives
        .iter()
        .flat_map(|p| {
            let world = held * p.node_matrix;
            p.positions.iter().map(move |v| world.transform_point3(Vec3::from(*v)))
        })
        .collect();
    let min = points.iter().fold(Vec3::splat(f32::INFINITY), |m, p| m.min(*p));
    let max = points.iter().fold(Vec3::splat(f32::NEG_INFINITY), |m, p| m.max(*p));
    let thin = if max.x - min.x <= max.z - min.z {
        Vec3::X
    } else {
        Vec3::Z
    };
    if points.is_empty() {
        return BladeShape { thin, hold_at: 0.0 };
    }
    let lowest = min.y + (max.y - min.y) / 3.0;
    let mut guard = min.y;
    let mut widest = -1.0_f32;
    for p in &points {
        if p.y > lowest {
            continue;
        }
        let width = p.x.hypot(p.z);
        if width > widest {
            widest = width;
            guard = p.y;
        }
    }
    BladeShape {
        thin,
        hold_at: min.y + (guard - min.y) * 0.75,
    }
}

/// `bladeStanceTurn`: carries a blade laid along the fingers (thin across
/// `thin`) onto `axis` with its flat faces toward `side`, so an edge leads.
#[must_use]
pub fn blade_stance_turn(thin: Vec3, axis: Vec3, side: Vec3) -> Quat {
    let length = Vec3::Y;
    let from = rotation_from_axes(thin, length.cross(thin), length);
    let to = rotation_from_axes(side, axis.cross(side), axis);
    to * from.inverse()
}

/// `holdBladeGrip` and `setBladeStance`: the grip's matrix in the weapon
/// hand's frame (offset included), at the view model's scale over the
/// body's: upright from the fist in the stance, else along the fingers.
#[must_use]
pub fn hold_blade_grip(shape: &BladeShape, visual_scale: f32, model_scale: f32, stance: Option<&BladeStance>) -> Mat4 {
    let scale = visual_scale / model_scale;
    let along = along_fingers();
    let (rotation, position) = match stance {
        Some(stance) => (
            blade_stance_turn(shape.thin, stance.axis, stance.side) * along,
            stance.fist - stance.axis * (shape.hold_at * scale),
        ),
        None => (along, Vec3::ZERO),
    };
    Mat4::from_scale_rotation_translation(Vec3::splat(scale), rotation, position) * blade_offset()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stance_turn_stands_the_blade_on_its_axis() {
        let axis = Vec3::new(0.0, 0.8, 0.6);
        let side = Vec3::X;
        let turn = blade_stance_turn(Vec3::Z, axis, side);
        assert!((turn * Vec3::Y - axis).length() < 1e-5);
        assert!((turn * Vec3::Z - side).length() < 1e-5);
    }

    #[test]
    fn the_fist_holds_the_handle_below_the_guard() {
        let stance = BladeStance {
            axis: Vec3::Y,
            side: Vec3::X,
            fist: Vec3::new(0.1, 0.2, 0.3),
        };
        let shape = BladeShape {
            thin: Vec3::X,
            hold_at: 2.0,
        };
        let m = hold_blade_grip(&shape, 3.6, 36.0, Some(&stance));
        // The point hold_at up the blade (before the offset) sits in the fist.
        let held = m * blade_offset().inverse();
        let at = held.transform_point3(Quat::inverse(along_fingers()) * Vec3::new(0.0, 2.0, 0.0));
        assert!((at - stance.fist).length() < 1e-5, "{at}");
    }
}
