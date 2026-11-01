//! `washoi view`: a map drawn by the renderer (RUST.md 21.10),
//! from a spawn point. With `--screenshot` it renders one frame offscreen and
//! writes a PNG; otherwise it opens a window with a free camera.

use std::path::{Path, PathBuf};

use shagger::loader::bsp::bsp_loader::{BspFile, load};
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::texture::resolve_and_load_texture;
use shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT;
use shagger::rendering::camera::Camera;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_to_rgba};
use shagger::rendering::sky_renderer::{SkyRenderer, apply_skybox};
use shagger::rendering::wgpu;
use shagger::rendering::world_culling::WorldCulling;
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};

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
}

/// The packages, their shader scripts, the map and its spawn points.
pub struct LoadedMap {
    pub pk3: Pk3Loader,
    pub shaders: Q3ShaderRegistry,
    pub bsp: BspFile,
    pub spawns: Vec<Camera>,
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
    })
}

/// The map's sky and world, and which world batches the camera can see.
pub struct MapRenderer {
    pub world: WorldRenderer,
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
        if let Some(sky) = &self.sky {
            sky.prepare(gpu, camera, aspect);
        }
    }

    /// Records the sky, then the world.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(sky) = &self.sky {
            sky.draw(pass);
        }
        if self.cull {
            self.world.draw_visible(pass, self.culling.visible());
        } else {
            self.world.draw(pass);
        }
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
    MapRenderer {
        world: renderer,
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
