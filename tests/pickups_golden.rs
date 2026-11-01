//! Map pickups against wash-fps's own (shagger `tools/golden/pickups.mjs`):
//! the pickup table each shipped map gives (`extractPickups`), and where
//! every item comes to rest (`landMapPickups`) through the collision stack,
//! bit for bit, with the spawn toss of pickup ids 0 to 63.
//!
//! The map half needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills)
//! and is skipped without it.

mod movement_harness;

use std::path::PathBuf;

use movement_harness::read_vec;
use shagger::loader::bsp::bsp_loader::load_collision;
use shagger::physics::sweep_backend_adapter::SweepBackendAdapter;
use shagger::physics::sweep_collision_backend::SweepCollisionBackend;
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::json::{self, Value};
use washoi::shared::bsp::pickup_table::extract_pickups;
use washoi::shared::items::item_throw::{land_map_pickups, map_weapon_toss_velocity};

fn golden() -> Value {
    json::parse(include_str!("golden/pickups.json")).unwrap()
}

#[test]
fn spawn_tosses_match() {
    let doc = golden();
    let tosses = doc.get("tosses").and_then(Value::as_array).unwrap();
    assert_eq!(tosses.len(), 64);
    for (id, want) in tosses.iter().enumerate() {
        let got = map_weapon_toss_velocity(id as u32);
        let want = read_vec(want.as_array().unwrap());
        assert_eq!(
            [got.x.to_bits(), got.y.to_bits(), got.z.to_bits()],
            [want.x.to_bits(), want.y.to_bits(), want.z.to_bits()],
            "id {id}"
        );
    }
}

#[test]
fn map_pickups_land_where_wash_fps_lands_them() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = golden();
    let maps = doc.get("maps").and_then(Value::as_array).unwrap();
    assert_eq!(maps.len(), 7);
    let mut checked = 0;
    for m in maps {
        let file = m.get("map").and_then(Value::as_str).unwrap();
        let bsp = load_collision(&std::fs::read(dir.join(file)).unwrap()).unwrap();
        let table = extract_pickups(&bsp.entities);
        let backend = SweepCollisionBackend::new(bsp.clone());
        let mut world = SharedCollisionWorld::new(SweepBackendAdapter { backend });
        let landed = land_map_pickups(&table, Some(&mut world));
        let want = m.get("pickups").and_then(Value::as_array).unwrap();
        assert_eq!(table.len(), want.len(), "{file}");
        for ((entry, rest), w) in table.iter().zip(&landed).zip(want) {
            let w = w.as_array().unwrap();
            let id = entry.id;
            assert_eq!(f64::from(entry.id), w[0].as_f64().unwrap(), "{file}");
            assert_eq!(Some(entry.classname.as_str()), w[1].as_str(), "{file} {id}");
            assert_eq!(Some(entry.kind_type.name()), w[2].as_str(), "{file} {id}");
            assert_eq!(Some(entry.kind), w[3].as_str(), "{file} {id}");
            assert_eq!(entry.amount.map(f64::from), w[4].as_f64(), "{file} {id}");
            assert_eq!(Some(entry.suspended), w[5].as_bool(), "{file} {id}");
            let bits = |v: shagger::shared::server::simulation::types::Vec3| [v.x, v.y, v.z].map(f64::to_bits);
            assert_eq!(bits(entry.position), bits(read_vec(&w[6..9])), "{file} {id} origin");
            assert_eq!(bits(rest.position), bits(read_vec(&w[9..12])), "{file} {id} landed");
            checked += 1;
        }
    }
    assert_eq!(checked, 187);
}
