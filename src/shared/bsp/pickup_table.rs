//! wash-fps `shared/bsp/PickupTable.ts`: the pickups a map places, in BSP
//! order, with the ids the client and the server share.

use shagger::shared::bsp::map_entities::EntityLike;
use shagger::shared::js::{js_string_to_number, to_int32};
use shagger::shared::server::simulation::types::Vec3;

/// `CLASSNAME_TO_WEAPON`.
pub const CLASSNAME_TO_WEAPON: [(&str, &str); 8] = [
    ("weapon_machinegun", "machinegun"),
    ("weapon_riotgun", "riotgun"),
    ("weapon_grenadelauncher", "glauncher"),
    ("weapon_rocketlauncher", "rlauncher"),
    ("weapon_plasmagun", "plasmagun"),
    ("weapon_voltaic", "voltaic"),
    ("weapon_lasergun", "voltaic"),
    ("weapon_electrobolt", "electrobolt"),
];

/// `PickupType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupType {
    Weapon,
    Ammo,
    Armor,
    Health,
    Powerup,
}

impl PickupType {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Weapon => "weapon",
            Self::Ammo => "ammo",
            Self::Armor => "armor",
            Self::Health => "health",
            Self::Powerup => "powerup",
        }
    }
}

/// `PickupClassInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickupClassInfo {
    pub kind_type: PickupType,
    /// Weapon id, ammo type, armor or health kind.
    pub kind: &'static str,
    pub amount: Option<u32>,
}

const fn ammo(kind: &'static str, amount: u32) -> PickupClassInfo {
    PickupClassInfo {
        kind_type: PickupType::Ammo,
        kind,
        amount: Some(amount),
    }
}

const fn of(kind_type: PickupType, kind: &'static str) -> PickupClassInfo {
    PickupClassInfo {
        kind_type,
        kind,
        amount: None,
    }
}

/// `AMMO_CLASSNAMES`.
const AMMO_CLASSNAMES: [(&str, PickupClassInfo); 14] = [
    ("ammo_machinegun", ammo("bullets", 50)),
    ("ammo_riotgun", ammo("shells", 10)),
    ("ammo_grenadelauncher", ammo("grenades", 10)),
    ("ammo_rocketlauncher", ammo("rockets", 10)),
    ("ammo_plasmagun", ammo("cells", 50)),
    ("ammo_lasergun", ammo("lasers", 100)),
    ("ammo_electrobolt", ammo("bolts", 10)),
    ("ammo_bullets", ammo("bullets", 50)),
    ("ammo_shells", ammo("shells", 10)),
    ("ammo_rockets", ammo("rockets", 10)),
    ("ammo_cells", ammo("cells", 50)),
    ("ammo_grenades", ammo("grenades", 10)),
    ("ammo_bolts", ammo("bolts", 10)),
    ("ammo_lasers", ammo("lasers", 100)),
];

/// The armor, health and powerup part of `PICKUP_CLASSNAMES`.
const ITEM_CLASSNAMES: [(&str, PickupClassInfo); 11] = [
    ("item_armor_shard", of(PickupType::Armor, "shard")),
    ("item_armor_ga", of(PickupType::Armor, "ga")),
    ("item_armor_ya", of(PickupType::Armor, "ya")),
    ("item_armor_ra", of(PickupType::Armor, "ra")),
    ("item_health_small", of(PickupType::Health, "small")),
    ("item_health_medium", of(PickupType::Health, "medium")),
    ("item_health", of(PickupType::Health, "medium")),
    ("item_health_large", of(PickupType::Health, "large")),
    ("item_health_mega", of(PickupType::Health, "mega")),
    ("item_health_ultra", of(PickupType::Health, "ultra")),
    ("item_quad", of(PickupType::Powerup, "quad")),
];

/// `PICKUP_CLASSNAMES[classname]`.
#[must_use]
pub fn pickup_class_info(classname: &str) -> Option<PickupClassInfo> {
    CLASSNAME_TO_WEAPON
        .iter()
        .find(|(c, _)| *c == classname)
        .map(|(_, weapon)| of(PickupType::Weapon, weapon))
        .or_else(|| AMMO_CLASSNAMES.iter().find(|(c, _)| *c == classname).map(|(_, i)| *i))
        .or_else(|| ITEM_CLASSNAMES.iter().find(|(c, _)| *c == classname).map(|(_, i)| *i))
}

/// `PickupEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct PickupEntry {
    /// Index in BSP order: the pickup's id on the client and the server.
    pub id: u32,
    pub classname: String,
    pub kind_type: PickupType,
    pub kind: &'static str,
    pub amount: Option<u32>,
    /// Babylon axes, as the BSP origin gives it.
    pub position: Vec3,
    /// Spawnflag 1: hangs where it is put instead of dropping.
    pub suspended: bool,
}

/// `extractPickups`.
#[must_use]
pub fn extract_pickups<E: EntityLike>(entities: &[E]) -> Vec<PickupEntry> {
    let mut out = Vec::new();
    let mut id = 0;
    for entity in entities {
        let Some(info) = pickup_class_info(entity.classname()) else {
            continue;
        };
        let Some(origin) = entity.property("origin").filter(|o| !o.is_empty()) else {
            continue;
        };
        // `origin.split(" ").map(Number)`: single spaces, so a double space
        // reads an empty part as 0.
        let parts: Vec<f64> = origin.split(' ').map(js_string_to_number).collect();
        let n = |i: usize| parts.get(i).copied().unwrap_or(f64::NAN);
        let (x, y, z) = (n(0), n(1), n(2));
        if !(x.is_finite() && y.is_finite() && z.is_finite()) {
            continue;
        }
        let flags = entity.property("spawnflags").map_or(f64::NAN, js_string_to_number);
        let flags = if flags.is_nan() { 0.0 } else { flags };
        out.push(PickupEntry {
            id,
            classname: entity.classname().to_owned(),
            kind_type: info.kind_type,
            kind: info.kind,
            amount: info.amount,
            position: Vec3::new(x, z, y),
            suspended: to_int32(flags) & 1 != 0,
        });
        id += 1;
    }
    out
}
