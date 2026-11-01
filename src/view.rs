//! `washoi view`: a map drawn by the renderer's first slice (RUST.md 21.10),
//! from a spawn point. With `--screenshot` it renders one frame offscreen and
//! writes a PNG; otherwise it opens a window with a free camera.

use std::path::{Path, PathBuf};

use shagger::loader::bsp::bsp_loader::{BspFile, load};
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::texture::resolve_and_load_texture;
use shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT;
use shagger::rendering::camera::Camera;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_to_rgba};
use shagger::rendering::wgpu;
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};

/// `SPAWN_CLASSNAMES` (wash-fps `shared/bsp/BspDataLoader.ts`).
const SPAWN_CLASSNAMES: [&str; 4] = [
    "info_player_deathmatch",
    "info_player_start",
    "info_player_team1",
    "info_player_team2",
];

/// The clear colour until the sky is drawn.
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
}

/// The packages, the map and its spawn points.
pub struct LoadedMap {
    pub pk3: Pk3Loader,
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
    Ok(LoadedMap { pk3, bsp, spawns })
}

/// Builds the world and uploads it with its textures.
pub fn build_renderer(gpu: &Gpu, map: &mut LoadedMap, format: wgpu::TextureFormat) -> WorldRenderer {
    let geometry = build_world_geometry(&map.bsp);
    let mut missing = Vec::new();
    let pk3 = &mut map.pk3;
    let renderer = WorldRenderer::new(
        gpu,
        &map.bsp,
        &geometry,
        |name| {
            let found = resolve_and_load_texture(pk3, name).map(|t| Rgba8Image {
                width: t.width,
                height: t.height,
                pixels: t.pixels,
            });
            if found.is_none() {
                missing.push(name.to_owned());
            }
            found
        },
        format,
    );
    let triangles: u32 = geometry.batches.iter().map(|b| b.index_count / 3).sum();
    println!(
        "world: {} vertices, {triangles} triangles in {} batches, {} textures ({} without an image file yet), {} sky faces",
        geometry.vertices.len(),
        geometry.batches.len(),
        renderer.texture_count(),
        missing.len(),
        geometry.sky_faces.len()
    );
    if !missing.is_empty() {
        println!(
            "  no image file (shader-defined, for the shader port): {}",
            missing.join(", ")
        );
    }
    renderer
}

pub fn run(o: &ViewOptions) -> i32 {
    let mut map = match load_map(&o.data, &o.map) {
        Ok(m) => m,
        Err(why) => {
            eprintln!("{why}");
            return 1;
        }
    };
    let Some(&camera) = map.spawns.get(o.spawn).or(map.spawns.first()) else {
        eprintln!("{} has no spawn point", o.map);
        return 1;
    };
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
    let renderer = build_renderer(&gpu, &mut map, OFFSCREEN_FORMAT);
    renderer.prepare(&gpu, camera.view_projection(o.width as f32 / o.height as f32));
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
