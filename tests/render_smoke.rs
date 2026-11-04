//! The renderer draws a shipped map: from each spawn point of
//! washoi_dm2, nearly every pixel must be world, not the clear colour. A
//! flipped cull side or a broken camera leaves most of the frame empty.
//!
//! Needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills, with its
//! `pk3` folder) and a GPU adapter, software ones included; it reports a skip
//! without either.

use std::path::PathBuf;

use shagger::loader::bsp::bsp_loader::load;
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::texture::resolve_and_load_texture;
use shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT;
use shagger::rendering::camera::Camera;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_to_rgba};
use shagger::rendering::wgpu;
use shagger::rendering::world_geometry::build_world_geometry;
use shagger::rendering::world_renderer::{Rgba8Image, WorldRenderer};

#[test]
fn dm2_fills_the_frame_from_every_spawn() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU adapter");
        return;
    };
    let mut pk3 = Pk3Loader::new();
    assert!(ingest_pk3_dir(&mut pk3, &dir.join("pk3")) > 0);
    let bsp = load(&pk3.get_file("maps/washoi_dm2.bsp").unwrap()).unwrap();
    let mut shaders = Q3ShaderRegistry::new();
    shaders.load_shaders(&mut pk3);
    let geometry = build_world_geometry(&bsp, |name, options| {
        shaders.resolve_material_passes(&pk3, name, options)
    });
    let renderer = WorldRenderer::new(
        &gpu,
        &bsp,
        &geometry,
        |name| {
            resolve_and_load_texture(&mut pk3, name).map(|t| Rgba8Image {
                width: t.width,
                height: t.height,
                pixels: t.pixels,
            })
        },
        &std::collections::HashMap::new(),
        OFFSCREEN_FORMAT,
    );
    let clear = wgpu::Color {
        r: 1.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };
    let spawns: Vec<Camera> = bsp
        .common
        .entities
        .iter()
        .filter(|e| e.classname == "info_player_deathmatch")
        .map(|e| {
            let o: Vec<f32> = e
                .properties
                .get("origin")
                .unwrap()
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect();
            let angle = e.properties.get("angle").map_or(0.0, |a| a.parse().unwrap());
            Camera::from_q3([o[0], o[1], o[2] + DEFAULT_VIEWHEIGHT as f32], angle)
        })
        .collect();
    assert!(spawns.len() >= 4);
    let (w, h) = (320, 180);
    for (i, camera) in spawns.iter().enumerate() {
        renderer.prepare(&gpu, camera.view_projection(w as f32 / h as f32), camera.position);
        let pixels = render_to_rgba(&gpu, w, h, clear, |pass| renderer.draw(pass));
        let empty = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| **p == [255, 0, 255, 255])
            .count();
        let share = empty as f64 / f64::from(w * h);
        assert!(share < 0.02, "spawn {i}: {:.1}% of the frame is empty", share * 100.0);
    }
}
