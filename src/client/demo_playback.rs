//! Demo playback for the window: the recorded frames fed to the remote
//! players' interpolation buffer on the demo's own clock, as wash-fps's
//! demo viewer feeds `RemotePlayerRenderer`.
//!
//! wash-fps plays demos through its deterministic `DemoTimeline`, which is
//! not ported yet; here each frame enters the [`SnapshotBuffer`] at the
//! moment it was recorded, so the buffer brackets it the way a live
//! connection with no jitter would. A seek replays the frames up to its
//! tick into fresh buffers (so the match state is right) and drops their
//! effects; pausing and time scale are the highlight reel's.

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
    /// Demo time per real time (`DemoViewer.setSpeed`).
    speed: f64,
    paused: bool,
    /// The index's `durationMs`.
    pub duration_ms: f64,
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
        let duration_ms = reader.read_index().map_or(0.0, |i| f64::from(i.duration_ms));
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
            speed: 1.0,
            paused: false,
            duration_ms,
        })
    }

    /// Every recorded frame, for the highlight analysis.
    pub fn frames(&self) -> &[DemoFrame<SnapshotEvent>] {
        &self.frames
    }

    /// The header's tick rate (20 when it says 0).
    pub fn tick_rate(&self) -> f64 {
        if self.header.tick_rate > 0 {
            f64::from(self.header.tick_rate)
        } else {
            20.0
        }
    }

    /// `playheadTick`: the tick the demo clock is at, between frames.
    pub fn playhead_tick(&self) -> f64 {
        f64::from(self.frames[0].tick) + self.clock_ms * self.tick_rate() / 1000.0
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.max(0.0);
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    pub fn play(&mut self) {
        self.paused = false;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// `seekToTick`: the buffers as they would be had the demo played up
    /// to `tick`, without the effects of the frames on the way.
    pub fn seek_to_tick(&mut self, tick: f64) {
        let first = f64::from(self.frames[0].tick);
        self.clock_ms = ((tick - first) * 1000.0 / self.tick_rate()).max(0.0);
        self.feed = SnapshotFeed::new();
        self.next = 0;
        while self.next < self.frames.len() && self.frame_time(self.next) <= self.clock_ms {
            let at = self.frame_time(self.next);
            self.feed.push(at, &self.frames[self.next].state, "");
            self.next += 1;
        }
        let _ = self.feed.sample(self.clock_ms);
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
        (f64::from(self.frames[i].tick) - first) * 1000.0 / self.tick_rate()
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
    /// The match as the snapshots so far tell it.
    #[must_use]
    pub fn match_state(&self) -> &crate::client::match_state_manager::MatchStateManager {
        &self.feed.match_state
    }

    /// The newest snapshot's players.
    pub fn roster(&self) -> &[(String, shagger::shared::net::game_state::PlayerState)] {
        &self.feed.roster
    }

    pub fn advance(&mut self, dt: f64) -> DemoView {
        if !self.paused {
            self.clock_ms += dt * 1000.0 * self.speed;
        }
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
