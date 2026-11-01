//! The renderer's map geometry and lightmaps against SHAG's own TypeScript
//! (shagger `tools/golden/render_maps.mjs`): every face of every shipped map
//! built by the face builders, every lightmap page encoded three ways, the
//! IBSP transfer curve, the vertex-colour scaling and each map's lighting
//! profile, all bit for bit.
//!
//! Like `movement_maps_golden.rs`, it needs `WASHOI_MAPS` (the folder
//! `tools/fetch_maps.sh` fills) and reports a skip without it, except for the
//! transfer curve, which needs no map.

mod movement_harness;

use std::path::PathBuf;

use movement_harness::{fnv, num};
use sha2::{Digest, Sha256};
use shagger::loader::bsp::bsp_lighting_profile::bsp_lighting_profile;
use shagger::loader::bsp::bsp_loader::{BspVertices, load};
use shagger::loader::bsp::mesh::face_builders::{build_mesh_face, build_patch_face, build_polygon_face};
use shagger::loader::bsp::mesh::lightmaps::{bsp_lightmap_rgba, ibsp_lightmap_byte, scale_bsp_vertex_colors};
use shagger::loader::bsp::mesh::types::FaceGeometry;
use shagger::shared::json::{self, Value};

fn doc() -> Value {
    json::parse(include_str!("golden/render_maps.json")).unwrap()
}

fn line(i: usize, face_type: i32, g: &FaceGeometry) -> String {
    let nums = |v: &[f64]| v.iter().map(|&x| num(x)).collect::<Vec<_>>().join(",");
    let indices = g.indices.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    format!(
        "{i} {face_type} {} {} {} {} {} {indices}",
        nums(&g.positions),
        nums(&g.normals),
        nums(&g.uvs),
        nums(&g.uv2s),
        nums(&g.colors)
    )
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap()
}

#[test]
fn ibsp_lightmap_curve_matches_shag() {
    let doc = doc();
    let curve = doc.get("curve").and_then(Value::as_array).unwrap();
    for (c, want) in curve.iter().enumerate() {
        let want = want.as_f64().unwrap() as u8;
        assert_eq!(ibsp_lightmap_byte(c as u8), want, "byte {c}");
    }
}

#[test]
fn map_geometry_and_lightmaps_match_shag() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = doc();
    let group = doc.get("group").and_then(Value::as_f64).unwrap() as usize;
    let mut faces_checked = 0;
    for v in doc.get("maps").and_then(Value::as_array).unwrap() {
        let name = str_of(v, "map");
        let bytes = std::fs::read(dir.join(format!("{name}.bsp"))).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            str_of(v, "sha256"),
            "{name}: not the map the vectors were made from"
        );
        let bsp = load(&bytes).unwrap();

        // The worldspawn lighting profile.
        let want = v.get("profile").unwrap();
        let profile = bsp_lighting_profile(&bsp);
        assert_eq!(num(profile.gain), str_of(want, "gain"), "{name}: gain");
        let ambient: Vec<&str> = want
            .get("ambient")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert_eq!(profile.ambient.map(num).to_vec(), ambient, "{name}: ambient");
        assert_eq!(
            profile.deluxe,
            want.get("deluxe").and_then(Value::as_bool).unwrap(),
            "{name}: deluxe"
        );

        // Faces, hashed in runs of `group`.
        let want = v.get("faces").and_then(Value::as_array).unwrap();
        let mut hashes = Vec::new();
        for (i, face) in bsp.common.faces.iter().enumerate() {
            let built = match face.face_type {
                1 => build_polygon_face(&bsp, face),
                2 => build_patch_face(&bsp, face),
                3 => build_mesh_face(&bsp, face),
                _ => None,
            };
            let text = match (face.face_type, built) {
                (1..=3, Some(g)) => line(i, face.face_type, &g),
                (1..=3, None) => panic!("{name} face {i}: the builder refused a face SHAG builds"),
                (t, _) => format!("{i} {t}"),
            };
            hashes.push(fnv(&text));
            if hashes.len() == group || i + 1 == bsp.common.faces.len() {
                let run = i / group;
                let got = fnv(&hashes.concat());
                assert_eq!(
                    got,
                    want[run].as_str().unwrap(),
                    "{name}: faces {}..={i} differ (DUMP={name},<face> prints the TypeScript line; face {i}: {text:.300})",
                    run * group
                );
                hashes.clear();
                faces_checked += group.min(i + 1 - run * group);
            }
        }
        assert_eq!(want.len(), bsp.common.faces.len().div_ceil(group), "{name}: face count");

        // Lightmap pages: IBSP curve, raw, and deluxe direction.
        let size = bsp.lightmap_size as usize;
        let sha = |b: &[u8]| format!("{:x}", Sha256::digest(b));
        let pages = v.get("pages").and_then(Value::as_array).unwrap();
        assert_eq!(pages.len(), bsp.lightmaps.len(), "{name}: lightmap pages");
        for (p, (rgb, want)) in bsp.lightmaps.iter().zip(pages).enumerate() {
            assert_eq!(
                sha(&bsp_lightmap_rgba(rgb, size, false, false)),
                str_of(want, "ibsp"),
                "{name} page {p} ibsp"
            );
            assert_eq!(
                sha(&bsp_lightmap_rgba(rgb, size, true, false)),
                str_of(want, "raw"),
                "{name} page {p} raw"
            );
            assert_eq!(
                sha(&bsp_lightmap_rgba(rgb, size, true, true)),
                str_of(want, "direction"),
                "{name} page {p} direction"
            );
        }

        // Vertex colour scaling.
        let mut colors = Vec::new();
        let count = bsp.vertices.len().min(4096);
        for i in 0..count {
            let c = match &bsp.vertices {
                BspVertices::Ibsp(v) => v[i].color,
                BspVertices::Fbsp(v) => v[i].colors[0],
            };
            colors.extend(c.map(|x| f64::from(x) / 255.0));
        }
        let scaled = v.get("scaled").and_then(Value::as_array).unwrap();
        for (gain, want) in [0.5, 1.0, 1.7, 2.0, 4.0].into_iter().zip(scaled) {
            let got = scale_bsp_vertex_colors(&colors, gain)
                .iter()
                .map(|&x| num(x))
                .collect::<Vec<_>>()
                .join(",");
            assert_eq!(
                fnv(&got),
                want.as_str().unwrap(),
                "{name}: vertex colours at gain {gain}"
            );
        }
    }
    assert!(faces_checked > 10_000, "{faces_checked} faces");
    eprintln!("{faces_checked} faces matched");
}
