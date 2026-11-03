//! Mutation fuzzing of live snapshots with Washoi's events, on stable
//! Rust: the golden vectors (SHAG's own encodings of every event kind) are
//! damaged, decoded with and without a baseline, and fed through the
//! client's `SnapshotFeed` as a live match would. Nothing may panic.
//!
//! Ignored by default:
//!
//! ```text
//! FUZZ_ITERS=20000 cargo test --release --test fuzz_snapshots -- --ignored --nocapture
//! ```

mod events_harness;

use std::panic::{AssertUnwindSafe, catch_unwind};

use events_harness::bytes;
use shagger::shared::json::{self, Value};
use shagger::shared::net::snapshot::decode_snapshot;
use washoi::client::snapshot_feed::SnapshotFeed;
use washoi::shared::net::snapshot_events::WashoiEvents;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    const INTERESTING: [u32; 8] = [0, 1, 0x7f, 0xff, 0xffff, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff];
    let mut data = seed.to_vec();
    for _ in 0..=rng.below(6) {
        if data.is_empty() {
            break;
        }
        // Half in the header and index, where the counts are.
        let span = if rng.below(2) == 0 {
            data.len().min(2048)
        } else {
            data.len()
        };
        let at = rng.below(span);
        match rng.below(6) {
            0 => data[at] ^= 1 << rng.below(8),
            1 => data[at] = rng.next() as u8,
            2 if at + 4 <= data.len() => {
                let v = INTERESTING[rng.below(INTERESTING.len())];
                data[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
            3 if at + 8 <= data.len() => {
                let v = [f64::NAN, f64::INFINITY, -1e300, 1e300, 0.0][rng.below(5)];
                data[at..at + 8].copy_from_slice(&v.to_le_bytes());
            }
            4 => data.truncate(at),
            _ => {
                let len = rng.below(64).min(data.len() - at);
                data.drain(at..at + len);
            }
        }
    }
    data
}

fn play(
    input: &[u8],
    baseline: Option<&shagger::shared::net::game_state::GameState<washoi::shared::net::snapshot_events::SnapshotEvent>>,
) {
    let mut feed = SnapshotFeed::new();
    for state in [
        decode_snapshot(input, None, &WashoiEvents),
        decode_snapshot(input, baseline, &WashoiEvents),
    ]
    .into_iter()
    .flatten()
    {
        for (i, id) in ["", "p1", "player_1"].iter().enumerate() {
            feed.push(f64::from(i as u32) * 50.0, &state, id);
        }
        let _ = feed.sample(75.0);
        let _ = feed.sample(1e12);
        let _ = feed.sample(f64::NAN);
    }
}

#[test]
#[ignore = "fuzzing: run by hand"]
fn fuzz_snapshots() {
    let iterations: u64 = std::env::var("FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let doc = json::parse(include_str!("golden/snapshot_events.json")).expect("golden json");
    let seeds: Vec<Vec<u8>> = doc
        .get("vectors")
        .and_then(Value::as_array)
        .expect("vectors")
        .iter()
        .filter_map(|v| v.get("bytes").and_then(Value::as_str).map(bytes))
        .collect();
    std::panic::set_hook(Box::new(|info| {
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        LAST_PANIC.with(|p| *p.borrow_mut() = format!("{at}: {message}"));
    }));
    let mut failures = std::collections::BTreeMap::<String, String>::new();
    let per_seed = (iterations / seeds.len().max(1) as u64).max(20);
    let started = std::time::Instant::now();
    for (index, seed) in seeds.iter().enumerate() {
        let baseline = decode_snapshot(seed, None, &WashoiEvents).ok();
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (index as u64 + 1));
        for i in 0..per_seed {
            let input = if i == 0 { seed.clone() } else { mutate(seed, &mut rng) };
            if catch_unwind(AssertUnwindSafe(|| play(&input, baseline.as_ref()))).is_err() {
                let message = LAST_PANIC.with(|p| p.borrow().clone());
                failures.entry(message.clone()).or_insert_with(|| {
                    let hex: String = input.iter().map(|b| format!("{b:02x}")).collect();
                    eprintln!("fuzz: PANIC {message}\n      vector {index}: {hex}");
                    hex
                });
            }
        }
    }
    let _ = std::panic::take_hook();
    eprintln!(
        "fuzz: {} seeds x {per_seed} in {:.0}s, {} distinct panics",
        seeds.len(),
        started.elapsed().as_secs_f64(),
        failures.len()
    );
    assert!(failures.is_empty(), "{} distinct panics", failures.len());
}

thread_local! {
    static LAST_PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}
