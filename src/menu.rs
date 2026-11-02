//! `washoi menu <ws://lobby>`: the game as a player starts it. The Play page
//! (`client::lobby_panel`, wash-fps `client/LobbyPanel.ts`) in the window
//! until the lobby hands out a game server, then the match in the same
//! window's place, then back to the menu when the match window closes.
//!
//! With `--screenshot`, the page runs offscreen for `--seconds` (`--quick`
//! presses Quick play once it can) and its last frame is saved.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
use shagger::rendering::wgpu;
use shagger::ui::egui_layer::{EguiLayer, egui};
use washoi::client::lobby_panel::{LobbyChoice, LobbyPanel, install_style};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::window::{OpenWindow, open_window, run_on_event_loop};

/// Behind the page: Washoi's night blue.
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.035,
    g: 0.03,
    b: 0.07,
    a: 1.0,
};

struct MenuWindow {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layer: EguiLayer,
}

struct MenuApp {
    panel: LobbyPanel,
    open: Option<MenuWindow>,
    choice: Option<LobbyChoice>,
    error: Option<String>,
}

impl MenuApp {
    fn redraw(&mut self) {
        self.panel.poll();
        if let Some(choice) = self.panel.take_choice() {
            self.choice = Some(choice);
            return;
        }
        let Some(m) = &mut self.open else { return };
        let frame = match m.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                m.surface.configure(&m.gpu.device, &m.config);
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let panel = &mut self.panel;
        let ui = m.layer.run(&m.window, |ui| panel.ui(ui));
        let mut encoder = m
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("menu") });
        clear(&mut encoder, &view);
        m.layer
            .paint(&m.gpu, &mut encoder, &view, [m.config.width, m.config.height], ui);
        m.gpu.queue.submit([encoder.finish()]);
        m.window.pre_present_notify();
        m.gpu.queue.present(frame);
    }
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("menu clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(BACKGROUND),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

impl ApplicationHandler for MenuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() {
            return;
        }
        match open_window(event_loop) {
            Ok(OpenWindow {
                window,
                gpu,
                surface,
                config,
            }) => {
                window.set_title("Washoi");
                let layer = EguiLayer::new(&gpu, &window, config.format);
                install_style(&layer.ctx);
                self.open = Some(MenuWindow {
                    window,
                    gpu,
                    surface,
                    config,
                    layer,
                });
            }
            Err(why) => {
                self.error = Some(why);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(m) = &mut self.open
            && m.layer.on_window_event(&m.window, &event)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(m) = &mut self.open {
                    m.config.width = size.width.max(1);
                    m.config.height = size.height.max(1);
                    m.surface.configure(&m.gpu.device, &m.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.choice.is_some() {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(m) = &self.open {
            m.window.request_redraw();
        }
    }
}

/// The menu in its window until the lobby sends this player somewhere;
/// `None` when the window closes first.
pub fn choose(url: &str, key_file: Option<std::path::PathBuf>) -> Result<Option<LobbyChoice>, String> {
    let mut app = MenuApp {
        panel: LobbyPanel::connect(url, key_file),
        open: None,
        choice: None,
        error: None,
    };
    let ran = run_on_event_loop(&mut app);
    app.open = None;
    ran?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok(app.choice)
}

/// The page offscreen for `seconds`, its last frame saved to `path`.
pub fn screenshot(
    url: &str,
    key_file: Option<std::path::PathBuf>,
    quick: bool,
    seconds: f64,
    (width, height): (u32, u32),
    path: &Path,
) -> Result<Option<LobbyChoice>, String> {
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
    install_style(&layer.ctx);
    let mut panel = LobbyPanel::connect(url, key_file);
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32, height as f32),
        )),
        ..Default::default()
    };
    let started = Instant::now();
    let mut pressed = false;
    let mut status = String::new();
    let mut pixels = Vec::new();
    let mut choice = None;
    while started.elapsed() < Duration::from_secs_f64(seconds) {
        panel.poll();
        if quick && !pressed && panel.quick_play_enabled() {
            println!("menu: quick play");
            panel.start_quick_play();
            pressed = true;
        }
        if let Some(room) = panel.in_room() {
            let now = format!("{} {}: {}", room.gametype_label, room.lobby_id, room.status());
            if now != status {
                println!("menu: {now}");
                status = now;
            }
        }
        let frame = layer.run_input(input(), |ui| panel.ui(ui));
        pixels = render_encoded_to_rgba(&gpu, width, height, BACKGROUND, |encoder, view| {
            layer.paint(&gpu, encoder, view, [width, height], frame);
        });
        if let Some(c) = panel.take_choice() {
            choice = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(33));
    }
    if let Some(why) = &panel.closed {
        println!("menu: lobby closed: {why}");
    }
    image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    println!("menu: saved {}", path.display());
    Ok(choice)
}
