//! `washoi view`: a map drawn by the renderer (RUST.md 21.10),
//! from a spawn point. With `--screenshot` it renders one frame offscreen and
//! writes a PNG; otherwise it opens a window with a free camera. Players
//! given with `--player` stand on the map as the Vanguard model.

use std::path::{Path, PathBuf};

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
use shagger::rendering::glam::{Mat4, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ImageProcessing, ModelDraw, ModelRenderer};
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_to_rgba};
use shagger::rendering::sky_renderer::{SkyRenderer, apply_skybox};
use shagger::rendering::wgpu;
use shagger::rendering::world_culling::WorldCulling;
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};
use shagger::shared::collision::shared_collision_world::SharedCollisionWorld;
use shagger::weapon::projectile_manager::{
    MAX_TRAIL_POINTS, ProjectileModel, ProjectileTrail, load_projectile_model, projectile_matrix,
};
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
    pub players: Vec<PlayerView>,
    pub projectiles: Vec<ProjectileView>,
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
    /// The joint palettes `model_draws` index.
    pub joints: Vec<Mat4>,
    pub sky: Option<SkyRenderer>,
    pub culling: WorldCulling,
    /// Off draws every batch (`--no-cull`), for comparing.
    pub cull: bool,
}

impl MapRenderer {
    /// Writes this frame's camera and culls for it.
    pub fn prepare(&mut self, gpu: &Gpu, camera: &Camera, aspect: f32) {
        self.culling.update(camera, aspect);
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
        self.models.draw_blended(pass);
    }
}

/// Builds the world and its sky and uploads them with their textures.
pub fn build_renderer(gpu: &Gpu, map: &mut LoadedMap, format: wgpu::TextureFormat) -> MapRenderer {
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
    // SHAG's lights for a BSP map, with r_bsp_pbr at its default, and the
    // scene's image processing (contrast 1.05).
    models.set_lights(bsp_mode_lights(true));
    models.set_image_processing(ImageProcessing {
        contrast: 1.05,
        exposure: 1.0,
    });
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
                projectiles.push((model, projectile_matrix(position, direction, scale)));
                if def.trail_effect.is_some() {
                    trails.push((held_trail(position, direction, def.speed, def.trail_length), light));
                }
            }
            Err(why) => eprintln!("projectile model: {why}"),
        }
    }
    MapRenderer {
        projectiles,
        trails,
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
    // Shader animation as it stands `--time` seconds after the map loads.
    renderer.world.animate(&gpu, o.time);
    renderer.item_time = o.time;
    renderer.prepare(&gpu, &camera, o.width as f32 / o.height as f32);
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
    let pixels = render_to_rgba(&gpu, o.width, o.height, CLEAR, |pass| renderer.draw(pass));
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
