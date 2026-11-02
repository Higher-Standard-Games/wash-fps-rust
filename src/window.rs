//! The window for `washoi view` and `washoi demo`: winit and a wgpu surface,
//! with a free camera. A demo or a watched match plays its players and
//! projectiles on the map in real time.
//!
//! Mouse looks (click to capture it, Escape to let go or quit), W A S D move,
//! F chases the next player of a demo or match (and back to free),
//! Space and C go up and down, Shift is faster. `--seconds` closes the window
//! after that long, for unattended runs.

use std::sync::Arc;
use std::time::{Duration, Instant};

use shagger::rendering::camera::Camera;
use shagger::rendering::glam::{DVec3, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::hdr_target::HdrTarget;
use shagger::rendering::offscreen::{begin_main_pass, begin_overlay_pass, begin_ui_pass};

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

use crate::view::{
    CLEAR, DemoViewSounds, Feed, LoadedMap, MapRenderer, build_renderer, chase_camera, play_view_sounds,
    styled_players, update_watch_hud,
};
use shagger::ui::egui_layer::EguiLayer;
use washoi::client::game_events::GameAudio;
use washoi::client::lobby_panel::install_style;
use washoi::console::{ConsoleRequest, GameConsole};

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
    /// The player the camera chases (F cycles), by place in id order.
    follow: Option<usize>,
    /// The match's sound, when there is a sound card.
    audio: Option<GameAudio>,
    /// The drop-down console (the backquote key) and its commands.
    console: GameConsole,
    /// A command asked to leave (`quit`, `disconnect`).
    leave: bool,
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
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.physical_key == PhysicalKey::Code(KeyCode::Backquote)
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
            let input = self.play_input();
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
        self.aiming = with_input(|i| i.is_active(fps::AIM));
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
            let view = feed.advance(f64::from(dt));
            if let Feed::Play(p, _) = feed {
                // First person: the eye where prediction puts it.
                if let Some(yaw) = p.take_spawn_facing() {
                    // `faceSpawn`: the spawn's facing, level.
                    self.camera.yaw = yaw as f32;
                    self.camera.pitch = 0.0;
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
            }
            let players = styled_players(view.players);
            s.renderer.set_live_players(&players, f64::from(dt));
            if let Some(i) = self.follow {
                match players.get(i) {
                    Some((_, p, _)) => chase_camera(&mut self.camera, p),
                    None => self.follow = None,
                }
            }
            s.renderer.set_live_projectiles(&view.projectiles, f64::from(dt));
            s.renderer.play_effects(&view.effects, &view.weapons, f64::from(dt));
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
            if let Some(hud) = &mut s.hud {
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
            if let Some(why) = feed.ended() {
                // Hold the last frame; the title says so.
                s.window.set_title(&format!("Washoi ({why})"));
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
        s.renderer
            .prepare(&s.gpu, &self.camera, [s.config.width as f32, s.config.height as f32]);
        let mut encoder = s
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = begin_main_pass(&mut encoder, &s.hdr.color, &s.hdr.depth, CLEAR);
            s.renderer.draw(&mut pass);
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
        if let Some(hud) = &s.hud {
            let mut pass = begin_ui_pass(&mut encoder, &view);
            hud.draw(&mut pass);
        }
        let console = &mut self.console;
        let ui = s.egui.run(&s.window, |ui| console.ui(ui.ctx()));
        s.egui
            .paint(&s.gpu, &mut encoder, &view, [s.config.width, s.config.height], ui);
        s.gpu.queue.submit([encoder.finish()]);
        s.window.pre_present_notify();
        s.gpu.queue.present(frame);
        self.frames += 1;
        self.serve_console();
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
                ConsoleRequest::Disconnect => self.leave = true,
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
                    ElementState::Pressed if code == KeyCode::Escape => {
                        if self.captured {
                            self.set_captured(false);
                        } else {
                            event_loop.exit();
                        }
                    }
                    ElementState::Pressed if code == KeyCode::KeyF && !event.repeat => {
                        // Chase the next player, then back to the free camera.
                        self.follow = match self.follow {
                            None => Some(0),
                            Some(i) => Some(i + 1),
                        };
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
pub fn open_window(event_loop: &ActiveEventLoop) -> Result<OpenWindow, String> {
    let attributes = Window::default_attributes()
        .with_title("Washoi")
        .with_inner_size(PhysicalSize::new(1280, 720));
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
) -> i32 {
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
        audio,
        leave: false,
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
