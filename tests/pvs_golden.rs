//! PVS culling against SHAG's own (shagger `tools/golden/pvs.mjs`): each
//! shipped map's face clusters (`buildFaceClusters`), then `PVSCuller` with
//! every face as a mesh while the camera walks 400 points, frame by frame:
//! the camera's cluster, the stats and which meshes show.
//!
//! Needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills); skipped
//! without it.

mod movement_harness;

use std::path::PathBuf;

use movement_harness::fnv;
use shagger::loader::bsp::bsp_loader::load;
use shagger::loader::bsp::mesh::pvs::build_face_clusters;
use shagger::loader::bsp::pvs_culler::PvsCuller;
use shagger::rendering::glam::Vec3;
use shagger::shared::json::{self, Value};

#[test]
fn pvs_culls_as_shag_culls() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = json::parse(include_str!("golden/pvs.json")).unwrap();
    let maps = doc.get("maps").and_then(Value::as_array).unwrap();
    assert_eq!(maps.len(), 7);
    let mut frames_checked = 0;
    for m in maps {
        let file = m.get("map").and_then(Value::as_str).unwrap();
        let bsp = load(&std::fs::read(dir.join(file)).unwrap()).unwrap();
        let clusters = build_face_clusters(&bsp);
        let text = format!(
            "[{}]",
            clusters
                .iter()
                .map(|c| format!("[{}]", c.iter().map(i32::to_string).collect::<Vec<_>>().join(",")))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            fnv(&text),
            m.get("faceClusters").and_then(Value::as_str).unwrap(),
            "{file}"
        );
        let mut culler = PvsCuller::new(&bsp);
        assert_eq!(
            Some(culler.has_pvs()),
            m.get("hasPVS").and_then(Value::as_bool),
            "{file}"
        );
        culler.register_from_world_meshes(&clusters);
        for (i, frame) in m.get("frames").and_then(Value::as_array).unwrap().iter().enumerate() {
            let f = frame.as_array().unwrap();
            let position = f[0].as_array().map(|p| {
                let c = |i: usize| p[i].as_f64().unwrap() as f32;
                Vec3::new(c(0), c(1), c(2))
            });
            let stats = culler.update(position);
            let n = |v: &Value| v.as_f64().unwrap();
            let got = (
                stats.enabled,
                f64::from(stats.current_cluster),
                stats.visible_clusters as f64,
                stats.visible_meshes as f64,
                stats.culled_meshes as f64,
            );
            let want = (f[1].as_bool().unwrap(), n(&f[2]), n(&f[3]), n(&f[4]), n(&f[5]));
            assert_eq!(got, want, "{file} frame {i}");
            let shown: String = (0..clusters.len())
                .map(|mesh| if culler.is_mesh_visible(mesh) { '1' } else { '0' })
                .collect();
            assert_eq!(fnv(&shown), f[6].as_str().unwrap(), "{file} frame {i}");
            frames_checked += 1;
        }
    }
    assert_eq!(frames_checked, 2800);
}
