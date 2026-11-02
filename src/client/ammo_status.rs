//! wash-fps `client/AmmoStatus.ts`: whether the weapon in hand can fire or
//! should reload, read the same way by the crosshair and the HUD.

use shagger::shared::json::Value;

use crate::shared::entities::definitions::weapons::weapons_table::{weapon_clip_size, weapon_reload_model};

/// `HUDWeaponAmmo`: a clip gun's magazine and reserve, or a bare pool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HudWeaponAmmo {
    Pool(f64),
    Clip { clip: f64, reserve: f64 },
}

impl HudWeaponAmmo {
    /// One entry of `weapon_state`'s ammo JSON.
    #[must_use]
    pub fn from_json(value: &Value) -> Option<Self> {
        if let Some(n) = value.as_f64() {
            return Some(Self::Pool(n));
        }
        let clip = value.get("clip")?.as_f64()?;
        let reserve = value.get("reserve").and_then(Value::as_f64).unwrap_or(0.0);
        Some(Self::Clip { clip, reserve })
    }
}

/// `AmmoStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AmmoStatus {
    /// No clip and no reserve: the red cross and the banner.
    pub out_of_ammo: bool,
    /// A clip gun low on its magazine with reserve to reload from.
    pub low_clip: bool,
    /// The blade never runs dry.
    pub infinite: bool,
}

const LOW_CLIP_FRACTION: f64 = 0.2;

/// `getAmmoStatus`.
#[must_use]
pub fn get_ammo_status(weapon_id: &str, ammo: Option<HudWeaponAmmo>) -> AmmoStatus {
    if weapon_id == "blade" {
        return AmmoStatus {
            infinite: true,
            ..AmmoStatus::default()
        };
    }
    let (clip, reserve) = match ammo {
        Some(HudWeaponAmmo::Clip { clip, reserve }) => (clip, reserve),
        Some(HudWeaponAmmo::Pool(n)) => (0.0, n),
        None => (0.0, 0.0),
    };
    let out_of_ammo = clip <= 0.0 && reserve <= 0.0;
    let mut low_clip = false;
    let clip_size = weapon_clip_size(weapon_id).unwrap_or(0.0);
    if weapon_reload_model(weapon_id) == Some("clip") && clip_size > 0.0 && !out_of_ammo {
        let threshold = (clip_size * LOW_CLIP_FRACTION).ceil().max(1.0);
        low_clip = clip <= threshold && reserve > 0.0;
    }
    AmmoStatus {
        out_of_ammo,
        low_clip,
        infinite: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_clips_and_pools() {
        let json = shagger::shared::json::parse(r#"{"machinegun":{"clip":5,"reserve":40},"rlauncher":0}"#).unwrap();
        let mg = HudWeaponAmmo::from_json(json.get("machinegun").unwrap());
        assert_eq!(
            mg,
            Some(HudWeaponAmmo::Clip {
                clip: 5.0,
                reserve: 40.0
            })
        );
        assert!(get_ammo_status("machinegun", mg).low_clip);
        let rl = HudWeaponAmmo::from_json(json.get("rlauncher").unwrap());
        assert!(get_ammo_status("rlauncher", rl).out_of_ammo);
        assert!(get_ammo_status("blade", None).infinite);
    }
}
