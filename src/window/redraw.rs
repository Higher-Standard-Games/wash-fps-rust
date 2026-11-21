//! One frame of the window: the match, the demo or the view drawn, with
//! the HUD, the menus and the console over it. Split out of `window.rs`; the same module tree.

use super::*;

impl App {
    pub(super) fn redraw(&mut self) {
        let now = Instant::now();
        // The bench steps the demo 60 times a second whatever the frame
        // took, so every run draws the same frames.
        let dt = if self.bench.is_some() {
            1.0 / 60.0
        } else {
            now.duration_since(self.last).as_secs_f32().min(0.1)
        };
        self.last = now;
        if let Some(b) = &mut self.bench {
            b.begin_frame();
        }
        self.poll_pads(dt);
        with_input(InputSystem::update);
        // `FPSController`'s `hipfireBlocked`: the trigger on a launcher puts
        // the aim up, its look and feel too (`keys.aim = true`), not only
        // the server's ALT_FIRE.
        let launcher = self.hipfire_blocked_in_hand();
        self.aiming = self.captured && with_input(|i| i.is_active(fps::AIM) || (launcher && i.is_active(fps::FIRE)));
        self.step(dt);
        with_input(InputSystem::end_frame);
        // `cl_fov`, as `currentBaseFov` gives every camera.
        self.camera.fov_y = with_cvars(|c| current_base_fov(c)) as f32;
        let Some(s) = &mut self.surface else { return };
        // `r_frame_latency` takes effect at once (the bench keeps its own).
        let latency = frame_latency();
        if self.bench.is_none() && s.config.desired_maximum_frame_latency != latency {
            s.config.desired_maximum_frame_latency = latency;
            s.surface.configure(&s.gpu.device, &s.config);
        }
        s.renderer.world.animate(&s.gpu, f64::from(dt));
        // The rotators turn and the lifts stand where the lift state says.
        let feed = self.feed.as_ref();
        if let Some(f) = feed {
            s.renderer.lifts.restore(&f.lift_states());
        }
        let lifts = &s.renderer.lifts;
        s.renderer.world.move_movers(
            &s.gpu,
            f64::from(dt),
            |model| feed.and_then(|f| f.lift_travel(model)),
            |model| {
                let o = lifts.button_offset(model);
                (o.x != 0.0 || o.y != 0.0 || o.z != 0.0).then_some([o.x, o.y, o.z])
            },
        );
        s.renderer.item_time += f64::from(dt);
        if let Some(feed) = &mut self.feed {
            if let (Some(reel), Feed::Demo(demo)) = (&mut self.reel, &mut *feed) {
                reel.update(
                    f64::from(dt) * 1000.0,
                    demo,
                    &mut SceneReelWorld {
                        shots: s.renderer.shot_world_mut(),
                        players: &self.drawn,
                    },
                );
                self.overlay.update(reel, f64::from(dt));
            }
            if matches!(feed, Feed::Demo(_)) && self.console.ctx.demo.is_none() {
                self.console.ctx.demo = Some(DemoConsole { hud_visible: true });
            }
            if let (None, Feed::Demo(demo)) = (&self.reel, &*feed) {
                let hud = self.demo_hud.get_or_insert_with(|| DemoOverlay::new(demo));
                hud.update(f64::from(dt), demo.paused());
            }
            let playhead = match &*feed {
                Feed::Demo(d) => Some(d.playhead_tick()),
                _ => None,
            };
            let view = feed.advance(f64::from(dt));
            if let Some(b) = &mut self.bench {
                b.mark("feed");
            }
            // `_syncDemoPresentationClock`: in a demo the effects and the
            // HUD's timed entries run on the demo clock (still while paused,
            // slow at a quarter speed), and a seek drops them.
            let mut effect_dt = f64::from(dt);
            let mut seeked = false;
            if let (Feed::Demo(d), Some(before)) = (&*feed, playhead) {
                effect_dt = ((d.playhead_tick() - before) / d.tick_rate()).clamp(0.0, 0.25);
                if d.discontinuity() != self.demo_discontinuity {
                    seeked = true;
                    self.demo_discontinuity = d.discontinuity();
                    effect_dt = 0.0;
                    s.renderer.discontinuity();
                    if let Some(audio) = &mut self.audio {
                        audio.discontinuity();
                    }
                    if let Some(hud) = &mut s.hud {
                        hud.state.kill_feed.clear();
                        hud.state.powerups.clear();
                        hud.chat.clear_messages();
                    }
                }
            }
            self.view_offset = None;
            self.on_ladder = false;
            if let Feed::Play(p, held) = feed {
                // First person: the eye where prediction puts it.
                if let Some(yaw) = p.take_spawn_facing() {
                    // `faceSpawn`: the spawn's facing, level.
                    self.camera.yaw = yaw as f32;
                    self.camera.pitch = 0.0;
                }
                if let Some(t) = p.take_teleport() {
                    // `setViewYaw(exitYaw)`, then `localTeleported`.
                    self.camera.yaw = t.exit_yaw as f32;
                    s.renderer.teleports.local_teleported(
                        DVec3::new(t.from.x, t.from.y, t.from.z),
                        DVec3::new(t.to.x, t.to.y, t.to.z),
                        t.entry_yaw,
                        t.exit_yaw,
                        TeamColor::Alpha,
                    );
                }
                if let Some(eye) = p.eye() {
                    self.camera.position = Vec3::new(eye.x as f32, eye.y as f32, eye.z as f32);
                }
                // `FPSController.update`: steps, slide, ledge and the view
                // effects, drawn but never aimed with.
                match p.feet() {
                    Some(feet) => {
                        let m = p.movement();
                        let v = p.velocity();
                        let body = BodyFrame {
                            feet: DVec3::new(feet.x, feet.y, feet.z),
                            eye_lift: shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT
                                + m.crouch_view_offset,
                            velocity: DVec3::new(v.x, v.y, v.z),
                            grounded: p.grounded(),
                            sliding: m.is_sliding,
                            horizontal_speed: m.actual_horiz_speed,
                            slide_phase: m.slide_phase,
                            ledge_grab_phase: m.ledge_grab_phase,
                            ledge_grab: m.ledge_grab_state,
                            wall_jump: m.wall_jump_state,
                        };
                        let keys = ViewKeys {
                            forward: held.forward,
                            backward: held.backward,
                            left: held.left,
                            right: held.right,
                            jump: held.jump,
                            sprint: held.sprint,
                            aim: self.aiming,
                            fire: held.fire,
                        };
                        let (cvars, step_height, smooth_factor) = view_cvars();
                        self.view_offset = Some(self.first_person.update(
                            f64::from(dt),
                            &body,
                            &keys,
                            &cvars,
                            step_height,
                            smooth_factor,
                        ));
                        self.on_ladder = m.on_ladder;
                        // `FPSHintPanel.moved`: the coach sees each step.
                        if let Some(hud) = &mut s.hud {
                            hud.hints.moved(
                                &MovementSample {
                                    grounded: body.grounded,
                                    sliding: body.sliding,
                                    horizontal_speed: js_hypot(&[v.x, v.z]),
                                    vertical_speed: v.y,
                                    wall_jump: body.wall_jump,
                                    ledge_grab: body.ledge_grab,
                                    forward: held.forward,
                                    strafing: held.left || held.right,
                                },
                                &hud.state.current_weapon,
                            );
                        }
                        // `Game`'s own trail, from where we are drawn.
                        s.renderer.slide_dust.emit(
                            "local",
                            body.feet,
                            body.velocity.x,
                            body.velocity.z,
                            p.alive && p.grounded() && m.is_sliding && m.water_level == 0 && !m.on_ladder,
                        );
                    }
                    None => self.first_person.reset(),
                }
                let v = p.velocity();
                self.velocity = Vec3::new(v.x as f32, v.y as f32, v.z as f32);
                self.grounded = p.grounded();
                self.follow = None;
                // `switchWeapon` slides a picked weapon in at once; the
                // server's weapon is put in hand when its `weapon_state`
                // arrives and no switch is under way (`handleWeaponState`),
                // which heals a pick the server did not take.
                let arrived = p.take_weapon_state();
                let predicted = self.predicted_weapon.take();
                // A teammate's eyes put their weapon in hand.
                match p.weapon_in_hand() {
                    _ if self.spectate.watching().is_some() => {}
                    None => s.renderer.hold(None),
                    Some(_) if predicted.is_some() => s.renderer.hold(predicted.as_deref()),
                    Some(server) => {
                        if s.renderer.view_model.is_none() || (arrived && !s.renderer.switch_anim.is_switching()) {
                            s.renderer.hold(Some(server));
                        }
                    }
                }
                if let Some(at) = p.take_jumppad_launch()
                    && let Some(audio) = &mut self.audio
                {
                    play_jumppad_launch(&mut audio.sfx, DVec3::new(at.x, at.y, at.z), true);
                }
                // `LocalLiquid.emitLocalCrossing`: our own splash and air.
                let local = p.take_local_liquid(f64::from(dt));
                if local.respawned {
                    s.renderer.liquid.forget("local");
                }
                let at = DVec3::new(local.position.x, local.position.y, local.position.z);
                s.renderer
                    .notify_liquid("local", local.level, at, local.vertical_speed, true);
                if let (Some(event), Some(audio)) = (local.air, &mut self.audio) {
                    match event {
                        AirEvent::StartedDrowning => play_drown_gasp(&mut audio.sfx),
                        AirEvent::DrownTick => play_drown_tick(&mut audio.sfx),
                    }
                }
            }
            let appearance_view = AppearanceView {
                teams: &view.teams,
                looks: &view.looks,
                names: &view.names,
                perspective: self.pov_entity.as_deref(),
                own: feed.local_entity(),
                team_based: feed.match_state().team_based(),
            };
            let (players, models) = styled_players(view.players, &mut self.appearance, &appearance_view);
            // Our own body is never drawn; its corpse wears our style.
            let local_style = match feed {
                Feed::Play(..) => feed.local_entity().map(|id| {
                    self.appearance.style(
                        id,
                        &appearance_view,
                        washoi::client::remote_player_renderer::MODEL_SCALE,
                    )
                }),
                _ => None,
            };
            s.renderer.set_local_style(local_style);
            s.renderer.live_models = models;
            // `watchMatchPhase`: the winner on show through the post-game,
            // gone with the next phase.
            let phase = feed.match_state().phase();
            if self.showcase_phase != Some(phase) {
                self.showcase_phase = Some(phase);
                if s.renderer.showcase.take().is_some()
                    && let Some((yaw, pitch)) = self.showcase_look.take()
                {
                    (self.camera.yaw, self.camera.pitch) = (yaw, pitch);
                }
                let winner = (phase == MatchPhase::PostGame)
                    .then(|| feed.match_state().resolve_winner_player_id())
                    .flatten()
                    .and_then(|id| {
                        let key = format!("{CLIENT_KEY_PREFIX}{id}");
                        feed.roster()
                            .iter()
                            .find(|(k, _)| *k == key)
                            .map(|(_, p)| p.entity_id.clone())
                    });
                if let Some(entity) = winner {
                    // Our own gun is the one on screen; anyone else's the
                    // last seen in their hand.
                    let weapon = if feed.local_entity() == Some(entity.as_str()) {
                        s.renderer.view_model.as_ref().map(|v| v.view.weapon.clone())
                    } else {
                        None
                    }
                    .or_else(|| view.weapons.get(&entity).cloned());
                    let style = self.appearance.style(
                        &entity,
                        &appearance_view,
                        washoi::client::remote_player_renderer::MODEL_SCALE,
                    );
                    let aspect = f64::from(s.config.width) / f64::from(s.config.height.max(1));
                    s.renderer
                        .start_showcase(appearance_view.model_for(&entity), style, weapon, &self.camera, aspect);
                    self.showcase_look = Some((self.camera.yaw, self.camera.pitch));
                }
            }
            let base_fov = self.camera.fov_y;
            let eyes = self.reel.as_ref().and_then(|r| {
                self.follow = None;
                apply_reel_camera(&mut self.camera, r, &players, base_fov)
            });
            // Our own camera in play; a frame without players keeps
            // `--follow N` for the first frame that has them.
            if self.reel.is_none() && !matches!(feed, Feed::Play(..)) && !players.is_empty() {
                // `--follow N` names a place in the first frame's list.
                if let (Some(i), None) = (self.follow, &self.follow_id) {
                    self.follow_id = players.get(i).map(|(id, _, _)| id.clone());
                }
                // A demo watches someone until the viewer flies free, and
                // someone else when the one watched leaves.
                let gone = self
                    .follow_id
                    .as_ref()
                    .is_some_and(|id| !players.iter().any(|(p, _, _)| p == id));
                if (self.follow_id.is_none() || gone) && !self.free_camera && matches!(feed, Feed::Demo(_)) {
                    self.follow_id = name_order(feed.roster(), &players).into_iter().next();
                }
                self.follow = self
                    .follow_id
                    .as_ref()
                    .and_then(|id| players.iter().position(|(p, _, _)| p == id));
            }
            // `SpectateView`: dead in a round, a teammate's eyes after the
            // corpse, and what decided the round at its end.
            if let Feed::Play(p, _) = &*feed {
                self.spectate.set_roster_teams(std::sync::Arc::clone(&view.teams));
                let cue = self.spectate.update(
                    f64::from(dt),
                    !p.alive,
                    &p.entity_id,
                    feed.match_state(),
                    players.iter().map(|(id, _, _)| id.as_str()),
                );
                match cue {
                    DeathCameraCue::Keep => {}
                    DeathCameraCue::Focus(at) => {
                        // From behind wherever we were looking.
                        self.death_camera.start(at, f64::from(self.camera.yaw));
                        self.death_follow = None;
                        self.death_focus = Some(at);
                    }
                    DeathCameraCue::Stop => {
                        self.death_camera.stop();
                        self.death_focus = None;
                    }
                    DeathCameraCue::Resume => {
                        let at = self.camera.position.as_dvec3()
                            - DVec3::Y * shagger::physics::player_physics_constants::DEFAULT_VIEWHEIGHT;
                        self.death_camera.start(at, f64::from(self.camera.yaw));
                        self.death_follow = None;
                        self.death_focus = None;
                    }
                }
                self.follow = self
                    .spectate
                    .watching()
                    .and_then(|id| players.iter().position(|(p, _, _)| p == id));
            }
            // `FollowedViewWeapon` and `SpectateEyeCamera`: the watched
            // player's eyes, their screen's view effects, and their weapon
            // in hand, swaying with their stride.
            self.followed_aiming = None;
            // `FollowedViewWeapon`: the watched player's gun glows while they
            // carry the quad, as the player's own does, in a demo or
            // spectating a live match.
            let mut followed_quad = false;
            let followed = self
                .follow
                .and_then(|i| players.get(i))
                .map(|(id, p, _)| (id.clone(), *p));
            match followed {
                Some((id, p)) => {
                    let cut = self.pov_entity.as_deref() != Some(id.as_str());
                    if cut {
                        self.pov.reset();
                        self.pov_entity = Some(id.clone());
                        self.followed_changed = true;
                    }
                    followed_quad = mask_has_quad(p.powerup_mask.unwrap_or(0));
                    let aiming = p.aiming.unwrap_or(false);
                    self.pov.set_aiming(aiming);
                    self.followed_aiming = Some(aiming);
                    let (cvars, step_height, smooth_factor) = view_cvars();
                    let pose = self
                        .pov
                        .update(&id, &p, effect_dt, seeked, &cvars, step_height, smooth_factor);
                    let eye = watched_eye_pose(&p, Some(&pose));
                    self.camera.position = eye.position.as_vec3();
                    self.camera.yaw = eye.yaw as f32;
                    self.camera.pitch = eye.pitch as f32;
                    self.camera.roll = 0.0;
                    let r = pose.effects.rotation_offset * (std::f64::consts::PI / 180.0);
                    self.view_offset = Some(ViewOffset {
                        eye: eye.position,
                        pitch: r.x,
                        yaw: r.y,
                        roll: r.z,
                        fov_multiplier: pose.effects.fov_multiplier,
                        local_offset: pose.effects.position_offset,
                        horizontal_speed: pose.horizontal_speed,
                        bob_cycle: pose.bob_cycle,
                        landing_velocity: pose.landing_velocity,
                    });
                    self.velocity = Vec3::new(p.vx as f32, p.vy as f32, p.vz as f32);
                    self.grounded = p.grounded;
                    // `showWeapon`: the first weapon shown is a cut, later
                    // ones switch.
                    if let Some(weapon) = view.weapons.get(&id) {
                        let weapon = canonical_weapon_id(weapon);
                        if cut {
                            s.renderer.hold_cut(Some(weapon));
                        } else {
                            s.renderer.hold(Some(weapon));
                        }
                    }
                }
                None => {
                    if self.pov_entity.take().is_some() {
                        self.followed_changed = true;
                        s.renderer.hold_cut(None);
                    }
                }
            }
            // Their own model would fill the screen from inside their head.
            let hidden: Vec<&str> = eyes.as_deref().into_iter().chain(self.pov_entity.as_deref()).collect();
            s.renderer.set_live_weapons(&view.weapons);
            s.renderer.set_live_players(&players, &hidden, f64::from(dt), effect_dt);
            s.renderer.play_showcase(f64::from(dt));
            let local = match feed {
                Feed::Play(..) => feed.local_entity(),
                _ => None,
            };
            s.renderer.set_local_player(local, f64::from(self.camera.yaw));
            s.renderer.set_followed_player(self.pov_entity.as_deref());
            s.renderer
                .set_live_projectiles(&view.projectiles, effect_dt, &self.camera);
            s.renderer
                .play_effects(&view.effects, &view.weapons, effect_dt, &self.camera);
            s.renderer.dropped_weapons.use_server_transforms = matches!(feed, Feed::Demo(_));
            s.renderer
                .play_dropped_weapons(&view.effects, &view.dropped_weapons, effect_dt);
            s.renderer.play_bomb_sites(feed.match_state(), effect_dt);
            // `triggerCameraShake`: explosions near our own body.
            let spawned = std::mem::take(&mut s.renderer.spawned_explosions);
            if let Feed::Play(p, _) = &*feed
                && let Some(body) = p.feet()
            {
                let screenshake = with_cvars(|c| c.get_bool("cg_screenshake"));
                let body = DVec3::new(body.x, body.y, body.z);
                for (at, look) in spawned {
                    self.first_person.explosion_at(look, at, body, screenshake);
                }
            }
            s.renderer.play_pickups(&view.pickup_active, effect_dt);
            s.renderer.play_gibs(&view.effects, effect_dt);
            {
                let local = match &*feed {
                    Feed::Play(p, _) => p.feet().map(|f| (p.entity_id.as_str(), DVec3::new(f.x, f.y, f.z))),
                    _ => None,
                };
                s.renderer.play_spatter(
                    &view.effects,
                    view_entity(
                        feed.local_entity().filter(|_| self.spectate.watching().is_none()),
                        &players,
                        self.follow,
                    ),
                    &players,
                    local,
                    effect_dt,
                );
            }
            // Our own death: the camera follows the corpse or the burst
            // until we are back (`applyDeathCamera`).
            if let Some((follow, at, yaw)) = s.renderer.take_local_death() {
                self.death_camera.start(at, yaw);
                self.death_follow = Some(follow);
            }
            if let Feed::Play(p, _) = &*feed
                && p.alive
            {
                self.death_camera.stop();
            }
            if self.death_camera.is_active() {
                let target = self
                    .death_focus
                    .or_else(|| self.death_follow.and_then(|f| s.renderer.follow_position(f)));
                let renderer = &mut s.renderer;
                let mut trace = |from, to| renderer.trace_map(from, to);
                if let Some((at, look)) = self.death_camera.update(f64::from(dt), target, Some(&mut trace)) {
                    let (yaw, pitch) = look_angles(at, look);
                    self.camera.position = at.as_vec3();
                    self.camera.yaw = yaw as f32;
                    self.camera.pitch = pitch as f32;
                }
            }
            // The diorama camera frames the post-game's winner.
            if let Some(showcase) = &s.renderer.showcase {
                showcase.frame(&mut self.camera);
            }
            // `presentFire` and the reload, for the eyes we look through:
            // the server's shots and weapon state (wash-fps `FPSPlayer`).
            if let Some(viewed) = view_entity(
                feed.local_entity().filter(|_| self.spectate.watching().is_none()),
                &players,
                self.follow,
            ) {
                let viewed = viewed.to_owned();
                for event in &view.effects {
                    match event {
                        SnapshotEvent::WeaponFire {
                            entity_id, weapon_id, ..
                        } if *entity_id == viewed => {
                            let projectile_fire = std::mem::take(&mut self.view_charging);
                            if let Some(view_model) = &mut s.renderer.view_model {
                                let style = shot_style(canonical_weapon_id(weapon_id));
                                view_model.view.present_fire(&style, projectile_fire);
                                // `kickLocalView`: only our own shots kick.
                                if feed.local_entity() == Some(entity_id.as_str()) {
                                    self.first_person
                                        .kick_for_shot(canonical_weapon_id(weapon_id), view_model.view.ads_progress);
                                    // `noteLocalShotForHints(canAimWith(weaponId))`.
                                    if let Some(hud) = &mut s.hud {
                                        hud.hints.shot(
                                            view_model.view.ads_progress < AIMING,
                                            can_aim_with(weapon_id),
                                            weapon_id,
                                        );
                                    }
                                }
                            }
                            s.renderer.fire_view_muzzle_flash(&self.camera, projectile_fire);
                        }
                        SnapshotEvent::WeaponCharge {
                            entity_id, charging, ..
                        } if *entity_id == viewed => {
                            self.view_charging = *charging;
                            // `noteLocalChargeForHints`.
                            if *charging
                                && feed.local_entity() == Some(entity_id.as_str())
                                && let Some(hud) = &mut s.hud
                            {
                                hud.hints.charged();
                            }
                        }
                        // The pain shake, our own only (`GameEvents`).
                        SnapshotEvent::Damage {
                            entity_id,
                            damage,
                            weapon_id,
                            ..
                        } if feed.local_entity() == Some(entity_id.as_str()) => {
                            // `noteLocalFallForHints`.
                            if matches!(weapon_id.as_str(), "fall" | "fall_crush")
                                && let Some(hud) = &mut s.hud
                            {
                                let weapon = hud.state.current_weapon.clone();
                                hud.hints.fell(&weapon);
                            }
                            let hitshake = with_cvars(|c| c.get_bool("cg_hitshake"));
                            self.first_person.hit(f64::from(*damage), weapon_id, hitshake);
                        }
                        SnapshotEvent::WeaponState {
                            entity_id,
                            current_weapon,
                            reloading,
                            ..
                        } if *entity_id == viewed => {
                            let weapon = canonical_weapon_id(current_weapon);
                            if let Some(view_model) = &mut s.renderer.view_model
                                && view_model.view.weapon == weapon
                            {
                                match (*reloading, view_model.view.reload_anim.is_reloading()) {
                                    (true, false) if !self.view_reloading => {
                                        if let Some(time) = clip_reload_time(weapon) {
                                            view_model.view.start_reload(time);
                                        }
                                    }
                                    (false, true) => view_model.view.cancel_reload(),
                                    _ => {}
                                }
                            }
                            self.view_reloading = *reloading;
                        }
                        _ => {}
                    }
                }
            }
            s.renderer.play_hit_feedback(
                &view.effects,
                view_entity(
                    feed.local_entity().filter(|_| self.spectate.watching().is_none()),
                    &players,
                    self.follow,
                ),
                effect_dt,
            );
            if self.audio.is_none() {
                s.renderer.drop_sounds();
            }
            if let Some(audio) = &mut self.audio {
                play_view_sounds(
                    audio,
                    &mut s.renderer,
                    &self.camera,
                    &DemoViewSounds {
                        effects: &view.effects,
                        teams: &view.teams,
                        looks: &view.looks,
                        snapshots: &view.snapshots,
                        match_state: feed.match_state(),
                        local: feed.local_entity(),
                    },
                    &players,
                    self.follow,
                    f64::from(dt),
                );
            }
            // `FPSPlayer`: our quad puts its outline on the gun.
            let quad = followed_quad
                || matches!(feed, Feed::Play(..))
                    && s.hud.as_mut().is_some_and(|h| {
                        let now = h.state.now;
                        h.state.powerups.is_active("quad", now)
                    });
            // `FPSPickup`'s and `DroppedWeaponRenderer`'s swap prompts: in
            // play, alive, with the mouse ours; asked for under each owner.
            if let Some(hud) = &mut s.hud {
                let at = match &*feed {
                    Feed::Play(p, _)
                        if !self.pause_menu.open && !self.settings_open && !self.loadout_picker.is_open() =>
                    {
                        p.feet()
                    }
                    _ => None,
                };
                let (map, floor) = match at {
                    Some(at) => s.renderer.weapons_in_reach(DVec3::new(at.x, at.y, at.z)),
                    None => (None, None),
                };
                let key = with_input(|i| action_key_label(i, fps::USE));
                let state = &hud.state;
                let ask = |weapon: Option<&str>| {
                    weapon
                        .filter(|w| asks_to_swap(w, &state.weapons, &state.current_weapon))
                        .map(|w| swap_text(&key, w))
                };
                let (pickup, dropped) = (ask(map), ask(floor));
                hud.pickup_prompt.set("pickup", pickup);
                hud.pickup_prompt.set("dropped", dropped);
                // `FPSHintPanel.update`'s inputs: the keys, fire held from
                // the hip with a gun that aims, the water; and `FPSPlayer`'s
                // emptied clip, where it reloads by itself.
                let gamepad = self.device.gamepad();
                with_input(|i| {
                    hud.hints.set_keys(i, gamepad);
                    // The cards that name keys, for the device in hand.
                    hud.ready.pad_key = gamepad.then(|| device_key_label(i, fps::READY, true));
                    hud.spectator_cue.pad = gamepad.then(|| {
                        let swap = with_cvars(|c| c.exists("in_pad_swap") && c.get_number("in_pad_swap") != 0.0);
                        let label = |a| device_key_label(i, a, true);
                        pad_bindings(swap, &label(fps::JUMP), &label(fps::CROUCH), &label(fps::FIRE))
                    });
                });
                let (ads, shown) = s
                    .renderer
                    .view_model
                    .as_ref()
                    .map_or((1.0, ""), |v| (v.view.ads_progress, v.view.weapon.as_str()));
                let (fire, water) = match &*feed {
                    Feed::Play(p, held) => (p.alive && held.fire, p.movement().water_level),
                    _ => (false, 0),
                };
                hud.hints.holding_fire = fire && ads < AIMING && can_aim_with(shown);
                hud.hints.water_level = water;
                let weapon = hud.state.current_weapon.clone();
                let dry = fire
                    && matches!(hud.state.ammo.get(&weapon),
                        Some(HudWeaponAmmo::Clip { clip, reserve }) if *clip <= 0.0 && *reserve > 0.0);
                hud.hints.dry_fire(dry, &weapon);
            }
            let outline = quad.then(|| quad_view_weapon_outline(s.renderer.item_time));
            if let Some(view_model) = &mut s.renderer.view_model {
                view_model.powerup_outline = outline;
            }
            let notices = feed.take_notices();
            if let Some(hud) = &mut s.hud {
                for notice in &notices {
                    hud.chat.add_notice(notice);
                }
            }
            for notice in &notices {
                println!("server: {notice}");
            }
            if let (Some(hud), None) = (&mut s.hud, &self.reel) {
                hud.scoreboard.held = self.held.contains(&KeyCode::Tab) || self.pad_scoreboard;
                hud.scale = s.window.scale_factor() as f32;
                // The keys under the board light under a free mouse.
                let scale = hud.scale;
                hud.scoreboard.set_pointer(
                    self.cursor
                        .filter(|_| !self.captured)
                        .map(|(x, y)| (x as f32 / scale, y as f32 / scale)),
                );
                // The hand over them is egui's to show, with the game's cursors.
                self.pointing = hud.scoreboard.key_hovered();
                hud.set_markers(
                    s.renderer.bomb_site_poses(),
                    &self.camera,
                    s.config.width as f32 / s.config.height.max(1) as f32,
                );
                update_watch_hud(
                    hud,
                    feed,
                    &players,
                    self.follow,
                    f64::from(self.camera.yaw),
                    &view.effects,
                    effect_dt,
                    s.config.width as f32,
                    s.config.height as f32,
                );
                hud.prepare(&s.gpu);
                // `FPSHud`: `setAmmoCount(currentClip(state))`.
                if let Some(view_model) = &mut s.renderer.view_model {
                    view_model.ammo = hud.state.current_clip();
                }
                // `FPSPlayer.updateFiring`: the Voltaic's beam shows while fire
                // is held, alive, with a loaded clip, neither reloading nor
                // switching, and not while a freeze holds movement (the
                // post-game, a round's countdown and its end).
                self.beam_wanted = matches!(&*feed, Feed::Play(p, held)
                    if p.alive
                        && held.fire
                        && p.weapon_in_hand().map(canonical_weapon_id) == Some("voltaic")
                        && !p.match_state().is_movement_frozen())
                    && hud.state.current_clip() > 0.0
                    && !self.view_reloading
                    && !s.renderer.switch_anim.is_switching();
                // The post-game opens up: the mouse comes back for the way
                // home, and the chat is ready (`FPSHud`).
                let offered = hud.scoreboard.return_offered();
                if offered {
                    MATCH_COMPLETED.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if offered && !self.opened_up {
                    self.open_up_due = true;
                }
                self.opened_up = offered;
            }
            if let (Feed::Demo(demo), Some(overlay), None) = (&*feed, &mut self.demo_hud, &self.reel) {
                overlay.present_ends(demo, || demo_final(s.hud.as_ref().map(|h| &h.state)));
            }
            self.drawn = players;
            // Hold the last frame; the title says so, until a seek goes back.
            let title = feed
                .ended()
                .map_or_else(|| "Washoi".to_owned(), |why| format!("Washoi ({why})"));
            if s.title.as_deref() != Some(title.as_str()) {
                s.window.set_title(&title);
                s.title = Some(title);
            }
        }
        s.renderer.advance_switch(f64::from(dt));
        let light = s.renderer.light_at(self.camera.position);
        let submersion = s.renderer.underwater.submersion_amount();
        if let Some(view_model) = &mut s.renderer.view_model {
            view_model.view.apply_cvars(&with_cvars(|c| ViewModelCvars::read(c)));
            // `setMovementHidden(onLadder)`: a climb takes both hands.
            view_model.view.set_movement_hidden(self.on_ladder);
            view_model.view.update_lighting(f64::from(dt), light.as_ref());
            view_model.view.submersion_amount = submersion;
            let mut horizontal = f64::from(self.velocity.x.hypot(self.velocity.z));
            // Our own eyes: the weapon bob follows the view's walk bob and
            // feels the landings it does (`updateWeaponBob`). Anyone else's:
            // a stride per 160 units, as a run reads.
            let mut landing_velocity = 0.0;
            self.bob_cycle += horizontal * f64::from(dt) / 160.0 * std::f64::consts::PI;
            let mut bob_cycle = self.bob_cycle;
            if let Some(o) = &self.view_offset {
                horizontal = o.horizontal_speed;
                bob_cycle = o.bob_cycle;
                landing_velocity = o.landing_velocity;
            }
            // Whom you watch changed: not a weapon switch for the blade's
            // charge.
            if std::mem::take(&mut self.followed_changed) {
                view_model.view.followed_changed();
            }
            view_model.view.update(&ViewModelInput {
                delta_time: f64::from(dt),
                grounded: self.grounded,
                horizontal_speed: horizontal,
                velocity_y: f64::from(self.velocity.y),
                landing_velocity,
                bob_cycle,
                aiming: self.followed_aiming.unwrap_or(self.aiming),
                reloading: false,
            });
        }
        // The view's motion goes on for drawing and comes off after, so the
        // mouse and the aim never carry it.
        let mouse = (self.camera.pitch, self.camera.yaw);
        let offset = self
            .view_offset
            .take()
            .filter(|_| !self.death_camera.is_active() && s.renderer.showcase.is_none());
        if let Some(o) = offset {
            self.camera.pitch += o.pitch as f32;
            self.camera.yaw += o.yaw as f32;
            self.camera.roll = o.roll as f32;
            self.camera.fov_y *= o.fov_multiplier as f32;
            let eye = o.eye_in_world(self.camera.right().as_dvec3(), self.camera.forward().as_dvec3());
            self.camera.position = eye.as_vec3();
        }
        s.renderer.update_underwater(&mut self.camera, f64::from(dt));
        if let Some(b) = &mut self.bench {
            b.mark("scene");
        }
        // The frame is acquired only now: the match, the sounds and the
        // effects above advance whether or not there is one to draw.
        let frame = match next_frame(&s.gpu, &s.window, &mut s.surface, &s.config) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(why) => {
                self.error = Some(why);
                return;
            }
        };
        let target = frame
            .texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        if let Some(b) = &mut self.bench {
            b.mark("acquire");
        }
        // `setBeamActive` and `updateBeam`: our beam from this frame's muzzle.
        s.renderer.set_beam_active(std::mem::take(&mut self.beam_wanted));
        s.renderer.update_beam(&self.camera);
        s.renderer
            .prepare(&s.gpu, &self.camera, [s.config.width as f32, s.config.height as f32]);
        if offset.is_some() {
            (self.camera.pitch, self.camera.yaw) = mouse;
            self.camera.roll = 0.0;
        }
        if let Some(b) = &mut self.bench {
            b.mark("prepare");
        }
        let mut encoder = s
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let mut stamp = |encoder: &mut wgpu::CommandEncoder, name| {
            if let Some(b) = &mut self.bench {
                b.stamp(encoder, name);
            }
        };
        stamp(&mut encoder, "start");
        {
            let mut pass = begin_main_pass(&mut encoder, &s.hdr.color, &s.hdr.depth, CLEAR);
            s.renderer.draw_opaque(&mut pass);
        }
        stamp(&mut encoder, "opaque");
        s.renderer.capture(&mut encoder, &s.hdr);
        stamp(&mut encoder, "capture");
        {
            let mut pass = begin_continue_pass(&mut encoder, &s.hdr.color, &s.hdr.depth);
            s.renderer.draw_transparent(&mut pass);
        }
        stamp(&mut encoder, "transparent");
        // With antialiasing on, the image and the weapon go through FXAA on
        // their way to the output; the HUD draws over it untouched.
        let smooth = antialias_on();
        let image = if smooth {
            s.fxaa.input(&s.gpu, (s.config.width, s.config.height))
        } else {
            &target
        };
        s.hdr.resolve(
            &s.gpu,
            &mut encoder,
            &s.renderer.post,
            &s.renderer.image_processing,
            image,
        );
        stamp(&mut encoder, "post");
        {
            // The weapon camera draws after the world camera's image
            // processing, straight to the output (`begin_overlay_pass`).
            let mut pass = begin_overlay_pass(&mut encoder, image, &s.hdr.depth);
            s.renderer.draw_overlay(&mut pass);
        }
        stamp(&mut encoder, "overlay");
        if smooth {
            s.fxaa.apply(&mut encoder, &target);
            stamp(&mut encoder, "fxaa");
        }
        if let (Some(hud), None) = (&s.hud, &self.reel) {
            let mut pass = begin_ui_pass(&mut encoder, &target);
            hud.draw(&mut pass);
        }
        let cards = self.console.ctx.demo.is_none_or(|d| d.hud_visible);
        let console = &mut self.console;
        let (overlay, reel, feed) = (&self.overlay, &self.reel, &self.feed);
        let followed = self
            .follow
            .and_then(|i| self.drawn.get(i))
            .map(|(id, _, _)| id.as_str());
        let mut demo_action = None;
        let mut menu_action = None;
        let pause_menu = &mut self.pause_menu;
        let (kicker, player) = match feed {
            Some(Feed::Play(p, _)) if pause_menu.open || self.settings_open => {
                let state = p.match_state();
                let kicker = [state.gametype_name(), p.map.as_str()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                let player = p
                    .roster()
                    .iter()
                    .find(|(id, _)| *id == p.player_id)
                    .map(|(_, s)| s.name.clone())
                    .unwrap_or_default();
                (kicker, player)
            }
            _ => (String::new(), String::new()),
        };
        self.pause_menu.settings = true;
        // `matchMenuModeKeys`: Loadout, in a mode with kits, hinted with the kit.
        self.pause_menu.loadout = match feed {
            Some(Feed::Play(p, _)) if p.match_state().loadout_mode().is_some() => Some(
                loadout_picker::remembered(p.match_state().gametype_name())
                    .map_or_else(|| "Choose your kit".to_owned(), loadout_picker::describe_loadout),
            ),
            _ => None,
        };
        let pause_menu = &mut self.pause_menu;
        let native_settings = &mut self.native_settings;
        let mut settings_back = false;
        let loadout = &mut self.loadout_picker;
        let mut loadout_outcome = None;
        let demo_hud = self.demo_hud.as_ref().filter(|_| cards);
        let pointing = self.pointing;
        let captured = self.captured;
        let ui = s.egui.run(&s.window, |ui| {
            if pointing {
                ui.ctx()
                    .set_cursor_icon(shagger::ui::egui_layer::egui::CursorIcon::PointingHand);
            }
            if let (Some(reel), Some(Feed::Demo(demo)), true) = (reel, feed, cards) {
                overlay.ui(ui.ctx(), reel, demo);
            }
            if let (None, Some(Feed::Demo(demo)), Some(hud)) = (reel, feed, demo_hud) {
                let name = followed.and_then(|id| {
                    demo.roster()
                        .iter()
                        .find(|(_, p)| p.entity_id == id)
                        .map(|(_, p)| p.name.as_str())
                });
                let ends = hud.ends_ui(ui.ctx(), demo);
                demo_action = hud
                    .ui(ui.ctx(), demo, followed.map(|id| (id, name.unwrap_or(id))))
                    .or(ends);
            }
            menu_action = pause_menu.ui(ui.ctx(), &kicker, &player);
            if let Some(view) = native_settings {
                settings_back = washoi::client::settings::fps_settings_view::over_match(ui, view, &kicker);
            }
            loadout_outcome = loadout.ui(ui.ctx());
            console.ui(ui.ctx());
            // While the mouse is captured egui must not show its cursor:
            // `egui_winit` calls `set_cursor_visible(true)` whenever its icon
            // changes (the Resume button's hand turning back into the arrow)
            // or the pointer comes back after alt-tab, which left the arrow
            // drawn over play. An icon of `None` keeps it hidden.
            if captured {
                ui.ctx()
                    .set_cursor_icon(shagger::ui::egui_layer::egui::CursorIcon::None);
            }
        });
        s.egui
            .paint(&s.gpu, &mut encoder, &target, [s.config.width, s.config.height], ui);
        if let Some(b) = &mut self.bench {
            b.stamp(&mut encoder, "ui");
            b.resolve(&mut encoder);
            b.mark("encode");
        }
        s.gpu.queue.submit([encoder.finish()]);
        if let Some(b) = &mut self.bench {
            b.mark("submit");
        }
        present(&s.gpu, &s.window, &mut s.surface, &s.config, frame);
        if let Some(b) = &mut self.bench {
            b.mark("present");
            b.end_frame(&s.gpu);
        }
        self.frames += 1;
        if let Some(action) = menu_action {
            self.pause_menu_action(action);
        }
        if let Some(outcome) = loadout_outcome {
            self.loadout_outcome(outcome);
        }
        self.offer_loadout();
        if settings_back {
            self.close_native_settings();
        }
        if let (Some(action), Some(Feed::Demo(demo))) = (demo_action, &mut self.feed) {
            match action {
                DemoOverlayAction::TogglePause => demo.toggle_play_pause(),
                DemoOverlayAction::Seek(progress) => demo.seek_to_progress(progress),
                DemoOverlayAction::Speed(step) => {
                    demo.step_speed(step);
                }
                // A demo starts again from its first frame.
                DemoOverlayAction::Replay => {
                    demo.seek_to_frame(0);
                    demo.play();
                }
                DemoOverlayAction::Exit => {
                    println!("demo: back to the lobby");
                    self.leave = true;
                }
            }
            if let Some(hud) = &mut self.demo_hud {
                hud.wake();
            }
        }
        if std::mem::take(&mut self.open_up_due) {
            self.set_captured(false);
            if let Some(hud) = self.surface.as_mut().and_then(|s| s.hud.as_mut())
                && !hud.chat.is_input_active()
            {
                hud.chat.open_input(false);
            }
        }
        self.serve_console();
    }
}
