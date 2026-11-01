//! Washoi's snapshot events against what SHAG's `BinaryProtocol.decodeEvents`
//! makes of the same bytes (shagger `tools/golden/protocol.mjs`).

mod events_harness;

use events_harness::{bytes, canon, to_js};
use shagger::shared::json::{self, Value};
use shagger::shared::net::snapshot::decode_snapshot;
use washoi::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

#[test]
fn events_decode_like_shag() {
    let doc = json::parse(include_str!("golden/snapshot_events.json")).expect("golden json");
    let vectors = doc.get("vectors").and_then(Value::as_array).expect("vectors");
    let mut seen = std::collections::BTreeSet::new();
    for (i, v) in vectors.iter().enumerate() {
        let data = bytes(v.get("bytes").and_then(Value::as_str).unwrap());
        match (decode_snapshot(&data, None, &WashoiEvents), v.get("events")) {
            (Ok(state), Some(expected)) => {
                let got = Value::Array(state.events.iter().map(|e| canon(to_js(e))).collect());
                assert_eq!(&got, expected, "vector {i}");
                seen.extend(state.events.iter().map(SnapshotEvent::type_name));
            }
            (Err(_), None) => {}
            (Ok(_), None) => panic!("vector {i}: SHAG failed with {:?}, Rust decoded", v.get("error")),
            (Err(e), Some(_)) => panic!("vector {i}: SHAG decoded, Rust failed with {e}"),
        }
    }
    assert_eq!(seen.len(), 22, "event kinds covered: {seen:?}");
}
