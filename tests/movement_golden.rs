//! Washoi's movement against wash-fps's own `MovementProfile` (shagger
//! `tools/golden/movement.mjs`): scripted players on synthetic maps, stepped
//! as `V2NetworkClient` predicts (sanitize, lifts, knockback drag, simulate).
//! Each step's position, velocity and a hash of every result field and the
//! whole saved profile state must match.

mod movement_harness;

use movement_harness::{fnv, hex, num, replay, triggers};
use shagger::loader::bsp::bsp_loader::load_collision;
use shagger::shared::json::{self, Value};
use washoi::simulation::movement_profile::MovementProfile;

#[test]
fn movement_matches_wash_fps() {
    let doc = json::parse(include_str!("golden/movement.json")).unwrap();
    assert_eq!(
        MovementProfile::default().determinism_fingerprint(),
        doc.get("fingerprint").and_then(Value::as_str).unwrap(),
        "movement config fingerprint"
    );
    let mut steps = 0;
    for (m, v) in doc.get("vectors").and_then(Value::as_array).unwrap().iter().enumerate() {
        let bsp = load_collision(&hex(v.get("bytes").and_then(Value::as_str).unwrap())).unwrap();
        let triggers = triggers(m, &bsp, v);
        for (run_index, run) in v.get("runs").and_then(Value::as_array).unwrap().iter().enumerate() {
            replay(&bsp, &triggers, run, |step, s| {
                let want = s.want.as_array().unwrap();
                let (p, v) = (s.position, s.velocity);
                let got_pv = [p.x, p.y, p.z, v.x, v.y, v.z].map(num);
                let want_pv: Vec<&str> = want[..6].iter().map(|x| x.as_str().unwrap()).collect();
                let hash = want[6].as_str().unwrap();
                let line = &s.line;
                assert!(
                    got_pv.iter().zip(&want_pv).all(|(a, b)| a == b) && fnv(line) == hash,
                    "map {m} run {run_index} step {step}: position/velocity {got_pv:?}, wash-fps {want_pv:?}; \
                     hash {} vs {hash}\nline: {line}\n(DUMP={m},{run_index},{step} prints the TypeScript line)",
                    fnv(line)
                );
                steps += 1;
            });
        }
    }
    assert!(steps > 20_000, "{steps} steps");
}
