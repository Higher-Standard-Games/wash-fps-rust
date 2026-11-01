//! The sky box loader against SHAG's own (shagger `tools/golden/skybox.mjs`):
//! `transformFace` and `scaleFaceTo` over synthetic faces, then each shipped
//! map's far box as `loadSkybox` finds, decodes, scales and turns it into the
//! six cube slots, compared by SHA-256.
//!
//! The map half needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills)
//! and is skipped without it.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use shagger::loader::bsp::bsp_loader::load;
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::shader::q3_shader_types::ResolveMaterialOptions;
use shagger::loader::texture::skybox_loader::{
    FaceTransform, Q3_FACE_SUFFIXES, load_skybox, load_skybox_face, scale_face_to, transform_face,
};
use shagger::shared::json::{self, Value};

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn pattern(size: u32) -> Vec<u8> {
    (0..size * size * 4).map(|i| ((i * 7 + 3) & 255) as u8).collect()
}

fn golden() -> Value {
    json::parse(include_str!("golden/skybox.json")).unwrap()
}

fn uint(v: &Value) -> u32 {
    v.as_f64().unwrap() as u32
}

#[test]
fn faces_turn_and_scale_as_shag_turns_them() {
    let doc = golden();
    let transforms = doc.get("transforms").and_then(Value::as_array).unwrap();
    for t in transforms {
        let t = t.as_array().unwrap();
        let size = uint(&t[0]);
        let transform = match t[1].as_str().unwrap() {
            "none" => FaceTransform::None,
            "flipH" => FaceTransform::FlipH,
            "flipV" => FaceTransform::FlipV,
            "rotate180" => FaceTransform::Rotate180,
            "rotate90CW" => FaceTransform::Rotate90Cw,
            "rotate90CCW" => FaceTransform::Rotate90Ccw,
            "rotate90CW_flipH" => FaceTransform::Rotate90CwFlipH,
            "rotate90CCW_flipH" => FaceTransform::Rotate90CcwFlipH,
            other => panic!("{other}"),
        };
        assert_eq!(
            sha(&transform_face(&pattern(size), size, transform)),
            t[2].as_str().unwrap(),
            "{size} {transform:?}"
        );
    }
    assert_eq!(transforms.len(), 40);
    for s in doc.get("scales").and_then(Value::as_array).unwrap() {
        let s = s.as_array().unwrap();
        let (size, target) = (uint(&s[0]), uint(&s[1]));
        assert_eq!(
            sha(&scale_face_to(&pattern(size), size, target)),
            s[2].as_str().unwrap(),
            "{size} to {target}"
        );
    }
}

#[test]
fn far_boxes_load_as_shag_loads_them() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let mut pk3 = Pk3Loader::new();
    assert!(ingest_pk3_dir(&mut pk3, &dir.join("pk3")) > 0);
    let mut registry = Q3ShaderRegistry::new();
    registry.load_shaders(&mut pk3);
    let options = ResolveMaterialOptions {
        world_surface: true,
        ..ResolveMaterialOptions::default()
    };
    let doc = golden();
    let maps = doc.get("maps").and_then(Value::as_array).unwrap();
    assert_eq!(maps.len(), 7);
    for m in maps {
        let file = m.get("map").and_then(Value::as_str).unwrap();
        let bsp = load(&std::fs::read(dir.join(file)).unwrap()).unwrap();
        let farbox = bsp.common.faces.iter().find_map(|face| {
            let texture = bsp.common.textures.get(usize::try_from(face.texture_index).ok()?)?;
            let props = registry.resolve_material_props(&pk3, &texture.name, &options);
            if !(props.is_sky || texture.surface_flags & 4 != 0) {
                return None;
            }
            props.sky_parms.map(|p| p.farbox).filter(|f| !f.is_empty())
        });
        assert_eq!(farbox.as_deref(), m.get("farbox").and_then(Value::as_str), "{file}");
        let Some(farbox) = farbox else { continue };
        let sources = m.get("sources").and_then(Value::as_array).unwrap();
        for (suffix, want) in Q3_FACE_SUFFIXES.iter().zip(sources) {
            let (face, path) = load_skybox_face(&mut pk3, &farbox, suffix)
                .or_else(|| load_skybox_face(&mut pk3, &farbox, ""))
                .unwrap();
            assert_eq!(
                Some(path.as_str()),
                want.get("path").and_then(Value::as_str),
                "{file}{suffix}"
            );
            assert_eq!(face.size, uint(want.get("size").unwrap()), "{file} {path}");
            assert_eq!(
                sha(&face.data),
                want.get("sha").and_then(Value::as_str).unwrap(),
                "{file} {path}"
            );
        }
        let skybox = load_skybox(&mut pk3, &farbox).unwrap();
        let loaded: Vec<&str> = m
            .get("loadedFaces")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(skybox.loaded_faces, loaded, "{file}");
        assert_eq!(skybox.size, uint(m.get("size").unwrap()), "{file}");
        let slots = m.get("slots").and_then(Value::as_array).unwrap();
        for (i, (face, want)) in skybox.faces.iter().zip(slots).enumerate() {
            assert_eq!(sha(face), want.as_str().unwrap(), "{file} slot {i}");
        }
    }
}
