//! wash-fps `shared/entities/definitions/items.ts`: the packaged item models.
//!
//! The armor pickups and the megahealth draw packaged models from
//! `washoi_models.pk3`, which float, bob and turn like classic Quake items.
//! The other health pickups are procedural
//! (`world/pickups/procedural/procedural_pickup_recipes.rs`), and so are
//! these when their package is missing: their recipes stay as the fallback.

use crate::shared::bsp::pickup_table::PickupType;

/// The packaged item models are about 1.9 units tall; this makes the
/// megahealth about 34.
const ITEM_MODEL_SCALE: f32 = 18.0;
/// The armors draw a tenth larger again: about 38 units; green a little
/// smaller, red larger.
const ARMOR_MODEL_SCALE: f32 = 19.8;
const GREEN_ARMOR_SCALE: f32 = ARMOR_MODEL_SCALE * 0.95;
const RED_ARMOR_SCALE: f32 = ARMOR_MODEL_SCALE * 1.1;
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

pub const ITEM_MODELS: [ItemModelDef; 5] = [
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
];

/// The packaged model of a map pickup, when it has one.
#[must_use]
pub fn item_model(pickup: PickupType, kind: &str) -> Option<&'static ItemModelDef> {
    ITEM_MODELS.iter().find(|d| d.pickup == pickup && d.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_armors_and_megahealth_have_models() {
        assert_eq!(item_model(PickupType::Health, "mega").unwrap().float_lift, Some(10.0));
        assert!((item_model(PickupType::Armor, "ra").unwrap().scale - 21.78).abs() < 1e-4);
        assert!((item_model(PickupType::Armor, "ga").unwrap().scale - 18.81).abs() < 1e-4);
        assert!(item_model(PickupType::Health, "small").is_none());
    }
}
