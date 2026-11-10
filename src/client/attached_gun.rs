//! wash-fps `client/AttachedGun.ts`: a gun held in a player's right hand,
//! its barrel resting on the supporting palm and along the body's front.
//!
//! The hand frame is the right-handed branch, as the rest of the native
//! held weapons are: shagger's glTF models are not mirrored, so the gun's
//! held frame is the browser's with its sideways (Z) axis reversed, and its
//! shape is measured in that frame.

use shagger::animation::player_model_fit::GunSupport;
use shagger::babylon::maths::math_glam::{from_euler_angles, from_unit_vectors, rotation_axis};
use shagger::loader::model::gltf_model::GltfModel;
use shagger::rendering::glam::{Mat4, Quat, Vec3};
use shagger::world::pickup::flattened_pickup_matrices;

/// An assembled gun in its held frame before the grip's scale: barrel along
/// +Y, top toward +X (`GunShape`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GunShape {
    /// The muzzle tag, on the bore line; None when the model has none.
    pub muzzle: Option<Vec3>,
    /// How far the gun reaches behind the grip along the barrel (zero or
    /// less).
    pub back: f32,
}

/// How far below its bore line, in gun model units, a gun rests on the
/// supporting palm (`BORE_ABOVE_PALM`).
pub const BORE_ABOVE_PALM: f32 = 4.3;

/// `handedTurn`: the right-handed branch, `FromEulerAngles(PI, 0, -PI/2)`.
#[must_use]
pub fn handed_turn() -> Quat {
    from_euler_angles(std::f32::consts::PI, 0.0, -std::f32::consts::FRAC_PI_2)
}

/// `measureGunShape`: the muzzle and the rear end in the held frame.
#[must_use]
pub fn measure_gun_shape(model: &GltfModel) -> GunShape {
    let held = Mat4::from_quat(handed_turn());
    let mut back = 0.0_f32;
    for (primitive, world) in model.primitives.iter().zip(flattened_pickup_matrices(model, held)) {
        for p in &primitive.positions {
            back = back.min(world.transform_point3(Vec3::from(*p)).y);
        }
    }
    let muzzle = model
        .attachment_points
        .get("tag_flash")
        .map(|tag| held.transform_point3(tag.position));
    GunShape { muzzle, back }
}

/// `gunHold`: the held gun's rotation and position in the weapon hand's
/// frame. The gun turns so its bore line passes [`BORE_ABOVE_PALM`] above
/// the supporting palm, then swings about its own up axis, around its rear
/// end, until the barrel points along the body's front.
#[must_use]
pub fn gun_hold(shape: Option<&GunShape>, scale: f32, support: &GunSupport) -> (Quat, Vec3) {
    let reach = support.palm.length();
    let mut resting = Vec3::Y;
    if let Some(muzzle) = shape.and_then(|s| s.muzzle) {
        let below = (muzzle.x - BORE_ABOVE_PALM) * scale;
        let across = muzzle.z * scale;
        let along = (reach * reach - below * below - across * across)
            .max(reach * reach / 4.0)
            .sqrt();
        resting = Vec3::new(below, along, across).normalize();
    }
    let rest = from_unit_vectors(resting, support.palm * (1.0 / reach));
    let up = rest * Vec3::X;
    let flatten = |v: Vec3| (v - up * v.dot(up)).normalize();
    let barrel = flatten(rest * Vec3::Y);
    let front = flatten(support.forward);
    let yaw = barrel.cross(front).dot(up).atan2(barrel.dot(front));
    let swing = rotation_axis(up, if yaw.is_finite() { yaw } else { 0.0 });
    let rear = rest * Vec3::new(0.0, shape.map_or(0.0, |s| s.back) * scale, 0.0);
    (swing * rest, rear - swing * rear)
}

/// `holdGunGrip`: the grip's matrix in the weapon hand's frame, at half the
/// view model's scale over the body's; on the support palm when the body
/// has one, else just turned along the fingers.
#[must_use]
pub fn hold_gun_grip(
    shape: Option<&GunShape>,
    visual_scale: f32,
    model_scale: f32,
    support: Option<&GunSupport>,
) -> Mat4 {
    let scale = visual_scale / model_scale * 0.5;
    let handed = handed_turn();
    let (rotation, position) = match support {
        Some(support) => {
            let (turn, position) = gun_hold(shape, scale, support);
            // The hold is in the weapon hand's frame, after the handedness turn.
            (turn * handed, position)
        }
        None => (handed, Vec3::ZERO),
    };
    Mat4::from_scale_rotation_translation(Vec3::splat(scale), rotation, position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bore_rests_above_the_palm_and_points_forward() {
        let support = GunSupport {
            palm: Vec3::new(0.0, 0.3, 0.1),
            forward: Vec3::new(0.0, 0.2, 1.0).normalize(),
        };
        let shape = GunShape {
            muzzle: Some(Vec3::new(4.3, 40.0, 0.0)),
            back: -10.0,
        };
        let scale = 0.01;
        let (turn, position) = gun_hold(Some(&shape), scale, &support);
        // Before the swing the bore line (on the muzzle's x) aims at the palm.
        let rest = from_unit_vectors(Vec3::Y, support.palm.normalize());
        assert!((rest * Vec3::Y - support.palm.normalize()).length() < 1e-5);
        let dir = turn * Vec3::Y;
        // The rear end stays put while the barrel swings to the front.
        let rear = Vec3::new(0.0, -10.0 * scale, 0.0);
        assert!((position + turn * rear - rest * rear).length() < 1e-5);
        let up = turn * Vec3::X;
        let flat = |v: Vec3| (v - up * v.dot(up)).normalize();
        assert!(flat(dir).dot(flat(support.forward)) > 0.9999);
    }

    #[test]
    fn without_support_the_gun_lies_along_the_fingers() {
        let m = hold_gun_grip(None, 3.6, 36.0, None);
        let barrel = m.transform_vector3(Vec3::X).normalize();
        assert!((barrel - Vec3::Y).length() < 1e-5, "{barrel}");
    }
}
