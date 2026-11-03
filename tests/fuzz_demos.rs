//! Mutation fuzzing of `.shagdemo` playback, on stable Rust: real demos are
//! inflated, damaged (bit flips, boundary values, cuts, splices) and played
//! through `DemoPlayback`, which must refuse them or play them, never
//! panic. The engine's parsers have their own harness (shagger's
//! `tests/fuzz_parsers.rs`); this one needs Washoi's event codec.
//!
//! Ignored by default:
//!
//! ```text
//! FUZZ_DEMOS=<dir of .shagdemo> FUZZ_ITERS=500 cargo test --release \
//!     --test fuzz_demos -- --ignored --nocapture
//! ```

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use washoi::client::demo_playback::DemoPlayback;

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

/// Loads and plays it all: seeks, speed changes and the death scan.
fn play(path: &std::path::Path) {
    let Ok(mut demo) = DemoPlayback::load(path) else { return };
    let _ = demo.scan_deaths();
    demo.set_speed(4.0);
    demo.play();
    for step in 0..2000 {
        let _ = demo.advance(1.0 / 30.0);
        if step == 500 {
            demo.seek_to_progress(0.25);
        }
        if step == 900 {
            demo.seek_to_tick(-1e9);
        }
        if demo.finished() {
            break;
        }
    }
    demo.seek_to_progress(1.0);
    let _ = demo.advance(0.1);
}

#[test]
#[ignore = "fuzzing: run by hand with FUZZ_DEMOS"]
fn fuzz_demos() {
    let Some(dir) = std::env::var_os("FUZZ_DEMOS") else {
        eprintln!("FUZZ_DEMOS is not set; nothing to fuzz");
        return;
    };
    let iterations: u64 = std::env::var("FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let out = PathBuf::from(std::env::var_os("FUZZ_OUT").unwrap_or_else(|| "target/fuzz".into()));
    std::fs::create_dir_all(&out).unwrap();
    let mut seeds: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "shagdemo"))
        .collect();
    seeds.sort();
    assert!(!seeds.is_empty());
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
    let scratch = out.join("current.shagdemo");
    let mut failures = std::collections::BTreeMap::<String, String>::new();
    for (index, seed) in seeds.iter().enumerate() {
        // Inflated, so a mutation reaches the records rather than the CRC.
        let bytes = std::fs::read(seed).unwrap();
        let mut raw = Vec::new();
        if flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut raw).is_err() {
            raw = bytes;
        }
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (index as u64 + 1));
        let started = std::time::Instant::now();
        for i in 0..iterations {
            let input = if i == 0 { raw.clone() } else { mutate(&raw, &mut rng) };
            std::fs::write(&scratch, &input).unwrap();
            if catch_unwind(AssertUnwindSafe(|| play(&scratch))).is_err() {
                let message = LAST_PANIC.with(|p| p.borrow().clone());
                failures.entry(message.clone()).or_insert_with(|| {
                    let file = out.join(format!("demo-{index}-{i}.shagdemo"));
                    std::fs::write(&file, &input).unwrap();
                    eprintln!("fuzz: PANIC {message}\n      -> {}", file.display());
                    file.display().to_string()
                });
            }
        }
        eprintln!(
            "fuzz: {iterations} runs in {:.1}s  {}",
            started.elapsed().as_secs_f64(),
            seed.display()
        );
    }
    let _ = std::panic::take_hook();
    eprintln!("fuzz: {} distinct panics", failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
}

thread_local! {
    static LAST_PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}
