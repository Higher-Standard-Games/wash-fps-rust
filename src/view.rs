//! `washoi view`: a map drawn by the renderer (RUST.md 21.10),
//! from a spawn point. With `--screenshot` it renders one frame offscreen and
//! writes a PNG; otherwise it opens a window with a free camera. Players
//! given with `--player` stand on the map as the Vanguard model.

use shagger::animation::player_model_manager::PlayerModelInstance;
use shagger::lighting::map_emissive_surfaces::{SurfaceEmission, extract_emissive_surfaces};
use shagger::physics::physics_system::world_triangles;
use shagger::physics::sweep_backend_adapter::to_shared_contents;
use shagger::rendering::water::water_surface_lighting::{water_map_lights, water_surface_lights};
use shagger::shared::js::js_hypot;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use washoi::client::cartoon_hit_effect::{CartoonHitEffect, CartoonHitRenderer};
use washoi::client::damage_numbers::{DamageNumberRenderer, DamageNumbers};
use washoi::client::fps_liquid_audio::play_liquid_crossing;
use washoi::client::gib_system::{GibRenderer, GibSystem};
use washoi::client::quad_damage_visual::{get_quad_damage_pulse, mask_has_quad};
use washoi::client::ragdoll_system::{RagdollSystem, RagdollTrails};
use washoi::client::remote_player_renderer::{HandWeapons, body_matrix, update_powerup_glow};
use washoi::client::teleport_effect::{TeamColor, TeleportEffect, TeleportRenderer, legacy_team_color, play_tele_in};
use washoi::liquid::client::liquid_interaction_system::{LiquidInteractionSystem, WaterRipples};
use washoi::liquid::client::splash_droplet_renderer::SplashDropletRenderer;
use washoi::liquid::client::underwater_view::{UnderwaterFog, UnderwaterView};
use washoi::liquid::liquid_types::LiquidKind;

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
use shagger::rendering::hdr_target::HdrTarget;
use shagger::rendering::model_renderer::{ModelDraw, ModelRenderer};
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_hdr_scene_with_ui_to_rgba};

use shagger::loader::bsp::liquid_classifier::{classify_liquids, liquid_kind_from_surface_parms};
use shagger::loader::bsp::world_fog::parse_world_fog;
use shagger::loader::texture::skybox_loader::load_environment_cubemap;
use shagger::rendering::sky_renderer::{SkyRenderer, apply_skybox, upload_cube};
use shagger::rendering::water::liquid_surface_system::{LiquidSurfaceSystem, WaterSettings};
use shagger::rendering::wgpu;
use shagger::rendering::world_culling::WorldCulling;
use shagger::rendering::world_fog_controller::{FogUniforms, WorldFogController};
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::entities::projectile_def::ProjectileDef;
use shagger::shared::net::game_state::PlayerState;
use shagger::shared::spread_utils::mulberry32;
use shagger::ui::egui_layer::egui;
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
use washoi::client::cinematic_demo_player::{
    CinematicDemoPlayer, REEL_TITLE_MS, ReelCamera, ReelWorld, look_at_rotation,
};
use washoi::client::demo_playback::{DemoPlayback, DemoView, LiveProjectile};
use washoi::client::fps_hud::{FpsHud, WatchFrame};
use washoi::client::game_events::{EffectTargets, GameAudio, play_remote_shots};
use washoi::client::live_play::{LivePlay, PlayInput};
use washoi::client::live_watch::LiveWatch;
use washoi::client::match_state_manager::MatchStateManager;
use washoi::client::weapon_view::{WeaponViewModel, quad_view_weapon_outline};
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
    pub hits: Vec<HitView>,
    pub teleports: Vec<TeleportView>,
    pub gibs: Vec<GibView>,
    pub ragdolls: Vec<RagdollView>,
    /// The first-person weapon to hold (`--weapon`), and whether it is
    /// aimed down its sights (`--aim`).
    pub weapon: Option<String>,
    pub aim: bool,
    /// The gun wears the quad outline (`--quad`).
    pub quad: bool,
    /// A demo to play on its map (`washoi demo`); `time` seeks it.
    pub demo: Option<PathBuf>,
    /// A live match to spectate (`washoi watch`): server, name, ticket.
    pub watch: Option<(String, String, Option<String>)>,
    /// The player (by place in id order) to put the camera behind.
    pub follow: Option<usize>,
    /// Play the demo's highlight reel of about this many minutes instead.
    pub reel: Option<f64>,
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
/// `reelWorld` (`DemoService.ts`): the players as drawn, and where a line
/// meets solid map (what the server's shots stop on; the browser picks the
/// drawn brushes of the same contents).
pub struct SceneReelWorld<'a> {
    pub shots: Option<&'a mut SweepCollisionBackend>,
    pub players: &'a [(String, InterpolatedEntity, PlayerModelVisualStyle)],
}

impl ReelWorld for SceneReelWorld<'_> {
    fn position_of(&mut self, entity_id: &str) -> Option<shagger::shared::net::game_state::Vec3> {
        self.players
            .iter()
            .find(|(id, _, _)| id == entity_id)
            .map(|(_, p, _)| shagger::shared::net::game_state::Vec3::new(p.x, p.y, p.z))
    }

    fn blocked_at(
        &mut self,
        from: shagger::shared::net::game_state::Vec3,
        to: shagger::shared::net::game_state::Vec3,
    ) -> Option<shagger::shared::net::game_state::Vec3> {
        let world = self.shots.as_deref_mut()?;
        let along = DVec3::new(to.x - from.x, to.y - from.y, to.z - from.z);
        let length = along.length();
        if length < 1.0 {
            return None;
        }
        let hit = world.trace_shot(DVec3::new(from.x, from.y, from.z), along / length, length)?;
        Some(shagger::shared::net::game_state::Vec3::new(
            hit.point.x,
            hit.point.y,
            hit.point.z,
        ))
    }
}

/// Puts `camera` where the reel says: on its fly-by, or behind a player's
/// eyes. Returns the player whose eyes it is, whose body is then left out.
/// `base_fov` is the camera's own, for shots that do not set one.
pub fn apply_reel_camera(
    camera: &mut Camera,
    reel: &CinematicDemoPlayer,
    players: &[(String, InterpolatedEntity, PlayerModelVisualStyle)],
    base_fov: f32,
) -> Option<String> {
    match reel.camera()? {
        ReelCamera::Freefly {
            position,
            look_at,
            fov_degrees,
        } => {
            let (pitch, yaw) = look_at_rotation(*position, *look_at);
            camera.position = Vec3::new(position.x as f32, position.y as f32, position.z as f32);
            camera.pitch = pitch as f32;
            camera.yaw = yaw as f32;
            camera.fov_y = fov_degrees.map_or(base_fov, |f| f.to_radians() as f32);
            None
        }
        ReelCamera::Chase(id) => {
            camera.fov_y = base_fov;
            let (_, p, _) = players.iter().find(|(pid, _, _)| pid == id)?;
            let lift = DEFAULT_VIEWHEIGHT + p.crouch_view_offset.unwrap_or(0.0);
            camera.position = Vec3::new(p.x as f32, (p.y + lift) as f32, p.z as f32);
            camera.yaw = p.yaw as f32;
            camera.pitch = p.pitch.unwrap_or(0.0) as f32;
            Some(id.clone())
        }
    }
}

pub fn chase_camera(camera: &mut Camera, player: &InterpolatedEntity) {
    let yaw = player.yaw as f32;
    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let at = Vec3::new(player.x as f32, player.y as f32, player.z as f32);
    camera.position = at - forward * 150.0 + Vec3::Y * 40.0;
    camera.yaw = yaw;
    camera.pitch = 0.15;
}

/// The player on screen: our own, else the one followed (none for a free
/// camera).
#[must_use]
pub fn view_entity<'a>(
    local: Option<&'a str>,
    players: &'a [(String, InterpolatedEntity, PlayerModelVisualStyle)],
    follow: Option<usize>,
) -> Option<&'a str> {
    local.or_else(|| follow.and_then(|i| players.get(i)).map(|(id, _, _)| id.as_str()))
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
    let view_entity = view_entity(view.local, players, follow);
    let position = |id: &str| {
        players
            .iter()
            .find(|(p, _, _)| p == id)
            .map(|(_, e, _)| DVec3::new(e.x, e.y, e.z))
    };
    audio.play_event_sounds(effects, view_entity, teams, &position, now * 1000.0);
    for sound in renderer.teleports.take_sounds() {
        play_tele_in(&mut audio.sfx, &sound);
    }
    for sound in renderer.liquid.take_sounds() {
        play_liquid_crossing(&mut audio.sfx, &sound);
    }
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

/// `RemotePlayerRenderer.TELEPORT_FX_DIST_SQ`.
const TELEPORT_FX_DIST_SQ: f64 = 256.0 * 256.0;

/// A teleport held at an age (`--teleport`): a ghost and a burst at both
/// ends, the entry's collapsing.
#[derive(Debug, Clone, PartialEq)]
pub struct TeleportView {
    /// Quake origins.
    pub from: [f64; 3],
    pub to: [f64; 3],
    /// Babylon yaw, radians.
    pub yaw: f64,
    /// Seconds since it happened.
    pub age: f64,
}

/// What the death camera follows: a ragdoll or a gib burst, by its handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathFollow {
    Ragdoll(u64),
    Gibs(u64),
}

/// A body fallen as a ragdoll, held at an age (`--ragdoll`).
#[derive(Debug, Clone, PartialEq)]
pub struct RagdollView {
    /// The body, standing idle where it died.
    pub player: PlayerView,
    /// The killing push, in Quake axes.
    pub dir: [f64; 3],
    /// Seconds since it happened.
    pub age: f64,
}

/// A body broken into shards, held at an age (`--gib`).
#[derive(Debug, Clone, PartialEq)]
pub struct GibView {
    /// The death's Quake origin.
    pub origin: [f64; 3],
    /// Seconds since it happened.
    pub age: f64,
}

/// A hit the player on screen dealt, held at an age (`--hit`): its damage
/// number and, for a hard one, its cartoon sprite.
#[derive(Debug, Clone, PartialEq)]
pub struct HitView {
    /// The victim's Quake origin.
    pub origin: [f64; 3],
    pub damage: u16,
    pub head: bool,
    /// Seconds since it landed.
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
    /// Carrying Quad Damage (its glow, half way through the pulse).
    pub quad: bool,
    /// The weapon in hand, if any.
    pub weapon: Option<String>,
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
            powerup_mask: self.quad.then_some(1),
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
    /// Hits to hold on the map.
    pub hits: Vec<HitView>,
    /// Teleports to hold on the map.
    pub teleports: Vec<TeleportView>,
    /// Gibs to hold on the map.
    pub gibs: Vec<GibView>,
    /// Ragdolls to hold on the map.
    pub ragdolls: Vec<RagdollView>,
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
        hits: Vec::new(),
        teleports: Vec::new(),
        gibs: Vec::new(),
        ragdolls: Vec::new(),
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
    /// The map's world fog and the `r_fog` cvars over it (MapLoader's
    /// `WorldFogController.setMap`).
    pub fog: WorldFogController,
    /// The cvars' serial the fog was last resolved at.
    fog_serial: Option<u64>,
    /// The liquid surfaces, drawn with the unified water material
    /// (MapLoader's `claimLiquidSurfaces`); none with `r_water 0`.
    pub water: Option<LiquidSurfaceSystem>,
    /// `item_time` at the water's last frame, for its clock.
    water_time: f64,
    /// Splashes, wakes and rings where bodies and shots cross a surface
    /// (`LocalLiquid`'s `LiquidInteractionSystem`).
    pub liquid: LiquidInteractionSystem,
    /// The liquid interactions' clock, in seconds.
    liquid_clock: f64,
    /// The splash crowns' spray.
    splash: SplashDropletRenderer,
    /// Ghosts and speed-line bursts where players teleport.
    pub teleports: TeleportEffect,
    teleport_renderer: TeleportRenderer,
    /// Each live player's last origin and yaw, for spotting a teleport.
    teleport_last: HashMap<String, (DVec3, f64)>,
    /// Bodies breaking into shards on a gib death.
    pub gibs: GibSystem,
    gib_renderer: GibRenderer,
    /// Each live player's tint, for the colour their shards take.
    live_tints: HashMap<String, Vec3>,
    /// Non-gib deaths fall as ragdolls (`RagdollSystem`); none without the
    /// player model.
    pub ragdolls: Option<RagdollSystem>,
    ragdoll_trails: RagdollTrails,
    /// Guns in players' hands (`updateWeaponModel`), and who holds which.
    hand_weapons: HandWeapons,
    live_weapons: HashMap<String, String>,
    /// Players whose body a ragdoll took, hidden until they leave the list.
    ragdolled: HashSet<String>,
    /// Poses of players who left the list this frame, for a death that
    /// arrives with their leaving.
    departed: HashMap<String, PlayerModelInstance>,
    /// Each live player's body matrix, kept a frame past their leaving.
    live_worlds: HashMap<String, Mat4>,
    /// The local player (whose own body is not drawn) and their view yaw,
    /// for their corpse (`startLocalRagdollDeathCamera`).
    local_player: Option<(String, f64)>,
    /// What the death camera follows, and where and facing what the local
    /// player died; taken by the window to start it.
    local_death: Option<(DeathFollow, DVec3, f64)>,
    /// Numbers floating over the players the one on screen hurts.
    pub damage_numbers: DamageNumbers,
    damage_number_renderer: DamageNumberRenderer,
    /// POW! sprites over hard hits; no renderer without their images.
    pub cartoon_hits: CartoonHitEffect,
    cartoon_hit_renderer: Option<CartoonHitRenderer>,
    /// Tint, fog and view warp while the camera is under a surface.
    pub underwater: UnderwaterView,
    /// The scene fog the underwater view holds this frame.
    underwater_fog: Option<UnderwaterFog>,
    /// The fog last handed to the materials.
    applied_fog: Option<FogUniforms>,
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
    /// What a reel's cameras and the ground probe trace against.
    pub fn shot_world_mut(&mut self) -> Option<&mut SweepCollisionBackend> {
        self.shot_world.as_mut()
    }

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
        // A ragdoll's player stays hidden until they leave the list.
        self.ragdolled.retain(|id| players.iter().any(|(p, _, _)| p == id));
        let ragdolled = &self.ragdolled;
        self.departed = renderer
            .retain(|id| players.iter().any(|(p, _, _)| p == id) && !ragdolled.contains(id))
            .into_iter()
            .collect();
        let departed = &self.departed;
        self.live_worlds
            .retain(|id, _| departed.contains_key(id) || players.iter().any(|(p, _, _)| p == id));
        self.live_tints
            .retain(|id, _| departed.contains_key(id) || players.iter().any(|(p, _, _)| p == id));
        for (id, interp, style) in players {
            if self.ragdolled.contains(id) {
                continue;
            }
            if let Some(tint) = style.tint_color {
                self.live_tints.insert(id.clone(), tint);
            } else {
                self.live_tints.remove(id);
            }
            self.live_worlds.insert(id.clone(), body_matrix(interp));
            renderer.update(id, interp, dt, 1.0, None);
            let first = self.live_draws.len();
            renderer.draws(id, interp, style, &mut self.joints, &mut self.live_draws);
            update_powerup_glow(
                &mut self.live_draws[first..],
                interp.powerup_mask.unwrap_or(0),
                self.item_time,
            );
            if let (Some(weapon), Some(hand)) = (self.live_weapons.get(id), renderer.hand_world(id, interp)) {
                let quad =
                    mask_has_quad(interp.powerup_mask.unwrap_or(0)).then(|| get_quad_damage_pulse(self.item_time));
                self.hand_weapons.draws(weapon, hand, quad, &mut self.live_draws);
            }
        }
        // RemotePlayerRenderer's teleport detection: a jump of more than
        // 256 units between frames. A player gone from the list (dead) is
        // forgotten, so a respawn starts afresh.
        self.teleports.update(dt);
        self.teleport_last
            .retain(|id, _| players.iter().any(|(p, _, _)| p == id));
        for (id, interp, _) in players {
            let at = DVec3::new(interp.x, interp.y, interp.z);
            if let Some((last, last_yaw)) = self.teleport_last.insert(id.clone(), (at, interp.yaw))
                && at.distance_squared(last) > TELEPORT_FX_DIST_SQ
            {
                self.teleports
                    .player_teleported(last, at, last_yaw, interp.yaw, legacy_team_color(None, id));
            }
        }
        self.teleport_renderer.advance(Some(&renderer.model), dt);
        self.teleport_renderer.ghost_draws(
            Some(&renderer.model),
            &self.teleports,
            &mut self.joints,
            &mut self.live_draws,
        );
        // Splashes come from the water level already on the wire
        // (`RemotePlayerRenderer`): replicate causes, derive effects.
        self.liquid_clock += dt;
        let gone: Vec<String> = self
            .liquid
            .body_ids()
            .filter(|id| !players.iter().any(|(p, _, _)| p == id))
            .map(str::to_owned)
            .collect();
        for id in gone {
            self.liquid.forget(&id);
        }
        for (id, interp, _) in players {
            self.notify_liquid(
                id,
                interp.water_level.unwrap_or(0),
                DVec3::new(interp.x, interp.y, interp.z),
                interp.vy,
                false,
            );
        }
    }

    /// A body's water level this frame (`LiquidInteractionSystem.notify`),
    /// against the map's liquids and pools: `local` is our own predicted
    /// body, heard at our ears.
    pub fn notify_liquid(&mut self, id: &str, level: u8, position: DVec3, vertical_speed: f64, local: bool) {
        let shot_world = self.shot_world.as_ref();
        let contents = |p: DVec3| {
            shot_world.map_or(0, |w| {
                to_shared_contents(
                    w.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                )
            })
        };
        let collision: Option<&dyn Fn(DVec3) -> u32> = shot_world.is_some().then_some(&contents);
        self.liquid.notify(
            id,
            level,
            LiquidKind::Water,
            position,
            vertical_speed,
            local,
            self.liquid_clock,
            collision,
            self.water.as_mut().map(|w| w as &mut dyn WaterRipples),
        );
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
        // A shot into a pool splashes (`ServerProjectiles`).
        let gone: Vec<String> = self
            .liquid
            .projectile_ids()
            .filter(|id| !projectiles.iter().any(|p| &p.id == id))
            .map(str::to_owned)
            .collect();
        for id in gone {
            self.liquid.forget_projectile(&id);
        }
        if let Some(world) = self.shot_world.as_ref() {
            let contents = |p: DVec3| {
                to_shared_contents(
                    world.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                )
            };
            for p in projectiles {
                self.liquid.notify_projectile(
                    &p.id,
                    DVec3::from(p.position),
                    js_hypot(&p.velocity),
                    Some(&contents),
                    self.water.as_mut().map(|w| w as &mut dyn WaterRipples),
                );
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

    /// FrameEffects' damage numbers and cartoon hits, then GameEvents'
    /// `handleOutgoingDamage` for each hit the player on screen dealt
    /// (`view_entity`): a number over the victim, and a sprite for a hard one.
    pub fn play_hit_feedback(&mut self, events: &[SnapshotEvent], view_entity: Option<&str>, dt: f64) {
        self.damage_numbers.update(dt);
        self.cartoon_hits.update(dt);
        let Some(viewer) = view_entity else {
            return;
        };
        for event in events {
            if let SnapshotEvent::Damage {
                entity_id,
                attacker_id,
                damage,
                at,
                hit_location,
                ..
            } = event
                && attacker_id == viewer
                && attacker_id != entity_id
            {
                let at = DVec3::new(at.x, at.y, at.z);
                if *damage > 0 {
                    self.damage_numbers.spawn(at, *damage, *hit_location);
                }
                self.cartoon_hits.spawn(at, *damage);
            }
        }
    }

    /// The shards fly on, and GameEvents' `handleDeathEvent` breaks a body
    /// up when the death gibs: a rail, the bomb, or an explosion of 50 or
    /// more. The shards take the colour the body was drawn in.
    pub fn play_gibs(&mut self, events: &[SnapshotEvent], dt: f64) {
        use shagger::physics::collision_backend::TraceOptions;
        use shagger::physics::content_flags::masks;
        {
            let shot_world = self.shot_world.as_mut();
            let mut trace = shot_world.map(|w| {
                move |from: DVec3, to: DVec3| {
                    let v = |p: DVec3| shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z);
                    let hit = w.trace(
                        v(from),
                        v(to),
                        TraceOptions {
                            radius: Some(0.0),
                            height: Some(0.0),
                            content_mask: Some(masks::SHOT),
                        },
                    );
                    if hit.start_solid || hit.fraction >= 1.0 {
                        return None;
                    }
                    let n = hit.normal?;
                    Some((
                        DVec3::new(hit.end_pos.x, hit.end_pos.y, hit.end_pos.z),
                        DVec3::new(n.x, n.y, n.z),
                    ))
                }
            });
            self.gibs.update(
                dt,
                trace
                    .as_mut()
                    .map(|t| t as &mut dyn FnMut(DVec3, DVec3) -> Option<(DVec3, DVec3)>),
            );
        }
        // RagdollSystem.update: a ragdoll that has faded bursts here.
        if let (Some(ragdolls), Some(renderer)) = (&mut self.ragdolls, &self.remote_players) {
            ragdolls.update(&renderer.model, dt);
            for (at, color) in ragdolls.take_gibs() {
                self.gibs.spawn(at, DVec3::ZERO, color);
            }
        }
        for event in events {
            if let SnapshotEvent::Death {
                entity_id,
                weapon_id,
                damage_type,
                final_damage,
                at,
                vel,
                kb_dir,
                ..
            } = event
            {
                let kind = if damage_type.is_empty() { weapon_id } else { damage_type };
                let gib = kind == "rail" || kind == "bomb" || (kind == "explosion" && *final_damage >= 50);
                let color = self.live_tints.get(entity_id).copied();
                let local = self
                    .local_player
                    .as_ref()
                    .filter(|(id, _)| id == entity_id)
                    .map(|(_, yaw)| *yaw);
                let death_pos = DVec3::new(at.x, at.y, at.z);
                if gib {
                    let burst = self.gibs.spawn(death_pos, DVec3::new(vel.x, vel.y, vel.z), color);
                    if let (Some(yaw), Some(burst)) = (local, burst) {
                        self.local_death = Some((DeathFollow::Gibs(burst), death_pos, yaw));
                    }
                    continue;
                }
                // The local player's corpse: a fresh body where they died,
                // facing as they looked (`createDetachedModelAt`).
                if let (Some(yaw), Some(renderer), Some(ragdolls)) =
                    (local, &mut self.remote_players, &mut self.ragdolls)
                {
                    let interp = InterpolatedEntity {
                        x: at.x,
                        y: at.y,
                        z: at.z,
                        yaw,
                        ..InterpolatedEntity::default()
                    };
                    let id = format!("{entity_id}#corpse");
                    renderer.update(&id, &interp, 0.0, 1.0, Some("idle"));
                    if let Some(instance) = renderer.take(&id)
                        && let Some(serial) = ragdolls.spawn(
                            &renderer.model,
                            instance,
                            body_matrix(&interp),
                            DVec3::new(kb_dir.x, kb_dir.y, kb_dir.z),
                            color,
                        )
                    {
                        self.local_death = Some((DeathFollow::Ragdoll(serial), death_pos, yaw));
                    }
                    continue;
                }
                // `detachModel`: the body leaves the renderer for a ragdoll.
                let Some(renderer) = &mut self.remote_players else {
                    continue;
                };
                let Some(instance) = renderer.take(entity_id).or_else(|| self.departed.remove(entity_id)) else {
                    continue;
                };
                self.ragdolled.insert(entity_id.clone());
                let world = self.live_worlds.get(entity_id).copied();
                match (&mut self.ragdolls, world) {
                    (Some(ragdolls), Some(world)) => {
                        ragdolls.spawn(
                            &renderer.model,
                            instance,
                            world,
                            DVec3::new(kb_dir.x, kb_dir.y, kb_dir.z),
                            color,
                        );
                    }
                    // No ragdolls: the body breaks up instead.
                    _ => {
                        self.gibs.spawn(death_pos, DVec3::new(vel.x, vel.y, vel.z), color);
                    }
                }
            }
        }
    }

    /// Who holds which weapon (`enemyWeapons`), before
    /// [`Self::set_live_players`].
    pub fn set_live_weapons(&mut self, weapons: &HashMap<String, String>) {
        self.live_weapons.clone_from(weapons);
    }

    /// Names the local player and their view yaw before [`Self::play_gibs`],
    /// so their death leaves a corpse for the death camera.
    pub fn set_local_player(&mut self, id: Option<&str>, yaw: f64) {
        self.local_player = id.map(|id| (id.to_owned(), yaw));
    }

    /// The local player's death this frame: what to follow, where they died
    /// and their yaw (the death camera's `start`).
    pub fn take_local_death(&mut self) -> Option<(DeathFollow, DVec3, f64)> {
        self.local_death.take()
    }

    /// The follow handle's `getPosition`.
    #[must_use]
    pub fn follow_position(&self, follow: DeathFollow) -> Option<DVec3> {
        match follow {
            DeathFollow::Ragdoll(serial) => self.ragdolls.as_ref()?.position_of(serial),
            DeathFollow::Gibs(burst) => self.gibs.burst_position(burst),
        }
    }

    /// A ray through the map's brushes (shot mask): where it first hits.
    pub fn trace_map(&mut self, from: DVec3, to: DVec3) -> Option<DVec3> {
        use shagger::physics::collision_backend::TraceOptions;
        use shagger::physics::content_flags::masks;
        let world = self.shot_world.as_mut()?;
        let v = |p: DVec3| shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z);
        let hit = world.trace(
            v(from),
            v(to),
            TraceOptions {
                radius: Some(0.0),
                height: Some(0.0),
                content_mask: Some(masks::SHOT),
            },
        );
        (!hit.start_solid && hit.fraction < 1.0).then(|| DVec3::new(hit.end_pos.x, hit.end_pos.y, hit.end_pos.z))
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

    /// The underwater view (`UnderwaterView`, run before the camera renders):
    /// asks what the camera's eye is in, eases the tint and fog, and warps
    /// `camera`'s field of view. Call before [`Self::prepare`].
    pub fn update_underwater(&mut self, camera: &mut Camera, dt: f64) {
        let shot_world = self.shot_world.as_ref();
        let contents = |p: DVec3| {
            shot_world.map_or(0, |w| {
                to_shared_contents(
                    w.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                )
            })
        };
        let comfort = shagger::core::cvar::with_cvars(|c| c.get_number("r_underwater_distortion"));
        self.underwater_fog = self.underwater.update(
            camera,
            dt.min(0.1),
            shot_world.is_some().then_some(&contents as &dyn Fn(DVec3) -> u32),
            self.fog.scene_color(),
            comfort,
        );
    }

    /// The fog every material draws with this frame: the underwater view's
    /// while it holds the scene fog, else world fog.
    fn frame_fog(&self) -> FogUniforms {
        self.underwater_fog.map_or_else(
            || self.fog.uniforms(),
            |f| FogUniforms::scene_exp2(f.color, f.density, self.fog.sky_height()),
        )
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
        let serial = shagger::core::cvar::with_cvars(|c| {
            let serial = c.serial();
            if self.fog_serial != Some(serial) {
                self.fog.refresh(c);
            }
            serial
        });
        self.fog_serial = Some(serial);
        let uniforms = self.frame_fog();
        if self.applied_fog != Some(uniforms) {
            self.applied_fog = Some(uniforms);
            self.world.set_fog(uniforms);
            self.models.set_fog(uniforms);
            self.explosions.set_fog(uniforms);
            if let Some(sky) = &mut self.sky {
                sky.set_fog(uniforms);
            }
        }
        // `Q3SoftParticleDepth`: the scene depth, while `r_softhalos` is on
        // and a soft halo is in view.
        let soft =
            shagger::core::cvar::with_cvars(|c| c.get("r_softhalos").is_none() || c.get_number("r_softhalos") != 0.0)
                && self
                    .world
                    .has_visible_soft_particles(if self.cull { self.culling.visible() } else { &[] });
        self.world.set_soft_particles(
            gpu,
            camera.view(),
            camera.near,
            camera.far,
            (size[0].round() as u32, size[1].round() as u32),
            soft,
        );
        self.world.prepare(gpu, camera.view_projection(aspect), camera.position);
        self.frame_draws.clone_from(&self.model_draws);
        self.frame_draws.extend_from_slice(&self.live_draws);
        self.gib_renderer.draws(&self.gibs, &mut self.frame_draws);
        // The ragdolls' palettes go after the frame's, and leave with it.
        let joints_base = self.joints.len();
        if let (Some(ragdolls), Some(renderer)) = (&self.ragdolls, &self.remote_players) {
            let (lit, fade) = renderer.handles();
            ragdolls.draws(&renderer.model, lit, fade, &mut self.joints, &mut self.frame_draws);
            self.ragdoll_trails
                .draws(gpu, &mut self.models, ragdolls, &mut self.frame_draws);
        }
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
        self.joints.truncate(joints_base);
        self.explosions
            .prepare(gpu, camera.view_projection(aspect), camera.view(), self.effect_time);
        let frame = EffectFrame {
            view_proj: camera.view_projection(aspect),
            view: camera.view(),
            projection: camera.projection(aspect),
            camera_position: camera.position,
            viewport: size,
            time: self.effect_time,
            fog: uniforms,
        };
        self.hitscan.streaks.prepare(gpu, &frame);
        self.hitscan.muzzle_flashes.prepare(gpu, &frame);
        self.hitscan.impact_sparks.prepare(gpu, &frame);
        self.hitscan.rails.prepare(gpu, &frame);
        self.hitscan.voltaic.prepare(gpu, &frame);
        if let Some(sky) = &self.sky {
            sky.prepare(gpu, camera, aspect);
        }
        {
            let shot_world = self.shot_world.as_ref();
            let contents = |p: DVec3| {
                shot_world.map_or(0, |w| {
                    to_shared_contents(
                        w.point_contents(shagger::babylon::maths::math_vector::Vector3::new(p.x, p.y, p.z), None),
                    )
                })
            };
            self.liquid.update(
                self.item_time - self.water_time,
                shot_world.is_some().then_some(&contents as &dyn Fn(DVec3) -> u32),
                self.water.as_mut().map(|w| w as &mut dyn WaterRipples),
            );
        }
        self.splash
            .prepare(gpu, &frame, self.liquid.droplets().map_or(&[], |d| d.sprites()));
        self.teleport_renderer.prepare(gpu, &frame, &self.teleports);
        self.damage_number_renderer.prepare(gpu, &frame, &self.damage_numbers);
        if let Some(cartoon) = &mut self.cartoon_hit_renderer {
            cartoon.prepare(gpu, &frame, &self.cartoon_hits);
        }
        if let Some(water) = &mut self.water {
            water.update(self.item_time - self.water_time);
            self.water_time = self.item_time;
            let settings = shagger::core::cvar::with_cvars(|c| WaterSettings {
                quality: c.get_number("r_water_quality"),
                debug: c.get_number("r_water_debug"),
                highlights: c.get_number("r_water_highlights"),
                roughness: c.get_number("r_water_roughness"),
                reflection: c.get_number("r_water_reflection"),
                blur: c.get_number("r_water_blur"),
                detail: c.get_number("r_water_detail"),
                contact: c.get_number("r_water_contact"),
            });
            let visible = if self.cull { self.culling.visible() } else { &[] };
            water.prepare(
                gpu,
                camera,
                (size[0] as u32, size[1] as u32),
                &settings,
                uniforms,
                visible,
            );
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

    /// Records the sky and the opaque world and models.
    pub fn draw_opaque(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(sky) = &self.sky {
            sky.draw(pass);
        }
        let visible = if self.cull { self.culling.visible() } else { &[] };
        self.world.draw_visible_opaque(pass, visible);
        self.models.draw_opaque(pass);
    }

    /// Copies the opaque scene for the water to look through, between
    /// [`Self::draw_opaque`] and [`Self::draw_transparent`].
    pub fn capture(&self, encoder: &mut wgpu::CommandEncoder, hdr: &HdrTarget) {
        if let Some(water) = &self.water {
            water.capture(encoder, &hdr.color_texture, &hdr.depth_texture);
        }
        self.world.capture_soft_depth(encoder, &hdr.depth_texture);
    }

    /// Records the water, the blended world, the effects and the blended
    /// models, over what [`Self::draw_opaque`] drew.
    pub fn draw_transparent(&self, pass: &mut wgpu::RenderPass<'_>) {
        let visible = if self.cull { self.culling.visible() } else { &[] };
        if let Some(water) = &self.water {
            water.draw(pass, self.world.vertex_buffer(), self.world.index_buffer(), visible);
        }
        self.world.draw_visible_blended(pass, visible);
        // The effects' alpha indices (40 to 46) put them before the models'.
        self.hitscan.voltaic.draw_beams(pass);
        self.hitscan.rails.draw_beams(pass);
        self.hitscan.voltaic.draw_impacts(pass);
        self.hitscan.rails.draw_impacts(pass);
        self.hitscan.streaks.draw(pass);
        self.hitscan.muzzle_flashes.draw(pass);
        self.hitscan.impact_sparks.draw(pass);
        self.splash.draw(pass);
        self.teleport_renderer.draw(pass);
        self.explosions.draw(pass);
        self.models.draw_blended(pass);
        self.damage_number_renderer.draw(pass);
        // Rendering group 1: over everything, so last and without depth.
        if let Some(cartoon) = &self.cartoon_hit_renderer {
            cartoon.draw(pass);
        }
    }
}

/// MapLoader's `claimLiquidSurfaces`: the liquid batches the classifier
/// finds (content flags, then the shader's `surfaceparm`), drawn with the
/// unified water material over the map's sky cube, unless `r_water` is 0.
fn claim_liquid_surfaces(
    gpu: &Gpu,
    map: &mut LoadedMap,
    geometry: &shagger::rendering::world_geometry::WorldGeometry,
    sky: Option<&SkyRenderer>,
    format: wgpu::TextureFormat,
) -> Option<LiquidSurfaceSystem> {
    if shagger::core::cvar::with_cvars(|c| c.get("r_water").is_some() && c.get_number("r_water") == 0.0) {
        println!("water: r_water 0, liquid surfaces left on the legacy renderer");
        return None;
    }
    let liquids = classify_liquids(
        &map.bsp,
        &std::collections::HashMap::new(),
        |name| {
            map.shaders
                .get_shader(name)
                .and_then(|s| liquid_kind_from_surface_parms(s.surface_parms.iter().map(String::as_str)))
        },
        |name| {
            let shader = map.shaders.get_shader(name)?;
            let unconditional = if shader.unconditional_stages.is_empty() {
                &shader.stages
            } else {
                &shader.unconditional_stages
            };
            unconditional
                .iter()
                .chain(shader.conditional_blocks.iter().flatten().flat_map(|b| &b.stages))
                .find_map(|stage| stage.cube_map.clone())
        },
    );
    let probe_lift = shagger::core::cvar::with_cvars(|c| match c.get("r_water_probe_height") {
        Some(_) => c.get_number("r_water_probe_height"),
        None => 64.0,
    });
    // A liquid's own `cubemap`, the first one each kind names.
    let mut kind_environments: Vec<(_, wgpu::TextureView)> = Vec::new();
    for liquid in liquids.iter().filter(|l| l.drawn) {
        let Some(base) = liquid.environment_map.as_deref() else {
            continue;
        };
        if kind_environments.iter().any(|(k, _)| *k == liquid.kind) {
            continue;
        }
        match load_environment_cubemap(&mut map.pk3, base) {
            Ok(faces) => {
                let (kind, size) = (liquid.kind.as_str(), faces.size);
                println!("water: {kind} environment {base} ({size}x{size})");
                kind_environments.push((liquid.kind, upload_cube(gpu, &faces, "water environment")));
            }
            Err(e) => eprintln!(
                "[Water] Could not load {} environment {base}: {e}",
                liquid.kind.as_str()
            ),
        }
    }
    let water = LiquidSurfaceSystem::new(
        gpu,
        geometry,
        &liquids,
        sky.map(SkyRenderer::cube),
        &kind_environments,
        format,
        probe_lift,
    )?;
    let kinds: Vec<String> = liquids
        .iter()
        .filter(|l| l.drawn)
        .map(|l| format!("{} ({})", l.shader_name, l.kind.as_str()))
        .collect();
    println!(
        "water: {} surfaces of {}{}",
        water.claimed_count(),
        kinds.join(", "),
        if water.has_environment() {
            ", reflecting the sky"
        } else {
            ""
        }
    );
    Some(water)
}

/// Builds the world and its sky and uploads them with their textures. The
/// scene draws into an HDR target; image processing writes `output_format`.
pub fn build_renderer(gpu: &Gpu, map: &mut LoadedMap, output_format: wgpu::TextureFormat) -> MapRenderer {
    let format = HDR_FORMAT;
    let geometry = build_world_geometry(&map.bsp, |name, options| {
        map.shaders.resolve_material_passes(&map.pk3, name, options)
    });
    let pk3 = &mut map.pk3;
    let mut renderer = WorldRenderer::new(
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
    let mut sky = match apply_skybox(&mut map.pk3, &geometry.sky_shaders) {
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
    // World fog from worldspawn; the sky stands at its shader's height for it.
    let world_fog = map
        .bsp
        .entities
        .iter()
        .find(|e| e.classname == "worldspawn")
        .and_then(|w| parse_world_fog(&w.properties.iter().collect::<Vec<_>>()));
    let sky_height = geometry
        .sky_shaders
        .iter()
        .find_map(|s| s.sky_parms.as_ref())
        .map_or(0.0, |p| p.sky_height);
    if let Some(f) = &world_fog {
        println!(
            "fog: {:.0} units to 95%, colour {:?}, from {}{}",
            3.0 / f.density,
            f.color,
            f.min_dist,
            f.height
                .map(|h| format!(", clear at {} and full at {}", h.clear, h.full))
                .unwrap_or_default()
        );
    }
    let mut fog = WorldFogController::new(world_fog, sky_height);
    // The fog the water's room captures draw with.
    shagger::core::cvar::with_cvars(|c| fog.refresh(c));
    renderer.set_fog(fog.uniforms());
    if let Some(sky) = &mut sky {
        sky.set_fog(fog.uniforms());
    }
    let mut water = claim_liquid_surfaces(gpu, map, &geometry, sky.as_ref(), format);
    if let Some(water) = &mut water {
        renderer.set_hidden_batches(water.hidden_batches());
        let camera = Camera::default();
        let captured = water.capture_reflections(gpu, &geometry, &renderer, sky.as_ref(), camera.near, camera.far);
        // MapLoader: the map's lights, then its emitting panels.
        let options = shagger::loader::shader::q3_shader_types::ResolveMaterialOptions::default();
        let panels = extract_emissive_surfaces(&map.bsp, |name| {
            let strength = map
                .shaders
                .resolve_material_props(&map.pk3, name, &options)
                .surface_light;
            SurfaceEmission {
                emissive: strength > 0.0,
                surface_light: strength,
            }
        });
        let mut sources = water_map_lights(&map.bsp.entities);
        sources.extend(water_surface_lights(&panels));
        let lit = water.prepare_lighting(gpu, &geometry, &sources);
        println!(
            "water: {} authored highlight candidates, {lit} cached static highlight sources",
            sources.len()
        );
        println!(
            "water: {captured} room captures of {} pools, at {}",
            water.probe_cache().patches.len(),
            water
                .probe_positions()
                .map(|p| format!("({:.0} {:.0} {:.0})", p.x, p.y, p.z))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
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
    let mut hand_weapons = HandWeapons::default();
    if !map.players.is_empty() || map.remote_players || !map.teleports.is_empty() || !map.ragdolls.is_empty() {
        let pk3 = &mut map.pk3;
        match RemotePlayerRenderer::load(gpu, &mut models, |path| pk3.get_file(path)) {
            Ok(mut players) => {
                hand_weapons = HandWeapons::load(gpu, &mut models, pk3);
                println!("hand weapons: {} loaded", hand_weapons.len());
                let style = get_player_model_visual_style(PlayerTeamSlot::Ffa, MODEL_SCALE);
                for (i, player) in map.players.iter().enumerate() {
                    let id = format!("player{i}");
                    let interp = player.interpolated();
                    players.update(&id, &interp, 0.0, 1.0, Some(&player.clip));
                    players.go_to_frame(&id, player.frame);
                    let first = model_draws.len();
                    players.draws(&id, &interp, &style, &mut joints, &mut model_draws);
                    update_powerup_glow(&mut model_draws[first..], interp.powerup_mask.unwrap_or(0), 0.0);
                    if let (Some(weapon), Some(hand)) = (&player.weapon, players.hand_world(&id, &interp)) {
                        let quad = player.quad.then_some(get_quad_damage_pulse(0.0));
                        hand_weapons.draws(weapon, hand, quad, &mut model_draws);
                    }
                }
                remote_players = Some(players);
            }
            Err(why) => eprintln!("player model: {why}"),
        }
    }
    let gib_renderer = GibRenderer::new(gpu, &mut models);
    // The ragdolls' world: the drawn map, liquid surfaces left out
    // (`createBspCollision`).
    let ragdolls = remote_players.as_ref().map(|_| {
        let gravity =
            shagger::core::cvar::with_cvars(|c| shagger::movement::movement_vars::movement_var(c, "sv_gravity"));
        let mut ragdolls = RagdollSystem::new(gravity as f32);
        let hidden = water.as_ref().map_or(&[][..], |w| w.hidden_batches());
        let (positions, triangles) = world_triangles(&geometry, hidden);
        ragdolls.physics.create_bsp_collision(&positions, &triangles);
        println!("ragdolls: {} collision triangles", triangles.len());
        ragdolls
    });
    let mut teleport_renderer = TeleportRenderer::new(
        gpu,
        format,
        &mut models,
        remote_players.as_ref().map(|r: &RemotePlayerRenderer| &r.model),
    );
    let mut teleports = TeleportEffect::new(teleport_renderer.has_ghosts());
    for held in &map.teleports {
        let from = babylon(held.from);
        let to = babylon(held.to);
        teleports.player_teleported(from, to, held.yaw, held.yaw, legacy_team_color(None, "player"));
    }
    if let Some(age) = map.teleports.iter().map(|t| t.age).reduce(f64::min) {
        // Held: the clamp on a frame would cut a long age short.
        for _ in 0..(age * 100.0).ceil() as usize {
            teleports.update(age / (age * 100.0).ceil());
        }
    }
    let player_model = remote_players.as_ref().map(|r: &RemotePlayerRenderer| &r.model);
    teleport_renderer.advance(player_model, 0.0);
    teleport_renderer.ghost_draws(player_model, &teleports, &mut joints, &mut model_draws);
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
    // Splash crowns from Medium up (`LocalLiquid.start`).
    let mut liquid = LiquidInteractionSystem::default();
    if shagger::core::cvar::with_cvars(|c| {
        c.get("r_water_quality")
            .map_or(2.0, |_| c.get_number("r_water_quality"))
    }) >= 2.0
    {
        liquid.enable_crowns();
    }
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
    // The HUD's Rajdhani Bold stands in for the numbers' Arial.
    let damage_font = shagger::ui::font::Font::from_bytes(include_bytes!("../assets/fonts/Rajdhani-Bold.ttf").to_vec())
        .expect("the client ships Rajdhani");
    let cartoon_hit_renderer = CartoonHitRenderer::load(gpu, format, &mut map.pk3);
    let mut damage_numbers = DamageNumbers::new();
    let mut cartoon_hits = CartoonHitEffect::new(
        cartoon_hit_renderer
            .as_ref()
            .map_or(0, CartoonHitRenderer::sprite_count),
    );
    for hit in &map.hits {
        let at = babylon(hit.origin);
        damage_numbers.spawn(at, hit.damage, hit.head.then_some("head"));
        cartoon_hits.spawn(at, hit.damage);
    }
    // Every held hit at its age (the newest's, if they differ).
    if let Some(age) = map.hits.iter().map(|h| h.age).reduce(f64::min) {
        damage_numbers.update(age);
        cartoon_hits.update(age);
    }
    let mut renderer = MapRenderer {
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
        fog,
        fog_serial: None,
        water,
        water_time: 0.0,
        liquid,
        liquid_clock: 0.0,
        splash: SplashDropletRenderer::new(gpu, format),
        teleports,
        teleport_renderer,
        teleport_last: HashMap::new(),
        gibs: GibSystem::default(),
        gib_renderer,
        live_tints: HashMap::new(),
        ragdolls,
        ragdoll_trails: RagdollTrails::default(),
        hand_weapons,
        live_weapons: HashMap::new(),
        ragdolled: HashSet::new(),
        departed: HashMap::new(),
        live_worlds: HashMap::new(),
        local_player: None,
        local_death: None,
        damage_numbers,
        damage_number_renderer: DamageNumberRenderer::new(gpu, format, &damage_font),
        cartoon_hits,
        cartoon_hit_renderer,
        underwater: UnderwaterView::default(),
        underwater_fog: None,
        applied_fog: None,
        culling,
        cull: true,
        lights,
        dynamic_lights: true,
        post: ImageProcessingPostProcess::new(gpu, output_format),
        image_processing: shag_image_processing(),
        view_model,
        holster,
    };
    // Held gibs (`--gib`): broken up, then flown on to their age.
    for held in &map.gibs {
        renderer.gibs.spawn(babylon(held.origin), DVec3::ZERO, None);
    }
    if let Some(age) = map.gibs.iter().map(|g| g.age).reduce(f64::min) {
        for _ in 0..(age * 60.0).round() as usize {
            renderer.play_gibs(&[], 1.0 / 60.0);
        }
    }
    // Held ragdolls (`--ragdoll`): an idle body launched, then fallen on to
    // its age; one that has faded by then has burst into gibs.
    if let (Some(ragdolls), Some(players)) = (&mut renderer.ragdolls, &mut renderer.remote_players) {
        let style = get_player_model_visual_style(PlayerTeamSlot::Ffa, MODEL_SCALE);
        for (i, held) in map.ragdolls.iter().enumerate() {
            let id = format!("ragdoll{i}");
            let interp = held.player.interpolated();
            players.update(&id, &interp, 0.0, 1.0, Some(&held.player.clip));
            players.go_to_frame(&id, held.player.frame);
            if let Some(instance) = players.take(&id) {
                ragdolls.spawn(
                    &players.model,
                    instance,
                    body_matrix(&interp),
                    babylon(held.dir),
                    style.tint_color,
                );
            }
        }
        if let Some(age) = map.ragdolls.iter().map(|r| r.age).reduce(f64::min) {
            for _ in 0..(age * 60.0).round() as usize {
                ragdolls.update(&players.model, 1.0 / 60.0);
                for (at, color) in ragdolls.take_gibs() {
                    renderer.gibs.spawn(at, DVec3::ZERO, color);
                }
            }
        }
    }
    renderer
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
    map.hits.clone_from(&o.hits);
    map.teleports.clone_from(&o.teleports);
    map.gibs.clone_from(&o.gibs);
    map.ragdolls.clone_from(&o.ragdolls);
    map.weapon.clone_from(&o.weapon);
    map.remote_players = feed.is_some();
    // The highlight reel: analyzed and cut before the first frame, its
    // ground probe against the map's brushes.
    let mut reel = match (&mut feed, o.reel) {
        (Some(Feed::Demo(demo)), Some(minutes)) => {
            let mut reel = CinematicDemoPlayer::new(REEL_TITLE_MS);
            reel.load_and_play(
                demo,
                &mut SceneReelWorld {
                    shots: map.shot_world.as_mut(),
                    players: &[],
                },
                minutes,
            );
            print_reel(&reel);
            Some(reel)
        }
        _ => None,
    };
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
        return crate::window::run(map, camera, o.seconds, feed, o.follow, audio, reel);
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
    let mut overlay = crate::reel_overlay::ReelOverlay::default();
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
        let base_fov = camera.fov_y;
        if reel.is_some() {
            // Movie mode: the reel draws its own cards.
            hud = None;
        }
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
            if let (Some(reel), Feed::Demo(demo)) = (&mut reel, &mut *feed) {
                reel.update(
                    1000.0 / 60.0,
                    demo,
                    &mut SceneReelWorld {
                        shots: renderer.shot_world_mut(),
                        players: &players,
                    },
                );
                overlay.update(reel, 1.0 / 60.0);
            }
            let view = feed.advance(1.0 / 60.0);
            if let Feed::Play(p, _) = feed {
                if let Some(yaw) = p.take_spawn_facing() {
                    camera.yaw = yaw as f32;
                    camera.pitch = 0.0;
                }
                if let Some(t) = p.take_teleport() {
                    // `setViewYaw(exitYaw)`, then `localTeleported`.
                    camera.yaw = t.exit_yaw as f32;
                    renderer.teleports.local_teleported(
                        DVec3::new(t.from.x, t.from.y, t.from.z),
                        DVec3::new(t.to.x, t.to.y, t.to.z),
                        t.entry_yaw,
                        t.exit_yaw,
                        TeamColor::Alpha,
                    );
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
            let eyes = reel
                .as_ref()
                .and_then(|r| apply_reel_camera(&mut camera, r, &players, base_fov));
            let drawn: Vec<_> = players
                .iter()
                .filter(|(id, _, _)| Some(id) != eyes.as_ref())
                .cloned()
                .collect();
            renderer.set_live_weapons(&view.weapons);
            renderer.set_live_players(&drawn, 1.0 / 60.0);
            renderer.set_live_projectiles(&projectiles, 1.0 / 60.0);
            renderer.play_effects(&view.effects, &view.weapons, 1.0 / 60.0);
            renderer.play_gibs(&view.effects, 1.0 / 60.0);
            renderer.play_hit_feedback(
                &view.effects,
                view_entity(feed.local_entity(), &players, o.follow),
                1.0 / 60.0,
            );
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
        view_model.powerup_outline = o.quad.then(|| quad_view_weapon_outline(0.0));
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
    // One long step: a still is fully under or fully dry.
    renderer.update_underwater(&mut camera, 1.0);
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
    if let (Some(reel), Some(Feed::Demo(demo))) = (&reel, &feed) {
        let info = reel.reel_info(demo);
        println!(
            "reel: {:?}, shot {} of {}, scene {} of {}, {:.1} of {:.1} s",
            info.state,
            (info.current_segment + 1).min(info.total_segments),
            info.total_segments,
            info.current_scene + 1,
            info.total_scenes,
            info.reel_elapsed_ms / 1000.0,
            info.reel_total_ms / 1000.0
        );
    }
    let mut pixels = render_hdr_scene_with_ui_to_rgba(
        &gpu,
        o.width,
        o.height,
        CLEAR,
        &renderer.post,
        &renderer.image_processing,
        |pass| renderer.draw_opaque(pass),
        |encoder, hdr| renderer.capture(encoder, hdr),
        |pass| renderer.draw_transparent(pass),
        |pass| renderer.draw_overlay(pass),
        |pass| {
            if let Some(hud) = &hud {
                hud.draw(pass);
            }
        },
    );
    if let (Some(reel), Some(Feed::Demo(demo))) = (&reel, &feed) {
        composite_reel_overlay(&gpu, &mut pixels, o.width, o.height, &overlay, reel, demo);
    }
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

/// The reel's shot list, as `demo highlights` prints it.
fn print_reel(reel: &CinematicDemoPlayer) {
    let Some(r) = reel.reel() else { return };
    println!(
        "reel: {} shots in {} scenes, {:.1} s",
        r.total_segments,
        r.total_scenes,
        r.total_duration_ms / 1000.0
    );
    for s in &r.segments {
        let card = washoi::client::reel_cards::shot_card(s, r.total_scenes);
        println!(
            "  {:>2}. ticks {}-{} {:<16} {:<6} {} by {} ({:.1} s)",
            s.segment_index + 1,
            s.start_tick,
            s.end_tick,
            s.camera_preset.as_str(),
            s.role.as_str(),
            card.title,
            card.player,
            s.duration_ms / 1000.0
        );
    }
}

/// Draws the reel's overlay offscreen and lays it over `pixels`.
fn composite_reel_overlay(
    gpu: &Gpu,
    pixels: &mut [u8],
    width: u32,
    height: u32,
    overlay: &crate::reel_overlay::ReelOverlay,
    reel: &CinematicDemoPlayer,
    demo: &DemoPlayback,
) {
    use shagger::ui::egui_layer::EguiLayer;
    let mut layer = EguiLayer::offscreen(gpu, OFFSCREEN_FORMAT);
    washoi::client::lobby_panel::install_style(&layer.ctx);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32, height as f32),
        )),
        ..Default::default()
    };
    // Twice: egui lays text out on its first frame, which also brings the
    // font atlas the second one draws with.
    let clear = wgpu::Color::TRANSPARENT;
    let mut ui = Vec::new();
    for _ in 0..2 {
        let frame = layer.run_input(input(), |ui| overlay.ui(ui.ctx(), reel, demo));
        ui = shagger::rendering::offscreen::render_encoded_to_rgba(gpu, width, height, clear, |encoder, view| {
            layer.paint(gpu, encoder, view, [width, height], frame);
        });
    }
    // egui's colours are premultiplied: over = ui + scene * (1 - alpha).
    for (dst, src) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(ui.as_chunks::<4>().0) {
        let keep = 255 - u16::from(src[3]);
        for c in 0..3 {
            dst[c] = (u16::from(src[c]) + u16::from(dst[c]) * keep / 255).min(255) as u8;
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
