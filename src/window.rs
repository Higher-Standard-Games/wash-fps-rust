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
use shagger::rendering::glam::Vec3;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::hdr_target::HdrTarget;
use shagger::rendering::offscreen::{begin_main_pass, begin_overlay_pass};
use shagger::rendering::wgpu;
use shagger::weapon::weapon_view_manager::ViewModelInput;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::view::{
    CLEAR, DemoViewSounds, Feed, LoadedMap, MapRenderer, build_renderer, chase_camera, play_view_sounds, styled_players,
};
use washoi::client::game_events::GameAudio;

/// Units per second (Q3's run speed is 320).
const SPEED: f32 = 320.0;
/// Radians per pixel of mouse motion.
const SENSITIVITY: f32 = 0.0022;

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    hdr: HdrTarget,
    renderer: MapRenderer,
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
    /// The view bob cycle (radians), advanced with horizontal speed.
    bob_cycle: f64,
    /// A demo or a live match playing on the map.
    feed: Option<Feed>,
    /// The player the camera chases (F cycles), by place in id order.
    follow: Option<usize>,
    /// The match's sound, when there is a sound card.
    audio: Option<GameAudio>,
}

impl App {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
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
        let map = self.map.as_mut().expect("the map is loaded before the window opens");
        let renderer = build_renderer(&gpu, map, config.format);
        let hdr = HdrTarget::new(&gpu, config.width, config.height);
        self.surface = Some(Surface {
            window,
            gpu,
            surface,
            config,
            hdr,
            renderer,
        });
        Ok(())
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

    fn step(&mut self, dt: f32) {
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
        self.step(dt);
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
                    },
                    &players,
                    self.follow,
                );
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
                grounded: true,
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
        s.gpu.queue.submit([encoder.finish()]);
        s.window.pre_present_notify();
        s.gpu.queue.present(frame);
        self.frames += 1;
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
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.set_captured(true),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => self.aiming = state == ElementState::Pressed,
            WindowEvent::Focused(false) => {
                self.held.clear();
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
            self.camera.yaw += delta.0 as f32 * SENSITIVITY;
            let limit = std::f32::consts::FRAC_PI_2 - 0.01;
            self.camera.pitch = (self.camera.pitch + delta.1 as f32 * SENSITIVITY).clamp(-limit, limit);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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

/// Opens the window and runs until it closes.
pub fn run(
    map: LoadedMap,
    camera: Camera,
    seconds: Option<f64>,
    feed: Option<Feed>,
    follow: Option<usize>,
    audio: Option<GameAudio>,
) -> i32 {
    let event_loop = match EventLoop::new() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("no window system: {e}");
            return 1;
        }
    };
    event_loop.set_control_flow(ControlFlow::Poll);
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
        bob_cycle: 0.0,
        feed,
        follow,
        audio,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop: {e}");
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
