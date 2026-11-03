//! `washoi bench <demo>`: the game's own frame loop (`window.rs`) playing a
//! recorded match at a fixed 60 steps a second, as fast as the machine
//! draws it (no vsync), timing every stage of every frame on the CPU and,
//! where the GPU can stamp its command stream, every pass on the GPU.
//!
//! It is the measurement the renderer's optimisation is judged by
//! (RUST.md 23): run it before and after a change, on the same demo, and
//! compare the report. Nothing here is a port; the browser's equivalent
//! was the profiler.

use std::path::PathBuf;
use std::time::Instant;

use shagger::rendering::gpu::Gpu;
use shagger::rendering::wgpu;

/// Timestamps a frame can hold: a start and one per pass.
const MAX_STAMPS: u32 = 16;

/// The timings of one stage, in milliseconds, frame by frame.
#[derive(Default)]
struct Series {
    name: &'static str,
    samples: Vec<f64>,
}

impl Series {
    fn summary(&self) -> Option<Summary> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_by(f64::total_cmp);
        let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
        Some(Summary {
            mean: sorted.iter().sum::<f64>() / sorted.len() as f64,
            p50: at(0.5),
            p95: at(0.95),
            p99: at(0.99),
            max: sorted[sorted.len() - 1],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Summary {
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

/// The GPU's own clock around each pass, read back after the frame.
struct GpuStamps {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// Nanoseconds per tick.
    period: f64,
    labels: Vec<&'static str>,
}

/// What `washoi bench` collects while the window plays the demo.
pub struct Bench {
    /// Frames to time after the warm-up, at most.
    pub frames: u64,
    /// Frames played before timing starts: pipelines and caches settle.
    pub warmup: u64,
    pub out: Option<PathBuf>,
    played: u64,
    cpu: Vec<Series>,
    gpu: Vec<Series>,
    frame: Series,
    frame_started: Option<Instant>,
    mark: Option<Instant>,
    stamps: Option<GpuStamps>,
    gpu_name: String,
}

impl Bench {
    #[must_use]
    pub fn new(frames: u64, warmup: u64, out: Option<PathBuf>) -> Self {
        Self {
            frames,
            warmup,
            out,
            played: 0,
            cpu: Vec::new(),
            gpu: Vec::new(),
            frame: Series {
                name: "frame",
                samples: Vec::new(),
            },
            frame_started: None,
            mark: None,
            stamps: None,
            gpu_name: String::new(),
        }
    }

    /// The device to stamp on, when it can stamp inside an encoder.
    pub fn attach(&mut self, gpu: &Gpu) {
        self.gpu_name = gpu.describe();
        let features = gpu.device.features();
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            println!("bench: this GPU cannot time its passes; CPU timings only");
            return;
        }
        let size = u64::from(MAX_STAMPS) * 8;
        self.stamps = Some(GpuStamps {
            queries: gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("bench"),
                ty: wgpu::QueryType::Timestamp,
                count: MAX_STAMPS,
            }),
            resolve: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bench resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bench readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            period: f64::from(gpu.queue.get_timestamp_period()),
            labels: Vec::new(),
        });
    }

    fn timing(&self) -> bool {
        self.played >= self.warmup
    }

    /// The frame starts: the stage clock resets.
    pub fn begin_frame(&mut self) {
        let now = Instant::now();
        self.frame_started = Some(now);
        self.mark = Some(now);
        if let Some(s) = &mut self.stamps {
            s.labels.clear();
        }
    }

    /// The stage `name` ended now: the time since the last mark is its.
    pub fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        let Some(last) = self.mark.replace(now) else { return };
        if !self.timing() {
            return;
        }
        let ms = (now - last).as_secs_f64() * 1000.0;
        match self.cpu.iter_mut().find(|s| s.name == name) {
            Some(series) => series.samples.push(ms),
            None => self.cpu.push(Series {
                name,
                samples: vec![ms],
            }),
        }
    }

    /// Stamps the GPU's clock: the pass `name` ends here (the first stamp
    /// of a frame is its start, named `start`).
    pub fn stamp(&mut self, encoder: &mut wgpu::CommandEncoder, name: &'static str) {
        let Some(s) = &mut self.stamps else { return };
        let index = s.labels.len() as u32;
        if index >= MAX_STAMPS {
            return;
        }
        encoder.write_timestamp(&s.queries, index);
        s.labels.push(name);
    }

    /// Copies the frame's stamps where they can be read; before submitting.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(s) = &self.stamps else { return };
        let count = s.labels.len() as u32;
        if count < 2 {
            return;
        }
        encoder.resolve_query_set(&s.queries, 0..count, &s.resolve, 0);
        encoder.copy_buffer_to_buffer(&s.resolve, 0, &s.readback, 0, u64::from(count) * 8);
    }

    /// The frame was submitted: waits for the GPU to finish it (which the
    /// `gpu wait` stage measures) and reads the passes' times.
    pub fn end_frame(&mut self, gpu: &Gpu) {
        if let Some(s) = &self.stamps
            && s.labels.len() >= 2
        {
            let count = s.labels.len();
            let slice = s.readback.slice(..count as u64 * 8);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            let ticks: Vec<u64> = slice.get_mapped_range().map_or_else(
                |_| vec![0; count],
                |view| view.as_chunks::<8>().0.iter().map(|b| u64::from_le_bytes(*b)).collect(),
            );
            s.readback.unmap();
            let timing = self.timing();
            let (period, labels) = (s.period, s.labels.clone());
            if timing {
                for (i, name) in labels.iter().enumerate().skip(1) {
                    let ms = ticks[i].saturating_sub(ticks[i - 1]) as f64 * period / 1_000_000.0;
                    match self.gpu.iter_mut().find(|s| s.name == *name) {
                        Some(series) => series.samples.push(ms),
                        None => self.gpu.push(Series {
                            name,
                            samples: vec![ms],
                        }),
                    }
                }
            }
        } else {
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        }
        self.mark("gpu wait");
        if let Some(started) = self.frame_started.take()
            && self.timing()
        {
            self.frame.samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        self.played += 1;
    }

    /// Enough frames timed.
    #[must_use]
    pub fn done(&self) -> bool {
        self.frame.samples.len() as u64 >= self.frames
    }

    /// The table, and the JSON beside it when `--out` names a file.
    #[must_use]
    pub fn report(&self, size: [u32; 2]) -> String {
        let mut out = format!(
            "bench: {} frames timed after {} warm-up, {}x{} on {}\n",
            self.frame.samples.len(),
            self.warmup.min(self.played),
            size[0],
            size[1],
            self.gpu_name
        );
        out.push_str(&format!(
            "{:<18} {:>8} {:>8} {:>8} {:>8} {:>8}\n",
            "stage (ms)", "mean", "p50", "p95", "p99", "max"
        ));
        let row = |out: &mut String, label: &str, s: &Series| {
            if let Some(m) = s.summary() {
                out.push_str(&format!(
                    "{label:<18} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3}\n",
                    m.mean, m.p50, m.p95, m.p99, m.max
                ));
            }
        };
        for s in &self.cpu {
            row(&mut out, &format!("cpu {}", s.name), s);
        }
        for s in &self.gpu {
            row(&mut out, &format!("gpu {}", s.name), s);
        }
        row(&mut out, "frame", &self.frame);
        if let Some(m) = self.frame.summary() {
            out.push_str(&format!("{:.1} fps (mean frame)\n", 1000.0 / m.mean));
        }
        out
    }

    /// The same as JSON, for comparing runs.
    #[must_use]
    pub fn json(&self, size: [u32; 2]) -> serde_json::Value {
        let series = |list: &[Series]| -> serde_json::Value {
            list.iter()
                .filter_map(|s| {
                    let m = s.summary()?;
                    Some((
                        s.name.to_owned(),
                        serde_json::json!({"mean": m.mean, "p50": m.p50, "p95": m.p95, "p99": m.p99, "max": m.max}),
                    ))
                })
                .collect::<serde_json::Map<_, _>>()
                .into()
        };
        serde_json::json!({
            "version": washoi::version_line(),
            "gpu": self.gpu_name,
            "size": size,
            "frames": self.frame.samples.len(),
            "cpu": series(&self.cpu),
            "gpu_passes": series(&self.gpu),
            "frame": series(std::slice::from_ref(&self.frame)).get("frame").cloned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_come_from_the_sorted_samples() {
        let s = Series {
            name: "x",
            samples: (1..=100).rev().map(f64::from).collect(),
        };
        let m = s.summary().unwrap();
        assert_eq!((m.p50, m.p95, m.p99, m.max), (51.0, 95.0, 99.0, 100.0));
        assert!((m.mean - 50.5).abs() < 1e-12);
    }

    #[test]
    fn warm_up_frames_are_not_timed() {
        let mut bench = Bench::new(2, 1, None);
        for _ in 0..3 {
            bench.begin_frame();
            bench.mark("update");
            bench.frame_started.take();
            bench.played += 1;
            if bench.timing() {
                bench.frame.samples.push(1.0);
            }
        }
        assert_eq!(bench.cpu[0].samples.len(), 2);
        assert!(bench.done());
    }
}
