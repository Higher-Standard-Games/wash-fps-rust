//! `washoi view`: a map drawn by the renderer (RUST.md 21.10),
//! from a spawn point. With `--screenshot` it renders one frame offscreen and
//! writes a PNG; otherwise it opens a window with a free camera. Players
//! given with `--player` stand on the map as the Vanguard model.

use shagger::lighting::map_emissive_surfaces::{SurfaceEmission, extract_emissive_surfaces};
use shagger::loader::bsp::fog_volumes::{FogVolumes, fog_volumes_from};
use shagger::loader::bsp::map_particles::MapParticles;
use shagger::physics::physics_system::world_triangles;
use shagger::physics::sweep_backend_adapter::to_shared_contents;
use shagger::rendering::water::water_surface_lighting::{water_map_lights, water_surface_lights};
use shagger::shared::bsp::brush_model_state::{BrushModelState, extract_brush_models};
use shagger::shared::bsp::lift_system::{LiftState, LiftSystem, extract_lifts};
use shagger::shared::js::js_hypot;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use washoi::client::bomb_renderer::{BombRenderer, CARRY_FALLBACK_HEIGHT};
use washoi::client::bomb_site_debris::BombSiteDebris;
use washoi::client::bomb_site_markers::{BLAST_SCALE, BLAST_SHAKE_RANGE, BombSiteMarkers, MarkerPose};
use washoi::client::cartoon_hit_effect::{CartoonHitEffect, CartoonHitRenderer};
use washoi::client::damage_numbers::{DamageNumberRenderer, DamageNumbers};
use washoi::client::dropped_weapon_renderer::{DropThrow, DroppedWeaponInfo, DroppedWeaponRenderer};
use washoi::client::fps_liquid_audio::play_liquid_crossing;
use washoi::client::fps_player_audio::{player_voice_for_entity, set_local_player_voice};
use washoi::client::gib_system::{GibRenderer, GibSystem};
use washoi::client::hit_spatter::{Hit, HitSpatter, HitSpatterRenderer, HitZone, bloodless};
use washoi::client::hud::widgets::loadout_picker;
use washoi::client::hud::widgets::pickup_prompt::{horizontal_distance_sq, in_pickup_range};
use washoi::client::kill_taunt::{KillFacts, choose_kill_taunt};
use washoi::client::player_appearance::{AppearanceView, PlayerAppearance};
use washoi::client::player_model_selection::{PlayerModelSide, player_model_choice};
use washoi::client::post_game_showcase::{PostGameShowcase, ShowcaseStart};
use washoi::client::quad_damage_visual::{get_quad_damage_pulse, mask_has_quad};
use washoi::client::ragdoll::death_clip::DeathFacts;
use washoi::client::ragdoll_system::{RagdollSystem, RagdollTrails};
use washoi::client::remote_player_renderer::{DetachedBody, HandWeapons, update_powerup_glow};
use washoi::client::slide_dust::{SlideDust, SlideDustRenderer};
use washoi::client::teleport_effect::{TeamColor, TeleportEffect, TeleportRenderer, legacy_team_color, play_tele_in};
use washoi::liquid::client::liquid_interaction_system::{LiquidInteractionSystem, Ring, WaterRipples};
use washoi::liquid::client::liquid_ring_renderer::LiquidRingRenderer;
use washoi::liquid::client::splash_droplet_renderer::SplashDropletRenderer;
use washoi::liquid::client::underwater_view::{UnderwaterFog, UnderwaterView};
use washoi::liquid::liquid_types::LiquidKind;

use shagger::animation::player_model_manager::PlayerModelVisualStyle;
use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};
use shagger::babylon::post_processes::image_processing_post_process::{
    ImageProcessingConfiguration, ImageProcessingPostProcess, Vignette, VignetteBlendMode,
};
use shagger::core::cvar::with_cvars;
use shagger::lighting::sky_light_system::bsp_mode_lights;
use shagger::loader::bsp::bsp_loader::{BspFile, BspLightVol, LightGrid, load, load_collision};
use shagger::loader::bsp::light_grid_sampling::{LightSample, sample_light_vols};
use shagger::loader::bsp::pvs_culler::{PvsCuller, create_pvs_culler};
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
use shagger::rendering::render_toggles::render_toggle;
use washoi::client::fps_bomb_audio::BOMB_BLAST_SOUND;

use shagger::effects::color_correction_post_process::active_color_matrix;
use shagger::loader::bsp::liquid_classifier::{classify_liquids, liquid_kind_from_surface_parms};
use shagger::loader::bsp::world_fog::parse_world_fog;
use shagger::loader::texture::skybox_loader::load_environment_cubemap;
use shagger::rendering::sky_renderer::{SkyRenderer, apply_skybox, upload_cube};
use shagger::rendering::water::liquid_surface_system::{LiquidSurfaceSystem, WaterSettings};
use shagger::rendering::water::water_actor_reflection::ReflectionActor;
use shagger::rendering::wgpu;
use shagger::rendering::world_culling::WorldCulling;
use shagger::rendering::world_fog_controller::{FogUniforms, WorldFogController};
use shagger::rendering::world_geometry::build_world_geometry_with;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::entities::projectile_def::ProjectileDef;
use shagger::shared::net::game_state::PlayerState;
use shagger::shared::spread_utils::mulberry32;
use shagger::ui::egui_layer::egui;
use shagger::weapon::explosion::explosion_batch_renderer::{ExplosionBatchRenderer, ExplosionLook};
use shagger::weapon::explosion_effect::{ROCKET_EXPLOSION, explosion_flash, explosion_for};
use shagger::weapon::hitscan_effects::{BeamConfig, HitscanEffects, ShotWorld};
use shagger::weapon::impact::impact_spark_renderer::ImpactSparkBurst;
use shagger::weapon::muzzle::muzzle_flash_renderer::{MuzzleFlashSpawn, MuzzleFlashStyle};
use shagger::weapon::point_light_pool::{PointLightPool, PooledLight};
use shagger::weapon::projectile_manager::{
    MAX_TRAIL_POINTS, ProjectileModel, ProjectileTrail, load_projectile_model, projectile_matrix,
};
use shagger::weapon::rail::rail_profiles::{RailEffectSample, RailImpactKind};
use shagger::weapon::tracer::streak_tracer_renderer::StreakTracerShot;
use shagger::weapon::weapon_slash::SlashTrailRenderer;
use shagger::weapon::weapon_switch_anim::{SwitchAnimOffsets, WeaponSwitchAnim};
use shagger::weapon::weapon_view_manager::{ViewModelCvars, ViewModelInput};
use washoi::client::cinematic_demo_player::{
    CinematicDemoPlayer, REEL_TITLE_MS, ReelCamera, ReelWorld, look_at_rotation,
};
use washoi::client::demo_playback::{DemoPlayback, DemoView, LiveProjectile};
use washoi::client::fps_hud::{FpsHud, WatchFrame};
use washoi::client::game_events::{EffectTargets, GameAudio, ShotMuzzles, play_shots};
use washoi::client::live_play::{LivePlay, PlayInput};
use washoi::client::live_watch::LiveWatch;
use washoi::client::match_state_manager::MatchStateManager;
use washoi::client::weapon_view::{WeaponViewModel, blade_tip_flash, muzzle_flash, quad_view_weapon_outline};
use washoi::shared::entities::definitions::projectiles::{PROJECTILES, projectile_def};
use washoi::shared::net::snapshot_events::SnapshotEvent;

use washoi::client::player_model_visual_settings::{PlayerTeamSlot, get_player_model_visual_style};
use washoi::client::remote_player_renderer::{MODEL_SCALE, RemotePlayerRenderer};
use washoi::shared::bsp::pickup_table::{PickupEntry, PickupType, extract_pickups};
use washoi::shared::items::item_throw::land_map_pickups;
use washoi::world::fps_pickup::{ItemModels, build_item_models, build_weapon_pickups};
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
    /// The side to ask for on joining, from the lobby room.
    pub team: Option<String>,
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
    pub spatters: Vec<SpatterView>,
    pub rings: Vec<RingView>,
    pub ragdolls: Vec<RagdollView>,
    /// The first-person weapon to hold (`--weapon`), and whether it is
    /// aimed down its sights (`--aim`).
    pub weapon: Option<String>,
    pub aim: bool,
    /// The gun wears the quad outline (`--quad`).
    pub quad: bool,
    /// The gun fired along the view this many seconds ago (`--fire`).
    pub fire: Option<f64>,
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
    /// `washoi bench`: frames to time, warm-up frames, where to write JSON.
    pub bench: Option<(u64, u64, Option<PathBuf>)>,
    /// Hold the scoreboard open (`--scoreboard`, Tab in the window).
    pub scoreboard: bool,
    /// Show the post-game's winner in front of the camera (`--showcase`),
    /// holding this weapon (`none` for none), in the enemy side's style.
    pub showcase: Option<String>,
    /// Open the loadout picker over the shot (`--loadout`), on this weapon
    /// (`""` for the kit's primary).
    pub loadout: Option<String>,
    /// Join the match in `watch` as a player (`washoi play`).
    pub join: bool,
    /// Something to say once in the match (`--say`, headless play).
    pub say: Option<String>,
}

/// The HUD's frame of a demo or a match being watched, behind the
/// followed player (if any). In play, `follow` is the teammate watched
/// while dead, whom the HUD then describes.
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
    let followed = follow.and_then(|i| players.get(i));
    let view_entity = local.or_else(|| followed.map(|(id, _, _)| id.as_str()));
    let watched = local.and(followed).map(|(id, _, _)| id.as_str());
    let speed = match feed {
        Feed::Play(p, _) if followed.is_none() => {
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
            watched,
            positions: &positions,
            yaw,
            roster: feed.roster(),
            match_state: feed.match_state(),
            events,
            view_entity,
            playback: matches!(feed, Feed::Demo(_)),
            speed,
            dt,
            air: match feed {
                Feed::Play(p, _) => Some((p.air.air(), p.air.drowning())),
                _ => None,
            },
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

/// `washoi view --fire AGE`: our `weapon` fires once along `camera`'s view
/// as a live shot is presented (the server's `weapon_fire`, and for the
/// rail and the beam their contact 600 units out), then the view and the
/// effects run on for `age` seconds.
fn fire_view_shot(renderer: &mut MapRenderer, camera: &Camera, weapon: &str, age: f64, aiming: bool) {
    use washoi::shared::net::snapshot_events::Point;
    let local = "local";
    renderer.set_local_player(Some(local), f64::from(camera.yaw));
    let eye = camera.position.as_dvec3();
    let forward = camera.forward().as_dvec3();
    let point = |p: DVec3| Point { x: p.x, y: p.y, z: p.z };
    let mut events = vec![SnapshotEvent::WeaponFire {
        entity_id: local.to_owned(),
        weapon_id: weapon.to_owned(),
        origin: point(eye),
        dir: point(forward),
        pellet: Some((7, 8.0)),
    }];
    if matches!(weapon, "electrobolt" | "voltaic") {
        events.push(SnapshotEvent::Beam {
            electro_rail: weapon == "electrobolt",
            entity_id: local.to_owned(),
            origin: point(eye),
            dir: point(forward),
            end: point(eye + forward * 600.0),
            normal: point(-forward),
            impact_kind: 1,
        });
    }
    if let Some(view_model) = &mut renderer.view_model {
        view_model
            .view
            .present_fire(&washoi::client::weapon_view::shot_style(weapon), false);
    }
    renderer.fire_view_muzzle_flash(camera, false);
    let weapons = HashMap::from([(local.to_owned(), weapon.to_owned())]);
    renderer.play_effects(&events, &weapons, 0.0, camera);
    let steps = (age * 60.0).round() as usize;
    for _ in 0..=steps {
        renderer.set_beam_active(weapon == "voltaic");
        renderer.update_beam(camera);
        if let Some(view_model) = &mut renderer.view_model {
            view_model.view.update(&ViewModelInput {
                delta_time: age / steps.max(1) as f64,
                grounded: true,
                aiming,
                ..ViewModelInput::default()
            });
        }
        renderer.play_effects(&[], &weapons, age / steps.max(1) as f64, camera);
    }
    let (streaks, sparks, lights) = renderer.hitscan.debug_counts();
    let marks = renderer.hitscan.wall_marks.count();
    println!(
        "fire: {weapon} {age} s ago: {streaks} streaks, {sparks} spark bursts, {lights} flash lights, {marks} wall marks"
    );
}

/// `MUZZLE_CONVERGE_SEC`: how long our projectile takes to converge from
/// the barrel onto its true flight.
const MUZZLE_CONVERGE_SECONDS: f64 = 0.22;
/// `PROJECTILE_FORGET_TICKS`: the longest projectile life and a second.
const PROJECTILE_FORGET_SECONDS: f64 = 11.0;

/// `getProjectileMuzzlePosition`: a projectile within 120 units of the eye
/// flying within about 45 degrees of the view starts 34 units out along the
/// screen ray through the muzzle (deep enough that its model and trail
/// clear the near plane); `None` for anything else.
fn projectile_muzzle(view_model: &WeaponViewModel, camera: &Camera, position: DVec3, velocity: DVec3) -> Option<DVec3> {
    if position.distance(camera.position.as_dvec3()) > 120.0 {
        return None;
    }
    let speed = velocity.length();
    if speed <= 1.0 || velocity.dot(camera.forward().as_dvec3()) / speed < 0.7 {
        return None;
    }
    Some(view_model.muzzle_world_position(camera, 34.0))
}

/// `getWeaponMuzzleWorldPositionToRef`'s choices for one drawn player:
/// their gun's muzzle tag, the gun's origin, then the body.
#[derive(Debug, Clone, Copy)]
struct RemoteMuzzle {
    tag: Option<DVec3>,
    root: Option<DVec3>,
    body: DVec3,
}

impl RemoteMuzzle {
    /// The muzzle for a shot along `direction`: the tag; else 10 units
    /// out from the gun; else 5 out from 20 above the body's origin.
    fn resolve(&self, direction: DVec3) -> DVec3 {
        let length = direction.length();
        let forward = if length > 0.0001 { direction / length } else { DVec3::Z };
        if let Some(tag) = self.tag {
            return tag;
        }
        if let Some(root) = self.root {
            return root + forward * 10.0;
        }
        self.body + DVec3::new(0.0, 20.0, 0.0) + forward * 5.0
    }
}

/// This frame's muzzles: ours on screen, the others in their hands.
struct FrameMuzzles<'a> {
    local: Option<&'a str>,
    /// The player watched through their eyes, whose gun is the one on
    /// screen (`setFollowedMuzzle`).
    followed: Option<&'a str>,
    view: Option<(&'a WeaponViewModel, &'a Camera)>,
    remote: &'a HashMap<String, RemoteMuzzle>,
}

impl ShotMuzzles for FrameMuzzles<'_> {
    fn local(&self) -> Option<&str> {
        self.local
    }

    fn view_muzzle(&self, distance: f64) -> Option<DVec3> {
        self.view
            .map(|(view_model, camera)| view_model.muzzle_world_position(camera, distance))
    }

    fn remote_muzzle(&self, entity_id: &str, direction: DVec3) -> Option<DVec3> {
        self.followed_muzzle(entity_id)
            .or_else(|| self.remote.get(entity_id).map(|m| m.resolve(direction)))
    }

    fn followed_muzzle(&self, entity_id: &str) -> Option<DVec3> {
        if self.followed != Some(entity_id) {
            return None;
        }
        self.view_muzzle(5.0)
    }
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
    audio.sfx.maintain();
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
    // A player sounds like the body they are drawn with.
    let looks = view.looks;
    let voice_of = |id: &str| player_voice_for_entity(teams, looks, id, view_entity);
    if let Some(local) = view.local {
        set_local_player_voice(Some(voice_of(local)));
    }
    audio.play_event_sounds(effects, view_entity, &voice_of, &position, now * 1000.0);
    for sound in renderer.teleports.take_sounds() {
        play_tele_in(&mut audio.sfx, &sound);
    }
    for sound in renderer.liquid.take_sounds() {
        play_liquid_crossing(&mut audio.sfx, &sound);
    }
    // The detonation, 2D: everyone hears the round end wherever they are.
    for _ in 0..renderer.take_blast_sounds() {
        audio.sfx.play_2d(
            BOMB_BLAST_SOUND,
            &SfxPlayOptions {
                volume: Some(1.0),
                max_instances: Some(4),
                ..SfxPlayOptions::default()
            },
        );
    }
    audio.play_movement_sounds(view.snapshots, view_entity, &voice_of, &mut |at| {
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
    /// Each free-for-all player's server-assigned look, by entity.
    pub looks: &'a HashMap<String, u8>,
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

    /// The lifts as they stand, whose callers' cooldowns press the buttons
    /// in (`handleButton` reads `buttonOffset`).
    #[must_use]
    pub fn lift_states(&self) -> Vec<LiftState> {
        match self {
            Self::Demo(d) => d.lift_states().to_vec(),
            Self::Live(l) => l.lift_states().to_vec(),
            Self::Play(p, _) => p.lift_states(),
        }
    }

    /// A lift's travel by its model index, which places its brush model.
    #[must_use]
    pub fn lift_travel(&self, model_index: usize) -> Option<f64> {
        match self {
            Self::Demo(d) => d.lift_travel(model_index),
            Self::Live(l) => l.lift_travel(model_index),
            Self::Play(p, _) => p.lift_travel(model_index),
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

    /// The game server's messages since the last call, for the chat.
    pub fn take_notices(&mut self) -> Vec<String> {
        match self {
            Self::Demo(_) => Vec::new(),
            Self::Live(l) => std::mem::take(&mut l.notices),
            Self::Play(p, _) => std::mem::take(&mut p.notices),
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
    /// Nothing to follow (gibs off): the camera pulls back from where the
    /// body was.
    Still,
}

/// Whether the image is antialiased: `r_msaa`, read as the browser reads it
/// for its canvas (`>= 0.5`, on when unset). The browser's world draws
/// single sampled into its post-process chain, so only what reaches the
/// canvas after it (the view weapon) is smoothed there; here FXAA smooths
/// the image and the weapon together.
#[must_use]
pub fn antialias_on() -> bool {
    with_cvars(|c| c.get("r_msaa").is_none() || c.get_number("r_msaa") >= 0.5)
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

/// A liquid ring held on the map (`--ring`).
#[derive(Debug, Clone, PartialEq)]
pub struct RingView {
    /// The ring's Quake centre.
    pub origin: [f64; 3],
    pub kind: washoi::liquid::liquid_types::LiquidKind,
    pub radius: f64,
    pub alpha: f64,
}

/// A hit's spatter, held at an age (`--spatter`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpatterView {
    /// The hit player's Quake origin.
    pub origin: [f64; 3],
    /// Seconds since the hit.
    pub age: f64,
    /// Sparks off a robot rather than blood.
    pub robot: bool,
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
    /// The body (`LOOK_MODELS`); Police when `None`.
    pub body: Option<String>,
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
    /// Hit spatter to hold on the map (`--spatter`); a non-zero `kind` is a
    /// robot's sparks.
    pub spatters: Vec<SpatterView>,
    /// Liquid rings to hold on the map.
    pub rings: Vec<RingView>,
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

/// Reads every package in `data` and the map's BSP from them, reporting
/// the load's phases as it reaches them.
pub fn load_map(data: &Path, map: &str, report: &mut dyn FnMut(PrepareStep)) -> Result<LoadedMap, String> {
    let mut phase = |phase: &'static str, detail: Option<String>| report(PrepareStep::Phase { phase, detail });
    phase("Dependencies", Some("Reading the packages".into()));
    let mut pk3 = Pk3Loader::new();
    let count = ingest_pk3_dir(&mut pk3, data);
    if count == 0 {
        return Err(format!("no packages in {}", data.display()));
    }
    let path = format!("maps/{map}.bsp");
    phase("Geometry", Some(path.clone()));
    let bytes = pk3
        .get_file(&path)
        .ok_or_else(|| format!("{path} is in none of the {count} packages"))?;
    let bsp = load(&bytes).map_err(|e| format!("{path}: {e}"))?;
    phase("Collision", None);
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
    phase("Shaders", None);
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
        spatters: Vec::new(),
        rings: Vec::new(),
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

mod build_renderer;
mod map_renderer;
mod start;

pub use build_renderer::*;
pub use map_renderer::*;
pub use start::*;

#[cfg(test)]
mod tests {
    use super::RemoteMuzzle;
    use shagger::rendering::glam::DVec3;

    #[test]
    fn a_remote_shot_leaves_the_tag_then_the_gun_then_the_body() {
        let mut m = RemoteMuzzle {
            tag: Some(DVec3::new(1.0, 2.0, 3.0)),
            root: Some(DVec3::new(4.0, 5.0, 6.0)),
            body: DVec3::new(10.0, 0.0, 0.0),
        };
        let dir = DVec3::new(0.0, 0.0, 2.0);
        assert_eq!(m.resolve(dir), DVec3::new(1.0, 2.0, 3.0));
        m.tag = None;
        assert_eq!(m.resolve(dir), DVec3::new(4.0, 5.0, 16.0));
        m.root = None;
        assert_eq!(m.resolve(dir), DVec3::new(10.0, 20.0, 5.0));
        // No direction: straight ahead along +Z.
        assert_eq!(m.resolve(DVec3::ZERO), DVec3::new(10.0, 20.0, 5.0));
    }
}
