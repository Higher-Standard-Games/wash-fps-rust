//! wash-fps `shared/items/WeaponBoxes.ts`: each weapon's collision box as
//! its model is drawn on the ground: the union of its meshes' POSITION
//! bounds in the GLB times the weapon's scale (the models carry no node
//! transforms, and world weapons draw their meshes unparented from the glTF
//! root). The barrel points along +x.
//!
//! `centre` is the box's centre in the model's own space: a body at pose P
//! draws its model at P.position − P.rotation · centre.

/// `WeaponBox`: the box's size and its centre in model space (units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponBox {
    pub size: [f64; 3],
    pub centre: [f64; 3],
}

const fn weapon_box_of(size: [f64; 3], centre: [f64; 3]) -> WeaponBox {
    WeaponBox { size, centre }
}

/// `WEAPON_BOXES`.
const WEAPON_BOXES: &[(&str, WeaponBox)] = &[
    ("machinegun", weapon_box_of([99.2, 31.8, 9.4], [21.8, 3.3, 0.0])),
    ("riotgun", weapon_box_of([82.0, 27.1, 10.0], [21.0, 7.0, 0.0])),
    ("glauncher", weapon_box_of([67.6, 26.9, 13.5], [31.6, 7.6, 0.0])),
    ("rlauncher", weapon_box_of([85.6, 24.3, 14.0], [29.0, 6.9, 0.0])),
    ("plasmagun", weapon_box_of([79.0, 28.6, 17.2], [20.5, 7.7, 0.0])),
    ("voltaic", weapon_box_of([71.2, 26.0, 10.1], [23.2, 7.0, 0.0])),
    ("lasergun", weapon_box_of([71.2, 26.0, 10.1], [23.2, 7.0, 0.0])),
    ("electrobolt", weapon_box_of([95.9, 27.7, 14.2], [27.9, 7.2, 0.0])),
];

/// `DEFAULT_BOX`: a gun of middling size, for a weapon without a measured box.
const DEFAULT_BOX: WeaponBox = weapon_box_of([80.0, 27.0, 12.0], [24.0, 6.0, 0.0]);

/// `weaponBox`.
#[must_use]
pub fn weapon_box(weapon_id: &str) -> WeaponBox {
    WEAPON_BOXES
        .iter()
        .find(|(id, _)| *id == weapon_id)
        .map_or(DEFAULT_BOX, |(_, b)| *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_measured_gun_has_its_box_and_any_other_the_default() {
        assert_eq!(weapon_box("rlauncher").centre, [29.0, 6.9, 0.0]);
        assert_eq!(weapon_box("lasergun"), weapon_box("voltaic"));
        assert_eq!(weapon_box("blade"), DEFAULT_BOX);
    }
}
