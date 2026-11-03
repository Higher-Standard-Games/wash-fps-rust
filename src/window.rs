//! The window for `washoi view` and `washoi demo`: winit and a wgpu surface,
//! with a free camera. A demo or a watched match plays its players and
//! projectiles on the map in real time.
//!
//! Mouse looks (click to capture it, Escape to let go or quit), W A S D move,
//! F chases the next player of a demo or match (and back to free),
//! Space and C go up and down, Shift is faster. `--seconds` closes the window
//! after that long, for unattended runs. A highlight reel drives the camera
//! itself: Space pauses it, Left and Right step through its shots, R
//! replays it.

use std::sync::Arc;
use std::time::{Duration, Instant};
use washoi::client::fps_liquid_audio::{play_drown_gasp, play_drown_tick};
use washoi::liquid::client::local_air_tracker::AirEvent;

use shagger::rendering::camera::Camera;
use shagger::rendering::glam::{DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::hdr_target::HdrTarget;
use shagger::rendering::offscreen::{begin_continue_pass, begin_main_pass, begin_overlay_pass, begin_ui_pass};

use shagger::camera::fov_settings::current_base_fov;
use shagger::core::cvar::with_cvars;
use shagger::input::input_system::{InputSystem, with_input};
use shagger::input::key_codes::{dom_code, dom_mouse_button};
use shagger::movement::movement_vars::movement_var;
use shagger::rendering::wgpu;
use shagger::weapon::weapon_view_manager::ViewModelInput;
use washoi::client::fps_actions::fps;
use washoi::client::fps_hud::FpsHud;
use washoi::client::fps_movement_audio::play_jumppad_launch;
use washoi::client::live_play::PlayInput;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::demo_overlay::{DemoOverlay, DemoOverlayAction};
use crate::reel_overlay::ReelOverlay;
use crate::view::{
    CLEAR, DeathFollow, DemoViewSounds, Feed, LoadedMap, MapRenderer, SceneReelWorld, apply_reel_camera,
    build_renderer, chase_camera, play_view_sounds, styled_players, update_watch_hud, view_entity,
};
use shagger::animation::player_model_manager::PlayerModelVisualStyle;
use shagger::debug::debug_console::LogType;
use shagger::loader::pk3::pk3_loader::locale_compare;
use shagger::network::client::snapshot_buffer::InterpolatedEntity;
use shagger::shared::net::game_state::PlayerState;
use shagger::ui::egui_layer::EguiLayer;
use washoi::client::cinematic_demo_player::{CinematicDemoPlayer, REEL_TITLE_MS, analyze_demo};
use washoi::client::death_camera::{DeathCamera, look_angles};
use washoi::client::demo_playback_keys::{DemoKeyAction, DemoKeyContext, Modifiers, demo_key_action, frag_seek_tick};
use washoi::client::demo_service::analysis_report;
use washoi::client::game_events::GameAudio;
use washoi::client::lobby_panel::install_style;
use washoi::client::teleport_effect::TeamColor;
use washoi::client::weapon_view::quad_view_weapon_outline;
use washoi::console::{ConsoleRequest, DemoConsole, GameConsole};

/// Units per second (Q3's run speed is 320).
const SPEED: f32 = 320.0;
/// Radians per count of mouse motion at `m_sensitivity` 1
/// (`CameraInputHandler`'s `mouseSensitivity`).
const BASE_SENSITIVITY: f64 = 0.002;

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    hdr: HdrTarget,
    renderer: MapRenderer,
    /// The HUD over a demo or a match.
    hud: Option<FpsHud>,
    /// The console's drawing.
    egui: EguiLayer,
}

struct App {
    map: Option<LoadedMap>,
    camera: Camera,
    surface: Option<Surface>,
    held: std::collections::HashSet<KeyCode>,
    captured: bool,
    last: Instant,
    started: Instant,
    seconds: Option<f64>,
    frames: u64,
    error: Option<String>,
    /// The free camera's velocity, which sways the view model.
    velocity: Vec3,
    /// The right mouse button, aiming the view model down its sights.
    aiming: bool,
    /// The left mouse button held while the mouse is captured.
    firing: bool,
    /// The view bob cycle (radians), advanced with horizontal speed.
    bob_cycle: f64,
    /// On the ground, for the view bob (always, flying free).
    grounded: bool,
    /// A demo or a live match playing on the map.
    feed: Option<Feed>,
    /// The player the camera chases this frame, by place in the drawn list;
    /// worked out each frame from [`App::follow_id`].
    follow: Option<usize>,
    /// Whom the camera chases, by entity id (`_followedEntityId`), so a
    /// player joining, leaving or dying does not hand the camera to another.
    follow_id: Option<String>,
    /// The viewer chose the free camera (F past the last player); until
    /// then a demo follows its first player (`refreshHudPlayerList`).
    free_camera: bool,
    /// The view after our own death, and what it follows (`DeathCamera`).
    death_camera: DeathCamera,
    death_follow: Option<DeathFollow>,
    /// The match's sound, when there is a sound card.
    audio: Option<GameAudio>,
    /// The drop-down console (the backquote key) and its commands.
    console: GameConsole,
    /// A command asked to leave (`quit`, `disconnect`).
    leave: bool,
    /// A demo's highlight reel, driving the demo and the camera.
    reel: Option<CinematicDemoPlayer>,
    overlay: ReelOverlay,
    /// A whole demo's transport bar (`DemoViewerHUD`), when no reel drives it.
    demo_hud: Option<DemoOverlay>,
    /// The players as drawn last frame, for the reel to aim at.
    drawn: Vec<(String, InterpolatedEntity, PlayerModelVisualStyle)>,
}

impl App {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let OpenWindow {
            window,
            gpu,
            surface,
            config,
        } = open_window(event_loop)?;
        let map = self.map.as_mut().expect("the map is loaded before the window opens");
        let renderer = build_renderer(&gpu, map, config.format);
        let hdr = HdrTarget::new(&gpu, config.width, config.height);
        // A reel is in movie mode: its own cards, no HUD (it is kept for
        // `demoplay`, which leaves the reel).
        let hud = self.feed.as_ref().and_then(|_| match FpsHud::new(&gpu, config.format) {
            Ok(hud) => Some(hud),
            Err(why) => {
                println!("no HUD: {why}");
                None
            }
        });
        let egui = EguiLayer::new(&gpu, &window, config.format);
        install_style(&egui.ctx);
        self.surface = Some(Surface {
            window,
            gpu,
            surface,
            config,
            hdr,
            renderer,
            hud,
            egui,
        });
        Ok(())
    }

    /// The backquote opens and closes the console (`DebugConsole`'s
    /// keyboard handler); while it is open, the keys and the mouse are its.
    fn console_event(&mut self, event: &WindowEvent) -> bool {
        let chat_typing = self
            .surface
            .as_ref()
            .and_then(|s| s.hud.as_ref())
            .is_some_and(|h| h.chat.is_input_active());
        // Escape closes the open console (`DebugConsole`'s key handler).
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && self.console.console.open
            && key.physical_key == PhysicalKey::Code(KeyCode::Escape)
        {
            if key.state == ElementState::Pressed && !key.repeat {
                self.console.console.toggle();
            }
            return true;
        }
        // A backquote typed into the chat is the chat's.
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.physical_key == PhysicalKey::Code(KeyCode::Backquote)
            && !chat_typing
        {
            if key.state == ElementState::Pressed && !key.repeat {
                self.console.console.toggle();
                if self.console.console.open {
                    self.set_captured(false);
                    self.held.clear();
                    self.firing = false;
                    with_input(InputSystem::release_held_input);
                }
            }
            return true;
        }
        let Some(s) = &mut self.surface else { return false };
        let used = s.egui.on_window_event(&s.window, event);
        // A click the demo bar took is not a click on the game.
        // Its release still reaches the input system, or a button would stay held.
        if used
            && !self.captured
            && self.demo_hud.is_some()
            && matches!(
                event,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                }
            )
        {
            return true;
        }
        self.console.console.open
            && (used
                || matches!(
                    event,
                    WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
                ))
    }

    fn set_captured(&mut self, captured: bool) {
        let Some(s) = &self.surface else { return };
        let grab = if captured {
            s.window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| s.window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok()
        } else {
            let _ = s.window.set_cursor_grab(CursorGrabMode::None);
            false
        };
        s.window.set_cursor_visible(!grab);
        if self.captured && !grab {
            // Pointer lock lost: nothing stays held (`releaseHeldInput`).
            self.held.clear();
            self.firing = false;
            with_input(InputSystem::release_held_input);
        }
        self.captured = grab;
    }

    /// Talk mode's keys (`FPSChat`'s input): T or Y opens it while
    /// playing; while open, every key types, Enter sends and Escape
    /// cancels. True when the key was the chat's.
    fn chat_key(&mut self, code: KeyCode, event: &winit::event::KeyEvent) -> bool {
        let Some(Feed::Play(play, _)) = &self.feed else {
            return false;
        };
        let Some(hud) = self.surface.as_mut().and_then(|s| s.hud.as_mut()) else {
            return false;
        };
        let chat = &mut hud.chat;
        if !chat.is_input_active() {
            if event.state == ElementState::Pressed && !event.repeat && matches!(code, KeyCode::KeyT | KeyCode::KeyY) {
                chat.open_input(code == KeyCode::KeyY);
                // Nothing stays held while typing.
                self.held.clear();
                self.firing = false;
                with_input(InputSystem::release_held_input);
                return true;
            }
            return false;
        }
        if event.state != ElementState::Pressed {
            return true;
        }
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some((text, team_only)) = chat.submit() {
                    play.say(&text, team_only);
                }
            }
            KeyCode::Escape => chat.close_input(),
            KeyCode::Backspace => chat.backspace(),
            _ => {
                if let Some(text) = &event.text {
                    chat.type_text(text);
                }
            }
        }
        true
    }

    /// This frame's actions as a player's input, through the player's
    /// bindings (`FPSActions`).
    fn play_input(&self) -> PlayInput {
        with_input(|i| PlayInput {
            forward: i.is_active(fps::MOVE_FORWARD),
            backward: i.is_active(fps::MOVE_BACKWARD),
            left: i.is_active(fps::MOVE_LEFT),
            right: i.is_active(fps::MOVE_RIGHT),
            jump: i.is_active(fps::JUMP),
            crouch: i.is_active(fps::CROUCH),
            sprint: i.is_active(fps::SPRINT),
            // The first click captures the mouse rather than firing.
            fire: self.captured && i.is_active(fps::FIRE),
            alt_fire: i.is_active(fps::AIM),
            reload: i.is_active(fps::RELOAD),
            use_key: i.is_active(fps::USE),
            yaw: f64::from(self.camera.yaw),
            pitch: f64::from(self.camera.pitch),
            // Held rather than pressed: the input is sampled each tick.
            weapon_slot: fps::WEAPONS
                .iter()
                .position(|id| i.is_active(id))
                .map_or(0, |n| n as u8 + 1),
        })
    }

    fn step(&mut self, dt: f32) {
        if matches!(self.feed, Some(Feed::Play(..))) {
            // `FPSController.update`: without the pointer lock, no keys play.
            let input = if self.captured {
                self.play_input()
            } else {
                PlayInput {
                    yaw: f64::from(self.camera.yaw),
                    pitch: f64::from(self.camera.pitch),
                    ..PlayInput::default()
                }
            };
            if let Some(Feed::Play(_, held)) = &mut self.feed {
                *held = input;
            }
            self.velocity = Vec3::ZERO;
            return;
        }
        let key = |k| self.held.contains(&k);
        let axis = |plus, minus| f32::from(u8::from(key(plus))) - f32::from(u8::from(key(minus)));
        let speed = if key(KeyCode::ShiftLeft) { SPEED * 3.0 } else { SPEED };
        let movement = self.camera.forward() * axis(KeyCode::KeyW, KeyCode::KeyS)
            + self.camera.right() * axis(KeyCode::KeyD, KeyCode::KeyA)
            + Vec3::Y * axis(KeyCode::Space, KeyCode::KeyC);
        let velocity = movement.normalize_or_zero() * speed;
        self.camera.position += velocity * dt;
        self.velocity = velocity;
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        with_input(InputSystem::update);
        self.aiming = self.captured && with_input(|i| i.is_active(fps::AIM));
        self.step(dt);
        with_input(InputSystem::end_frame);
        // `cl_fov`, as `currentBaseFov` gives every camera.
        self.camera.fov_y = with_cvars(|c| current_base_fov(c)) as f32;
        let Some(s) = &mut self.surface else { return };
        let frame = match s.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                s.surface.configure(&s.gpu.device, &s.config);
                return;
            }
            other => {
                self.error = Some(format!("surface: {other:?}"));
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        s.renderer.world.animate(&s.gpu, f64::from(dt));
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
            let view = feed.advance(f64::from(dt));
            if let Feed::Play(p, _) = feed {
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
                let v = p.velocity();
                self.velocity = Vec3::new(v.x as f32, v.y as f32, v.z as f32);
                self.grounded = p.grounded();
                self.follow = None;
                s.renderer.hold(p.weapon_in_hand());
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
            let players = styled_players(view.players);
            let base_fov = self.camera.fov_y;
            let eyes = self.reel.as_ref().and_then(|r| {
                self.follow = None;
                apply_reel_camera(&mut self.camera, r, &players, base_fov)
            });
            let drawn: Vec<_> = players
                .iter()
                .filter(|(id, _, _)| Some(id) != eyes.as_ref())
                .cloned()
                .collect();
            s.renderer.set_live_weapons(&view.weapons);
            s.renderer.set_live_players(&drawn, f64::from(dt));
            if self.reel.is_none() {
                // `--follow N` names a place in the first frame's list.
                if let (Some(i), None) = (self.follow, &self.follow_id) {
                    self.follow_id = players.get(i).map(|(id, _, _)| id.clone());
                }
                // A demo watches someone until the viewer flies free.
                if self.follow_id.is_none() && !self.free_camera && matches!(feed, Feed::Demo(_)) {
                    self.follow_id = name_order(feed.roster(), &players).into_iter().next();
                }
                self.follow = self
                    .follow_id
                    .as_ref()
                    .and_then(|id| players.iter().position(|(p, _, _)| p == id));
            }
            if let Some(i) = self.follow
                && let Some((_, p, _)) = players.get(i)
            {
                chase_camera(&mut self.camera, p);
            }
            s.renderer.set_live_projectiles(&view.projectiles, f64::from(dt));
            s.renderer.play_effects(&view.effects, &view.weapons, f64::from(dt));
            let local = match feed {
                Feed::Play(..) => feed.local_entity(),
                _ => None,
            };
            s.renderer.set_local_player(local, f64::from(self.camera.yaw));
            s.renderer.play_gibs(&view.effects, f64::from(dt));
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
                let target = self.death_follow.and_then(|f| s.renderer.follow_position(f));
                let renderer = &mut s.renderer;
                let mut trace = |from, to| renderer.trace_map(from, to);
                if let Some((at, look)) = self.death_camera.update(f64::from(dt), target, Some(&mut trace)) {
                    let (yaw, pitch) = look_angles(at, look);
                    self.camera.position = at.as_vec3();
                    self.camera.yaw = yaw as f32;
                    self.camera.pitch = pitch as f32;
                }
            }
            s.renderer.play_hit_feedback(
                &view.effects,
                view_entity(feed.local_entity(), &players, self.follow),
                f64::from(dt),
            );
            if let Some(audio) = &mut self.audio {
                play_view_sounds(
                    audio,
                    &mut s.renderer,
                    &self.camera,
                    &DemoViewSounds {
                        effects: &view.effects,
                        teams: &view.teams,
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
            let quad = matches!(feed, Feed::Play(..))
                && s.hud.as_mut().is_some_and(|h| {
                    let now = h.state.now;
                    h.state.powerups.is_active("quad", now)
                });
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
                hud.scoreboard.held = self.held.contains(&KeyCode::Tab);
                update_watch_hud(
                    hud,
                    feed,
                    &players,
                    self.follow,
                    f64::from(self.camera.yaw),
                    &view.effects,
                    f64::from(dt),
                    s.config.width as f32,
                    s.config.height as f32,
                );
                hud.prepare(&s.gpu);
            }
            self.drawn = players;
            // Hold the last frame; the title says so, until a seek goes back.
            let title = feed
                .ended()
                .map_or_else(|| "Washoi".to_owned(), |why| format!("Washoi ({why})"));
            if title != s.window.title() {
                s.window.set_title(&title);
            }
        }
        if let Some(view_model) = &mut s.renderer.view_model {
            let horizontal = f64::from(self.velocity.x.hypot(self.velocity.z));
            // The view bob's walk cycle: a stride per 160 units, as a run reads.
            self.bob_cycle += horizontal * f64::from(dt) / 160.0 * std::f64::consts::PI;
            view_model.view.update(&ViewModelInput {
                delta_time: f64::from(dt),
                grounded: self.grounded,
                horizontal_speed: horizontal,
                velocity_y: f64::from(self.velocity.y),
                landing_velocity: 0.0,
                bob_cycle: self.bob_cycle,
                aiming: self.aiming,
                reloading: false,
            });
        }
        s.renderer.update_underwater(&mut self.camera, f64::from(dt));
        s.renderer
            .prepare(&s.gpu, &self.camera, [s.config.width as f32, s.config.height as f32]);
        let mut encoder = s
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = begin_main_pass(&mut encoder, &s.hdr.color, &s.hdr.depth, CLEAR);
            s.renderer.draw_opaque(&mut pass);
        }
        s.renderer.capture(&mut encoder, &s.hdr);
        {
            let mut pass = begin_continue_pass(&mut encoder, &s.hdr.color, &s.hdr.depth);
            s.renderer.draw_transparent(&mut pass);
        }
        {
            let mut pass = begin_overlay_pass(&mut encoder, &s.hdr.color, &s.hdr.depth);
            s.renderer.draw_overlay(&mut pass);
        }
        s.hdr.resolve(
            &s.gpu,
            &mut encoder,
            &s.renderer.post,
            &s.renderer.image_processing,
            &view,
        );
        if let (Some(hud), None) = (&s.hud, &self.reel) {
            let mut pass = begin_ui_pass(&mut encoder, &view);
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
        let demo_hud = self.demo_hud.as_ref().filter(|_| cards);
        let ui = s.egui.run(&s.window, |ui| {
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
                demo_action = hud.ui(ui.ctx(), demo, followed.map(|id| (id, name.unwrap_or(id))));
            }
            console.ui(ui.ctx());
        });
        s.egui
            .paint(&s.gpu, &mut encoder, &view, [s.config.width, s.config.height], ui);
        s.gpu.queue.submit([encoder.finish()]);
        s.window.pre_present_notify();
        s.gpu.queue.present(frame);
        self.frames += 1;
        if let (Some(action), Some(Feed::Demo(demo))) = (demo_action, &mut self.feed) {
            match action {
                DemoOverlayAction::TogglePause => demo.toggle_play_pause(),
                DemoOverlayAction::Seek(progress) => demo.seek_to_progress(progress),
                DemoOverlayAction::Speed(step) => {
                    demo.step_speed(step);
                }
            }
            if let Some(hud) = &mut self.demo_hud {
                hud.wake();
            }
        }
        self.serve_console();
    }

    /// A whole demo's keys (`DemoPlaybackKeys` outside a reel): Space
    /// pauses, the arrows seek and change speed, comma and period step a
    /// frame, N and P jump between frags, Tab and the digits pick whose
    /// eyes to watch through. Flying free, Space and the arrows move.
    fn demo_key(&mut self, code: KeyCode, repeat: bool) -> bool {
        let (None, Some(Feed::Demo(demo)), Some(hud)) = (&self.reel, &mut self.feed, &mut self.demo_hud) else {
            return false;
        };
        let held = |k| self.held.contains(&k);
        let modifiers = Modifiers {
            shift: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
            alt: held(KeyCode::AltLeft) || held(KeyCode::AltRight),
            ctrl: held(KeyCode::ControlLeft) || held(KeyCode::ControlRight),
            meta: held(KeyCode::SuperLeft) || held(KeyCode::SuperRight),
        };
        let context = DemoKeyContext {
            reel: false,
            free_camera: self.free_camera,
        };
        let Some(action) = demo_key_action(&dom_code(code), modifiers, context) else {
            return false;
        };
        let repeats = matches!(action, DemoKeyAction::Seek { .. } | DemoKeyAction::Frame { .. });
        if repeat && !repeats {
            return true;
        }
        hud.wake();
        let rate = demo.tick_rate();
        let order = name_order(demo.roster(), &self.drawn);
        let players = order.len();
        match action {
            DemoKeyAction::TogglePause => demo.toggle_play_pause(),
            DemoKeyAction::Seek { seconds } => demo.seek_to_tick(demo.playhead_tick() + seconds * rate),
            DemoKeyAction::Frame { step } => {
                demo.pause();
                demo.seek_to_frame(demo.current_frame() as isize + isize::from(step));
            }
            DemoKeyAction::Speed { step } => {
                demo.step_speed(step);
            }
            DemoKeyAction::Frag { step } => {
                if let Some(tick) = frag_seek_tick(&hud.frag_ticks(), demo.playhead_tick(), step, rate) {
                    demo.seek_to_tick(tick);
                }
            }
            DemoKeyAction::Player { step } => {
                if players > 0 {
                    let current = self
                        .follow_id
                        .as_ref()
                        .and_then(|id| order.iter().position(|o| o == id));
                    let at = current.unwrap_or(if step > 0 { players - 1 } else { 0 });
                    let next = (at as isize + isize::from(step)).rem_euclid(players as isize) as usize;
                    self.follow_id = Some(order[next].clone());
                    self.free_camera = false;
                }
            }
            DemoKeyAction::PlayerAt { index } => {
                if let Some(id) = order.get(index) {
                    self.follow_id = Some(id.clone());
                    self.free_camera = false;
                }
            }
            DemoKeyAction::Shot { .. } => return false,
        }
        true
    }

    /// The reel's keys (`DemoPlaybackKeys` in reel mode): Space pauses,
    /// Left and Right step through the shots, R plays it again.
    fn reel_key(&mut self, code: KeyCode) -> bool {
        let (Some(reel), Some(Feed::Demo(demo))) = (&mut self.reel, &mut self.feed) else {
            return false;
        };
        let shots = self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut());
        let mut world = SceneReelWorld {
            shots,
            players: &self.drawn,
        };
        match code {
            KeyCode::Space => reel.toggle_play_pause(demo),
            KeyCode::ArrowRight => reel.next_segment(demo, &mut world),
            KeyCode::ArrowLeft => reel.prev_segment(demo, &mut world),
            KeyCode::KeyR => reel.replay(demo, &mut world),
            _ => return false,
        }
        true
    }

    /// What the console's commands asked for this frame.
    fn serve_console(&mut self) {
        for request in std::mem::take(&mut self.console.ctx.requests) {
            match request {
                ConsoleRequest::Say { text, team } => {
                    if let Some(Feed::Play(p, _)) = &self.feed {
                        p.say(&text, team);
                    }
                }
                ConsoleRequest::Disconnect | ConsoleRequest::DemoStop => self.leave = true,
                ConsoleRequest::DemoPlay => {
                    if let Some(Feed::Demo(demo)) = &mut self.feed {
                        self.reel = None;
                        demo.set_speed(1.0);
                        demo.seek_to_frame(0);
                        demo.play();
                    }
                }
                ConsoleRequest::DemoCinematic { minutes } => {
                    if let Some(Feed::Demo(demo)) = &mut self.feed {
                        let mut reel = CinematicDemoPlayer::new(REEL_TITLE_MS);
                        reel.load_and_play(
                            demo,
                            &mut SceneReelWorld {
                                shots: self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut()),
                                players: &self.drawn,
                            },
                            minutes,
                        );
                        self.follow = None;
                        self.reel = Some(reel);
                    }
                }
                ConsoleRequest::DemoAnalyze => {
                    if let Some(Feed::Demo(demo)) = &self.feed {
                        let analysis = analyze_demo(
                            demo,
                            &mut SceneReelWorld {
                                shots: self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut()),
                                players: &self.drawn,
                            },
                        );
                        let report = analysis_report(&analysis, &demo.header.map, demo.duration_ms);
                        self.console.console.log(LogType::Result, &report);
                    }
                }
            }
        }
        if self.console.ctx.quit {
            self.leave = true;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_some() {
            return;
        }
        if let Err(why) = self.open(event_loop) {
            self.error = Some(why);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.console_event(&event) {
            return;
        }
        match &event {
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(b) = dom_mouse_button(*button) {
                    with_input(|i| match state {
                        ElementState::Pressed => i.mouse_down(b),
                        ElementState::Released => i.mouse_up(b),
                    });
                }
            }
            // `WheelEvent.deltaY`: positive scrolls down, where winit's
            // positive scrolls up.
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -f64::from(*y) * 100.0,
                    MouseScrollDelta::PixelDelta(p) => -p.y,
                };
                with_input(|i| i.wheel(dy));
            }
            _ => {}
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(s) = &mut self.surface {
                    s.config.width = size.width.max(1);
                    s.config.height = size.height.max(1);
                    s.surface.configure(&s.gpu.device, &s.config);
                    s.hdr.resize(&s.gpu, s.config.width, s.config.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                if self.chat_key(code, &event) {
                    return;
                }
                with_input(|i| match event.state {
                    ElementState::Pressed => i.key_down(&dom_code(code)),
                    ElementState::Released => i.key_up(&dom_code(code)),
                });
                match event.state {
                    // A held Escape frees the mouse once; its repeats do not quit.
                    ElementState::Pressed if code == KeyCode::Escape && event.repeat => {}
                    ElementState::Pressed if code == KeyCode::Escape => {
                        if self.captured {
                            self.set_captured(false);
                        } else {
                            event_loop.exit();
                        }
                    }
                    ElementState::Pressed if !event.repeat && self.reel_key(code) => {}
                    // A reel's keys do not repeat (`bindPlaybackKeys`).
                    ElementState::Pressed if event.repeat && self.reel.is_some() => {}
                    ElementState::Pressed if self.demo_key(code, event.repeat) => {}
                    ElementState::Pressed if code == KeyCode::KeyF && !event.repeat => {
                        // Chase the next player by name, then back to the free camera.
                        let order = self
                            .feed
                            .as_ref()
                            .map_or_else(Vec::new, |f| name_order(f.roster(), &self.drawn));
                        let next = match self
                            .follow_id
                            .as_ref()
                            .and_then(|id| order.iter().position(|o| o == id))
                        {
                            None if !self.free_camera || self.follow_id.is_none() => order.first(),
                            None => None,
                            Some(i) => order.get(i + 1),
                        };
                        self.follow_id = next.cloned();
                        self.free_camera = self.follow_id.is_none();
                    }
                    ElementState::Pressed => {
                        self.held.insert(code);
                    }
                    ElementState::Released => {
                        self.held.remove(&code);
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if !self.captured && state == ElementState::Pressed {
                    self.set_captured(true);
                } else {
                    self.firing = state == ElementState::Pressed;
                }
            }
            WindowEvent::CursorMoved { .. } => {
                // `DemoViewerHUD`: a move of the mouse brings the bar back.
                if let Some(hud) = &mut self.demo_hud {
                    hud.wake();
                }
            }
            WindowEvent::Focused(false) => {
                self.held.clear();
                self.firing = false;
                with_input(InputSystem::release_held_input);
                self.set_captured(false);
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.error.is_some() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && self.captured
        {
            // `m_sensitivity`, and `cl_ads_sensitivity`'s share while aiming
            // (the browser eases it in with the aim; this takes it whole).
            let sensitivity = with_cvars(|c| {
                let ads = if self.aiming {
                    movement_var(c, "cl_ads_sensitivity")
                } else {
                    1.0
                };
                BASE_SENSITIVITY * movement_var(c, "m_sensitivity") * ads
            }) as f32;
            self.camera.yaw += delta.0 as f32 * sensitivity;
            let limit = std::f32::consts::FRAC_PI_2 - 0.01;
            self.camera.pitch = (self.camera.pitch + delta.1 as f32 * sensitivity).clamp(-limit, limit);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.leave {
            event_loop.exit();
            return;
        }
        if let Some(limit) = self.seconds
            && self.started.elapsed() >= Duration::from_secs_f64(limit)
        {
            event_loop.exit();
            return;
        }
        if let Some(s) = &self.surface {
            s.window.request_redraw();
        }
    }
}

thread_local! {
    /// One event loop for the process: winit makes only one, so the menu
    /// and then the match take turns on it (`run_app_on_demand`).
    static EVENT_LOOP: std::cell::RefCell<Option<EventLoop<()>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `app` on the process's event loop until it exits.
pub fn run_on_event_loop(app: &mut impl ApplicationHandler) -> Result<(), String> {
    EVENT_LOOP.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(EventLoop::new().map_err(|e| format!("no window system: {e}"))?);
        }
        let event_loop = slot.as_mut().expect("made above");
        event_loop.set_control_flow(ControlFlow::Poll);
        event_loop
            .run_app_on_demand(app)
            .map_err(|e| format!("event loop: {e}"))
    })
}

/// A window with a wgpu surface on it, configured.
pub struct OpenWindow {
    pub window: Arc<Window>,
    pub gpu: Gpu,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

/// Opens Washoi's window, 1280 × 720, with a non-sRGB surface in vsync.
/// The game's icon for the title bar and the task bar (macOS takes the
/// app bundle's instead).
fn window_icon() -> Option<winit::window::Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/icon/washoi-128.png"))
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    winit::window::Icon::from_rgba(image.into_raw(), width, height).ok()
}

pub fn open_window(event_loop: &ActiveEventLoop) -> Result<OpenWindow, String> {
    let attributes = Window::default_attributes()
        .with_title("Washoi")
        .with_inner_size(PhysicalSize::new(1280, 720))
        .with_window_icon(window_icon());
    let window = Arc::new(event_loop.create_window(attributes).map_err(|e| e.to_string())?);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
        event_loop.owned_display_handle(),
    )));
    let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
    let gpu = Gpu::new(instance, Some(&surface)).map_err(|e| e.to_string())?;
    println!("gpu: {}", gpu.describe());
    let size = window.inner_size();
    let mut config = surface
        .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
        .ok_or("the surface does not fit this adapter")?;
    // Babylon writes display-space bytes as they are: prefer a non-sRGB target.
    let formats = surface.get_capabilities(&gpu.adapter).formats;
    if let Some(&plain) = formats.iter().find(|f| !f.is_srgb()) {
        config.format = plain;
    }
    config.present_mode = wgpu::PresentMode::AutoVsync;
    surface.configure(&gpu.device, &config);
    Ok(OpenWindow {
        window,
        gpu,
        surface,
        config,
    })
}

/// Opens the window and runs until it closes.
pub fn run(
    map: LoadedMap,
    camera: Camera,
    seconds: Option<f64>,
    feed: Option<Feed>,
    follow: Option<usize>,
    audio: Option<GameAudio>,
    reel: Option<CinematicDemoPlayer>,
) -> i32 {
    // A key still down when the last window closed never saw its release.
    with_input(InputSystem::release_held_input);
    let mut app = App {
        map: Some(map),
        camera,
        surface: None,
        held: std::collections::HashSet::new(),
        captured: false,
        last: Instant::now(),
        started: Instant::now(),
        seconds,
        frames: 0,
        error: None,
        velocity: Vec3::ZERO,
        aiming: false,
        firing: false,
        bob_cycle: 0.0,
        grounded: true,
        console: {
            let mut console = GameConsole::new(washoi::config_dir());
            console.ctx.server = feed.as_ref().and_then(Feed::server_url);
            console
        },
        feed,
        follow,
        follow_id: None,
        free_camera: false,
        death_camera: DeathCamera::default(),
        death_follow: None,
        audio,
        leave: false,
        reel,
        overlay: ReelOverlay::default(),
        demo_hud: None,
        drawn: Vec::new(),
    };
    let ran = run_on_event_loop(&mut app);
    // Close the window before the next one opens on the same loop.
    app.surface = None;
    if let Err(e) = ran {
        eprintln!("{e}");
        return 1;
    }
    let elapsed = app.started.elapsed().as_secs_f64();
    println!(
        "{} frames in {elapsed:.1} s ({:.1} fps)",
        app.frames,
        app.frames as f64 / elapsed
    );
    if let Some(why) = app.error {
        eprintln!("{why}");
        return 1;
    }
    0
}

/// The drawn players' ids by name, as the viewer cycles them
/// (`playerOrder`: `name.localeCompare`, then id).
fn name_order(
    roster: &[(String, PlayerState)],
    drawn: &[(String, InterpolatedEntity, PlayerModelVisualStyle)],
) -> Vec<String> {
    let name = |id: &str| {
        roster
            .iter()
            .find(|(_, p)| p.entity_id == id && !p.name.is_empty())
            .map_or_else(|| id.to_lowercase(), |(_, p)| p.name.to_lowercase())
    };
    let mut ids: Vec<&str> = drawn.iter().map(|(id, _, _)| id.as_str()).collect();
    ids.sort_by(|a, b| locale_compare(&name(a), &name(b)).then(a.cmp(b)));
    ids.into_iter().map(str::to_owned).collect()
}
