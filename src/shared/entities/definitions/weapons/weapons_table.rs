//! wash-fps `shared/entities/definitions/weapons/WeaponsTable.ts` and the
//! weapon definitions it collects (`blade.ts`, `electrobolt.ts`,
//! `glauncher.ts`, `machinegun.ts`, `plasmagun.ts`, `riotgun.ts`,
//! `rlauncher.ts`, `voltaic.ts`).
//!
//! Only what drawing a weapon in the world needs so far: the model, its
//! scale and its barrel sub-models. Stats, ADS and view-model placement come
//! with the systems that use them.

/// A barrel sub-model (`BarrelDef`): its model and its offset from the mount.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarrelModel {
    pub model_path: &'static str,
    pub offset: Option<[f32; 3]>,
}

/// The model half of a `WeaponDef`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponModel {
    pub id: &'static str,
    pub model_path: &'static str,
    pub scale: f32,
    pub barrels: &'static [BarrelModel],
}

pub const BLADE: WeaponModel = WeaponModel {
    id: "blade",
    model_path: "/data/models/weapons/blade/arc_blade.glb",
    scale: 1.0,
    barrels: &[],
};

pub const ELECTROBOLT: WeaponModel = WeaponModel {
    id: "electrobolt",
    model_path: "/data/models/weapons/electrobolt/electrobolt.glb",
    scale: 2.0,
    barrels: &[],
};

pub const GRENADE_LAUNCHER: WeaponModel = WeaponModel {
    id: "glauncher",
    model_path: "/data/models/weapons/glauncher/glauncher.glb",
    scale: 2.0,
    barrels: &[BarrelModel {
        model_path: "/data/models/weapons/glauncher/glauncher_barrel.glb",
        offset: None,
    }],
};

pub const MACHINEGUN: WeaponModel = WeaponModel {
    id: "machinegun",
    model_path: "/data/models/weapons/machinegun/machinegun.glb",
    scale: 2.0,
    barrels: &[BarrelModel {
        model_path: "/data/models/weapons/machinegun/machinegun_barrel.glb",
        offset: None,
    }],
};

/// Dual counter-rotating chambers, side by side.
pub const PLASMAGUN: WeaponModel = WeaponModel {
    id: "plasmagun",
    model_path: "/data/models/weapons/plasmagun/plasmagun.glb",
    scale: 2.0,
    barrels: &[
        BarrelModel {
            model_path: "/data/models/weapons/plasmagun/plasmagun_barrel.glb",
            offset: Some([0.0, 0.0, -4.0]),
        },
        BarrelModel {
            model_path: "/data/models/weapons/plasmagun/plasmagun_barrel.glb",
            offset: Some([0.0, 0.0, 0.0]),
        },
    ],
};

pub const RIOTGUN: WeaponModel = WeaponModel {
    id: "riotgun",
    model_path: "/data/models/weapons/riotgun/riotgun.glb",
    scale: 2.0,
    barrels: &[],
};

pub const ROCKET_LAUNCHER: WeaponModel = WeaponModel {
    id: "rlauncher",
    model_path: "/data/models/weapons/rlauncher/rlauncher.glb",
    scale: 2.0,
    barrels: &[],
};

pub const VOLTAIC: WeaponModel = WeaponModel {
    id: "voltaic",
    model_path: "/data/models/weapons/voltaic/voltaic.glb",
    scale: 2.0,
    barrels: &[],
};

/// `WEAPONS[id]`, with `lasergun` the compatibility id of the voltaic.
#[must_use]
pub fn weapon_model(id: &str) -> Option<&'static WeaponModel> {
    const ALL: [&WeaponModel; 8] = [
        &MACHINEGUN,
        &RIOTGUN,
        &GRENADE_LAUNCHER,
        &ROCKET_LAUNCHER,
        &PLASMAGUN,
        &VOLTAIC,
        &ELECTROBOLT,
        &BLADE,
    ];
    if id == "lasergun" {
        return Some(&VOLTAIC);
    }
    ALL.into_iter().find(|w| w.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::bsp::pickup_table::CLASSNAME_TO_WEAPON;

    #[test]
    fn every_map_weapon_has_a_model() {
        for (classname, id) in CLASSNAME_TO_WEAPON {
            let model = weapon_model(id).unwrap_or_else(|| panic!("{classname}"));
            assert!(model.model_path.ends_with(".glb"));
        }
        assert_eq!(weapon_model("lasergun"), Some(&VOLTAIC));
        assert_eq!(weapon_model("nothing"), None);
    }
}
