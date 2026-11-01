//! Real matches against SHAG's own `DemoReader` (shagger
//! `tools/golden/protocol.mjs`): demos recorded with wash-fps's `demo:record`
//! (bots on the shipped maps) in `golden/demos/`, read frame by frame. Every
//! frame's state, Washoi events and recorded inputs must match.

mod events_harness;

use std::io::Read;

use events_harness::{canon, canonical, num, obj, to_js};
use flate2::read::GzDecoder;
use shagger::shared::demo::demo_reader::DemoReader;
use shagger::shared::json::{self, Value};
use washoi::shared::net::snapshot_events::WashoiEvents;

fn n(x: impl Into<f64>) -> Value {
    Value::Number(x.into())
}

#[test]
fn recorded_demos_read_like_shag() {
    let mut text = String::new();
    GzDecoder::new(&include_bytes!("golden/demo_frames.json.gz")[..])
        .read_to_string(&mut text)
        .unwrap();
    let doc = json::parse(&text).unwrap();
    let mut frames_checked = 0;
    let mut events_checked = 0;
    for demo in doc.get("demos").and_then(Value::as_array).unwrap() {
        let file = demo.get("file").and_then(Value::as_str).unwrap();
        let bytes = std::fs::read(format!("{}/tests/golden/demos/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut reader = DemoReader::new(bytes).unwrap();
        let h = reader.read_header().unwrap().clone();
        let header = obj(vec![
            ("version", n(h.version)),
            ("map", Value::String(h.map.clone())),
            ("gametype", Value::String(h.gametype.clone())),
            ("tickRate", n(h.tick_rate)),
            ("snapshotRate", n(h.snapshot_rate)),
            ("startTime", num(h.start_time)),
            ("matchId", h.match_id.clone().map_or(Value::Null, Value::String)),
            ("private", h.private.map_or(Value::Null, Value::Bool)),
            (
                "players",
                Value::Array(
                    h.players
                        .iter()
                        .map(|p| {
                            Value::Array(vec![
                                Value::String(p.player_id.clone()),
                                Value::String(p.name.clone()),
                                Value::String(p.team.clone()),
                                p.account_id.map_or(Value::Null, num),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]);
        assert_eq!(&header, demo.get("header").unwrap(), "{file}: header");
        let index = reader.read_index().unwrap();
        assert_eq!(
            f64::from(index.frame_count),
            demo.get("frameCount").and_then(Value::as_f64).unwrap()
        );
        assert_eq!(
            f64::from(index.duration_ms),
            demo.get("durationMs").and_then(Value::as_f64).unwrap()
        );

        let frames = reader.read_all_frames(&WashoiEvents).unwrap();
        let want = demo.get("frames").and_then(Value::as_array).unwrap();
        assert_eq!(frames.len(), want.len(), "{file}: frame count");
        for (k, (f, w)) in frames.iter().zip(want).enumerate() {
            assert_eq!(
                f64::from(f.tick),
                w.get("tick").and_then(Value::as_f64).unwrap(),
                "{file} frame {k}: tick"
            );
            assert_eq!(&canonical(&f.state), w.get("state").unwrap(), "{file} frame {k}: state");
            let events = Value::Array(f.state.events.iter().map(|e| canon(to_js(e))).collect());
            assert_eq!(&events, w.get("events").unwrap(), "{file} frame {k}: events");
            let inputs = Value::Array(
                f.inputs
                    .iter()
                    .map(|r| {
                        Value::Array(vec![
                            Value::String(r.player_id.clone()),
                            n(r.buttons),
                            num(r.yaw),
                            num(r.pitch),
                            num(r.forward),
                            num(r.right),
                            n(r.weapon_slot),
                            n(r.commands),
                        ])
                    })
                    .collect(),
            );
            assert_eq!(&inputs, w.get("inputs").unwrap(), "{file} frame {k}: inputs");
            frames_checked += 1;
            events_checked += f.state.events.len();
        }
    }
    assert!(
        frames_checked > 2000 && events_checked > 20_000,
        "{frames_checked} frames, {events_checked} events"
    );
}
