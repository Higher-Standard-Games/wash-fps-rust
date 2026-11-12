//! The native lobby on the event loop (docs/research/NATIVE_LOBBY.md): the
//! window around [`washoi::client::lobby_panel`], as the region and data
//! screens are around theirs. No TypeScript source of its own: the browser
//! shows `LobbyPanel` as a page.
//!
//! The panel outlives the screen: `main` keeps it through every match, so
//! the lobby connection, the room and the chat are there when the player
//! comes back, as the browser's hidden page keeps them.

use std::path::Path;
use std::time::{Duration, Instant};

use shagger::rendering::gpu::Gpu;
use shagger::rendering::offscreen::{OFFSCREEN_FORMAT, render_encoded_to_rgba};
use shagger::rendering::wgpu;
use shagger::ui::components::lobby_twin::TwinStyle;
use shagger::ui::egui_layer::{EguiLayer, egui};
use washoi::client::lobby_panel::{LobbyChoice, LobbyPanel, PanelAction};
use washoi::client::washoi_style::install_style;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::lobby_webview::LobbyOutcome;
use crate::window::{OpenWindow, end_screen, next_frame, open_window, present, run_on_event_loop};

/// The stage's blue, under the egui backdrop.
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.035,
    g: 0.03,
    b: 0.07,
    a: 1.0,
};

/// What the native lobby ended in.
#[derive(Debug)]
pub enum NativeOutcome {
    Lobby(LobbyOutcome),
    /// Back to the web lobby, for a page the native one does not have yet.
    WebLobby,
}

struct Open {
    window: std::sync::Arc<winit::window::Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    layer: EguiLayer,
}

struct LobbyApp<'a> {
    panel: &'a mut LobbyPanel,
    style: TwinStyle,
    open: Option<Open>,
    outcome: Option<NativeOutcome>,
    error: Option<String>,
}

impl LobbyApp<'_> {
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        self.panel.poll();
        if let Some(choice) = self.panel.take_choice() {
            self.outcome = Some(NativeOutcome::Lobby(LobbyOutcome::Match(choice)));
            end_screen(event_loop);
            return;
        }
        let Some(m) = &mut self.open else { return };
        let frame = match next_frame(&m.gpu, &m.window, &mut m.surface, &m.config) {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(why) => {
                self.error = Some(why);
                end_screen(event_loop);
                return;
            }
        };
        let view = frame
            .texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let (panel, style) = (&mut *self.panel, &self.style);
        let mut action = None;
        let ui = m.layer.run(&m.window, |ui| action = panel.ui(ui, style));
        let mut encoder = m
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("lobby") });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lobby clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
        m.layer
            .paint(&m.gpu, &mut encoder, &view, [m.config.width, m.config.height], ui);
        m.gpu.queue.submit([encoder.finish()]);
        present(&m.gpu, &m.window, &mut m.surface, &m.config, frame);
        match action {
            Some(PanelAction::WebLobby) => {
                self.outcome = Some(NativeOutcome::WebLobby);
                end_screen(event_loop);
            }
            Some(PanelAction::Quit) => {
                self.outcome = Some(NativeOutcome::Lobby(LobbyOutcome::Quit));
                end_screen(event_loop);
            }
            Some(PanelAction::OpenUrl(url)) => open_url(url),
            None => {}
        }
    }
}

impl ApplicationHandler for LobbyApp<'_> {
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
                // Out of the match's hold: the cursor free and shown.
                window.set_cursor_visible(true);
                let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                shagger::input::native_cursor::set_cursor_hidden(false);
                let layer = EguiLayer::new(&gpu, &window, config.format);
                install_style(&layer.ctx);
                self.open = Some(Open {
                    window,
                    gpu,
                    surface,
                    config,
                    layer,
                });
            }
            Err(why) => {
                self.error = Some(why);
                end_screen(event_loop);
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(m) = &mut self.open {
            let _ = m.layer.on_window_event(&m.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => {
                self.outcome = Some(NativeOutcome::Lobby(LobbyOutcome::Quit));
                end_screen(event_loop);
            }
            WindowEvent::Resized(size) => {
                if let Some(m) = &mut self.open {
                    m.config.width = size.width.max(1);
                    m.config.height = size.height.max(1);
                    m.surface.configure(&m.gpu.device, &m.config);
                }
            }
            WindowEvent::RedrawRequested if self.outcome.is_none() => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(m) = &self.open {
            m.window.request_redraw();
        }
    }
}

/// Opens `url` in the system's browser.
fn open_url(url: &str) {
    let spawned = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(why) = spawned {
        println!("lobby: could not open {url}: {why}");
    }
}

/// The lobby until it hands out a match, the player quits, or asks for
/// the web lobby. On a quit the panel uploads the settings and lets go of
/// the connection.
///
/// # Errors
/// No window could open, or its surface failed.
pub fn run(panel: &mut LobbyPanel, style: TwinStyle) -> Result<NativeOutcome, String> {
    let mut app = LobbyApp {
        panel,
        style,
        open: None,
        outcome: None,
        error: None,
    };
    let ran = run_on_event_loop(&mut app);
    if let Some(m) = &app.open {
        crate::window::remember_placement(&m.window);
    }
    app.open = None;
    let outcome = app.outcome.take();
    let error = app.error.take();
    ran?;
    if let Some(why) = error {
        return Err(why);
    }
    let outcome = outcome.unwrap_or(NativeOutcome::Lobby(LobbyOutcome::Quit));
    if matches!(
        outcome,
        NativeOutcome::Lobby(LobbyOutcome::Quit) | NativeOutcome::WebLobby
    ) {
        panel.leave();
    }
    Ok(outcome)
}

/// The lobby `seconds` after it connects to `lobby_url`, saved to `path`
/// without a window: for checking its look against the browser's. With
/// `quick_play` it presses Quick play once the lobby allows it, and returns
/// the match it is handed as soon as it is, saving that frame. With `room`
/// it draws that room's state (a `lobby:created` message's `lobby`) instead.
///
/// # Errors
/// No GPU, or the image could not be written.
pub fn screenshot(
    lobby_url: &str,
    key_file: Option<&Path>,
    path: &Path,
    size: (u32, u32),
    seconds: f64,
    quick_play: bool,
    room: Option<&Path>,
) -> Result<Option<LobbyChoice>, String> {
    let (width, height) = size;
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut layer = EguiLayer::offscreen(&gpu, OFFSCREEN_FORMAT);
    install_style(&layer.ctx);
    let mut panel = LobbyPanel::connect(lobby_url, key_file.map(Path::to_path_buf));
    let style = crate::match_loading_screen::twin_style();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width as f32, height as f32),
        )),
        ..Default::default()
    };
    let started = Instant::now();
    let mut pixels = Vec::new();
    let mut pressed = false;
    let room = match room {
        Some(file) => {
            let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            Some(serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let mut choice = None;
    while choice.is_none() && (started.elapsed() < Duration::from_secs_f64(seconds) || pixels.is_empty()) {
        panel.poll();
        // Without a lobby the dropped connection closes the room: open it again.
        if let Some(state) = &room
            && !panel.in_room()
        {
            panel.show_room_state(state);
        }
        if quick_play && !pressed && panel.quick_play_enabled() {
            pressed = true;
            panel.start_quick_play();
        }
        choice = panel.take_choice();
        let frame = layer.run_input(input(), |ui| {
            panel.ui(ui, &style);
        });
        pixels = render_encoded_to_rgba(&gpu, width, height, BACKGROUND, |encoder, view| {
            layer.paint(&gpu, encoder, view, [width, height], frame);
        });
        std::thread::sleep(Duration::from_millis(33));
    }
    panel.leave();
    image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    println!("lobby: saved {}", path.display());
    Ok(choice)
}
