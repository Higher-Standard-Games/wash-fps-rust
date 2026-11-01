//! wash-fps `shared/items/ItemThrow.ts`: the throw every loose item uses,
//! and where map items come to rest when a map loads.

use shagger::shared::collision::collision_world::{Aabb, CollisionWorld};
use shagger::shared::js::{js_cos, js_sin};
use shagger::shared::server::simulation::types::Vec3;

use crate::shared::bsp::pickup_table::{PickupEntry, PickupType};

/// `ITEM_GRAVITY`.
pub const ITEM_GRAVITY: f64 = 800.0;

/// `ITEM_BOUNDS`.
pub const ITEM_BOUNDS: Aabb = Aabb {
    min: Vec3::new(-4.0, -4.0, -4.0),
    max: Vec3::new(4.0, 4.0, 4.0),
};

/// `ITEM_STEP_SECONDS`.
pub const ITEM_STEP_SECONDS: f64 = 1.0 / 125.0;

const GROUND_FRICTION: f64 = 0.82;
const SETTLE_SPEED: f64 = 5.0;
const MAX_LANDING_STEPS: u32 = 5 * 125;

/// `DROPPED_WEAPON_BOUNCE`.
pub const DROPPED_WEAPON_BOUNCE: f64 = 0.35;
const BOUNCE_MIN_SPEED: f64 = 90.0;
const BOUNCE_KEEP_HORIZONTAL: f64 = 0.7;
const MAP_TOSS_UP: f64 = 95.0;
const MAP_TOSS_SPREAD: f64 = 22.0;

/// `ThrownItem`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThrownItem {
    pub position: Vec3,
    pub velocity: Vec3,
    pub settled: bool,
    /// Share of falling speed kept on hitting the floor.
    pub bounce: Option<f64>,
}

/// `stepThrownItem`.
pub fn step_thrown_item(item: &mut ThrownItem, collision: &mut impl CollisionWorld, dt: f64) {
    if item.settled {
        return;
    }
    item.velocity.y -= ITEM_GRAVITY * dt;
    let fall_speed = -item.velocity.y;
    let result = collision.step_slide_move(item.position, item.velocity, dt, ITEM_BOUNDS);
    item.position = result.end_pos;
    item.velocity = result.velocity;
    if !result.grounded {
        return;
    }
    let velocity = &mut item.velocity;
    // `item.bounce &&`: 0 and absent both mean no bounce.
    if let Some(bounce) = item.bounce.filter(|b| *b != 0.0 && !b.is_nan())
        && fall_speed * bounce > BOUNCE_MIN_SPEED
    {
        velocity.y = fall_speed * bounce;
        velocity.x *= BOUNCE_KEEP_HORIZONTAL;
        velocity.z *= BOUNCE_KEEP_HORIZONTAL;
        return;
    }
    velocity.x *= GROUND_FRICTION;
    velocity.z *= GROUND_FRICTION;
    let horizontal_speed = (velocity.x * velocity.x + velocity.z * velocity.z).sqrt();
    if horizontal_speed < SETTLE_SPEED && velocity.y.abs() < SETTLE_SPEED {
        *velocity = Vec3::new(0.0, 0.0, 0.0);
        item.settled = true;
    }
}

/// `landThrownItem`: where the item settles, or `None` when it does not
/// within `max_steps`.
pub fn land_thrown_item(
    start: Vec3,
    velocity: Vec3,
    collision: &mut impl CollisionWorld,
    max_steps: u32,
) -> Option<Vec3> {
    let mut item = ThrownItem {
        position: start,
        velocity,
        settled: false,
        bounce: None,
    };
    let mut step = 0;
    while step < max_steps && !item.settled {
        step_thrown_item(&mut item, collision, ITEM_STEP_SECONDS);
        step += 1;
    }
    item.settled.then_some(item.position)
}

/// `mapWeaponTossVelocity`.
#[must_use]
pub fn map_weapon_toss_velocity(pickup_id: u32) -> Vec3 {
    let id = f64::from(pickup_id);
    let angle = ((id * 2.399963) % (std::f64::consts::PI * 2.0)) + 0.5;
    let spread = MAP_TOSS_SPREAD * (0.35 + ((id * 0.618) % 1.0) * 0.65);
    Vec3::new(js_cos(angle) * spread, MAP_TOSS_UP, js_sin(angle) * spread)
}

/// `landMapPickups`: weapons where their spawn toss lands, the rest dropped
/// straight down; suspended non-weapons and items that never settle stay.
pub fn land_map_pickups(entries: &[PickupEntry], mut collision: Option<&mut impl CollisionWorld>) -> Vec<PickupEntry> {
    entries
        .iter()
        .map(|entry| {
            let Some(collision) = collision.as_deref_mut() else {
                return entry.clone();
            };
            let weapon = entry.kind_type == PickupType::Weapon;
            if !weapon && entry.suspended {
                return entry.clone();
            }
            let velocity = if weapon {
                map_weapon_toss_velocity(entry.id)
            } else {
                Vec3::new(0.0, 0.0, 0.0)
            };
            match land_thrown_item(entry.position, velocity, collision, MAX_LANDING_STEPS) {
                Some(rest) => PickupEntry {
                    position: rest,
                    ..entry.clone()
                },
                None => entry.clone(),
            }
        })
        .collect()
}
