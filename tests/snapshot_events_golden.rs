//! Washoi's snapshot events against what SHAG's `BinaryProtocol.decodeEvents`
//! makes of the same bytes (shagger `tools/golden/protocol.mjs`).

use shagger::shared::json::{self, Value};
use shagger::shared::net::snapshot::decode_snapshot;
use washoi::shared::net::snapshot_events::{Point, SnapshotEvent, WashoiEvents};

fn bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Numbers as `protocol.mjs` writes them: f64 bits in hex, "nan" for NaN.
fn canon(v: Value) -> Value {
    match v {
        Value::Number(x) if x.is_nan() => Value::String("nan".into()),
        Value::Number(x) => Value::String(format!("{:016x}", x.to_bits())),
        Value::Array(items) => Value::Array(items.into_iter().map(canon).collect()),
        Value::Object(fields) => Value::Object(fields.into_iter().map(|(k, v)| (k, canon(v))).collect()),
        other => other,
    }
}

fn is_index(key: &str) -> bool {
    key.parse::<u32>().is_ok_and(|n| n != u32::MAX && n.to_string() == key)
}

/// `{ type, ...value }` with JavaScript's key order: index keys first.
fn spread(type_name: &str, value: Value) -> Vec<(String, Value)> {
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
fn to_js(event: &SnapshotEvent) -> Value {
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
        } => o
            .s("entityId", &entity_id)
            .s("killerId", &killer_id)
            .s("weaponId", &weapon_id)
            .s("damageType", &damage_type)
            .n("finalDamage", final_damage)
            .p("", at)
            .p("vel", vel)
            .p("kbDir", kb_dir)
            .done(),
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

#[test]
fn events_decode_like_shag() {
    let doc = json::parse(include_str!("golden/snapshot_events.json")).expect("golden json");
    let vectors = doc.get("vectors").and_then(Value::as_array).expect("vectors");
    let mut seen = std::collections::BTreeSet::new();
    for (i, v) in vectors.iter().enumerate() {
        let data = bytes(v.get("bytes").and_then(Value::as_str).unwrap());
        match (decode_snapshot(&data, None, &WashoiEvents), v.get("events")) {
            (Ok(state), Some(expected)) => {
                let got = Value::Array(state.events.iter().map(|e| canon(to_js(e))).collect());
                assert_eq!(&got, expected, "vector {i}");
                seen.extend(state.events.iter().map(SnapshotEvent::type_name));
            }
            (Err(_), None) => {}
            (Ok(_), None) => panic!("vector {i}: SHAG failed with {:?}, Rust decoded", v.get("error")),
            (Err(e), Some(_)) => panic!("vector {i}: SHAG decoded, Rust failed with {e}"),
        }
    }
    assert_eq!(seen.len(), 22, "event kinds covered: {seen:?}");
}
