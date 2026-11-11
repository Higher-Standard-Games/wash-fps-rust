//! Shared by the golden tests that compare Washoi's snapshot events with
//! SHAG's: hex bytes, the f64 bit-string form, and an event as the JavaScript
//! object `BinaryProtocol.decodeEvents` builds.
#![allow(dead_code)]

use shagger::shared::json::Value;
use shagger::shared::net::game_state::GameState;
use washoi::shared::net::snapshot_events::{Point, SnapshotEvent};

pub fn bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Numbers as `protocol.mjs` writes them: f64 bits in hex, "nan" for NaN.
pub fn canon(v: Value) -> Value {
    match v {
        Value::Number(x) if x.is_nan() => Value::String("nan".into()),
        Value::Number(x) => Value::String(format!("{:016x}", x.to_bits())),
        Value::Array(items) => Value::Array(items.into_iter().map(canon).collect()),
        Value::Object(fields) => Value::Object(fields.into_iter().map(|(k, v)| (k, canon(v))).collect()),
        other => other,
    }
}

pub fn is_index(key: &str) -> bool {
    key.parse::<u32>().is_ok_and(|n| n != u32::MAX && n.to_string() == key)
}

/// `{ type, ...value }` with JavaScript's key order: index keys first.
pub fn spread(type_name: &str, value: Value) -> Vec<(String, Value)> {
    let mut fields = vec![("type".to_owned(), Value::String(type_name.into()))];
    let own: Vec<(String, Value)> = match value {
        Value::Object(f) => f,
        Value::Array(items) => items.into_iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        Value::String(s) => s
            .encode_utf16()
            .enumerate()
            .map(|(i, u)| (i.to_string(), Value::String(String::from_utf16_lossy(&[u]))))
            .collect(),
        _ => Vec::new(),
    };
    for (k, v) in own {
        match fields.iter_mut().find(|(key, _)| *key == k) {
            Some(slot) => slot.1 = v,
            None => fields.push((k, v)),
        }
    }
    let (mut index, rest): (Vec<_>, Vec<_>) = fields.into_iter().partition(|(k, _)| is_index(k));
    index.sort_by_key(|(k, _)| k.parse::<u32>().unwrap());
    index.extend(rest);
    index
}

struct Obj(Vec<(String, Value)>);

impl Obj {
    fn new(type_name: &str) -> Self {
        Self(vec![("type".into(), Value::String(type_name.into()))])
    }
    fn s(mut self, k: &str, v: &str) -> Self {
        self.0.push((k.into(), Value::String(v.into())));
        self
    }
    fn n(mut self, k: &str, v: impl Into<f64>) -> Self {
        self.0.push((k.into(), Value::Number(v.into())));
        self
    }
    fn b(mut self, k: &str, v: bool) -> Self {
        self.0.push((k.into(), Value::Bool(v)));
        self
    }
    fn v(mut self, k: &str, v: Value) -> Self {
        self.0.push((k.into(), v));
        self
    }
    /// `{prefix}X`, `{prefix}Y`, `{prefix}Z`.
    fn p(self, prefix: &str, p: Point) -> Self {
        let [x, y, z] = if prefix.is_empty() {
            ["x".to_owned(), "y".to_owned(), "z".to_owned()]
        } else {
            ["X", "Y", "Z"].map(|c| format!("{prefix}{c}"))
        };
        self.n(&x, p.x).n(&y, p.y).n(&z, p.z)
    }
    fn done(self) -> Value {
        Value::Object(self.0)
    }
}

/// The event as the TypeScript object, field for field.
pub fn to_js(event: &SnapshotEvent) -> Value {
    use SnapshotEvent as E;
    let o = Obj::new(event.type_name());
    match event.clone() {
        E::CombatState {
            entity_id,
            health,
            armor,
            dead,
        } => o
            .s("entityId", &entity_id)
            .n("health", health)
            .n("armor", armor)
            .b("dead", dead)
            .done(),
        E::MatchState(value) => Value::Object(spread("match_state", value)),
        E::WeaponState {
            entity_id,
            current_weapon,
            weapons,
            ammo,
            reloading,
            last_fire_tick,
        } => o
            .s("entityId", &entity_id)
            .s("currentWeapon", &current_weapon)
            .v("weapons", weapons)
            .v("ammo", ammo)
            .b("reloading", reloading)
            .n("lastFireTick", last_fire_tick)
            .done(),
        E::WeaponFire {
            entity_id,
            weapon_id,
            origin,
            dir,
            pellet,
        } => {
            let o = o
                .s("entityId", &entity_id)
                .s("weaponId", &weapon_id)
                .p("origin", origin)
                .p("dir", dir);
            match pellet {
                Some((seed, spread)) => o.n("pelletSeed", seed).n("pelletSpread", spread),
                None => o,
            }
            .done()
        }
        E::WeaponCharge {
            entity_id,
            weapon_id,
            charging,
        } => o
            .s("entityId", &entity_id)
            .s("weaponId", &weapon_id)
            .b("charging", charging)
            .done(),
        E::ProjectileSpawn { entity_id, def_id } => o.s("entityId", &entity_id).s("defId", &def_id).done(),
        E::ProjectileImpact { def_id, at, normal } => o.s("defId", &def_id).p("", at).p("normal", normal).done(),
        E::ItemPickup {
            pickup_id,
            weapon_id,
            entity_id,
            dropped_weapon_id,
            at,
        } => o
            .n("pickupId", pickup_id)
            .s("weaponId", &weapon_id)
            .s("entityId", &entity_id)
            .s("droppedWeaponId", &dropped_weapon_id)
            .p("", at)
            .done(),
        E::WeaponDrop {
            entity_id,
            weapon_id,
            at,
            vel,
        } => o
            .s("entityId", &entity_id)
            .s("weaponId", &weapon_id)
            .p("", at)
            .p("vel", vel)
            .done(),
        E::Damage {
            entity_id,
            attacker_id,
            damage,
            weapon_id,
            at,
            hit_location,
            pain_tier,
        } => {
            let mut o = o
                .s("entityId", &entity_id)
                .s("attackerId", &attacker_id)
                .n("damage", damage)
                .s("weaponId", &weapon_id)
                .p("", at);
            if let Some(l) = hit_location {
                o = o.s("hitLocation", l);
            }
            if let Some(t) = pain_tier {
                o = o.n("painTier", t);
            }
            o.done()
        }
        E::Death {
            entity_id,
            killer_id,
            weapon_id,
            damage_type,
            final_damage,
            at,
            vel,
            kb_dir,
            kill_weapon,
            hit_location,
        } => {
            let mut o = o
                .s("entityId", &entity_id)
                .s("killerId", &killer_id)
                .s("weaponId", &weapon_id)
                .s("damageType", &damage_type)
                .n("finalDamage", final_damage)
                .p("", at)
                .p("vel", vel)
                .p("kbDir", kb_dir);
            if let Some(l) = hit_location {
                o = o.s("hitLocation", l);
            }
            if let Some(w) = kill_weapon {
                o = o.s("killWeapon", &w);
            }
            o.done()
        }
        E::Award {
            award_type,
            player_id,
            player_name,
            target_name,
            value,
        } => o
            .s("awardType", &award_type)
            .s("playerId", &player_id)
            .s("playerName", &player_name)
            .s("targetName", &target_name)
            .n("value", value)
            .done(),
        E::WeaponChain { entity_id, points } => {
            let points = points.into_iter().map(|p| Obj(Vec::new()).p("", p).done()).collect();
            o.s("entityId", &entity_id).v("points", Value::Array(points)).done()
        }
        E::Chat {
            sender_id,
            sender_name,
            team,
            text,
        } => o
            .s("senderId", &sender_id)
            .s("senderName", &sender_name)
            .s("team", &team)
            .s("text", &text)
            .done(),
        E::PickupNotify { entity_id, label } => o.s("entityId", &entity_id).s("label", &label).done(),
        E::Beam {
            entity_id,
            origin,
            dir,
            end,
            normal,
            impact_kind,
            ..
        } => o
            .s("entityId", &entity_id)
            .p("origin", origin)
            .p("dir", dir)
            .p("end", end)
            .p("normal", normal)
            .n("impactKind", impact_kind)
            .done(),
        E::PowerupState {
            entity_id,
            kind,
            seconds,
        } => o
            .s("entityId", &entity_id)
            .s("kind", &kind)
            .n("seconds", seconds)
            .done(),
        E::PowerupSpawn { kind, at } => o.s("kind", &kind).p("", at).done(),
        E::AgentDirectives { entity_id, directives } => o.s("entityId", &entity_id).v("directives", directives).done(),
        E::LiftState { lifts } => {
            let lifts = lifts
                .into_iter()
                .map(|l| {
                    Obj(Vec::new())
                        .n("modelIndex", l.model_index)
                        .n("offset", l.offset)
                        .s("phase", l.phase)
                        .n("remaining", l.remaining)
                        .v(
                            "cooldowns",
                            Value::Array(l.cooldowns.into_iter().map(Value::Number).collect()),
                        )
                        .done()
                })
                .collect();
            o.v("lifts", Value::Array(lifts)).done()
        }
        E::KnockbackState { entity_id, age_ticks } => o
            .s("entityId", &entity_id)
            .v("ageTicks", age_ticks.map_or(Value::Null, |a| Value::Number(a.into())))
            .done(),
    }
}

/// The canonical float form `protocol.mjs` writes.
pub fn num(x: f64) -> Value {
    Value::String(if x.is_nan() {
        "nan".to_owned()
    } else {
        format!("{:016x}", x.to_bits())
    })
}

pub fn obj(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn int(x: impl Into<f64>) -> Value {
    Value::Number(x.into())
}

fn text(x: &str) -> Value {
    Value::String(x.to_owned())
}

fn vec3(v: shagger::shared::net::game_state::Vec3) -> Value {
    Value::Array(vec![num(v.x), num(v.y), num(v.z)])
}

/// The shape of `canonicalState` in `protocol.mjs`.
pub fn canonical<E>(state: &GameState<E>) -> Value {
    let entities = state
        .entities
        .iter()
        .map(|(id, e)| {
            let mut c = Vec::new();
            if let Some(p) = e.position {
                c.push(("position", vec3(p)));
            }
            if let Some(t) = e.transform {
                let rotation = t.rotation.iter().map(|&r| num(r)).collect();
                c.push((
                    "transform",
                    obj(vec![
                        ("position", vec3(t.position)),
                        ("rotation", Value::Array(rotation)),
                    ]),
                ));
            }
            if let Some(v) = e.velocity {
                c.push((
                    "velocity",
                    obj(vec![
                        ("linear", vec3(v.linear)),
                        ("grounded", Value::Bool(v.grounded)),
                        ("isCrouching", Value::Bool(v.is_crouching)),
                        ("isSliding", Value::Bool(v.is_sliding)),
                    ]),
                ));
            }
            if let Some(m) = e.movement_state {
                let mut fields = vec![
                    ("isCrouching", Value::Bool(m.is_crouching)),
                    ("isSliding", Value::Bool(m.is_sliding)),
                    (
                        "wallJumpState",
                        text(if m.is_wall_jumping { "charging" } else { "none" }),
                    ),
                    ("isAiming", Value::Bool(m.is_aiming)),
                ];
                if let Some(view) = m.view {
                    fields.push((
                        "view",
                        Value::Array(vec![
                            int(view.strafe_input),
                            int(view.forward_input),
                            num(view.slide_phase),
                            num(view.ledge_grab_phase),
                            num(view.crouch_view_offset),
                        ]),
                    ));
                }
                c.push(("movementState", obj(fields)));
            }
            if let Some(w) = &e.weapon_meta {
                c.push((
                    "weaponMeta",
                    obj(vec![
                        ("weaponId", text(&w.weapon_id)),
                        ("settled", Value::Bool(w.settled)),
                    ]),
                ));
            }
            if let Some(p) = e.powerups {
                c.push(("powerups", int(p)));
            }
            Value::Array(vec![text(id), obj(c)])
        })
        .collect();
    let players = state
        .players
        .iter()
        .map(|(id, p)| {
            Value::Array(vec![
                text(id),
                obj(vec![
                    ("entityId", text(&p.entity_id)),
                    ("name", text(&p.name)),
                    ("team", text(&p.team)),
                    ("score", int(p.score)),
                    ("deaths", int(p.deaths)),
                    ("ping", int(p.ping)),
                    ("look", p.look.map_or(Value::Null, int)),
                ]),
            ])
        })
        .collect();
    let mask: String = state
        .pickup_active_mask
        .iter()
        .map(|&b| if b { '1' } else { '0' })
        .collect();
    obj(vec![
        ("tick", int(state.tick)),
        ("serverTimestamp", num(state.server_timestamp)),
        ("ackedInputSeq", int(state.acked_input_seq)),
        ("entities", Value::Array(entities)),
        ("players", Value::Array(players)),
        ("events", int(u32::try_from(state.events.len()).unwrap())),
        ("pickupActiveMask", Value::String(mask)),
    ])
}
