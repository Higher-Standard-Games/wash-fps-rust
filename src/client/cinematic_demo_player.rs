//! Port of wash-fps `client/CinematicDemoPlayer.ts`: plays a highlight reel.
//! It analyzes the demo, builds the reel and its camera paths, then, each
//! frame, moves through the shots as the demo's playhead passes them, eases
//! the playback speed through each shot's slow motion, and says where the
//! camera is: behind a player's eyes, or on a fly-by path aimed at the
//! players and kept in front of walls.
//!
//! The TypeScript drives Babylon's `DemoViewer` and `CameraSystem`; here the
//! player drives [`DemoPlayback`] and leaves the camera to the window, which
//! reads [`CinematicDemoPlayer::camera`].

use std::cell::RefCell;

use shagger::shared::bsp::spawn_point_table::PLAYER_ORIGIN_HEIGHT;
use shagger::shared::demo::cinematic_camera_paths::{CameraPath, generate_camera_path, interpolate_path};
use shagger::shared::demo::demo_analyzer::{DemoAnalysis, DemoAnalyzer};
use shagger::shared::demo::demo_reader::DemoHeader;
use shagger::shared::demo::sequence_builder::{self, CameraPreset, CinematicReel, CinematicSegment, Transition};
use shagger::shared::js::js_atan2;
use shagger::shared::net::game_state::Vec3;

use crate::client::demo_playback::DemoPlayback;
use crate::client::reel_cards::{ReelSummary, ReelTitle, reel_summary, reel_title};

/// `CinematicPlayerState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReelState {
    Idle,
    Playing,
    Paused,
    Ended,
}

/// How fast a speed change settles, per second of real time.
const SPEED_EASE_RATE: f64 = 12.0;
/// How long the reel fades in from black at its start and out at its end.
const REEL_FADE_MS: f64 = 1000.0;
/// How strongly a crossfade dips through black at its deepest.
const CROSSFADE_DIP: f64 = 0.6;
/// A white flash's brightest, so the frame under it still reads.
const FLASH_PEAK: f64 = 0.9;
/// How far below a player the ground is looked for when scoring airborne frags.
const GROUND_PROBE_DEPTH: f64 = 1024.0;
/// How fast the fly-by camera turns toward the players and settles, per second.
const FLYBY_FOLLOW_RATE: f64 = 8.0;
/// How far in front of a wall a pulled-in fly-by camera stops.
const WALL_MARGIN: f64 = 24.0;
/// Aim at a player's chest rather than their origin.
const CHEST_HEIGHT: f64 = 12.0;
/// Players further apart than this don't fit one fly-by shot.
const FRAME_BOTH_WITHIN: f64 = 300.0;
/// `REEL_TITLE_MS` (`DemoService.ts`): the title card's hold on black.
pub const REEL_TITLE_MS: f64 = 2800.0;

/// A full-screen veil over the picture, for the reel's transitions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReelVeil {
    pub white: bool,
    /// 0 clear to 1 covered.
    pub opacity: f64,
}

/// What the reel reads from the scene it plays in.
pub trait ReelWorld {
    /// Where a player is drawn at the playhead, or `None` while they are not in the recording.
    fn position_of(&mut self, entity_id: &str) -> Option<Vec3>;
    /// Where the line from `from` to `to` first meets the map, or `None` if it is clear.
    fn blocked_at(&mut self, from: Vec3, to: Vec3) -> Option<Vec3>;
}

/// `new DemoAnalyzer(groundClearanceIn(world)).analyze(...)`: the demo's
/// highlights, its airborne frags told by a probe of the map below the
/// killer's feet.
pub fn analyze_demo(demo: &DemoPlayback, world: &mut dyn ReelWorld) -> DemoAnalysis {
    let world = RefCell::new(world);
    // `groundClearanceIn`: feet height above the map below the origin.
    let clearance = |origin: Vec3| {
        let below = Vec3::new(origin.x, origin.y - GROUND_PROBE_DEPTH, origin.z);
        world
            .borrow_mut()
            .blocked_at(origin, below)
            .map(|floor| (origin.y - floor.y - PLAYER_ORIGIN_HEIGHT).max(0.0))
    };
    DemoAnalyzer::new(Some(&clearance)).analyze(demo.frames(), &demo.header, demo.duration_ms)
}

/// `NO_WORLD`.
pub struct NoWorld;

impl ReelWorld for NoWorld {
    fn position_of(&mut self, _: &str) -> Option<Vec3> {
        None
    }
    fn blocked_at(&mut self, _: Vec3, _: Vec3) -> Option<Vec3> {
        None
    }
}

/// Where the camera is this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum ReelCamera {
    /// Behind this player's eyes.
    Chase(String),
    /// A fly-by: where it is, where it looks, and the path's own field of
    /// view in degrees (`None`: the camera's own).
    Freefly {
        position: Vec3,
        look_at: Vec3,
        fov_degrees: Option<f64>,
    },
}

/// `ReelInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelInfo {
    pub state: ReelState,
    pub current_segment: usize,
    pub total_segments: usize,
    pub current_scene: usize,
    pub total_scenes: usize,
    pub reel_elapsed_ms: f64,
    pub reel_total_ms: f64,
    pub reel_progress: f64,
}

/// `CinematicDemoPlayer`.
pub struct CinematicDemoPlayer {
    reel: Option<CinematicReel>,
    camera_paths: Vec<CameraPath>,
    tick_rate: f64,
    shot_played_ms: f64,
    flyby_pos: Option<Vec3>,
    flyby_look: Option<Vec3>,
    current: usize,
    state: ReelState,
    speed: f64,
    chase_target: Option<String>,
    title_hold_total_ms: f64,
    title_hold_ms: f64,
    header: Option<DemoHeader>,
    analysis: Option<DemoAnalysis>,
    camera: Option<ReelCamera>,
    /// Shots that started since the last [`Self::take_segment_changes`].
    segment_changes: Vec<usize>,
}

fn sub(a: Vec3, b: Vec3) -> (f64, f64, f64) {
    (a.x - b.x, a.y - b.y, a.z - b.z)
}

impl CinematicDemoPlayer {
    #[must_use]
    pub fn new(title_hold_ms: f64) -> Self {
        Self {
            reel: None,
            camera_paths: Vec::new(),
            tick_rate: 60.0,
            shot_played_ms: 0.0,
            flyby_pos: None,
            flyby_look: None,
            current: 0,
            state: ReelState::Idle,
            speed: 1.0,
            chase_target: None,
            title_hold_total_ms: title_hold_ms,
            title_hold_ms: 0.0,
            header: None,
            analysis: None,
            camera: None,
            segment_changes: Vec::new(),
        }
    }

    /// `loadAndPlay`: analyze, build the reel and its paths, start the first shot.
    pub fn load_and_play(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld, duration_minutes: f64) {
        let header = demo.header.clone();
        let analysis = analyze_demo(demo, world);
        // The TypeScript keeps the header's rate, which a demo may leave at 0.
        self.tick_rate = demo.tick_rate();
        let reel = sequence_builder::build(&analysis, duration_minutes, f64::from(header.tick_rate));
        self.camera_paths = reel
            .segments
            .iter()
            .map(|s| {
                generate_camera_path(
                    s.camera_preset,
                    &s.highlight,
                    s.start_tick as f64,
                    s.end_tick as f64,
                    // The TypeScript passes the header's rate, 0 and all.
                    f64::from(header.tick_rate),
                )
            })
            .collect();
        self.header = Some(header);
        self.analysis = Some(analysis);
        self.reel = Some(reel);
        self.start_playback(demo, world);
    }

    #[must_use]
    pub fn reel(&self) -> Option<&CinematicReel> {
        self.reel.as_ref()
    }

    #[must_use]
    pub fn analysis(&self) -> Option<&DemoAnalysis> {
        self.analysis.as_ref()
    }

    /// Where the camera is this frame; `None` before the first shot.
    #[must_use]
    pub fn camera(&self) -> Option<&ReelCamera> {
        self.camera.as_ref()
    }

    /// The shots that started since the last call (`onSegmentChange`).
    pub fn take_segment_changes(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.segment_changes)
    }

    #[must_use]
    pub fn current_segment(&self) -> Option<&CinematicSegment> {
        self.reel.as_ref()?.segments.get(self.current)
    }

    /// `update`: once per frame, before the demo advances.
    pub fn update(&mut self, delta_ms: f64, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        if self.state != ReelState::Playing {
            return;
        }
        if self.reel.is_none() {
            return;
        }
        if self.title_hold_ms > 0.0 {
            self.title_hold_ms = (self.title_hold_ms - delta_ms).max(0.0);
            if self.title_hold_ms == 0.0 {
                demo.play();
            }
            return;
        }
        let Some(segment) = self.current_segment().cloned() else {
            return;
        };
        let playhead = demo.playhead_tick();
        // A shot cut past the last frame ends with the demo, which stops there.
        if playhead >= segment.end_tick as f64 || demo.ended() {
            self.advance_segment(demo, world);
            return;
        }
        let mut target = segment.playback_speed;
        for kf in &segment.speed_keyframes {
            if playhead >= kf.tick as f64 {
                target = kf.speed;
            }
        }
        self.shot_played_ms += delta_ms;
        self.speed += (target - self.speed) * (1.0 - (-SPEED_EASE_RATE * delta_ms / 1000.0).exp());
        if (target - self.speed).abs() < 0.005 {
            self.speed = target;
        }
        demo.set_speed(self.speed);

        match &self.camera_paths[segment.segment_index] {
            CameraPath::Chase { .. } => self.follow_chase_target(&segment, playhead, world),
            CameraPath::Freefly {
                keyframes,
                interpolation,
            } if !keyframes.is_empty() => {
                let planned = interpolate_path(keyframes, playhead, *interpolation);
                let own_fov = keyframes.iter().any(|k| k.fov.is_some());
                let (position, look_at) =
                    self.frame_flyby(&segment, planned.position, planned.look_at, delta_ms, world);
                self.camera = Some(ReelCamera::Freefly {
                    position,
                    look_at,
                    fov_degrees: own_fov.then_some(planned.fov),
                });
            }
            CameraPath::Freefly { .. } => {}
        }
    }

    pub fn pause(&mut self, demo: &mut DemoPlayback) {
        if self.state == ReelState::Playing {
            self.state = ReelState::Paused;
            demo.pause();
        }
    }

    pub fn resume(&mut self, demo: &mut DemoPlayback) {
        if self.state == ReelState::Paused {
            self.state = ReelState::Playing;
            if self.title_hold_ms == 0.0 {
                demo.play();
            }
        }
    }

    pub fn toggle_play_pause(&mut self, demo: &mut DemoPlayback) {
        match self.state {
            ReelState::Playing => self.pause(demo),
            ReelState::Paused => self.resume(demo),
            _ => {}
        }
    }

    pub fn skip_to_segment(&mut self, index: usize, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        let Some(reel) = &self.reel else { return };
        if reel.segments.is_empty() {
            return;
        }
        self.current = index.min(reel.segments.len() - 1);
        self.title_hold_ms = 0.0;
        self.apply_segment(demo, world);
        if matches!(self.state, ReelState::Paused | ReelState::Ended) {
            self.state = ReelState::Playing;
        }
    }

    /// `replay`: from the first shot, title card and opening fade and all.
    pub fn replay(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        self.skip_to_segment(0, demo, world);
        self.hold_for_title(demo);
    }

    pub fn next_segment(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        self.skip_to_segment(self.current + 1, demo, world);
    }

    pub fn prev_segment(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        self.skip_to_segment(self.current.saturating_sub(1), demo, world);
    }

    /// The reel is holding on black under its title card.
    #[must_use]
    pub fn is_showing_title(&self) -> bool {
        self.title_hold_ms > 0.0 && self.state != ReelState::Ended
    }

    #[must_use]
    pub fn state(&self) -> ReelState {
        self.state
    }

    /// The opening card.
    #[must_use]
    pub fn title(&self) -> Option<ReelTitle> {
        self.header.as_ref().map(|h| reel_title(&h.players))
    }

    /// The closing card.
    #[must_use]
    pub fn summary(&self) -> Option<ReelSummary> {
        Some(reel_summary(self.reel.as_ref()?, self.analysis.as_ref()?))
    }

    /// Where each shot starts, as fractions of the reel.
    #[must_use]
    pub fn shot_starts(&self) -> Vec<f64> {
        let total = self.total_shot_ms();
        let mut at = 0.0;
        self.reel
            .iter()
            .flat_map(|r| &r.segments)
            .map(|s| {
                let start = if total > 0.0 { at / total } else { 0.0 };
                at += s.duration_ms;
                start
            })
            .collect()
    }

    /// `getReelInfo`: read from the playhead, so a skip or a pause leaves it right.
    #[must_use]
    pub fn reel_info(&self, demo: &DemoPlayback) -> ReelInfo {
        let segments = self.reel.as_ref().map_or(&[][..], |r| &r.segments[..]);
        let segment = segments.get(self.current);
        let total = self.total_shot_ms();
        let mut elapsed: f64 = segments.iter().take(self.current).map(|s| s.duration_ms).sum();
        if let Some(s) = segment {
            let span = (s.end_tick - s.start_tick) as f64;
            let through = if span > 0.0 {
                (demo.playhead_tick() - s.start_tick as f64) / span
            } else {
                0.0
            };
            elapsed += s.duration_ms * through.clamp(0.0, 1.0);
        }
        if self.state == ReelState::Ended {
            elapsed = total;
        }
        ReelInfo {
            state: self.state,
            current_segment: self.current,
            total_segments: segments.len(),
            current_scene: segment.map_or(0, |s| s.scene),
            total_scenes: self.reel.as_ref().map_or(0, |r| r.total_scenes),
            reel_elapsed_ms: elapsed,
            reel_total_ms: total,
            reel_progress: if total > 0.0 { (elapsed / total).min(1.0) } else { 0.0 },
        }
    }

    fn total_shot_ms(&self) -> f64 {
        self.reel.iter().flat_map(|r| &r.segments).map(|s| s.duration_ms).sum()
    }

    fn hold_for_title(&mut self, demo: &mut DemoPlayback) {
        if self.title_hold_total_ms <= 0.0 {
            return;
        }
        self.title_hold_ms = self.title_hold_total_ms;
        demo.pause();
    }

    fn start_playback(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        if self.reel.as_ref().is_none_or(|r| r.segments.is_empty()) {
            self.end_reel(demo);
            return;
        }
        self.current = 0;
        self.apply_segment(demo, world);
        self.state = ReelState::Playing;
        self.hold_for_title(demo);
    }

    /// `getVeil`: a shot's way in and its way out.
    #[must_use]
    pub fn veil(&self, demo: &DemoPlayback) -> ReelVeil {
        let clear = ReelVeil {
            white: false,
            opacity: 0.0,
        };
        let Some(reel) = &self.reel else { return clear };
        if self.state == ReelState::Ended {
            let closed = reel
                .segments
                .last()
                .is_some_and(|s| s.transition_out == Transition::FadeToBlack);
            return ReelVeil {
                white: false,
                opacity: if closed { 1.0 } else { 0.0 },
            };
        }
        let Some(segment) = reel.segments.get(self.current) else {
            return clear;
        };
        if self.title_hold_ms > 0.0 {
            return ReelVeil {
                white: false,
                opacity: 1.0,
            };
        }
        let span = segment.transition_duration_ms.max(1.0);
        let first = self.current == 0;
        let last = self.current + 1 == reel.segments.len();
        let into = 1.0 - (self.shot_played_ms / if first { REEL_FADE_MS } else { span }).min(1.0);
        let ticks_left = segment.end_tick as f64 - demo.playhead_tick();
        let ms_left = ticks_left / self.tick_rate * 1000.0 / self.speed.max(0.05);
        let out = 1.0 - (ms_left.max(0.0) / if last { REEL_FADE_MS } else { span }).min(1.0);
        let mut veil = clear;
        let mut take = |white: bool, opacity: f64| {
            if opacity > veil.opacity {
                veil = ReelVeil { white, opacity };
            }
        };
        match segment.transition_in {
            Transition::FadeToBlack => take(false, into),
            Transition::WhiteFlash => take(true, FLASH_PEAK * into * into),
            Transition::Crossfade => take(false, CROSSFADE_DIP * into),
            Transition::Cut => {}
        }
        match segment.transition_out {
            Transition::FadeToBlack => take(false, out),
            Transition::WhiteFlash => take(true, FLASH_PEAK * out * out),
            _ => {}
        }
        if reel
            .segments
            .get(self.current + 1)
            .is_some_and(|n| n.transition_in == Transition::Crossfade)
        {
            take(false, CROSSFADE_DIP * out);
        }
        veil
    }

    fn apply_segment(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        let Some(segment) = self.current_segment().cloned() else {
            return;
        };
        self.shot_played_ms = 0.0;
        self.flyby_pos = None;
        self.flyby_look = None;
        demo.seek_to_tick(segment.start_tick as f64);
        self.speed = segment.playback_speed;
        demo.set_speed(self.speed);
        demo.play();
        match &self.camera_paths[segment.segment_index] {
            CameraPath::Chase { .. } => {
                self.chase_target = None;
                self.follow_chase_target(&segment, segment.start_tick as f64, world);
            }
            CameraPath::Freefly { keyframes, .. } => {
                // Snap to the first keyframe at once.
                if let Some(first) = keyframes.first() {
                    self.camera = Some(ReelCamera::Freefly {
                        position: first.position,
                        look_at: first.look_at,
                        fov_degrees: first.fov,
                    });
                }
            }
        }
        self.segment_changes.push(segment.segment_index);
    }

    /// `_followChaseTarget`: the player the shot is about. A victim shot
    /// watches the victim up to the frag and the killer after it; whoever
    /// the shot wants gives way to the other while they are not recorded.
    fn follow_chase_target(&mut self, segment: &CinematicSegment, playhead: f64, world: &mut dyn ReelWorld) {
        let killer = segment.highlight.primary_player.clone();
        let victim = segment.highlight.victim_player.clone();
        let named = match &self.camera_paths[segment.segment_index] {
            CameraPath::Chase { target_entity_id } => target_entity_id.clone(),
            CameraPath::Freefly { .. } => segment.camera_target.clone().unwrap_or_else(|| killer.clone()),
        };
        let mut wanted =
            if segment.camera_preset == CameraPreset::VictimPov && playhead >= segment.highlight.peak_tick as f64 {
                killer.clone()
            } else {
                named
            };
        let other = if wanted == killer { victim } else { Some(killer) };
        if world.position_of(&wanted).is_none()
            && let Some(other) = other
            && world.position_of(&other).is_some()
        {
            wanted = other;
        }
        if self.chase_target.as_deref() != Some(wanted.as_str()) {
            self.chase_target = Some(wanted.clone());
        }
        self.camera = Some(ReelCamera::Chase(wanted));
    }

    /// `_frameFlyby`: aim at the players as drawn and keep walls out from
    /// between; both ease, so nothing snaps.
    fn frame_flyby(
        &mut self,
        segment: &CinematicSegment,
        planned_pos: Vec3,
        planned_look: Vec3,
        delta_ms: f64,
        world: &mut dyn ReelWorld,
    ) -> (Vec3, Vec3) {
        let h = &segment.highlight;
        let killer = world.position_of(&h.primary_player);
        let victim = h.victim_player.as_deref().and_then(|v| world.position_of(v));
        let together = match (killer, victim) {
            (Some(k), Some(v)) => {
                let (dx, dy, dz) = sub(k, v);
                shagger::shared::js::js_hypot(&[dx, dy, dz]) <= FRAME_BOTH_WITHIN
            }
            _ => false,
        };
        let lead = if segment.camera_preset == CameraPreset::ReplayOrbit {
            victim.or(killer)
        } else {
            killer.or(victim)
        };
        let subjects: Vec<Vec3> = if together {
            vec![killer.unwrap_or_default(), victim.unwrap_or_default()]
        } else {
            lead.into_iter().collect()
        };
        let focus = if subjects.is_empty() {
            planned_look
        } else {
            let n = subjects.len() as f64;
            Vec3::new(
                subjects.iter().map(|p| p.x).sum::<f64>() / n,
                subjects.iter().map(|p| p.y).sum::<f64>() / n + CHEST_HEIGHT,
                subjects.iter().map(|p| p.z).sum::<f64>() / n,
            )
        };
        let mut target = planned_pos;
        if let Some(wall) = world.blocked_at(focus, planned_pos) {
            let (dx, dy, dz) = sub(focus, wall);
            let length = shagger::shared::js::js_hypot(&[dx, dy, dz]);
            let length = if length == 0.0 { 1.0 } else { length };
            let margin = WALL_MARGIN.min(length / 2.0) / length;
            target = Vec3::new(wall.x + dx * margin, wall.y + dy * margin, wall.z + dz * margin);
        }
        let k = 1.0 - (-FLYBY_FOLLOW_RATE * delta_ms / 1000.0).exp();
        let ease = |from: Option<Vec3>, to: Vec3| match from {
            Some(f) => Vec3::new(f.x + (to.x - f.x) * k, f.y + (to.y - f.y) * k, f.z + (to.z - f.z) * k),
            None => to,
        };
        let pos = ease(self.flyby_pos, target);
        let look = ease(self.flyby_look, focus);
        self.flyby_pos = Some(pos);
        self.flyby_look = Some(look);
        (pos, look)
    }

    fn advance_segment(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        self.current += 1;
        if self.reel.as_ref().is_none_or(|r| self.current >= r.segments.len()) {
            self.end_reel(demo);
            return;
        }
        self.apply_segment(demo, world);
    }

    /// The reel is the whole show: stop on its last shot.
    fn end_reel(&mut self, demo: &mut DemoPlayback) {
        demo.pause();
        self.state = ReelState::Ended;
    }
}

/// `_lookAtRotation`: (pitch, yaw) to look from `from` at `to`; positive pitch looks down.
#[must_use]
pub fn look_at_rotation(from: Vec3, to: Vec3) -> (f64, f64) {
    let (dx, dy, dz) = sub(to, from);
    let horizontal = (dx * dx + dz * dz).sqrt();
    (-js_atan2(dy, horizontal), js_atan2(dx, dz))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Every recorded demo with a reel plays it through: each shot in turn,
    /// the title first, and the end on its closing black.
    #[test]
    fn reels_play_through() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/demos");
        let mut reels = 0;
        for file in std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "shagdemo"))
        {
            let mut demo = DemoPlayback::load(&file).unwrap();
            let mut reel = CinematicDemoPlayer::new(REEL_TITLE_MS);
            reel.load_and_play(&mut demo, &mut NoWorld, 1.0);
            let shots = reel.reel().map_or(0, |r| r.segments.len());
            if shots == 0 {
                assert_eq!(reel.state(), ReelState::Ended);
                continue;
            }
            reels += 1;
            assert!(reel.is_showing_title());
            assert_eq!(reel.veil(&demo).opacity, 1.0);
            let mut seen = reel.take_segment_changes();
            let mut frames = 0;
            while reel.state() == ReelState::Playing {
                reel.update(1000.0 / 60.0, &mut demo, &mut NoWorld);
                let _ = demo.advance(1.0 / 60.0);
                seen.extend(reel.take_segment_changes());
                assert!(reel.camera().is_some());
                frames += 1;
                assert!(frames < 60 * 600, "{} never ends", file.display());
            }
            assert_eq!(seen, (0..shots).collect::<Vec<_>>(), "{}", file.display());
            assert_eq!(reel.veil(&demo).opacity, 1.0);
            let info = reel.reel_info(&demo);
            assert_eq!(info.reel_progress, 1.0);
            // About as long as the shot list says, title card aside.
            let played = f64::from(frames) / 60.0 - REEL_TITLE_MS / 1000.0;
            assert!(
                (played - info.reel_total_ms / 1000.0).abs() < 2.0 + 0.1 * played,
                "{played}"
            );
            assert!(reel.summary().is_some());
        }
        assert!(reels > 0);
    }
}
