//! Port of wash-fps `client/CinematicDemoPlayer.ts`: plays a highlight reel.
//! It analyzes the demo, builds the reel and its camera paths, then, each
//! frame, moves through the shots as the demo's playhead passes them, eases
//! the playback speed through each shot's slow motion, and says where the
//! camera is: behind a player's eyes, or on a fly-by path aimed at the
//! players and kept in front of walls.
//!
//! Each shot plays at its planned speeds and holds its freeze frame, and the
//! player reports the transitions ([`CinematicDemoPlayer::veil`]) and the
//! comic effects over the picture ([`CinematicDemoPlayer::effects`]).
//!
//! The TypeScript drives Babylon's `DemoViewer` and `CameraSystem`; here the
//! player drives [`DemoPlayback`] and leaves the camera to the window, which
//! reads [`CinematicDemoPlayer::camera`]. The video export's cuts on the
//! music's beats (`alignCutsToBeats`, `cutAtMs`) are not ported: the native
//! client has no video export.

use std::cell::RefCell;

use shagger::shared::bsp::spawn_point_table::PLAYER_ORIGIN_HEIGHT;
use shagger::shared::demo::cinematic_camera_paths::{CameraPath, generate_camera_path, interpolate_path};
use shagger::shared::demo::demo_analyzer::{
    DemoAnalysis, DemoAnalyzer, HighlightCandidate, HighlightTag, HighlightType,
};
use shagger::shared::demo::demo_reader::DemoHeader;
use shagger::shared::demo::sequence_builder::{self, CameraPreset, CinematicReel, CinematicSegment, Transition};
use shagger::shared::js::js_atan2;
use shagger::shared::net::game_state::Vec3;

use crate::client::demo_playback::DemoPlayback;
use crate::client::reel_cards::{ReelSummary, ReelTitle, reel_summary, reel_title};
use crate::client::reel_effects::{ReelEffects, ReelFreezeEffect, ReelStampEffect, ReelTagEffect, ReelWipe};

/// `CinematicPlayerState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReelState {
    Idle,
    Playing,
    Paused,
    Ended,
}

fn weapon_name(weapon: &str) -> Option<&'static str> {
    Some(match weapon {
        "rlauncher" => "Rocket",
        "glauncher" => "Grenade",
        "electrobolt" => "Electrobolt",
        "lasergun" => "Laser",
        "plasmagun" => "Plasma",
        "riotgun" => "Riotgun",
        "machinegun" => "Machinegun",
        "voltaic" => "Voltaic",
        "blade" => "Blade",
        _ => return None,
    })
}

const MULTI_KILL_NAMES: [&str; 5] = ["", "Frag", "Double kill", "Triple kill", "Quad kill"];

/// `SceneTag`: what a scene's tag says: whose play it is, and what they did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTag {
    pub who: String,
    pub what: String,
}

/// `sceneTag`: the player, and the one trick that names the play.
#[must_use]
pub fn scene_tag(h: &HighlightCandidate) -> SceneTag {
    SceneTag {
        who: h.primary_player_name.clone(),
        what: trick_name(h),
    }
}

/// `trickName`.
#[must_use]
pub fn trick_name(h: &HighlightCandidate) -> String {
    let kills = h.kill_count.unwrap_or(1);
    let has = |tag: HighlightTag| h.has(tag);
    if h.kind == HighlightType::FragSequence {
        return format!("{kills}-frag run");
    }
    if kills >= 2 {
        return MULTI_KILL_NAMES
            .get(kills as usize)
            .map_or_else(|| format!("{kills} kills"), |&n| n.to_owned());
    }
    let name = if h.kind == HighlightType::Movement {
        if has(HighlightTag::WallJump) && has(HighlightTag::HeightGain) {
            "Wall climb"
        } else if has(HighlightTag::BunnyHop) {
            "Bunny hops"
        } else if has(HighlightTag::WallJump) {
            "Wall jump"
        } else if has(HighlightTag::Slide) && !has(HighlightTag::TrickJump) {
            "Slide"
        } else {
            "Trick jump"
        }
    } else if has(HighlightTag::AirRocket) {
        "Air rocket"
    } else if has(HighlightTag::Blade) {
        "Blade frag"
    } else if has(HighlightTag::RocketJump) {
        "Rocket jump frag"
    } else if has(HighlightTag::Airborne) {
        "Mid-air frag"
    } else if has(HighlightTag::LongRange) {
        "Long shot"
    } else if has(HighlightTag::LowHp) {
        "Clutch frag"
    } else if has(HighlightTag::Revenge) {
        "Revenge"
    } else if has(HighlightTag::FirstBlood) {
        "First blood"
    } else {
        return match h.weapon_id.as_deref().and_then(weapon_name) {
            Some(weapon) => format!("{weapon} frag"),
            None => "Frag".to_owned(),
        };
    };
    name.to_owned()
}

/// How long a frag counter stays on the picture.
const STAMP_MS: f64 = 900.0;
/// A scene's tag comes up this far into the scene, once its cut has landed,
/// and stays this long.
const TAG_DELAY_MS: f64 = 250.0;
const TAG_MS: f64 = 3200.0;
/// The light flash at the middle of a speed-line cut.
const WIPE_FLASH_PEAK: f64 = 0.3;
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
    /// What is left of the current freeze frame; 0 while the shot plays.
    freeze_left_ms: f64,
    /// The current shot's freeze frame has been held.
    froze_this_shot: bool,
    /// When each of the shot's frag counters came up, in shot time.
    stamp_shown_at: Vec<(i64, f64)>,
    /// Real time the current scene has played, freezes left out, for its tag.
    scene_played_ms: f64,
    /// The scene the last shot belonged to: a new one starts its tag again.
    tag_scene: Option<usize>,
    /// Where the fly-by's victim was last seen in this shot: they leave the
    /// recording when they die.
    last_victim_pos: Option<Vec3>,
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
            freeze_left_ms: 0.0,
            froze_this_shot: false,
            stamp_shown_at: Vec::new(),
            scene_played_ms: 0.0,
            tag_scene: None,
            last_victim_pos: None,
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
        // A freeze frame: the picture holds still, the camera with it.
        if self.freeze_left_ms > 0.0 {
            self.shot_played_ms += delta_ms;
            self.freeze_left_ms = (self.freeze_left_ms - delta_ms).max(0.0);
            if self.freeze_left_ms == 0.0 {
                demo.play();
            }
            return;
        }
        let playhead = demo.playhead_tick();
        // A shot cut past the last frame ends with the demo, which stops there.
        if playhead >= segment.end_tick as f64 || demo.ended() {
            self.advance_segment(demo, world);
            return;
        }
        if let Some(freeze) = &segment.freeze
            && !self.froze_this_shot
            && playhead >= freeze.tick as f64
        {
            self.froze_this_shot = true;
            self.freeze_left_ms = freeze.ms;
            demo.pause();
            return;
        }
        // The speed: the plan at the playhead, its ramps and all.
        self.shot_played_ms += delta_ms;
        self.scene_played_ms += delta_ms;
        self.speed = segment.planned_speed_at(playhead);
        demo.set_speed(self.speed);
        for stamp in &segment.stamps {
            if playhead >= stamp.tick as f64 && !self.stamp_shown_at.iter().any(|(t, _)| *t == stamp.tick) {
                self.stamp_shown_at.push((stamp.tick, self.shot_played_ms));
            }
        }

        match &self.camera_paths[segment.segment_index] {
            CameraPath::Chase { .. } => self.follow_chase_target(&segment, world),
            CameraPath::Freefly {
                keyframes,
                interpolation,
            } if !keyframes.is_empty() => {
                let planned = interpolate_path(keyframes, playhead, *interpolation);
                let own_fov = keyframes.iter().any(|k| k.fov.is_some());
                let (position, look_at) =
                    self.frame_flyby(&segment, planned.position, planned.look_at, delta_ms, playhead, world);
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
            if self.title_hold_ms == 0.0 && self.freeze_left_ms == 0.0 {
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
        // A jump shows the scene's tag again.
        self.tag_scene = None;
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
        Some(reel_summary(self.reel.as_ref()?, self.analysis.as_ref()?, |s| {
            trick_name(&s.highlight)
        }))
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
            let freeze_ms = s.freeze.as_ref().map_or(0.0, |f| f.ms);
            let frozen_ms = if self.froze_this_shot {
                freeze_ms - self.freeze_left_ms
            } else {
                0.0
            };
            elapsed += frozen_ms + (s.duration_ms - freeze_ms) * through.clamp(0.0, 1.0);
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
        self.tag_scene = None;
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
        let ms_left = self.shot_ms_left(segment, demo);
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
            Transition::SpeedWipe => take(true, WIPE_FLASH_PEAK * into * into),
            Transition::Cut => {}
        }
        match segment.transition_out {
            Transition::FadeToBlack => take(false, out),
            Transition::WhiteFlash => take(true, FLASH_PEAK * out * out),
            Transition::SpeedWipe => take(true, WIPE_FLASH_PEAK * out * out),
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

    /// `_shotMsLeft`: real time left in the shot, through its ticks at its
    /// speed now, and its freeze.
    fn shot_ms_left(&self, segment: &CinematicSegment, demo: &DemoPlayback) -> f64 {
        let ticks_left = segment.end_tick as f64 - demo.playhead_tick();
        let freeze = match &segment.freeze {
            Some(f) if !self.froze_this_shot => f.ms,
            _ => self.freeze_left_ms,
        };
        ticks_left / self.tick_rate * 1000.0 / self.speed.max(0.05) + freeze
    }

    /// `getEffects`: the comic effects over the picture now: speed lines
    /// across a cut that jumps ahead, the freeze frame's panel and caption,
    /// a frag counter and the scene's tag.
    #[must_use]
    pub fn effects(&self, demo: &DemoPlayback) -> ReelEffects {
        let mut effects = ReelEffects::default();
        let Some(segment) = self.current_segment() else {
            return effects;
        };
        if self.state == ReelState::Ended || self.title_hold_ms > 0.0 {
            return effects;
        }
        let span = segment.transition_duration_ms.max(1.0);
        // Each wipe sweeps one way, the next the other.
        let wipe_angle = |index: i64| if index % 2 == 0 { -8.0 } else { 172.0 };
        let index = self.current as i64;
        if segment.transition_in == Transition::SpeedWipe && self.shot_played_ms < span {
            effects.wipe = Some(ReelWipe {
                progress: 0.5 + 0.5 * (self.shot_played_ms / span),
                angle: wipe_angle(index - 1),
                seed: index - 1,
            });
        } else if segment.transition_out == Transition::SpeedWipe {
            let left = self.shot_ms_left(segment, demo);
            if left < span {
                effects.wipe = Some(ReelWipe {
                    progress: 0.5 * (1.0 - left.max(0.0) / span),
                    angle: wipe_angle(index),
                    seed: index,
                });
            }
        }
        if let Some(freeze) = &segment.freeze
            && self.froze_this_shot
            && self.freeze_left_ms > 0.0
        {
            effects.freeze = Some(ReelFreezeEffect {
                elapsed_ms: freeze.ms - self.freeze_left_ms,
                total_ms: freeze.ms,
                caption: freeze.caption.clone(),
                seed: index,
            });
        }
        for stamp in &segment.stamps {
            let Some(&(_, at)) = self.stamp_shown_at.iter().find(|(t, _)| *t == stamp.tick) else {
                continue;
            };
            let age_ms = self.shot_played_ms - at;
            if age_ms < STAMP_MS && effects.stamp.as_ref().is_none_or(|s| age_ms < s.age_ms) {
                effects.stamp = Some(ReelStampEffect {
                    text: stamp.text.clone(),
                    age_ms,
                    total_ms: STAMP_MS,
                });
            }
        }
        // The scene's tag, while it is new; a freeze frame has its own caption.
        let tag_age_ms = self.scene_played_ms - TAG_DELAY_MS;
        if effects.freeze.is_none() && (0.0..TAG_MS).contains(&tag_age_ms) {
            let tag = scene_tag(&segment.highlight);
            effects.tag = Some(ReelTagEffect {
                who: tag.who,
                what: tag.what,
                age_ms: tag_age_ms,
                total_ms: TAG_MS,
            });
        }
        effects
    }

    fn apply_segment(&mut self, demo: &mut DemoPlayback, world: &mut dyn ReelWorld) {
        let Some(segment) = self.current_segment().cloned() else {
            return;
        };
        self.shot_played_ms = 0.0;
        if self.tag_scene != Some(segment.scene) {
            self.tag_scene = Some(segment.scene);
            self.scene_played_ms = 0.0;
        }
        self.flyby_pos = None;
        self.flyby_look = None;
        self.freeze_left_ms = 0.0;
        self.froze_this_shot = false;
        self.stamp_shown_at.clear();
        self.last_victim_pos = None;
        demo.seek_to_tick(segment.start_tick as f64);
        self.speed = segment.planned_speed_at(segment.start_tick as f64);
        demo.set_speed(self.speed);
        demo.play();
        match &self.camera_paths[segment.segment_index] {
            CameraPath::Chase { .. } => {
                self.chase_target = None;
                self.follow_chase_target(&segment, world);
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

    /// `_followChaseTarget`: the player the shot is about. A victim's shot
    /// ends on the frag, so the camera never has to leave their eyes for the
    /// killer's. Whoever the shot wants gives way to the other while they
    /// are not recorded.
    fn follow_chase_target(&mut self, segment: &CinematicSegment, world: &mut dyn ReelWorld) {
        let killer = segment.highlight.primary_player.clone();
        let victim = segment.highlight.victim_player.clone();
        let mut wanted = match &self.camera_paths[segment.segment_index] {
            CameraPath::Chase { target_entity_id } => target_entity_id.clone(),
            CameraPath::Freefly { .. } => segment.camera_target.clone().unwrap_or_else(|| killer.clone()),
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
        demo_playhead: f64,
        world: &mut dyn ReelWorld,
    ) -> (Vec3, Vec3) {
        // A scene's ending orbits the frag itself, so when the two are apart
        // it watches the victim take the hit, and keeps watching where they
        // died once they have left the recording.
        let h = &segment.highlight;
        let killer = world.position_of(&h.primary_player);
        let victim_id = segment.victim_target.as_deref().or(h.victim_player.as_deref());
        let seen = victim_id.and_then(|v| world.position_of(v));
        if seen.is_some() {
            self.last_victim_pos = seen;
        }
        let frag_done = demo_playhead >= h.peak_tick as f64;
        let death_spot = if segment.camera_preset == CameraPreset::ReplayOrbit && frag_done {
            h.victim_pos
        } else {
            None
        };
        let victim = seen.or(self.last_victim_pos).or(death_spot);
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
            let segments = reel.reel().unwrap().segments.clone();
            let freezes = segments.iter().filter(|s| s.freeze.is_some()).count();
            let stamped = segments.iter().any(|s| !s.stamps.is_empty());
            let wiped = segments.iter().any(|s| s.transition_in == Transition::SpeedWipe);
            let (mut frozen, mut stamps, mut wipes, mut tags) = (Vec::new(), false, false, false);
            while reel.state() == ReelState::Playing {
                reel.update(1000.0 / 60.0, &mut demo, &mut NoWorld);
                let playhead = demo.playhead_tick();
                let _ = demo.advance(1.0 / 60.0);
                let effects = reel.effects(&demo);
                if let Some(freeze) = &effects.freeze {
                    // A freeze frame holds the picture still.
                    assert_eq!(demo.playhead_tick(), playhead, "{}", file.display());
                    if !frozen.contains(&reel.current) {
                        frozen.push(reel.current);
                    }
                    assert!(!freeze.caption.is_empty());
                    assert!(effects.tag.is_none());
                }
                stamps |= effects.stamp.is_some();
                wipes |= effects.wipe.is_some();
                tags |= effects.tag.is_some();
                seen.extend(reel.take_segment_changes());
                assert!(reel.camera().is_some());
                frames += 1;
                assert!(frames < 60 * 600, "{} never ends", file.display());
            }
            assert_eq!(seen, (0..shots).collect::<Vec<_>>(), "{}", file.display());
            assert_eq!(frozen.len(), freezes, "{}", file.display());
            assert_eq!(stamps, stamped, "{}", file.display());
            assert_eq!(wipes, wiped, "{}", file.display());
            assert!(tags);
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
