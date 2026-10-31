//! Washoi's snapshot events (SHAG `src/shared/net/BinaryProtocol.ts`,
//! `decodeEvents` and the code tables above it).
//!
//! SHAG hardcodes this catalogue in its codec; it is Washoi's in substance, so
//! here it is the [`EventCodec`] Washoi hands to the engine (RUST.md 14.2).
//! The bytes on the wire are unchanged.

use shagger::shared::json::{self, Value};
use shagger::shared::net::DecodeError;
use shagger::shared::net::binary_protocol::BinaryDecoder;
use shagger::shared::net::snapshot::EventCodec;

/// `EVT_NAME_MAP`: the event name for each wire code. Codes 23 and 25 are
/// `damage`, 24 is `weapon_fire`.
pub const EVENT_NAMES: [&str; 26] = [
    "",
    "combat_state",
    "match_state",
    "weapon_state",
    "weapon_fire",
    "weapon_charge",
    "projectile_spawn",
    "projectile_impact",
    "item_pickup",
    "weapon_drop",
    "damage",
    "death",
    "award",
    "weapon_chain",
    "chat",
    "pickup_notify",
    "voltaic_beam",
    "electro_rail",
    "powerup_state",
    "powerup_spawn",
    "agent_directives",
    "lift_state",
    "knockback_state",
    "damage",
    "weapon_fire",
    "damage",
];

/// A damage event that names where the hit landed.
pub const DAMAGE_LOCATED: u8 = 23;
/// A weapon_fire event that carries a pellet seed.
pub const FIRE_SEEDED: u8 = 24;
/// A damage event that carries the victim's pain tier.
pub const DAMAGE_PAINED: u8 = 25;

/// Wire order of the hit location byte. Append only: the index is the encoding.
pub const HIT_LOCATIONS: [&str; 3] = ["body", "head", "legs"];

/// Wire order of a lift's phase byte.
pub const LIFT_PHASES: [&str; 4] = ["bottom", "up", "top", "down"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// One lift in a `lift_state` event.
#[derive(Debug, Clone, PartialEq)]
pub struct LiftState {
    pub model_index: u16,
    pub offset: f64,
    /// One of [`LIFT_PHASES`]; an unknown byte reads as `"bottom"`.
    pub phase: &'static str,
    pub remaining: f64,
    pub cooldowns: Vec<f64>,
}

/// A snapshot event, one variant per payload layout. Field names follow the
/// TypeScript event objects.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotEvent {
    CombatState {
        entity_id: String,
        health: u16,
        armor: u16,
        dead: bool,
    },
    /// The TypeScript spreads the parsed JSON into the event
    /// (`{ type, ...JSON.parse(text) }`); this keeps the parsed value.
    MatchState(Value),
    WeaponState {
        entity_id: String,
        current_weapon: String,
        weapons: Value,
        ammo: Value,
        reloading: bool,
        last_fire_tick: u32,
    },
    /// Codes 4 and 24. The seed and spread travel only with code 24.
    WeaponFire {
        entity_id: String,
        weapon_id: String,
        origin: Point,
        dir: Point,
        pellet: Option<(i32, f64)>,
    },
    WeaponCharge {
        entity_id: String,
        weapon_id: String,
        charging: bool,
    },
    ProjectileSpawn {
        entity_id: String,
        def_id: String,
    },
    ProjectileImpact {
        def_id: String,
        at: Point,
        normal: Point,
    },
    ItemPickup {
        pickup_id: i16,
        weapon_id: String,
        entity_id: String,
        dropped_weapon_id: String,
        at: Point,
    },
    WeaponDrop {
        entity_id: String,
        weapon_id: String,
        at: Point,
        vel: Point,
    },
    /// Codes 10, 23 and 25.
    Damage {
        entity_id: String,
        attacker_id: String,
        damage: u16,
        weapon_id: String,
        at: Point,
        /// Absent means a body hit.
        hit_location: Option<&'static str>,
        /// 0 to 3, code 25 only.
        pain_tier: Option<u8>,
    },
    Death {
        entity_id: String,
        killer_id: String,
        weapon_id: String,
        damage_type: String,
        final_damage: u16,
        at: Point,
        vel: Point,
        kb_dir: Point,
    },
    Award {
        award_type: String,
        player_id: String,
        player_name: String,
        target_name: String,
        value: u16,
    },
    WeaponChain {
        entity_id: String,
        points: Vec<Point>,
    },
    Chat {
        sender_id: String,
        sender_name: String,
        team: String,
        text: String,
    },
    PickupNotify {
        entity_id: String,
        label: String,
    },
    /// `voltaic_beam` (16) and `electro_rail` (17) share this payload.
    Beam {
        electro_rail: bool,
        entity_id: String,
        origin: Point,
        dir: Point,
        end: Point,
        normal: Point,
        impact_kind: u8,
    },
    PowerupState {
        entity_id: String,
        kind: String,
        seconds: u16,
    },
    PowerupSpawn {
        kind: String,
        at: Point,
    },
    /// A malformed payload reads as an empty list, as in the TypeScript.
    AgentDirectives {
        entity_id: String,
        directives: Value,
    },
    LiftState {
        lifts: Vec<LiftState>,
    },
    /// `None` when the age byte is 255.
    KnockbackState {
        entity_id: String,
        age_ticks: Option<u8>,
    },
}

impl SnapshotEvent {
    /// The TypeScript `type` field.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::CombatState { .. } => "combat_state",
            Self::MatchState(_) => "match_state",
            Self::WeaponState { .. } => "weapon_state",
            Self::WeaponFire { .. } => "weapon_fire",
            Self::WeaponCharge { .. } => "weapon_charge",
            Self::ProjectileSpawn { .. } => "projectile_spawn",
            Self::ProjectileImpact { .. } => "projectile_impact",
            Self::ItemPickup { .. } => "item_pickup",
            Self::WeaponDrop { .. } => "weapon_drop",
            Self::Damage { .. } => "damage",
            Self::Death { .. } => "death",
            Self::Award { .. } => "award",
            Self::WeaponChain { .. } => "weapon_chain",
            Self::Chat { .. } => "chat",
            Self::PickupNotify { .. } => "pickup_notify",
            Self::Beam {
                electro_rail: false, ..
            } => "voltaic_beam",
            Self::Beam { electro_rail: true, .. } => "electro_rail",
            Self::PowerupState { .. } => "powerup_state",
            Self::PowerupSpawn { .. } => "powerup_spawn",
            Self::AgentDirectives { .. } => "agent_directives",
            Self::LiftState { .. } => "lift_state",
            Self::KnockbackState { .. } => "knockback_state",
        }
    }
}

/// Washoi's [`EventCodec`].
#[derive(Debug, Clone, Copy, Default)]
pub struct WashoiEvents;

fn point(d: &mut BinaryDecoder<'_>) -> Result<Point, DecodeError> {
    Ok(Point {
        x: d.read_float32()?,
        y: d.read_float32()?,
        z: d.read_float32()?,
    })
}

fn parse_json(text: &str) -> Result<Value, DecodeError> {
    Ok(json::parse(text)?)
}

impl EventCodec for WashoiEvents {
    type Event = SnapshotEvent;

    #[allow(clippy::too_many_lines)]
    fn decode_event(&self, code: u8, d: &mut BinaryDecoder<'_>) -> Result<Option<SnapshotEvent>, DecodeError> {
        use SnapshotEvent as E;
        Ok(Some(match code {
            1 => E::CombatState {
                entity_id: d.read_string()?,
                health: d.read_uint16()?,
                armor: d.read_uint16()?,
                dead: d.read_uint8()? != 0,
            },
            2 => E::MatchState(parse_json(&d.read_string()?)?),
            21 => {
                let mut lifts = Vec::new();
                for _ in 0..d.read_uint8()? {
                    let model_index = d.read_uint16()?;
                    let offset = d.read_float32()?;
                    let phase = LIFT_PHASES
                        .get(usize::from(d.read_uint8()?))
                        .copied()
                        .unwrap_or("bottom");
                    let remaining = d.read_float32()?;
                    let mut cooldowns = Vec::new();
                    for _ in 0..d.read_uint8()? {
                        cooldowns.push(d.read_float32()?);
                    }
                    lifts.push(LiftState {
                        model_index,
                        offset,
                        phase,
                        remaining,
                        cooldowns,
                    });
                }
                E::LiftState { lifts }
            }
            3 => E::WeaponState {
                entity_id: d.read_string()?,
                current_weapon: d.read_string()?,
                weapons: parse_json(&d.read_string()?)?,
                ammo: parse_json(&d.read_string()?)?,
                reloading: d.read_uint8()? != 0,
                last_fire_tick: d.read_uint32()?,
            },
            4 | FIRE_SEEDED => E::WeaponFire {
                entity_id: d.read_string()?,
                weapon_id: d.read_string()?,
                origin: point(d)?,
                dir: point(d)?,
                pellet: if code == FIRE_SEEDED {
                    Some((d.read_int32()?, d.read_float32()?))
                } else {
                    None
                },
            },
            5 => E::WeaponCharge {
                entity_id: d.read_string()?,
                weapon_id: d.read_string()?,
                charging: d.read_uint8()? != 0,
            },
            13 => {
                let entity_id = d.read_string()?;
                let count = d.read_uint8()?;
                let points = (0..count).map(|_| point(d)).collect::<Result<_, _>>()?;
                E::WeaponChain { entity_id, points }
            }
            6 => E::ProjectileSpawn {
                entity_id: d.read_string()?,
                def_id: d.read_string()?,
            },
            7 => E::ProjectileImpact {
                def_id: d.read_string()?,
                at: point(d)?,
                normal: point(d)?,
            },
            8 => E::ItemPickup {
                pickup_id: d.read_int16()?,
                weapon_id: d.read_string()?,
                entity_id: d.read_string()?,
                dropped_weapon_id: d.read_string()?,
                at: point(d)?,
            },
            9 => E::WeaponDrop {
                entity_id: d.read_string()?,
                weapon_id: d.read_string()?,
                at: point(d)?,
                vel: point(d)?,
            },
            10 | DAMAGE_LOCATED | DAMAGE_PAINED => {
                let entity_id = d.read_string()?;
                let attacker_id = d.read_string()?;
                let damage = d.read_uint16()?;
                let weapon_id = d.read_string()?;
                let at = point(d)?;
                let location = |byte: u8| HIT_LOCATIONS.get(usize::from(byte)).copied();
                let hit_location = match code {
                    DAMAGE_LOCATED => Some(location(d.read_uint8()?).unwrap_or("body")),
                    // A body hit is left without a location, as code 10 leaves it.
                    DAMAGE_PAINED => location(d.read_uint8()?).filter(|&l| l != "body"),
                    _ => None,
                };
                let pain_tier = if code == DAMAGE_PAINED {
                    Some(d.read_uint8()?.min(3))
                } else {
                    None
                };
                E::Damage {
                    entity_id,
                    attacker_id,
                    damage,
                    weapon_id,
                    at,
                    hit_location,
                    pain_tier,
                }
            }
            11 => E::Death {
                entity_id: d.read_string()?,
                killer_id: d.read_string()?,
                weapon_id: d.read_string()?,
                damage_type: d.read_string()?,
                final_damage: d.read_uint16()?,
                at: point(d)?,
                vel: point(d)?,
                kb_dir: point(d)?,
            },
            12 => E::Award {
                award_type: d.read_string()?,
                player_id: d.read_string()?,
                player_name: d.read_string()?,
                target_name: d.read_string()?,
                value: d.read_uint16()?,
            },
            14 => E::Chat {
                sender_id: d.read_string()?,
                sender_name: d.read_string()?,
                team: d.read_string()?,
                text: d.read_string()?,
            },
            15 => E::PickupNotify {
                entity_id: d.read_string()?,
                label: d.read_string()?,
            },
            16 | 17 => E::Beam {
                electro_rail: code == 17,
                entity_id: d.read_string()?,
                origin: point(d)?,
                dir: point(d)?,
                end: point(d)?,
                normal: point(d)?,
                impact_kind: d.read_uint8()?,
            },
            18 => E::PowerupState {
                entity_id: d.read_string()?,
                kind: d.read_string()?,
                seconds: d.read_uint16()?,
            },
            19 => E::PowerupSpawn {
                kind: d.read_string()?,
                at: point(d)?,
            },
            20 => {
                let entity_id = d.read_string()?;
                let directives = json::parse(&d.read_string()?).unwrap_or(Value::Array(Vec::new()));
                E::AgentDirectives { entity_id, directives }
            }
            22 => {
                let entity_id = d.read_string()?;
                let age = d.read_uint8()?;
                E::KnockbackState {
                    entity_id,
                    age_ticks: (age != 255).then_some(age),
                }
            }
            _ => return Ok(None),
        }))
    }
}
