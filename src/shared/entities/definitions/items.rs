//! wash-fps `shared/entities/definitions/items.ts`: the packaged item models.
//!
//! Every armor, health, ammo and powerup pickup draws a packaged model from
//! `washoi_models.pk3` (with the ammo definitions' `ammo/box.ts`), which
//! floats, bobs and turns like a classic Quake item. The procedural recipes
//! (`world/pickups/procedural/procedural_pickup_recipes.rs`) stand in while
//! the package is missing.

use crate::shared::bsp::pickup_table::PickupType;

/// The packaged item models are about 1.9 units tall; this makes the
/// megahealth about 34.
const ITEM_MODEL_SCALE: f32 = 18.0;
/// The armors draw a tenth larger again: about 38 units; green a little
/// smaller, red larger.
const ARMOR_MODEL_SCALE: f32 = 19.8;
const GREEN_ARMOR_SCALE: f32 = ARMOR_MODEL_SCALE * 0.95;
const RED_ARMOR_SCALE: f32 = ARMOR_MODEL_SCALE * 1.1;
/// The health bubble is about 0.96 across; the +25 draws it about 26, the
/// +50 a tenth larger, the +5 a tenth smaller.
const HEALTH_BUBBLE_MODEL: &str = "/data/models/health.glb";
const HEALTH_BUBBLE_SCALE: f32 = 27.0;
/// `ammo/box.ts`: every ammo type draws the packaged ammo box, with its
/// weapon's symbol on each side. It is about 0.95 units tall; this draws it
/// about 23.
const AMMO_BOX_MODEL: &str = "/data/models/ammobox.glb";
const AMMO_BOX_SCALE: f32 = 24.0;
/// The armor shard is worth 5 and draws smaller than the armors: about 25
/// units.
const SHARD_MODEL_SCALE: f32 = 13.2;

/// An item definition's model fields (`EntityDef`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemModelDef {
    pub id: &'static str,
    pub pickup: PickupType,
    /// The pickup table's kind.
    pub kind: &'static str,
    pub model_path: &'static str,
    pub scale: f32,
    /// How much higher than its lowest point clearing the rest position the
    /// model floats.
    pub float_lift: Option<f64>,
}

pub const ITEM_MODELS: [ItemModelDef; 17] = [
    ItemModelDef {
        id: "armor_shard",
        pickup: PickupType::Armor,
        kind: "shard",
        model_path: "/data/models/armor_shard.glb",
        scale: SHARD_MODEL_SCALE,
        float_lift: None,
    },
    ItemModelDef {
        id: "armor_ga",
        pickup: PickupType::Armor,
        kind: "ga",
        model_path: "/data/models/ga.glb",
        scale: GREEN_ARMOR_SCALE,
        float_lift: None,
    },
    ItemModelDef {
        id: "armor_ya",
        pickup: PickupType::Armor,
        kind: "ya",
        model_path: "/data/models/ya.glb",
        scale: ARMOR_MODEL_SCALE,
        float_lift: None,
    },
    ItemModelDef {
        id: "armor_ra",
        pickup: PickupType::Armor,
        kind: "ra",
        model_path: "/data/models/ra.glb",
        scale: RED_ARMOR_SCALE,
        float_lift: None,
    },
    ItemModelDef {
        id: "health_mega",
        pickup: PickupType::Health,
        kind: "mega",
        model_path: "/data/models/mega.glb",
        scale: ITEM_MODEL_SCALE,
        // It floats higher than the armors, the prize it is
        float_lift: Some(10.0),
    },
    health("health_small", "small", HEALTH_BUBBLE_SCALE * 0.9),
    health("health_medium", "medium", HEALTH_BUBBLE_SCALE),
    health("health_large", "large", HEALTH_BUBBLE_SCALE * 1.1),
    ItemModelDef {
        id: "health_ultra",
        pickup: PickupType::Health,
        kind: "ultra",
        // The megahealth's model, its core purple (`FPSPickup`)
        model_path: "/data/models/mega.glb",
        scale: ITEM_MODEL_SCALE,
        float_lift: Some(10.0),
    },
    ItemModelDef {
        id: "powerup_quad",
        pickup: PickupType::Powerup,
        kind: "quad",
        model_path: "/data/models/quad.glb",
        // A tenth larger than the megahealth, and floating as high
        scale: ITEM_MODEL_SCALE * 1.1,
        float_lift: Some(10.0),
    },
    ammo("ammo_bullets", "bullets"),
    ammo("ammo_shells", "shells"),
    ammo("ammo_grenades", "grenades"),
    ammo("ammo_rockets", "rockets"),
    ammo("ammo_plasmagun", "cells"),
    ammo("ammo_lasers", "lasers"),
    ammo("ammo_bolts", "bolts"),
];

const fn health(id: &'static str, kind: &'static str, scale: f32) -> ItemModelDef {
    ItemModelDef {
        id,
        pickup: PickupType::Health,
        kind,
        model_path: HEALTH_BUBBLE_MODEL,
        scale,
        float_lift: None,
    }
}

/// An ammo definition (`definitions/ammo/*.ts`), by its ammo type.
const fn ammo(id: &'static str, kind: &'static str) -> ItemModelDef {
    ItemModelDef {
        id,
        pickup: PickupType::Ammo,
        kind,
        model_path: AMMO_BOX_MODEL,
        scale: AMMO_BOX_SCALE,
        float_lift: None,
    }
}

/// The packaged model of a map pickup, when it has one.
#[must_use]
pub fn item_model(pickup: PickupType, kind: &str) -> Option<&'static ItemModelDef> {
    ITEM_MODELS.iter().find(|d| d.pickup == pickup && d.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_has_a_model() {
        assert_eq!(item_model(PickupType::Health, "mega").unwrap().float_lift, Some(10.0));
        assert!((item_model(PickupType::Armor, "ra").unwrap().scale - 21.78).abs() < 1e-4);
        assert!((item_model(PickupType::Armor, "ga").unwrap().scale - 18.81).abs() < 1e-4);
        assert!((item_model(PickupType::Health, "small").unwrap().scale - 24.3).abs() < 1e-4);
        assert!((item_model(PickupType::Powerup, "quad").unwrap().scale - 19.8).abs() < 1e-4);
        assert_eq!(item_model(PickupType::Ammo, "cells").unwrap().id, "ammo_plasmagun");
        assert_eq!(
            item_model(PickupType::Health, "ultra").unwrap().model_path,
            "/data/models/mega.glb"
        );
    }
}
