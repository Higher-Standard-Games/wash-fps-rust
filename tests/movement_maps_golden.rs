//! Washoi's movement on the shipped maps against wash-fps's own
//! `MovementProfile` (shagger `tools/golden/movement_maps.mjs`): players start
//! at spawn points, on teleporters and jumppads and at random points, and each
//! step's hash of every result field and the whole saved profile state must
//! match.
//!
//! The maps are not in the repository. `tools/fetch_maps.sh` downloads them
//! from the published data and extracts the BSPs; point `WASHOI_MAPS` at that
//! folder. Without it the test reports that it skipped. With it, every map in
//! the vectors must be there with the same SHA-256.

mod movement_harness;

use std::path::PathBuf;

use movement_harness::{fnv, replay, triggers};
use sha2::{Digest, Sha256};
use shagger::loader::bsp::bsp_loader::load_collision;
use shagger::shared::json::{self, Value};
use washoi::simulation::movement_profile::MovementProfile;

#[test]
fn movement_on_shipped_maps_matches_wash_fps() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = json::parse(include_str!("golden/movement_maps.json")).unwrap();
    assert_eq!(
        MovementProfile::default().determinism_fingerprint(),
        doc.get("fingerprint").and_then(Value::as_str).unwrap(),
        "movement config fingerprint"
    );
    let mut steps = 0;
    for (m, v) in doc.get("vectors").and_then(Value::as_array).unwrap().iter().enumerate() {
        let name = v.get("map").and_then(Value::as_str).unwrap();
        let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let sha = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            sha,
            v.get("sha256").and_then(Value::as_str).unwrap(),
            "{name}: not the map the vectors were made from"
        );
        let bsp = load_collision(&bytes).unwrap();
        let triggers = triggers(m, &bsp, v);
        for (run_index, run) in v.get("runs").and_then(Value::as_array).unwrap().iter().enumerate() {
            replay(&bsp, &triggers, run, |step, s| {
                let hash = s.want.as_str().unwrap();
                let line = &s.line;
                assert!(
                    fnv(line) == hash,
                    "{name} (map {m}) run {run_index} step {step}: hash {} vs {hash}\nline: {line}\n\
                     (DUMP={m},{run_index},{step} prints the TypeScript line)",
                    fnv(line)
                );
                steps += 1;
            });
        }
    }
    assert!(steps > 50_000, "{steps} steps");
}
