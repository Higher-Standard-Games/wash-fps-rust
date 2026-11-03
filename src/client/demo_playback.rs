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
use shagger::shared::js::js_round;

use crate::client::snapshot_feed::SnapshotFeed;
pub use crate::client::snapshot_feed::{DemoView, LiveProjectile};
use crate::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

/// `SPEED_OPTIONS`.
pub const SPEED_OPTIONS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// `DemoDeath`: a death on the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoDeath {
    pub tick: f64,
    /// Where it sits on the timeline (0 to 1).
    pub progress: f64,
    pub victim_id: String,
    /// None for a suicide or the world.
    pub killer_id: Option<String>,
    pub victim_name: String,
    pub killer_name: Option<String>,
}

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
    /// Played to its last frame (`DemoViewer`'s "ended" state): held there
    /// until a seek, and play starts it over.
    ended: bool,
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
        // Recorded ticks are the time source: without a rate the playhead
        // cannot advance (`DemoViewer.load`).
        if header.tick_rate == 0 {
            return Err(format!(
                "{}: Legacy demo (no tickRate in header) — re-record with a current server.",
                path.display()
            ));
        }
        Ok(Self {
            header,
            frames,
            next: 0,
            clock_ms: 0.0,
            feed: SnapshotFeed::new(),
            speed: 1.0,
            paused: false,
            ended: false,
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

    /// `play`: an ended demo starts over.
    pub fn play(&mut self) {
        if self.ended {
            self.seek_to_tick(f64::from(self.frames[0].tick));
        }
        self.paused = false;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// `seekToTick`: the buffers as they would be had the demo played up
    /// to `tick`, without the effects of the frames on the way.
    pub fn seek_to_tick(&mut self, tick: f64) {
        let first = f64::from(self.frames[0].tick);
        let end = self.frame_time(self.frames.len() - 1);
        let target = ((tick - first) * 1000.0 / self.tick_rate()).clamp(0.0, end);
        self.ended = false;
        // Forward from where it is, the buffers it has hold; backward, they
        // are rebuilt from the first frame.
        if target < self.clock_ms {
            self.feed = SnapshotFeed::new();
            self.next = 0;
        }
        self.clock_ms = target;
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

    /// Whether the clock is held.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// `togglePlayPause`: an ended demo plays again from the start; one
    /// paused on its last frame plays on from there.
    pub fn toggle_play_pause(&mut self) {
        if self.paused {
            self.play();
        } else {
            self.paused = true;
        }
    }

    /// Whether playback reached the end and stopped there.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// `stepSpeed`: the next of [`SPEED_OPTIONS`] from the nearest one.
    pub fn step_speed(&mut self, step: i8) -> f64 {
        let nearest = SPEED_OPTIONS
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - self.speed).abs().total_cmp(&(b.1 - self.speed).abs()))
            .map_or(2, |(i, _)| i);
        let index = (nearest as isize + isize::from(step)).clamp(0, SPEED_OPTIONS.len() as isize - 1);
        self.speed = SPEED_OPTIONS[index as usize];
        self.speed
    }

    /// `currentFrame`: the newest frame delivered (0 before any).
    pub fn current_frame(&self) -> usize {
        self.next.saturating_sub(1)
    }

    /// `seekToFrame`, clamped to the demo.
    pub fn seek_to_frame(&mut self, frame: isize) {
        let i = frame.clamp(0, self.frames.len() as isize - 1) as usize;
        self.seek_to_tick(f64::from(self.frames[i].tick));
    }

    /// The first and last frames' ticks.
    pub fn tick_span(&self) -> (f64, f64) {
        (
            f64::from(self.frames[0].tick),
            f64::from(self.frames[self.frames.len() - 1].tick),
        )
    }

    /// `progressOfTick`: where a tick falls on the timeline (0 to 1).
    pub fn progress_of_tick(&self, tick: f64) -> f64 {
        let (first, last) = self.tick_span();
        ((tick - first) / (last - first).max(1.0)).clamp(0.0, 1.0)
    }

    /// `seekToProgress`: the frame nearest that point of the timeline.
    pub fn seek_to_progress(&mut self, progress: f64) {
        let (first, last) = self.tick_span();
        let target = js_round(first + (last - first) * progress.clamp(0.0, 1.0));
        let frame = self.frames.partition_point(|f| f64::from(f.tick) < target);
        self.seek_to_frame(frame as isize);
    }

    /// `scanDeaths`: every death in the demo, with whose frag it was.
    pub fn scan_deaths(&self) -> Vec<DemoDeath> {
        let (first, last) = self.tick_span();
        let span = (last - first).max(1.0);
        let mut deaths = Vec::new();
        for frame in &self.frames {
            for event in &frame.state.events {
                let SnapshotEvent::Death {
                    entity_id, killer_id, ..
                } = event
                else {
                    continue;
                };
                if entity_id.is_empty() {
                    continue;
                }
                let killer = (!killer_id.is_empty() && killer_id != entity_id).then(|| killer_id.clone());
                let name = |id: &str| {
                    frame.state.players.values().find(|p| p.entity_id == id).map_or_else(
                        || id.to_owned(),
                        |p| {
                            if p.name.is_empty() {
                                id.to_owned()
                            } else {
                                p.name.clone()
                            }
                        },
                    )
                };
                deaths.push(DemoDeath {
                    tick: f64::from(frame.tick),
                    progress: (f64::from(frame.tick) - first) / span,
                    victim_name: name(entity_id),
                    killer_name: killer.as_deref().map(name),
                    victim_id: entity_id.clone(),
                    killer_id: killer,
                });
            }
        }
        deaths
    }

    /// Demo time in seconds.
    pub fn time(&self) -> f64 {
        self.clock_ms / 1000.0
    }

    pub fn finished(&self) -> bool {
        self.ended || self.next >= self.frames.len()
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
            let end = self.frame_time(self.frames.len() - 1);
            self.clock_ms = (self.clock_ms + dt * 1000.0 * self.speed).min(end);
            // `playheadMs` stops at the duration and the state is "ended".
            if self.clock_ms >= end {
                self.paused = true;
                self.ended = true;
            }
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

    fn first_demo() -> DemoPlayback {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/demos");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "shagdemo"))
            .collect();
        files.sort();
        DemoPlayback::load(&files[0]).unwrap()
    }

    /// The clock stops on the last frame (`DemoViewer`'s "ended"), a seek
    /// back works from there in one step, and play starts an ended demo over.
    #[test]
    fn the_end_holds_and_play_starts_over() {
        let mut demo = first_demo();
        let (first, last) = demo.tick_span();
        for _ in 0..(demo.duration() as usize + 30) {
            let _ = demo.advance(1.0);
        }
        assert!(demo.ended() && demo.paused());
        assert_eq!(demo.playhead_tick(), last);
        demo.seek_to_tick(last - 5.0 * demo.tick_rate());
        assert!(!demo.ended());
        assert!((demo.playhead_tick() - (last - 5.0 * demo.tick_rate())).abs() < 1e-6);
        demo.seek_to_tick(last + 100.0);
        assert_eq!(demo.playhead_tick(), last);
        // Paused on the last frame by a seek, play goes on from there.
        demo.pause();
        demo.toggle_play_pause();
        assert_eq!(demo.playhead_tick(), last);
        let _ = demo.advance(1.0);
        assert!(demo.ended());
        demo.toggle_play_pause();
        assert_eq!(demo.playhead_tick(), first);
        assert!(!demo.paused());
    }

    /// A seek forward keeps the buffers it has and lands where a seek from
    /// the start would.
    #[test]
    fn a_forward_seek_matches_one_from_the_start() {
        let mut a = first_demo();
        let mut b = first_demo();
        let (first, last) = a.tick_span();
        let mid = first + (last - first) / 2.0;
        a.seek_to_tick(first + (last - first) / 4.0);
        a.seek_to_tick(mid);
        b.seek_to_tick(mid);
        assert_eq!(a.current_frame(), b.current_frame());
        let (va, vb) = (a.advance(0.05), b.advance(0.05));
        assert_eq!(va.players.len(), vb.players.len());
        for ((ia, pa), (ib, pb)) in va.players.iter().zip(vb.players.iter()) {
            assert_eq!(ia, ib);
            assert!((pa.x - pb.x).abs() < 1e-9 && (pa.z - pb.z).abs() < 1e-9);
        }
    }
}
