//! `washoi view`: a map drawn by the renderer (RUST.md 21.10),
//! from a spawn point. With `--screenshot` it renders one frame offscreen and
//! writes a PNG; otherwise it opens a window with a free camera. Players
//! given with `--player` stand on the map as the Vanguard model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use shagger::animation::player_model_manager::PlayerModelVisualStyle;
use shagger::audio::audio_v2_sfx::AudioV2Sfx;
use shagger::babylon::post_processes::image_processing_post_process::{
    ImageProcessingConfiguration, ImageProcessingPostProcess, Vignette, VignetteBlendMode,
};
use shagger::lighting::sky_light_system::bsp_mode_lights;
use shagger::loader::bsp::bsp_loader::{BspFile, load, load_collision};
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::texture::resolve_and_load_texture;
use shagger::network::client::snapshot_buffer::{EntitySnapshot, InterpolatedEntity};
use shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT;
use shagger::physics::sweep_backend_adapter::SweepBackendAdapter;
use shagger::physics::sweep_collision_backend::SweepCollisionBackend;
use shagger::rendering::camera::Camera;
use shagger::rendering::dynamic_lights::DynamicLight;
use shagger::rendering::effect_quad_batch::EffectFrame;
use shagger::rendering::glam::{DVec3, Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::hdr_target::HDR_FORMAT;
use shagger::rendering::model_renderer::{ModelDraw, ModelRenderer};
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_hdr_with_ui_to_rgba};

use shagger::rendering::sky_renderer::{SkyRenderer, apply_skybox};
use shagger::rendering::wgpu;
use shagger::rendering::world_culling::WorldCulling;
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::entities::projectile_def::ProjectileDef;
use shagger::shared::net::game_state::PlayerState;
use shagger::shared::spread_utils::mulberry32;
use shagger::weapon::explosion::explosion_batch_renderer::{ExplosionBatchRenderer, ExplosionLook};
use shagger::weapon::explosion_effect::{explosion_flash, explosion_for};
use shagger::weapon::hitscan_effects::{HitscanEffects, ShotWorld};
use shagger::weapon::impact::impact_spark_renderer::ImpactSparkBurst;
use shagger::weapon::muzzle::muzzle_flash_renderer::{MuzzleFlashSpawn, MuzzleFlashStyle};
use shagger::weapon::point_light_pool::{PointLightPool, PooledLight};
use shagger::weapon::projectile_manager::{
    MAX_TRAIL_POINTS, ProjectileModel, ProjectileTrail, load_projectile_model, projectile_matrix,
};
use shagger::weapon::rail::rail_profiles::{RailEffectSample, RailImpactKind};
use shagger::weapon::tracer::streak_tracer_renderer::StreakTracerShot;
use shagger::weapon::weapon_view_manager::ViewModelInput;
use washoi::client::demo_playback::{DemoPlayback, DemoView, LiveProjectile};
use washoi::client::fps_hud::{FpsHud, WatchFrame};
use washoi::client::game_events::{EffectTargets, GameAudio, play_remote_shots};
use washoi::client::live_play::{LivePlay, PlayInput};
use washoi::client::live_watch::LiveWatch;
use washoi::client::match_state_manager::MatchStateManager;
use washoi::client::weapon_view::WeaponViewModel;
use washoi::shared::entities::definitions::projectiles::{PROJECTILES, projectile_def};
use washoi::shared::net::snapshot_events::SnapshotEvent;

use washoi::client::player_model_visual_settings::{PlayerTeamSlot, get_player_model_visual_style};
use washoi::client::remote_player_renderer::{MODEL_SCALE, RemotePlayerRenderer};
use washoi::shared::bsp::pickup_table::{PickupEntry, extract_pickups};
use washoi::shared::items::item_throw::land_map_pickups;
use washoi::world::fps_pickup::build_weapon_pickups;
use washoi::world::pickups::procedural::procedural_pickup_factory::{ProceduralPickups, build_procedural_pickups};
use washoi::world::pickups::procedural::procedural_pickup_visual::camera_rotation;

/// `SPAWN_CLASSNAMES` (wash-fps `shared/bsp/BspDataLoader.ts`).
const SPAWN_CLASSNAMES: [&str; 4] = [
    "info_player_deathmatch",
    "info_player_start",
    "info_player_team1",
    "info_player_team2",
];

/// The clear colour where no sky is drawn.
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.12,
    g: 0.14,
    b: 0.18,
    a: 1.0,
};

/// `--audio-out`'s rate.
const AUDIO_OUT_RATE: u32 = 48_000;

/// Interleaved stereo f32 as a 16-bit PCM WAV.
fn write_wav(path: &Path, rate: u32, pcm: &[f32]) -> std::io::Result<()> {
    let data: Vec<u8> = pcm
        .iter()
        .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    std::fs::write(path, out)
}

pub struct ViewOptions {
    pub seconds: Option<f64>,
    pub map: String,
    pub data: PathBuf,
    pub screenshot: Option<PathBuf>,
    pub spawn: usize,
    pub width: u32,
    pub height: u32,
    /// Seconds of shader animation before a screenshot.
    pub time: f64,
    /// A Q3 position and yaw to view from instead of a spawn point.
    pub at: Option<[f32; 4]>,
    /// Degrees; negative looks up.
    pub pitch: f32,
    /// Draw every batch instead of culling.
    pub no_cull: bool,
    /// Leave the projectiles' and explosions' lights off.
    pub no_lights: bool,
    pub players: Vec<PlayerView>,
    pub projectiles: Vec<ProjectileView>,
    pub explosions: Vec<ExplosionView>,
    pub effects: Vec<EffectView>,
    /// The first-person weapon to hold (`--weapon`), and whether it is
    /// aimed down its sights (`--aim`).
    pub weapon: Option<String>,
    pub aim: bool,
    /// A demo to play on its map (`washoi demo`); `time` seeks it.
    pub demo: Option<PathBuf>,
    /// A live match to spectate (`washoi watch`): server, name, ticket.
    pub watch: Option<(String, String, Option<String>)>,
    /// The player (by place in id order) to put the camera behind.
    pub follow: Option<usize>,
    /// Where to write the demo's or match's sound (`--audio-out`), mixed
    /// offline as heard from the camera.
    pub audio_out: Option<PathBuf>,
    /// Hold the scoreboard open (`--scoreboard`, Tab in the window).
    pub scoreboard: bool,
    /// Join the match in `watch` as a player (`washoi play`).
    pub join: bool,
    /// Something to say once in the match (`--say`, headless play).
    pub say: Option<String>,
}

/// The HUD's frame of a demo or a match being watched, behind the
/// followed player (if any).
#[allow(clippy::too_many_arguments)]
pub fn update_watch_hud(
    hud: &mut FpsHud,
    feed: &Feed,
    players: &[(String, InterpolatedEntity, PlayerModelVisualStyle)],
    follow: Option<usize>,
    yaw: f64,
    events: &[SnapshotEvent],
    dt: f64,
    width: f32,
    height: f32,
) {
    let local = feed.local_entity();
    let view_entity = local.or_else(|| follow.and_then(|i| players.get(i)).map(|(id, _, _)| id.as_str()));
    let speed = match feed {
        Feed::Play(p, _) => {
            let v = p.velocity();
            v.x.hypot(v.z)
        }
        _ => follow
            .and_then(|i| players.get(i))
            .map_or(0.0, |(_, e, _)| e.vx.hypot(e.vz)),
    };
    let mut positions: Vec<(String, [f64; 3])> =
        players.iter().map(|(id, e, _)| (id.clone(), [e.x, e.y, e.z])).collect();
    if let Feed::Play(p, _) = feed
        && let Some(eye) = p.eye()
    {
        positions.push((p.entity_id.clone(), [eye.x, eye.y, eye.z]));
    }
    hud.update_watching(
        &WatchFrame {
            playing: local.is_some(),
            positions: &positions,
            yaw,
            roster: feed.roster(),
            match_state: feed.match_state(),
            events,
            view_entity,
            playback: matches!(feed, Feed::Demo(_)),
            speed,
            dt,
        },
        width,
        height,
    );
}

/// A chase camera behind a player: 150 units back and 40 up along the
/// player's yaw, looking slightly down.
pub fn chase_camera(camera: &mut Camera, player: &InterpolatedEntity) {
    let yaw = player.yaw as f32;
    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let at = Vec3::new(player.x as f32, player.y as f32, player.z as f32);
    camera.position = at - forward * 150.0 + Vec3::Y * 40.0;
    camera.yaw = yaw;
    camera.pitch = 0.15;
}

/// The sounds of a frame's events, heard at `camera` from the followed
/// player (if any), at `now` seconds.
#[allow(clippy::too_many_arguments)]
pub fn play_view_sounds(
    audio: &mut GameAudio,
    renderer: &mut MapRenderer,
    camera: &Camera,
    view: &DemoViewSounds<'_>,
    players: &[(String, InterpolatedEntity, PlayerModelVisualStyle)],
    follow: Option<usize>,
    dt: f64,
) {
    let (effects, teams, now) = (view.effects, view.teams, renderer.effect_time);
    audio
        .sfx
        .set_listener(camera.position.as_dvec3(), camera.forward().as_dvec3(), DVec3::Y);
    let view_entity = view
        .local
        .or_else(|| follow.and_then(|i| players.get(i)).map(|(id, _, _)| id.as_str()));
    let position = |id: &str| {
        players
            .iter()
            .find(|(p, _, _)| p == id)
            .map(|(_, e, _)| DVec3::new(e.x, e.y, e.z))
    };
    audio.play_event_sounds(effects, view_entity, teams, &position, now * 1000.0);
    audio.play_movement_sounds(view.snapshots, view_entity, teams, &mut |at| {
        renderer.ground_surface_flags(at)
    });
    // A spectator: the match flow is called, never a lead; the bomb's
    // lines are read for the side of the player followed.
    let attacking = view
        .match_state
        .is_attacking(view_entity.and_then(|id| teams.get(id)).map(String::as_str));
    audio.update_announcer(view.match_state, false, None, attacking, dt, now * 1000.0);
    audio.update_music(view.match_state, dt);
}

/// What [`play_view_sounds`] needs of a frame.
pub struct DemoViewSounds<'a> {
    pub effects: &'a [SnapshotEvent],
    pub teams: &'a HashMap<String, String>,
    pub snapshots: &'a [HashMap<String, EntitySnapshot>],
    pub match_state: &'a MatchStateManager,
    /// Our own entity when playing.
    pub local: Option<&'a str>,
}

/// Where the moving players and projectiles come from.
pub enum Feed {
    Demo(DemoPlayback),
    Live(LiveWatch),
    /// Playing: the connection, and this frame's keys and mouse.
    Play(Box<LivePlay>, PlayInput),
}

impl Feed {
    /// The game server, for a live match.
    #[must_use]
    pub fn server_url(&self) -> Option<String> {
        match self {
            Self::Demo(_) => None,
            Self::Live(l) => Some(l.url.clone()),
            Self::Play(p, _) => Some(p.url.clone()),
        }
    }

    /// The next frame, `dt` seconds on (a live match keeps its own clock).
    pub fn advance(&mut self, dt: f64) -> DemoView {
        match self {
            Self::Demo(d) => d.advance(dt),
            Self::Live(l) => l.advance(),
            Self::Play(p, input) => p.advance(input, dt),
        }
    }

    /// The match as the snapshots so far tell it.
    #[must_use]
    pub fn match_state(&self) -> &MatchStateManager {
        match self {
            Self::Demo(d) => d.match_state(),
            Self::Live(l) => l.match_state(),
            Self::Play(p, _) => p.match_state(),
        }
    }

    /// The newest snapshot's players.
    #[must_use]
    pub fn roster(&self) -> &[(String, PlayerState)] {
        match self {
            Self::Demo(d) => d.roster(),
            Self::Live(l) => l.roster(),
            Self::Play(p, _) => p.roster(),
        }
    }

    /// Why the feed has nothing more to show, once it has not.
    pub fn ended(&self) -> Option<String> {
        match self {
            Self::Demo(d) => d.finished().then(|| "demo ended".to_owned()),
            Self::Live(l) => l.closed.clone(),
            Self::Play(p, _) => p.closed.clone(),
        }
    }

    /// Our own entity when playing.
    #[must_use]
    pub fn local_entity(&self) -> Option<&str> {
        match self {
            Self::Play(p, _) if !p.entity_id.is_empty() => Some(&p.entity_id),
            _ => None,
        }
    }
}

/// An explosion held at an age (`--explosion`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExplosionView {
    /// The projectile whose explosion it is.
    pub id: String,
    /// Its Quake origin.
    pub origin: [f32; 3],
    /// Seconds since it went off.
    pub age: f64,
}

/// A bullet streak, muzzle flash, impact or rail held at an age (`--tracer`,
/// `--muzzle`, `--impact`, `--rail`), in Quake axes.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectView {
    Tracer {
        start: [f64; 3],
        end: [f64; 3],
        color: [f32; 3],
        age: f64,
    },
    Muzzle {
        position: [f64; 3],
        color: [f32; 3],
        style: MuzzleFlashStyle,
        scale: f64,
        duration: f64,
        age: f64,
    },
    Impact {
        position: [f64; 3],
        normal: [f64; 3],
        incoming: [f64; 3],
        color: [f32; 3],
        heavy: bool,
        age: f64,
    },
    Lightning {
        mode: LightningMode,
        start: [f64; 3],
        end: [f64; 3],
        color: [f32; 3],
        kind: RailImpactKind,
        age: f64,
    },
    Rail {
        start: [f64; 3],
        end: [f64; 3],
        color: [f32; 3],
        kind: RailImpactKind,
        normal: Option<[f64; 3]>,
        age: f64,
    },
}

/// Which Voltaic beam `--lightning` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightningMode {
    Hero,
    Remote,
    Chain,
}

impl LightningMode {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "hero" => Some(Self::Hero),
            "remote" => Some(Self::Remote),
            "chain" => Some(Self::Chain),
            _ => None,
        }
    }
}

/// A Quake point or direction in Babylon axes.
fn babylon([x, y, z]: [f64; 3]) -> DVec3 {
    DVec3::new(x, z, y)
}

/// A projectile held still on the map (`--projectile`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileView {
    /// The definition's id.
    pub id: String,
    /// Its Quake origin.
    pub origin: [f32; 3],
    /// The flight's Quake yaw and pitch in degrees (pitch: positive is
    /// down).
    pub yaw: f32,
    pub pitch: f32,
}

impl ProjectileView {
    /// Babylon position and flight direction.
    #[must_use]
    pub fn babylon(&self) -> (Vec3, Vec3) {
        let [x, y, z] = self.origin;
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        // Quake forward, (cos yaw cos pitch, sin yaw cos pitch, -sin pitch),
        // in Babylon axes.
        let dir = Vec3::new(yaw.cos() * pitch.cos(), -pitch.sin(), yaw.sin() * pitch.cos());
        (Vec3::new(x, z, y), dir)
    }
}

/// A player model standing still on the map (`--player`).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerView {
    /// The player's Quake origin.
    pub origin: [f32; 3],
    /// Quake yaw in degrees.
    pub angle: f32,
    pub clip: String,
    /// The clip's frame, held.
    pub frame: f64,
}

impl PlayerView {
    /// The player as the snapshot buffer would hand it over: Babylon space,
    /// standing on the ground, Babylon's yaw.
    #[must_use]
    pub fn interpolated(&self) -> InterpolatedEntity {
        let [x, y, z] = self.origin.map(f64::from);
        InterpolatedEntity {
            x,
            y: z,
            z: y,
            yaw: (90.0 - f64::from(self.angle)).to_radians(),
            grounded: true,
            ..Default::default()
        }
    }
}

/// The packages, their shader scripts, the map and its spawn points.
pub struct LoadedMap {
    pub pk3: Pk3Loader,
    pub shaders: Q3ShaderRegistry,
    pub bsp: BspFile,
    pub spawns: Vec<Camera>,
    /// The map's pickups where they come to rest (`landMapPickups`).
    pub pickups: Vec<PickupEntry>,
    /// Players to stand on the map.
    pub players: Vec<PlayerView>,
    /// Projectiles to hold on the map.
    pub projectiles: Vec<ProjectileView>,
    /// Explosions to hold on the map.
    pub explosions: Vec<ExplosionView>,
    /// Streaks, flashes and impacts to hold on the map.
    pub effects: Vec<EffectView>,
    /// The first-person weapon, if any.
    pub weapon: Option<String>,
    /// Load the player model for players that move (a demo or a match).
    pub remote_players: bool,
    /// Load every weapon's view model, for a player in the match to hold
    /// whichever the server says.
    pub hands: bool,
    /// The map's brushes, for the shots of a demo or match to stop on.
    pub shot_world: Option<SweepCollisionBackend>,
}

/// Reads every package in `data` and the map's BSP from them.
pub fn load_map(data: &Path, map: &str) -> Result<LoadedMap, String> {
    let mut pk3 = Pk3Loader::new();
    let count = ingest_pk3_dir(&mut pk3, data);
    if count == 0 {
        return Err(format!("no packages in {}", data.display()));
    }
    let path = format!("maps/{map}.bsp");
    let bytes = pk3
        .get_file(&path)
        .ok_or_else(|| format!("{path} is in none of the {count} packages"))?;
    let bsp = load(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let collision = load_collision(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let table = extract_pickups(&collision.entities);
    let mut world = SharedCollisionWorld::new(SweepBackendAdapter {
        backend: SweepCollisionBackend::new(collision),
    });
    let pickups = land_map_pickups(&table, Some(&mut world));
    let spawns = bsp
        .common
        .entities
        .iter()
        .filter(|e| SPAWN_CLASSNAMES.contains(&e.classname.as_str()))
        .filter_map(|e| {
            let origin: Vec<f32> = e
                .properties
                .get("origin")?
                .split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect();
            let angle = e
                .properties
                .get("angle")
                .and_then(|a| a.trim().parse().ok())
                .unwrap_or(0.0);
            let [x, y, z] = origin[..] else { return None };
            Some(Camera::from_q3([x, y, z + DEFAULT_VIEWHEIGHT as f32], angle))
        })
        .collect();
    let mut shaders = Q3ShaderRegistry::new();
    shaders.load_shaders(&mut pk3);
    Ok(LoadedMap {
        pk3,
        shaders,
        bsp,
        spawns,
        pickups,
        players: Vec::new(),
        projectiles: Vec::new(),
        explosions: Vec::new(),
        effects: Vec::new(),
        weapon: None,
        remote_players: false,
        hands: false,
        shot_world: Some(SweepCollisionBackend::new(
            load_collision(&bytes).map_err(|e| format!("{path}: {e}"))?,
        )),
    })
}

/// The map's sky, world and pickup models, and which world batches the
/// camera can see.
pub struct MapRenderer {
    pub world: WorldRenderer,
    pub models: ModelRenderer,
    /// Draws that stay put: weapon pickups and players.
    pub model_draws: Vec<ModelDraw>,
    /// Health, armour, ammo and the quad, posed each frame.
    pub items: ProceduralPickups,
    /// Seconds since the items were built (`visualElapsed`).
    pub item_time: f64,
    /// This frame's draws.
    frame_draws: Vec<ModelDraw>,
    /// Projectile models and where they are held.
    pub projectiles: Vec<(ProjectileModel, Mat4)>,
    /// Their trails, with the definitions' light colours.
    pub trails: Vec<(ProjectileTrail, Option<Vec3>)>,
    /// Held explosions, drawn at their ages (the clock stays at 0).
    pub explosions: ExplosionBatchRenderer,
    /// Bullet streaks, pellets, rails, Voltaic beams, muzzle flashes and
    /// impact sparks: held ones, and the shots of a demo or match.
    pub hitscan: HitscanEffects,
    /// What the shots of a demo or match stop on.
    shot_world: Option<SweepCollisionBackend>,
    /// The joint palettes `model_draws` index, then the live players'.
    pub joints: Vec<Mat4>,
    /// How many of `joints` belong to `model_draws`.
    static_joints: usize,
    /// The player model, when players are drawn.
    pub remote_players: Option<RemotePlayerRenderer>,
    /// This frame's live players and projectiles.
    live_draws: Vec<ModelDraw>,
    /// Every projectile type's model, for projectiles that fly.
    live_projectile_models: Vec<(&'static ProjectileDef, ProjectileModel)>,
    /// The flying projectiles' trails, by entity, and finished ones kept
    /// for the next projectile of their type.
    live_trails: HashMap<String, (&'static ProjectileDef, ProjectileTrail)>,
    spare_trails: Vec<(&'static str, ProjectileTrail)>,
    /// The flying projectiles' pool lights, by entity.
    live_projectile_lights: HashMap<String, PooledLight>,
    /// The pool's clock for them.
    live_light_clock: f64,
    /// The effects' clock (seconds) for a demo or match; held effects
    /// (`--explosion` and the like) stay at 0.
    pub effect_time: f64,
    /// Explosion flashes from events: their lights, looks and start times.
    live_flashes: Vec<(PooledLight, &'static ExplosionLook, f64)>,
    pub sky: Option<SkyRenderer>,
    pub culling: WorldCulling,
    /// Off draws every batch (`--no-cull`), for comparing.
    pub cull: bool,
    /// The effect lights (projectiles, explosion flashes).
    pub lights: PointLightPool,
    /// Off leaves them dark (`--no-lights`).
    pub dynamic_lights: bool,
    /// Image processing from the HDR target into the output.
    pub post: ImageProcessingPostProcess,
    pub image_processing: ImageProcessingConfiguration,
    /// The first-person weapon, drawn after the scene with depth cleared.
    pub view_model: Option<WeaponViewModel>,
    /// The other weapons a player in the match may switch to, by id.
    pub holster: HashMap<String, WeaponViewModel>,
}

/// SHAG's image processing: PostProcessingManager's default settings
/// (`colorGrading` and `vignette`), on DefaultRenderingPipeline's
/// `imageProcessing` with Babylon's other defaults.
#[must_use]
pub fn shag_image_processing() -> ImageProcessingConfiguration {
    ImageProcessingConfiguration {
        exposure: 1.0,
        contrast: 1.05,
        vignette: Some(Vignette {
            weight: 0.3,
            stretch: 0.5,
            color: [0.0; 3],
            blend_mode: VignetteBlendMode::Multiply,
            ..Vignette::default()
        }),
    }
}

/// The weapons a player can hold (`WeaponsTable`'s ids).
const HELD_WEAPONS: [&str; 8] = [
    "blade",
    "machinegun",
    "riotgun",
    "glauncher",
    "rlauncher",
    "plasmagun",
    "voltaic",
    "electrobolt",
];

impl MapRenderer {
    /// Puts `id` in hand from the holster (the view model's weapon switch),
    /// or nothing when dead. The weapon in hand keeps its sway when it
    /// stays.
    pub fn hold(&mut self, id: Option<&str>) {
        let id = id.map(|id| if id == "lasergun" { "voltaic" } else { id });
        let current = self.view_model.as_ref().map(|v| v.view.weapon.clone());
        if current.as_deref() == id {
            return;
        }
        if let Some(old) = self.view_model.take() {
            self.holster.insert(old.view.weapon.clone(), old);
        }
        self.view_model = id.and_then(|id| self.holster.remove(id));
    }

    /// Writes this frame's camera and culls for it.
    /// Poses and places the players a demo or a match shows this frame,
    /// advancing their clips by `dt`; players missing from `players` are
    /// forgotten.
    pub fn set_live_players(&mut self, players: &[(String, InterpolatedEntity, PlayerModelVisualStyle)], dt: f64) {
        self.joints.truncate(self.static_joints);
        self.live_draws.clear();
        let Some(renderer) = &mut self.remote_players else {
            return;
        };
        renderer.retain(|id| players.iter().any(|(p, _, _)| p == id));
        for (id, interp, style) in players {
            renderer.update(id, interp, dt, 1.0, None);
            renderer.draws(id, interp, style, &mut self.joints, &mut self.live_draws);
        }
    }

    /// Draws the projectiles in flight this frame, facing along their
    /// velocity, each carrying its pool light; call after
    /// [`Self::set_live_players`], which clears the frame's live draws.
    pub fn set_live_projectiles(&mut self, projectiles: &[LiveProjectile], dt: f64) {
        self.live_light_clock += dt;
        let gone: Vec<String> = self
            .live_projectile_lights
            .keys()
            .filter(|id| !projectiles.iter().any(|p| &p.id == *id))
            .cloned()
            .collect();
        for id in gone {
            if let Some(handle) = self.live_projectile_lights.remove(&id) {
                self.lights.release(handle);
            }
        }
        let ended: Vec<String> = self
            .live_trails
            .keys()
            .filter(|id| !projectiles.iter().any(|p| &p.id == *id))
            .cloned()
            .collect();
        for id in ended {
            if let Some((def, mut trail)) = self.live_trails.remove(&id) {
                trail.reset();
                self.spare_trails.push((def.id, trail));
            }
        }
        for p in projectiles {
            let Some((def, model)) = self.live_projectile_models.iter().find(|(d, _)| d.id == p.def_id) else {
                continue;
            };
            let position = DVec3::from(p.position).as_vec3();
            let velocity = DVec3::from(p.velocity).as_vec3();
            let direction = if velocity.length_squared() > 1e-6 {
                velocity
            } else {
                Vec3::X
            };
            let scale = def.scale.unwrap_or(1.0) as f32;
            model.draws(
                projectile_matrix(position, direction, scale),
                self.item_time,
                &mut self.live_draws,
            );
            if def.trail_effect.is_some() {
                let def: &'static ProjectileDef = def;
                let spare = &mut self.spare_trails;
                let (_, trail) = self.live_trails.entry(p.id.clone()).or_insert_with(|| {
                    let reused = spare
                        .iter()
                        .position(|(id, _)| *id == def.id)
                        .map(|i| spare.swap_remove(i).1);
                    (def, reused.unwrap_or_default())
                });
                trail.push(
                    DVec3::from(p.position),
                    DVec3::from(p.velocity).normalize_or_zero(),
                    def.trail_length,
                );
            }
            if let Some(intensity) = def.light_intensity.filter(|i| *i > 0.0) {
                match self.live_projectile_lights.get(&p.id) {
                    Some(&handle) => self.lights.set_position(handle, position),
                    None => {
                        let color = def
                            .light_color
                            .map_or(Vec3::ONE, |c| Vec3::new(c.r as f32, c.g as f32, c.b as f32));
                        let handle = self.lights.acquire(
                            self.live_light_clock,
                            position,
                            color,
                            intensity as f32,
                            def.light_range.unwrap_or(20.0) as f32,
                        );
                        self.live_projectile_lights.insert(p.id.clone(), handle);
                    }
                }
            }
        }
    }

    /// Advances the effects' clock by `dt` and starts the effects of the
    /// events that arrived (`GameEvents.ts`): a projectile's impact explodes
    /// with its flash; other players' shots draw their streaks, pellets,
    /// beams and rails.
    /// `getGroundSurfaceFlags`: the surface flags of the ground up to 48
    /// units under `at`, 0 when nothing is there.
    pub fn ground_surface_flags(&mut self, at: DVec3) -> i32 {
        use shagger::babylon::maths::math_vector::Vector3;
        use shagger::physics::collision_backend::TraceOptions;
        use shagger::physics::content_flags::masks;
        let Some(world) = &mut self.shot_world else { return 0 };
        let trace = world.trace(
            Vector3::new(at.x, at.y, at.z),
            Vector3::new(at.x, at.y - 48.0, at.z),
            TraceOptions {
                radius: Some(1.0),
                height: Some(0.0),
                content_mask: Some(masks::PLAYERSOLID),
            },
        );
        if trace.fraction < 1.0 {
            trace.hit_surface_flags
        } else {
            0
        }
    }

    pub fn play_effects(&mut self, events: &[SnapshotEvent], weapons: &HashMap<String, String>, dt: f64) {
        self.effect_time += dt;
        let now = self.effect_time;
        self.explosions.update(now);
        for event in events {
            if let SnapshotEvent::ProjectileImpact { def_id, at, normal } = event {
                // Older demos name the weapon; anything else is a rocket.
                let projectile = match def_id.as_str() {
                    "glauncher" => "grenade",
                    "plasmagun" => "plasma_bolt",
                    "rlauncher" => "rocket",
                    other => other,
                };
                let look =
                    explosion_for(projectile).unwrap_or_else(|| explosion_for("rocket").expect("the rocket explodes"));
                let position = DVec3::new(at.x, at.y, at.z);
                let normal = DVec3::new(normal.x, normal.y, normal.z);
                let mut random = mulberry32(self.live_flashes.len() as u32 + now.to_bits() as u32);
                self.explosions.spawn(now, position, look, normal, &mut random);
                if let Some((intensity, range)) = explosion_flash(look, 0.0) {
                    let light = self.lights.acquire(
                        self.live_light_clock,
                        position.as_vec3(),
                        look.outer_color,
                        intensity,
                        range,
                    );
                    self.live_flashes.push((light, look, now));
                }
            }
        }
        let mut no_world = ();
        let world: &mut dyn ShotWorld = match &mut self.shot_world {
            Some(world) => world,
            None => &mut no_world,
        };
        play_remote_shots(
            events,
            &mut EffectTargets {
                now,
                hitscan: &mut self.hitscan,
                world,
                lights: &mut self.lights,
            },
            &|id| weapons.get(id).cloned(),
        );
        self.hitscan.update(now, &mut self.lights);
        // Fade the flashes, and give back the ones that are out.
        let lights = &mut self.lights;
        self.live_flashes
            .retain(|&(light, look, born)| match explosion_flash(look, now - born) {
                Some((intensity, range)) => {
                    lights.set_intensity_range(light, intensity, range);
                    true
                }
                None => {
                    lights.release(light);
                    false
                }
            });
    }

    pub fn prepare(&mut self, gpu: &Gpu, camera: &Camera, size: [f32; 2]) {
        let aspect = size[0] / size[1];
        self.culling.update(camera, aspect);
        let lights: Vec<DynamicLight> = if self.dynamic_lights {
            self.lights.lights().collect()
        } else {
            Vec::new()
        };
        self.models.set_dynamic_lights(&lights);
        self.world.set_dynamic_lights(&lights);
        self.world.prepare(gpu, camera.view_projection(aspect), camera.position);
        self.frame_draws.clone_from(&self.model_draws);
        self.frame_draws.extend_from_slice(&self.live_draws);
        self.items.draws(
            self.item_time,
            camera_rotation(camera.view().as_dmat4()),
            &mut self.frame_draws,
        );
        for (model, world) in &self.projectiles {
            model.draws(*world, self.item_time, &mut self.frame_draws);
        }
        for (trail, color) in &mut self.trails {
            trail.update(gpu, &mut self.models, Some(camera.position.as_dvec3()), *color);
            trail.draws(&mut self.frame_draws);
        }
        for (def, trail) in self.live_trails.values_mut() {
            let color = def.light_color.map(|c| Vec3::new(c.r as f32, c.g as f32, c.b as f32));
            trail.update(gpu, &mut self.models, Some(camera.position.as_dvec3()), color);
            trail.draws(&mut self.frame_draws);
        }
        self.models.prepare(
            gpu,
            camera.view_projection(aspect),
            camera.position,
            &self.frame_draws,
            &self.joints,
        );
        self.explosions
            .prepare(gpu, camera.view_projection(aspect), camera.view(), self.effect_time);
        let frame = EffectFrame {
            view_proj: camera.view_projection(aspect),
            view: camera.view(),
            projection: camera.projection(aspect),
            camera_position: camera.position,
            viewport: size,
            time: self.effect_time,
        };
        self.hitscan.streaks.prepare(gpu, &frame);
        self.hitscan.muzzle_flashes.prepare(gpu, &frame);
        self.hitscan.impact_sparks.prepare(gpu, &frame);
        self.hitscan.rails.prepare(gpu, &frame);
        self.hitscan.voltaic.prepare(gpu, &frame);
        if let Some(sky) = &self.sky {
            sky.prepare(gpu, camera, aspect);
        }
        if let Some(view_model) = &mut self.view_model {
            view_model.prepare(gpu, camera, aspect);
        }
    }

    /// Records the first-person weapon, in a pass after [`Self::draw`]'s
    /// with the depth cleared.
    pub fn draw_overlay(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(view_model) = &self.view_model {
            view_model.draw(pass);
        }
    }

    /// Records the sky, the opaque world and models, then the blended ones.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(sky) = &self.sky {
            sky.draw(pass);
        }
        let visible = if self.cull { self.culling.visible() } else { &[] };
        self.world.draw_visible_opaque(pass, visible);
        self.models.draw_opaque(pass);
        self.world.draw_visible_blended(pass, visible);
        // The effects' alpha indices (40 to 46) put them before the models'.
        self.hitscan.voltaic.draw_beams(pass);
        self.hitscan.rails.draw_beams(pass);
        self.hitscan.voltaic.draw_impacts(pass);
        self.hitscan.rails.draw_impacts(pass);
        self.hitscan.streaks.draw(pass);
        self.hitscan.muzzle_flashes.draw(pass);
        self.hitscan.impact_sparks.draw(pass);
        self.explosions.draw(pass);
        self.models.draw_blended(pass);
    }
}

/// Builds the world and its sky and uploads them with their textures. The
/// scene draws into an HDR target; image processing writes `output_format`.
pub fn build_renderer(gpu: &Gpu, map: &mut LoadedMap, output_format: wgpu::TextureFormat) -> MapRenderer {
    let format = HDR_FORMAT;
    let geometry = build_world_geometry(&map.bsp, |name, options| {
        map.shaders.resolve_material_passes(&map.pk3, name, options)
    });
    let pk3 = &mut map.pk3;
    let renderer = WorldRenderer::new(
        gpu,
        &map.bsp,
        &geometry,
        |path| {
            resolve_and_load_texture(pk3, path).map(|t| Rgba8Image {
                width: t.width,
                height: t.height,
                pixels: t.pixels,
            })
        },
        format,
    );
    let triangles: u32 = geometry.batches.iter().map(|b| b.index_count / 3).sum();
    println!(
        "world: {} vertices, {triangles} triangles in {} batches, {} material passes ({} animated), {} images, {} shaders, {} sky faces, {} autosprite faces",
        geometry.vertices.len(),
        geometry.batches.len(),
        renderer.pass_count(),
        renderer.animated_pass_count(),
        renderer.texture_count(),
        map.shaders.shader_count(),
        geometry.sky_faces.len(),
        geometry.sprite_faces
    );
    if !renderer.missing_images().is_empty() {
        println!("  no image found: {}", renderer.missing_images().join(", "));
    }
    let sky = match apply_skybox(&mut map.pk3, &geometry.sky_shaders) {
        Some((faces, base)) => {
            println!("sky: {base} ({}x{} faces)", faces.size, faces.size);
            Some(SkyRenderer::new(gpu, &faces, format))
        }
        None if geometry.sky_shaders.is_empty() => None,
        None => {
            println!("sky: no far box for {} sky shader(s)", geometry.sky_shaders.len());
            None
        }
    };
    let culling = WorldCulling::new(&map.bsp, &geometry);
    let mut models = ModelRenderer::new(gpu, format);
    // SHAG's lights for a BSP map, with r_bsp_pbr at its default.
    models.set_lights(bsp_mode_lights(true));
    let pickups = build_weapon_pickups(gpu, &mut map.pk3, &mut models, &map.pickups);
    for failure in &pickups.failures {
        eprintln!("pickup model: {failure}");
    }
    let mut model_draws = pickups.draws;
    let items = build_procedural_pickups(gpu, &mut map.pk3, &mut models, &map.pickups);
    println!("items: {} procedural pickups", items.len());
    if !items.missing_icons.is_empty() {
        println!("  no ammo icon: {}", items.missing_icons.join(", "));
    }
    let mut joints = Vec::new();
    let mut remote_players = None;
    if !map.players.is_empty() || map.remote_players {
        let pk3 = &mut map.pk3;
        match RemotePlayerRenderer::load(gpu, &mut models, |path| pk3.get_file(path)) {
            Ok(mut players) => {
                let style = get_player_model_visual_style(PlayerTeamSlot::Ffa, MODEL_SCALE);
                for (i, player) in map.players.iter().enumerate() {
                    let id = format!("player{i}");
                    let interp = player.interpolated();
                    players.update(&id, &interp, 0.0, 1.0, Some(&player.clip));
                    players.go_to_frame(&id, player.frame);
                    players.draws(&id, &interp, &style, &mut joints, &mut model_draws);
                }
                remote_players = Some(players);
            }
            Err(why) => eprintln!("player model: {why}"),
        }
    }
    let mut projectiles = Vec::new();
    let mut trails = Vec::new();
    let mut lights = PointLightPool::new();
    // A demo or a match: every projectile type, ready to fly.
    let mut live_projectile_models = Vec::new();
    if map.remote_players {
        for def in PROJECTILES {
            let Some(path) = def.model_path else { continue };
            let light = def.light_color.map(|c| Vec3::new(c.r as f32, c.g as f32, c.b as f32));
            match load_projectile_model(gpu, &mut models, &mut map.pk3, &map.shaders, def.id, path, light) {
                Ok(model) => live_projectile_models.push((def, model)),
                Err(why) => eprintln!("projectile model: {why}"),
            }
        }
    }
    let mut acquired = 0.0;
    for view in &map.projectiles {
        let Some(def) = projectile_def(&view.id) else {
            eprintln!("projectile: no definition {}", view.id);
            continue;
        };
        let Some(path) = def.model_path else { continue };
        let light = def.light_color.map(|c| Vec3::new(c.r as f32, c.g as f32, c.b as f32));
        match load_projectile_model(gpu, &mut models, &mut map.pk3, &map.shaders, def.id, path, light) {
            Ok(model) => {
                if !model.missing_textures.is_empty() {
                    println!("  no projectile texture: {}", model.missing_textures.join(", "));
                }
                let (position, direction) = view.babylon();
                let scale = def.scale.unwrap_or(1.0) as f32;
                // The projectile carries its pool light at its origin.
                if let Some(intensity) = def.light_intensity.filter(|i| *i > 0.0) {
                    acquired += 1.0;
                    lights.acquire(
                        acquired,
                        position,
                        light.unwrap_or(Vec3::ONE),
                        intensity as f32,
                        def.light_range.unwrap_or(20.0) as f32,
                    );
                }
                projectiles.push((model, projectile_matrix(position, direction, scale)));
                if def.trail_effect.is_some() {
                    trails.push((held_trail(position, direction, def.speed, def.trail_length), light));
                }
            }
            Err(why) => eprintln!("projectile model: {why}"),
        }
    }
    let mut view_model = map.weapon.as_deref().and_then(|id| {
        WeaponViewModel::load(gpu, &mut map.pk3, id, format)
            .inspect(|_| println!("view model: {id}"))
            .inspect_err(|why| eprintln!("view model: {why}"))
            .ok()
    });
    let mut holster = HashMap::new();
    if map.hands {
        for id in HELD_WEAPONS {
            match WeaponViewModel::load(gpu, &mut map.pk3, id, format) {
                Ok(model) => {
                    holster.insert(id.to_owned(), model);
                }
                Err(why) => eprintln!("view model: {why}"),
            }
        }
        println!("view models: {} weapons to hold", holster.len());
    }
    // Pipeline precompile at load (21.10): every pipeline the loaded models
    // can take, so the first frame that shows one does not stall.
    let started = std::time::Instant::now();
    let mut compiled = models.precompile(gpu);
    for view_model in view_model.iter_mut().chain(holster.values_mut()) {
        compiled += view_model.precompile(gpu);
    }
    println!("pipelines: {compiled} model pipelines in {:.0?}", started.elapsed());
    let mut explosions = ExplosionBatchRenderer::new(gpu, format);
    for view in &map.explosions {
        let Some(look) = explosion_for(&view.id) else {
            eprintln!("explosion: {} does not explode", view.id);
            continue;
        };
        let [x, y, z] = view.origin;
        let position = DVec3::new(f64::from(x), f64::from(z), f64::from(y));
        explosions.spawn(-view.age, position, look, DVec3::Y, &mut mulberry32(1));
        // Its flash light, faded to its age.
        if let Some((intensity, range)) = explosion_flash(look, view.age) {
            acquired += 1.0;
            lights.acquire(acquired, position.as_vec3(), look.outer_color, intensity, range);
        }
    }
    let mut hitscan = HitscanEffects::new(gpu, format);
    for effect in &map.effects {
        match effect {
            EffectView::Tracer { start, end, color, age } => {
                hitscan.streaks.fire(
                    -age,
                    &StreakTracerShot {
                        start: babylon(*start),
                        end: babylon(*end),
                        color: Vec3::from(*color),
                        intensity: None,
                    },
                );
            }
            EffectView::Muzzle {
                position,
                color,
                style,
                scale,
                duration,
                age,
            } => hitscan.muzzle_flashes.spawn(
                -age,
                &MuzzleFlashSpawn {
                    position: babylon(*position),
                    color: Vec3::from(*color),
                    style: *style,
                    scale: Some(*scale),
                    intensity: None,
                    duration: Some(*duration),
                },
            ),
            EffectView::Impact {
                position,
                normal,
                incoming,
                color,
                heavy,
                age,
            } => hitscan.impact_sparks.burst(
                -age,
                &ImpactSparkBurst {
                    position: babylon(*position),
                    normal: babylon(*normal),
                    incoming: babylon(*incoming),
                    color: Vec3::from(*color),
                    count: None,
                    heavy: *heavy,
                    speed: None,
                    intensity: None,
                },
            ),
            EffectView::Lightning {
                mode,
                start,
                end,
                color,
                kind,
                age,
            } => {
                let sample = RailEffectSample {
                    start: babylon(*start),
                    end: babylon(*end),
                    color: Vec3::from(*color),
                    impact_kind: *kind,
                    impact_normal: None,
                    intensity: (*mode == LightningMode::Chain).then_some(1.12),
                };
                match mode {
                    // The held beam has no age; it is simply on.
                    LightningMode::Hero => hitscan.voltaic.update_hero(&sample),
                    LightningMode::Remote => {
                        let key = format!("remote{}", hitscan.voltaic.debug_counts().0);
                        hitscan
                            .voltaic
                            .update_remote(-age, &key, &sample, sample.end - sample.start);
                    }
                    // HitscanEffects.fireLightningArc's default duration.
                    LightningMode::Chain => hitscan.voltaic.fire_chain_arc(-age, &sample, 0.14),
                }
            }
            EffectView::Rail {
                start,
                end,
                color,
                kind,
                normal,
                age,
            } => hitscan.rails.fire_rail(
                -age,
                &RailEffectSample {
                    start: babylon(*start),
                    end: babylon(*end),
                    color: Vec3::from(*color),
                    impact_kind: *kind,
                    impact_normal: normal.map(babylon),
                    intensity: None,
                },
                None,
            ),
        }
    }
    MapRenderer {
        projectiles,
        trails,
        explosions,
        hitscan,
        shot_world: map.shot_world.take(),
        world: renderer,
        models,
        model_draws,
        items,
        item_time: 0.0,
        frame_draws: Vec::new(),
        static_joints: joints.len(),
        joints,
        remote_players,
        live_draws: Vec::new(),
        live_projectile_models,
        live_projectile_lights: HashMap::new(),
        live_trails: HashMap::new(),
        spare_trails: Vec::new(),
        live_light_clock: 0.0,
        effect_time: 0.0,
        live_flashes: Vec::new(),
        sky,
        culling,
        cull: true,
        lights,
        dynamic_lights: true,
        post: ImageProcessingPostProcess::new(gpu, output_format),
        image_processing: shag_image_processing(),
        view_model,
        holster,
    }
}

/// The players a demo or a match shows, styled as a spectator sees them:
/// no perspective, so every player takes the FFA slot (wash-fps's per-player
/// FFA colours are not ported yet).
#[must_use]
pub fn styled_players(
    players: Vec<(String, InterpolatedEntity)>,
) -> Vec<(String, InterpolatedEntity, PlayerModelVisualStyle)> {
    let style = get_player_model_visual_style(PlayerTeamSlot::Ffa, MODEL_SCALE);
    players.into_iter().map(|(id, p)| (id, p, style)).collect()
}

pub fn run(o: &ViewOptions) -> i32 {
    let mut feed = if let Some(path) = &o.demo {
        match DemoPlayback::load(path) {
            Ok(d) => {
                println!(
                    "demo: {} on {}, {} frames, {:.0} s, {} players",
                    d.header.gametype,
                    d.header.map,
                    d.frame_count(),
                    d.duration(),
                    d.header.players.len()
                );
                Some(Feed::Demo(d))
            }
            Err(why) => {
                eprintln!("{why}");
                return 1;
            }
        }
    } else if let Some((url, name, ticket)) = &o.watch
        && o.join
    {
        println!("connecting to {url} as {name}, to play");
        match LivePlay::connect(url, name, ticket.clone(), &o.data, std::time::Duration::from_secs(15)) {
            Ok(p) => Some(Feed::Play(Box::new(p), PlayInput::default())),
            Err(why) => {
                eprintln!("{why}");
                return 1;
            }
        }
    } else if let Some((url, name, ticket)) = &o.watch {
        println!("connecting to {url} as {name}");
        match LiveWatch::connect(url, name, ticket.clone(), std::time::Duration::from_secs(15)) {
            Ok(l) => Some(Feed::Live(l)),
            Err(why) => {
                eprintln!("{why}");
                return 1;
            }
        }
    } else {
        None
    };
    let map_name = match &feed {
        Some(Feed::Demo(d)) => d.header.map.clone(),
        Some(Feed::Live(l)) => l.map.clone(),
        Some(Feed::Play(p, _)) => p.map.clone(),
        None => o.map.clone(),
    };
    let mut map = match load_map(&o.data, &map_name) {
        Ok(m) => m,
        Err(why) => {
            eprintln!("{why}");
            return 1;
        }
    };
    map.players.clone_from(&o.players);
    map.projectiles.clone_from(&o.projectiles);
    map.explosions.clone_from(&o.explosions);
    map.effects.clone_from(&o.effects);
    map.weapon.clone_from(&o.weapon);
    map.remote_players = feed.is_some();
    map.hands = o.join;
    let at = o.at.map(|[x, y, z, angle]| Camera::from_q3([x, y, z], angle));
    let Some(mut camera) = at.or_else(|| map.spawns.get(o.spawn).or(map.spawns.first()).copied()) else {
        eprintln!("{} has no spawn point", map_name);
        return 1;
    };
    camera.pitch = o.pitch.to_radians();
    // `cl_fov` (90 at 16:9 unless set), as the browser's camera starts.
    camera.fov_y = shagger::core::cvar::with_cvars(|c| shagger::camera::fov_settings::current_base_fov(c)) as f32;
    println!(
        "{}: {} spawn points, viewing from spawn {}",
        map_name,
        map.spawns.len(),
        o.spawn.min(map.spawns.len() - 1)
    );
    let Some(path) = &o.screenshot else {
        let audio = feed.as_ref().and_then(|_| match AudioV2Sfx::open_device() {
            Ok(sfx) => {
                let mut audio = GameAudio::new(sfx);
                println!(
                    "audio: {} sounds, {} music tracks at {} Hz",
                    audio.preload(&mut map.pk3),
                    audio.load_music(&map.pk3, &o.data),
                    audio.sfx.sample_rate()
                );
                Some(audio)
            }
            Err(why) => {
                println!("no audio: {why}");
                None
            }
        });
        return crate::window::run(map, camera, o.seconds, feed, o.follow, audio);
    };
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(why) => {
            eprintln!("{why}");
            return 1;
        }
    };
    println!("gpu: {}", gpu.describe());
    let mut renderer = build_renderer(&gpu, &mut map, OFFSCREEN_FORMAT);
    renderer.cull = !o.no_cull;
    renderer.dynamic_lights = !o.no_lights;
    // Shader animation as it stands `--time` seconds after the map loads.
    renderer.world.animate(&gpu, o.time);
    renderer.item_time = o.time;
    let mut hud: Option<FpsHud> = None;
    if let Some(feed) = &mut feed {
        // Play up to `--time` at 60 frames a second (live: in real time).
        let steps = (o.time * 60.0).round() as usize;
        let mut players = Vec::new();
        let mut projectiles = Vec::new();
        let live = matches!(feed, Feed::Live(_) | Feed::Play(..));
        // Offline sound at 48 kHz, 800 frames a step.
        let mut audio = o.audio_out.as_ref().map(|_| {
            let mut audio = GameAudio::new(AudioV2Sfx::offline(AUDIO_OUT_RATE));
            audio.music.set_shuffle_seed(1);
            println!(
                "audio: {} sounds, {} music tracks",
                audio.preload(&mut map.pk3),
                audio.load_music(&map.pk3, &o.data)
            );
            audio
        });
        let mut pcm: Vec<f32> = Vec::new();
        hud = match FpsHud::new(&gpu, OFFSCREEN_FORMAT) {
            Ok(hud) => Some(hud),
            Err(why) => {
                eprintln!("no HUD: {why}");
                None
            }
        };
        let mut said = o.say.as_deref();
        for step in 0..steps.max(1) {
            if live {
                std::thread::sleep(std::time::Duration::from_micros(16_667));
            }
            if let Feed::Play(_, input) = feed {
                // Headless play: run forward looking where the camera looks,
                // firing through the first half (which also asks to respawn).
                *input = PlayInput {
                    forward: true,
                    fire: step < steps / 2,
                    yaw: f64::from(camera.yaw),
                    pitch: f64::from(camera.pitch),
                    ..PlayInput::default()
                };
            }
            let view = feed.advance(1.0 / 60.0);
            if let Feed::Play(p, _) = feed {
                if let Some(yaw) = p.take_spawn_facing() {
                    camera.yaw = yaw as f32;
                    camera.pitch = 0.0;
                }
                if let Some(eye) = p.eye() {
                    camera.position = Vec3::new(eye.x as f32, eye.y as f32, eye.z as f32);
                }
                renderer.hold(p.weapon_in_hand());
                if p.alive
                    && let Some(text) = said.take()
                {
                    p.say(text, false);
                }
            }
            players = styled_players(view.players);
            projectiles = view.projectiles;
            renderer.set_live_players(&players, 1.0 / 60.0);
            renderer.set_live_projectiles(&projectiles, 1.0 / 60.0);
            renderer.play_effects(&view.effects, &view.weapons, 1.0 / 60.0);
            if let Some(hud) = &mut hud {
                hud.scoreboard.held = o.scoreboard;
                update_watch_hud(
                    hud,
                    feed,
                    &players,
                    o.follow,
                    f64::from(camera.yaw),
                    &view.effects,
                    1.0 / 60.0,
                    o.width as f32,
                    o.height as f32,
                );
            }
            if let Some(audio) = &mut audio {
                let mut ear = camera;
                if let Some((_, p, _)) = o.follow.and_then(|i| players.get(i)) {
                    chase_camera(&mut ear, p);
                }
                play_view_sounds(
                    audio,
                    &mut renderer,
                    &ear,
                    &DemoViewSounds {
                        effects: &view.effects,
                        teams: &view.teams,
                        snapshots: &view.snapshots,
                        match_state: feed.match_state(),
                        local: feed.local_entity(),
                    },
                    &players,
                    o.follow,
                    1.0 / 60.0,
                );
                let start = pcm.len();
                pcm.resize(start + 2 * AUDIO_OUT_RATE as usize / 60, 0.0);
                audio.sfx.render(&mut pcm[start..]);
            }
            for event in &view.effects {
                match event {
                    SnapshotEvent::ProjectileImpact { def_id, at, .. } => println!(
                        "  {:.2} s: {def_id} hits at {:.0},{:.0},{:.0}",
                        renderer.effect_time, at.x, at.z, at.y
                    ),
                    SnapshotEvent::WeaponFire {
                        entity_id,
                        weapon_id,
                        origin: at,
                        ..
                    } => println!(
                        "  {:.2} s: {entity_id} fires {weapon_id} from {:.0},{:.0},{:.0}",
                        renderer.effect_time, at.x, at.z, at.y
                    ),
                    _ => {}
                }
            }
        }
        let (streaks, sparks, flashes) = renderer.hitscan.debug_counts();
        println!(
            "at {:.2} s: {} players, {} projectiles, {streaks} streaks, {sparks} sparks, {flashes} impact lights, {} Voltaic beams",
            o.time,
            players.len(),
            projectiles.len(),
            renderer.hitscan.voltaic.debug_counts().0
        );
        let m = feed.match_state();
        println!(
            "  match: {} {} ({}), overtime {}",
            m.get_phase_name(),
            m.get_time_string(false),
            m.gametype_name(),
            m.overtime()
        );
        if let Feed::Play(p, _) = feed {
            let at = p.eye().unwrap_or_default();
            println!(
                "  you: {} {}, {}, eye at {:.0},{:.0},{:.0}",
                p.entity_id,
                if p.alive { "alive" } else { "dead" },
                p.weapon,
                at.x,
                at.z,
                at.y
            );
        }
        if let Some(why) = feed.ended() {
            println!("  ({why})");
        }
        if let Some(out) = &o.audio_out {
            let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            match write_wav(out, AUDIO_OUT_RATE, &pcm) {
                Ok(()) => println!(
                    "wrote {} ({:.2} s of sound, peak {peak:.2})",
                    out.display(),
                    pcm.len() as f64 / 2.0 / f64::from(AUDIO_OUT_RATE)
                ),
                Err(why) => eprintln!("{}: {why}", out.display()),
            }
        }
        if let Feed::Live(l) = feed {
            println!("  {} snapshots", l.snapshots);
        }
        if let Some(i) = o.follow {
            match players.get(i) {
                Some((id, p, _)) => {
                    chase_camera(&mut camera, p);
                    println!("  following {id}");
                }
                None => println!("  no player {i} to follow"),
            }
        }
        for p in &projectiles {
            let [x, y, z] = p.position;
            println!("  {} ({}) at {x:.0},{z:.0},{y:.0}", p.id, p.def_id);
        }
        for (id, p, _) in &players {
            // Quake axes, as --at takes them.
            println!("  {id} at {:.0},{:.0},{:.0} yaw {:.2}", p.x, p.z, p.y, p.yaw);
        }
    }
    if let Some(view_model) = &mut renderer.view_model {
        // Settle the aim (and the bob, standing still) over two seconds.
        for _ in 0..120 {
            view_model.view.update(&ViewModelInput {
                delta_time: 1.0 / 60.0,
                grounded: true,
                aiming: o.aim,
                ..ViewModelInput::default()
            });
        }
    }
    renderer.prepare(&gpu, &camera, [o.width as f32, o.height as f32]);
    match renderer.culling.pvs_stats() {
        Some(pvs) => println!(
            "culling: cluster {} sees {} of {} clusters; {} batches left out by the PVS, {} by the frustum",
            pvs.current_cluster,
            pvs.visible_clusters,
            pvs.total_clusters,
            renderer.culling.pvs_culled,
            renderer.culling.frustum_culled
        ),
        None => println!(
            "culling: no vis data; {} batches left out by the frustum",
            renderer.culling.frustum_culled
        ),
    }
    if let Some(hud) = &mut hud {
        hud.prepare(&gpu);
    }
    let pixels = render_hdr_with_ui_to_rgba(
        &gpu,
        o.width,
        o.height,
        CLEAR,
        &renderer.post,
        &renderer.image_processing,
        |pass| renderer.draw(pass),
        |pass| renderer.draw_overlay(pass),
        |pass| {
            if let Some(hud) = &hud {
                hud.draw(pass);
            }
        },
    );
    match image::save_buffer(path, &pixels, o.width, o.height, image::ExtendedColorType::Rgba8) {
        Ok(()) => {
            println!("wrote {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            1
        }
    }
}

/// A held projectile's trail as if it had flown straight at its speed for
/// the last second at 60 frames a second, ending where it is held.
fn held_trail(position: Vec3, direction: Vec3, speed: f64, max_length: Option<f64>) -> ProjectileTrail {
    let (head, dir) = (position.as_dvec3(), direction.as_dvec3());
    let step = speed / 60.0;
    let mut trail = ProjectileTrail::default();
    trail.points = (0..MAX_TRAIL_POINTS)
        .map(|k| head - dir * (step * (MAX_TRAIL_POINTS - k) as f64))
        .collect();
    trail.push(head, dir, max_length);
    trail
}
