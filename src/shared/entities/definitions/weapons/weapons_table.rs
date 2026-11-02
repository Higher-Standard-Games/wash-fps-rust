//! wash-fps `shared/entities/definitions/weapons/WeaponsTable.ts` and the
//! weapon definitions it collects (`blade.ts`, `electrobolt.ts`,
//! `glauncher.ts`, `machinegun.ts`, `plasmagun.ts`, `riotgun.ts`,
//! `rlauncher.ts`, `voltaic.ts`).
//!
//! Only what drawing a weapon needs so far: the model, its scale, its
//! barrel sub-models and its view-model placement (`viewModel`, `ads.time`,
//! `adsRequireReady`). Stats come with the systems that use them.

/// A barrel sub-model (`BarrelDef`): its model and its offset from the mount.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarrelModel {
    pub model_path: &'static str,
    pub offset: Option<[f32; 3]>,
}

/// A GLB view model's placement (`viewModel`) and its aim (`ads.time`,
/// `adsRequireReady`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewModelPlacement {
    pub hip_offset: [f64; 3],
    pub hip_rotation: [f64; 3],
    pub ads_offset: [f64; 3],
    pub scale: f64,
    pub ads_time: f64,
    pub ads_require_ready: bool,
}

/// Most guns' placement: low and to the hand, the sights a little higher
/// and closer when aimed.
const fn gun_view(ads_y: f64, ads_time: f64, ads_require_ready: bool) -> ViewModelPlacement {
    ViewModelPlacement {
        hip_offset: [0.0, -25.0, -1.0],
        hip_rotation: [0.0, 0.0, 0.0],
        ads_offset: [0.0, ads_y, -5.0],
        scale: 2.0,
        ads_time,
        ads_require_ready,
    }
}

/// The model half of a `WeaponDef`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponModel {
    pub id: &'static str,
    pub model_path: &'static str,
    pub scale: f32,
    pub barrels: &'static [BarrelModel],
    pub view: ViewModelPlacement,
}

pub const BLADE: WeaponModel = WeaponModel {
    id: "blade",
    model_path: "/data/models/weapons/blade/arc_blade.glb",
    scale: 1.0,
    barrels: &[],
    view: ViewModelPlacement {
        hip_offset: [10.0, -28.0, 10.0],
        hip_rotation: [
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_6 + std::f64::consts::PI,
        ],
        ads_offset: [10.0, -28.0, 10.0],
        scale: 30.0,
        ads_time: 0.08,
        ads_require_ready: false,
    },
};

pub const ELECTROBOLT: WeaponModel = WeaponModel {
    id: "electrobolt",
    model_path: "/data/models/weapons/electrobolt/electrobolt.glb",
    scale: 2.0,
    barrels: &[],
    view: gun_view(-21.0, 0.375, true),
};

pub const GRENADE_LAUNCHER: WeaponModel = WeaponModel {
    id: "glauncher",
    model_path: "/data/models/weapons/glauncher/glauncher.glb",
    scale: 2.0,
    barrels: &[BarrelModel {
        model_path: "/data/models/weapons/glauncher/glauncher_barrel.glb",
        offset: None,
    }],
    view: gun_view(-20.0, 0.16, true),
};

pub const MACHINEGUN: WeaponModel = WeaponModel {
    id: "machinegun",
    model_path: "/data/models/weapons/machinegun/machinegun.glb",
    scale: 2.0,
    barrels: &[BarrelModel {
        model_path: "/data/models/weapons/machinegun/machinegun_barrel.glb",
        offset: None,
    }],
    view: gun_view(-20.0, 0.08, false),
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
    view: gun_view(-22.0, 0.08, false),
};

pub const RIOTGUN: WeaponModel = WeaponModel {
    id: "riotgun",
    model_path: "/data/models/weapons/riotgun/riotgun.glb",
    scale: 2.0,
    barrels: &[],
    view: gun_view(-21.0, 0.08, false),
};

pub const ROCKET_LAUNCHER: WeaponModel = WeaponModel {
    id: "rlauncher",
    model_path: "/data/models/weapons/rlauncher/rlauncher.glb",
    scale: 2.0,
    barrels: &[],
    view: gun_view(-20.0, 0.16, true),
};

pub const VOLTAIC: WeaponModel = WeaponModel {
    id: "voltaic",
    model_path: "/data/models/weapons/voltaic/voltaic.glb",
    scale: 2.0,
    barrels: &[],
    view: gun_view(-21.0, 0.08, false),
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

/// `WEAPONS[id]?.speedBonus ?? 0`: the weapon's run speed bonus, which the
/// server applies per entity and prediction must match (blade +10, riot gun
/// +5, electrobolt and rocket launcher -25).
#[must_use]
pub fn weapon_speed_bonus(id: &str) -> f64 {
    match id {
        "blade" => 10.0,
        "riotgun" => 5.0,
        "electrobolt" | "rlauncher" => -25.0,
        _ => 0.0,
    }
}

/// `WEAPONS[id]?.reloadModel`: `clip` guns reload a magazine, `pool` ones
/// draw from their ammo, the blade never reloads.
#[must_use]
pub fn weapon_reload_model(id: &str) -> Option<&'static str> {
    match id {
        "machinegun" | "plasmagun" | "riotgun" | "voltaic" => Some("clip"),
        "electrobolt" | "glauncher" | "rlauncher" => Some("pool"),
        "blade" => Some("none"),
        _ => None,
    }
}

/// `WEAPONS[id]?.clipSize`: a clip gun's magazine (0 for the others).
#[must_use]
pub fn weapon_clip_size(id: &str) -> Option<f64> {
    match id {
        "machinegun" => Some(30.0),
        "plasmagun" => Some(50.0),
        "riotgun" => Some(8.0),
        "voltaic" | "lasergun" => Some(120.0),
        "blade" | "electrobolt" | "glauncher" | "rlauncher" => Some(0.0),
        _ => None,
    }
}

/// `WEAPONS[id]?.maxAmmo`: the most a weapon carries (a clip gun's reserve).
#[must_use]
pub fn weapon_max_ammo(id: &str) -> Option<f64> {
    match id {
        "blade" => Some(1.0),
        "electrobolt" => Some(10.0),
        "glauncher" | "rlauncher" => Some(20.0),
        "machinegun" => Some(120.0),
        "plasmagun" => Some(150.0),
        "riotgun" => Some(24.0),
        "voltaic" | "lasergun" => Some(60.0),
        _ => None,
    }
}

/// `CrosshairConfig`'s style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrosshairStyle {
    Cross,
    Dot,
    Circle,
    CrossDot,
}

/// `CrosshairConfig`: the reticle a weapon draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrosshairConfig {
    pub style: CrosshairStyle,
    pub base_gap: f64,
    pub line_length: f64,
    pub line_width: f64,
}

const fn crosshair(style: CrosshairStyle, base_gap: f64, line_length: f64, line_width: f64) -> CrosshairConfig {
    CrosshairConfig {
        style,
        base_gap,
        line_length,
        line_width,
    }
}

/// `WEAPONS[id]?.crosshair`.
#[must_use]
pub fn weapon_crosshair(id: &str) -> Option<CrosshairConfig> {
    use CrosshairStyle::{Cross, CrossDot, Dot};
    match id {
        "blade" => Some(crosshair(Dot, 0.0, 0.0, 2.0)),
        "electrobolt" => Some(crosshair(CrossDot, 2.0, 5.0, 1.0)),
        "glauncher" | "rlauncher" => Some(crosshair(CrossDot, 6.0, 6.0, 2.0)),
        "machinegun" => Some(crosshair(Cross, 4.0, 6.0, 2.0)),
        "plasmagun" => Some(crosshair(CrossDot, 4.0, 5.0, 2.0)),
        "riotgun" => Some(crosshair(Cross, 12.0, 8.0, 2.0)),
        "voltaic" | "lasergun" => Some(crosshair(Dot, 0.0, 0.0, 3.0)),
        _ => None,
    }
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
