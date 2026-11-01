//! The shader registry against SHAG's own (shagger
//! `tools/golden/shader_materials.mjs`): every published package mounted,
//! every script loaded, and every shader name and map texture resolved to its
//! material passes under three option sets, compared as canonical JSON.
//!
//! Needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills); skipped
//! without it.

mod movement_harness;

use std::collections::BTreeSet;
use std::path::PathBuf;

use movement_harness::{fnv, num};
use shagger::loader::bsp::bsp_loader::load;
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::shader::q3_shader_types::ResolveMaterialOptions;
use shagger::shared::json::{self, Value};

/// The generator's `canonical`: keys sorted, numbers as f64 hex bits.
fn canonical(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        J::Number(n) => J::String(num(n.as_f64().unwrap())),
        J::Array(a) => J::Array(a.into_iter().map(canonical).collect()),
        J::Object(o) => J::Object(o.into_iter().map(|(k, v)| (k, canonical(v))).collect()),
        other => other,
    }
}

/// The generator's `OPTION_SETS`.
const OPTION_SETS: [ResolveMaterialOptions; 3] = [
    ResolveMaterialOptions {
        world_surface: true,
        pass_index: None,
        deluxe: false,
        texture_cube_map: None,
        without_generated_cubemap: false,
    },
    ResolveMaterialOptions {
        world_surface: true,
        pass_index: None,
        deluxe: true,
        texture_cube_map: None,
        without_generated_cubemap: false,
    },
    ResolveMaterialOptions {
        world_surface: false,
        pass_index: None,
        deluxe: false,
        texture_cube_map: Some(false),
        without_generated_cubemap: false,
    },
];

#[test]
fn shader_materials_resolve_as_shag_resolves_them() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = json::parse(include_str!("golden/shader_materials.json")).unwrap();
    let mut pk3 = Pk3Loader::new();
    let packages = ingest_pk3_dir(&mut pk3, &dir.join("pk3"));
    assert_eq!(packages as f64, doc.get("packages").and_then(Value::as_f64).unwrap());
    let mut registry = Q3ShaderRegistry::new();
    let count = registry.load_shaders(&mut pk3);
    assert_eq!(count as f64, doc.get("shaders").and_then(Value::as_f64).unwrap());

    let mut names: BTreeSet<String> = registry.shader_names().map(str::to_owned).collect();
    let mut maps: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| f.ends_with(".bsp"))
        .collect();
    maps.sort();
    for map in maps {
        let bsp = load(&std::fs::read(dir.join(map)).unwrap()).unwrap();
        names.extend(bsp.textures.iter().map(|t| t.name.to_lowercase()));
    }

    let want = doc.get("materials").and_then(Value::as_array).unwrap();
    assert_eq!(names.len(), want.len());
    let mut mismatched = Vec::new();
    for (name, entry) in names.iter().zip(want) {
        let entry = entry.as_array().unwrap();
        assert_eq!(name, entry[0].as_str().unwrap(), "name lists differ");
        let sets: Vec<_> = OPTION_SETS
            .iter()
            .map(|options| {
                serde_json::json!({
                    "cubemap": registry.has_generated_cubemap(name, options),
                    "passes": registry.resolve_material_passes(&pk3, name, options),
                })
            })
            .collect();
        let doc = serde_json::json!({
            "shader": registry.get_shader(name).map(|s| s.name.clone()),
            "sets": sets,
        });
        let text = serde_json::to_string(&canonical(doc)).unwrap();
        if fnv(&text) != entry[1].as_str().unwrap() {
            mismatched.push(format!("{name}: {text:.1500}"));
        }
    }
    assert!(
        mismatched.is_empty(),
        "{} of {} differ (DUMP=<name> prints the TypeScript JSON):\n{}",
        mismatched.len(),
        names.len(),
        mismatched[..mismatched.len().min(3)].join("\n")
    );
    eprintln!("{} materials matched", names.len());
}
