//! Demo playback for the window: the recorded frames fed to the remote
//! players' interpolation buffer on the demo's own clock, as wash-fps's
//! demo viewer feeds `RemotePlayerRenderer`.
//!
//! wash-fps plays demos through its deterministic `DemoTimeline`, which is
//! not ported yet; here each frame enters the [`SnapshotBuffer`] at the
//! moment it was recorded, so the buffer brackets it the way a live
//! connection with no jitter would. Seeking, pausing and time scale are
//! not ported either.

use std::collections::HashMap;
use std::path::Path;

use shagger::network::client::snapshot_buffer::{EntitySnapshot, InterpolatedEntity, SnapshotBuffer};
use shagger::shared::demo::demo_reader::{DemoFrame, DemoHeader, DemoReader};
use shagger::shared::net::game_state::GameState;

use crate::client::remote_player_logic::{apply_roster, extract_entities};
use crate::shared::net::snapshot_events::{SnapshotEvent, WashoiEvents};

/// A projectile in flight, interpolated, in Babylon axes.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProjectile {
    pub id: String,
    /// From its `projectile_spawn` event.
    pub def_id: String,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

/// What a frame shows.
#[derive(Debug, Clone, Default)]
pub struct DemoView {
    pub players: Vec<(String, InterpolatedEntity)>,
    pub projectiles: Vec<LiveProjectile>,
}

pub struct DemoPlayback {
    pub header: DemoHeader,
    frames: Vec<DemoFrame<SnapshotEvent>>,
    /// The next frame to deliver.
    next: usize,
    /// Demo time in ms since the first frame.
    clock_ms: f64,
    buffer: SnapshotBuffer,
    /// The projectiles' own buffer (`processProjectileEntities`).
    projectile_buffer: SnapshotBuffer,
    /// `proj_` entity → its definition, from `projectile_spawn` events.
    def_ids: HashMap<String, String>,
    /// Each player's team from the roster ("ffa" for none).
    pub teams: HashMap<String, String>,
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
            buffer: SnapshotBuffer::new(),
            projectile_buffer: SnapshotBuffer::new(),
            def_ids: HashMap::new(),
            teams: HashMap::new(),
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
            let frame = &self.frames[self.next];
            apply_roster(&mut self.teams, &frame.state, "");
            let entities = extract_entities(&frame.state, "");
            self.buffer.push(at, frame.tick, entities, None);
            for event in &frame.state.events {
                if let SnapshotEvent::ProjectileSpawn { entity_id, def_id } = event {
                    self.def_ids.insert(entity_id.clone(), def_id.clone());
                }
            }
            let projectiles = extract_projectiles(&frame.state);
            self.projectile_buffer.push(at, frame.tick, projectiles, None);
            self.next += 1;
        }
        let players = sample_all(&mut self.buffer, self.clock_ms);
        let projectiles = sample_all(&mut self.projectile_buffer, self.clock_ms)
            .into_iter()
            .filter_map(|(id, e)| {
                let def_id = self.def_ids.get(&id)?.clone();
                Some(LiveProjectile {
                    id,
                    def_id,
                    position: [e.x, e.y, e.z],
                    velocity: [e.vx, e.vy, e.vz],
                })
            })
            .collect();
        DemoView { players, projectiles }
    }
}

/// Every entity in the newest snapshot, sampled at `now`, by id.
fn sample_all(buffer: &mut SnapshotBuffer, now: f64) -> Vec<(String, InterpolatedEntity)> {
    buffer.begin_frame(now);
    let mut ids: Vec<&str> = buffer.latest_entity_ids().into_iter().collect();
    ids.sort_unstable();
    ids.into_iter()
        .filter_map(|id| buffer.sample_in_frame(id).map(|e| (id.to_owned(), e)))
        .collect()
}

/// `processProjectileEntities`' extraction: each `proj_` entity's position
/// and velocity.
fn extract_projectiles(state: &GameState<SnapshotEvent>) -> HashMap<String, EntitySnapshot> {
    let mut out = HashMap::new();
    for (id, entity) in state.entities.iter() {
        if !id.starts_with("proj_") {
            continue;
        }
        let Some(position) = entity.position else {
            continue;
        };
        let velocity = entity.velocity.map(|v| v.linear).unwrap_or_default();
        out.insert(
            id.to_owned(),
            EntitySnapshot {
                x: position.x,
                y: position.y,
                z: position.z,
                vx: velocity.x,
                vy: velocity.y,
                vz: velocity.z,
                ..EntitySnapshot::default()
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded bot demos play through: players move between frames,
    /// and projectiles fly in at least one of them.
    #[test]
    fn plays_recorded_demos() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/demos");
        let mut projectiles = 0;
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
    }
}
