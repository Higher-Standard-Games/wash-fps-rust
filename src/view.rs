//! `washoi view`: a map drawn by the renderer (RUST.md 21.10),
//! from a spawn point. With `--screenshot` it renders one frame offscreen and
//! writes a PNG; otherwise it opens a window with a free camera. Players
//! given with `--player` stand on the map as the Vanguard model.

use std::path::{Path, PathBuf};

use shagger::babylon::post_processes::image_processing_post_process::{
    ImageProcessingConfiguration, ImageProcessingPostProcess, Vignette, VignetteBlendMode,
};
use shagger::lighting::sky_light_system::bsp_mode_lights;
use shagger::loader::bsp::bsp_loader::{BspFile, load, load_collision};
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::texture::resolve_and_load_texture;
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
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
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_hdr_to_rgba};
use shagger::rendering::sky_renderer::{SkyRenderer, apply_skybox};
use shagger::rendering::wgpu;
use shagger::rendering::world_culling::WorldCulling;
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::shared::spread_utils::mulberry32;
use shagger::weapon::explosion::explosion_batch_renderer::ExplosionBatchRenderer;
use shagger::weapon::explosion_effect::{explosion_flash, explosion_for};
use shagger::weapon::impact::impact_spark_renderer::{ImpactSparkBurst, ImpactSparkRenderer};
use shagger::weapon::muzzle::muzzle_flash_renderer::{MuzzleFlashRenderer, MuzzleFlashSpawn, MuzzleFlashStyle};
use shagger::weapon::point_light_pool::PointLightPool;
use shagger::weapon::projectile_manager::{
    MAX_TRAIL_POINTS, ProjectileModel, ProjectileTrail, load_projectile_model, projectile_matrix,
};
use shagger::weapon::rail::rail_effect_renderer::RailEffectRenderer;
use shagger::weapon::rail::rail_profiles::{RailEffectSample, RailImpactKind};
use shagger::weapon::tracer::streak_tracer_renderer::{StreakTracerRenderer, StreakTracerShot};
use shagger::weapon::voltaic::voltaic_effect_renderer::VoltaicEffectRenderer;
use washoi::shared::entities::definitions::projectiles::projectile_def;

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
    /// Held bullet streaks, muzzle flashes and impact sparks, on the same
    /// clock.
    pub tracers: StreakTracerRenderer,
    pub muzzle_flashes: MuzzleFlashRenderer,
    pub impact_sparks: ImpactSparkRenderer,
    pub rails: RailEffectRenderer,
    pub lightning: VoltaicEffectRenderer,
    /// The joint palettes `model_draws` index.
    pub joints: Vec<Mat4>,
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

impl MapRenderer {
    /// Writes this frame's camera and culls for it.
    pub fn prepare(&mut self, gpu: &Gpu, camera: &Camera, size: [f32; 2]) {
        let aspect = size[0] / size[1];
        self.culling.update(camera, aspect);
        let lights: Vec<DynamicLight> = if self.dynamic_lights {
            self.lights.lights().collect()
        } else {
            Vec::new()
        };
        // The world keeps its lights off until its companion surfaces (normal
        // and gloss maps, drawn as PBRMaterials in the browser) are ported:
        // those take the lights through PBR's BRDF, not StandardMaterial's.
        self.models.set_dynamic_lights(&lights);
        self.world.prepare(gpu, camera.view_projection(aspect), camera.position);
        self.frame_draws.clone_from(&self.model_draws);
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
        self.models.prepare(
            gpu,
            camera.view_projection(aspect),
            camera.position,
            &self.frame_draws,
            &self.joints,
        );
        self.explosions
            .prepare(gpu, camera.view_projection(aspect), camera.view(), 0.0);
        let frame = EffectFrame {
            view_proj: camera.view_projection(aspect),
            view: camera.view(),
            projection: camera.projection(aspect),
            camera_position: camera.position,
            viewport: size,
            time: 0.0,
        };
        self.tracers.prepare(gpu, &frame);
        self.muzzle_flashes.prepare(gpu, &frame);
        self.impact_sparks.prepare(gpu, &frame);
        self.rails.prepare(gpu, &frame);
        self.lightning.prepare(gpu, &frame);
        if let Some(sky) = &self.sky {
            sky.prepare(gpu, camera, aspect);
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
        self.lightning.draw_beams(pass);
        self.rails.draw_beams(pass);
        self.lightning.draw_impacts(pass);
        self.rails.draw_impacts(pass);
        self.tracers.draw(pass);
        self.muzzle_flashes.draw(pass);
        self.impact_sparks.draw(pass);
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
    if !map.players.is_empty() {
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
            }
            Err(why) => eprintln!("player model: {why}"),
        }
    }
    let mut projectiles = Vec::new();
    let mut trails = Vec::new();
    let mut lights = PointLightPool::new();
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
    let mut tracers = StreakTracerRenderer::new(gpu, format);
    let mut muzzle_flashes = MuzzleFlashRenderer::new(gpu, format);
    let mut impact_sparks = ImpactSparkRenderer::new(gpu, format);
    let mut rails = RailEffectRenderer::new(gpu, format);
    let mut lightning = VoltaicEffectRenderer::new(gpu, format);
    for effect in &map.effects {
        match effect {
            EffectView::Tracer { start, end, color, age } => {
                tracers.fire(
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
            } => muzzle_flashes.spawn(
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
            } => impact_sparks.burst(
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
                    LightningMode::Hero => lightning.update_hero(&sample),
                    LightningMode::Remote => {
                        let key = format!("remote{}", lightning.debug_counts().0);
                        lightning.update_remote(-age, &key, &sample, sample.end - sample.start);
                    }
                    // HitscanEffects.fireLightningArc's default duration.
                    LightningMode::Chain => lightning.fire_chain_arc(-age, &sample, 0.14),
                }
            }
            EffectView::Rail {
                start,
                end,
                color,
                kind,
                normal,
                age,
            } => rails.fire_rail(
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
        tracers,
        muzzle_flashes,
        impact_sparks,
        rails,
        lightning,
        world: renderer,
        models,
        model_draws,
        items,
        item_time: 0.0,
        frame_draws: Vec::new(),
        joints,
        sky,
        culling,
        cull: true,
        lights,
        dynamic_lights: true,
        post: ImageProcessingPostProcess::new(gpu, output_format),
        image_processing: shag_image_processing(),
    }
}

pub fn run(o: &ViewOptions) -> i32 {
    let mut map = match load_map(&o.data, &o.map) {
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
    let at = o.at.map(|[x, y, z, angle]| Camera::from_q3([x, y, z], angle));
    let Some(mut camera) = at.or_else(|| map.spawns.get(o.spawn).or(map.spawns.first()).copied()) else {
        eprintln!("{} has no spawn point", o.map);
        return 1;
    };
    camera.pitch = o.pitch.to_radians();
    println!(
        "{}: {} spawn points, viewing from spawn {}",
        o.map,
        map.spawns.len(),
        o.spawn.min(map.spawns.len() - 1)
    );
    let Some(path) = &o.screenshot else {
        return crate::window::run(map, camera, o.seconds);
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
    let pixels = render_hdr_to_rgba(
        &gpu,
        o.width,
        o.height,
        CLEAR,
        &renderer.post,
        &renderer.image_processing,
        |pass| renderer.draw(pass),
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
