//! The KTX1 loader against SHAG's own (shagger `tools/golden/ktx.mjs`):
//! every .ktx in the shipped packages parsed and decoded to the RGBA SHAG
//! uploads when the GPU takes no compressed formats (the software ETC1 decode
//! and the uncompressed conversions), compared by SHA-256.
//!
//! Needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills); skipped
//! without it.

use std::collections::HashMap;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use shagger::loader::pk3::node_pk3::ingest_pk3_file;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::texture::ktx1_loader::{ktx1_rgba, parse_ktx1};
use shagger::shared::json::{self, Value};

#[test]
fn ktx_files_decode_as_shag_decodes_them() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = json::parse(include_str!("golden/ktx.json")).unwrap();
    let files = doc.get("files").and_then(Value::as_array).unwrap();
    let mut packages: HashMap<String, Pk3Loader> = HashMap::new();
    let mut decoded = 0;
    for f in files {
        let source = f.get("source").and_then(Value::as_str).unwrap();
        let (package, path) = source.split_once(':').unwrap();
        let pk3 = packages.entry(package.to_owned()).or_insert_with(|| {
            let mut loader = Pk3Loader::new();
            assert!(ingest_pk3_file(&mut loader, &dir.join("pk3"), package).unwrap());
            loader
        });
        let bytes = pk3.get_file(path).unwrap();
        let parsed = parse_ktx1(&bytes);
        if let Some(error) = f.get("error").and_then(Value::as_str) {
            assert_eq!(parsed.unwrap_err().0, error, "{source}");
            continue;
        }
        let rgba = ktx1_rgba(&parsed.unwrap());
        let Some(want) = f.get("sha256").and_then(Value::as_str) else {
            assert!(rgba.is_none(), "{source}: SHAG does not decode it");
            continue;
        };
        let (w, h, pixels) = rgba.unwrap_or_else(|| panic!("{source}: not decoded"));
        assert_eq!(
            f64::from(w),
            f.get("width").and_then(Value::as_f64).unwrap(),
            "{source}"
        );
        assert_eq!(
            f64::from(h),
            f.get("height").and_then(Value::as_f64).unwrap(),
            "{source}"
        );
        assert_eq!(format!("{:x}", Sha256::digest(&pixels)), want, "{source}");
        decoded += 1;
    }
    eprintln!("{} KTX files, {decoded} decoded, all as SHAG decodes them", files.len());
    // The beta9 packages ship their textures as WebP; the minimaps stay KTX.
    assert!(decoded > 0);
}
