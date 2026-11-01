//! The window for `washoi view`: winit and a wgpu surface, with a free camera.
//!
//! Mouse looks (click to capture it, Escape to let go or quit), W A S D move,
//! Space and C go up and down, Shift is faster. `--seconds` closes the window
//! after that long, for unattended runs.

use std::sync::Arc;
use std::time::{Duration, Instant};

use shagger::rendering::camera::Camera;
use shagger::rendering::glam::Vec3;
use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::begin_main_pass;
use shagger::rendering::wgpu;
use shagger::rendering::world_renderer::DEPTH_FORMAT;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::view::{CLEAR, LoadedMap, MapRenderer, build_renderer};

/// Units per second (Q3's run speed is 320).
const SPEED: f32 = 320.0;
/// Radians per pixel of mouse motion.
const SENSITIVITY: f32 = 0.0022;

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,
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
}

fn depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
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
        let depth = depth_view(&gpu.device, config.width, config.height);
        self.surface = Some(Surface {
            window,
            gpu,
            surface,
            config,
            depth,
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
        self.camera.position += movement.normalize_or_zero() * speed * dt;
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
        let aspect = s.config.width as f32 / s.config.height as f32;
        s.renderer.world.animate(&s.gpu, f64::from(dt));
        s.renderer.item_time += f64::from(dt);
        s.renderer.prepare(&s.gpu, &self.camera, aspect);
        let mut encoder = s
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = begin_main_pass(&mut encoder, &view, &s.depth, CLEAR);
            s.renderer.draw(&mut pass);
        }
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
                    s.depth = depth_view(&s.gpu.device, s.config.width, s.config.height);
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
pub fn run(map: LoadedMap, camera: Camera, seconds: Option<f64>) -> i32 {
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
