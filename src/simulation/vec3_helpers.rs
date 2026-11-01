//! Vector helpers for the movement code (wash-fps `simulation/Vec3Helpers.ts`).

use shagger::shared::server::simulation::types::Vec3;

pub const VEC3_ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);

#[must_use]
pub fn vec3_add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

#[must_use]
pub fn vec3_scale(v: Vec3, s: f64) -> Vec3 {
    Vec3::new(v.x * s, v.y * s, v.z * s)
}

#[must_use]
pub fn vec3_length(v: Vec3) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

#[must_use]
pub fn vec3_length_xz(v: Vec3) -> f64 {
    (v.x * v.x + v.z * v.z).sqrt()
}

/// `vec3Normalize`: zero below a length of 0.0001.
#[must_use]
pub fn vec3_normalize(v: Vec3) -> Vec3 {
    let len = vec3_length(v);
    if len < 0.0001 {
        VEC3_ZERO
    } else {
        vec3_scale(v, 1.0 / len)
    }
}

#[must_use]
pub fn vec3_dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

#[must_use]
pub fn vec3_dot_xz(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.z * b.z
}

/// `vec3MulAdd`: `origin + dir * scale`.
#[must_use]
pub fn vec3_mul_add(origin: Vec3, dir: Vec3, scale: f64) -> Vec3 {
    Vec3::new(
        origin.x + dir.x * scale,
        origin.y + dir.y * scale,
        origin.z + dir.z * scale,
    )
}
