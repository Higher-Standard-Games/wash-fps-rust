//! Shader animation against SHAG's own (shagger
//! `tools/golden/stage_evaluation.mjs`): every animated stage of every
//! shipped shader, its tcMods stepped 600 frames and its waves sampled, bit
//! for bit.
//!
//! Needs `WASHOI_MAPS` (the folder `tools/fetch_maps.sh` fills); skipped
//! without it.

mod movement_harness;

use std::path::PathBuf;

use movement_harness::{fnv, num};
use shagger::loader::pk3::node_pk3::ingest_pk3_dir;
use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::shader::q3_shader_registry::Q3ShaderRegistry;
use shagger::loader::shader::q3_stage_evaluation::{create_tc_state, evaluate_wave_func, step_tc_mods_into};
use shagger::shared::json::{self, Value};

/// The generator's frame lengths.
fn frame_seconds(i: usize) -> f64 {
    0.016 + (i % 7) as f64 * 0.001
}

/// The generator's Park-Miller `noise` source.
fn park_miller() -> impl FnMut() -> f64 {
    let mut x = 1.0f64;
    move || {
        x = (x * 16807.0) % 2_147_483_647.0;
        x / 2_147_483_647.0
    }
}

#[test]
fn shader_animation_steps_as_shag_steps_it() {
    let Some(dir) = std::env::var_os("WASHOI_MAPS").map(PathBuf::from) else {
        eprintln!("skipped: set WASHOI_MAPS to the folder tools/fetch_maps.sh fills");
        return;
    };
    let doc = json::parse(include_str!("golden/stage_evaluation.json")).unwrap();
    let frames = doc.get("frames").and_then(Value::as_f64).unwrap() as usize;
    let want = doc.get("shaders").and_then(Value::as_array).unwrap();
    let mut pk3 = Pk3Loader::new();
    assert!(ingest_pk3_dir(&mut pk3, &dir.join("pk3")) > 0);
    let mut registry = Q3ShaderRegistry::new();
    registry.load_shaders(&mut pk3);
    let mut names: Vec<&str> = registry.shader_names().collect();
    names.sort_unstable();

    let mut got = Vec::new();
    for name in names {
        let shader = registry.get_shader(name).unwrap();
        let mut lines = Vec::new();
        for (index, stage) in shader.stages.iter().enumerate() {
            let tc_mods = stage.tc_mod.clone().unwrap_or_default();
            let waves: Vec<_> = [stage.rgb_gen.and_then(|g| g.wave), stage.alpha_gen.and_then(|g| g.wave)]
                .into_iter()
                .flatten()
                .collect();
            if tc_mods.is_empty() && waves.is_empty() {
                continue;
            }
            let mut rng = park_miller();
            let mut state = create_tc_state(&tc_mods);
            let mut elapsed = 0.0;
            for i in 0..frames {
                let dt = frame_seconds(i);
                elapsed += dt;
                step_tc_mods_into(&mut state, &tc_mods, dt, elapsed, &mut rng);
                let sampled: Vec<String> = waves
                    .iter()
                    .map(|w| num(evaluate_wave_func(w, elapsed, &mut rng)))
                    .collect();
                lines.push(format!(
                    "{index} {i} {} {}",
                    [
                        state.u_offset,
                        state.v_offset,
                        state.rotation,
                        state.u_scale,
                        state.v_scale
                    ]
                    .map(num)
                    .join(","),
                    sampled.join(",")
                ));
            }
        }
        if !lines.is_empty() {
            got.push((name.to_owned(), fnv(&lines.join("\n"))));
        }
    }
    assert_eq!(got.len(), want.len(), "animated shader count");
    for ((name, hash), entry) in got.iter().zip(want) {
        let entry = entry.as_array().unwrap();
        assert_eq!(name, entry[0].as_str().unwrap());
        assert_eq!(
            hash,
            entry[1].as_str().unwrap(),
            "{name} (DUMP={name} prints SHAG's first lines)"
        );
    }
    eprintln!("{} animated shaders matched", got.len());
}
