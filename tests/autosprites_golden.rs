//! Autosprite records against SHAG's own (shagger
//! `tools/golden/autosprites.mjs`): every autosprite and autosprite2 world
//! face of the shipped maps, its billboard record and vertex attributes, bit
//! for bit.
//!
//! Needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills); skipped
//! without it.

mod movement_harness;

use std::path::PathBuf;

use movement_harness::{fnv, num};
use shagger::loader::bsp::bsp_loader::load;
use shagger::loader::bsp::mesh::autosprite_records::{autosprite_record, autosprite_vertex_data};
use shagger::loader::bsp::mesh::face_builders::{build_mesh_face, build_patch_face, build_polygon_face};
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::shader::q3_shader_types::ResolveMaterialOptions;
use shagger::shared::json::{self, Value};

#[test]
fn autosprites_turn_as_shag_turns_them() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = json::parse(include_str!("golden/autosprites.json")).unwrap();
    let mut pk3 = Pk3Loader::new();
    assert!(ingest_pk3_dir(&mut pk3, &dir.join("pk3")) > 0);
    let mut registry = Q3ShaderRegistry::new();
    registry.load_shaders(&mut pk3);
    let options = ResolveMaterialOptions {
        world_surface: true,
        ..ResolveMaterialOptions::default()
    };
    let mut total = 0;
    for entry in doc.get("maps").and_then(Value::as_array).unwrap() {
        let map = entry.get("map").and_then(Value::as_str).unwrap();
        let bsp = load(&std::fs::read(dir.join(format!("{map}.bsp"))).unwrap()).unwrap();
        let world = &bsp.common.models[0];
        let mut lines = Vec::new();
        for i in world.first_face as usize..(world.first_face + world.num_faces) as usize {
            let face = &bsp.common.faces[i];
            let name = usize::try_from(face.texture_index)
                .ok()
                .and_then(|t| bsp.common.textures.get(t))
                .map_or("", |t| t.name.as_str());
            let props = registry.resolve_material_props(&pk3, name, &options);
            if !props.autosprite && !props.autosprite2 {
                continue;
            }
            let built = match face.face_type {
                1 => build_polygon_face(&bsp, face),
                2 => build_patch_face(&bsp, face),
                3 => build_mesh_face(&bsp, face),
                _ => None,
            };
            let Some(g) = built.filter(|g| !g.positions.is_empty()) else {
                continue;
            };
            let n = (g.positions.len() / 3) as f64;
            let mut c = [0.0f64; 3];
            for v in g.positions.as_chunks::<3>().0.iter() {
                for k in 0..3 {
                    c[k] += v[k];
                }
            }
            let center = c.map(|s| s / n);
            let record = autosprite_record(&g.positions, center, props.autosprite2, 0, &g.positions, &g.indices);
            let data = autosprite_vertex_data(std::slice::from_ref(&record), &g.positions);
            let floats = |a: &[f32]| a.iter().map(|&v| num(f64::from(v))).collect::<Vec<_>>().join(",");
            lines.push(format!(
                "{i} {} {} {} {} {} {}",
                num(record.half_width),
                num(record.half_height),
                record.is_autosprite2,
                floats(&data.center),
                floats(&data.frame),
                floats(&data.right)
            ));
        }
        assert_eq!(
            lines.len() as f64,
            entry.get("sprites").and_then(Value::as_f64).unwrap(),
            "{map}: sprite count"
        );
        assert_eq!(
            fnv(&lines.join("\n")),
            entry.get("hash").and_then(Value::as_str).unwrap(),
            "{map} (DUMP={map},<face> prints SHAG's line)"
        );
        total += lines.len();
    }
    eprintln!("{total} autosprite faces matched");
}
