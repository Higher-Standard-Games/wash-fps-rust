//! Building a [`MapRenderer`] for a loaded map: wash-fps `MapLoader.ts`'s
//! scene setup. Split out of `view.rs`; the same module tree.

use super::*;

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
    // A fog brush's faces are drawn by its fog volume, not as surfaces.
    let mut geometry = build_world_geometry_with(
        &map.bsp,
        |name, options| map.shaders.resolve_material_passes(&map.pk3, name, options),
        |name| map.shaders.get_shader(name).is_some_and(|s| s.fog_parms.is_some()),
    );
    // `loadTextures`' glow overlay ShaderMaterial (light fixtures).
    geometry.find_glow_overlays(|name| map.shaders.get_shader(name));
    // Authored cubemaps (`surroundmap`/`cubemap`), as `loadTextures` loads
    // them; one that fails draws as a plain material.
    let mut environments = HashMap::new();
    for base in geometry.environment_maps() {
        match load_environment_cubemap(&mut map.pk3, &base) {
            Ok(faces) => {
                println!("environment: {base} ({}x{} faces)", faces.size, faces.size);
                environments.insert(base, faces);
            }
            Err(e) => eprintln!("[BspMeshBuilder] {e}"),
        }
    }
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
        &environments,
        format,
    );
    let triangles: u32 = geometry.batches.iter().map(|b| b.index_count / 3).sum();
    println!(
        "world: {} vertices, {triangles} triangles in {} batches, {} material passes ({} animated), {} images, {} shaders, {} sky faces, {} autosprite faces, {} movers",
        geometry.vertices.len(),
        geometry.batches.len(),
        renderer.pass_count(),
        renderer.animated_pass_count(),
        renderer.texture_count(),
        map.shaders.shader_count(),
        geometry.sky_faces.len(),
        geometry.sprite_faces,
        geometry.movers.len()
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
    let item_pvs = create_pvs_culler(&map.bsp);
    let mut models = ModelRenderer::new(gpu, format);
    // SHAG's lights for a BSP map, with r_bsp_pbr at its default.
    models.set_lights(bsp_mode_lights(true));
    let pickups = build_weapon_pickups(gpu, &mut map.pk3, &mut models, &map.pickups);
    for failure in &pickups.failures {
        eprintln!("pickup model: {failure}");
    }
    let mut model_draws = pickups.draws;
    let weapon_pickup_spans = pickups.spans;
    let weapon_pickup_spots = map
        .pickups
        .iter()
        .filter(|e| e.kind_type == PickupType::Weapon)
        .map(|e| (e.id, e.kind, DVec3::new(e.position.x, e.position.y, e.position.z)))
        .collect();
    let item_models = build_item_models(gpu, &mut map.pk3, &mut models, &map.pickups);
    if !item_models.is_empty() || item_models.failures.is_empty() {
        println!("items: {} packaged models", item_models.len());
    } else {
        // `washoi_models.pk3` is not published yet: their recipes stand in.
        println!("items: no packaged models ({})", item_models.failures.join(", "));
    }
    let recipe_entries: Vec<_> = map.pickups.iter().filter(|e| !item_models.has(e.id)).cloned().collect();
    let mut items = build_procedural_pickups(gpu, &mut map.pk3, &mut models, &recipe_entries);
    if map.remote_players {
        // A match or demo: the server says which are up (`startHidden`).
        items.set_server_managed();
    }
    println!("items: {} procedural pickups", items.len());
    if !items.missing_icons.is_empty() {
        println!("  no ammo icon: {}", items.missing_icons.join(", "));
    }
    let mut joints = Vec::new();
    let mut remote_players = None;
    let mut hand_weapons = HandWeapons::default();
    let mut static_actors = Vec::new();
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
                    players.update(&id, &interp, 0.0, 1.0, Some(&player.clip), player.body.as_deref());
                    players.go_to_frame(&id, player.frame);
                    let first = model_draws.len();
                    players.draws(&id, &interp, &style, &mut joints, &mut model_draws);
                    update_powerup_glow(
                        &mut model_draws[first..],
                        interp.powerup_mask.unwrap_or(0),
                        0.0,
                        players.model.scale_factor(),
                    );
                    if let (Some(weapon), Some(hand)) = (&player.weapon, players.hand_world(&id, &interp)) {
                        let quad = player.quad.then_some(get_quad_damage_pulse(0.0));
                        hand_weapons.draws(weapon, &hand, quad, &mut model_draws);
                    }
                    let at = DVec3::new(interp.x, interp.y, interp.z);
                    static_actors.push((
                        ReflectionActor {
                            min: at + DVec3::new(-15.0, -24.0, -15.0),
                            max: at + DVec3::new(15.0, 32.0, 15.0),
                            meshes: model_draws.len() - first,
                        },
                        first..model_draws.len(),
                    ));
                }
                remote_players = Some(players);
            }
            Err(why) => eprintln!("player model: {why}"),
        }
    }
    let gib_renderer = GibRenderer::new(gpu, &mut models);
    let spatter_renderer = HitSpatterRenderer::new(gpu, &mut models);
    let bomb = BombRenderer::new(gpu, &mut models);
    let slide_dust_renderer = SlideDustRenderer::new(gpu, &mut models);
    // The `func_explosive` brush models (`BrushModelState`), and in a demo
    // or a match the debris models they throw (`BombSiteDebris.preload`).
    let brush_models = BrushModelState::new(extract_brush_models(&map.bsp.entities, &map.bsp.models));
    let mut debris_models = HashMap::new();
    if map.remote_players {
        for path in brush_models.definitions.iter().flat_map(|d| &d.debris) {
            if debris_models.contains_key(path) {
                continue;
            }
            match load_projectile_model(gpu, &mut models, &mut map.pk3, &map.shaders, "debris", path, None) {
                Ok(model) => {
                    debris_models.insert(path.clone(), model);
                }
                Err(why) => eprintln!("debris model: {why}"),
            }
        }
    }
    if !brush_models.definitions.is_empty() {
        println!(
            "brush models: {} explosive, {} debris model(s)",
            brush_models.definitions.len(),
            debris_models.len()
        );
    }
    // The ragdolls' world: the drawn map, liquid surfaces left out
    // (`createBspCollision`).
    let ragdolls = remote_players.as_ref().map(|_| {
        let gravity =
            shagger::core::cvar::with_cvars(|c| shagger::movement::movement_vars::movement_var(c, "sv_gravity"));
        let mut ragdolls = RagdollSystem::new(gravity as f32);
        // Nor the `func_explosive` brushes, which the bomb can take away.
        let water_hidden = water.as_ref().map_or(&[][..], |w| w.hidden_batches());
        let hidden: Vec<bool> = geometry
            .brush_model_batches()
            .iter()
            .enumerate()
            .map(|(i, &brush)| brush || water_hidden.get(i).copied().unwrap_or(false))
            .collect();
        let (positions, triangles) = world_triangles(&geometry, &hidden);
        ragdolls.physics.create_bsp_collision(&positions, &triangles);
        println!("ragdolls: {} collision triangles", triangles.len());
        ragdolls
    });
    let mut teleport_renderer = TeleportRenderer::new(
        gpu,
        format,
        &mut models,
        remote_players.as_ref().map(|r: &RemotePlayerRenderer| &*r.model),
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
    let player_model = remote_players.as_ref().map(|r: &RemotePlayerRenderer| &*r.model);
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
        WeaponViewModel::load(gpu, &mut map.pk3, id, output_format)
            .inspect(|_| println!("view model: {id}"))
            .inspect_err(|why| eprintln!("view model: {why}"))
            .ok()
    });
    let mut holster = HashMap::new();
    if map.hands {
        for id in HELD_WEAPONS {
            match WeaponViewModel::load(gpu, &mut map.pk3, id, output_format) {
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
                    anchor: None,
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
    let damage_font =
        shagger::ui::font::Font::from_bytes(include_bytes!("../../assets/fonts/Rajdhani-Bold.ttf").to_vec())
            .expect("the client ships Rajdhani");
    let cartoon_hit_renderer = CartoonHitRenderer::load(gpu, format, &mut map.pk3);
    let fog_volumes = FogVolumes::new(
        gpu,
        format,
        renderer.scene_depth_layout(),
        fog_volumes_from(&map.bsp, &map.shaders),
    );
    if !fog_volumes.is_empty() {
        println!("fog volumes: {}", fog_volumes.len());
    }
    let map_particles = MapParticles::load(gpu, format, &map.bsp.entities, &map.shaders, &mut map.pk3, 0.0);
    if map_particles.emitter_count() > 0 {
        println!("misc_particles: {} emitters", map_particles.emitter_count());
    }
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
        item_models,
        weapon_pickup_spans,
        weapon_pickup_spots,
        item_pvs,
        pickup_active: Vec::new(),
        item_time: 0.0,
        dropped_weapons: DroppedWeaponRenderer::default(),
        frame_draws: Vec::new(),
        static_joints: joints.len(),
        joints,
        remote_players,
        live_draws: Vec::new(),
        live_actors: Vec::new(),
        static_actors,
        actor_draws: Vec::new(),
        frame_lights: Vec::new(),
        frame_actors: Vec::new(),
        live_projectile_models,
        live_projectile_lights: HashMap::new(),
        projectile_seen: HashMap::new(),
        projectile_offsets: HashMap::new(),
        live_trails: HashMap::new(),
        spare_trails: Vec::new(),
        live_light_clock: 0.0,
        effect_time: 0.0,
        live_flashes: Vec::new(),
        spawned_explosions: Vec::new(),
        charging: Vec::new(),
        charge_glows: Vec::new(),
        sky,
        fog,
        fog_serial: None,
        water,
        water_time: 0.0,
        liquid,
        liquid_clock: 0.0,
        splash: SplashDropletRenderer::new(gpu, format),
        rings: LiquidRingRenderer::new(gpu, format),
        held_rings: Vec::new(),
        teleports,
        teleport_renderer,
        map_particles,
        fog_volumes,
        teleport_last: HashMap::new(),
        gibs: GibSystem::default(),
        gib_renderer,
        spatter: HitSpatter::default(),
        spatter_renderer,
        brush_models,
        lifts: LiftSystem::new(extract_lifts(&map.bsp.entities, &map.bsp.models, None)),
        applied_disabled: None,
        bomb_sites: BombSiteMarkers::default(),
        bomb_debris: BombSiteDebris::default(),
        debris_models,
        bomb,
        slide_dust: SlideDust::default(),
        slide_dust_renderer,
        bomb_anchor: None,
        bomb_carrier: None,
        blast_sounds: 0,
        live_styles: HashMap::new(),
        local_style: None,
        ragdolls,
        ragdoll_trails: RagdollTrails::default(),
        slash_trails: SlashTrailRenderer::default(),
        hand_weapons,
        showcase: None,
        live_weapons: Arc::default(),
        live_models: HashMap::new(),
        ragdolled: HashSet::new(),
        departed: HashMap::new(),
        live_worlds: HashMap::new(),
        local_player: None,
        followed_player: None,
        remote_muzzles: HashMap::new(),
        local_beam: false,
        blade_tip_flashes: Vec::new(),
        next_flash_anchor: 0,
        frame_height: 720.0,
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
        switch_anim: WeaponSwitchAnim::default(),
        switch_clock: 0.0,
        light_grid: map.bsp.light_grid,
        light_vols: map.bsp.light_vols.clone(),
    };
    // Held liquid rings (`--ring`).
    renderer.held_rings = map
        .rings
        .iter()
        .map(|r| Ring {
            position: babylon(r.origin),
            kind: r.kind,
            alpha: r.alpha,
            scale: r.radius,
            age: 0.0,
            max_radius: r.radius,
        })
        .collect();
    // Held gibs (`--gib`): broken up, then flown on to their age.
    for held in &map.gibs {
        renderer.gibs.spawn(babylon(held.origin), DVec3::ZERO, None);
    }
    if let Some(age) = map.gibs.iter().map(|g| g.age).reduce(f64::min) {
        for _ in 0..(age * 60.0).round() as usize {
            renderer.play_gibs(&[], 1.0 / 60.0);
        }
    }
    // Held hit spatter (`--spatter`): a hard body hit from the west, flown
    // on to its age.
    for held in &map.spatters {
        let hit = Hit {
            origin: babylon(held.origin),
            zone: HitZone::Body,
            damage: 60.0,
            away: Some((1.0, 0.0)),
            robot: held.robot,
            scale: 1.0,
        };
        renderer.spawn_spatter(&hit, true);
    }
    if let Some(age) = map.spatters.iter().map(|g| g.age).reduce(f64::min) {
        for _ in 0..(age * 60.0).round() as usize {
            renderer.spatter.update(1.0 / 60.0);
        }
    }
    // Held ragdolls (`--ragdoll`): an idle body launched, then fallen on to
    // its age; one that has faded by then has burst into gibs.
    if let (Some(ragdolls), Some(players)) = (&mut renderer.ragdolls, &mut renderer.remote_players) {
        let style = get_player_model_visual_style(PlayerTeamSlot::Ffa, MODEL_SCALE);
        for (i, held) in map.ragdolls.iter().enumerate() {
            let id = format!("ragdoll{i}");
            let interp = held.player.interpolated();
            players.update(
                &id,
                &interp,
                0.0,
                1.0,
                Some(&held.player.clip),
                held.player.body.as_deref(),
            );
            players.go_to_frame(&id, held.player.frame);
            let world = players.world_of(&id, &interp);
            if let (Some(DetachedBody { body, instance }), Some(world)) = (players.take(&id), world) {
                ragdolls.spawn(body, instance, world, babylon(held.dir), Some(&style), None);
            }
        }
        if let Some(age) = map.ragdolls.iter().map(|r| r.age).reduce(f64::min) {
            for _ in 0..(age * 60.0).round() as usize {
                ragdolls.update(1.0 / 60.0);
                for (at, color) in ragdolls.take_gibs() {
                    renderer.gibs.spawn(at, DVec3::ZERO, color);
                }
            }
        }
    }
    renderer
}

/// The players a demo or a match shows, each styled as the viewer sees
/// them (`visualStyle`), and the body each is drawn with
/// (`modelChoiceFor`), for [`MapRenderer::live_models`].
#[must_use]
pub fn styled_players(
    players: Vec<(String, InterpolatedEntity)>,
    appearance: &mut PlayerAppearance,
    view: &AppearanceView<'_>,
) -> StyledPlayers {
    let models = players
        .iter()
        .map(|(id, _)| (id.clone(), (view.model_for(id), view.scale_for(id))))
        .collect();
    let players = players
        .into_iter()
        .map(|(id, p)| {
            let style = appearance.style(&id, view, MODEL_SCALE);
            (id, p, style)
        })
        .collect();
    (players, models)
}

/// [`styled_players`]: the players with their styles, and their bodies.
pub type StyledPlayers = (
    Vec<(String, InterpolatedEntity, PlayerModelVisualStyle)>,
    HashMap<String, (&'static str, f64)>,
);
