//! Demo playback for the window: the recorded frames fed to the remote
//! players' interpolation buffer on the demo's own clock, as wash-fps's
//! demo viewer feeds `RemotePlayerRenderer`.
//!
//! wash-fps plays demos through its deterministic `DemoTimeline`, which is
//! not ported yet; here each frame enters the [`SnapshotBuffer`] at the
//! moment it was recorded, so the buffer brackets it the way a live
//! connection with no jitter would. Seeking, pausing and time scale are
//! not ported either.

use std::path::Path;

use shagger::shared::demo::demo_reader::{DemoFrame, DemoHeader, DemoReader};

use crate::client::snapshot_feed::SnapshotFeed;
pub use crate::client::snapshot_feed::{DemoView, LiveProjectile};
use crate::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

pub struct DemoPlayback {
    pub header: DemoHeader,
    frames: Vec<DemoFrame<SnapshotEvent>>,
    /// The next frame to deliver.
    next: usize,
    /// Demo time in ms since the first frame.
    clock_ms: f64,
    feed: SnapshotFeed,
}

impl DemoPlayback {
    /// Reads a `.shagdemo` (gzipped or not) and all its frames.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut reader = DemoReader::new(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let header = reader
            .read_header()
            .map_err(|e| format!("{}: {e}", path.display()))?
            .clone();
        let frames = reader
            .read_all_frames(&WashoiEvents)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if frames.is_empty() {
            return Err(format!("{}: no frames", path.display()));
        }
        Ok(Self {
            header,
            frames,
            next: 0,
            clock_ms: 0.0,
            feed: SnapshotFeed::new(),
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Demo length in seconds.
    pub fn duration(&self) -> f64 {
        self.frame_time(self.frames.len() - 1) / 1000.0
    }

    /// When frame `i` was recorded, in ms after the first.
    fn frame_time(&self, i: usize) -> f64 {
        let first = f64::from(self.frames[0].tick);
        (f64::from(self.frames[i].tick) - first) * 1000.0 / f64::from(self.header.tick_rate.max(1))
    }

    /// Demo time in seconds.
    pub fn time(&self) -> f64 {
        self.clock_ms / 1000.0
    }

    pub fn finished(&self) -> bool {
        self.next >= self.frames.len()
    }

    /// The newest frame delivered, if any.
    pub fn current(&self) -> Option<&DemoFrame<SnapshotEvent>> {
        self.next.checked_sub(1).map(|i| &self.frames[i])
    }

    /// Advances the demo clock by `dt` seconds, delivers the frames recorded
    /// by then, and samples every player and projectile in the newest one.
    pub fn advance(&mut self, dt: f64) -> DemoView {
        self.clock_ms += dt * 1000.0;
        while self.next < self.frames.len() && self.frame_time(self.next) <= self.clock_ms {
            let at = self.frame_time(self.next);
            self.feed.push(at, &self.frames[self.next].state, "");
            self.next += 1;
        }
        self.feed.sample(self.clock_ms)
    }
}

#[cfg(test)]
mod tests {
    use shagger::network::client::snapshot_buffer::InterpolatedEntity;

    use super::*;

    /// The recorded bot demos play through: players move between frames,
    /// and projectiles fly in at least one of them.
    #[test]
    fn plays_recorded_demos() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/demos");
        let mut projectiles = 0;
        let mut shots = 0;
        for file in std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "shagdemo"))
        {
            let mut demo = DemoPlayback::load(&file).unwrap();
            let mut seen = 0;
            let mut first: Option<(String, InterpolatedEntity)> = None;
            let mut moved = false;
            while !demo.finished() {
                let view = demo.advance(1.0 / 60.0);
                projectiles += view.projectiles.len();
                shots += view
                    .effects
                    .iter()
                    .filter(|e| matches!(e, SnapshotEvent::WeaponFire { .. }))
                    .count();
                seen = seen.max(view.players.len());
                if let Some((id, p)) = &first
                    && let Some((_, q)) = view.players.iter().find(|(i, _)| i == id)
                    && (q.x - p.x).abs() + (q.z - p.z).abs() > 1.0
                {
                    moved = true;
                }
                if first.is_none() {
                    first = view.players.into_iter().next();
                }
            }
            assert!(seen > 0, "no players in {}", file.display());
            assert!(moved, "nobody moved in {}", file.display());
        }
        assert!(projectiles > 0, "no projectiles flew");
        assert!(shots > 0, "nobody fired");
    }
}
