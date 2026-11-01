//! The shader script parser against SHAG's own (shagger
//! `tools/golden/shaders.mjs`): every shader script in the shipped packages
//! and four hand-written edge-case scripts, parsed in order by one parser (so
//! templates carry over), each compared as canonical JSON.
//!
//! The hand-written scripts are in the vectors and always run. The packaged
//! ones need `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills) and are
//! skipped without it.

mod movement_harness;

use std::collections::HashMap;
use std::path::PathBuf;

use movement_harness::{fnv, num};
use shagger::loader::pk3::node_pk3::ingest_pk3_file;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_parser::Q3ShaderParser;
use shagger::shared::json::{self, Value};

/// The generator's `canonical`: keys sorted, numbers as f64 hex bits, NaN and
/// infinities as null (serde_json has them as null already).
fn canonical(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        J::Number(n) => J::String(num(n.as_f64().unwrap())),
        J::Array(a) => J::Array(a.into_iter().map(canonical).collect()),
        J::Object(o) => J::Object(o.into_iter().map(|(k, v)| (k, canonical(v))).collect()),
        other => other,
    }
}

fn parse(parser: &mut Q3ShaderParser, text: &str) -> (i64, String) {
    let (count, doc) = match parser.parse_shader_file(text) {
        Ok(shaders) => (shaders.len() as i64, serde_json::json!({ "shaders": shaders })),
        Err(_) => (-1, serde_json::json!({ "error": true })),
    };
    (count, serde_json::to_string(&canonical(doc)).unwrap())
}

#[test]
fn shader_scripts_parse_as_shag_parses_them() {
    let doc = json::parse(include_str!("golden/shaders.json")).unwrap();
    let scripts = doc.get("scripts").and_then(Value::as_array).unwrap();
    let dir = std::env::var_os("WASHOI_MAPS").map(PathBuf::from);
    let mut packages: HashMap<String, Pk3Loader> = HashMap::new();
    let mut parser = Q3ShaderParser::new();
    let (mut checked, mut shaders) = (0, 0);
    for (i, s) in scripts.iter().enumerate() {
        let source = s.get("source").and_then(Value::as_str).unwrap();
        let text = if let Some(text) = s.get("text").and_then(Value::as_str) {
            text.to_owned()
        } else {
            // A packaged script: "<package>:<path>".
            let Some(dir) = &dir else { break };
            let (package, path) = source.split_once(':').unwrap();
            let pk3 = packages.entry(package.to_owned()).or_insert_with(|| {
                let mut loader = Pk3Loader::new();
                assert!(
                    ingest_pk3_file(&mut loader, &dir.join("pk3"), package).unwrap(),
                    "{package} is missing"
                );
                loader
            });
            String::from_utf8_lossy(&pk3.get_file(path).unwrap()).into_owned()
        };
        let (count, canon) = parse(&mut parser, &text);
        let want = s.get("hash").and_then(Value::as_str).unwrap();
        assert!(
            fnv(&canon) == want && count == s.get("count").and_then(Value::as_f64).unwrap() as i64,
            "script {i} ({source}): {count} shaders, hash {} vs {want} (DUMP={i} prints the TypeScript JSON)\n{canon:.2000}",
            fnv(&canon)
        );
        checked += 1;
        shaders += count.max(0);
    }
    if dir.is_none() {
        eprintln!("packaged scripts skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        assert!(checked >= 4);
    } else {
        assert_eq!(checked, scripts.len());
        eprintln!("{checked} scripts, {shaders} shaders matched");
    }
}
