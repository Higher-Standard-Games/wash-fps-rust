//! Opening a view, a demo or a match in its window: `washoi view`,
//! `washoi demo` and play. Split out of `view.rs`; the same module tree.

use super::*;

/// The view, demo or match until its window closes; the exit code.
pub fn run(o: &ViewOptions) -> i32 {
    start(o).unwrap_or_else(|why| {
        eprintln!("{why}");
        1
    })
}

/// The same, with why it could not start (a refused join, a missing map)
/// returned rather than printed, for the menu to show.
///
/// # Errors
/// Why the view could not start.
pub fn start(o: &ViewOptions) -> Result<i32, String> {
    let prepared = prepare(o, &mut |_| {})?;
    start_prepared(o, prepared)
}

/// What a match, a demo or a map needs before its window opens: the feed
/// connected or read, the map loaded, and the sound decoded.
pub struct Prepared {
    pub feed: Option<Feed>,
    pub map: LoadedMap,
    pub map_name: String,
    /// For the window: the match's sound (none for a screenshot).
    pub audio: Option<GameAudio>,
}

/// How far [`prepare`] has got, for the loading screen.
#[derive(Debug, Clone, PartialEq)]
pub enum PrepareStep {
    /// The game server welcomed us; its map is next.
    Connected { map: String },
    /// A phase of the map's load, by `TransferLoadingView`'s phase names.
    Phase {
        phase: &'static str,
        detail: Option<String>,
    },
}

/// Connects to the match or reads the demo, then loads its map, reporting
/// each step: the part of [`start`] the loading screen runs on a thread of
/// its own (`MatchMapLoader.loadServerMap`).
///
/// # Errors
/// Why the view could not start.
pub fn prepare(o: &ViewOptions, report: &mut dyn FnMut(PrepareStep)) -> Result<Prepared, String> {
    let feed = if let Some(path) = &o.demo {
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
                return Err(why.to_string());
            }
        }
    } else if let Some((url, name, ticket)) = &o.watch
        && o.join
    {
        println!("connecting to {url} as {name}, to play");
        match LivePlay::connect(
            url,
            name,
            ticket.clone(),
            o.team.clone(),
            &o.data,
            std::time::Duration::from_secs(15),
        ) {
            Ok(p) => Some(Feed::Play(Box::new(p), PlayInput::default())),
            Err(why) => {
                return Err(why.to_string());
            }
        }
    } else if let Some((url, name, ticket)) = &o.watch {
        println!("connecting to {url} as {name}");
        match LiveWatch::connect(url, name, ticket.clone(), std::time::Duration::from_secs(15)) {
            Ok(l) => Some(Feed::Live(l)),
            Err(why) => {
                return Err(why.to_string());
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
    report(PrepareStep::Connected { map: map_name.clone() });
    let mut map = load_map(&o.data, &map_name, report)?;
    // Without a sound card the game still keeps its sounds, and plays
    // them once one appears (`AudioV2Sfx::maintain`).
    let audio = (o.screenshot.is_none() && feed.is_some()).then(|| {
        report(PrepareStep::Phase {
            phase: "Assets",
            detail: Some("Sounds".into()),
        });
        let mut audio = GameAudio::new(AudioV2Sfx::open_device_or_wait());
        println!(
            "audio: {} sounds, {} music tracks at {} Hz",
            audio.preload(&mut map.pk3),
            audio.load_music(&map.pk3, &o.data),
            audio.sfx.sample_rate()
        );
        audio
    });
    Ok(Prepared {
        feed,
        map,
        map_name,
        audio,
    })
}

/// The rest of [`start`], once [`prepare`] is done: the window, or the
/// screenshot.
///
/// # Errors
/// Why the view could not start.
pub fn start_prepared(o: &ViewOptions, prepared: Prepared) -> Result<i32, String> {
    let Prepared {
        mut feed,
        mut map,
        map_name,
        audio,
    } = prepared;
    map.players.clone_from(&o.players);
    map.projectiles.clone_from(&o.projectiles);
    map.explosions.clone_from(&o.explosions);
    map.effects.clone_from(&o.effects);
    map.hits.clone_from(&o.hits);
    map.teleports.clone_from(&o.teleports);
    map.gibs.clone_from(&o.gibs);
    map.spatters.clone_from(&o.spatters);
    map.rings.clone_from(&o.rings);
    map.ragdolls.clone_from(&o.ragdolls);
    map.weapon.clone_from(&o.weapon);
    map.remote_players = feed.is_some() || o.showcase.is_some();
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
    // The weapon in hand: our own in a match, and the one a demo or a
    // live match shows through a player's eyes.
    map.hands = o.join || feed.is_some();
    let at = o.at.map(|[x, y, z, angle]| Camera::from_q3([x, y, z], angle));
    let Some(mut camera) = at.or_else(|| map.spawns.get(o.spawn).or(map.spawns.first()).copied()) else {
        return Err(format!("{map_name} has no spawn point"));
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
        let bench = o
            .bench
            .as_ref()
            .map(|(frames, warmup, out)| crate::bench::Bench::new(*frames, *warmup, out.clone()));
        if bench.is_some() {
            crate::window::request_size(o.width, o.height);
        }
        return crate::window::run(map, camera, o.seconds, feed, o.follow, audio, reel, bench);
    };
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(why) => {
            return Err(why.to_string());
        }
    };
    println!("gpu: {}", gpu.describe());
    let mut renderer = build_renderer(&gpu, &mut map, OFFSCREEN_FORMAT);
    renderer.cull = !o.no_cull;
    renderer.dynamic_lights = !o.no_lights;
    // Shader animation as it stands `--time` seconds after the map loads.
    renderer.world.animate(&gpu, o.time);
    // The rotators as they have turned by then; the lifts at rest.
    renderer.world.move_movers(&gpu, o.time, |_| None, |_| None);
    renderer.item_time = o.time;
    // The map's particles as they have flowed by then, at 60 frames a second.
    for step in 0..=(o.time * 60.0).round() as usize {
        renderer.map_particles.advance(
            camera.position.as_dvec3(),
            (step as f64 * 1000.0 / 60.0).min(o.time * 1000.0),
        );
    }
    let mut hud: Option<FpsHud> = None;
    let mut overlay = crate::reel_overlay::ReelOverlay;
    // Whom a demo's transport bar says the view follows.
    let mut followed: Option<String> = None;
    if let Some(feed) = &mut feed {
        // Play up to `--time` at 60 frames a second (live: in real time).
        let steps = (o.time * 60.0).round() as usize;
        let mut players = Vec::new();
        let mut appearance = PlayerAppearance::default();
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
            if let Feed::Play(p, input) = feed {
                // No one to show the kit picker to: the remembered kit, or
                // the default, goes in so the join is not held up.
                if p.wants_loadout() {
                    let kit = loadout_picker::remembered(p.match_state().gametype_name())
                        .unwrap_or(loadout_picker::DEFAULT_LOADOUT);
                    p.choose_loadout(kit.primary, kit.secondary);
                }
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
            let before = match &*feed {
                Feed::Demo(d) => Some(d.playhead_tick()),
                _ => None,
            };
            let view = feed.advance(1.0 / 60.0);
            // In a demo the effects run on the demo clock, as the window's
            // do: still through a pause or a reel's freeze frame.
            let effect_dt = match (&*feed, before) {
                (Feed::Demo(d), Some(before)) => ((d.playhead_tick() - before) / d.tick_rate()).clamp(0.0, 0.25),
                _ => 1.0 / 60.0,
            };
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
                renderer.advance_switch(1.0 / 60.0);
                if p.alive
                    && let Some(text) = said.take()
                {
                    p.say(text, false);
                }
            }
            let models;
            (players, models) = styled_players(
                view.players,
                &mut appearance,
                &AppearanceView {
                    teams: &view.teams,
                    looks: &view.looks,
                    names: &view.names,
                    perspective: followed.as_deref(),
                    own: feed.local_entity(),
                    team_based: feed.match_state().team_based(),
                },
            );
            renderer.live_models = models;
            projectiles = view.projectiles;
            let eyes = reel
                .as_ref()
                .and_then(|r| apply_reel_camera(&mut camera, r, &players, base_fov));
            renderer.set_live_weapons(&view.weapons);
            let hidden: Vec<&str> = eyes.as_deref().into_iter().collect();
            renderer.set_live_players(&players, &hidden, 1.0 / 60.0, effect_dt);
            renderer.set_live_projectiles(&projectiles, effect_dt, &camera);
            renderer.play_effects(&view.effects, &view.weapons, effect_dt, &camera);
            renderer.dropped_weapons.use_server_transforms = matches!(feed, Feed::Demo(_));
            renderer.play_dropped_weapons(&view.effects, &view.dropped_weapons, effect_dt);
            renderer.play_bomb_sites(feed.match_state(), effect_dt);
            renderer.play_pickups(&view.pickup_active, effect_dt);
            renderer.play_gibs(&view.effects, effect_dt);
            {
                let local = match &*feed {
                    Feed::Play(p, _) => p.feet().map(|f| (p.entity_id.as_str(), DVec3::new(f.x, f.y, f.z))),
                    _ => None,
                };
                renderer.play_spatter(
                    &view.effects,
                    view_entity(feed.local_entity(), &players, o.follow),
                    &players,
                    local,
                    effect_dt,
                );
            }
            renderer.play_hit_feedback(
                &view.effects,
                view_entity(feed.local_entity(), &players, o.follow),
                1.0 / 60.0,
            );
            if let Some(hud) = &mut hud {
                hud.scoreboard.held = o.scoreboard;
                hud.set_markers(
                    renderer.bomb_site_poses(),
                    &camera,
                    o.width as f32 / o.height.max(1) as f32,
                );
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
                        looks: &view.looks,
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
                    followed = Some(id.clone());
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
    let light = renderer.light_at(camera.position);
    if let Some(view_model) = &mut renderer.view_model {
        view_model.powerup_outline = o.quad.then(|| quad_view_weapon_outline(0.0));
        view_model.view.aspect_ratio = f64::from(o.width) / f64::from(o.height.max(1));
        view_model
            .view
            .apply_cvars(&shagger::core::cvar::with_cvars(|c| ViewModelCvars::read(c)));
        // Settle the aim, the light (and the bob, standing still) over two seconds.
        for _ in 0..120 {
            view_model.view.update_lighting(1.0 / 60.0, light.as_ref());
            view_model.view.update(&ViewModelInput {
                delta_time: 1.0 / 60.0,
                grounded: true,
                aiming: o.aim,
                ..ViewModelInput::default()
            });
        }
    }
    // `--fire AGE`: our gun fires once along the view, AGE seconds ago.
    if let (Some(age), Some(weapon)) = (o.fire, renderer.view_model.as_ref().map(|v| v.view.weapon.clone())) {
        fire_view_shot(&mut renderer, &camera, &weapon, age, o.aim);
    }
    if let Some(weapon) = &o.showcase {
        use washoi::client::player_model_visual_settings::{PlayerTeamSlot, get_player_model_visual_style};
        let style = get_player_model_visual_style(PlayerTeamSlot::Enemy, MODEL_SCALE);
        let weapon = (weapon != "none").then(|| weapon.clone());
        let aspect = f64::from(o.width) / f64::from(o.height.max(1));
        renderer.start_showcase("police", style, weapon, &camera, aspect);
        if let Some(showcase) = &mut renderer.showcase {
            for _ in 0..90 {
                showcase.advance(1.0 / 60.0);
            }
            showcase.frame(&mut camera);
        }
        renderer.play_showcase(0.0);
    }
    // One long step: a still is fully under or fully dry.
    renderer.update_underwater(&mut camera, 1.0);
    renderer.prepare(&gpu, &camera, [o.width as f32, o.height as f32]);
    if let Some(water) = &renderer.water {
        let reflection = water.actor_reflection();
        println!(
            "water: actor reflection {}, {} reflected",
            reflection.status, reflection.actor_count
        );
    }
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
        antialias_on(),
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
    match (&reel, &feed) {
        (Some(reel), Some(Feed::Demo(demo))) => {
            composite_ui(&gpu, &mut pixels, o.width, o.height, |ctx| overlay.ui(ctx, reel, demo));
        }
        // A whole demo: its transport bar, as it stands on a pause.
        (None, Some(Feed::Demo(demo))) => {
            let mut bar = crate::demo_overlay::DemoOverlay::new(demo);
            // An ended demo shows its end card with the fade done.
            bar.present_ends(demo, || crate::window::demo_final(hud.as_ref().map(|h| &h.state)));
            bar.update(1.0, true);
            let name = followed.as_deref().map(|id| {
                demo.roster()
                    .iter()
                    .find(|(_, p)| p.entity_id == id)
                    .map_or(id, |(_, p)| p.name.as_str())
            });
            let who = followed.as_deref().zip(name);
            composite_ui(&gpu, &mut pixels, o.width, o.height, |ctx| {
                bar.ends_ui(ctx, demo);
                bar.ui(ctx, demo, who);
            });
        }
        _ => {}
    }
    if let Some(weapon) = &o.loadout {
        let picker = std::cell::RefCell::new(loadout_picker::LoadoutPicker::default());
        picker.borrow_mut().open("Free For All", None, Some("respawn"), false);
        if !weapon.is_empty() {
            picker.borrow_mut().focus(weapon);
        }
        composite_ui(&gpu, &mut pixels, o.width, o.height, |ctx| {
            picker.borrow_mut().ui(ctx);
        });
    }
    match image::save_buffer(path, &pixels, o.width, o.height, image::ExtendedColorType::Rgba8) {
        Ok(()) => {
            println!("wrote {}", path.display());
            Ok(0)
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
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
        let tag = washoi::client::cinematic_demo_player::scene_tag(&s.highlight);
        let freeze = s
            .freeze
            .as_ref()
            .map_or(String::new(), |f| format!(", freeze {}", f.caption));
        println!(
            "  {:>2}. ticks {}-{} {:<16} {:<6} {} by {} ({:.1} s, {} in, {} out{freeze})",
            s.segment_index + 1,
            s.start_tick,
            s.end_tick,
            s.camera_preset.as_str(),
            s.role.as_str(),
            tag.what,
            tag.who,
            s.duration_ms / 1000.0,
            s.transition_in.as_str(),
            s.transition_out.as_str(),
        );
    }
}

/// Draws an egui overlay (the reel's cards, a demo's bar) offscreen and
/// lays it over `pixels`.
fn composite_ui(gpu: &Gpu, pixels: &mut [u8], width: u32, height: u32, draw: impl Fn(&egui::Context)) {
    use shagger::ui::egui_layer::EguiLayer;
    let mut layer = EguiLayer::offscreen(gpu, OFFSCREEN_FORMAT);
    washoi::client::washoi_style::install_style(&layer.ctx);
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
        let frame = layer.run_input(input(), |ui| draw(ui.ctx()));
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
pub(super) fn held_trail(position: Vec3, direction: Vec3, speed: f64, max_length: Option<f64>) -> ProjectileTrail {
    let (head, dir) = (position.as_dvec3(), direction.as_dvec3());
    let step = speed / 60.0;
    let mut trail = ProjectileTrail::default();
    trail.points = (0..MAX_TRAIL_POINTS)
        .map(|k| head - dir * (step * (MAX_TRAIL_POINTS - k) as f64))
        .collect();
    trail.push(head, dir, max_length);
    trail
}
